use std::path::Path;

use crate::config;

use super::locks;
use super::style::{self, Theme};

/// Stable context shown in the status bar that does not change per prompt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusContext {
    /// The configured sandbox backend name, or `none`.
    pub sandbox_backend: String,
    /// The configured default model profile, or `none`.
    pub default_model: String,
    /// Locks whose owning process is still alive.
    pub live_locks: usize,
    /// Leftover locks whose owner is gone (safe to clean with `locks clean`).
    pub stale_locks: usize,
}

impl StatusContext {
    pub fn load(project_dir: &Path) -> Self {
        let (sandbox_backend, default_model) = Self::config_part(project_dir);
        Self::from_parts(sandbox_backend, default_model, &locks::scan())
    }

    /// Reads the configured sandbox backend and default model, degrading to
    /// `none` when the configuration is absent or invalid.
    pub fn config_part(project_dir: &Path) -> (String, String) {
        match config::load(project_dir) {
            Ok(cfg) => (
                cfg.sandbox
                    .as_ref()
                    .map(|sandbox| sandbox.backend.clone())
                    .unwrap_or_else(|| "none".to_owned()),
                if !cfg.agent.developer.profile.is_empty() {
                    cfg.agent.developer.profile.clone()
                } else {
                    cfg.agent
                        .developer
                        .models
                        .first()
                        .map(|model| model.name.clone())
                        .unwrap_or_else(|| "none".to_owned())
                },
            ),
            Err(_) => ("none".to_owned(), "none".to_owned()),
        }
    }

    /// Builds the status context from already-resolved parts (the injection
    /// point for tests).
    pub fn from_parts(
        sandbox_backend: String,
        default_model: String,
        locks: &[super::locks::LockEntry],
    ) -> Self {
        let live_locks = locks.iter().filter(|lock| lock.live).count();
        Self {
            sandbox_backend,
            default_model,
            live_locks,
            stale_locks: locks.len() - live_locks,
        }
    }
}

/// Returns a concise, modern prompt for the active line editor.
///
/// Layout: `kvist <component>/ (<branch>) <failure marker> ❯ ` — the
/// component is set with `cd` and hidden at the root; the failure marker
/// (`✘`) appears after a failed command so the next prompt reflects the last
/// exit state; the branch is hidden outside a VCS repository.
pub fn short_prompt(
    theme: Theme,
    branch: Option<&str>,
    component: Option<&str>,
    failed: bool,
) -> String {
    let mut prompt = theme.style("1;34", "kvist");
    let component = component
        .filter(|c| !c.is_empty() && *c != ".")
        .map(|c| theme.style("1;35", &format!("{c}/")));
    let branch = branch
        .filter(|b| !b.is_empty() && *b != "no-vcs")
        .map(|b| theme.dim(&format!("({b})")));
    let marker = if failed {
        Some(theme.style("1;31", "✘"))
    } else {
        None
    };
    for part in [component, branch, marker].into_iter().flatten() {
        prompt.push(' ');
        prompt.push_str(&part);
    }
    prompt.push_str(&format!(" {}", theme.style("1;32", "❯")));
    prompt.push(' ');
    prompt
}

/// Formats the compact status badge displayed in the right prompt area.
pub fn status_bar_label(theme: Theme, status: &StatusContext) -> String {
    let mut parts = vec![status.sandbox_backend.clone()];
    if status.live_locks > 0 {
        parts.push(format!("🔒 {}", status.live_locks));
    }
    if status.stale_locks > 0 {
        parts.push(format!("⚠ {} stale", status.stale_locks));
    }
    parts.push(status.default_model.clone());
    theme.dim(&format!("[{}]", parts.join(" · ")))
}

/// Formats the welcome banner text: a titled box that fits the terminal.
pub fn format_welcome_banner(
    theme: Theme,
    status: &StatusContext,
    branch: Option<&str>,
    component: Option<&str>,
) -> String {
    let branch_str = branch.unwrap_or("no-vcs");
    let component_str = component.filter(|c| !c.is_empty()).unwrap_or(". (root)");

    let mut rows = vec![
        format!("Branch:    {branch_str}"),
        format!("Component:  {component_str}"),
        format!("Sandbox:   {}", status.sandbox_backend),
        format!("Model:     {}", status.default_model),
    ];
    if status.live_locks > 0 || status.stale_locks > 0 {
        let stale = if status.stale_locks > 0 {
            format!(", {} stale (run `locks clean`)", status.stale_locks)
        } else {
            String::new()
        };
        rows.push(format!("Locks:     {} active{stale}", status.live_locks));
    }
    rows.push("Commands: overview, status, task run, help".to_owned());
    rows.push(
        "Keys: TAB complete · ↑↓ pick · Ctrl+C cancel · Ctrl+L clear · Ctrl+D exit".to_owned(),
    );
    let titled = style::titled_box(
        theme,
        "Kvist Interactive Workspace Shell",
        &rows,
        style::terminal_size().map(|(width, _)| width),
    );
    format!(
        "{}\n{}\n{}",
        titled.top,
        titled.rows.join("\n"),
        titled.bottom
    )
}

