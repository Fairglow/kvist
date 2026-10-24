use std::io::{self, Write};

use crate::RenderOptions;
use crate::markdown::{MarkdownRenderer, inert_source};
use crate::session::{Message, Session, load_messages};

/// List session metadata without interpreting authored terminal controls.
pub fn list_sessions(sessions: &[Session]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    for session in sessions {
        writeln!(
            stdout,
            "{} - {} - {} messages - {}",
            inert_source(&session.id),
            inert_source(session.model.as_deref().unwrap_or("unknown")),
            session.message_count,
            if session.completed {
                "completed"
            } else {
                "active"
            }
        )?;
    }
    Ok(())
}

/// Show the current session snapshot with default rendering options.
pub fn show_session(session: &Session) -> io::Result<()> {
    show_session_with_options(session, &RenderOptions::default())
}

/// Render a session snapshot using explicit host settings.
pub fn show_session_with_options(session: &Session, options: &RenderOptions) -> io::Result<()> {
    let messages = load_messages(session)?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "Session: {}", inert_source(&session.id))?;
    if let Some(model) = &session.model {
        writeln!(stdout, "Model: {}", inert_source(model))?;
    }
    if let Some(created_at) = &session.created_at {
        writeln!(stdout, "Started: {}", inert_source(created_at))?;
    }
    writeln!(stdout, "Messages: {}\n", session.message_count)?;
    let renderer = MarkdownRenderer::with_options(options.clone());
    for message in &messages {
        print_message(message, &mut stdout, &renderer)?;
    }
    if !session.completed {
        writeln!(
            stdout,
            "(session active; snapshot only - re-run to see updates)"
        )?;
    }
    Ok(())
}

/// Dump the current session snapshot without terminal escapes.
pub fn dump_session(session: &Session) -> io::Result<()> {
    show_session_with_options(session, &RenderOptions::plain())
}

/// Export a session to a combined source Markdown document.
pub fn export_session(session: &Session, output: Option<&std::path::Path>) -> io::Result<()> {
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
    for message in load_messages(session)? {
        doc.push_str(&format!(
            "## {}\n\n",
            inert_source(message_label(&message.kind))
        ));
        doc.push_str(&message.content);
        if !message.content.ends_with('\n') {
            doc.push('\n');
        }
        doc.push_str("\n---\n\n");
    }
    match output {
        Some(path) => {
            std::fs::File::create(path)?.write_all(doc.as_bytes())?;
            println!("Exported to {}", inert_source(&path.display().to_string()));
        }
        None => io::stdout().write_all(doc.as_bytes())?,
    }
    Ok(())
}

fn message_label(kind: &str) -> &str {
    match kind {
        "user" => "User",
        "assistant" => "Assistant",
        "reasoning" => "Thinking",
        "tool_dispatch" => "Tool Call",
        "tool_result" => "Tool Result",
        "notice" => "Notice",
        _ => kind,
    }
}

fn print_message(
    message: &Message,
    writer: &mut impl Write,
    renderer: &MarkdownRenderer,
) -> io::Result<()> {
    let disposition = match message.disposition.as_deref() {
        Some("final") => " [final]",
        Some("interim") => " [interim]",
        _ => "",
    };
    writeln!(
        writer,
        "### {}{disposition}",
        inert_source(message_label(&message.kind))
    )?;
    renderer.render_markdown(&message.content, writer)?;
    writeln!(writer)?;
    Ok(())
}
