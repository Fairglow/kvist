//! External agent execution and response capture.

use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use nix::sys::signal::Signal;
use nix::unistd::Pid;
use serde::Deserialize;

use crate::{
    KvistError, Result,
    config::{AgentProfile, SandboxConfig, VcsSelection},
    sandbox,
    task_queue::Timestamp,
};

use agent_runtime::{
    CancellationToken, DirectModelTransport, Error as ModelError, LocalModelProvider, ModelMessage,
    ModelRequest, ModelTransport, ModelTurn, ToolChoice,
};

/// Structured execution result of an external agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunResult {
    /// True if the agent succeeded and exited with status 0.
    pub success: bool,
    /// Number of prompt/input tokens used, if reported.
    pub tokens_input: Option<usize>,
    /// Number of output/completion tokens used, if reported.
    pub tokens_output: Option<usize>,
    /// Path to the raw execution log file.
    pub log_path: PathBuf,
    /// Bounded redacted combined output retained as execution evidence.
    pub stdout: String,
    /// Empty because evidence is normalized into the combined output field.
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

/// Run-record schema parsed from the agent's run metadata file.
#[derive(Debug, Deserialize)]
struct RunRecord {
    #[allow(dead_code)]
    status: String,
    tokens_input: Option<usize>,
    tokens_output: Option<usize>,
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

/// Brief connect attempts made while proving the gateway is accepting traffic.
/// A down gateway fails on the first attempt; these ride out a gateway that is
/// still starting its listeners.
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

/// Proves a loopback model gateway is accepting connections before a turn runs.
///
/// `endpoint` is a numeric loopback URL such as `http://127.0.0.1:9931`. This
/// performs only a TCP connect; it never issues an HTTP request, so it cannot
/// load, select, or shift a model slot. It is a pure liveness check that lets a
/// down gateway fail fast with an actionable message instead of a cryptic
/// transport error buried deep inside a turn.
fn probe_gateway_reachable(endpoint: &str) -> Result<()> {
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
        match std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(_) => return Ok(()),
            Err(source) if attempt < GATEWAY_PROBE_ATTEMPTS => {
                tracing::warn!(
                    attempt,
                    %source,
                    endpoint,
                    "local model gateway not yet accepting connections; retrying liveness probe"
                );
                thread::sleep(GATEWAY_PROBE_BACKOFF);
                attempt += 1;
                continue;
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
/// availability problems (nothing is listening yet, or the port is still
/// starting up). Response-level failures (bad HTTP status, malformed body,
/// oversized response, cancellation) are never retried, since they indicate a
/// real answer rather than an unavailable gateway.
fn is_retryable_transport_error(error: &ModelError) -> bool {
    match error {
        ModelError::ModelTransportTimedOut => true,
        ModelError::ModelTransportIo { source, .. } => {
            matches!(
                source.kind(),
                io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::TimedOut
                    | io::ErrorKind::Interrupted
            )
        }
        _ => false,
    }
}

/// Performs one model turn, retrying transient gateway-availability failures up
/// to `MODEL_TURN_MAX_ATTEMPTS` times. Real response failures are returned
/// immediately so they are never masked by a retry.
fn complete_turn_with_retries(
    transport: &DirectModelTransport,
    request: &ModelRequest,
    cancellation: &CancellationToken,
    task_id: &str,
) -> Result<ModelTurn> {
    let mut attempt = 1u32;
    loop {
        match transport.complete(request, cancellation) {
            Ok(turn) => return Ok(turn),
            Err(source)
                if is_retryable_transport_error(&source) && attempt < MODEL_TURN_MAX_ATTEMPTS =>
            {
                tracing::warn!(
                    task_id,
                    attempt,
                    %source,
                    "local model gateway unreachable during turn; retrying"
                );
                thread::sleep(MODEL_TURN_RETRY_BACKOFF);
                attempt += 1;
                continue;
            }
            Err(source) => return Err(source.into()),
        }
    }
}

/// Performs one model turn on the host, outside the effect sandbox. The transport
/// is constructed from the resolved command template; the turn text is returned
/// for the caller to record. A failure returns `Err` and writes nothing on
/// success. The local model gateway is liveness-probed first, and transient
/// availability failures are retried, so a down or cold-starting gateway yields
/// a fast, actionable error rather than a cryptic transport failure.
fn execute_host_turn(
    choice: &ModelChoice,
    prompt: &str,
    deadline: Duration,
    max_response_bytes: usize,
    task_id: &str,
) -> Result<String> {
    if choice.is_none {
        return Err(KvistError::InvalidModelSelection {
            model_name: choice.model.clone(),
            role: crate::config::Role::Developer,
            available: "a model that performs work".to_owned(),
        });
    }
    let provider = provider_from_command(&choice.command);
    let endpoint = extract_loopback_endpoint(&choice.command)?;

    // Liveness pre-check: fail fast with an actionable message when the local
    // model gateway is down, instead of surfacing a cryptic transport error
    // inside a turn. A TCP connect only confirms the port; it never loads or
    // shifts a model slot.
    probe_gateway_reachable(&endpoint)?;

    let transport = DirectModelTransport::new(provider, &endpoint, deadline, max_response_bytes)?;
    let cancellation = CancellationToken::new();
    let mut messages = Vec::new();
    if let Some(system) = choice
        .system_prompt
        .as_ref()
        .filter(|text| !text.trim().is_empty())
    {
        messages.push(ModelMessage::System(system.clone()));
    }
    messages.push(ModelMessage::User(prompt.to_owned()));
    let request = ModelRequest {
        model: choice.model.clone(),
        messages,
        tools: Vec::new(),
        tool_choice: ToolChoice::None,
        reasoning_effort: None,
        output_schema: None,
    };
    let turn = complete_turn_with_retries(&transport, &request, &cancellation, task_id)?;
    Ok(turn.text)
}

/// Bounded result of one host-side model turn.
///
/// A turn is successful only when it neither timed out, exceeded the output
/// budget, was cancelled, nor failed to exit cleanly.
struct HostTurn {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    success: bool,
    timed_out: bool,
    output_limit_exceeded: bool,
    cancelled: bool,
}

impl HostTurn {
    fn failure() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            success: false,
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }
    }

    fn from_text(text: String) -> Self {
        // A turn that produced no text is treated as a failure so a silent
        // model service never masquerades as a successful authoring turn.
        let empty = text.trim().is_empty();
        Self {
            stdout: text.into_bytes(),
            stderr: Vec::new(),
            success: !empty,
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }
    }
}

/// Bounded result of one authoring sandbox request.
struct AuthoringTurn {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    success: bool,
    timed_out: bool,
    output_limit_exceeded: bool,
    cancelled: bool,
}

impl AuthoringTurn {
    /// No authoring was required. A loopback turn is a pure network call to the
    /// provider and never issues an authoring sandbox request, so this is
    /// treated as satisfied and the combined result follows the host turn.
    fn not_required() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            success: true,
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }
    }

    fn from_result(result: crate::sandbox::ExecutionResult) -> Self {
        let success = !result.timed_out
            && !result.output_limit_exceeded
            && !result.cancelled
            && result.output.status.success();
        Self {
            stdout: result.output.stdout,
            stderr: result.output.stderr,
            success,
            timed_out: result.timed_out,
            output_limit_exceeded: result.output_limit_exceeded,
            cancelled: result.cancelled,
        }
    }

    /// An authoring request could not be dispatched (sandbox error or an
    /// unapproved backend). The turn is treated as failed so the combined
    /// result transitions the task rather than succeeding blindly.
    fn failure() -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            success: false,
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }
    }
}

