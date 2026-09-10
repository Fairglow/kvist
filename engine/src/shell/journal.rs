use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::Result;

/// Session journal file path relative to the project root.
pub const SESSION_JOURNAL: &str = ".kvist/session.log";

/// A single entry in the session journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEntry {
    /// Timestamp of the entry (RFC 3339 formatted).
    pub timestamp: String,
    /// The command that was run.
    pub command: String,
    /// The result or output summary.
    pub result: String,
    /// Whether this was a transient stream (excluded from permanent disk persistence).
    pub transient: bool,
}

/// Manages the REPL session journal with atomic disk persistence.
pub struct SessionJournal {
    /// Path to the journal file relative to the project root.
    journal_path: PathBuf,
    /// In-memory cache of entries.
    entries: Mutex<Vec<JournalEntry>>,
}

impl SessionJournal {
    /// Creates a new session journal for the given project root.
    pub fn new(project_root: &Path) -> Self {
        Self {
            journal_path: project_root.join(SESSION_JOURNAL),
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Appends an entry to the journal and periodically persists.
    pub fn append(&self, entry: JournalEntry) {
        let mut entries = self.entries.lock().unwrap();
        entries.push(entry);
        if entries.len().is_multiple_of(100) {
            self.persist_locked(&entries);
        }
    }

    /// Flushes all pending entries to disk.
    pub fn flush(&self) {
        let entries = self.entries.lock().unwrap().clone();
        self.persist_locked(&entries);
    }

    /// Internal persistence helper.
    fn persist_locked(&self, entries: &[JournalEntry]) {
        if entries.is_empty() {
            return;
        }
        if let Some(parent) = self.journal_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let temp_path = format!("{}.tmp", self.journal_path.display());
        let mut content = "# Session Journal
# Format: timestamp|command|result|transient
"
        .to_string();
        for entry in entries {
            if entry.transient {
                continue;
            }
            content.push_str(&format!(
                "{}|{}|{}|{}
",
                entry.timestamp, entry.command, entry.result, entry.transient
            ));
        }
        if let Err(e) = fs::write(&temp_path, &content) {
            eprintln!("Warning: could not persist session journal: {}", e);
            return;
        }
        if fs::rename(&temp_path, &self.journal_path).is_err() {
            let _ = fs::remove_file(&temp_path);
        }
    }

    /// Returns entries since a given index.
    #[allow(dead_code)]
    pub fn since(&self, since_index: usize) -> Vec<String> {
        let entries = self.entries.lock().unwrap().clone();
        entries
            .iter()
            .skip(since_index)
            .map(|e| e.command.clone())
            .collect()
    }

    /// Returns a copy of all current in-memory entries.
    pub fn entries(&self) -> Vec<JournalEntry> {
        self.entries.lock().unwrap().clone()
    }
}

/// Displays the current session journal entries.
pub fn display_session_status(journal: &SessionJournal) -> Result<()> {
    let entries = journal.entries();
    let permanent: Vec<_> = entries.iter().filter(|e| !e.transient).collect();
    if permanent.is_empty() {
        println!("Session journal: (empty)");
        return Ok(());
    }

    println!("Session journal:");
    for (i, entry) in permanent.iter().enumerate() {
        println!("  {}. [{}] {}", i + 1, entry.timestamp, entry.command);
        if !entry.result.is_empty() {
            println!("     -> {}", entry.result);
        }
    }
    println!("  Total: {} entries", permanent.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn journal_creation_and_append() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        assert!(journal.entries().is_empty());

        journal.append(JournalEntry {
            timestamp: "2026-09-10T12:00:00Z".into(),
            command: "task run . write-tests".into(),
            result: "ok".into(),
            transient: false,
        });

        assert_eq!(journal.entries().len(), 1);
        assert_eq!(journal.since(0), vec!["task run . write-tests"]);
        assert_eq!(journal.since(1), Vec::<String>::new());
    }

    #[test]
    fn journal_flush_persists_to_file_excluding_transient() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());

        journal.append(JournalEntry {
            timestamp: "2026-09-10T12:00:00Z".into(),
            command: "status".into(),
            result: "ok".into(),
            transient: false,
        });
        journal.append(JournalEntry {
            timestamp: "2026-09-10T12:01:00Z".into(),
            command: "stream-progress".into(),
            result: "chunk".into(),
            transient: true,
        });
        journal.flush();

        let log_file = dir.path().join(SESSION_JOURNAL);
        assert!(log_file.exists());
        let contents = fs::read_to_string(&log_file).unwrap();
        assert!(contents.contains("status"));
        assert!(!contents.contains("stream-progress"));
    }

    #[test]
    fn display_session_status_handles_empty_and_populated() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        assert!(display_session_status(&journal).is_ok());

        journal.append(JournalEntry {
            timestamp: "2026-09-10T12:00:00Z".into(),
            command: "status".into(),
            result: "ok".into(),
            transient: false,
        });
        assert!(display_session_status(&journal).is_ok());
    }
}
