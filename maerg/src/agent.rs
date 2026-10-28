//! External agent execution and response capture.
//!
//! The supervised task path performs the model turn on the host against a
//! numeric loopback model gateway (never inside the effect sandbox, which has
//! its own empty loopback). The turn advertises the closed authoring tool set;
//! any intent the model proposes is reduced by the broker in [`crate::authoring`]
//! to capability-bound effects that are applied exclusively inside the effect
//! sandbox. Any other command is refused, so no agent command ever runs
//! outside the effect sandbox.

mod live;
use live::{LiveRun, ObservedTransport};
mod controls;
use controls::RunControls;
mod redaction;
use redaction::StreamRedactor;
mod accounting;
mod context;
use accounting::TokenBudget;
#[cfg(test)]
mod regression_tests;

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use crate::{
    KvistError, Result,
    authoring::{
        self, AuthoringPlan, BrokerPolicy, CheckedIntent, DependencyRequest, DroppedIntent,
        ProposedDecision,
    },
    config::{AgentProfile, SandboxConfig, VcsSelection},
    sandbox,
    task_queue::Timestamp,
};

use sav::{
    CancellationToken, DirectModelTransport, Error as ModelError, LocalModelProvider, ModelRequest,
    ModelStreamEvent, ModelTransport, ModelTurn,
};
#[cfg(test)]
use sav::{ModelMessage, ToolChoice};

/// Structured execution result of an external agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunResult {
    /// True when the turn produced a usable result, no proposed intent was
    /// dropped, and every authorized effect applied.
    pub success: bool,
    /// Cumulative known prompt/input tokens; `None` if no turn reported usage.
    pub tokens_input: Option<usize>,
    /// Cumulative known output tokens; unavailable turns remain explicit in the log.
    pub tokens_output: Option<usize>,
    /// Path to the redacted execution log file.
    pub log_path: PathBuf,
    /// Bounded redacted model output retained as execution evidence.
    pub stdout: String,
    /// Bounded redacted failure diagnostics, empty on a clean turn.
    pub stderr: String,
    pub timed_out: bool,
    pub output_limit_exceeded: bool,
    /// True when the run paused for an impactful, uncovered decision the human
    /// must resolve; it is neither a success nor a failure.
    pub surfaced_decision: bool,
}

/// Inputs for one sandboxed agent task.
pub struct AgentExecutionRequest<'a> {
    pub project_root: &'a Path,
    pub vcs_selection: VcsSelection,
    pub prompt: &'a str,
    pub context_paths: &'a [PathBuf],
    pub read_only_mounts: &'a [sandbox::ReadOnlyMount],
    pub target_dir: &'a Path,
    pub task_id: &'a str,
    pub stream_output: bool,
    /// The agent role (architect or developer) for which the model was selected.
    pub role: super::config::Role,
    /// The authenticated execution-approval digest bound as the request policy
    /// identity.
    pub policy_identity: &'a str,
}

/// Splits and interpolates command arguments safely without spawning a shell.
pub fn split_command(
    template: &str,
    prompt: &str,
    context_paths: &[PathBuf],
    target_dir: &Path,
) -> Result<(String, Vec<String>)> {
    sav::render_command(template, prompt, context_paths, target_dir).map_err(Into::into)
}

/// The brokered and applied outcomes of one model turn within a run.
///
/// A run is a sequence of these; [`record_turn_outcome`](record_turn_outcome)
/// folds them into the durable log and trajectory so intermediate turns are
/// inspectable rather than lost.
struct RunTurn {
    /// The untrusted model turn.
    turn: ModelTurn,
    /// Authorized effects applied for this turn.
    effects: Vec<EffectRecord>,
    /// Brokered intents the model proposed that were not authorized.
    drops: Vec<DroppedIntent>,
    /// Decisions surfaced for the human this turn; non-empty means the run must
    /// pause in awaiting-decision and apply no effects from this turn.
    surfaced_decisions: Vec<ProposedDecision>,
    /// In-policy dependency requests recorded for acquisition this turn.
    accepted_requests: Vec<DependencyRequest>,
}

/// A resolved model identity plus the command template and system prompt for one selection.
struct ModelChoice {
    /// Pinned shared backend; absent only for explicit local command overrides.
    native: Option<sav::registry::NativeModel>,
    /// Resolved provider model identifier.
    model: String,
    /// Command template for the selected model.
    command: String,
    /// System prompt injected at the start of the message, when configured.
    system_prompt: Option<String>,
    /// True when the model performs no work (a no-op placeholder).
    is_none: bool,
}

