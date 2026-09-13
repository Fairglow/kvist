//! Task-lock inspection for the workspace shell.
//!
//! Kvist records one lock file per (project, component) pair under the
//! user-state directory while a task executes. This module scans those files,
//! treats their contents as untrusted bounded input, and classifies each lock
//! as live or stale by process liveness so the shell never presents a leftover
//! lock as active work.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Maximum bytes read from one lock file; larger files are treated as
/// malformed and reported as stale.
const MAX_LOCK_BYTES: u64 = 4096;

/// One parsed task-lock record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockEntry {
    /// Absolute path of the lock file.
    pub path: PathBuf,
    /// Task ID recorded in the lock, when parseable.
    pub task_id: Option<String>,
    /// Owning process ID, when parseable.
    pub pid: Option<u32>,
    /// Whether the owning process currently exists.
    pub live: bool,
    /// Age of the lock file in seconds, when the mtime is readable.
    pub age_secs: Option<i64>,
}

/// The user-state base directory (`$XDG_STATE_HOME` or `~/.local/state`).
pub(crate) fn user_state_base() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state")))
}

/// The task-lock directory under the user-state base, when it exists.
fn lock_directory() -> Option<PathBuf> {
    let dir = user_state_base()?.join("kvist").join("task-locks-v1");
    dir.is_dir().then_some(dir)
}

/// Scans the user-state task-lock directory.
///
/// Never fails: an absent or unreadable directory yields an empty list.
/// Non-lock entries (temporary files, directories) are ignored.
pub fn scan() -> Vec<LockEntry> {
    match lock_directory() {
        Some(dir) => scan_dir(&dir),
        None => Vec::new(),
    }
}

/// Scans one task-lock directory. Never fails: an absent or unreadable
/// directory yields an empty list; non-lock entries are ignored.
pub fn scan_dir(dir: &Path) -> Vec<LockEntry> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut locks = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("lock") {
            continue;
        }
        locks.push(parse_lock(&path));
    }
    locks.sort_by(|a, b| a.path.cmp(&b.path));
    locks
}

/// Parses one lock file into a [`LockEntry`], bounding the read.
fn parse_lock(path: &Path) -> LockEntry {
    let metadata = fs::metadata(path);
    let age_secs = metadata
        .as_ref()
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .ok()
                .map(|age| age.as_secs() as i64)
        });

    let mut task_id = None;
    let mut pid = None;
    let mut malformed = false;

    if let Ok(meta) = &metadata
        && meta.len() > MAX_LOCK_BYTES
    {
        malformed = true;
    }
    if !malformed {
        if let Ok(contents) = fs::read_to_string(path) {
            for line in contents.lines() {
                if let Some(value) = line.strip_prefix("task_id: ") {
                    task_id = Some(value.trim_matches('"').to_owned());
                } else if let Some(value) = line.strip_prefix("pid: ") {
                    pid = value.trim().parse::<u32>().ok();
                }
            }
            if pid.is_none() {
                malformed = true;
            }
        } else {
            malformed = true;
        }
    }

    let live = if malformed {
        false
    } else {
        crate::task_commands::lock_owner_appears_live(&fs::read_to_string(path).unwrap_or_default())
    };

    LockEntry {
        path: path.to_path_buf(),
        task_id,
        pid,
        live,
        age_secs,
    }
}

/// Removes stale (non-live) lock files from the user-state directory,
/// returning the removed paths.
///
/// Live locks are never touched. Removal failures are skipped, not fatal.
pub fn clean_stale() -> Vec<PathBuf> {
    match lock_directory() {
        Some(dir) => clean_stale_dir(&dir),
        None => Vec::new(),
    }
}

/// Removes stale (non-live) lock files from one directory, returning the
/// removed paths. Live locks are never touched; removal failures are
/// skipped, not fatal.
pub fn clean_stale_dir(dir: &Path) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    for entry in scan_dir(dir) {
        if entry.live {
            continue;
        }
        if fs::remove_file(&entry.path).is_ok() {
            removed.push(entry.path);
        }
    }
    removed
}

/// Formats a human-readable age from whole seconds.
pub fn format_age(age_secs: Option<i64>) -> String {
    let Some(secs) = age_secs else {
        return "unknown".to_owned();
    };
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_lock(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn lock_dir_in(dir: &Path) -> PathBuf {
        let lock_dir = dir.join("kvist").join("task-locks-v1");
        fs::create_dir_all(&lock_dir).unwrap();
        lock_dir
    }

    #[test]
    fn scan_dir_ignores_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(scan_dir(&dir.path().join("absent")).is_empty());
    }

    #[test]
    fn scan_dir_classifies_live_and_stale_locks() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = lock_dir_in(dir.path());

        let live = write_lock(
            &lock_dir,
            "live.lock",
            &format!(
                "schema_version: 1\nstarted_at: 2026-09-13T12:00:00Z\ntask_id: \"t1\"\npid: {}\nnonce: aa\n",
                std::process::id()
            ),
        );
        let stale = write_lock(
            &lock_dir,
            "stale.lock",
            "schema_version: 1\nstarted_at: 2026-09-01T12:00:00Z\ntask_id: \"t2\"\npid: 999999\nnonce: bb\n",
        );
        // Non-lock entries must be ignored.
        fs::write(lock_dir.join("scratch.tmp"), "x").unwrap();

        let locks = scan_dir(&lock_dir);

        assert_eq!(locks.len(), 2);
        assert!(
            locks
                .iter()
                .any(|l| l.path == live && l.live && l.task_id.as_deref() == Some("t1"))
        );
        assert!(
            locks
                .iter()
                .any(|l| l.path == stale && !l.live && l.pid == Some(999999))
        );
    }

    #[test]
    fn clean_stale_dir_removes_only_stale_locks() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = lock_dir_in(dir.path());

        let live = write_lock(
            &lock_dir,
            "live.lock",
            &format!(
                "schema_version: 1\ntask_id: \"t1\"\npid: {}\nnonce: aa\n",
                std::process::id()
            ),
        );
        let stale = write_lock(
            &lock_dir,
            "stale.lock",
            "schema_version: 1\ntask_id: \"t2\"\npid: 999999\nnonce: bb\n",
        );

        let removed = clean_stale_dir(&lock_dir);

        assert_eq!(removed, vec![stale]);
        assert!(live.exists());
    }

    #[test]
    fn malformed_lock_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let lock_dir = lock_dir_in(dir.path());

        let bad = write_lock(&lock_dir, "bad.lock", "not a lock file\n");
        let locks = scan_dir(&lock_dir);

        assert_eq!(locks.len(), 1);
        assert!(!locks[0].live);
        assert!(locks[0].path == bad);
    }

    #[test]
    fn format_age_renders_units() {
        assert_eq!(format_age(Some(5)), "5s");
        assert_eq!(format_age(Some(125)), "2m");
        assert_eq!(format_age(Some(7300)), "2h");
        assert_eq!(format_age(Some(200_000)), "2d");
        assert_eq!(format_age(None), "unknown".to_owned());
    }
}