/// Runs a rendered host command as a bounded, supervised subprocess.
///
/// This is host supervision, not a security boundary: the sandbox provides the
/// authorization and effect boundary. The capture stays bounded and the
/// process group is terminated on the deadline so a hung or runaway host
/// command can never block task execution indefinitely.
fn run_host_subprocess(
    program: &str,
    arguments: &[String],
    timeout: Duration,
    max_output_bytes: usize,
) -> HostTurn {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(source) => {
            tracing::warn!(program, %source, "failed to spawn host agent subprocess");
            return HostTurn::failure();
        }
    };

    let stdout_pipe = child.stdout.take().expect("stdout pipe is piped");
    let stderr_pipe = child.stderr.take().expect("stderr pipe is piped");

    let stdout_cap = StreamCapture::new(max_output_bytes);
    let stderr_cap = StreamCapture::new(max_output_bytes);
    let reader_stdout = stdout_cap.clone();
    let reader_stderr = stderr_cap.clone();
    let stdout_thread = thread::spawn(move || reader_stdout.feed(stdout_pipe));
    let stderr_thread = thread::spawn(move || reader_stderr.feed(stderr_pipe));

    let start = Instant::now();
    let mut timed_out = false;
    let mut status = None;
    loop {
        if let Ok(Some(exit)) = child.try_wait() {
            status = Some(exit);
            break;
        }
        if start.elapsed() >= timeout {
            timed_out = true;
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }

    // When the deadline fired the child is still running; terminate and reap it.
    let status = match status {
        Some(exit) => exit,
        None => reap_terminated(&mut child),
    };

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    // A timed-out turn is a failure regardless of the reap status.
    let success = !timed_out && status.success();
    HostTurn {
        stdout: stdout_cap.take_bytes(),
        stderr: stderr_cap.take_bytes(),
        success,
        timed_out,
        output_limit_exceeded: stdout_cap.exceeded() || stderr_cap.exceeded(),
        cancelled: false,
    }
}

