//! Process entry point for the `agent-runner` binary.
//!
//! Logging initialisation, configuration resolution, and dispatch to the
//! interactive terminal UI. All logic lives in the `agent_runner` library;
//! this binary only wires the CLI to it and maps failures to exit codes.

use std::path::Path;
use std::process::ExitCode;

use agent_runner::{
    Cli, Error, RunLimits, ToolProfile, config::Config, init_logging, tui, tui::Overrides,
};
use clap::Parser;

fn main() -> ExitCode {
    init_logging();
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{}", error.describe());
            ExitCode::from(error.exit_code())
        }
    }
}

fn run() -> agent_runner::Result<ExitCode> {
    let cli = Cli::parse();

    // Importing Kvist agent profiles is a read-only, non-interactive command
    // that does not need an agent-runner configuration.
    if cli.import_kvist {
        return import_kvist(cli.kvist_config.clone())
            .map_err(|reason| Error::Config { path: None, reason });
    }

    // Resolve the configuration once so both `--config` and `--list-models`
    // share the same discovery rules and error messages.
    let config_path = agent_runner::resolve_config_path(cli.config.clone())
        .map_err(|reason| Error::Config { path: None, reason })?;

    // Listing configured models is a read-only, non-interactive command.
    if cli.list_models {
        return list_models(&config_path).map_err(|reason| Error::Config { path: None, reason });
    }

    let config = Config::load(&config_path)?;

    let effort = match &cli.effort {
        Some(value) => {
            Some(
                agent_runner::parse_effort(value).map_err(|_| Error::InvalidEffort {
                    value: value.clone(),
                })?,
            )
        }
        None => None,
    };
    let profile = match &cli.profile {
        Some(name) => Some(ToolProfile::from_id(name).ok_or_else(|| {
            Error::Config { path:None, reason:format!(
                "unknown tool profile `{name}`; expected generic, python, rust, javascript, go, or c"
            ) }
        })?),
        None => None,
    };

    // The UI theme: the `--theme` flag overrides the configuration's `theme`
    // key. Unknown names fail here with the accepted list, so a typo in the
    // flag is caught before the terminal takes over.
    let theme = match &cli.theme {
        Some(name) => {
            Some(
                agent_runner::tui::Theme::by_name(name).ok_or_else(|| Error::Config {
                    path: None,
                    reason: format!(
                        "unknown theme `{name}`; expected one of: {}",
                        agent_runner::tui::THEME_NAMES.join(", ")
                    ),
                })?,
            )
        }
        None => None,
    };

    let overrides = Overrides {
        model: cli.model.clone(),
        effort,
        cwd: cli.cwd.clone(),
        profile,
        log_dir: cli.log_dir.clone(),
        context_limit: cli.context_limit,
        response_reserve: cli.response_reserve,
        no_logs: cli.no_logs,
        config_path: Some(config_path.clone()),
        allow_host_execution: cli.allow_host_execution,
        host_turns: cli.host_turns,
        prompt: cli.prompt.clone(),
        limits: RunLimits {
            wall_time: std::time::Duration::from_secs(cli.max_run_secs),
            max_tokens: cli.max_run_tokens,
            response_reserve: cli.response_reserve.unwrap_or(1024),
        },
        theme,
    };
    if cli.headless {
        let summary = agent_runner::headless::run(config, overrides, cli.json)?;
        if let Some(failure) = &summary.failure {
            eprintln!("{}", agent_runner::error::terminal_text(failure));
        }
        Ok(ExitCode::from(if summary.success() {
            0
        } else if summary.cancelled {
            130
        } else {
            1
        }))
    } else {
        Ok(tui::run(config, overrides))
    }
}

/// Prints `[[models]]` entries derived from a Kvist project configuration so
/// the project's agents can be reused in the agent-runner configuration.
fn import_kvist(kvist_config: Option<std::path::PathBuf>) -> std::result::Result<ExitCode, String> {
    let path = agent_runner::resolve_kvist_config_path(kvist_config)?;
    let snippet = agent_runner::import_models(&path).map_err(|error| error.describe())?;
    println!(
        "# imported from {}",
        agent_runner::error::terminal_text(&path.display().to_string())
    );
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
                "  {:<12} {:<12} {}  {}  (deadline {}s)",
                agent_runner::error::terminal_text(&model.id),
                match model.provider {
                    agent_runner::ModelProvider::LlamaServer => "llama-server",
                    agent_runner::ModelProvider::Ollama => "ollama",
                },
                agent_runner::error::terminal_text(&model.base_url),
                agent_runner::error::terminal_text(&model.model),
                model.deadline_secs
            );
        }
    }
    println!(
        "\ndefault model: {}",
        agent_runner::error::terminal_text(&config.default_model)
    );
    Ok(ExitCode::SUCCESS)
}
