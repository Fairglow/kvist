#[cfg(not(target_os = "linux"))]
compile_error!("sav currently supports Linux only");

use std::{
    io::{IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use sav::{
    CancellationToken, CatalogProvider, CommandSpec, DirectModelTransport, Error,
    LocalModelProvider, ModelDiscoveryOptions, ModelMessage, ModelRequest, ModelStreamEvent,
    ModelTransport, ReasoningEffort, SetupOptions, SupervisionPolicy, ToolChoice, discover_models,
    render_command_with_reasoning_effort, resolve_prompt, run_registry_setup, run_supervised,
    run_supervised_capture,
};

#[derive(Debug, Parser)]
#[command(
    name = "sav-run",
    version,
    about = "Run an AI prompt under bounded Linux host-process supervision"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Send a text-only request directly to local Ollama or llama-server.
    Model(ModelArguments),
    /// Discover provider-advertised model identifiers without running inference.
    Models(ModelsArguments),
    Run(RunArguments),
    /// Replay an agent execution session from a structured JSONL journal file.
    Replay(ReplayArguments),
    /// Interactively create or update a reusable provider profile.
    Setup(SetupArguments),
    /// Inspect configured shared profiles without discovery or qualification.
    Registry(RegistryArguments),
}

#[derive(Debug, Args)]
struct RegistryArguments {
    #[arg(long, value_name = "PATH")]
    registry: Option<PathBuf>,
    #[arg(long)]
    profile: Option<String>,
}

#[derive(Debug, Args)]
struct ReplayArguments {
    /// Path to the session JSONL journal file.
    #[arg(value_name = "SESSION_JSONL")]
    session_file: PathBuf,

    /// Maximum turn to step through (optional).
    #[arg(long)]
    max_turns: Option<usize>,

    /// Emit replay summary as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ModelsArguments {
    #[arg(
        long,
        value_enum,
        required_unless_present = "profile",
        conflicts_with = "profile"
    )]
    provider: Option<CatalogProviderArgument>,

    #[arg(long, conflicts_with_all = ["endpoint", "executable"])]
    profile: Option<String>,

    #[arg(long, value_name = "PATH", requires = "profile")]
    registry: Option<PathBuf>,

    #[arg(long, value_name = "HTTP_LOOPBACK_URL")]
    endpoint: Option<String>,

    #[arg(long, value_name = "PATH")]
    executable: Option<String>,

    /// Acknowledge one no-prompt ACP provider session.
    #[arg(long)]
    allow_host_discovery: bool,

    #[arg(long, default_value_t = 5)]
    timeout: u64,

    #[arg(long, default_value_t = 65_536)]
    max_response_bytes: usize,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CatalogProviderArgument {
    Ollama,
    LlamaServer,
    Copilot,
    Gemini,
    LlamaCli,
    CustomScript,
}

impl CatalogProviderArgument {
    fn catalog_provider(self) -> sav::Result<CatalogProvider> {
        match self {
            Self::Ollama => Ok(CatalogProvider::Ollama),
            Self::LlamaServer => Ok(CatalogProvider::LlamaServer),
            Self::Copilot => Ok(CatalogProvider::Copilot),
            Self::Gemini => Ok(CatalogProvider::Gemini),
            Self::LlamaCli => Err(Error::UnsupportedCapability {
                provider: "llama-cli",
                capability: "provider model catalog",
            }),
            Self::CustomScript => Err(Error::UnsupportedCapability {
                provider: "custom-script",
                capability: "provider model catalog",
            }),
        }
    }
}

#[derive(Debug, Args)]
struct ModelArguments {
    #[arg(value_name = "PROMPT", conflicts_with_all = ["file", "editor"])]
    prompt: Option<String>,

    #[arg(short, long, value_name = "PROMPT_FILE", conflicts_with_all = ["prompt", "editor"])]
    file: Option<PathBuf>,

    #[arg(long, conflicts_with_all = ["prompt", "file"])]
    editor: bool,

