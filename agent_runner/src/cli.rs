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
    /// Path to the TOML configuration file. Without this flag the
    /// configuration is resolved in order: `./agent-runner.toml` in the
    /// current directory, then `$XDG_CONFIG_HOME/agent-runner/config.toml`
    /// (or `$HOME/.config` when `XDG_CONFIG_HOME` is unset), then the
    /// `XDG_CONFIG_DIRS` system locations (default `/etc/xdg`).
    #[arg(short, long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Print `[[models]]` entries derived from the agent profiles declared in
    /// a Kvist `kvist.toml` and exit (non-interactive).
    #[arg(long)]
    pub import_kvist: bool,

    /// Path to the Kvist project configuration to import agent profiles from
    /// (default: `./kvist.toml`).
    #[arg(long, value_name = "PATH")]
    pub kvist_config: Option<PathBuf>,

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

/// The per-app configuration directory name under the XDG config directories.
const CONFIG_SUBDIR: &str = "agent-runner";
/// The configuration file name inside the XDG configuration directory.
const CONFIG_FILE: &str = "config.toml";
/// The project-local configuration file name searched in the current directory.
const LOCAL_CONFIG_FILE: &str = "agent-runner.toml";
/// The default XDG system configuration directory (per the XDG Base Directory
/// specification).
const DEFAULT_XDG_CONFIG_DIRS: &str = "/etc/xdg";

/// Resolves which configuration file to load, honouring an explicit `--config`
/// path first, then the project-local file in the current directory, then the
/// XDG Base Directory specification: `$XDG_CONFIG_HOME` (or `$HOME/.config`
/// when unset) followed by the `$XDG_CONFIG_DIRS` system locations (default
/// `/etc/xdg`). Only the current directory is searched for the project-local
/// file; subdirectories are never searched.
pub fn resolve_config_path(
    explicit: Option<PathBuf>,
) -> std::result::Result<std::path::PathBuf, String> {
    resolve_config_path_with(explicit, std::env::current_dir().ok(), |name| {
        std::env::var_os(name)
    })
}

