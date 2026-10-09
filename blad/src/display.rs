//! Session and file display for Blad.

use std::io::{self, Write};

use crate::session::{Message, Session, load_messages};

/// Show a completed session transcript.
pub fn show_session(session: &Session) {
    println!("Session: {}", session.id);
    if let Some(model) = &session.model {
        println!("Model: {}", model);
    }
    if let Some(created_at) = &session.created_at {
        println!("Started: {}", created_at);
    }
    println!("Messages: {}", session.message_count);
    println!();

    let messages = load_messages(session);
    for msg in &messages {
        print_message(msg);
    }
}

/// Stream a live or recently-completed session.
pub fn stream_session(session: &Session) {
    let messages = load_messages(session);
    for msg in &messages {
        print_message(msg);
    }

    if !session.completed {
        println!("\n(session still active - re-run to see updates)");
    }
}

/// View a standalone markdown file.
pub fn view_markdown(content: &str) {
    print!("{}", content);
}

/// Export a session to a combined markdown document.
pub fn export_session(session: &Session, output: Option<&std::path::Path>) {
    let mut doc = String::new();
    doc.push_str("# Session Transcript\n\n");
    doc.push_str(&format!("Session: {}\n", session.id));
    if let Some(model) = &session.model {
        doc.push_str(&format!("Model: {}\n", model));
    }
    if let Some(created_at) = &session.created_at {
        doc.push_str(&format!("Started: {}\n", created_at));
    }
    doc.push_str("\n---\n\n");

    let messages = load_messages(session);
    for msg in &messages {
        let kind_label = match msg.kind.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            "reasoning" => "Thinking",
            "tool_dispatch" => "Tool Call",
            "tool_result" => "Tool Result",
            "notice" => "Notice",
            _ => &msg.kind,
        };
        doc.push_str(&format!("## {}\n\n", kind_label));
        doc.push_str(&msg.content);
        doc.push_str("\n\n---\n\n");
    }

    match output {
        Some(path) => {
            let mut file = std::fs::File::create(path).expect("failed to create output file");
            file.write_all(doc.as_bytes())
                .expect("failed to write output file");
            println!("Exported to {}", path.display());
        }
        None => {
            io::stdout()
                .write_all(doc.as_bytes())
                .expect("failed to write output");
        }
    }
}

/// Print a single message with its label.
fn print_message(msg: &Message) {
    let kind_label = match msg.kind.as_str() {
        "user" => "User",
        "assistant" => "Assistant",
        "reasoning" => "Thinking",
        "tool_dispatch" => "Tool Call",
        "tool_result" => "Tool Result",
        "notice" => "Notice",
        _ => &msg.kind,
    };

    let disposition = match &msg.disposition {
        Some(d) if d == "final" => " (final)",
        Some(d) if d == "interim" => " (interim)",
        _ => "",
    };

    println!("--- {}{} ---", kind_label, disposition);
    println!("{}", msg.content);
    println!();
}
