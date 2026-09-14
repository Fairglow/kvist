use std::path::Path;

use crate::config;

use super::locks;

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
        let all_locks = locks::scan();
        let live_locks = all_locks.iter().filter(|lock| lock.live).count();
        let stale_locks = all_locks.len() - live_locks;
        match config::load(project_dir) {
            Ok(cfg) => Self {
                sandbox_backend: cfg
                    .sandbox
                    .as_ref()
                    .map(|sandbox| sandbox.backend.clone())
                    .unwrap_or_else(|| "none".to_owned()),
                default_model: if !cfg.agent.developer.profile.is_empty() {
                    cfg.agent.developer.profile.clone()
                } else {
                    cfg.agent
                        .developer
                        .models
                        .first()
                        .map(|model| model.name.clone())
                        .unwrap_or_else(|| "none".to_owned())
                },
                live_locks,
                stale_locks,
            },
            Err(_) => Self {
                sandbox_backend: "none".to_owned(),
                default_model: "none".to_owned(),
                live_locks,
                stale_locks,
            },
        }
    }
}

/// Returns a concise, modern prompt for the active line editor.
///
/// The current component (set with `cd`) is shown when it is not the root;
/// the root component is the default and keeps the prompt uncluttered.
pub fn short_prompt(branch: Option<&str>, component: Option<&str>) -> String {
    let component = component
        .filter(|c| !c.is_empty() && *c != ".")
        .map(|c| format!("{c}/ "))
        .unwrap_or_default();
    match branch {
        Some(b) if !b.is_empty() && b != "no-vcs" => format!("kvist {component}({b}) ❯ "),
        _ => format!("kvist {component}❯ "),
    }
}

/// Formats the compact status badge displayed in the right bar / right prompt area.
pub fn status_bar_label(status: &StatusContext) -> String {
    let mut parts = vec![status.sandbox_backend.clone()];
    if status.live_locks > 0 {
        parts.push(format!("🔒 {}", status.live_locks));
    }
    if status.stale_locks > 0 {
        parts.push(format!("⚠ {} stale", status.stale_locks));
    }
    parts.push(status.default_model.clone());
    format!("[{}]", parts.join(" · "))
}

/// Formats the welcome banner text with static environment information.
pub fn format_welcome_banner(
    status: &StatusContext,
    branch: Option<&str>,
    component: Option<&str>,
) -> String {
    let branch_str = branch.unwrap_or("no-vcs");
    let component_str = component.filter(|c| !c.is_empty()).unwrap_or(". (root)");

    let mut banner = String::new();
    banner.push_str("╭── Kvist Interactive Workspace Shell ─────────────────────────────\n");
    banner.push_str(&format!("│  Branch:   {}\n", branch_str));
    banner.push_str(&format!("│  Component:  {component_str}\n"));
    banner.push_str(&format!("│  Sandbox:  {}\n", status.sandbox_backend));
    banner.push_str(&format!("│  Model:    {}\n", status.default_model));
    if status.live_locks > 0 || status.stale_locks > 0 {
        let stale = if status.stale_locks > 0 {
            format!(", {} stale (run `locks clean`)", status.stale_locks)
        } else {
            String::new()
        };
        banner.push_str(&format!(
            "│  Locks:    {} active{stale}\n",
            status.live_locks
        ));
    }
    banner.push_str("│  Commands: 'overview', 'status', 'task run', 'help'\n");
    banner.push_str("│  Press TAB for autocomplete (arrows to pick)  ·  'exit'\n");
    banner.push_str("╰──────────────────────────────────────────────────────────────────");
    banner
}

/// Prints a modern, styled welcome banner with static environment information.
pub fn print_welcome_banner(status: &StatusContext, branch: Option<&str>, component: Option<&str>) {
    println!("{}", format_welcome_banner(status, branch, component));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_prompt_and_status_bar() {
        assert_eq!(short_prompt(Some("main"), None), "kvist (main) ❯ ");
        assert_eq!(short_prompt(None, None), "kvist ❯ ");
        assert_eq!(short_prompt(Some("no-vcs"), None), "kvist ❯ ");
        assert_eq!(
            short_prompt(Some("main"), Some("engine")),
            "kvist engine/ (main) ❯ "
        );
        assert_eq!(short_prompt(None, Some("engine")), "kvist engine/ ❯ ");
        assert_eq!(short_prompt(Some("main"), Some(".")), "kvist (main) ❯ ");

        let status = StatusContext {
            sandbox_backend: "bubblewrap".to_owned(),
            default_model: "ollama".to_owned(),
            live_locks: 0,
            stale_locks: 0,
        };
        assert_eq!(status_bar_label(&status), "[bubblewrap · ollama]");

        let status_locked = StatusContext {
            sandbox_backend: "bubblewrap".to_owned(),
            default_model: "ollama".to_owned(),
            live_locks: 2,
            stale_locks: 1,
        };
        assert_eq!(
            status_bar_label(&status_locked),
            "[bubblewrap · 🔒 2 · ⚠ 1 stale · ollama]"
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
            &status,
            Some("feature/long-branch-name-overflow-test"),
            Some("engine"),
        );
        for line in banner.lines() {
            assert!(line.starts_with('╭') || line.starts_with('│') || line.starts_with('╰'));
        }
        assert!(banner.contains("│  Branch:   feature/long-branch-name-overflow-test"));
        assert!(banner.contains("│  Sandbox:  bubblewrap"));
        assert!(banner.contains("│  Model:    ollama-long-model-name"));
        assert!(banner.contains("│  Locks:    1 active, 2 stale (run `locks clean`)"));
    }

    #[test]
    fn welcome_banner_omits_locks_when_none() {
        let status = StatusContext::default();
        let banner = format_welcome_banner(&status, None, None);
        assert!(!banner.contains("Locks:"));
        assert!(banner.contains("│  Component:  . (root)"));
    }
}
