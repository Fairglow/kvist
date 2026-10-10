//! Session and file display for Blad.
//!
//! Parses Markdown message bodies and applies syntax highlighting to fenced
//! code blocks. Unsupported languages (e.g. hemlock) fall back to plain text
//! without crashing.

use std::io::{self, Write};

use crate::markdown::render_markdown;
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
    let mut stdout = io::stdout();
    for msg in &messages {
        print_message(msg, &mut stdout).unwrap();
    }
}

/// Dump a completed session transcript to plain text (no ANSI).
pub fn dump_session(session: &Session) {
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
    let mut stdout = io::stdout();
    for msg in &messages {
        dump_message(msg, &mut stdout).unwrap();
    }
}

/// Stream a live or recently-completed session.
pub fn stream_session(session: &Session) {
    let messages = load_messages(session);
    let mut stdout = io::stdout();
    for msg in &messages {
        print_message(msg, &mut stdout).unwrap();
    }

    if !session.completed {
        println!("\n(session still active - re-run to see updates)");
    }
}

/// View a standalone markdown file interactively.
pub fn view_markdown(content: &str) {
    // Render markdown to a string first, then display interactively
    let mut output = String::new();
    render_markdown(content, &mut output).unwrap();

    // Display interactively with navigation
    interactive_viewer(&output);
}

/// Dump a standalone markdown file to plain text (no ANSI).
pub fn dump_markdown(content: &str) {
    // Render markdown to a string, output directly
    let mut output = String::new();
    render_markdown(content, &mut output).unwrap();
    print!("{}", output);
}

/// Interactive viewer with navigation (up/down arrows, page up/down).
fn interactive_viewer(content: &str) {
    use crossterm::event::{self, Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind};
    use crossterm::execute;
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};

    // Split content into lines
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // Terminal dimensions
    let (cols, rows) = match crossterm::terminal::size() {
        Ok(s) => s,
        Err(_) => return,
    };

    let mut scroll_offset = 0;
    let visible_lines = (rows - 2) as usize; // Reserve 2 rows for header/footer

    // Enter alternate screen
    enable_raw_mode().expect("failed to enable raw mode");
    execute!(io::stdout(), EnterAlternateScreen).expect("failed to enter alternate screen");

    loop {
        // Calculate visible range
        let start = scroll_offset;
        let end = (scroll_offset + visible_lines).min(total_lines);

        // Clear and redraw
        execute!(io::stdout(), crossterm::Clear(crossterm::ClearType::All)).expect("failed to clear");
        execute!(io::stdout(), crossterm::cursor::MoveTo(0, 0)).expect("failed to move cursor");

        // Header
        println!("=== Blad Markdown Viewer ===");

        // Render visible lines with soft wrapping
        for i in start..end {
            if i < total_lines {
                let line = lines[i];
                // Soft wrap: split line at terminal width
                let remaining = cols as usize - 1;
                if line.len() > remaining {
                    let wrapped = soft_wrap(line, remaining);
                    for wline in wrapped {
                        println!("{}", wline);
                    }
                } else {
                    println!("{}", line);
                }
            }
        }

        // Footer with scroll position
        let pct = if total_lines > 0 {
            (scroll_offset as f64 / total_lines as f64) * 100.0
        } else {
            0.0
        };
        println!("{}%", pct);

        // Event handling
        match event::read() {
            Ok(Event::Key(KeyEvent { code, .. })) => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if scroll_offset > 0 { scroll_offset -= 1; }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if scroll_offset < total_lines.saturating_sub(1) { scroll_offset += 1; }
                }
                KeyCode::PageUp | KeyCode::Char('u') => {
                    scroll_offset = scroll_offset.saturating_sub(visible_lines);
                }
                KeyCode::PageDown | KeyCode::Char('d') => {
                    scroll_offset = (scroll_offset + visible_lines).min(total_lines.saturating_sub(1));
                }
                KeyCode::Char('g') | KeyCode::Char('H') => {
                    scroll_offset = 0;
                }
                KeyCode::Char('G') | KeyCode::Char('L') => {
                    scroll_offset = total_lines.saturating_sub(1);
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    break;
                }
                _ => {}
            },
            Ok(Event::Mouse(MouseEvent { kind, .. })) => match kind {
                MouseEventKind::ScrollUp => {
                    if scroll_offset > 0 { scroll_offset -= 1; }
                }
                MouseEventKind::ScrollDown => {
                    if scroll_offset < total_lines.saturating_sub(1) { scroll_offset += 1; }
                }
                _ => {}
            },
            Ok(_) => {}
            Err(_) => break,
        }
    }

    disable_raw_mode().expect("failed to disable raw mode");
    execute!(io::stdout(), LeaveAlternateScreen).expect("failed to leave alternate screen");
}

/// Soft-wrap a line to fit within the given width.
fn soft_wrap(line: &str, width: usize) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();

    for word in line.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            result.push(current);
            current = word.to_string();
        }
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
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
fn print_message(msg: &Message, w: &mut impl Write) -> std::io::Result<()> {
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

    writeln!(w, "--- {}{} ---", kind_label, disposition)?;
    // Parse markdown and highlight fenced code blocks
    render_markdown(&msg.content, w)?;
    if !msg.content.ends_with('\n') {
        writeln!(w)?;
    }
    Ok(())
}

/// Dump a single message to plain text (no ANSI).
fn dump_message(msg: &Message, w: &mut impl Write) -> std::io::Result<()> {
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

    writeln!(w, "--- {}{} ---", kind_label, disposition)?;
    // Output content as-is (no ANSI)
    write!(w, "{}", msg.content)?;
    if !msg.content.ends_with('\n') {
        writeln!(w)?;
    }
    Ok(())
}
