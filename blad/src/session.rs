//! Skott session transcript reader.
//!
//! Reads Skott's structured session transcripts from .skott/runs/.
//! Each session directory contains a session.json manifest and a messages/
//! subdirectory with .md message body files.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// A session as loaded from disk.
#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub path: PathBuf,
    pub manifest: Value,
    pub model: Option<String>,
    pub completed: bool,
    pub created_at: Option<String>,
    pub message_count: usize,
}

/// List recent sessions from the given log directory.
pub fn list_sessions(log_dir: &Path, limit: usize) -> Vec<Session> {
    let mut sessions = Vec::new();

    if !log_dir.exists() {
        return sessions;
    }

    let entries = match fs::read_dir(log_dir) {
        Ok(entries) => entries,
        Err(_) => return sessions,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Look for session.json in the directory
        let manifest_path = path.join("session.json");
        if !manifest_path.exists() {
            continue;
        }

        let json = match fs::read_to_string(&manifest_path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let manifest: Value = match serde_json::from_str(&json) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let id = manifest
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let model = manifest
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let status = manifest
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let created_at = manifest
            .get("created_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let entry_count = manifest
            .get("entries")
            .and_then(|v| v.as_array())
            .map(|v| v.len())
            .unwrap_or(0);

        let session = Session {
            id,
            path,
            manifest,
            model,
            completed: status == "completed",
            created_at,
            message_count: entry_count,
        };

        sessions.push(session);
    }

    // Sort by created_at (newest first), fall back to id comparison
    sessions.sort_by(|a, b| match (&a.created_at, &b.created_at) {
        (Some(a_ts), Some(b_ts)) => b_ts.cmp(a_ts),
        _ => b.id.cmp(&a.id),
    });

    sessions.truncate(limit);
    sessions
}

/// Load a session by path.
pub fn load_session_by_path(path: &Path) -> Option<Session> {
    if !path.is_dir() {
        return None;
    }

    let manifest_path = path.join("session.json");
    if !manifest_path.exists() {
        return None;
    }

    let json = match fs::read_to_string(&manifest_path) {
        Ok(content) => content,
        Err(_) => return None,
    };

    let manifest: Value = match serde_json::from_str(&json) {
        Ok(m) => m,
        Err(_) => return None,
    };

    let id = manifest
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let model = manifest
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let status = manifest
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let created_at = manifest
        .get("created_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let entry_count = manifest
        .get("entries")
        .and_then(|v| v.as_array())
        .map(|v| v.len())
        .unwrap_or(0);

    Some(Session {
        id,
        path: path.to_path_buf(),
        manifest,
        model,
        completed: status == "completed",
        created_at,
        message_count: entry_count,
    })
}

/// Load a session by ID prefix or full ID.
pub fn load_session(log_dir: &Path, session_id: &str) -> Option<Session> {
    let sessions = list_sessions(log_dir, 1000);

    // Try exact match first
    for s in &sessions {
        if s.id == session_id {
            return Some(s.clone());
        }
    }

    // Fall back to prefix match
    for s in &sessions {
        if s.id.starts_with(session_id) {
            return Some(s.clone());
        }
    }

    None
}

/// Load all messages for a session.
pub fn load_messages(session: &Session) -> Vec<Message> {
    let mut messages = Vec::new();

    let entries = session.manifest.get("entries").and_then(|v| v.as_array());

    if let Some(entries) = entries {
        for entry in entries {
            let seq = entry.get("sequence").and_then(|v| v.as_u64()).unwrap_or(0);
            let kind = entry
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let path = entry
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let disposition = entry
                .get("disposition")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let full_path = session.path.join(&path);
            let content = fs::read_to_string(&full_path).unwrap_or_default();

            messages.push(Message {
                sequence: seq as usize,
                kind,
                disposition,
                content,
            });
        }
    }

    messages
}

/// A single message from a session.
///
/// The sequence field is reserved for potential future display of message
/// ordering but is not currently used in the default display mode.
pub struct Message {
    #[allow(dead_code)]
    pub sequence: usize,
    pub kind: String,
    pub disposition: Option<String>,
    pub content: String,
}