    #[arg(
        long,
        value_enum,
        required_unless_present = "profile",
        conflicts_with = "profile"
    )]
    provider: Option<ModelProviderArgument>,

    #[arg(long, conflicts_with_all = ["provider", "endpoint", "model"])]
    profile: Option<String>,

    #[arg(long, value_name = "PATH", requires = "profile")]
    registry: Option<PathBuf>,

    /// Seconds to wait for the endpoint to begin serving before failing.
    /// Local servers spawn a requested model on a routed port, which can take
    /// longer on cold start than any single request; raise this when swapping
    /// to a model that is not already loaded. Direct-provider mode only
    /// (default: 60).
    #[arg(long, conflicts_with = "profile")]
    slot_timeout: Option<u64>,

    /// Seconds to wait for the first token before failing. Must be greater
    /// than `--slot-timeout` and less than `--timeout`.
    /// Direct-provider mode only (default: 120).
    #[arg(long, conflicts_with = "profile")]
    ttft_timeout: Option<u64>,

    #[arg(
        long,
        value_name = "HTTP_LOOPBACK_URL",
        required_unless_present = "profile"
    )]
    endpoint: Option<String>,

    #[arg(long, value_name = "MODEL", required_unless_present = "profile")]
    model: Option<String>,

    #[arg(long)]
    stream: bool,

    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=86_400))]
    timeout: u64,

    #[arg(long, default_value_t = 1_048_576)]
    max_response_bytes: usize,

    #[arg(long, value_enum)]
    /// Provider reasoning-effort hint (native transport only).
    reasoning_effort: Option<ReasoningEffortArgument>,

    /// Apply a provider-native JSON Schema generation constraint.
    #[arg(long, value_name = "JSON_SCHEMA")]
    output_schema: Option<String>,

    /// Show provider-supplied reasoning on stderr.
    #[arg(long, conflicts_with = "json")]
    show_reasoning: bool,

    /// Emit one canonical model-turn JSON object.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ModelProviderArgument {
    Ollama,
    LlamaServer,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReasoningEffortArgument {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl From<ReasoningEffortArgument> for ReasoningEffort {
    fn from(value: ReasoningEffortArgument) -> Self {
        match value {
            ReasoningEffortArgument::None => Self::None,
            ReasoningEffortArgument::Minimal => Self::Minimal,
            ReasoningEffortArgument::Low => Self::Low,
            ReasoningEffortArgument::Medium => Self::Medium,
            ReasoningEffortArgument::High => Self::High,
            ReasoningEffortArgument::Xhigh => Self::Xhigh,
            ReasoningEffortArgument::Max => Self::Max,
        }
    }
}

impl From<ModelProviderArgument> for LocalModelProvider {
    fn from(value: ModelProviderArgument) -> Self {
        match value {
            ModelProviderArgument::Ollama => Self::Ollama,
            ModelProviderArgument::LlamaServer => Self::LlamaServer,
        }
    }
}

#[derive(Debug, Args)]
struct RunArguments {
    #[arg(value_name = "PROMPT", conflicts_with_all = ["file", "editor"])]
    prompt: Option<String>,

    #[arg(short, long, value_name = "PROMPT_FILE", conflicts_with_all = ["prompt", "editor"])]
    file: Option<PathBuf>,

    #[arg(long, conflicts_with_all = ["prompt", "file"])]
    editor: bool,

    #[arg(
        long,
        value_name = "TEMPLATE",
        required_unless_present = "profile",
        conflicts_with = "profile"
    )]
    command: Option<String>,

    #[arg(
        long,
        value_name = "NAME",
        required_unless_present = "command",
        conflicts_with = "command"
    )]
    profile: Option<String>,

    #[arg(long, value_name = "PATH")]
    registry: Option<PathBuf>,

    #[arg(long = "context", value_name = "PATH")]
    context_paths: Vec<PathBuf>,

    #[arg(long, value_name = "DIR", default_value = ".")]
    working_directory: PathBuf,

    #[arg(long, default_value_t = 900)]
    idle_timeout: u64,

    #[arg(long)]
    detect_loops: bool,

    /// Automatic command retries (default: 3); external profiles permit only 0.
    #[arg(long)]
    max_retries: Option<u32>,

    #[arg(long, default_value_t = 1_048_576)]
    max_output_bytes: usize,

    #[arg(long)]
    allow_host_execution: bool,

    #[arg(long, value_enum)]
    reasoning_effort: Option<ReasoningEffortArgument>,

    /// Emit one JSON object with the captured provider stdout as `content`.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SetupArguments {
    /// Override the Linux user profile configuration path.
    #[arg(long, value_name = "PATH")]
    registry: Option<PathBuf>,

    /// Save the profile even when mandatory live qualification fails.
    #[arg(long)]
    force: bool,
}

fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute() -> sav::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Model(arguments) => model(arguments),
        Command::Models(arguments) => models(arguments),
        Command::Run(arguments) => run(arguments),
        Command::Replay(arguments) => replay(arguments),
        Command::Setup(arguments) => setup(arguments),
        Command::Registry(arguments) => {
            let registry = registry_paths(arguments.registry)?.load()?;
            let profiles = match arguments.profile {
                Some(name) => vec![registry.resolve(&name)?],
                None => registry.profiles().cloned().collect(),
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&profiles).map_err(|_| Error::ProfileSetup {
                    reason: "cannot encode registry".into()
                })?
            );
            Ok(())
        }
    }
}

fn replay(arguments: ReplayArguments) -> sav::Result<()> {
    let report = sav::replay_trajectory(&arguments.session_file, arguments.max_turns)?;
    if arguments.json {
        let json = serde_json::to_string(&report).map_err(|e| Error::InvalidModelRequest {
            reason: format!("failed to serialize replay report: {e}"),
        })?;
        println!("{json}");
    } else {
        println!(
            "Replaying session `{}` for task `{}`:",
            report.session_id.as_deref().unwrap_or("unknown"),
            report.task_id.as_deref().unwrap_or("unknown"),
        );
        println!(
            "Total events: {}, Replayed turns: {}, Tool calls: {}",
            report.total_events, report.replayed_turns, report.tool_calls_count
        );
        println!("------------------------------------------------------------");
        for event in &report.events {
            match event {
                sav::TrajectoryEvent::SessionStart {
                    session_id,
                    task_id,
                    timestamp,
                } => {
                    println!("[SessionStart] id={session_id} task={task_id} time={timestamp}");
                }
                sav::TrajectoryEvent::TurnStart { turn, timestamp } => {
                    println!("\n--- Turn {turn} (time={timestamp}) ---");
                }
                sav::TrajectoryEvent::PromptEval {
                    turn,
                    new_tokens,
                    cached_tokens,
                    ..
                } => {
                    println!(
                        "[Turn {turn} PromptEval] new_tokens={new_tokens:?} cached_tokens={cached_tokens:?}"
                    );
                }
                sav::TrajectoryEvent::ModelReasoning { turn, reasoning } => {
                    println!("[Turn {turn} Reasoning]: {reasoning}");
                }
                sav::TrajectoryEvent::ToolDispatch {
                    turn,
                    tool,
                    args,
                    action_hash,
                    ..
                } => {
                    println!(
                        "[Turn {turn} ToolDispatch] {tool} (hash: {action_hash})\n  args: {args}"
                    );
                }
                sav::TrajectoryEvent::ToolResult {
                    turn,
                    tool,
                    exit_code,
                    bytes,
                    state_mutated,
                    ..
                } => {
                    println!(
                        "[Turn {turn} ToolResult] {tool} exit_code={exit_code} bytes={bytes} mutated={state_mutated}"
                    );
                }
                sav::TrajectoryEvent::TurnFinish {
                    turn,
                    finish_reason,
                    output_tokens,
                } => {
                    println!(
                        "[Turn {turn} Finish] reason={finish_reason} output_tokens={output_tokens:?}"
                    );
                }
                sav::TrajectoryEvent::SessionFinish {
                    total_turns,
                    total_tokens,
                    success,
                    ..
                } => {
                    println!(
                        "\n[SessionFinish] turns={total_turns} tokens={total_tokens} success={success}"
                    );
                }
            }
        }
    }
    Ok(())
}

