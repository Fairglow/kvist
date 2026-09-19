//! Command-line interface for the agent-runner binary.

use std::path::PathBuf;
use std::str::FromStr;

use clap::Parser;

/// A first-class, sandbox-integrated interactive agent runner shell.
#[derive(Debug, Parser)]
#[command(
    name = "agent-runner",
    version,
    about = "Talk to an AI coding agent that works inside a sandbox.",
    long_about = None,
)]
pub struct Cli {
    /// Path to the TOML configuration file.
    #[arg(short, long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Select a configured model id for this session.
    #[arg(short, long, value_name = "ID")]
    pub model: Option<String>,

    /// Set the thinking effort (none, minimal, low, medium, high, xhigh, max).
    #[arg(short = 'e', long, value_name = "LEVEL")]
    pub effort: Option<String>,

    /// Set the working directory (must exist).
    #[arg(long, value_name = "PATH")]
    pub cwd: Option<PathBuf>,

    /// Select a language tool profile (generic, rust, python).
    #[arg(short, long, value_name = "NAME")]
    pub profile: Option<String>,

    /// Print the configured models and exit.
    #[arg(long)]
    pub list_models: bool,

    /// Directory for the session journal and transcript (default
    /// `.agent-runner/runs` under the working directory).
    #[arg(long, value_name = "PATH")]
    pub log_dir: Option<PathBuf>,

    /// Model context window in tokens, controlling when compaction begins
    /// (default 8192).
    #[arg(long, value_name = "TOKENS")]
    pub context_limit: Option<usize>,

    /// Do not write the durable session journal and transcript.
    #[arg(long)]
    pub no_logs: bool,

    /// An initial prompt to submit (optional).
    pub prompt: Option<String>,
}

/// The default configuration directory (relative to `CONFIG_HOME` or
/// `$XDG_CONFIG_HOME`), falling back to `$HOME/.config`.
const CONFIG_SUBDIR: &str = "agent-runner";
/// The project-local configuration file name searched in the working directory.
const LOCAL_CONFIG_FILE: &str = "agent-runner.toml";

/// Resolves which configuration file to load, honouring an explicit `--config`
/// path first and then the conventional environment and project-local
/// locations.
pub fn resolve_config_path(
    explicit: Option<PathBuf>,
) -> std::result::Result<std::path::PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    for env_var in ["CONFIG_HOME", "XDG_CONFIG_HOME", "HOME"] {
        if let Some(dir) = base_config_dir(env_var) {
            let candidate = dir.join(CONFIG_SUBDIR).join("config.toml");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    if let Ok(workdir) = std::env::current_dir() {
        let local = workdir.join(LOCAL_CONFIG_FILE);
        if local.is_file() {
            return Ok(local);
        }
    }
    Err(format!(
        "no configuration found; pass --config PATH, place {CONFIG_SUBDIR}/config.toml in your \n\
         config directory, or copy config.example.toml to {LOCAL_CONFIG_FILE}"
    ))
}

fn base_config_dir(env_var: &str) -> Option<std::path::PathBuf> {
    match env_var {
        "HOME" => std::env::var_os(env_var)
            .map(std::path::PathBuf::from)
            .map(|root| root.join(".config")),
        _ => std::env::var_os(env_var).map(std::path::PathBuf::from),
    }
}

/// Parses a thinking effort level for the CLI, with a descriptive error.
pub fn parse_effort(value: &str) -> std::result::Result<agent_runtime::ReasoningEffort, String> {
    agent_runtime::ReasoningEffort::from_str(value).map_err(|_| {
        "invalid thinking effort `{value}`; expected none, minimal, low, medium, high, xhigh, or max"
            .to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_model_effort_cwd_profile_overrides_and_prompt() {
        let cli = Cli::parse_from([
            "agent-runner",
            "-m",
            "llama",
            "-e",
            "high",
            "--cwd",
            "/tmp/work",
            "-p",
            "rust",
            "do the thing",
        ]);
        assert_eq!(cli.model.as_deref(), Some("llama"));
        assert_eq!(cli.effort.as_deref(), Some("high"));
        assert_eq!(cli.cwd, Some(PathBuf::from("/tmp/work")));
        assert_eq!(cli.profile.as_deref(), Some("rust"));
        assert_eq!(cli.prompt.as_deref(), Some("do the thing"));
        assert!(!cli.list_models);
    }

    #[test]
    fn parses_list_models_and_session_flags() {
        let cli = Cli::parse_from([
            "agent-runner",
            "--list-models",
            "--no-logs",
            "--context-limit",
            "4096",
            "--log-dir",
            "/tmp/logs",
        ]);
        assert!(cli.list_models);
        assert!(cli.no_logs);
        assert_eq!(cli.context_limit, Some(4096));
        assert_eq!(cli.log_dir, Some(PathBuf::from("/tmp/logs")));
    }

    #[test]
    fn parses_an_empty_cli_when_idle() {
        let cli = Cli::parse_from(["agent-runner"]);
        assert!(cli.model.is_none());
        assert!(cli.effort.is_none());
        assert!(cli.cwd.is_none());
        assert!(cli.profile.is_none());
        assert!(cli.prompt.is_none());
        assert!(!cli.list_models);
    }
}
