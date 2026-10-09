//! Session export.

use std::fs;
use std::io::Write;
use std::path::Path;

/// Run the export session command.
pub fn run_export_session(session_id: &str, output: Option<std::path::PathBuf>) {
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
    let created_at = manifest["created_at"].as_str().unwrap();
    let status = manifest["status"].as_str().unwrap();

    let mut content = format!("# Session {}\n\n", session_id);
    content.push_str(&format!("**Created:** {}\n", created_at));
    content.push_str(&format!("**Status:** {}\n\n", status));
    content.push_str("---\n\n");

    // Export all messages
    let entries = manifest["entries"].as_array().unwrap();

    for entry in entries {
        let sequence = entry["sequence"].as_u64().unwrap() as usize;
        let kind = entry["kind"].as_str().unwrap();
        let path = session_path.join(entry["path"].as_str().unwrap());
        let msg_content = fs::read_to_string(&path).unwrap();

        content.push_str(&format!("## Message {} ({})\n\n", sequence, kind));
        content.push_str(&msg_content);
        content.push_str("\n\n---\n\n");
    }

    // Write output
    if let Some(output_path) = output {
        let mut file = fs::File::create(&output_path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        println!("Exported to {}", output_path.display());
    } else {
        print!("{}", content);
    }
}
