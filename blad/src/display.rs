//! Session display.

use std::fs;
use std::path::Path;

/// Run the show session command.
pub fn run_show_session(session_id: &str, message: Option<usize>) {
    let base_dir = Path::new(".blad/sessions");
    let session_path = base_dir.join(session_id);

    if !session_path.exists() {
        println!("Session not found: {}", session_id);
        return;
    }

    // Read manifest
    let manifest_path = session_path.join("manifest.json");
    let json = fs::read_to_string(&manifest_path).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&json).unwrap();

    let session_id = manifest["session_id"].as_str().unwrap();
    let status = manifest["status"].as_str().unwrap();
    let created_at = manifest["created_at"].as_str().unwrap();
    let entry_count = manifest["entry_count"].as_u64().unwrap() as usize;

    println!("Session: {}", session_id);
    println!("Status: {}", status);
    println!("Created: {}", created_at);
    println!("Messages: {}", entry_count);
    println!();

    // Show messages
    let entries = manifest["entries"].as_array().unwrap();
    let mut shown = 0;

    for entry in entries {
        let sequence = entry["sequence"].as_u64().unwrap() as usize;
        let kind = entry["kind"].as_str().unwrap();
        let complete = entry["complete"].as_bool().unwrap();

        if let Some(msg_num) = message {
            if sequence != msg_num {
                continue;
            }
        }

        let path = session_path.join(entry["path"].as_str().unwrap());
        let content = fs::read_to_string(&path).unwrap();

        println!(
            "--- Message {} ({}{}) ---",
            sequence,
            kind,
            if complete { "" } else { " (streaming)" }
        );
        println!("{}", content);
        println!();

        shown += 1;
        if let Some(_) = message {
            break;
        }
    }

    if message.is_some() && shown == 0 {
        println!("Message not found: {}", message.unwrap());
    }
}
