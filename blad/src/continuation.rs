//! Session continuation with context import.

use std::fs;
use std::io;
use std::path::Path;

use crate::session_manager::SessionManager;

/// Import context from a previous session for continuation.
pub fn import_session_context(base_dir: &Path, session_id: &str) -> io::Result<String> {
    let session = SessionManager::load(base_dir, session_id)?;
    let manifest = session.manifest();

    // Collect user prompts and final assistant answers
    let mut user_prompts = Vec::new();
    let mut assistant_answers = Vec::new();
    let mut total_bytes = 0;

    for entry in &manifest.entries {
        if !entry.complete {
            continue;
        }

        let message_path = session.session_dir().join(&entry.path);
        let content = fs::read_to_string(&message_path)?;
        total_bytes += content.len();

        match entry.kind.as_str() {
            "user" => {
                user_prompts.push(content.trim().to_string());
            }
            "assistant" => {
                if entry.disposition == Some("final".to_string()) {
                    assistant_answers.push(content.trim().to_string());
                }
            }
            _ => {}
        }
    }

    // Estimate token count (rough: 1 token per 4 bytes)
    let estimated_tokens = total_bytes / 4;

    // Build continuation context
    let mut context = format!(
        "Session continuation context:\n\
         - Original session: {}\n\
         - Original path: {}\n\
         - Original size: {} tokens (estimated)\n\
         - Imported: {} user prompts, {} assistant answers\n\n",
        session_id,
        session.session_dir().display(),
        estimated_tokens,
        user_prompts.len(),
        assistant_answers.len()
    );

    // Add user prompts and assistant answers
    for (i, prompt) in user_prompts.iter().enumerate() {
        context.push_str(&format!("User prompt {}:\n{}\n\n", i + 1, prompt));
    }

    for (i, answer) in assistant_answers.iter().enumerate() {
        context.push_str(&format!("Assistant answer {}:\n{}\n\n", i + 1, answer));
    }

    Ok(context)
}
