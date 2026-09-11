use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use crate::config;

/// Active locks tracked by the workspace shell.
#[derive(Debug, Default)]
pub struct ActiveLocks {
    /// Lock paths currently held by writers, keyed by lock file path.
    locks: Mutex<BTreeMap<PathBuf, LockInfo>>,
}

/// Information about an active lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockInfo {
    /// Task ID that holds the lock.
    pub task_id: String,
    /// When the lock was acquired.
    pub acquired_at: Instant,
    /// The process or thread that holds it.
    pub owner: String,
}

impl ActiveLocks {
    /// Creates a new active locks manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a new active lock.
    pub fn record(&self, path: PathBuf, task_id: String, owner: String) {
        if let Ok(mut locks) = self.locks.lock() {
            locks.insert(
                path,
                LockInfo {
                    task_id,
                    acquired_at: Instant::now(),
                    owner,
                },
            );
        }
    }

    /// Removes a lock when released.
    pub fn release(&self, path: &Path) {
        if let Ok(mut locks) = self.locks.lock() {
            locks.remove(path);
        }
    }

    /// Returns all active locks as a list for display.
    pub fn as_list(&self) -> Vec<(PathBuf, LockInfo)> {
        self.locks
            .lock()
            .map(|l| l.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    /// Returns the number of currently recorded active locks.
    pub fn count(&self) -> usize {
        self.locks.lock().map(|l| l.len()).unwrap_or(0)
    }

    /// Scans the system task-lock state directory for active locks.
    pub fn scan_system_locks() -> usize {
        user_state_base()
            .map(|base| base.join("kvist").join("task-locks-v1"))
            .and_then(|dir| fs::read_dir(dir).ok())
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("lock"))
                    .count()
            })
            .unwrap_or(0)
    }
}

fn user_state_base() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state")))
}

/// Stable context shown in the status bar that does not change per prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusContext {
    pub sandbox_backend: String,
    pub default_model: String,
    pub active_locks: usize,
}

impl StatusContext {
    pub fn load(project_dir: &Path) -> Self {
        let active_locks = ActiveLocks::scan_system_locks();
        match config::load(project_dir) {
            Ok(cfg) => Self {
                sandbox_backend: cfg
                    .sandbox
                    .as_ref()
                    .map(|sandbox| sandbox.backend.clone())
                    .unwrap_or_else(|| "none".to_owned()),
                default_model: cfg
                    .agent
                    .developer
                    .models
                    .first()
                    .map(|model| model.name.clone())
                    .unwrap_or_else(|| "none".to_owned()),
                active_locks,
            },
            Err(_) => Self {
                sandbox_backend: "none".to_owned(),
                default_model: "none".to_owned(),
                active_locks,
            },
        }
    }
}

/// Formats the styled prompt label according to ADR 0007.
pub fn prompt_label(status: &StatusContext, branch: Option<&str>) -> String {
    let locks_part = if status.active_locks > 0 {
        format!(" [locks: {}]", status.active_locks)
    } else {
        String::new()
    };
    format!(
        "kvist ({}) [sandbox: {}] [model: {}]{} > ",
        branch.unwrap_or("no-vcs"),
        status.sandbox_backend,
        status.default_model,
        locks_part
    )
}

/// Returns a concise, modern prompt for the active line editor.
pub fn short_prompt(branch: Option<&str>) -> String {
    match branch {
        Some(b) if !b.is_empty() && b != "no-vcs" => format!("kvist ({b}) ❯ "),
        _ => "kvist ❯ ".to_owned(),
    }
}

/// Formats the compact status badge displayed in the right bar / right prompt area.
pub fn status_bar_label(status: &StatusContext) -> String {
    let locks = if status.active_locks > 0 {
        format!(" · 🔒 {}", status.active_locks)
    } else {
        String::new()
    };
    format!(
        "[{}{} · {}]",
        status.sandbox_backend, locks, status.default_model
    )
}

/// Prints a modern, styled welcome banner with static environment information.
pub fn print_welcome_banner(status: &StatusContext, branch: Option<&str>) {
    let branch_str = branch.unwrap_or("no-vcs");
    println!("╭──────────────────────────────────────────────────────────────╮");
    println!("│  ⚡ Kvist Interactive Workspace Shell                        │");
    println!(
        "│  Branch: {:<12} Sandbox: {:<12} Model: {:<11}│",
        branch_str, status.sandbox_backend, status.default_model
    );
    if status.active_locks > 0 {
        println!("│  Active locks: {:<46}│", status.active_locks);
    }
    println!("│  Commands: 'task next', 'task run', 'status', 'help'         │");
    println!("│  Press TAB for autocomplete (arrows to pick)  ·  'exit'      │");
    println!("╰──────────────────────────────────────────────────────────────╯");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_label_shows_branch_and_context() {
        let status = StatusContext {
            sandbox_backend: "bubblewrap".to_owned(),
            default_model: "ollama".to_owned(),
            active_locks: 0,
        };
        assert_eq!(
            prompt_label(&status, Some("main")),
            "kvist (main) [sandbox: bubblewrap] [model: ollama] > "
        );
        assert_eq!(
            prompt_label(&status, None),
            "kvist (no-vcs) [sandbox: bubblewrap] [model: ollama] > "
        );
    }

    #[test]
    fn prompt_label_shows_active_locks_when_present() {
        let status = StatusContext {
            sandbox_backend: "bubblewrap".to_owned(),
            default_model: "ollama".to_owned(),
            active_locks: 3,
        };
        assert_eq!(
            prompt_label(&status, Some("main")),
            "kvist (main) [sandbox: bubblewrap] [model: ollama] [locks: 3] > "
        );
    }

    #[test]
    fn short_prompt_and_status_bar() {
        assert_eq!(short_prompt(Some("main")), "kvist (main) ❯ ");
        assert_eq!(short_prompt(None), "kvist ❯ ");
        assert_eq!(short_prompt(Some("no-vcs")), "kvist ❯ ");

        let status = StatusContext {
            sandbox_backend: "bubblewrap".to_owned(),
            default_model: "ollama".to_owned(),
            active_locks: 0,
        };
        assert_eq!(status_bar_label(&status), "[bubblewrap · ollama]");

        let status_locked = StatusContext {
            sandbox_backend: "bubblewrap".to_owned(),
            default_model: "ollama".to_owned(),
            active_locks: 2,
        };
        assert_eq!(
            status_bar_label(&status_locked),
            "[bubblewrap · 🔒 2 · ollama]"
        );
    }

    #[test]
    fn active_locks_record_and_release() {
        let locks = ActiveLocks::new();
        assert_eq!(locks.count(), 0);

        let path = PathBuf::from("/tmp/test.lock");
        locks.record(path.clone(), "task-1".into(), "user".into());
        assert_eq!(locks.count(), 1);
        assert_eq!(locks.as_list().len(), 1);

        locks.release(&path);
        assert_eq!(locks.count(), 0);
    }
}