fn models(arguments: ModelsArguments) -> sav::Result<()> {
    let resolved = arguments
        .profile
        .as_deref()
        .map(|name| {
            registry_paths(arguments.registry.clone())?
                .load()?
                .resolve(name)
        })
        .transpose()?;
    let (provider, endpoint, executable) = if let Some(profile) = resolved {
        use sav::registry::{Backend, ExternalProvider};
        match profile.profile.backend {
            Backend::Native(model) => (
                match model.provider {
                    LocalModelProvider::Ollama => CatalogProvider::Ollama,
                    LocalModelProvider::LlamaServer => CatalogProvider::LlamaServer,
                },
                Some(model.base_url),
                None,
            ),
            Backend::ExternalAgent(agent) => {
                let provider = match agent.provider {
                    ExternalProvider::Copilot => CatalogProvider::Copilot,
                    ExternalProvider::Gemini => CatalogProvider::Gemini,
                    _ => {
                        return Err(Error::ProfileSetup {
                            reason: "selected external provider has no catalog".into(),
                        });
                    }
                };
                (provider, None, Some(agent.executable))
            }
        }
    } else {
        (
            arguments
                .provider
                .ok_or_else(|| Error::ProfileSetup {
                    reason: "provider or profile required".into(),
                })?
                .catalog_provider()?,
            arguments.endpoint,
            arguments.executable,
        )
    };
    match provider {
        CatalogProvider::Ollama | CatalogProvider::LlamaServer if executable.is_some() => {
            return Err(Error::ModelCatalogInvalid {
                reason: "--executable is only valid for ACP providers".to_owned(),
            });
        }
        CatalogProvider::Copilot | CatalogProvider::Gemini if endpoint.is_some() => {
            return Err(Error::ModelCatalogInvalid {
                reason: "--endpoint is only valid for HTTP providers".to_owned(),
            });
        }
        _ => {}
    }
    let working_directory = std::env::current_dir().map_err(|source| Error::Io {
        operation: "determine discovery working directory",
        path: PathBuf::from("."),
        source,
    })?;
    let catalog = discover_models(
        provider,
        &ModelDiscoveryOptions {
            endpoint,
            executable,
            working_directory,
            timeout: Duration::from_secs(arguments.timeout),
            max_response_bytes: arguments.max_response_bytes,
            allow_host_discovery: arguments.allow_host_discovery,
        },
        &CancellationToken::new(),
    )?;
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    if arguments.json {
        serde_json::to_writer(&mut writer, &catalog).map_err(|source| Error::Io {
            operation: "write JSON model catalog",
            path: PathBuf::from("<stdout>"),
            source: std::io::Error::other(source),
        })?;
        writer.write_all(b"\n").map_err(|source| Error::Io {
            operation: "write JSON model catalog",
            path: PathBuf::from("<stdout>"),
            source,
        })?;
    } else {
        for model in catalog.models() {
            writeln!(writer, "{}", model.id()).map_err(|source| Error::Io {
                operation: "write model catalog",
                path: PathBuf::from("<stdout>"),
                source,
            })?;
        }
    }
    Ok(())
}

