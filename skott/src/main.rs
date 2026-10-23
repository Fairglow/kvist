//! Process entry point for the `skott` binary.
//!
//! Logging initialisation, configuration resolution, and dispatch to the
//! interactive terminal UI. All logic lives in the `skott` library;
//! this binary only wires the CLI to it and maps failures to exit codes.

use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use skott::{
    Cli, Error, RunLimits, ToolProfile, config::Config, init_logging, tui, tui::Overrides,
};

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

fn run() -> skott::Result<ExitCode> {
    let cli = Cli::parse();
    if let Some(path) = cli.attach.as_deref() {
        skott::inspection::inspect(
            path,
            cli.follow,
            cli.control.as_deref(),
            cli.cancel_run,
            cli.guide.as_deref(),
        )?;
        return Ok(ExitCode::SUCCESS);
    }

    if cli.setup {
        let registry = cli
            .registry
            .clone()
            .or_else(sav::registry::default_registry_path)
            .ok_or_else(|| Error::Config {
                path: None,
                reason: "cannot resolve registry; set HOME or --registry".into(),
            })?;
        let cwd = cli.cwd.clone().unwrap_or(std::env::current_dir()?);
        sav::run_registry_setup(
            &mut std::io::BufReader::new(std::io::stdin().lock()),
            &mut std::io::stdout().lock(),
            &cwd,
            &registry,
            sav::SetupOptions {
                force: cli.force,
                ..Default::default()
            },
        )?;
        return Ok(ExitCode::SUCCESS);
    }

    // Resolve the configuration once so both `--config` and `--list-models`
    // share the same discovery rules and error messages.
    let config_path = skott::resolve_config_path(cli.config.clone())
        .map_err(|reason| Error::Config { path: None, reason })?;

    // Listing configured models is a read-only, non-interactive command.
    if cli.list_models {
        return list_models(&config_path, cli.registry.as_deref())
            .map_err(|reason| Error::Config { path: None, reason });
    }

    let config = Config::load_with_registry(&config_path, cli.registry.as_deref())?;

    let effort = match &cli.effort {
        Some(value) => Some(
            skott::parse_effort(value).map_err(|_| Error::InvalidEffort {
                value: value.clone(),
            })?,
        ),
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
    // flag is caught before the terminal takes over. Looks in the themes
    // directory next to the resolved configuration file first, so a custom
    // theme there is selectable by name like a built-in.
    let themes_dir = skott::tui::theme_files::themes_dir_for_config(&config_path);
    let theme = match &cli.theme {
        Some(name) => Some(
            skott::tui::theme_files::load(name, themes_dir.as_deref())
                .map_err(|reason| Error::Config { path: None, reason })?,
        ),
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
    if let Some(profile) = cli
        .model
        .as_deref()
        .and_then(|id| config.shared_profile(id))
        && matches!(
            profile.profile.backend,
            sav::registry::Backend::ExternalAgent(_)
        )
    {
        return skott::external::run_profile(
            profile,
            &overrides,
            &config.working_directory,
            cli.headless,
        );
    }
    if config.models.is_empty() {
        return Err(Error::Config{path:None,reason:"only external-agent profiles are configured; use --model NAME --allow-host-execution \"PROMPT\" in a terminal".into()});
    }
    if cli.headless {
        let summary =
            skott::headless::run_with_controls(config, overrides, cli.json, cli.allow_control)?;
        if let Some(failure) = &summary.failure {
            eprintln!("{}", skott::error::terminal_text(failure));
        }
        Ok(ExitCode::from(if summary.success() {
            0
        } else if summary.cancelled {
            130
        } else {
            1
        }))
    } else {
        Ok(tui::run_with_controls(config, overrides, cli.allow_control))
    }
}

fn list_models(
    config_path: &Path,
    registry: Option<&Path>,
) -> std::result::Result<ExitCode, String> {
    let config =
        Config::load_with_registry(config_path, registry).map_err(|error| error.describe())?;

    println!("configured models:");
    if config.models.is_empty() {
        println!("  (none)");
    } else {
        for model in &config.models {
            let marker = if model.is_default { "*" } else { " " };
            let id = format!("{marker}{}", skott::error::terminal_text(&model.id));
            println!(
                "  {:<12} {:<12} {}  {}  (deadline {}s)",
                id,
                match model.provider {
                    skott::ModelProvider::LlamaServer => "llama-server",
                    skott::ModelProvider::Ollama => "ollama",
                },
                skott::error::terminal_text(&model.base_url),
                skott::error::terminal_text(&model.model),
                model.deadline_secs
            );
        }
    }
    for profile in config.shared_profiles.values() {
        if let sav::registry::Backend::ExternalAgent(backend) = &profile.profile.backend {
            println!(
                "  {}  external-agent {}  {} (explicit host execution only)",
                skott::error::terminal_text(&profile.name),
                backend.provider.as_str(),
                skott::error::terminal_text(backend.model.as_deref().unwrap_or("provider default"))
            );
        }
    }
    for profile in config.shared_profiles.values() {
        if let sav::registry::Backend::ExternalAgent(agent) = &profile.profile.backend {
            println!(
                "  {:<12} external-agent {}  {}  {}",
                skott::error::terminal_text(&profile.name),
                agent.provider.as_str(),
                skott::error::terminal_text(&agent.executable),
                skott::error::terminal_text(agent.model.as_deref().unwrap_or("(provider default)")),
            );
        }
    }
    let default_label = match config.default_model() {
        Some(model) => format!("{} ({}'s default model)", model.id, config.default_provider),
        None => format!(
            "(none — {} has no is_default model)",
            config.default_provider
        ),
    };
    println!(
        "\ndefault provider: {}\nfall-back model: {}",
        config.default_provider,
        skott::error::terminal_text(&default_label)
    );
    Ok(ExitCode::SUCCESS)
}