/// Prints the welcome banner with static environment information.
pub fn print_welcome_banner(
    theme: Theme,
    status: &StatusContext,
    branch: Option<&str>,
    component: Option<&str>,
) {
    println!(
        "{}",
        format_welcome_banner(theme, status, branch, component)
    );
}

#[cfg(test)]
mod tests {
    use super::super::locks::LockEntry;
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn short_prompt_layout_and_exit_state() {
        let theme = Theme::plain();
        assert_eq!(
            short_prompt(theme, Some("main"), None, false),
            "kvist (main) ❯ "
        );
        assert_eq!(short_prompt(theme, None, None, false), "kvist ❯ ");
        assert_eq!(short_prompt(theme, Some("no-vcs"), None, false), "kvist ❯ ");
        assert_eq!(
            short_prompt(theme, Some("main"), Some("engine"), false),
            "kvist engine/ (main) ❯ "
        );
        assert_eq!(
            short_prompt(theme, None, Some("engine"), false),
            "kvist engine/ ❯ "
        );
        assert_eq!(
            short_prompt(theme, Some("main"), Some("."), false),
            "kvist (main) ❯ "
        );
        // A failed last command shows the failure marker.
        assert_eq!(
            short_prompt(theme, Some("main"), Some("engine"), true),
            "kvist engine/ (main) ✘ ❯ "
        );
        assert_eq!(short_prompt(theme, None, None, true), "kvist ✘ ❯ ");
    }

    #[test]
    fn status_bar_label_marks_locks_and_is_dimmed_when_enabled() {
        let theme = Theme::plain();
        let status = StatusContext {
            sandbox_backend: "bubblewrap".to_owned(),
            default_model: "ollama".to_owned(),
            live_locks: 0,
            stale_locks: 0,
        };
        assert_eq!(status_bar_label(theme, &status), "[bubblewrap · ollama]");

        let status_locked = StatusContext {
            sandbox_backend: "bubblewrap".to_owned(),
            default_model: "ollama".to_owned(),
            live_locks: 2,
            stale_locks: 1,
        };
        assert_eq!(
            status_bar_label(theme, &status_locked),
            "[bubblewrap · 🔒 2 · ⚠ 1 stale · ollama]"
        );

        let colored = Theme::enabled();
        assert!(status_bar_label(colored, &status).starts_with("\x1b[2m"));
    }

    fn lock(live: bool) -> LockEntry {
        LockEntry {
            path: PathBuf::from("/state/kvist/task-locks-v1/x.lock"),
            task_id: Some("t".to_owned()),
            pid: Some(1),
            live,
            age_secs: Some(5),
        }
    }

    #[test]
    fn from_parts_splits_live_and_stale_locks() {
        let status = StatusContext::from_parts(
            "bubblewrap".to_owned(),
            "ollama".to_owned(),
            &[lock(true), lock(true), lock(false)],
        );
        assert_eq!(status.live_locks, 2);
        assert_eq!(status.stale_locks, 1);
    }

    #[test]
    fn config_part_degrades_to_none_without_configuration() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            StatusContext::config_part(dir.path()),
            ("none".to_owned(), "none".to_owned())
        );
    }

    #[test]
    fn welcome_banner_layout_and_bounds() {
        let status = StatusContext {
            sandbox_backend: "bubblewrap".to_owned(),
            default_model: "ollama-long-model-name".to_owned(),
            live_locks: 1,
            stale_locks: 2,
        };
        let banner = format_welcome_banner(
            Theme::plain(),
            &status,
            Some("feature/long-branch-name-overflow-test"),
            Some("engine"),
        );
        for line in banner.lines() {
            assert!(
                line.starts_with('╭') || line.starts_with('│') || line.starts_with('╰'),
                "line: {line}"
            );
        }
        assert!(banner.contains("Branch:    feature/long-branch-name-overflow-test"));
        assert!(banner.contains("Sandbox:   bubblewrap"));
        assert!(banner.contains("Model:     ollama-long-model-name"));
        assert!(banner.contains("Locks:     1 active, 2 stale (run `locks clean`)"));
        assert!(banner.contains("Ctrl+L clear"));
        // Every banner line shares one visible width.
        let widths: std::collections::BTreeSet<usize> =
            banner.lines().map(style::visible_len).collect();
        assert_eq!(widths.len(), 1, "banner lines have different widths");
    }

    #[test]
    fn welcome_banner_omits_locks_when_none() {
        let status = StatusContext::default();
        let banner = format_welcome_banner(Theme::plain(), &status, None, None);
        assert!(!banner.contains("Locks:"));
        assert!(banner.contains("Component:  . (root)"));
    }
}
