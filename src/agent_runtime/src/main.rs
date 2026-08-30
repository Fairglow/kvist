#[cfg(not(target_os = "linux"))]
compile_error!("agent-runtime currently supports Linux only");

use std::{io::Write, path::PathBuf, process::ExitCode, time::Duration};

#[cfg(feature = "rig-transport")]
use agent_runtime::RigModelTransport;
use agent_runtime::{
    CancellationToken, CommandSpec, DirectModelTransport, Error, LocalModelProvider, ModelMessage,
    ModelRequest, ModelStreamEvent, ModelTransport, SupervisionPolicy, ToolChoice,
    default_profile_config_path, load_profile, render_command, resolve_prompt, run_setup_wizard,
    run_supervised,
};
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "agent-run",
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
    Run(RunArguments),
    /// Interactively create or update a reusable provider profile.
    Setup(SetupArguments),
}

#[derive(Debug, Args)]
struct ModelArguments {
    #[arg(value_name = "PROMPT", conflicts_with_all = ["file", "editor"])]
    prompt: Option<String>,

    #[arg(short, long, value_name = "PROMPT_FILE", conflicts_with_all = ["prompt", "editor"])]
    file: Option<PathBuf>,

    #[arg(long, conflicts_with_all = ["prompt", "file"])]
    editor: bool,

    #[arg(long, value_enum)]
    provider: ModelProviderArgument,

    #[arg(long, value_enum, default_value_t = ModelTransportArgument::Direct)]
    transport: ModelTransportArgument,

    #[arg(long, value_name = "HTTP_LOOPBACK_URL")]
    endpoint: String,

    #[arg(long, value_name = "MODEL")]
    model: String,

    #[arg(long)]
    stream: bool,

    #[arg(long, default_value_t = 300)]
    timeout: u64,

    #[arg(long, default_value_t = 1_048_576)]
    max_response_bytes: usize,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ModelProviderArgument {
    Ollama,
    LlamaServer,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum ModelTransportArgument {
    #[default]
    Direct,
    #[cfg(feature = "rig-transport")]
    Rig,
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
    config: Option<PathBuf>,

    #[arg(long = "context", value_name = "PATH")]
    context_paths: Vec<PathBuf>,

    #[arg(long, value_name = "DIR", default_value = ".")]
    working_directory: PathBuf,

    #[arg(long, default_value_t = 900)]
    idle_timeout: u64,

    #[arg(long)]
    detect_loops: bool,

    #[arg(long, default_value_t = 3)]
    max_retries: u32,

    #[arg(long, default_value_t = 1_048_576)]
    max_output_bytes: usize,

    #[arg(long)]
    allow_host_execution: bool,
}

#[derive(Debug, Args)]
struct SetupArguments {
    /// Override the Linux user profile configuration path.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
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

fn execute() -> agent_runtime::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Model(arguments) => model(arguments),
        Command::Run(arguments) => run(arguments),
        Command::Setup(arguments) => setup(arguments),
    }
}

fn model(arguments: ModelArguments) -> agent_runtime::Result<()> {
    let prompt = resolve_prompt(
        arguments.prompt,
        arguments.file.as_deref(),
        arguments.editor,
    )?;
    let provider = arguments.provider.into();
    let timeout = Duration::from_secs(arguments.timeout);
    let transport: Box<dyn ModelTransport> = match arguments.transport {
        ModelTransportArgument::Direct => Box::new(DirectModelTransport::new(
            provider,
            &arguments.endpoint,
            timeout,
            arguments.max_response_bytes,
        )?),
        #[cfg(feature = "rig-transport")]
        ModelTransportArgument::Rig => Box::new(RigModelTransport::new(
            provider,
            &arguments.endpoint,
            timeout,
            arguments.max_response_bytes,
        )?),
    };
    let request = ModelRequest {
        model: arguments.model,
        messages: vec![ModelMessage::User(prompt)],
        tools: Vec::new(),
        tool_choice: ToolChoice::None,
    };
    let cancellation = CancellationToken::new();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();

    if arguments.stream {
        transport.stream(&request, &cancellation, &mut |event| match event {
            ModelStreamEvent::TextDelta(text) => writer
                .write_all(text.as_bytes())
                .and_then(|()| writer.flush())
                .map_err(|source| Error::Io {
                    operation: "write model output",
                    path: PathBuf::from("<stdout>"),
                    source,
                }),
            ModelStreamEvent::ToolIntent(_) => Ok(()),
        })?;
        writer.write_all(b"\n").map_err(|source| Error::Io {
            operation: "write model output",
            path: PathBuf::from("<stdout>"),
            source,
        })?;
    } else {
        let turn = transport.complete(&request, &cancellation)?;
        writeln!(writer, "{}", turn.text).map_err(|source| Error::Io {
            operation: "write model output",
            path: PathBuf::from("<stdout>"),
            source,
        })?;
    }
    Ok(())
}

fn run(arguments: RunArguments) -> agent_runtime::Result<()> {
    if !arguments.allow_host_execution {
        return Err(Error::HostExecutionNotAcknowledged);
    }
    let prompt = resolve_prompt(
        arguments.prompt,
        arguments.file.as_deref(),
        arguments.editor,
    )?;
    let policy = SupervisionPolicy {
        idle_timeout: Duration::from_secs(arguments.idle_timeout),
        attempt_timeout: None,
        detect_loops: arguments.detect_loops,
        max_retries: arguments.max_retries,
        max_output_bytes: arguments.max_output_bytes,
    };
    let command_template = match (arguments.command, arguments.profile) {
        (Some(command), None) => command,
        (None, Some(profile)) => {
            let config_path = resolve_config_path(arguments.config)?;
            load_profile(&config_path, &profile)?.command
        }
        _ => {
            return Err(Error::ProfileSetup {
                reason: "exactly one of --command or --profile is required".to_owned(),
            });
        }
    };

    run_supervised(&policy, |context| {
        let prompt = match context.retry_notice() {
            Some(notice) => format!("{prompt}\n\n{notice}"),
            None => prompt.clone(),
        };
        let (program, command_arguments) = render_command(
            &command_template,
            &prompt,
            &arguments.context_paths,
            &arguments.working_directory,
        )?;
        Ok(CommandSpec::new(program, command_arguments)
            .in_directory(arguments.working_directory.clone()))
    })?;
    Ok(())
}

fn setup(arguments: SetupArguments) -> agent_runtime::Result<()> {
    let config_path = resolve_config_path(arguments.config)?;
    let current_directory = std::env::current_dir().map_err(|source| Error::Io {
        operation: "determine setup working directory",
        path: PathBuf::from("."),
        source,
    })?;
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    run_setup_wizard(&mut reader, &mut writer, &current_directory, &config_path)?;
    Ok(())
}

fn resolve_config_path(path: Option<PathBuf>) -> agent_runtime::Result<PathBuf> {
    path.or_else(default_profile_config_path)
        .ok_or_else(|| Error::ProfileSetup {
            reason: "cannot resolve profile configuration; set HOME or XDG_CONFIG_HOME".to_owned(),
        })
}
