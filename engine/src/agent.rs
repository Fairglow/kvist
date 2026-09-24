//! External agent execution and response capture.
//!
//! The supervised task path performs the model turn on the host against a
//! numeric loopback model gateway (never inside the effect sandbox, which has
//! its own empty loopback). The turn advertises the closed authoring tool set;
//! any intent the model proposes is reduced by the broker in [`crate::authoring`]
//! to capability-bound effects that are applied exclusively inside the effect
//! sandbox. Any other command is refused, so no agent command ever runs
//! outside the effect sandbox.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use crate::{
    KvistError, Result,
    authoring::{self, AuthoringPlan, BrokerPolicy, CheckedIntent, DroppedIntent},
    config::{AgentProfile, SandboxConfig, VcsSelection},
    sandbox,
    task_queue::Timestamp,
};

use agent_runtime::{
    CancellationToken, DirectModelTransport, Error as ModelError, LocalModelProvider, ModelMessage,
    ModelRequest, ModelStreamEvent, ModelTransport, ModelTurn, ToolChoice,
};

/// Structured execution result of an external agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunResult {
    /// True when the turn produced a usable result, no proposed intent was
    /// dropped, and every authorized effect applied.
    pub success: bool,
    /// Number of prompt/input tokens used, if reported by the provider.
    pub tokens_input: Option<usize>,
    /// Number of output/completion tokens used, if reported by the provider.
    pub tokens_output: Option<usize>,
    /// Path to the redacted execution log file.
    pub log_path: PathBuf,
    /// Bounded redacted model output retained as execution evidence.
    pub stdout: String,
    /// Bounded redacted failure diagnostics, empty on a clean turn.
    pub stderr: String,
    pub timed_out: bool,
    pub output_limit_exceeded: bool,
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
    agent_runtime::render_command(template, prompt, context_paths, target_dir).map_err(Into::into)
}

/// The maximum number of model turns one `task run` may perform before it stops.
///
/// The shared wall-clock budget is the primary limiter; this is a hard safety
/// net so a model that keeps proposing effects can never loop forever.
const MAX_AGENT_TURNS: usize = 64;

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
}