/// The testable core of [`resolve_config_path`] with an injectable current
/// directory and environment lookup.
pub fn resolve_config_path_with(
    explicit: Option<PathBuf>,
    cwd: Option<std::path::PathBuf>,
    env: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> std::result::Result<std::path::PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Some(workdir) = cwd {
        let local = workdir.join(LOCAL_CONFIG_FILE);
        if local.is_file() {
            return Ok(local);
        }
    }
    if let Some(dir) = xdg_user_config_dir(&env) {
        let candidate = dir.join(CONFIG_SUBDIR).join(CONFIG_FILE);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    for dir in xdg_system_config_dirs(&env) {
        let candidate = dir.join(CONFIG_SUBDIR).join(CONFIG_FILE);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    let user_dir = xdg_user_config_dir(&env).map(|dir| dir.join(CONFIG_SUBDIR).join(CONFIG_FILE));
    Err(match user_dir {
        Some(path) => format!(
            "no configuration found; pass --config PATH, create {LOCAL_CONFIG_FILE} in the \n\
             current directory, or place {CONFIG_SUBDIR}/{CONFIG_FILE} at `{}`",
            path.display()
        ),
        None => format!(
            "no configuration found; pass --config PATH, create {LOCAL_CONFIG_FILE} in the \n\
             current directory, or place {CONFIG_SUBDIR}/{CONFIG_FILE} in an XDG config directory"
        ),
    })
}

/// The XDG user configuration directory: `$XDG_CONFIG_HOME` when set to an
/// absolute path (per the XDG Base Directory specification; a relative value
/// is treated as unset), otherwise `$HOME/.config`.
fn xdg_user_config_dir(
    env: &impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    match env("XDG_CONFIG_HOME").map(std::path::PathBuf::from) {
        Some(dir) if dir.is_absolute() => return Some(dir),
        // A relative XDG_CONFIG_HOME violates the specification; fall back to
        // the $HOME/.config default rather than treating it as a directory.
        Some(_) | None => {}
    }
    env("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
}

/// The XDG system configuration directories from `$XDG_CONFIG_DIRS`
/// (colon-separated, default `/etc/xdg`), in priority order.
fn xdg_system_config_dirs(
    env: &impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<std::path::PathBuf> {
    let raw = env("XDG_CONFIG_DIRS")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| DEFAULT_XDG_CONFIG_DIRS.to_owned());
    if raw.trim().is_empty() {
        return vec![std::path::PathBuf::from(DEFAULT_XDG_CONFIG_DIRS)];
    }
    raw.split(':')
        .filter(|entry| !entry.is_empty())
        .map(std::path::PathBuf::from)
        .collect()
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
    use std::path::Path;

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
        assert!(!cli.import_kvist);
        assert!(cli.kvist_config.is_none());
    }

    #[test]
    fn parses_import_kvist_with_explicit_kvist_config() {
        let cli = Cli::parse_from([
            "agent-runner",
            "--import-kvist",
            "--kvist-config",
            "/tmp/kvist.toml",
        ]);
        assert!(cli.import_kvist);
        assert_eq!(cli.kvist_config, Some(PathBuf::from("/tmp/kvist.toml")));
    }

    /// A fixed fake environment for the resolver tests (owned, so no lifetime
    /// is captured from a temporary slice).
    fn env_map(
        pairs: Vec<(&'static str, &'static str)>,
    ) -> impl Fn(&str) -> Option<std::ffi::OsString> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).into())
        }
    }

    fn no_env() -> impl Fn(&str) -> Option<std::ffi::OsString> {
        |_| None
    }

    #[test]
    fn resolve_config_path_prefers_an_explicit_path() {
        let explicit = PathBuf::from("/tmp/explicit.toml");
        let resolved = resolve_config_path_with(
            Some(explicit.clone()),
            Some(PathBuf::from("/tmp")),
            no_env(),
        )
        .expect("explicit path resolves");
        assert_eq!(resolved, explicit);
    }

    /// Creates the conventional layout under a temp root and returns the paths.
    fn config_tree() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let local = root.path().join("local");
        let user = root.path().join("xdg-user");
        let system = root.path().join("xdg-sys");
        for dir in [
            &local,
            &user.join("agent-runner"),
            &system.join("agent-runner"),
        ] {
            std::fs::create_dir_all(dir).expect("create dir");
        }
        let local_file = local.join(LOCAL_CONFIG_FILE);
        let user_file = user.join("agent-runner").join("config.toml");
        let system_file = system.join("agent-runner").join("config.toml");
        for file in [&local_file, &user_file, &system_file] {
            std::fs::write(file, "").expect("seed config file");
        }
        (root, local_file, user_file, system_file)
    }

    fn str_of(path: &Path) -> &'static str {
        Box::leak(Box::new(path.to_string_lossy().into_owned()))
    }

    #[test]
    fn resolve_config_path_prefers_the_local_file_over_xdg() {
        // The local file is honoured even when an XDG user config exists.
        let (_root, local_file, user_file, _) = config_tree();
        // XDG_CONFIG_HOME is the parent of the `agent-runner` directory.
        let xdg = user_file
            .parent()
            .expect("agent-runner dir")
            .parent()
            .expect("xdg dir");
        let env = env_map(vec![("XDG_CONFIG_HOME", str_of(xdg))]);
        let resolved = resolve_config_path_with(
            None,
            Some(local_file.parent().expect("parent").to_owned()),
            env,
        )
        .expect("local file resolves");
        assert_eq!(resolved, local_file);
    }

    #[test]
    fn resolve_config_path_uses_xdg_config_home_when_no_local_file() {
        let (_root, local_file, user_file, _) = config_tree();
        let local_dir = local_file.parent().expect("parent");
        // Remove the local file so the XDG user config is the first hit.
        let _ = std::fs::remove_file(&local_file);
        let xdg = user_file
            .parent()
            .expect("agent-runner dir")
            .parent()
            .expect("xdg dir");
        let env = env_map(vec![("XDG_CONFIG_HOME", str_of(xdg))]);
        let resolved = resolve_config_path_with(None, Some(local_dir.to_owned()), env)
            .expect("xdg user config resolves");
        assert_eq!(resolved, user_file);
    }

    #[test]
    fn resolve_config_path_falls_back_to_home_dot_config() {
        let (root, _, user_file, _) = config_tree();
        let _ = std::fs::remove_file(user_file);
        let home = root.path().join("home");
        let home_config = home
            .join(".config")
            .join("agent-runner")
            .join("config.toml");
        std::fs::create_dir_all(home_config.parent().expect("parent")).expect("create");
        std::fs::write(&home_config, "").expect("seed");
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).expect("create");
        let env = env_map(vec![("HOME", str_of(&home))]);
        let resolved =
            resolve_config_path_with(None, Some(empty), env).expect("HOME/.config resolves");
        assert_eq!(resolved, home_config);
    }

    #[test]
    fn resolve_config_path_treats_relative_xdg_config_home_as_unset() {
        // The XDG specification requires an absolute XDG_CONFIG_HOME; a
        // relative value must not be used as a directory.
        let (root, _, user_file, _) = config_tree();
        let _ = std::fs::remove_file(user_file);
        let home = root.path().join("home");
        let home_config = home
            .join(".config")
            .join("agent-runner")
            .join("config.toml");
        std::fs::create_dir_all(home_config.parent().expect("parent")).expect("create");
        std::fs::write(&home_config, "").expect("seed");
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).expect("create");
        let env = env_map(vec![
            ("XDG_CONFIG_HOME", "relative/dir"),
            ("HOME", str_of(&home)),
        ]);
        let resolved =
            resolve_config_path_with(None, Some(empty), env).expect("falls back to HOME/.config");
        assert_eq!(resolved, home_config);
    }

    #[test]
    fn resolve_config_path_walks_xdg_config_dirs_in_order() {
        // When no local file and no XDG user config exist, the first
        // XDG_CONFIG_DIRS entry that has the file wins.
        let (root, local_file, user_file, system_file) = config_tree();
        let _ = std::fs::remove_file(&local_file);
        let _ = std::fs::remove_file(user_file);
        let system = system_file
            .parent()
            .expect("parent")
            .parent()
            .expect("parent");
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).expect("create");
        let env = env_map(vec![
            ("XDG_CONFIG_DIRS", str_of(system)),
            ("HOME", str_of(root.path())),
        ]);
        let resolved = resolve_config_path_with(None, Some(empty), env)
            .expect("first XDG_CONFIG_DIRS entry resolves");
        assert_eq!(resolved, system_file);
    }

    #[test]
    fn resolve_config_path_reports_when_nothing_exists() {
        let (root, local_file, user_file, system_file) = config_tree();
        let _ = std::fs::remove_file(local_file);
        let _ = std::fs::remove_file(user_file);
        let _ = std::fs::remove_file(system_file);
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).expect("create");
        let env = env_map(vec![("HOME", str_of(root.path()))]);
        let err = resolve_config_path_with(None, Some(empty), env)
            .expect_err("no config anywhere must fail");
        let conventional = root
            .path()
            .join(".config")
            .join("agent-runner")
            .join("config.toml")
            .display()
            .to_string();
        assert!(
            err.contains(&conventional),
            "the error names the conventional location: {err}"
        );
        assert!(
            err.contains(LOCAL_CONFIG_FILE),
            "the error names the local file: {err}"
        );
    }
}