/// Selects the model identity, command template, and system prompt for one invocation.
///
/// Shared by the curl command path and the host-side transport path so both honor
/// the same profile, role, and per-invocation override selection.
fn select_model_choice(
    profile: &AgentProfile,
    role: crate::config::Role,
    model_override: Option<&str>,
) -> Result<ModelChoice> {
    if let Some(resolved) = &profile.resolved_profile {
        let native = resolved.native().map_err(|_|KvistError::AgentCommandNotModelGateway {
            model_name:resolved.name.clone(),
            reason:"protected task authoring requires a native model profile; external agents do not implement its broker contract".into(),
        })?;
        if !resolved.profile.enabled
            || model_override
                .or(profile.model.as_deref())
                .is_some_and(|selected| selected != resolved.name && selected != native.model)
        {
            return Err(KvistError::InvalidModelSelection {
                model_name: model_override
                    .or(profile.model.as_deref())
                    .unwrap_or(&resolved.name)
                    .into(),
                role,
                available: resolved.name.clone(),
            });
        }
        return Ok(ModelChoice {
            native: Some(native.clone()),
            model: native.model.clone(),
            command: String::new(),
            system_prompt: profile
                .models
                .first()
                .and_then(|model| model.system_prompt.clone()),
            is_none: false,
        });
    }
    let model_name = model_override
        .or(profile.model.as_deref())
        .unwrap_or(&profile.default_model);
    let choice = if matches!(model_name, "default" | "default-model") {
        if let Some(first) = profile.models.first() {
            ModelChoice {
                native: None,
                model: first.name.clone(),
                command: first.command.clone(),
                system_prompt: first.system_prompt.clone(),
                is_none: first.name == "none",
            }
        } else if !profile.command_template.is_empty() {
            ModelChoice {
                native: None,
                model: model_name.to_owned(),
                command: profile.command_template.clone(),
                system_prompt: None,
                is_none: false,
            }
        } else {
            return Err(KvistError::InvalidModelSelection {
                model_name: model_name.to_owned(),
                role,
                available: profile
                    .models
                    .iter()
                    .map(|m| m.name.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            });
        }
    } else if let Some(found) = profile.models.iter().find(|model| model.name == model_name) {
        ModelChoice {
            native: None,
            model: found.name.clone(),
            command: found.command.clone(),
            system_prompt: found.system_prompt.clone(),
            is_none: found.name == "none",
        }
    } else if !profile.command_template.is_empty()
        && (model_name == profile.profile || model_name == role.as_str())
    {
        ModelChoice {
            native: None,
            model: model_name.to_owned(),
            command: profile.command_template.clone(),
            system_prompt: None,
            is_none: false,
        }
    } else {
        return Err(KvistError::InvalidModelSelection {
            model_name: model_name.to_owned(),
            role,
            available: profile
                .models
                .iter()
                .map(|m| m.name.clone())
                .collect::<Vec<_>>()
                .join(", "),
        });
    };
    Ok(choice)
}

/// Extracts a numeric loopback endpoint (scheme + host + port) from a command template.
///
/// The template embeds the provider URL literally; only the first `http(s)://`
/// authority is considered. The transport validates that the result is numeric
/// loopback and rejects anything else, so an unparseable or remote URL fails
/// closed rather than falling back to a broad connection.
fn extract_loopback_endpoint(command: &str) -> Result<String> {
    let (i, proto, scheme_len) = if let Some(i) = command.find("https://") {
        (i, "https://", 8usize)
    } else if let Some(i) = command.find("http://") {
        (i, "http://", 7usize)
    } else {
        return Err(KvistError::InvalidModelSelection {
            model_name: "endpoint".to_owned(),
            role: crate::config::Role::Developer,
            available: "a command template containing an http(s):// loopback endpoint".to_owned(),
        });
    };
    let after_scheme = &command[i + scheme_len..];
    let end = after_scheme
        .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        .unwrap_or(after_scheme.len());
    let authority = after_scheme[..end].split('/').next().unwrap_or("");
    if authority.is_empty() {
        return Err(KvistError::InvalidModelSelection {
            model_name: "endpoint".to_owned(),
            role: crate::config::Role::Developer,
            available: "a non-empty loopback host:port in the command template".to_owned(),
        });
    }
    Ok(format!("{proto}{authority}"))
}

/// Gets the effective command for a given agent profile and model selection.
pub fn get_effective_command(
    profile: &AgentProfile,
    role: crate::config::Role,
    prompt: &str,
    context_paths: &[PathBuf],
    target_dir: &Path,
) -> Result<(String, Vec<String>)> {
    get_effective_command_with_options(profile, role, None, None, prompt, context_paths, target_dir)
}

/// Gets the effective command with explicit per-invocation model and effort selection.
pub fn get_effective_command_with_options(
    profile: &AgentProfile,
    role: crate::config::Role,
    model_override: Option<&str>,
    reasoning_effort: Option<sav::ReasoningEffort>,
    prompt: &str,
    context_paths: &[PathBuf],
    target_dir: &Path,
) -> Result<(String, Vec<String>)> {
    let ModelChoice {
        command: selected_command,
        system_prompt,
        is_none,
        ..
    } = select_model_choice(profile, role, model_override)?;

    if is_none {
        return split_raw_command(&selected_command);
    }

    let prompt = match system_prompt {
        Some(sys) if !sys.is_empty() => format!("{sys}\n\n{prompt}"),
        _ => prompt.to_owned(),
    };

    let effort = match reasoning_effort {
        Some(explicit) => Some(explicit),
        None => {
            if selected_command.contains("{reasoning_effort}") {
                profile.thinking_effort
            } else {
                None
            }
        }
    };

    sav::render_command_with_reasoning_effort(
        &selected_command,
        &prompt,
        context_paths,
        target_dir,
        effort,
    )
    .map_err(Into::into)
}

fn split_raw_command(template: &str) -> Result<(String, Vec<String>)> {
    sav::split_raw_command(template).map_err(Into::into)
}

/// Maps a command template to the provider wire protocol it targets.
fn provider_from_command(command: &str) -> LocalModelProvider {
    if command.contains("/api/chat") || command.contains("/generate") {
        LocalModelProvider::Ollama
    } else {
        LocalModelProvider::LlamaServer
    }
}

/// Recovers the provider-facing model identifier embedded in a command template.
///
/// The structured transport path builds its own request, so unlike the rendered
/// command it starts with no model identifier. For a llama-server/ollama command
/// the identifier is embedded in the request body the command would send (for
/// example the `model` of a curl `--json` payload), so this reads it out. The
/// command may be shell-quoted (with `\"`), so quote and backslash runs around
/// the key and value are tolerated and the value is unescaped. Returns `None`
/// when no identifier is present, letting the caller fall back to the selector.
fn extract_provider_model(command: &str) -> Option<String> {
    let bytes = command.as_bytes();
    let mut i = 0;
    while i + 5 <= bytes.len() {
        if &bytes[i..i + 5] != b"model" {
            i += 1;
            continue;
        }
        // Reject a `model` that is the tail of a longer token (for example the
        // `model` inside `my_model`) so it is not mistaken for a key. Any other
        // preceding byte (a `{`, quote, whitespace, comma, …) is accepted.
        let before_ok = i == 0
            || !matches!(
                bytes[i - 1],
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'
            );
        // Skip any optional escaped quote / whitespace after the key to the colon.
        let mut j = i + 5;
        while j < bytes.len() && matches!(bytes[j], b'"' | b'\\' | b' ' | b'\t') {
            j += 1;
        }
        let colon = bytes.get(j) == Some(&b':');
        if !before_ok || !colon {
            i += 1;
            continue;
        }
        // Skip the colon and any leading escaped quote / whitespace, then read
        // the value up to the next quote.
        j += 1;
        while j < bytes.len() && matches!(bytes[j], b'"' | b'\\' | b' ' | b'\t') {
            j += 1;
        }
        let start = j;
        while j < bytes.len() && bytes[j] != b'"' {
            j += 1;
        }
        if j < bytes.len() {
            // Drop the value's quote/backslash escaping (model identifiers carry
            // no quotes or backslashes of their own).
            return Some(command[start..j].replace(['\\', '"'], ""));
        }
        i += 1;
    }
    None
}

/// Brief connect attempts made while proving the gateway is accepting traffic.
/// A down gateway fails on the first attempt; these ride out a gateway that is
/// still starting its listeners. Every attempt and pause draws from the turn's
/// shared wall-clock budget.
const GATEWAY_PROBE_ATTEMPTS: u32 = 3;

/// Pause between gateway liveness-probe connect attempts.
const GATEWAY_PROBE_BACKOFF: Duration = Duration::from_millis(200);

/// Maximum model-turn attempts when the gateway passes the liveness probe but a
/// turn still hits a transient availability error (for example the gateway had
/// not finished spawning the model onto its ephemeral port). This counts the
/// first attempt plus `MODEL_TURN_MAX_ATTEMPTS - 1` retries.
const MODEL_TURN_MAX_ATTEMPTS: u32 = 3;

/// Pause between model-turn retries. Short and fixed keeps agent runs
/// deterministic while still letting a cold-starting gateway recover.
const MODEL_TURN_RETRY_BACKOFF: Duration = Duration::from_millis(250);

/// In-sandbox mount destination of one staged authoring intent.
const STAGED_INTENT_MOUNT_PATH: &str = "/workspace/authoring/intent.json";

/// The shared wall-clock budget for the model phase of one task run.
///
/// The liveness probe, every turn attempt, and every retry backoff all draw
/// from this single budget, so the model phase never exceeds the configured
/// timeout by more than scheduling slack. The per-attempt transport deadline
/// is whatever remains when the attempt starts.
struct TurnBudget {
    started: Instant,
    total: Duration,
}

impl TurnBudget {
    fn new(total: Duration) -> Self {
        Self {
            started: Instant::now(),
            total,
        }
    }

    /// The remaining budget, clamped at zero.
    fn remaining(&self) -> Duration {
        self.total.saturating_sub(self.started.elapsed())
    }

    fn model_deadline(&self, cancellation: &CancellationToken) -> Result<Duration> {
        if cancellation.is_cancelled() {
            return Err(KvistError::Sav(ModelError::ModelTransportCancelled));
        }
        let remaining = self.remaining();
        if remaining.is_zero() {
            return Err(KvistError::Sav(ModelError::ModelTransportTimedOut));
        }
        Ok(remaining.min(Duration::from_secs(86_400)))
    }
}

/// Proves a loopback model gateway is accepting connections before a turn runs,
/// within the shared wall-clock budget.
///
/// `endpoint` is a numeric loopback URL such as `http://127.0.0.1:9931`. This
/// performs only a TCP connect; it never issues an HTTP request, so it cannot
/// load, select, or shift a model slot. It is a pure liveness check that lets a
/// down gateway fail fast with an actionable message instead of a cryptic
/// transport error buried deep inside a turn.
fn probe_gateway_reachable(
    endpoint: &str,
    budget: &TurnBudget,
    cancellation: &CancellationToken,
) -> Result<()> {
    let authority = endpoint
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let addr: std::net::SocketAddr =
        authority
            .parse()
            .map_err(|source| KvistError::LocalModelGatewayUnreachable {
                endpoint: endpoint.to_owned(),
                reason: format!("endpoint `{endpoint}` is not a host:port address: {source}"),
            })?;
    if !addr.ip().is_loopback() {
        return Err(KvistError::Sav(ModelError::InvalidModelTransport {
            reason: "liveness probing requires a numeric loopback endpoint".into(),
        }));
    }

    let mut attempt = 1u32;
    loop {
        if cancellation.is_cancelled() {
            return Err(KvistError::Sav(ModelError::ModelTransportCancelled));
        }
        let remaining = budget.remaining();
        if remaining.is_zero() {
            return Err(KvistError::Sav(ModelError::ModelTransportTimedOut));
        }
        let connect_timeout = Duration::from_millis(100).min(remaining);
        match std::net::TcpStream::connect_timeout(&addr, connect_timeout) {
            Ok(_) => return Ok(()),
            Err(source) if attempt < GATEWAY_PROBE_ATTEMPTS => {
                tracing::warn!(
                    attempt,
                    %source,
                    endpoint,
                    "local model gateway not yet accepting connections; retrying liveness probe"
                );
                let until = Instant::now() + GATEWAY_PROBE_BACKOFF.min(budget.remaining());
                while Instant::now() < until && !cancellation.is_cancelled() {
                    thread::sleep(
                        Duration::from_millis(10)
                            .min(until.saturating_duration_since(Instant::now())),
                    );
                }
                attempt += 1;
            }
            Err(source) => {
                return Err(KvistError::LocalModelGatewayUnreachable {
                    endpoint: endpoint.to_owned(),
                    reason: source.to_string(),
                });
            }
        }
    }
}

/// Whether a transport failure is worth retrying: only transient gateway
/// availability problems (nothing is listening yet, the port is still starting
/// up, or the model slot has not been allocated yet). Response-level failures
/// (bad HTTP status, malformed body, oversized response, cancellation) are
/// never retried, since they indicate a real answer rather than an unavailable
/// gateway.
fn is_retryable_transport_error(error: &ModelError) -> bool {
    match error {
        // The slot-allocation timeout fires when the gateway accepted the
        // connection but has not finished loading or spawning the model; the
        // request has not started generating, so retrying is safe.
        ModelError::ModelTransportTimedOut | ModelError::SlotAllocationTimedOut { .. } => true,
        ModelError::ModelTransportIo { source, .. } => {
            // A refused, timed-out, interrupted, or peer-reset socket is a
            // gateway that is not ready to serve yet (nothing listening, still
            // loading a model, or mid-cold-start); it is safe to retry. A peer
            // reset is included for this reason, because it commonly
            // accompanies a gateway that accepted the connection before it was
            // fully up rather than a real rejection of the payload.
            matches!(
                source.kind(),
                io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::TimedOut
                    | io::ErrorKind::Interrupted
                    | io::ErrorKind::ConnectionReset
            )
        }
        _ => false,
    }
}

/// Runs one streaming attempt, relaying redacted text deltas to standard
/// output as they arrive, and reports whether any text was emitted.
///
/// Emitted text cannot be retracted, so a streamed attempt that fails after
/// emitting must not be retried; the assembled turn is returned so the caller
/// can record it in the log and evidence either way.
fn run_stream_attempt<T: ModelTransport>(
    transport: &T,
    request: &ModelRequest,
    cancellation: &CancellationToken,
    redactions: &[String],
) -> (sav::Result<ModelTurn>, bool) {
    let mut emitted = false;
    let mut redactor = StreamRedactor::new(redactions);
    let mut relay = |event: ModelStreamEvent| -> sav::Result<()> {
        if let ModelStreamEvent::TextDelta(delta) = event {
            emitted = true;
            StreamRedactor::emit_to_stdout(&redactor.absorb(&delta));
        }
        Ok(())
    };
    let turn = transport.stream(request, cancellation, &mut relay);
    StreamRedactor::emit_to_stdout(&redactor.finish());
    (turn, emitted)
}

/// Performs one model turn, retrying transient gateway-availability failures
/// under the shared wall-clock budget.
///
/// A turn that already streamed text to standard output is never retried,
/// because streamed bytes cannot be retracted. Real response failures
/// (non-success status, malformed or oversized body, cancellation) are
/// returned immediately so they are never masked by a retry.
#[cfg(test)]
fn complete_turn<T: ModelTransport>(
    make_transport: impl Fn() -> Result<T>,
    request: &ModelRequest,
    cancellation: &CancellationToken,
    budget: &TurnBudget,
    stream_output: bool,
    redactions: &[String],
    task_id: &str,
) -> Result<ModelTurn> {
    complete_turn_counted(
        make_transport,
        request,
        cancellation,
        budget,
        stream_output,
        redactions,
        task_id,
        &mut 0,
        &sav::runtime::RetryPolicy::new(
            MODEL_TURN_MAX_ATTEMPTS,
            MODEL_TURN_RETRY_BACKOFF,
            MODEL_TURN_RETRY_BACKOFF,
        ),
        &mut TokenBudget::new(None),
    )
}

#[allow(clippy::too_many_arguments)]
fn complete_turn_counted<T: ModelTransport>(
    make_transport: impl Fn() -> Result<T>,
    request: &ModelRequest,
    cancellation: &CancellationToken,
    budget: &TurnBudget,
    stream_output: bool,
    redactions: &[String],
    task_id: &str,
    attempts: &mut u32,
    retry: &sav::runtime::RetryPolicy,
    tokens: &mut TokenBudget,
) -> Result<ModelTurn> {
    loop {
        if cancellation.is_cancelled() {
            return Err(KvistError::Sav(ModelError::ModelTransportCancelled));
        }
        if *attempts >= retry.max_attempts {
            return Err(sav::runtime::Error::RunBudget {
                reason:
                    "model generation attempt limit exhausted, including steering interruptions"
                        .into(),
            }
            .into());
        }
        if budget.remaining().is_zero() {
            return Err(KvistError::Sav(ModelError::ModelTransportTimedOut));
        }
        *attempts += 1;
        let transport = make_transport()?;
        let mut request = request.clone();
        let reservation = tokens.reserve(&mut request)?;
        let (outcome, emitted) = if stream_output {
            run_stream_attempt(&transport, &request, cancellation, redactions)
        } else {
            (transport.complete(&request, cancellation), false)
        };
        match outcome {
            Ok(turn) => {
                tokens.finish(reservation, turn.usage);
                return Ok(turn);
            }
            Err(source)
                if is_retryable_transport_error(&source)
                    && *attempts < retry.max_attempts
                    && !emitted =>
            {
                tracing::warn!(
                    task_id,
                    attempt = *attempts,
                    %source,
                    "local model gateway unavailable during turn; retrying"
                );
                let deadline =
                    Instant::now() + retry.backoff_delay(*attempts + 1).min(budget.remaining());
                while Instant::now() < deadline && !cancellation.is_cancelled() {
                    thread::sleep(
                        Duration::from_millis(10)
                            .min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
            }
            Err(source) => return Err(KvistError::Sav(source)),
        }
    }
}

/// Performs one model turn on the host, outside the effect sandbox, within the
/// shared wall-clock budget.
///
/// The turn advertises the closed authoring tool set, so the model may propose
/// file effects alongside text. The gateway is liveness-probed first and
/// transient availability failures are retried, so a down or cold-starting
/// gateway yields a fast, actionable failure rather than a cryptic transport
/// error. A command that does not target a numeric loopback gateway is refused
/// before any transport work: no agent command ever runs on the host outside
/// the effect sandbox. The returned turn is untrusted; its tool intents are
/// brokered by [`crate::authoring`] before anything is applied.
///
/// The full conversation history is supplied by the caller so each turn feeds
/// its assistant message and tool results back to the model; the cancellation
/// token spans the whole run rather than a single turn.
#[allow(clippy::too_many_arguments)]
fn execute_host_turn(
    choice: &ModelChoice,
    profile: &AgentProfile,
    request: &AgentExecutionRequest<'_>,
    budget: &TurnBudget,
    redactions: &[String],
    model_request: &ModelRequest,
    cancellation: &CancellationToken,
    live: &LiveRun,
    attempts: &mut u32,
    tokens: &mut TokenBudget,
) -> Result<ModelTurn> {
    if choice.is_none {
        return Err(KvistError::InvalidModelSelection {
            model_name: choice.model.clone(),
            role: profile.role,
            available: "a model that performs work".to_owned(),
        });
    }
    let native = choice.native.as_ref();
    let (provider, endpoint, provider_model) = if let Some(native) = native {
        (
            native.provider,
            native.base_url.clone(),
            native.model.clone(),
        )
    } else {
        let endpoint = extract_loopback_endpoint(&choice.command).map_err(|source| {
            KvistError::AgentCommandNotModelGateway {
                model_name: choice.model.clone(),
                reason: source.to_string(),
            }
        })?;
        (
            provider_from_command(&choice.command),
            endpoint,
            extract_provider_model(&choice.command).unwrap_or_else(|| choice.model.clone()),
        )
    };

    let mut model_request = model_request.clone();
    model_request.model = provider_model;
    tokens.preflight(&model_request)?;

    // Liveness pre-check: fail fast with an actionable message when the local
    // model gateway is down, instead of surfacing a cryptic transport error
    // inside a turn. A TCP connect only confirms the port; it never loads or
    // shifts a model slot.
    // Construction validates native endpoint authority without network I/O.
    // This must precede even a TCP liveness probe.
    let deadline = budget.model_deadline(cancellation)?;
    if let Some(native) = native {
        native.transport_bounded(deadline, profile.max_output_bytes)?;
    } else {
        DirectModelTransport::new(provider, &endpoint, deadline, profile.max_output_bytes)?;
    }
    probe_gateway_reachable(&endpoint, budget, cancellation)?;

    // llama-server/ollama gateways route on the provider-facing model identifier,
    // which the command embeds; the selector alone is not a model the gateway
    // recognizes (it returns 400). Fall back to the selector when the command
    // carries no such identifier.
    complete_turn_counted(
        || {
            let deadline = budget.model_deadline(cancellation)?;
            let transport = if let Some(native) = native {
                native.transport_bounded(deadline, profile.max_output_bytes)?
            } else {
                DirectModelTransport::new(provider, &endpoint, deadline, profile.max_output_bytes)?
            };
            Ok(ObservedTransport {
                transport,
                live,
                redactions,
            })
        },
        &model_request,
        cancellation,
        budget,
        request.stream_output,
        redactions,
        request.task_id,
        attempts,
        &native.map_or_else(
            || {
                sav::runtime::RetryPolicy::new(
                    MODEL_TURN_MAX_ATTEMPTS,
                    MODEL_TURN_RETRY_BACKOFF,
                    MODEL_TURN_RETRY_BACKOFF,
                )
            },
            |model| {
                sav::runtime::RetryPolicy::new(
                    model.max_attempts.min(MODEL_TURN_MAX_ATTEMPTS),
                    Duration::from_secs(model.retry_base_delay_secs),
                    Duration::from_secs(model.retry_max_delay_secs),
                )
            },
        ),
        tokens,
    )
}

/// The approved, capability-confirmed sandbox context for effect application.
struct EffectSandbox<'a> {
    config: &'a SandboxConfig,
    expected_runner: &'a crate::sandbox::RunnerIdentity,
    probe: &'a crate::sandbox::SandboxProbe,
    steering: &'a RunControls<'a>,
    budget: &'a TurnBudget,
}

/// Bounded outcome of one authorized effect dispatch, for evidence.
#[derive(Clone)]
struct EffectRecord {
    /// Turn-local call identity echoed from the model intent.
    call_id: String,
    /// Tool name.
    tool: String,
    /// Component-relative destination.
    destination: String,
    /// True when the sandboxed applier confirmed the write.
    applied: bool,
    /// Dispatch may have mutated state, but completion was not confirmed.
    uncertain: bool,
    /// Bounded diagnostics when the effect did not apply.
    detail: String,
    /// True when the effect dispatch hit its wall-clock limit.
    timed_out: bool,
    /// True when the effect dispatch exceeded its output bound.
    output_limit_exceeded: bool,
}

/// Dispatches one effect: the kvist binary itself runs inside the effect
/// sandbox against a read-only staged-intent mount. The host never writes
/// component state for an effect; only the sandboxed applier touches the
/// filesystem. Sandbox infrastructure failures propagate to the caller.
fn dispatch_effect(
    profile: &AgentProfile,
    sandbox: &EffectSandbox<'_>,
    request: &AgentExecutionRequest<'_>,
    program: &str,
    arguments: &[String],
    staged_path: &Path,
) -> Result<crate::sandbox::ExecutionResult> {
    let mut mounts: Vec<crate::sandbox::ReadOnlyMount> = request.read_only_mounts.to_vec();
    mounts.push(crate::sandbox::ReadOnlyMount::file(
        staged_path.to_path_buf(),
        STAGED_INTENT_MOUNT_PATH.to_owned(),
    ));
    let execution_request = crate::sandbox::ExecutionRequest {
        project_root: request.project_root,
        vcs_selection: request.vcs_selection,
        component_dir: request.target_dir,
        phase: crate::sandbox::ExecutionPhase::Authoring,
        scratch_host_dir: None,
        program,
        arguments,
        environment: crate::sandbox::allowed_environment(sandbox.config, None),
        read_only_mounts: &mounts,
        backend: &sandbox.probe.backend,
        policy_identity: request.policy_identity,
    };
    crate::sandbox::execute_with_cancellation(
        sandbox.config,
        execution_request,
        crate::sandbox::ExecutionOptions {
            timeout: Some(sandbox.budget.remaining().min(Duration::from_secs(86_400))),
            output_limit: Some(profile.max_output_bytes),
            live_stdout: None,
        },
        sandbox.expected_runner,
        sandbox.steering.cancellation,
    )
}

/// Stages and applies every authorized effect, one sandbox request per effect.
///
/// The host stages each effect under the component state directory, dispatches
/// the in-sandbox `authoring-apply` invocation, and removes the staged file
/// once it is consumed. A sandbox infrastructure failure is fatal and
/// propagates after removing the staged file; a dispatch that ran but failed
/// is recorded as an unapplied effect so the turn fails closed rather than
/// pretending the model's request was honored.
#[allow(clippy::too_many_arguments)]
fn apply_authoring_effects(
    profile: &AgentProfile,
    sandbox: &EffectSandbox<'_>,
    request: &AgentExecutionRequest<'_>,
    plan: &AuthoringPlan,
    turn: &ModelTurn,
    session_id: &str,
    live: &LiveRun,
    records: &mut Vec<EffectRecord>,
) -> Result<()> {
    let kvist_binary =
        std::env::current_exe().map_err(|source| KvistError::SandboxUnavailable {
            runner: sandbox.config.runner.clone(),
            reason: format!(
                "cannot resolve the kvist binary for in-sandbox effect application: {source}"
            ),
        })?;
    let program = kvist_binary.to_string_lossy().into_owned();
    let arguments = [
        "authoring-apply".to_owned(),
        "--component".to_owned(),
        "/workspace/component".to_owned(),
        "--intent-file".to_owned(),
        STAGED_INTENT_MOUNT_PATH.to_owned(),
    ];

    apply_authoring_effects_with(
        request,
        plan,
        turn,
        session_id,
        live,
        sandbox.steering,
        records,
        |staged_path| dispatch_effect(profile, sandbox, request, &program, &arguments, staged_path),
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_authoring_effects_with(
    request: &AgentExecutionRequest<'_>,
    plan: &AuthoringPlan,
    turn: &ModelTurn,
    session_id: &str,
    live: &LiveRun,
    steering: &RunControls<'_>,
    records: &mut Vec<EffectRecord>,
    mut dispatch: impl FnMut(&Path) -> Result<sandbox::ExecutionResult>,
) -> Result<()> {
    for effect in &plan.effects {
        if steering.cancellation.is_cancelled() {
            return Err(KvistError::Sav(ModelError::Cancelled));
        }
        if !steering.mailbox.dispatch_ready()? {
            break;
        }
        let raw = turn
            .tool_intents
            .iter()
            .find(|intent| intent.id == effect.call_id)
            .ok_or_else(|| KvistError::AuthoringEffectFailed {
                call_id: effect.call_id.clone(),
                reason: "the authorized effect lost its originating model intent".to_owned(),
            })?;
        let staged_path =
            authoring::apply::stage_intent(request.target_dir, session_id, effect, raw)?;
        let _staged = StagedIntentCleanup(staged_path.clone());
        live.record(
            &serde_json::json!({"type":"tool_dispatch","data":{
                "call_id":effect.call_id,"name":effect.tool
            }}),
            true,
        )?;
        if steering.cancellation.is_cancelled() {
            return Err(KvistError::Sav(ModelError::Cancelled));
        }
        if !steering.mailbox.dispatch_ready()? {
            break;
        }
        records.push(EffectRecord {
            call_id: effect.call_id.clone(),
            tool: effect.tool.clone(),
            destination: effect.destination.clone(),
            applied: false,
            uncertain: true,
            detail: "dispatch completion unknown; state may have mutated; do not replay".into(),
            timed_out: false,
            output_limit_exceeded: false,
        });
        let result = dispatch(&staged_path)?;
        let cancelled = result.cancelled;
        let record = summarize_effect(effect, result);
        if let Some(retained) = records.last_mut() {
            *retained = record.clone();
        }
        live.record(
            &serde_json::json!({"type":"tool_result","data":{
                "call_id":record.call_id,"name":record.tool,"applied":record.applied,
                "uncertain":record.uncertain,
                "timed_out":record.timed_out,"output_limit_exceeded":record.output_limit_exceeded
            }}),
            true,
        )?;
        if cancelled {
            return Err(KvistError::Sav(sav::Error::Cancelled));
        }
    }
    Ok(())
}

struct StagedIntentCleanup(PathBuf);
impl Drop for StagedIntentCleanup {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(%error,path=%self.0.display(),"could not remove staged authoring intent");
        }
    }
}

/// Reduces one effect dispatch result to a bounded evidence record.
fn summarize_effect(
    effect: &CheckedIntent,
    result: crate::sandbox::ExecutionResult,
) -> EffectRecord {
    let output = &result.output;
    let applied = !result.timed_out
        && !result.output_limit_exceeded
        && !result.cancelled
        && output.status.success();
    let detail = if applied {
        String::new()
    } else {
        let mut text = format!(
            "effect for `{}` -> {} exited with {:?}",
            effect.tool, effect.destination, output.status
        );
        if result.cancelled {
            text.push_str(" after cancellation");
        } else if result.timed_out {
            text.push_str(" after its wall-clock limit");
        } else if result.output_limit_exceeded {
            text.push_str(" after exceeding its output bound");
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.trim().is_empty() {
            text.push_str("\nstdout:\n");
            text.push_str(&stdout);
        }
        if !stderr.trim().is_empty() {
            text.push_str("\nstderr:\n");
            text.push_str(&stderr);
        }
        text.push_str("\nstate may have mutated; completion unconfirmed; do not replay");
        truncate_utf8(&mut text, 4096);
        text
    };
    EffectRecord {
        call_id: effect.call_id.clone(),
        tool: effect.tool.clone(),
        destination: effect.destination.clone(),
        applied,
        uncertain: !applied,
        detail,
        timed_out: result.timed_out,
        output_limit_exceeded: result.output_limit_exceeded,
    }
}

/// The bounded reason a model turn or its effects produced no usable result.
struct TurnFailure {
    /// Bounded, redactable failure diagnostic.
    reason: String,
    /// True when the model phase or an effect hit a wall-clock deadline.
    timed_out: bool,
    /// True when a response or effect exceeded its output bound.
    output_limit_exceeded: bool,
    /// True when the run was cancelled (SIGINT/SIGTERM) mid-effect.
    cancelled: bool,
}

impl TurnFailure {
    fn from_kvist_error(error: &KvistError) -> Self {
        let (timed_out, output_limit_exceeded, cancelled) = match error {
            KvistError::Sav(model_error) => match model_error {
                sav::Error::ModelTransportTimedOut
                | sav::Error::SlotAllocationTimedOut { .. }
                | sav::Error::TtftTimedOut { .. }
                | sav::Error::InterTokenCadenceTimedOut { .. } => (true, false, false),
                sav::Error::ModelResponseLimitExceeded { .. } => (false, true, false),
                sav::Error::ModelTransportCancelled | sav::Error::Cancelled => (false, false, true),
                _ => (false, false, false),
            },
            _ => (false, false, false),
        };
        Self {
            reason: error.to_string(),
            timed_out,
            output_limit_exceeded,
            cancelled,
        }
    }
}

/// Executes a supervised task's model turn on the host and applies every
/// broker-authorized authoring effect inside the effect sandbox.
///
/// This is the brokered execution loop (ADR-0009): the model turn runs on the
/// host against a numeric loopback gateway and advertises the closed authoring
/// tool set; untrusted intents are reduced by [`crate::authoring`] into
/// capability-bound effects, and each effect is applied by the kvist binary
/// itself inside the effect sandbox. Dropped intents and unapplied effects
/// fail the turn (fail-closed); only a turn with no dropped intents and every
/// effect applied succeeds. A model command that does not target a loopback
/// gateway is refused: it is never executed on the host.
/// Builds the failure reason for a turn whose proposed intents were not
/// authorized and were therefore not applied.
fn drop_reason(dropped: &[DroppedIntent]) -> String {
    let mut reason = format!(
        "{} proposed intent(s) were not authorized and were not applied:\n",
        dropped.len()
    );
    for intent in dropped {
        reason.push_str(&format!(
            "- call {} (tool `{}`): {}\n",
            intent.call_id, intent.tool, intent.reason
        ));
    }
    reason.trim_end().to_owned()
}

/// Builds the failure reason for a turn whose authorized effects did not all
/// apply; partial application would misrepresent what the model requested.
fn apply_reason(effects: &[EffectRecord]) -> String {
    let mut reason = String::new();
    for record in effects.iter().filter(|record| !record.applied) {
        reason.push_str(&format!(
            "- effect call {} (tool `{}` -> {}): {}\n",
            record.call_id, record.tool, record.destination, record.detail
        ));
    }
    reason.trim_end().to_owned()
}

pub fn execute_agent(
    profile: &AgentProfile,
    sandbox_config: &SandboxConfig,
    expected_runner: Option<&crate::sandbox::RunnerIdentity>,
    probe: Option<&crate::sandbox::SandboxProbe>,
    request: AgentExecutionRequest<'_>,
) -> Result<AgentRunResult> {
    execute_agent_with_controls(
        profile,
        sandbox_config,
        expected_runner,
        probe,
        request,
        false,
    )
}

/// Runs protected authoring with an optional separately authorized private control socket.
pub fn execute_agent_with_controls(
    profile: &AgentProfile,
    sandbox_config: &SandboxConfig,
    expected_runner: Option<&crate::sandbox::RunnerIdentity>,
    probe: Option<&crate::sandbox::SandboxProbe>,
    request: AgentExecutionRequest<'_>,
    allow_control: bool,
) -> Result<AgentRunResult> {
    execute_agent_inner(
        profile,
        sandbox_config,
        expected_runner,
        probe,
        request,
        &sav::runtime::ControlMailbox::default(),
        &CancellationToken::new(),
        allow_control,
    )
}

/// Runs protected authoring with caller-owned shared controls, without opening IPC.
pub fn execute_agent_controlled(
    profile: &AgentProfile,
    sandbox_config: &SandboxConfig,
    expected_runner: Option<&crate::sandbox::RunnerIdentity>,
    probe: Option<&crate::sandbox::SandboxProbe>,
    request: AgentExecutionRequest<'_>,
    controls: &sav::runtime::ControlMailbox,
    cancellation: &CancellationToken,
) -> Result<AgentRunResult> {
    execute_agent_inner(
        profile,
        sandbox_config,
        expected_runner,
        probe,
        request,
        controls,
        cancellation,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_agent_inner(
    profile: &AgentProfile,
    sandbox_config: &SandboxConfig,
    expected_runner: Option<&crate::sandbox::RunnerIdentity>,
    probe: Option<&crate::sandbox::SandboxProbe>,
    request: AgentExecutionRequest<'_>,
    controls: &sav::runtime::ControlMailbox,
    cancellation: &CancellationToken,
    allow_control: bool,
) -> Result<AgentRunResult> {
    let mut choice = select_model_choice(profile, request.role, None)?;

    tracing::info!(
        task_id = %request.task_id,
        role = ?request.role,
        model = %choice.model,
        "executing agent model turn on host (effect sandbox reserved)"
    );

    let logs_dir = ensure_logs_directory(request.target_dir)?;

    let timestamp = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let session_id = format!(
        "{}_{}",
        request.task_id,
        timestamp.to_string().replace(":", "-")
    );
    let log_path = logs_dir.join(format!("{session_id}.log"));

    tracing::debug!(
        task_id = %request.task_id,
        model = %choice.model,
        log_path = %log_path.display(),
        timeout_seconds = profile.timeout_seconds,
        "prepared host-side agent model turn"
    );
    let mut log_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&log_path)
        .map_err(|source| KvistError::Io {
            operation: "create agent log file",
            path: log_path.clone(),
            source,
        })?;

    let runs_dir = request.target_dir.join(".kvist").join("runs");
    ensure_real_directory(&runs_dir, "create live run directory")?;
    let live = LiveRun::create(
        &runs_dir.join(format!("{session_id}.events.jsonl")),
        &session_id,
        request.task_id,
        &choice.model,
        &profile.redaction_values,
    )?;
    if let Some(resolved) = &profile.resolved_profile {
        live.record(
            &serde_json::json!({"type":"profile_selected","data":{
                "profile":resolved.name,"source":resolved.source,"digest":resolved.digest,
                "kind":"native","model":choice.model
            }}),
            true,
        )?;
    }
    let redactions = redaction_values(profile, sandbox_config);
    controls.begin_run()?;
    let steering = RunControls {
        mailbox: controls,
        cancellation,
        live: &live,
        redactions: &redactions,
    };
    let mut run_turns: Vec<RunTurn> = Vec::new();
    let mut tokens = TokenBudget::new(profile.token_limit);
    let outcome: Result<Option<TurnFailure>> = (|| {
        let _server = if allow_control {
            let server = sav::observation::bind_runtime_controls(controls, cancellation)?;
            eprintln!(
                "private run control: {}",
                server.path().display().to_string().escape_debug()
            );
            Some(server)
        } else {
            None
        };
        let budget = TurnBudget::new(Duration::from_secs(profile.timeout_seconds));
        let mut context = if let Some(native) = &mut choice.native {
            let limit = match native.context_limit {
                Some(limit) => limit,
                None => {
                    let retry = sav::runtime::RetryPolicy::new(
                        MODEL_TURN_MAX_ATTEMPTS,
                        MODEL_TURN_RETRY_BACKOFF,
                        MODEL_TURN_RETRY_BACKOFF,
                    );
                    let mut attempt = 0u32;
                    loop {
                        attempt += 1;
                        let transport = native.transport_bounded(
                            budget.model_deadline(cancellation)?,
                            profile.max_output_bytes,
                        )?;
                        match transport.context_limit(&native.model, cancellation) {
                            Ok(Some(limit)) => break limit,
                            Ok(None) => {
                                return Err(KvistError::Runtime(sav::runtime::Error::Config {
                                    path: None,
                                    reason: "shared native serving capacity is unavailable; configure context_limit in the selected registry profile".into(),
                                }));
                            }
                            Err(source) => {
                                if is_retryable_transport_error(&source)
                                    && attempt < retry.max_attempts
                                {
                                    tracing::warn!(
                                        task_id = %request.task_id,
                                        attempt,
                                        %source,
                                        "context-limit discovery failed; retrying"
                                    );
                                    let deadline = Instant::now()
                                        + retry.backoff_delay(attempt + 1).min(budget.remaining());
                                    while Instant::now() < deadline && !cancellation.is_cancelled()
                                    {
                                        thread::sleep(Duration::from_millis(10).min(
                                            deadline.saturating_duration_since(Instant::now()),
                                        ));
                                    }
                                } else {
                                    return Err(KvistError::Sav(source));
                                }
                            }
                        }
                    }
                }
            };
            let reserve = native
                .response_reserve
                .unwrap_or((limit / 4).clamp(1, 8192) as u32);
            native.context_limit = Some(limit);
            native.response_reserve = Some(reserve);
            live.record(&serde_json::json!({"type":"model_budget","data":{"context_limit":limit,"response_reserve":reserve}}),true)?;
            sav::runtime::ContextManager::new(limit, 4)
        } else {
            sav::runtime::ContextManager::new(sav::runtime::DEFAULT_CONTEXT_TOKENS, 4)
        };

        let mut conversation = sav::runtime::AgentSession::new(
            sav::runtime::ModelIdentity {
                id: choice.model.clone(),
                model: choice
                    .native
                    .as_ref()
                    .map(|native| native.model.clone())
                    .or_else(|| extract_provider_model(&choice.command))
                    .unwrap_or_else(|| choice.model.clone()),
            },
            profile
                .thinking_effort
                .unwrap_or(sav::ReasoningEffort::None),
            authoring::authoring_tool_definitions(),
            context::system_context(
                &request,
                choice.system_prompt.as_deref().unwrap_or_default(),
            )?,
        );
        conversation.push_user(request.prompt);

        let mut generations = 0u64;
        let mut model_attempts = 0;

        loop {
            if cancellation.is_cancelled() || controls.is_shutdown() {
                return Err(KvistError::Sav(ModelError::Cancelled));
            }
            if steering.deliver(&mut conversation)? {
                model_attempts = 0;
            }

            // 1. The model turn runs on the host: numeric loopback gateway only,
            //    liveness-probed, transient availability failures retried in the
            //    shared budget. Streamed deltas are redacted with the same values
            //    as the durable log.
            generations = generations.saturating_add(1);
            let operation = sav::runtime::Operation::start(cancellation, controls)?;
            let reserve = choice
                .native
                .as_ref()
                .and_then(|native| native.response_reserve)
                .unwrap_or(1024);
            let (model_request, compaction) = conversation
                .prepare_request(&mut context, reserve)?
                .ok_or_else(|| {
                    KvistError::Runtime(sav::runtime::Error::InvalidModelTurn {
                        reason: "protected conversation lost its original task".into(),
                    })
                })?;
            if let Some(compaction) = compaction {
                live.record(&serde_json::json!({"type":"context_compaction","data":{"completed_groups":compaction.compacted_turns,"compactions":context.compactions(),"lossy":true,"canonical_evidence":false}}),true)?;
            }
            let generated = execute_host_turn(
                &choice,
                profile,
                &request,
                &budget,
                &redactions,
                &model_request,
                operation.token(),
                &live,
                &mut model_attempts,
                &mut tokens,
            );
            // Retain a completed turn before validation, controls, or any
            // effect/error path can exit the owner.
            if let Ok(turn) = &generated {
                run_turns.push(RunTurn {
                    turn: turn.clone(),
                    effects: vec![],
                    drops: vec![],
                    surfaced_decisions: vec![],
                    accepted_requests: vec![],
                });
                tokens.check()?;
            }
            if cancellation.is_cancelled() || controls.is_shutdown() {
                return Err(KvistError::Sav(ModelError::Cancelled));
            }
            if (operation.was_steered() || controls.has_pending())
                && (generated.is_ok()
                    || matches!(
                        &generated,
                        Err(KvistError::Sav(ModelError::ModelTransportCancelled))
                    ))
            {
                live.record(&serde_json::json!({"type":"generation_interrupted","data":{"reason":"guidance","generation":generations}}),true)?;
                if let Ok(turn) = generated {
                    sav::runtime::validate_turn(&turn)?;
                    conversation.apply_assistant(turn.clone());
                    steering.skip_calls(turn.tool_intents.iter(), &[], &mut conversation)?;
                }
                drop(operation);
                continue;
            }
            drop(operation);
            let turn = match generated {
                Ok(turn) => turn,
                Err(source) => {
                    tracing::warn!(
                        task_id = %request.task_id,
                        %source,
                        "agent model turn failed"
                    );
                    let failure = TurnFailure::from_kvist_error(&source);
                    if failure.cancelled {
                        return Err(source);
                    }
                    return Ok(Some(failure));
                }
            };
            sav::runtime::validate_turn(&turn)?;
            let turn_index = run_turns.len() - 1;
            model_attempts = 0;

            // Feed the assistant message back so the model observes its own output.
            conversation.apply_assistant(turn.clone());
            if controls.has_pending() {
                steering.skip_calls(turn.tool_intents.iter(), &[], &mut conversation)?;
                continue;
            }

            // 2. Broker: reduce untrusted intents to capability-bound effects.
            let plan =
                authoring::authorize_turn(request.target_dir, &turn, &BrokerPolicy::default());
            run_turns[turn_index].drops = plan.dropped.clone();
            live.record(&serde_json::json!({"type":"broker_decision","data":{
            "authorized_effects":plan.effects.len(),"denied_intents":plan.dropped.len(),
            "decisions":plan.decisions.len(),"dependency_requests":plan.dependency_requests.len()
        }}), true)?;

            // 2a. An impactful, uncovered decision pauses the run for the human
            //     before any effect applies: record each surfaced decision as durable
            //     evidence and stop the run without applying this turn's effects. A
            //     decision takes precedence over a dropped intent so a decision-worthy
            //     issue is never masked by a turn failure; the turn still records the
            //     dropped intents for the evidence trail.
            if !plan.decisions.is_empty() {
                let mut recorded = Vec::with_capacity(plan.decisions.len());
                for decision in &plan.decisions {
                    let path = record_decision_evidence(request.target_dir, decision, &redactions)?;
                    tracing::info!(
                        task_id = %request.task_id,
                        path = %path.display(),
                        decision = %decision.call_id,
                        "surfaced a decision worthy of intervention; pausing run for the human"
                    );
                    recorded.push(decision.clone());
                }
                run_turns[turn_index].surfaced_decisions = recorded;
                run_turns[turn_index].accepted_requests = plan.dependency_requests.clone();
                return Ok(None);
            }

            // 2b. In-policy dependency requests are recorded as durable evidence and
            //     immediately trigger the acquisition phase. The agent continues
            //     without interruption after the new dependency is available.
            let mut accepted_requests = Vec::with_capacity(plan.dependency_requests.len());
            for dep_request in &plan.dependency_requests {
                let path =
                    record_dependency_evidence(request.target_dir, dep_request, &redactions)?;
                tracing::info!(
                    task_id = %request.task_id,
                    path = %path.display(),
                    dependency = %dep_request.name,
                    "accepted an in-policy dependency request for acquisition"
                );
                accepted_requests.push(dep_request.clone());
                run_turns[turn_index]
                    .accepted_requests
                    .push(dep_request.clone());
            }

            // Trigger the acquisition phase if there were accepted dependency requests.
            if !accepted_requests.is_empty() {
                tracing::info!(
                    task_id = %request.task_id,
                    dependency_count = accepted_requests.len(),
                    "triggering dependency acquisition phase"
                );
                let expected_runner = expected_runner.ok_or_else(|| {
                    KvistError::SandboxUnavailable {
                        runner: sandbox_config.runner.clone(),
                        reason: "protected dependency requests require the task-approved runner; host fallback is disabled".into(),
                    }
                })?;
                match crate::vendor_command::vendor_project_protected(
                    request.project_root,
                    crate::vendor_command::ProtectedVendorOptions {
                        sandbox: sandbox_config,
                        vcs_selection: request.vcs_selection,
                        expected_runner,
                        policy_identity: request.policy_identity,
                        cancellation: steering.cancellation,
                        remaining_budget: budget.remaining(),
                    },
                ) {
                    Ok(report) => {
                        tracing::info!(
                            task_id = %request.task_id,
                            ready = report.ready(),
                            "dependency acquisition phase completed"
                        );
                    }
                    Err(source) => return Err(source),
                }
            }

            // 3. A dropped intent fails the turn before any effect runs: partial
            //    application would misrepresent what the model requested. The model
            //    is told what was refused so it can pursue an authorized path.
            if !plan.dropped.is_empty() {
                let reason = drop_reason(&plan.dropped);
                tracing::warn!(task_id = %request.task_id, %reason);
                return Ok(Some(TurnFailure {
                    reason,
                    timed_out: false,
                    output_limit_exceeded: false,
                    cancelled: false,
                }));
            }

            // No intents at all: the model produced a final answer rather than
            // another action. Complete the run.
            if plan.effects.is_empty() {
                if !controls.finish_if_idle()? {
                    steering.skip_calls(turn.tool_intents.iter(), &[], &mut conversation)?;
                    continue;
                }
                break;
            }

            // Authorized effects, but no approved sandbox runner or
            // capability-confirmed probe: they cannot be applied safely. Fail
            // closed rather than skip them.
            let (Some(expected_runner), Some(probe)) = (expected_runner, probe) else {
                let reason = format!(
                    "{} authorized effect(s) cannot be applied: no approved sandbox runner or capability-confirmed probe is available for the authoring phase",
                    plan.effects.len()
                );
                tracing::warn!(task_id = %request.task_id, %reason);
                return Ok(Some(TurnFailure {
                    reason,
                    timed_out: false,
                    output_limit_exceeded: false,
                    cancelled: false,
                }));
            };

            let sandbox_context = EffectSandbox {
                config: sandbox_config,
                expected_runner,
                probe,
                steering: &steering,
                budget: &budget,
            };
            apply_authoring_effects(
                profile,
                &sandbox_context,
                &request,
                &plan,
                &turn,
                &session_id,
                &live,
                &mut run_turns[turn_index].effects,
            )?;
            let effects = &run_turns[turn_index].effects;

            // Feed each effect's outcome back to the model as a tool result so it
            // can react to a failed or skipped write on a later turn.
            for record in effects {
                let content = if record.applied {
                    "applied".to_owned()
                } else {
                    format!("did not apply: {}", record.detail)
                };
                conversation.record_tool_result(
                    &record.call_id,
                    &record.tool,
                    &sav::runtime::ToolOutcome {
                        exited: true,
                        status: Some(if record.applied { 0 } else { 1 }),
                        stdout: content.into_bytes(),
                        stderr: vec![],
                        timed_out: record.timed_out,
                        output_limit_exceeded: record.output_limit_exceeded,
                        cancelled: false,
                    },
                );
            }
            if effects.len() < plan.effects.len() {
                steering.skip_calls(
                    turn.tool_intents.iter().filter(|intent| {
                        plan.effects
                            .iter()
                            .any(|effect| effect.call_id == intent.id)
                    }),
                    effects,
                    &mut conversation,
                )?;
            }

            // An unapplied effect fails the turn: partial application would
            // misrepresent what the model requested.
            if effects.iter().any(|record| !record.applied) {
                let reason = apply_reason(effects);
                tracing::warn!(task_id = %request.task_id, %reason);
                return Ok(Some(TurnFailure {
                    reason,
                    timed_out: false,
                    output_limit_exceeded: false,
                    cancelled: false,
                }));
            }
        }
        Ok(None)
    })();
    let failure = match &outcome {
        Ok(failure) => failure.as_ref().map(|failure| TurnFailure {
            reason: failure.reason.clone(),
            timed_out: failure.timed_out,
            output_limit_exceeded: failure.output_limit_exceeded,
            cancelled: failure.cancelled,
        }),
        Err(error) => Some(TurnFailure::from_kvist_error(error)),
    };
    // Finalize once, outside the execution closure: errors must not discard
    // already completed turns or partially applied/uncertain effects.
    let recorded = record_turn_outcome(
        profile,
        &request,
        &session_id,
        &log_path,
        &mut log_file,
        &redactions,
        &run_turns,
        failure.as_ref(),
        tokens.uncertain_attempts,
    );
    let outcome = match outcome {
        Err(error) => {
            if let Err(recording) = &recorded {
                tracing::error!(%recording, "could not retain interrupted agent evidence");
            }
            Err(error)
        }
        Ok(_) => recorded,
    };
    let closed = steering.close();
    let disposition = match &outcome {
        Ok(result) if result.surfaced_decision => "awaiting_decision",
        Ok(result) if result.success => "completed",
        Ok(_) => "failed",
        Err(KvistError::Sav(ModelError::Cancelled | ModelError::ModelTransportCancelled)) => {
            "cancelled"
        }
        Err(_) => "failed",
    };
    let finished = live.finish(disposition);
    if outcome.is_ok() {
        closed?;
        finished?;
    }
    outcome
}

/// Assembles the bounded, redacted, durable evidence for one agent run (log
/// file, trajectory journal, structured result) and returns the result.
///
/// Called exactly once per run, on the success and failure paths alike, so
/// evidence can never diverge between them: stdout carries the model's text,
/// stderr carries the failure diagnostic (empty on a clean run), and the log
/// records both plus the broker's decision and every effect's outcome across
/// every turn. The run succeeds only when the final turn is a usable answer,
/// no proposed intent was dropped, and every authorized effect applied.
#[allow(clippy::too_many_arguments)]
fn record_turn_outcome(
    profile: &AgentProfile,
    request: &AgentExecutionRequest<'_>,
    session_id: &str,
    log_path: &Path,
    log_file: &mut fs::File,
    redactions: &[String],
    run_turns: &[RunTurn],
    failure: Option<&TurnFailure>,
    uncertain_attempts: u32,
) -> Result<AgentRunResult> {
    // The final turn is the run's answer; the earlier turns are the steps that
    // led to it. Aggregate effects and dropped intents across the whole run so
    // the evidence is complete regardless of which turn produced them.
    let all_effects: Vec<EffectRecord> = run_turns
        .iter()
        .flat_map(|run_turn| run_turn.effects.iter().cloned())
        .collect();
    let all_dropped: Vec<DroppedIntent> = run_turns
        .iter()
        .flat_map(|run_turn| run_turn.drops.iter().cloned())
        .collect();
    let all_decisions: Vec<ProposedDecision> = run_turns
        .iter()
        .flat_map(|run_turn| run_turn.surfaced_decisions.iter().cloned())
        .collect();
    let all_accepted_requests: Vec<DependencyRequest> = run_turns
        .iter()
        .flat_map(|run_turn| run_turn.accepted_requests.iter().cloned())
        .collect();
    let last_turn = run_turns.last().map(|run_turn| run_turn.turn.clone());

    let turn_ok = last_turn
        .as_ref()
        .is_some_and(|turn| !turn.text.trim().is_empty() || !turn.tool_intents.is_empty());
    // A surfaced decision pauses the run for the human; it is neither a success
    // nor a failure, so force success off and report it separately.
    let surfaced_decision = !all_decisions.is_empty();
    let success = last_turn.is_some()
        && turn_ok
        && failure.is_none()
        && all_dropped.is_empty()
        && all_effects.iter().all(|record| record.applied)
        && !surfaced_decision;

    let stdout = redact_bounded(
        last_turn
            .as_ref()
            .map(|turn| turn.text.as_str())
            .unwrap_or_default(),
        redactions,
        profile.max_output_bytes,
    );
    let stderr = redact_bounded(
        failure
            .map(|failure| failure.reason.as_str())
            .unwrap_or_default(),
        redactions,
        profile.max_output_bytes,
    );
    let timed_out = failure.is_some_and(|failure| failure.timed_out)
        || all_effects.iter().any(|record| record.timed_out);
    let output_limit_exceeded = failure.is_some_and(|failure| failure.output_limit_exceeded)
        || all_effects
            .iter()
            .any(|record| record.output_limit_exceeded);

    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut reported = 0;
    for usage in run_turns.iter().filter_map(|run_turn| run_turn.turn.usage) {
        input = input.saturating_add(usage.input_tokens);
        output = output.saturating_add(usage.output_tokens);
        reported += 1;
    }
    let tokens_input = (reported > 0).then_some(input.min(usize::MAX as u64) as usize);
    let tokens_output = (reported > 0).then_some(output.min(usize::MAX as u64) as usize);
    let mut log = format!(
        "token accounting: known input={tokens_input:?}, known output={tokens_output:?}; \
         reported turns={reported}, unavailable turns={}, conservatively charged/unknown attempts={uncertain_attempts}\n\n",
        run_turns.len() - reported,
    );
    for (index, run_turn) in run_turns.iter().enumerate() {
        if index > 0 {
            log.push_str("\n\n");
        }
        let turn = &run_turn.turn;
        log.push_str(&format!("turn {}:\n", index + 1));
        if let Some(reasoning) = turn
            .reasoning
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            log.push_str("reasoning:\n");
            log.push_str(reasoning);
            log.push_str("\n\n");
        }
        if !turn.text.trim().is_empty() {
            log.push_str(&turn.text);
            if !turn.text.ends_with('\n') {
                log.push('\n');
            }
        }
    }
    if !all_effects.is_empty() || !all_dropped.is_empty() {
        log.push_str("\n--- authoring broker ---\n");
        for record in &all_effects {
            if record.applied {
                log.push_str(&format!(
                    "applied {} -> {}\n",
                    record.tool, record.destination
                ));
            } else {
                log.push_str(&format!(
                    "FAILED {} -> {}\n{}\n",
                    record.tool, record.destination, record.detail
                ));
            }
        }
        for dropped in &all_dropped {
            log.push_str(&format!(
                "dropped call {} (tool `{}`): {}\n",
                dropped.call_id, dropped.tool, dropped.reason
            ));
        }
    }
    if !all_decisions.is_empty() || !all_accepted_requests.is_empty() {
        log.push_str("\n--- surfaced decisions and dependency requests ---\n");
        for decision in &all_decisions {
            log.push_str(&format!(
                "decision {id} ({summary}): {why}\n",
                id = decision.call_id,
                summary = decision.summary,
                why = decision.why,
            ));
            if let Some(patch) = &decision.patch {
                log.push_str(&format!("  patch:\n{}\n", patch));
            }
        }
        for request in &all_accepted_requests {
            log.push_str(&format!(
                "dependency {} -> {} ({})\n",
                request.name,
                request.origin,
                request.reason.as_deref().unwrap_or("accepted")
            ));
        }
    }
    if let Some(failure) = failure {
        log.push_str("\n--- failure ---\n");
        log.push_str(&failure.reason);
        if !failure.reason.ends_with('\n') {
            log.push('\n');
        }
    }
    let redacted_log = redact_bounded(&log, redactions, profile.max_output_bytes);
    log_file
        .write_all(redacted_log.as_bytes())
        .map_err(|source| KvistError::Io {
            operation: "write agent log",
            path: log_path.to_path_buf(),
            source,
        })?;
    log_file.sync_all().map_err(|source| KvistError::Io {
        operation: "sync agent log",
        path: log_path.to_owned(),
        source,
    })?;

    // Record the structured session trajectory journal: one dispatch/result
    // pair per applied-or-failed effect across every turn, so `state_mutated`
    // reflects effects that actually ran rather than the overall turn outcome.
    let runs_dir = request.target_dir.join(".kvist").join("runs");
    let trajectory_path = runs_dir.join(format!("{session_id}.trajectory.jsonl"));
    let recorder = sav::TrajectoryRecorder::new(&trajectory_path);
    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    recorder.record_event(&sav::TrajectoryEvent::SessionStart {
        session_id: redact(session_id, redactions),
        task_id: redact(request.task_id, redactions),
        timestamp: now_ts,
    })?;

    let mut total_tokens: u64 = 0;
    for (index, run_turn) in run_turns.iter().enumerate() {
        let turn = &run_turn.turn;
        let turn_number = index + 1;
        let usage = turn.usage;
        let new_tokens = usage.map(|usage| usage.input_tokens);
        let output_tokens = usage.map(|usage| usage.output_tokens);
        total_tokens = total_tokens
            .saturating_add(new_tokens.unwrap_or(0))
            .saturating_add(output_tokens.unwrap_or(0));

        recorder.record_event(&sav::TrajectoryEvent::TurnStart {
            turn: turn_number,
            timestamp: now_ts,
        })?;
        recorder.record_event(&sav::TrajectoryEvent::PromptEval {
            turn: turn_number,
            cached_tokens: None,
            new_tokens,
            eval_duration_ms: None,
        })?;
        for record in &run_turn.effects {
            let args =
                serde_json::json!({ "destination": redact(&record.destination, redactions) });
            let tool = redact(&record.tool, redactions);
            let call_id = redact(&record.call_id, redactions);
            recorder.record_event(&sav::TrajectoryEvent::ToolDispatch {
                turn: turn_number,
                call_id: call_id.clone(),
                tool: tool.clone(),
                args: args.clone(),
                action_hash: sav::compute_action_hash(&tool, &args),
            })?;
            recorder.record_event(&sav::TrajectoryEvent::ToolResult {
                turn: turn_number,
                call_id,
                tool,
                stdout: if record.applied {
                    "applied".to_owned()
                } else {
                    String::new()
                },
                stderr: redact_bounded(&record.detail, redactions, profile.max_output_bytes),
                exit_code: if record.applied { 0 } else { 1 },
                bytes: record.detail.len(),
                // The legacy trajectory boolean cannot represent uncertainty;
                // conservatively retain possible mutation and explain it in stderr.
                state_mutated: record.applied || record.uncertain,
            })?;
        }
        let finish_reason = serde_json::to_value(&turn.finish_reason)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "stop".to_owned());
        recorder.record_event(&sav::TrajectoryEvent::TurnFinish {
            turn: turn_number,
            output_tokens,
            finish_reason: redact(&finish_reason, redactions),
        })?;
    }

    recorder.record_event(&sav::TrajectoryEvent::SessionFinish {
        session_id: redact(session_id, redactions),
        task_id: redact(request.task_id, redactions),
        total_turns: run_turns.len(),
        total_tokens,
        success,
    })?;

    Ok(AgentRunResult {
        success,
        tokens_input,
        tokens_output,
        log_path: log_path.to_path_buf(),
        stdout,
        stderr,
        timed_out,
        output_limit_exceeded,
        surfaced_decision,
    })
}

fn ensure_logs_directory(target_dir: &Path) -> Result<PathBuf> {
    let kvist_dir = target_dir.join(".kvist");
    ensure_real_directory(&kvist_dir, "create agent state directory")?;
    let logs_dir = kvist_dir.join("logs");
    ensure_real_directory(&logs_dir, "create agent logs directory")?;
    Ok(logs_dir)
}

fn ensure_real_directory(path: &Path, operation: &'static str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            Ok(())
        }
        Ok(_) => Err(KvistError::Io {
            operation,
            path: path.to_path_buf(),
            source: io::Error::other("directory must be a real directory"),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|source| KvistError::Io {
                operation,
                path: path.to_path_buf(),
                source,
            })
        }
        Err(source) => Err(KvistError::Io {
            operation,
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// The component-state subdirectory that holds brokered, non-effect evidence:
/// surfaced decisions and accepted dependency requests. It lives under `.kvist`
/// so it is Kvist-managed state, never writable by the model through a tool.
const AUTHORING_EVIDENCE_DIR: &str = "authoring";

/// Ensures the named authoring-evidence subdirectory exists without following a
/// link, creating the `.kvist/authoring` parent chain first. Reuses the same
/// real-directory guard as the log directory so a link at any level is refused.
fn ensure_authoring_evidence_dir(target_dir: &Path, leaf: &str) -> Result<PathBuf> {
    let kvist_dir = target_dir.join(".kvist");
    ensure_real_directory(&kvist_dir, "create agent state directory")?;
    let authoring_dir = kvist_dir.join(AUTHORING_EVIDENCE_DIR);
    ensure_real_directory(&authoring_dir, "create authoring evidence directory")?;
    let leaf_dir = authoring_dir.join(leaf);
    ensure_real_directory(&leaf_dir, "create authoring evidence directory")?;
    Ok(leaf_dir)
}

/// Records a surfaced decision as durable, redacted evidence under the component
/// state directory. It never writes a protected intent document; it only persists
/// a bounded JSON proposal the human can review. Failures to persist are surfaced
/// as a run failure so an unreadable state directory cannot silently lose a
/// decision.
fn record_decision_evidence(
    target_dir: &Path,
    decision: &ProposedDecision,
    redactions: &[String],
) -> Result<PathBuf> {
    let proposals_dir = ensure_authoring_evidence_dir(target_dir, "proposals")?;
    let redacted = ProposedDecision {
        call_id: redact(&decision.call_id, redactions),
        summary: redact(&decision.summary, redactions),
        why: redact(&decision.why, redactions),
        patch: decision
            .patch
            .as_ref()
            .map(|patch| redact(patch, redactions)),
    };
    let payload = serde_json::to_string_pretty(&redacted).map_err(|source| {
        KvistError::AuthoringEffectFailed {
            call_id: decision.call_id.clone(),
            reason: format!("cannot serialize the surfaced proposal: {source}"),
        }
    })?;
    let path = proposals_dir.join(evidence_filename(&decision.call_id));
    write_atomic(&path, payload)?;
    Ok(path)
}

/// Records an accepted, in-policy dependency request as durable, redacted
/// evidence under the component state directory so the later acquisition step has
/// an exact, inspectable record of what the agent requested and why it was
/// allowed.
fn record_dependency_evidence(
    target_dir: &Path,
    request: &DependencyRequest,
    redactions: &[String],
) -> Result<PathBuf> {
    let authoring_dir = ensure_authoring_evidence_dir(target_dir, "dependencies")?;
    let redacted = DependencyRequest {
        call_id: redact(&request.call_id, redactions),
        name: redact(&request.name, redactions),
        origin: redact(&request.origin, redactions),
        reason: request
            .reason
            .as_ref()
            .map(|reason| redact(reason, redactions)),
    };
    let payload = serde_json::to_string_pretty(&redacted).map_err(|source| {
        KvistError::AuthoringEffectFailed {
            call_id: request.call_id.clone(),
            reason: format!("cannot serialize the accepted dependency request: {source}"),
        }
    })?;
    let path = authoring_dir.join(evidence_filename(&request.call_id));
    write_atomic(&path, payload)?;
    Ok(path)
}

fn evidence_filename(call_id: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{}.json", hex::encode(Sha256::digest(call_id.as_bytes())))
}

/// Uses synchronized same-directory replacement without a predictable staging path.
fn write_atomic(path: &Path, payload: String) -> Result<()> {
    crate::file_io::replace_file_atomically(path, &payload)
}

fn redaction_values(profile: &AgentProfile, sandbox_config: &SandboxConfig) -> Vec<String> {
    let mut values = profile.redaction_values.clone();
    for value in sandbox::allowed_environment(sandbox_config, None).into_values() {
        if !values.contains(&value) {
            values.push(value);
        }
    }
    values
}

/// Replaces every configured redaction value with the redaction marker.
fn redact(text: &str, redactions: &[String]) -> String {
    let mut output = text.to_owned();
    for value in redactions {
        if !value.is_empty() {
            output = output.replace(value, "[REDACTED]");
        }
    }
    output
}

/// Redacts and then bounds evidence text to the run's output budget.
fn redact_bounded(text: &str, redactions: &[String], limit: usize) -> String {
    let mut output = redact(text, redactions);
    truncate_utf8(&mut output, limit);
    output
}

fn truncate_utf8(value: &mut String, limit: usize) {
    if value.len() <= limit {
        return;
    }
    let mut boundary = limit;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Model;

    #[test]
    fn extract_loopback_endpoint_strips_path_and_keeps_loopback_authority() {
        let command = r#"curl --silent --request POST --json '{"model":"m"}' -- "http://127.0.0.1:43329/api/chat""#;
        assert_eq!(
            extract_loopback_endpoint(command).expect("endpoint"),
            "http://127.0.0.1:43329"
        );
    }

    #[test]
    fn extract_loopback_endpoint_preserves_https_scheme() {
        let command = "curl \"https://127.0.0.1:9931/v1/chat/completions\"";
        assert_eq!(
            extract_loopback_endpoint(command).expect("endpoint"),
            "https://127.0.0.1:9931"
        );
    }

    #[test]
    fn extract_loopback_endpoint_fails_without_a_scheme() {
        let command = "curl -- \"127.0.0.1:43329/api/chat\"";
        assert!(extract_loopback_endpoint(command).is_err());
    }

    #[test]
    fn provider_from_command_maps_paths_to_wire_protocol() {
        assert_eq!(
            provider_from_command("/api/chat"),
            LocalModelProvider::Ollama
        );
        assert_eq!(
            provider_from_command("/api/generate"),
            LocalModelProvider::Ollama
        );
        assert_eq!(
            provider_from_command("/v1/chat/completions"),
            LocalModelProvider::LlamaServer
        );
        // A command with no recognized path defaults to the OpenAI seam.
        assert_eq!(
            provider_from_command("http://127.0.0.1:9931"),
            LocalModelProvider::LlamaServer
        );
    }

    #[test]
    fn extract_provider_model_recovers_name_from_shell_quoted_command() {
        // The profile command is a TOML literal string, so the runtime command
        // carries backslash-escaped quotes around the embedded JSON body.
        let command = r##"curl --disable --silent --show-error --fail-with-body --request POST --json "{\"model\":\"Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL\",\"messages\":[]}" -- "http://127.0.0.1:9931/v1/chat/completions""##;
        assert_eq!(
            extract_provider_model(command),
            Some("Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL".to_owned())
        );
    }

    #[test]
    fn extract_provider_model_recovers_name_from_clean_json_command() {
        // A JSON body without surrounding shell quotes, plus one that injects
        // the identifier via a --data flag.
        let command = r##"curl --json "{\"model\":\"qwen-9b\",\"messages\":[]}" "http://127.0.0.1:8080/v1/chat/completions""##;
        assert_eq!(extract_provider_model(command), Some("qwen-9b".to_owned()));
        // A provider template that injects the identifier directly into the body.
        let command = "curl \"http://127.0.0.1:8080/v1/chat/completions\" --data {\"model\":\"gemini\",\"messages\":[]}";
        assert_eq!(extract_provider_model(command), Some("gemini".to_owned()));
    }

    #[test]
    fn extract_provider_model_returns_none_when_no_identifier() {
        // No model key at all.
        let command = "curl \"http://127.0.0.1:8080/v1/chat/completions\"";
        assert_eq!(extract_provider_model(command), None);
        // `model` inside another token is not a key.
        let command =
            r##"curl --json "{\"model_id\":\"x\",\"messages\":[]}" "http://127.0.0.1:8080""##;
        assert_eq!(extract_provider_model(command), None);
    }

    #[test]
    fn select_model_choice_prefers_explicit_then_default_then_role_template() {
        let profile = AgentProfile {
            role: crate::config::Role::Developer,
            profile: "twos".to_owned(),
            resolved_profile: None,
            command_template: "template-for-role".to_owned(),
            models: vec![
                Model {
                    name: "alpha".to_owned(),
                    command: "command-alpha".to_owned(),
                    system_prompt: Some("sys-alpha".to_owned()),
                },
                Model {
                    name: "beta".to_owned(),
                    command: "command-beta".to_owned(),
                    system_prompt: None,
                },
            ],
            default_model: "beta".to_owned(),
            model: None,
            thinking_effort: None,
            token_limit: None,
            timeout_seconds: 5,
            max_output_bytes: 1_024,
            redaction_values: vec![],
        };
        let alpha = select_model_choice(&profile, crate::config::Role::Developer, Some("alpha"))
            .expect("alpha selection");
        assert_eq!(alpha.model, "alpha");
        assert_eq!(alpha.command, "command-alpha");
        assert_eq!(alpha.system_prompt.as_deref(), Some("sys-alpha"));
        let beta =
            select_model_choice(&profile, crate::config::Role::Developer, None).expect("default");
        assert_eq!(beta.model, "beta");
        assert_eq!(beta.command, "command-beta");
        assert!(beta.system_prompt.is_none());
        let role = select_model_choice(&profile, crate::config::Role::Developer, Some("twos"))
            .expect("role template");
        assert_eq!(role.command, "template-for-role");
        assert!(
            select_model_choice(&profile, crate::config::Role::Developer, Some("ghost")).is_err()
        );
    }

    #[test]
    fn probe_gateway_reachable_reports_when_nothing_is_listening() {
        // Grab a free loopback port and release it so nothing listens there.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free port");
        let dead_port = listener.local_addr().expect("local addr").to_string();
        drop(listener);

        let endpoint = format!("http://{dead_port}");
        let budget = TurnBudget::new(Duration::from_secs(300));
        let error = probe_gateway_reachable(&endpoint, &budget, &CancellationToken::new())
            .expect_err("gateway should be unreachable");
        match error {
            KvistError::LocalModelGatewayUnreachable {
                endpoint: returned_endpoint,
                ..
            } => assert_eq!(returned_endpoint, endpoint),
            other => panic!("unexpected error kind: {other:?}"),
        }
    }

    #[test]
    fn stream_redaction_is_unicode_safe_and_partition_invariant() {
        for (text, values, expected) in [
            ("é", vec!["secret".into()], "é"),
            ("ababab", vec!["ababa".into()], "[REDACTED]b"),
            (
                "ésecret🙂secret",
                vec!["secret".into()],
                "é[REDACTED]🙂[REDACTED]",
            ),
            ("ababa", vec!["aba".into(), "ababa".into()], "[REDACTED]"),
        ] {
            let boundaries: Vec<usize> = text
                .char_indices()
                .map(|(index, _)| index)
                .chain(std::iter::once(text.len()))
                .collect();
            for first in &boundaries {
                for second in boundaries.iter().filter(|second| *second >= first) {
                    let mut redactor = StreamRedactor::new(&values);
                    let mut output = redactor.absorb(&text[..*first]);
                    output.push_str(&redactor.absorb(&text[*first..*second]));
                    output.push_str(&redactor.absorb(&text[*second..]));
                    output.push_str(&redactor.finish());
                    assert_eq!(output, expected, "partition {first}, {second}: {text}");
                }
            }
        }
    }

    #[test]
    fn probe_gateway_reachable_passes_when_a_listener_accepts() {
        // Hold a listener open for the duration of the probe so the connect
        // succeeds; dropping it at function end releases the port.
        let _listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free port");
        let endpoint = format!("http://{}", _listener.local_addr().expect("local addr"));
        let budget = TurnBudget::new(Duration::from_secs(300));
        probe_gateway_reachable(&endpoint, &budget, &CancellationToken::new())
            .expect("gateway should be reachable");
    }

    #[test]
    fn probe_gateway_reachable_stops_when_the_shared_budget_is_exhausted() {
        // Against a dead port a near-zero budget must fail fast with the
        // timeout classification rather than spending the full retry ladder.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free port");
        let dead_port = listener.local_addr().expect("local addr").to_string();
        drop(listener);
        let endpoint = format!("http://{dead_port}");
        let budget = TurnBudget::new(Duration::from_millis(1));
        thread::sleep(Duration::from_millis(5));
        let error = probe_gateway_reachable(&endpoint, &budget, &CancellationToken::new())
            .expect_err("budget must be exhausted");
        assert!(
            matches!(
                error,
                KvistError::Sav(ModelError::ModelTransportTimedOut)
                    | KvistError::LocalModelGatewayUnreachable { .. }
            ),
            "expected a timeout or unreachable classification, got {error:?}"
        );
    }

    #[test]
    fn is_retryable_transport_error_only_flags_transient_availability() {
        assert!(is_retryable_transport_error(
            &ModelError::ModelTransportTimedOut
        ));

        assert!(is_retryable_transport_error(
            &ModelError::ModelTransportIo {
                operation: "connect",
                source: std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused"),
            }
        ));
        assert!(is_retryable_transport_error(
            &ModelError::ModelTransportIo {
                operation: "connect",
                source: std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out"),
            }
        ));
        assert!(is_retryable_transport_error(
            &ModelError::ModelTransportIo {
                operation: "connect",
                source: std::io::Error::new(std::io::ErrorKind::Interrupted, "interrupted"),
            }
        ));
        assert!(is_retryable_transport_error(
            &ModelError::ModelTransportIo {
                operation: "connect",
                source: std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset by peer"),
            }
        ));
        // The model slot has not been allocated yet: the gateway accepted the
        // connection but is still loading or spawning the model.
        assert!(is_retryable_transport_error(
            &ModelError::SlotAllocationTimedOut {
                timeout: Duration::from_secs(15)
            }
        ));

        // Response-level failures must never be retried.
        assert!(!is_retryable_transport_error(
            &ModelError::ModelTransportCancelled
        ));
        assert!(!is_retryable_transport_error(&ModelError::TtftTimedOut {
            timeout: Duration::from_secs(45)
        }));
        assert!(!is_retryable_transport_error(
            &ModelError::ModelResponseLimitExceeded { max_bytes: 1024 }
        ));
        assert!(!is_retryable_transport_error(
            &ModelError::ModelProviderStatus { status: 500 }
        ));
        assert!(!is_retryable_transport_error(
            &ModelError::MalformedModelResponse {
                reason: "bad json".to_owned(),
            }
        ));
    }

    #[test]
    fn multi_day_task_deadlines_fit_direct_transport() {
        let budget = TurnBudget::new(Duration::from_secs(604_800));
        let deadline = budget.model_deadline(&CancellationToken::new()).unwrap();
        assert!(deadline <= Duration::from_secs(86_400));
        assert!(budget.remaining() > Duration::from_secs(6 * 86_400));
        DirectModelTransport::new(
            sav::LocalModelProvider::Ollama,
            "http://127.0.0.1:11434",
            deadline,
            65536,
        )
        .unwrap();
    }

    /// The failure class a scripted transport produces.
    #[derive(Clone, Copy)]
    enum FailureKind {
        /// A transient peer reset: retryable.
        Transient,
        /// A real provider status failure: never retryable.
        ProviderStatus(u16),
    }

    impl FailureKind {
        fn into_error(self) -> ModelError {
            match self {
                Self::Transient => ModelError::ModelTransportIo {
                    operation: "connect",
                    source: std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "reset by peer",
                    ),
                },
                Self::ProviderStatus(status) => ModelError::ModelProviderStatus { status },
            }
        }
    }

    /// A scripted model transport for retry-loop tests: it fails its first
    /// `failures` attempts with `failure`, then succeeds. Attempt counts let the
    /// tests assert exactly how many times the loop ran.
    #[derive(Clone)]
    struct FlakyTransport {
        failures: std::sync::Arc<std::sync::Mutex<u32>>,
        attempts: std::sync::Arc<std::sync::Mutex<u32>>,
        failure: FailureKind,
    }

    impl FlakyTransport {
        fn new(failures: u32, failure: FailureKind) -> Self {
            Self {
                failures: std::sync::Arc::new(std::sync::Mutex::new(failures)),
                attempts: std::sync::Arc::new(std::sync::Mutex::new(0)),
                failure,
            }
        }

        fn attempt_count(&self) -> u32 {
            *self
                .attempts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        fn fail_once(&self) -> Option<ModelError> {
            let mut failures = self
                .failures
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *failures > 0 {
                *failures -= 1;
                Some(self.failure.into_error())
            } else {
                None
            }
        }

        fn finish_turn(&self, model: &str) -> ModelTurn {
            ModelTurn {
                text: "recovered".to_owned(),
                reasoning: None,
                tool_intents: Vec::new(),
                finish_reason: sav::FinishReason::Stop,
                provider: LocalModelProvider::Ollama,
                model: model.to_owned(),
                response_id: None,
                provider_request_id: None,
                usage: None,
            }
        }
    }

    impl ModelTransport for FlakyTransport {
        fn complete(
            &self,
            request: &ModelRequest,
            _cancellation: &CancellationToken,
        ) -> sav::Result<ModelTurn> {
            let mut attempts = self
                .attempts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *attempts += 1;
            drop(attempts);
            if let Some(error) = self.fail_once() {
                return Err(error);
            }
            Ok(self.finish_turn(&request.model))
        }

        fn stream(
            &self,
            _request: &ModelRequest,
            _cancellation: &CancellationToken,
            on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            let mut attempts = self
                .attempts
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *attempts += 1;
            drop(attempts);
            // Stream some text, then fail with a transient error: the emitted
            // text must rule out any retry of this attempt.
            on_event(ModelStreamEvent::TextDelta("partial ".to_owned()))?;
            Err(self.failure.into_error())
        }

        fn deadline(&self) -> Duration {
            Duration::from_secs(1)
        }
    }

    fn test_request(model: &str) -> ModelRequest {
        ModelRequest {
            model: model.to_owned(),
            messages: vec![ModelMessage::User("prompt".to_owned())],
            tools: Vec::new(),
            tool_choice: ToolChoice::None,
            reasoning_effort: None,
            output_schema: None,
            max_output_tokens: None,
        }
    }

    #[test]
    fn turn_retry_recovers_from_transient_resets_within_the_attempt_bound() {
        let transport = FlakyTransport::new(2, FailureKind::Transient);
        let budget = TurnBudget::new(Duration::from_secs(60));
        let turn = complete_turn(
            || Ok::<FlakyTransport, KvistError>(transport.clone()),
            &test_request("model-a"),
            &CancellationToken::new(),
            &budget,
            false,
            &[],
            "task-retry",
        )
        .expect("turn recovers after transient resets");
        assert_eq!(turn.text, "recovered");
        assert_eq!(
            transport.attempt_count(),
            MODEL_TURN_MAX_ATTEMPTS,
            "first attempt plus two retries"
        );
    }

    #[test]
    fn non_retryable_failure_is_surfaced_immediately() {
        let transport = FlakyTransport::new(3, FailureKind::ProviderStatus(500));
        let budget = TurnBudget::new(Duration::from_secs(60));
        let error = complete_turn(
            || Ok::<FlakyTransport, KvistError>(transport.clone()),
            &test_request("model-a"),
            &CancellationToken::new(),
            &budget,
            false,
            &[],
            "task-status",
        )
        .expect_err("a real provider failure must not be retried");
        assert!(
            matches!(
                error,
                KvistError::Sav(ModelError::ModelProviderStatus { status: 500 })
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(transport.attempt_count(), 1);
    }

    #[test]
    fn retries_are_bounded_by_the_attempt_limit() {
        let transport = FlakyTransport::new(10, FailureKind::Transient);
        let budget = TurnBudget::new(Duration::from_secs(60));
        let error = complete_turn(
            || Ok::<FlakyTransport, KvistError>(transport.clone()),
            &test_request("model-a"),
            &CancellationToken::new(),
            &budget,
            false,
            &[],
            "task-exhausted",
        )
        .expect_err("retries must be bounded");
        assert!(
            matches!(error, KvistError::Sav(ModelError::ModelTransportIo { .. })),
            "the last transient error must be surfaced: {error:?}"
        );
        assert_eq!(transport.attempt_count(), MODEL_TURN_MAX_ATTEMPTS);
    }

    #[test]
    fn streamed_text_is_never_retried() {
        let transport = FlakyTransport::new(10, FailureKind::Transient);
        let budget = TurnBudget::new(Duration::from_secs(60));
        let error = complete_turn(
            || Ok::<FlakyTransport, KvistError>(transport.clone()),
            &test_request("model-a"),
            &CancellationToken::new(),
            &budget,
            true,
            &[],
            "task-streamed",
        )
        .expect_err("a streamed attempt that emitted text must fail, not retry");
        assert!(matches!(
            error,
            KvistError::Sav(ModelError::ModelTransportIo { .. })
        ));
        assert_eq!(
            transport.attempt_count(),
            1,
            "emitted text cannot be retracted, so the attempt must not repeat"
        );
    }

    #[test]
    fn exhausted_budget_fails_before_any_attempt() {
        let transport = FlakyTransport::new(0, FailureKind::Transient);
        let budget = TurnBudget::new(Duration::from_millis(1));
        thread::sleep(Duration::from_millis(5));
        let error = complete_turn(
            || Ok::<FlakyTransport, KvistError>(transport.clone()),
            &test_request("model-a"),
            &CancellationToken::new(),
            &budget,
            false,
            &[],
            "task-budget",
        )
        .expect_err("an exhausted budget must fail closed");
        assert!(
            matches!(error, KvistError::Sav(ModelError::ModelTransportTimedOut)),
            "unexpected error: {error:?}"
        );
        assert_eq!(transport.attempt_count(), 0);
    }

    pub(super) fn test_execution_request<'a>(target: &'a Path) -> AgentExecutionRequest<'a> {
        AgentExecutionRequest {
            project_root: target,
            vcs_selection: crate::config::VcsSelection::Git,
            prompt: "unused",
            context_paths: &[],
            read_only_mounts: &[],
            target_dir: target,
            task_id: "task-refusal",
            stream_output: false,
            role: crate::config::Role::Developer,
            policy_identity: "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        }
    }

    pub(super) fn test_profile(command: &str) -> AgentProfile {
        AgentProfile {
            role: crate::config::Role::Developer,
            profile: "default".to_owned(),
            resolved_profile: None,
            command_template: command.to_owned(),
            models: vec![Model {
                name: "default".to_owned(),
                command: command.to_owned(),
                system_prompt: None,
            }],
            default_model: "default".to_owned(),
            model: None,
            thinking_effort: None,
            token_limit: None,
            timeout_seconds: 5,
            max_output_bytes: 1_024,
            redaction_values: vec![],
        }
    }

    #[test]
    fn non_loopback_command_is_refused_before_any_transport_work() {
        let log_dir = tempfile::tempdir().expect("logs");
        let live = LiveRun::create(
            &log_dir.path().join("live"),
            "refusal",
            "fixture",
            "fixture",
            &[],
        )
        .expect("live");
        let target = std::env::temp_dir();
        let request = test_execution_request(&target);
        let profile = test_profile("claude --non-interactive --message '{prompt}'");
        let choice = ModelChoice {
            native: None,
            model: "claude-default".to_owned(),
            command: "claude --non-interactive --message '{prompt}'".to_owned(),
            system_prompt: None,
            is_none: false,
        };
        let budget = TurnBudget::new(Duration::from_secs(60));
        let error = execute_host_turn(
            &choice,
            &profile,
            &request,
            &budget,
            &[],
            &test_request("fixture"),
            &sav::CancellationToken::new(),
            &live,
            &mut 0,
            &mut TokenBudget::new(None),
        )
        .expect_err("a non-loopback command must be refused");
        assert!(
            matches!(error, KvistError::AgentCommandNotModelGateway { .. }),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn none_model_is_refused_without_reaching_the_gateway() {
        let log_dir = tempfile::tempdir().expect("logs");
        let live = LiveRun::create(
            &log_dir.path().join("live"),
            "refusal",
            "fixture",
            "fixture",
            &[],
        )
        .expect("live");
        let target = std::env::temp_dir();
        let request = test_execution_request(&target);
        let profile = test_profile("curl -- 'http://127.0.0.1:1/api/chat'");
        let choice = ModelChoice {
            native: None,
            model: "none".to_owned(),
            command: "none".to_owned(),
            system_prompt: None,
            is_none: true,
        };
        let budget = TurnBudget::new(Duration::from_secs(60));
        let error = execute_host_turn(
            &choice,
            &profile,
            &request,
            &budget,
            &[],
            &test_request("fixture"),
            &sav::CancellationToken::new(),
            &live,
            &mut 0,
            &mut TokenBudget::new(None),
        )
        .expect_err("the no-op model must be refused");
        assert!(
            matches!(error, KvistError::InvalidModelSelection { .. }),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn stream_redaction_replaces_values_within_a_single_delta() {
        let mut redactor = StreamRedactor::new(&["top-secret".to_owned()]);
        let emitted = redactor.absorb("hello top-secret world");
        let tail = redactor.finish();
        let output = format!("{emitted}{tail}");
        assert!(!output.contains("top-secret"), "secret leaked: {output}");
        assert!(output.contains("[REDACTED]"), "marker missing: {output}");
        assert_eq!(output, "hello [REDACTED] world");
    }

    #[test]
    fn stream_redaction_catches_values_split_across_deltas() {
        let mut redactor = StreamRedactor::new(&["top-secret".to_owned()]);
        let first = redactor.absorb("value is top-");
        let second = redactor.absorb("secret value");
        let tail = redactor.finish();
        let output = format!("{first}{second}{tail}");
        assert!(
            !output.contains("top-secret"),
            "split secret leaked: {output}"
        );
        assert!(output.contains("[REDACTED]"), "marker missing: {output}");
        assert_eq!(output, "value is [REDACTED] value");
    }

    #[test]
    fn stream_redaction_without_values_is_a_pass_through() {
        let mut redactor = StreamRedactor::new(&[]);
        let first = redactor.absorb("abc");
        let second = redactor.absorb("def");
        let tail = redactor.finish();
        assert_eq!(format!("{first}{second}{tail}"), "abcdef");
    }

    #[test]
    fn stream_redaction_emits_nothing_before_the_first_delta() {
        let mut redactor = StreamRedactor::new(&["secret".to_owned()]);
        assert!(redactor.finish().is_empty());
    }

    fn effect(call_id: &str) -> CheckedIntent {
        CheckedIntent {
            call_id: call_id.to_owned(),
            tool: "write_file".to_owned(),
            op: crate::authoring::EffectOp::Create,
            destination: "tests/generated.rs".to_owned(),
            content_identity:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
            replacement: None,
            content_bytes: 3,
            purpose: "test".to_owned(),
        }
    }

    fn execution_result(
        status_code: i32,
        stdout: &str,
        stderr: &str,
    ) -> crate::sandbox::ExecutionResult {
        crate::sandbox::ExecutionResult {
            output: std::process::Output {
                status: std::os::unix::process::ExitStatusExt::from_raw(status_code),
                stdout: stdout.as_bytes().to_vec(),
                stderr: stderr.as_bytes().to_vec(),
            },
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }
    }

    #[test]
    fn summarize_effect_records_success_without_detail() {
        let record = summarize_effect(&effect("call-1"), execution_result(0, "", ""));
        assert!(record.applied);
        assert!(record.detail.is_empty());
        assert_eq!(record.call_id, "call-1");
        assert_eq!(record.destination, "tests/generated.rs");
    }

    #[test]
    fn summarize_effect_captures_bounded_diagnostics_for_failures() {
        let mut result = execution_result(1, "stdout-line", "stderr-line");
        result.timed_out = true;
        let record = summarize_effect(&effect("call-1"), result);
        assert!(!record.applied);
        assert!(record.timed_out);
        assert!(record.detail.contains("after its wall-clock limit"));
        assert!(record.detail.contains("stdout-line"));
        assert!(record.detail.contains("stderr-line"));
    }

    #[test]
    fn summarize_effect_bounds_the_detail_to_four_kilobytes() {
        let huge = "x".repeat(8192);
        let record = summarize_effect(&effect("call-1"), execution_result(1, &huge, ""));
        assert!(!record.applied);
        assert!(record.detail.len() <= 4096);
    }

    #[test]
    fn turn_failure_classifies_deadlines_limits_and_cancellation() {
        let timeout =
            TurnFailure::from_kvist_error(&KvistError::Sav(ModelError::ModelTransportTimedOut));
        assert!(timeout.timed_out);
        assert!(!timeout.cancelled);

        let cancelled =
            TurnFailure::from_kvist_error(&KvistError::Sav(ModelError::ModelTransportCancelled));
        assert!(cancelled.cancelled);
        assert!(!cancelled.timed_out);

        let oversized = TurnFailure::from_kvist_error(&KvistError::Sav(
            ModelError::ModelResponseLimitExceeded { max_bytes: 1 },
        ));
        assert!(oversized.output_limit_exceeded);

        let other =
            TurnFailure::from_kvist_error(&KvistError::Sav(ModelError::ModelProviderStatus {
                status: 500,
            }));
        assert!(!other.timed_out && !other.output_limit_exceeded && !other.cancelled);
        assert!(other.reason.contains("500"));
    }
}