fn model(arguments: ModelArguments) -> sav::Result<()> {
    let prompt = resolve_prompt(
        arguments.prompt,
        arguments.file.as_deref(),
        arguments.editor,
    )?;
    let selected = arguments
        .profile
        .as_deref()
        .map(|name| {
            registry_paths(arguments.registry.clone())?
                .load()?
                .resolve(name)
        })
        .transpose()?;
    let output_schema = arguments
        .output_schema
        .map(|schema| {
            serde_json::from_str(&schema).map_err(|_| Error::InvalidModelRequest {
                reason: "--output-schema must contain one valid JSON value".to_owned(),
            })
        })
        .transpose()?;
    let timeout = Duration::from_secs(arguments.timeout);
    let (transport, model): (Box<dyn ModelTransport>, String) = if let Some(profile) = selected {
        let native = profile.native()?;
        (
            Box::new(native.transport_bounded(timeout, arguments.max_response_bytes)?),
            native.model.clone(),
        )
    } else {
        let slot_timeout = arguments.slot_timeout.unwrap_or(60);
        let ttft_timeout = arguments.ttft_timeout.unwrap_or(120);
        if slot_timeout >= ttft_timeout || ttft_timeout >= arguments.timeout {
            return Err(Error::InvalidModelTransport {
                reason: format!(
                    "--slot-timeout ({slot_timeout}) must be less than --ttft-timeout ({ttft_timeout}) which must be less than --timeout ({})",
                    arguments.timeout
                ),
            });
        }
        let missing = || Error::ProfileSetup {
            reason: "provider, endpoint and model are required without --profile".into(),
        };
        (
            Box::new(DirectModelTransport::with_watchdogs(
                arguments.provider.ok_or_else(missing)?.into(),
                &arguments.endpoint.ok_or_else(missing)?,
                timeout,
                arguments.max_response_bytes,
                Duration::from_secs(slot_timeout),
                Duration::from_secs(ttft_timeout),
            )?),
            arguments.model.ok_or_else(missing)?,
        )
    };
    let request = ModelRequest {
        model,
        messages: vec![ModelMessage::User(prompt.clone())],
        tools: Vec::new(),
        tool_choice: ToolChoice::None,
        reasoning_effort: arguments.reasoning_effort.map(Into::into),
        output_schema,
        max_output_tokens: None,
    };
    let cancellation = CancellationToken::new();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    if arguments.stream {
        if arguments.json {
            let turn = transport.stream(&request, &cancellation, &mut |_| Ok(()))?;
            write_json(&mut writer, &turn)?;
        } else {
            show_initial_prompt(&prompt);
            let show_reasoning = arguments.show_reasoning;
            transport.stream(&request, &cancellation, &mut |event| match event {
                ModelStreamEvent::TextDelta(text) => writer
                    .write_all(text.as_bytes())
                    .and_then(|()| writer.flush())
                    .map_err(|source| Error::Io {
                        operation: "write model output",
                        path: PathBuf::from("<stdout>"),
                        source,
                    }),
                ModelStreamEvent::ReasoningDelta(reasoning) if show_reasoning => {
                    write_stderr(reasoning.as_bytes())
                }
                ModelStreamEvent::ReasoningDelta(_) | ModelStreamEvent::ToolIntent(_) => Ok(()),
            })?;
        }
    } else {
        let turn = transport.complete(&request, &cancellation)?;
        if arguments.json {
            write_json(&mut writer, &turn)?;
        } else {
            show_initial_prompt(&prompt);
            if arguments.show_reasoning
                && let Some(reasoning) = &turn.reasoning
            {
                write_stderr(reasoning.as_bytes())?;
                write_stderr(b"\n")?;
            }
            writer
                .write_all(turn.text.as_bytes())
                .map_err(|source| Error::Io {
                    operation: "write model output",
                    path: PathBuf::from("<stdout>"),
                    source,
                })?;
        }
    }
    Ok(())
}