/// A resolved model identity plus the command template and system prompt for one selection.
struct ModelChoice {
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
    let model_name = model_override
        .or(profile.model.as_deref())
        .unwrap_or(&profile.default_model);
    let choice = if matches!(model_name, "default" | "default-model") {
        if let Some(first) = profile.models.first() {
            ModelChoice {
                model: first.name.clone(),
                command: first.command.clone(),
                system_prompt: first.system_prompt.clone(),
                is_none: first.name == "none",
            }
        } else if !profile.command_template.is_empty() {
            ModelChoice {
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
            model: found.name.clone(),
            command: found.command.clone(),
            system_prompt: found.system_prompt.clone(),
            is_none: found.name == "none",
        }
    } else if !profile.command_template.is_empty()
        && (model_name == profile.profile || model_name == role.as_str())
    {
        ModelChoice {
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
    reasoning_effort: Option<agent_runtime::ReasoningEffort>,
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

    agent_runtime::render_command_with_reasoning_effort(
        &selected_command,
        &prompt,
        context_paths,
        target_dir,
        effort,
    )
    .map_err(Into::into)
}

fn split_raw_command(template: &str) -> Result<(String, Vec<String>)> {
    agent_runtime::split_raw_command(template).map_err(Into::into)
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
}

/// Proves a loopback model gateway is accepting connections before a turn runs,
/// within the shared wall-clock budget.
///
/// `endpoint` is a numeric loopback URL such as `http://127.0.0.1:9931`. This
/// performs only a TCP connect; it never issues an HTTP request, so it cannot
/// load, select, or shift a model slot. It is a pure liveness check that lets a
/// down gateway fail fast with an actionable message instead of a cryptic
/// transport error buried deep inside a turn.
fn probe_gateway_reachable(endpoint: &str, budget: &TurnBudget) -> Result<()> {
    let authority = endpoint
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let addr: std::net::SocketAddr =
        authority
            .parse()
            .map_err(|source| KvistError::LocalModelGatewayUnreachable {
                endpoint: endpoint.to_owned(),
                reason: format!("endpoint `{endpoint}` is not a host:port address: {source}"),
            })?;

    let mut attempt = 1u32;
    loop {
        let remaining = budget.remaining();
        if remaining.is_zero() {
            return Err(KvistError::AgentRuntime(ModelError::ModelTransportTimedOut));
        }
        let connect_timeout = Duration::from_secs(2).min(remaining);
        match std::net::TcpStream::connect_timeout(&addr, connect_timeout) {
            Ok(_) => return Ok(()),
            Err(source) if attempt < GATEWAY_PROBE_ATTEMPTS => {
                tracing::warn!(
                    attempt,
                    %source,
                    endpoint,
                    "local model gateway not yet accepting connections; retrying liveness probe"
                );
                thread::sleep(GATEWAY_PROBE_BACKOFF.min(budget.remaining()));
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

/// Relays streaming text to standard output with the run's redaction values
/// applied.
///
/// A redaction value may straddle two deltas, so the redactor retains the
/// longest tail of every absorbed chunk that is still a prefix of some
/// redaction value and re-examines it as later deltas arrive; the retained
/// remainder is redacted once when the stream finishes. Without this carry, a
/// secret split across a delta boundary would reach the console unreplaced.
struct StreamRedactor {
    values: Vec<String>,
    carry: String,
    max_len: usize,
}

impl StreamRedactor {
    fn new(values: &[String]) -> Self {
        let max_len = values.iter().map(String::len).max().unwrap_or(0);
        Self {
            values: values.to_vec(),
            carry: String::new(),
            max_len,
        }
    }

    /// Folds a new delta into the retained tail and returns the redacted
    /// prefix that is safe to emit immediately.
    fn absorb(&mut self, delta: &str) -> String {
        self.carry.push_str(delta);
        let retain = self.max_suffix_prefix_len();
        let safe_len = self.carry.len() - retain;
        let safe = self.carry[..safe_len].to_owned();
        self.carry = self.carry[safe_len..].to_owned();
        redact(&safe, &self.values)
    }

    /// The longest carry suffix that is a prefix of a redaction value.
    ///
    /// Such a tail may complete into a secret once later deltas arrive, so it
    /// is held back instead of being emitted; every shorter candidate is
    /// covered by the longest one. A suffix that already equals a whole value
    /// is held back too, so it is redacted on the next absorb or on finish.
    fn max_suffix_prefix_len(&self) -> usize {
        let max = self.max_len.min(self.carry.len());
        (0..=max)
            .rev()
            .find(|&len| {
                let suffix = &self.carry[self.carry.len() - len..];
                self.values.iter().any(|value| value.starts_with(suffix))
            })
            .unwrap_or(0)
    }

    /// The redacted remainder to emit when the stream completes.
    fn finish(&mut self) -> String {
        let remainder = redact(&self.carry, &self.values);
        self.carry.clear();
        remainder
    }

    fn emit_to_stdout(text: &str) {
        if text.is_empty() {
            return;
        }
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
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
) -> (agent_runtime::Result<ModelTurn>, bool) {
    let mut emitted = false;
    let mut redactor = StreamRedactor::new(redactions);
    let mut relay = |event: ModelStreamEvent| -> agent_runtime::Result<()> {
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
fn complete_turn<T: ModelTransport>(
    make_transport: impl Fn() -> Result<T>,
    request: &ModelRequest,
    cancellation: &CancellationToken,
    budget: &TurnBudget,
    stream_output: bool,
    redactions: &[String],
    task_id: &str,
) -> Result<ModelTurn> {
    let mut attempt = 1u32;
    loop {
        if budget.remaining().is_zero() {
            return Err(KvistError::AgentRuntime(ModelError::ModelTransportTimedOut));
        }
        let transport = make_transport()?;
        let (outcome, emitted) = if stream_output {
            run_stream_attempt(&transport, request, cancellation, redactions)
        } else {
            (transport.complete(request, cancellation), false)
        };
        match outcome {
            Ok(turn) => return Ok(turn),
            Err(source)
                if is_retryable_transport_error(&source)
                    && attempt < MODEL_TURN_MAX_ATTEMPTS
                    && !emitted =>
            {
                tracing::warn!(
                    task_id,
                    attempt,
                    %source,
                    "local model gateway unavailable during turn; retrying"
                );
                thread::sleep(MODEL_TURN_RETRY_BACKOFF.min(budget.remaining()));
                attempt += 1;
            }
            Err(source) => return Err(KvistError::AgentRuntime(source)),
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
fn execute_host_turn(
    choice: &ModelChoice,
    profile: &AgentProfile,
    request: &AgentExecutionRequest<'_>,
    budget: &TurnBudget,
    redactions: &[String],
    messages: &[ModelMessage],
    cancellation: &CancellationToken,
) -> Result<ModelTurn> {
    if choice.is_none {
        return Err(KvistError::InvalidModelSelection {
            model_name: choice.model.clone(),
            role: profile.role,
            available: "a model that performs work".to_owned(),
        });
    }
    let provider = provider_from_command(&choice.command);
    let endpoint = extract_loopback_endpoint(&choice.command).map_err(|source| {
        KvistError::AgentCommandNotModelGateway {
            model_name: choice.model.clone(),
            reason: source.to_string(),
        }
    })?;

    // Liveness pre-check: fail fast with an actionable message when the local
    // model gateway is down, instead of surfacing a cryptic transport error
    // inside a turn. A TCP connect only confirms the port; it never loads or
    // shifts a model slot.
    probe_gateway_reachable(&endpoint, budget)?;

    // llama-server/ollama gateways route on the provider-facing model identifier,
    // which the command embeds; the selector alone is not a model the gateway
    // recognizes (it returns 400). Fall back to the selector when the command
    // carries no such identifier.
    let provider_model =
        extract_provider_model(&choice.command).unwrap_or_else(|| choice.model.clone());
    let model_request = ModelRequest {
        model: provider_model,
        messages: messages.to_vec(),
        tools: authoring::authoring_tool_definitions(),
        tool_choice: ToolChoice::Auto,
        reasoning_effort: profile.thinking_effort,
        output_schema: None,
    };

    complete_turn(
        || {
            DirectModelTransport::new(
                provider,
                &endpoint,
                budget.remaining(),
                profile.max_output_bytes,
            )
            .map_err(KvistError::AgentRuntime)
        },
        &model_request,
        cancellation,
        budget,
        request.stream_output,
        redactions,
        request.task_id,
    )
}

/// The approved, capability-confirmed sandbox context for effect application.
struct EffectSandbox<'a> {
    config: &'a SandboxConfig,
    expected_runner: &'a crate::sandbox::RunnerIdentity,
    probe: &'a crate::sandbox::SandboxProbe,
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
    mounts.push(crate::sandbox::ReadOnlyMount {
        source: staged_path.to_path_buf(),
        destination: STAGED_INTENT_MOUNT_PATH.to_owned(),
    });
    let execution_request = crate::sandbox::ExecutionRequest {
        project_root: request.project_root,
        vcs_selection: request.vcs_selection,
        component_dir: request.target_dir,
        phase: crate::sandbox::ExecutionPhase::Authoring,
        program,
        arguments,
        environment: crate::sandbox::allowed_environment(sandbox.config, None),
        read_only_mounts: &mounts,
        backend: &sandbox.probe.backend,
        policy_identity: request.policy_identity,
    };
    crate::sandbox::execute_with_timeout(
        sandbox.config,
        execution_request,
        crate::sandbox::ExecutionOptions {
            timeout: Some(Duration::from_secs(profile.timeout_seconds)),
            output_limit: Some(profile.max_output_bytes),
            live_stdout: None,
        },
        sandbox.expected_runner,
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
fn apply_authoring_effects(
    profile: &AgentProfile,
    sandbox: &EffectSandbox<'_>,
    request: &AgentExecutionRequest<'_>,
    plan: &AuthoringPlan,
    turn: &ModelTurn,
    session_id: &str,
) -> Result<Vec<EffectRecord>> {
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

    let mut records = Vec::with_capacity(plan.effects.len());
    for effect in &plan.effects {
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
        let result = match dispatch_effect(
            profile,
            sandbox,
            request,
            &program,
            &arguments,
            &staged_path,
        ) {
            Ok(result) => result,
            Err(source) => {
                // Infrastructure failure: remove the staged file, then
                // propagate so the caller fences the attempt.
                let _ = fs::remove_file(&staged_path);
                return Err(source);
            }
        };
        // The staged file is consumed; remove it so no residue remains under
        // the component state directory.
        let _ = fs::remove_file(&staged_path);
        if result.cancelled {
            return Err(KvistError::AgentRuntime(agent_runtime::Error::Cancelled));
        }
        records.push(summarize_effect(effect, result));
    }
    Ok(records)
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
        if result.timed_out {
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
        truncate_utf8(&mut text, 4096);
        text
    };
    EffectRecord {
        call_id: effect.call_id.clone(),
        tool: effect.tool.clone(),
        destination: effect.destination.clone(),
        applied,
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
            KvistError::AgentRuntime(model_error) => match model_error {
                agent_runtime::Error::ModelTransportTimedOut
                | agent_runtime::Error::SlotAllocationTimedOut { .. }
                | agent_runtime::Error::TtftTimedOut { .. }
                | agent_runtime::Error::InterTokenCadenceTimedOut { .. } => (true, false, false),
                agent_runtime::Error::ModelResponseLimitExceeded { .. } => (false, true, false),
                agent_runtime::Error::ModelTransportCancelled | agent_runtime::Error::Cancelled => {
                    (false, false, true)
                }
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
    let choice = select_model_choice(profile, request.role, None)?;

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

    let redactions = redaction_values(profile, sandbox_config);
    let budget = TurnBudget::new(Duration::from_secs(profile.timeout_seconds));

    // One cancellation token spans the whole run so Ctrl+C aborts every turn,
    // not just the one in flight.
    let cancellation = CancellationToken::new();

    // Rolling conversation: the system prompt plus task prompt seed the model;
    // each turn appends its own assistant message and any tool results, so the
    // model sees the full history as it iterates toward a final answer.
    let mut messages: Vec<ModelMessage> = Vec::new();
    if let Some(system) = choice
        .system_prompt
        .as_ref()
        .filter(|text| !text.trim().is_empty())
    {
        messages.push(ModelMessage::System(system.clone()));
    }
    messages.push(ModelMessage::User(request.prompt.to_owned()));

    let mut run_turns: Vec<RunTurn> = Vec::new();

    loop {
        if run_turns.len() >= MAX_AGENT_TURNS {
            let reason = format!(
                "agent did not complete within the {}-turn bound",
                MAX_AGENT_TURNS
            );
            tracing::warn!(
                task_id = %request.task_id,
                %reason,
                max_turns = MAX_AGENT_TURNS,
                "agent exceeded the multi-turn bound"
            );
            let failure = TurnFailure {
                reason,
                timed_out: false,
                output_limit_exceeded: false,
                cancelled: false,
            };
            return record_turn_outcome(
                profile,
                &request,
                &session_id,
                &log_path,
                &mut log_file,
                &redactions,
                &run_turns,
                Some(&failure),
            );
        }

        // 1. The model turn runs on the host: numeric loopback gateway only,
        //    liveness-probed, transient availability failures retried in the
        //    shared budget. Streamed deltas are redacted with the same values
        //    as the durable log.
        let turn = match execute_host_turn(
            &choice,
            profile,
            &request,
            &budget,
            &redactions,
            &messages,
            &cancellation,
        ) {
            Ok(turn) => turn,
            Err(source) => {
                tracing::warn!(
                    task_id = %request.task_id,
                    %source,
                    "agent model turn failed"
                );
                let failure = TurnFailure::from_kvist_error(&source);
                let result = record_turn_outcome(
                    profile,
                    &request,
                    &session_id,
                    &log_path,
                    &mut log_file,
                    &redactions,
                    &run_turns,
                    Some(&failure),
                )?;
                if failure.cancelled {
                    return Err(KvistError::AgentRuntime(agent_runtime::Error::Cancelled));
                }
                return Ok(result);
            }
        };

        // Feed the assistant message back so the model observes its own output.
        messages.push(ModelMessage::Assistant {
            text: turn.text.clone(),
            tool_intents: turn.tool_intents.clone(),
        });

        // 2. Broker: reduce untrusted intents to capability-bound effects.
        let plan = authoring::authorize_turn(request.target_dir, &turn, &BrokerPolicy::default());

        // 3. A dropped intent fails the turn before any effect runs: partial
        //    application would misrepresent what the model requested. The model
        //    is told what was refused so it can pursue an authorized path.
        if !plan.dropped.is_empty() {
            let reason = drop_reason(&plan.dropped);
            tracing::warn!(task_id = %request.task_id, %reason);
            run_turns.push(RunTurn {
                turn,
                effects: Vec::new(),
                drops: plan.dropped,
            });
            return record_turn_outcome(
                profile,
                &request,
                &session_id,
                &log_path,
                &mut log_file,
                &redactions,
                &run_turns,
                Some(&TurnFailure {
                    reason,
                    timed_out: false,
                    output_limit_exceeded: false,
                    cancelled: false,
                }),
            );
        }

        // No intents at all: the model produced a final answer rather than
        // another action. Complete the run.
        if plan.effects.is_empty() {
            run_turns.push(RunTurn {
                turn,
                effects: Vec::new(),
                drops: Vec::new(),
            });
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
            run_turns.push(RunTurn {
                turn,
                effects: Vec::new(),
                drops: Vec::new(),
            });
            return record_turn_outcome(
                profile,
                &request,
                &session_id,
                &log_path,
                &mut log_file,
                &redactions,
                &run_turns,
                Some(&TurnFailure {
                    reason,
                    timed_out: false,
                    output_limit_exceeded: false,
                    cancelled: false,
                }),
            );
        };

        let sandbox_context = EffectSandbox {
            config: sandbox_config,
            expected_runner,
            probe,
        };
        let effects = apply_authoring_effects(
            profile,
            &sandbox_context,
            &request,
            &plan,
            &turn,
            &session_id,
        )?;

        // Feed each effect's outcome back to the model as a tool result so it
        // can react to a failed or skipped write on a later turn.
        for record in &effects {
            let content = if record.applied {
                "applied".to_owned()
            } else {
                format!("did not apply: {}", record.detail)
            };
            messages.push(ModelMessage::ToolResult {
                call_id: record.call_id.clone(),
                name: record.tool.clone(),
                content,
            });
        }

        // An unapplied effect fails the turn: partial application would
        // misrepresent what the model requested.
        if effects.iter().any(|record| !record.applied) {
            let reason = apply_reason(&effects);
            tracing::warn!(task_id = %request.task_id, %reason);
            run_turns.push(RunTurn {
                turn,
                effects,
                drops: Vec::new(),
            });
            return record_turn_outcome(
                profile,
                &request,
                &session_id,
                &log_path,
                &mut log_file,
                &redactions,
                &run_turns,
                Some(&TurnFailure {
                    reason,
                    timed_out: false,
                    output_limit_exceeded: false,
                    cancelled: false,
                }),
            );
        }

        run_turns.push(RunTurn {
            turn,
            effects,
            drops: Vec::new(),
        });
    }

    let result = record_turn_outcome(
        profile,
        &request,
        &session_id,
        &log_path,
        &mut log_file,
        &redactions,
        &run_turns,
        None,
    )?;

    if result.success {
        tracing::info!(
            task_id = %request.task_id,
            tokens_input = ?result.tokens_input,
            tokens_output = ?result.tokens_output,
            total_turns = run_turns.len(),
            log_path = %result.log_path.display(),
            "agent execution completed successfully"
        );
    } else if result.timed_out {
        tracing::warn!(
            task_id = %request.task_id,
            log_path = %result.log_path.display(),
            "agent execution timed out"
        );
    } else if result.output_limit_exceeded {
        tracing::warn!(
            task_id = %request.task_id,
            log_path = %result.log_path.display(),
            "agent execution exceeded output limit"
        );
    } else {
        tracing::warn!(
            task_id = %request.task_id,
            log_path = %result.log_path.display(),
            "agent model turn failed"
        );
    }

    Ok(result)
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
    let last_turn = run_turns.last().map(|run_turn| run_turn.turn.clone());

    let turn_ok = last_turn
        .as_ref()
        .is_some_and(|turn| !turn.text.trim().is_empty() || !turn.tool_intents.is_empty());
    let success = last_turn.is_some()
        && turn_ok
        && failure.is_none()
        && all_dropped.is_empty()
        && all_effects.iter().all(|record| record.applied);

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

    let mut log = String::new();
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

    let usage = last_turn.and_then(|turn| turn.usage);
    let tokens_input = usage.map(|usage| usage.input_tokens as usize);
    let tokens_output = usage.map(|usage| usage.output_tokens as usize);

    // Record the structured session trajectory journal: one dispatch/result
    // pair per applied-or-failed effect across every turn, so `state_mutated`
    // reflects effects that actually ran rather than the overall turn outcome.
    let runs_dir = request.target_dir.join(".kvist").join("runs");
    let trajectory_path = runs_dir.join(format!("{session_id}.trajectory.jsonl"));
    let recorder = agent_runtime::TrajectoryRecorder::new(&trajectory_path);
    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::SessionStart {
        session_id: session_id.to_owned(),
        task_id: request.task_id.to_string(),
        timestamp: now_ts,
    });

    let mut total_tokens: u64 = 0;
    for (index, run_turn) in run_turns.iter().enumerate() {
        let turn = &run_turn.turn;
        let turn_number = index + 1;
        let usage = turn.usage;
        let new_tokens = usage.map(|usage| usage.input_tokens);
        let output_tokens = usage.map(|usage| usage.output_tokens);
        total_tokens += new_tokens.unwrap_or(0) + output_tokens.unwrap_or(0);

        let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::TurnStart {
            turn: turn_number,
            timestamp: now_ts,
        });
        let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::PromptEval {
            turn: turn_number,
            cached_tokens: None,
            new_tokens,
            eval_duration_ms: None,
        });
        for record in &run_turn.effects {
            let args = serde_json::json!({ "destination": record.destination });
            let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::ToolDispatch {
                turn: turn_number,
                call_id: record.call_id.clone(),
                tool: record.tool.clone(),
                args: args.clone(),
                action_hash: agent_runtime::compute_action_hash(&record.tool, &args),
            });
            let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::ToolResult {
                turn: turn_number,
                call_id: record.call_id.clone(),
                tool: record.tool.clone(),
                stdout: if record.applied {
                    "applied".to_owned()
                } else {
                    String::new()
                },
                stderr: redact_bounded(&record.detail, redactions, profile.max_output_bytes),
                exit_code: if record.applied { 0 } else { 1 },
                bytes: record.detail.len(),
                state_mutated: record.applied,
            });
        }
        let finish_reason = serde_json::to_value(&turn.finish_reason)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "stop".to_owned());
        let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::TurnFinish {
            turn: turn_number,
            output_tokens,
            finish_reason,
        });
    }

    let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::SessionFinish {
        session_id: session_id.to_owned(),
        task_id: request.task_id.to_string(),
        total_turns: run_turns.len(),
        total_tokens,
        success,
    });

    Ok(AgentRunResult {
        success,
        tokens_input,
        tokens_output,
        log_path: log_path.to_path_buf(),
        stdout,
        stderr,
        timed_out,
        output_limit_exceeded,
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
        let error =
            probe_gateway_reachable(&endpoint, &budget).expect_err("gateway should be unreachable");
        match error {
            KvistError::LocalModelGatewayUnreachable {
                endpoint: returned_endpoint,
                ..
            } => assert_eq!(returned_endpoint, endpoint),
            other => panic!("unexpected error kind: {other:?}"),
        }
    }

    #[test]
    fn probe_gateway_reachable_passes_when_a_listener_accepts() {
        // Hold a listener open for the duration of the probe so the connect
        // succeeds; dropping it at function end releases the port.
        let _listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free port");
        let endpoint = format!("http://{}", _listener.local_addr().expect("local addr"));
        let budget = TurnBudget::new(Duration::from_secs(300));
        probe_gateway_reachable(&endpoint, &budget).expect("gateway should be reachable");
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
        let error =
            probe_gateway_reachable(&endpoint, &budget).expect_err("budget must be exhausted");
        assert!(
            matches!(
                error,
                KvistError::AgentRuntime(ModelError::ModelTransportTimedOut)
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
                finish_reason: agent_runtime::FinishReason::Stop,
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
        ) -> agent_runtime::Result<ModelTurn> {
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
            on_event: &mut dyn FnMut(ModelStreamEvent) -> agent_runtime::Result<()>,
        ) -> agent_runtime::Result<ModelTurn> {
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
                KvistError::AgentRuntime(ModelError::ModelProviderStatus { status: 500 })
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
            matches!(
                error,
                KvistError::AgentRuntime(ModelError::ModelTransportIo { .. })
            ),
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
            KvistError::AgentRuntime(ModelError::ModelTransportIo { .. })
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
            matches!(
                error,
                KvistError::AgentRuntime(ModelError::ModelTransportTimedOut)
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(transport.attempt_count(), 0);
    }

    fn test_execution_request<'a>(target: &'a Path) -> AgentExecutionRequest<'a> {
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

    fn test_profile(command: &str) -> AgentProfile {
        AgentProfile {
            role: crate::config::Role::Developer,
            profile: "default".to_owned(),
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
        let target = std::env::temp_dir();
        let request = test_execution_request(&target);
        let profile = test_profile("claude --non-interactive --message '{prompt}'");
        let choice = ModelChoice {
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
            &[],
            &agent_runtime::CancellationToken::new(),
        )
        .expect_err("a non-loopback command must be refused");
        assert!(
            matches!(error, KvistError::AgentCommandNotModelGateway { .. }),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn none_model_is_refused_without_reaching_the_gateway() {
        let target = std::env::temp_dir();
        let request = test_execution_request(&target);
        let profile = test_profile("curl -- 'http://127.0.0.1:1/api/chat'");
        let choice = ModelChoice {
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
            &[],
            &agent_runtime::CancellationToken::new(),
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
        let timeout = TurnFailure::from_kvist_error(&KvistError::AgentRuntime(
            ModelError::ModelTransportTimedOut,
        ));
        assert!(timeout.timed_out);
        assert!(!timeout.cancelled);

        let cancelled = TurnFailure::from_kvist_error(&KvistError::AgentRuntime(
            ModelError::ModelTransportCancelled,
        ));
        assert!(cancelled.cancelled);
        assert!(!cancelled.timed_out);

        let oversized = TurnFailure::from_kvist_error(&KvistError::AgentRuntime(
            ModelError::ModelResponseLimitExceeded { max_bytes: 1 },
        ));
        assert!(oversized.output_limit_exceeded);

        let other = TurnFailure::from_kvist_error(&KvistError::AgentRuntime(
            ModelError::ModelProviderStatus { status: 500 },
        ));
        assert!(!other.timed_out && !other.output_limit_exceeded && !other.cancelled);
        assert!(other.reason.contains("500"));
    }
}
