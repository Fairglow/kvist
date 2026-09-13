//! Append-only, JSONL session journal.
//!
//! The journal persists one JSON object per line (`{timestamp, command,
//! result}`) in `.kvist/session.log` and is append-only across shell
//! sessions. Every append is written immediately in O_APPEND mode so an
//! interrupted session never loses recorded commands; loading tolerates
//! malformed or truncated lines instead of failing the shell.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::pager::display_output;

/// Session journal file path relative to the project root.
pub const SESSION_JOURNAL: &str = ".kvist/session.log";

/// Maximum bytes read from the journal when loading it into memory.
const MAX_JOURNAL_BYTES: u64 = 1_048_576;

/// A single entry in the session journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// Timestamp of the entry (RFC 3339 formatted).
    pub timestamp: String,
    /// The command that was run.
    pub command: String,
    /// The result or output summary.
    pub result: String,
}

/// Loads the existing journal file, tolerating malformed lines.
fn load_existing(path: &Path) -> Vec<JournalEntry> {
    let Ok(metadata) = fs::metadata(path) else {
        return Vec::new();
    };
    if metadata.len() > MAX_JOURNAL_BYTES {
        // An oversized journal is reported rather than materialized in memory.
        eprintln!(
            "warning: session journal at `{}` is larger than {} bytes and will not be listed; \
               truncate it to restore `journal` output",
            path.display(),
            MAX_JOURNAL_BYTES
        );
        return Vec::new();
    }
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<JournalEntry>(line).ok())
        .collect()
}

/// Manages the REPL session journal with append-only disk persistence.
pub struct SessionJournal {
    /// Path to the journal file relative to the project root.
    journal_path: PathBuf,
    /// In-memory cache of entries (existing plus this session's).
    entries: Mutex<Vec<JournalEntry>>,
}

impl SessionJournal {
    /// Creates a new session journal for the given project root, loading any
    /// existing journal so the history is append-only across sessions.
    pub fn new(project_root: &Path) -> Self {
        let journal_path = project_root.join(SESSION_JOURNAL);
        Self {
            journal_path: journal_path.clone(),
            entries: Mutex::new(load_existing(&journal_path)),
        }
    }

    /// Appends an entry to the journal and persists it immediately.
    pub fn append(&self, entry: JournalEntry) {
        if let Ok(mut guard) = self.entries.lock() {
            guard.push(entry.clone());
        }
        self.append_line(&entry);
    }

    /// Writes one entry as a single JSONL line using an append open.
    fn append_line(&self, entry: &JournalEntry) {
        let Ok(mut line) = serde_json::to_string(entry) else {
            return;
        };
        line.push('\n');
        if let Some(parent) = self.journal_path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!("warning: could not create journal directory: {error}");
            return;
        }
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.journal_path)
        {
            Ok(mut file) => {
                if file
                    .write_all(line.as_bytes())
                    .and_then(|_| file.flush())
                    .is_err()
                {
                    eprintln!("warning: could not append to session journal");
                }
            }
            Err(error) => eprintln!("warning: could not open session journal: {error}"),
        }
    }

    /// Returns a copy of all known entries (previous sessions plus this one).
    pub fn entries(&self) -> Vec<JournalEntry> {
        self.entries
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

/// Displays the session journal entries through the shared pager.
pub fn display_session_status(journal: &SessionJournal) {
    let entries = journal.entries();
    if entries.is_empty() {
        println!("Session journal: (empty)");
        return;
    }
    let mut text = format!("Session journal ({} entries):\n", entries.len());
    for (i, entry) in entries.iter().enumerate() {
        text.push_str(&format!(
            "  {}. [{}] {}\n",
            i + 1,
            entry.timestamp,
            entry.command
        ));
        if !entry.result.is_empty() {
            text.push_str(&format!("     -> {}\n", entry.result));
        }
    }
    display_output(&text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn entry(command: &str, result: &str) -> JournalEntry {
        JournalEntry {
            timestamp: "2026-09-10T12:00:00Z".into(),
            command: command.into(),
            result: result.into(),
        }
    }

    #[test]
    fn journal_creation_and_append_persists_immediately() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        assert!(journal.entries().is_empty());

        journal.append(entry("task run . write-tests", "ok"));

        assert_eq!(journal.entries().len(), 1);
        let log_file = dir.path().join(SESSION_JOURNAL);
        assert!(log_file.exists());
        let contents = fs::read_to_string(&log_file).unwrap();
        assert_eq!(contents.lines().count(), 1);
        let parsed: JournalEntry = serde_json::from_str(contents.lines().next().unwrap()).unwrap();
        assert_eq!(parsed, entry("task run . write-tests", "ok"));
    }

    #[test]
    fn journal_is_append_only_across_sessions() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        journal.append(entry("status", "ok"));

        // A second "session" over the same project root loads the prior entry
        // and appends beside it without rewriting existing lines.
        let journal2 = SessionJournal::new(dir.path());
        assert_eq!(journal2.entries().len(), 1);
        journal2.append(entry("task next .", "write-tests"));

        let contents = fs::read_to_string(dir.path().join(SESSION_JOURNAL)).unwrap();
        assert_eq!(contents.lines().count(), 2);
        assert_eq!(
            contents.lines().next().unwrap(),
            serde_json::to_string(&entry("status", "ok")).unwrap()
        );
        assert_eq!(journal2.entries().len(), 2);
    }

    #[test]
    fn journal_loading_tolerates_malformed_and_truncated_lines() {
        let dir = tempdir().unwrap();
        let log_file = dir.path().join(SESSION_JOURNAL);
        fs::create_dir_all(log_file.parent().unwrap()).unwrap();
        fs::write(
            &log_file,
            "not-json\n\n{\"timestamp\":\"2026-09-10T12:00:00Z\",\"command\":\"ok\",\"result\":\"ok\"}\n{\"timestamp\":\"trunc",
        )
        .unwrap();

        let journal = SessionJournal::new(dir.path());
        assert_eq!(journal.entries().len(), 1);
        assert_eq!(journal.entries()[0].command, "ok");
    }

    #[test]
    fn display_session_status_handles_empty_and_populated() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        display_session_status(&journal);

        journal.append(entry("status", "ok"));
        display_session_status(&journal);
    }
}