fn run(arguments: RunArguments) -> sav::Result<()> {
    if !arguments.allow_host_execution {
        return Err(Error::HostExecutionNotAcknowledged);
    }
    let prompt = resolve_prompt(
        arguments.prompt,
        arguments.file.as_deref(),
        arguments.editor,
    )?;
    let (command_template, external) = match (arguments.command, arguments.profile) {
        (Some(command), None) => (Some(command), None),
        (None, Some(profile)) => {
            let profile = registry_paths(arguments.registry)?
                .load()?
                .resolve(&profile)?;
            match profile.profile.backend {
                sav::registry::Backend::ExternalAgent(agent) => (None, Some(agent)),
                sav::registry::Backend::Native(_) => return Err(Error::ProfileSetup {
                    reason:
                        "native profiles use `sav-run model --profile NAME`, not process execution"
                            .into(),
                }),
            }
        }
        _ => {
            return Err(Error::ProfileSetup {
                reason: "exactly one of --command or --profile is required".to_owned(),
            });
        }
    };
    let max_retries = if external.is_some() {
        if arguments.max_retries.is_some_and(|retries| retries > 0) {
            return Err(Error::InvalidPolicy {
                reason: "--max-retries must be 0 for external-agent profiles; uncertain effects cannot be automatically replayed".into(),
            });
        }
        0
    } else {
        arguments.max_retries.unwrap_or(3)
    };
    let policy = SupervisionPolicy {
        idle_timeout: Duration::from_secs(arguments.idle_timeout),
        attempt_timeout: None,
        detect_loops: arguments.detect_loops,
        max_retries,
        max_output_bytes: arguments.max_output_bytes,
    };

    let reasoning_effort = arguments.reasoning_effort.map(Into::into);
    let command_for_attempt = |context: &sav::AttemptContext| {
        let prompt = match context.retry_notice() {
            Some(notice) => format!("{prompt}\n\n{notice}"),
            None => prompt.clone(),
        };
        if let Some(agent) = &external {
            return agent.command_spec(
                &prompt,
                &arguments.context_paths,
                &arguments.working_directory,
                reasoning_effort,
            );
        }
        let (program, command_arguments) = render_command_with_reasoning_effort(
            command_template
                .as_deref()
                .ok_or_else(|| Error::ProfileSetup {
                    reason: "command or external profile required".into(),
                })?,
            &prompt,
            &arguments.context_paths,
            &arguments.working_directory,
            reasoning_effort,
        )?;
        Ok(CommandSpec::new(program, command_arguments)
            .in_directory(arguments.working_directory.clone()))
    };
    if arguments.json {
        let report = run_supervised_capture(&policy, command_for_attempt)?;
        let content = String::from_utf8_lossy(&report.stdout);
        writeln!(
            std::io::stdout(),
            "{}",
            serde_json::json!({"content": content})
        )
        .map_err(|source| Error::Io {
            operation: "write JSON prompt output",
            path: PathBuf::from("<stdout>"),
            source,
        })?;
    } else {
        show_initial_prompt(&prompt);
        run_supervised(&policy, command_for_attempt)?;
    }
    Ok(())
}

fn setup(arguments: SetupArguments) -> sav::Result<()> {
    let config_path = arguments
        .registry
        .or_else(sav::registry::default_registry_path)
        .ok_or_else(|| Error::ProfileSetup {
            reason: "cannot resolve registry; set HOME or --registry".into(),
        })?;
    let current_directory = std::env::current_dir().map_err(|source| Error::Io {
        operation: "determine setup working directory",
        path: PathBuf::from("."),
        source,
    })?;
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    run_registry_setup(
        &mut reader,
        &mut writer,
        &current_directory,
        &config_path,
        SetupOptions {
            force: arguments.force,
            ..Default::default()
        },
    )?;
    Ok(())
}

fn show_initial_prompt(prompt: &str) {
    if std::io::stderr().is_terminal() {
        eprintln!("Prompt:\n{prompt}\n\nResponse:");
    }
}

fn write_stderr(bytes: &[u8]) -> sav::Result<()> {
    let stderr = std::io::stderr();
    let mut writer = stderr.lock();
    writer
        .write_all(bytes)
        .and_then(|()| writer.flush())
        .map_err(|source| Error::Io {
            operation: "write provider reasoning",
            path: PathBuf::from("<stderr>"),
            source,
        })
}

fn write_json(writer: &mut impl Write, turn: &sav::ModelTurn) -> sav::Result<()> {
    serde_json::to_writer(&mut *writer, turn).map_err(|source| Error::Io {
        operation: "write JSON model output",
        path: PathBuf::from("<stdout>"),
        source: std::io::Error::other(source),
    })?;
    writer.write_all(b"\n").map_err(|source| Error::Io {
        operation: "write JSON model output",
        path: PathBuf::from("<stdout>"),
        source,
    })
}

fn registry_paths(explicit: Option<PathBuf>) -> sav::Result<sav::registry::RegistryPaths> {
    let cwd = std::env::current_dir().map_err(|source| Error::Io {
        operation: "resolve registry project",
        path: PathBuf::from("."),
        source,
    })?;
    let mut paths = sav::registry::RegistryPaths::for_project(&cwd);
    paths.explicit = explicit;
    Ok(paths)
}
