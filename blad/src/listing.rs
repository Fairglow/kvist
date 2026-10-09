//! Session listing.

use std::fs;
use std::path::Path;

use crate::session_manager::SessionManifest;

/// Run the list sessions command.
pub fn run_list_sessions(limit: usize) {
    let base_dir = Path::new(".blad/sessions");

    if !base_dir.exists() {
        println!("No sessions found.");
        return;
    }

    // Read all session directories
    let mut sessions = Vec::new();

    let dir_iter = match fs::read_dir(base_dir) {
        Ok(d) => d,
        Err(_) => {
            println!("No sessions found.");
            return;
        }
    };
    for entry in dir_iter {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let manifest_path = path.join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }

        let json = match fs::read_to_string(&manifest_path) {
            Ok(j) => j,
            Err(_) => continue,
        };

        let manifest: SessionManifest = match serde_json::from_str(&json) {
            Ok(m) => m,
            Err(_) => continue,
        };

        sessions.push((path, manifest));
    }

    // Sort by created_at (newest first)
    sessions.sort_by(|a, b| b.1.created_at.cmp(&a.1.created_at));

    // Show limit
    let shown = sessions.iter().take(limit);
    for (path, manifest) in shown {
        let status = match manifest.status {
            crate::session_manager::SessionStatus::Completed => "completed",
            crate::session_manager::SessionStatus::Cancelled => "cancelled",
            crate::session_manager::SessionStatus::Failed => "failed",
            crate::session_manager::SessionStatus::Streaming => "streaming",
        };

        println!(
            "{} - {} - {} messages - {}",
            manifest.session_id, manifest.created_at, manifest.entry_count, status
        );
    }

    if sessions.len() > limit {
        println!("... and {} more", sessions.len() - limit);
    }
}
