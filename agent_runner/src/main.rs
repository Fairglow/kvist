//! Process entry point for the `agent-runner` binary.
//!
//! Logging initialisation, configuration resolution, and dispatch to the
//! interactive terminal UI. All logic lives in the `agent_runner` library;
//! this binary only wires the CLI to it and maps failures to exit codes.

use std::path::Path;
use std::process::ExitCode;

use agent_runner::{Cli, ToolProfile, config::Config, init_logging, tui, tui::Overrides};
use clap::Parser;

fn main() -> ExitCode {
    init_logging();
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> std::result::Result<ExitCode, String> {
    let cli = Cli::parse();

    // Importing Kvist agent profiles is a read-only, non-interactive command
    // that does not need an agent-runner configuration.
    if cli.import_kvist {
        return import_kvist(cli.kvist_config.clone());
    }

    // Resolve the configuration once so both `--config` and `--list-models`
    // share the same discovery rules and error messages.
    let config_path = agent_runner::resolve_config_path(cli.config.clone())?;

    // Listing configured models is a read-only, non-interactive command.
    if cli.list_models {
        return list_models(&config_path);
    }

    let config = Config::load(&config_path).map_err(|error| error.describe())?;

    let effort = match &cli.effort {
        Some(value) => Some(agent_runner::parse_effort(value)?),
        None => None,
    };
    let profile = match &cli.profile {
        Some(name) => Some(ToolProfile::from_id(name).ok_or_else(|| {
            format!(
                "unknown tool profile `{name}`; expected generic, python, rust, javascript, go, or c"
            )
        })?),
        None => None,
    };

    let overrides = Overrides {
        model: cli.model.clone(),
        effort,
        cwd: cli.cwd.clone(),
        profile,
        log_dir: cli.log_dir.clone(),
        context_limit: cli.context_limit,
        no_logs: cli.no_logs,
        config_path: Some(config_path.clone()),
        allow_host_execution: cli.allow_host_execution,
        host_turns: cli.host_turns,
        prompt: cli.prompt.clone(),
    };

    Ok(tui::run(config, overrides))
}

/// Prints `[[models]]` entries derived from a Kvist project configuration so
/// the project's agents can be reused in the agent-runner configuration.
fn import_kvist(kvist_config: Option<std::path::PathBuf>) -> std::result::Result<ExitCode, String> {
    let path = agent_runner::resolve_kvist_config_path(kvist_config)?;
    let snippet = agent_runner::import_models(&path).map_err(|error| error.describe())?;
    println!("# imported from {}", path.display());
    print!("{snippet}");
    Ok(ExitCode::SUCCESS)
}

fn list_models(config_path: &Path) -> std::result::Result<ExitCode, String> {
    let config = Config::load(config_path).map_err(|error| error.describe())?;

    println!("configured models:");
    if config.models.is_empty() {
        println!("  (none)");
    } else {
        for model in &config.models {
            println!(
                "  {:<12} {:<12?} {}  {}  (deadline {}s)",
                model.id, model.provider, model.base_url, model.model, model.deadline_secs
            );
        }
    }
    println!("\ndefault model: {}", config.default_model);
    Ok(ExitCode::SUCCESS)
}
