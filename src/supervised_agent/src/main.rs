#[cfg(not(target_os = "linux"))]
compile_error!("supervised-agent currently supports Linux only");

use std::{path::PathBuf, process::ExitCode, time::Duration};

use clap::{Args, Parser, Subcommand};
use supervised_agent::{
    CommandSpec, Error, SupervisionPolicy, default_profile_config_path, load_profile,
    render_command, resolve_prompt, run_setup_wizard, run_supervised,
};

#[derive(Debug, Parser)]
#[command(
    name = "supervised-agent",
    version,
    about = "Run an AI prompt under bounded Linux host-process supervision"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run(RunArguments),
    /// Interactively create or update a reusable provider profile.
    Setup(SetupArguments),
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

fn execute() -> supervised_agent::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(arguments) => run(arguments),
        Command::Setup(arguments) => setup(arguments),
    }
}

fn run(arguments: RunArguments) -> supervised_agent::Result<()> {
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

fn setup(arguments: SetupArguments) -> supervised_agent::Result<()> {
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

fn resolve_config_path(path: Option<PathBuf>) -> supervised_agent::Result<PathBuf> {
    path.or_else(default_profile_config_path)
        .ok_or_else(|| Error::ProfileSetup {
            reason: "cannot resolve profile configuration; set HOME or XDG_CONFIG_HOME".to_owned(),
        })
}
