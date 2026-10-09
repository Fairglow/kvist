//! Interactive agent session with live streaming and navigation.

use sav::ReasoningEffort;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::session_manager::SessionManager;
use crate::transcript_writer::TranscriptWriter;

/// Run a new interactive session.
pub fn run_new_session(model: Option<String>, effort: Option<String>, cwd: PathBuf, no_logs: bool) {
    if !no_logs {
        let base_dir = PathBuf::from(".blad/sessions");
        std::fs::create_dir_all(&base_dir).unwrap_or_else(|e| {
            println!("Failed to create session directory: {}", e);
        });

        let session = SessionManager::new(&base_dir).unwrap_or_else(|e| {
            println!("Failed to create session: {}", e);
            SessionManager::load(&base_dir, "fallback").unwrap()
        });

        let mut writer = TranscriptWriter::new(session);

        println!("Session started: {}", writer.session().session_id());
        println!("Artifacts: {}", writer.session().session_dir().display());

        run_interactive_loop(&mut writer, model, effort, cwd);

        writer.complete_session().unwrap_or_else(|e| {
            println!("Failed to complete session: {}", e);
        });
    } else {
        println!("Starting new blad session (no logs)...");
        run_interactive_loop_no_logs(model, effort, cwd);
    }
}

/// Run a continued session.
pub fn run_continue_session(session_id: &str, model: Option<String>, effort: Option<String>) {
    let base_dir = PathBuf::from(".blad/sessions");

    // Import context from the original session
    let context = crate::continuation::import_session_context(&base_dir, session_id);

    let session = SessionManager::new(&base_dir).unwrap_or_else(|e| {
        println!("Failed to create session: {}", e);
        SessionManager::load(&base_dir, "fallback").unwrap()
    });

    let mut writer = TranscriptWriter::new(session);

    println!("Continuing session: {}", session_id);
    println!("New session: {}", writer.session().session_id());

    if let Ok(context) = context {
        writer
            .start_message("system", Some("continuation"))
            .unwrap();
        writer.append(&context).unwrap();
        writer.flush_message(true).unwrap();
    }

    run_interactive_loop(&mut writer, model, effort, PathBuf::from("."));

    writer.complete_session().unwrap_or_else(|e| {
        println!("Failed to complete session: {}", e);
    });
}

/// Run the interactive loop with session artifacts.
fn run_interactive_loop(
    writer: &mut TranscriptWriter,
    model: Option<String>,
    effort: Option<String>,
    cwd: PathBuf,
) {
    let effort = match effort.as_deref() {
        Some("low") => ReasoningEffort::Low,
        Some("high") => ReasoningEffort::High,
        _ => ReasoningEffort::Medium,
    };

    let mut turn = 0;

    loop {
        turn += 1;

        print!("[turn {}] You: ", turn);
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim().to_string();

        if input.is_empty() || input == "/exit" || input == "/quit" {
            break;
        }

        writer.start_message("user", None).unwrap();
        writer.append(&input).unwrap();
        writer.flush_message(true).unwrap();

        process_turn(writer, &input, &effort);
    }
}

/// Run the interactive loop without session artifacts.
fn run_interactive_loop_no_logs(model: Option<String>, effort: Option<String>, cwd: PathBuf) {
    let effort = match effort.as_deref() {
        Some("low") => ReasoningEffort::Low,
        Some("high") => ReasoningEffort::High,
        _ => ReasoningEffort::Medium,
    };

    let mut turn = 0;

    loop {
        turn += 1;

        print!("You: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim().to_string();

        if input.is_empty() || input == "/exit" || input == "/quit" {
            break;
        }

        println!("You: {}", input);
    }
}

/// Process a single turn with the model.
fn process_turn(writer: &mut TranscriptWriter, user_input: &str, effort: &ReasoningEffort) {
    writer.start_message("assistant", Some("final")).unwrap();
    writer
        .append(&format!("Assistant response to: {}", user_input))
        .unwrap();
    writer.flush_message(true).unwrap();

    println!("Assistant: (response written to session)");
}