/// Reaps a child that has ignored the deadline: SIGTERM its process group, then
/// force with SIGKILL so a host command that spawned descendants is fully
/// reaped and never left orphaned. Always returns the child's final status.
fn reap_terminated(child: &mut Child) -> ExitStatus {
    let pid = Pid::from_raw(child.id() as i32);
    let _ = nix::sys::signal::killpg(pid, Signal::SIGTERM);
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) | Err(_) => {
                if start.elapsed() < Duration::from_millis(500) {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                // Grace period exhausted: force the process group to die, then
                // keep reaping until the kernel reports the final status.
                let _ = child.kill();
            }
        }
    }
}

/// A bounded per-stream capture that always drains its source but retains at
/// most `cap` bytes, flagging overflow when the source exceeds the budget.
#[derive(Clone)]
struct StreamCapture {
    inner: Arc<Mutex<StreamState>>,
    cap: usize,
}

struct StreamState {
    data: Vec<u8>,
    exceeded: bool,
}

impl StreamCapture {
    fn new(cap: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StreamState {
                data: Vec::with_capacity(cap.min(4096)),
                exceeded: false,
            })),
            cap,
        }
    }

    fn feed<R: Read>(&self, mut reader: R) {
        let mut chunk = [0u8; 8192];
        loop {
            let n = match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            let mut state = match self.inner.lock() {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };
            if state.data.len() < self.cap {
                let take = std::cmp::min(n, self.cap - state.data.len());
                state.data.extend_from_slice(&chunk[..take]);
                if take < n {
                    state.exceeded = true;
                }
            } else if n > 0 {
                state.exceeded = true;
            }
        }
    }

    fn take_bytes(&self) -> Vec<u8> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut state.data)
    }

    fn exceeded(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exceeded
    }
}

/// Builds and dispatches the authoring sandbox request for a successful host
/// turn of a non-loopback command. Only explicitly authorized effects reach the
/// sandbox; the model turn itself ran on the host and produced no mutation.
fn dispatch_authoring(
    profile: &AgentProfile,
    sandbox_config: &SandboxConfig,
    expected_runner: &crate::sandbox::RunnerIdentity,
    probe: &crate::sandbox::SandboxProbe,
    request: &AgentExecutionRequest<'_>,
    program: &str,
    arguments: &[String],
) -> Result<crate::sandbox::ExecutionResult> {
    let execution_request = crate::sandbox::ExecutionRequest {
        project_root: request.project_root,
        vcs_selection: request.vcs_selection,
        component_dir: request.target_dir,
        phase: crate::sandbox::ExecutionPhase::Authoring,
        program,
        arguments,
        environment: crate::sandbox::allowed_environment(sandbox_config, None),
        read_only_mounts: request.read_only_mounts,
        backend: &probe.backend,
        policy_identity: request.policy_identity,
    };
    crate::sandbox::execute_with_timeout(
        sandbox_config,
        execution_request,
        crate::sandbox::ExecutionOptions {
            timeout: Some(Duration::from_secs(profile.timeout_seconds)),
            output_limit: Some(profile.max_output_bytes),
            live_stdout: None,
        },
        expected_runner,
    )
}

/// Spawns the subprocess, redirects output to log file, and optionally streams to console.
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
    let log_file_name = format!(
        "{}_{}.log",
        request.task_id,
        timestamp.to_string().replace(':', "-")
    );
    let log_path = logs_dir.join(log_file_name);

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

    // The model turn runs on the host, outside the effect sandbox. A command
    // that targets a numeric loopback endpoint is served by the direct model
    // transport; any other command runs as a bounded host subprocess. A
    // loopback turn is a pure network call to the provider and therefore never
    // issues an authoring sandbox request.
    let redactions = redaction_values(profile, sandbox_config);
    let deadline = Duration::from_secs(profile.timeout_seconds);
    let loopback = extract_loopback_endpoint(&choice.command).is_ok();

    let host_turn: HostTurn = if loopback {
        match execute_host_turn(
            &choice,
            request.prompt,
            deadline,
            profile.max_output_bytes,
            request.task_id,
        ) {
            Ok(text) => HostTurn::from_text(text),
            Err(source) => {
                tracing::warn!(
                    task_id = %request.task_id,
                    %source,
                    "agent model turn failed"
                );
                HostTurn::failure()
            }
        }
    } else {
        match get_effective_command(
            profile,
            request.role,
            request.prompt,
            request.context_paths,
            request.target_dir,
        ) {
            Ok((program, arguments)) => {
                run_host_subprocess(&program, &arguments, deadline, profile.max_output_bytes)
            }
            Err(source) => {
                tracing::warn!(
                    task_id = %request.task_id,
                    %source,
                    "cannot render host agent command"
                );
                HostTurn::failure()
            }
        }
    };

    // A successful non-loopback turn yields model intent that the broker
    // authorizes and executes as a sandboxed request. The host turn alone never
    // mutates component state; only the bounded, authorized effect does. This is
    // the authoring-effect loop: intent is authorized on the host, effects are
    // applied only through the sandbox.
    let auth_turn: AuthoringTurn = if loopback || !host_turn.success {
        AuthoringTurn::not_required()
    } else if let (Some(expected_runner), Some(probe)) = (expected_runner, probe) {
        // A sandbox infrastructure failure (e.g. the runner changed after the
        // probe, or the backend is unavailable) is fatal: fail closed and let
        // the caller decide. Only a command that the sandbox ran but that
        // exited, timed out, or overflowed is a soft blocked transition.
        let (program, arguments) = get_effective_command(
            profile,
            request.role,
            request.prompt,
            request.context_paths,
            request.target_dir,
        )
        .map_err(|source| {
            tracing::warn!(
                task_id = %request.task_id,
                %source,
                "cannot render authoring command"
            );
            source
        })?;
        let result = dispatch_authoring(
            profile,
            sandbox_config,
            expected_runner,
            probe,
            &request,
            &program,
            &arguments,
        )
        .map_err(|source| {
            tracing::warn!(
                task_id = %request.task_id,
                %source,
                "authoring sandbox request failed"
            );
            source
        })?;
        AuthoringTurn::from_result(result)
    } else {
        AuthoringTurn::failure()
    };

    // Combine the host turn and the authoring effect into one bounded,
    // redacted evidence buffer. A failure or overflow on either side fails the
    // combined turn; the sandbox is the authority for effect-level bounds.
    let mut combined_stdout = host_turn.stdout;
    combined_stdout.extend_from_slice(&auth_turn.stdout);
    let mut combined_stderr = host_turn.stderr;
    combined_stderr.extend_from_slice(&auth_turn.stderr);
    let stdout = redact_combined_output(
        combined_stdout,
        combined_stderr,
        &redactions,
        profile.max_output_bytes,
    );
    let cancelled = host_turn.cancelled || auth_turn.cancelled;
    let timed_out = host_turn.timed_out || auth_turn.timed_out;
    let output_limit_exceeded = host_turn.output_limit_exceeded || auth_turn.output_limit_exceeded;
    let success = host_turn.success && auth_turn.success;
    log_file
        .write_all(stdout.as_bytes())
        .map_err(|source| KvistError::Io {
            operation: "write agent log",
            path: log_path.clone(),
            source,
        })?;

    // 4. Try parsing the JSON Run Record for token feedback
    // The run record should be written by the agent at .kvist/runs/<task_id>_<timestamp>.json
    let runs_dir = request.target_dir.join(".kvist").join("runs");
    let record_name = format!(
        "{}_{}.json",
        request.task_id,
        timestamp.to_string().replace(':', "-")
    );
    let record_path = runs_dir.join(record_name);

    let mut tokens_input = None;
    let mut tokens_output = None;

    if record_path.exists()
        && let Ok(contents) = fs::read_to_string(&record_path)
        && let Ok(record) = serde_json::from_str::<RunRecord>(&contents)
    {
        tokens_input = record.tokens_input;
        tokens_output = record.tokens_output;
    }

    // Record structured session trajectory journal
    let trajectory_path = runs_dir.join(format!(
        "{}_{}.trajectory.jsonl",
        request.task_id,
        timestamp.to_string().replace(':', "-")
    ));
    let recorder = agent_runtime::TrajectoryRecorder::new(&trajectory_path);
    let session_id = format!(
        "{}_{}",
        request.task_id,
        timestamp.to_string().replace(':', "-")
    );
    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::SessionStart {
        session_id: session_id.clone(),
        task_id: request.task_id.to_string(),
        timestamp: now_ts,
    });
    let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::TurnStart {
        turn: 1,
        timestamp: now_ts,
    });
    let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::PromptEval {
        turn: 1,
        cached_tokens: None,
        new_tokens: tokens_input.map(|t| t as u64),
        eval_duration_ms: None,
    });
    let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::ToolDispatch {
        turn: 1,
        call_id: "call_1".to_owned(),
        tool: choice.model.clone(),
        args: serde_json::json!({
            "prompt": request.prompt,
            "target_dir": request.target_dir.display().to_string(),
        }),
        action_hash: agent_runtime::compute_action_hash(
            &choice.model,
            &serde_json::json!({ "prompt": request.prompt }),
        ),
    });
    let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::ToolResult {
        turn: 1,
        call_id: "call_1".to_owned(),
        tool: choice.model.clone(),
        stdout: stdout.clone(),
        stderr: String::new(),
        exit_code: if success { 0 } else { 1 },
        bytes: stdout.len(),
        state_mutated: success,
    });
    let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::TurnFinish {
        turn: 1,
        output_tokens: tokens_output.map(|t| t as u64),
        finish_reason: if success {
            "stop".to_string()
        } else {
            "error".to_string()
        },
    });
    let _ = recorder.record_event(&agent_runtime::TrajectoryEvent::SessionFinish {
        session_id,
        task_id: request.task_id.to_string(),
        total_turns: 1,
        total_tokens: (tokens_input.unwrap_or(0) + tokens_output.unwrap_or(0)) as u64,
        success,
    });

    if cancelled {
        tracing::warn!(
            task_id = %request.task_id,
            log_path = %log_path.display(),
            "agent execution was interrupted before completion"
        );
        return Err(KvistError::AgentRuntime(agent_runtime::Error::Cancelled));
    }
    if success {
        tracing::info!(
            task_id = %request.task_id,
            tokens_input = ?tokens_input,
            tokens_output = ?tokens_output,
            log_path = %log_path.display(),
            "agent execution completed successfully"
        );
    } else if timed_out {
        tracing::warn!(
            task_id = %request.task_id,
            log_path = %log_path.display(),
            "agent execution timed out"
        );
    } else if output_limit_exceeded {
        tracing::warn!(
            task_id = %request.task_id,
            log_path = %log_path.display(),
            "agent execution exceeded output limit"
        );
    } else {
        tracing::warn!(
            task_id = %request.task_id,
            exit_code = if success { 0 } else { 1 },
            log_path = %log_path.display(),
            "agent model turn failed"
        );
    }

    Ok(AgentRunResult {
        success,
        tokens_input,
        tokens_output,
        log_path,
        stdout,
        stderr: String::new(),
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

fn redact_combined_output(
    mut stdout: Vec<u8>,
    stderr: Vec<u8>,
    redactions: &[String],
    limit: usize,
) -> String {
    stdout.extend_from_slice(&stderr);
    let mut text = String::from_utf8_lossy(&stdout).into_owned();
    for value in redactions {
        text = text.replace(value, "[REDACTED]");
    }
    truncate_utf8(&mut text, limit);
    text
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
        let error = probe_gateway_reachable(&endpoint).expect_err("gateway should be unreachable");
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
        probe_gateway_reachable(&endpoint).expect("gateway should be reachable");
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

        // Response-level failures must never be retried.
        assert!(!is_retryable_transport_error(
            &ModelError::ModelTransportCancelled
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
}
