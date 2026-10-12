//! Session and file display for Blad.
//!
//! Parses Markdown message bodies and applies syntax highlighting to fenced
//! code blocks. Unsupported languages (e.g. hemlock) fall back to plain text
//! without crashing.

use std::io::{self, Write};
use std::os::linux::fs::MetadataExt;

use crate::markdown::{render_markdown, render_markdown_width};
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

/// Check if stdout is a TTY (interactive terminal).
fn is_tty() -> bool {
    match std::fs::metadata("/dev/stdout") {
        Ok(meta) => meta.st_mode() & 0o170000 == 0o020000,
        Err(_) => false,
    }
}

/// Check if stdin is a TTY (interactive terminal).
fn is_stdin_tty() -> bool {
    match std::fs::metadata("/dev/stdin") {
        Ok(meta) => meta.st_mode() & 0o170000 == 0o020000,
        Err(_) => false,
    }
}

/// View a standalone markdown file interactively.
pub fn view_markdown(content: &str) {
    view_markdown_file(None, content);
}

/// View a markdown file with optional edit support.
pub fn view_markdown_file(path: Option<&std::path::Path>, content: &str) {
    // Pass original markdown to interactive viewer - it handles rendering internally

    if is_tty() && is_stdin_tty() {
        interactive_viewer(path, content);
    } else {
        // Non-interactive: render to stdout and exit
        dump_markdown(content);
    }
}

/// View a markdown file with terminal width awareness.
pub fn view_markdown_width(path: Option<&std::path::Path>, content: &str, _terminal_width: usize) {
    // Pass original markdown to interactive viewer - it handles rendering internally
    if is_tty() {
        let (cols, _rows) = match crossterm::terminal::size() {
            Ok(s) => s,
            Err(_) => {
                dump_markdown(content);
                return;
            }
        };
        let render_width = cols.saturating_sub(1) as usize;
        let mut output = Vec::new();
        render_markdown_width(content, &mut output, Some(render_width)).unwrap();
        let rendered = String::from_utf8(output).unwrap();
        interactive_viewer(path, &rendered);
    } else {
        // Non-interactive: render to stdout and exit
        dump_markdown(content);
    }
}

/// Dump a standalone markdown file to plain text (no ANSI).
pub fn dump_markdown(content: &str) {
    // Render markdown to a buffer, output directly
    let mut output = Vec::new();
    render_markdown(content, &mut output).unwrap();
    let rendered = String::from_utf8(output).unwrap();
    print!("{}", rendered);
}

/// Interactive viewer with navigation and search.
fn interactive_viewer(path: Option<&std::path::Path>, content: &str) {
    use crossterm::event::{self, Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind};
    use crossterm::execute;
    use crossterm::terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    };

    // Terminal dimensions
    let (cols, rows) = match crossterm::terminal::size() {
        Ok(s) => s,
        Err(_) => return,
    };

    // Render markdown with terminal width awareness for proper table wrapping
    // Subtract 1 for the scrollbar column
    let render_width = cols.saturating_sub(1) as usize;
    let mut output = Vec::new();
    render_markdown_width(content, &mut output, Some(render_width)).unwrap();
    let rendered = String::from_utf8(output).unwrap();
    let lines: Vec<&str> = rendered.lines().collect();
    let total_lines = lines.len();

    let mut scroll_offset = 0;
    let mut scroll_x = 0; // Horizontal scroll offset
    let visible_lines = (rows - 2) as usize; // Reserve 2 rows for header/footer
    let scrollbar_col = cols - 1;

    // Search state
    let mut search_mode = false;
    let mut search_query = String::new();
    let mut search_matches: Vec<usize> = Vec::new();
    let mut search_index = 0;

    // Enter alternate screen
    enable_raw_mode().expect("failed to enable raw mode");
    execute!(io::stdout(), EnterAlternateScreen).expect("failed to enter alternate screen");
    // Hide cursor
    write!(io::stdout(), "\x1b[?25l").expect("failed to hide cursor");
    io::stdout().flush().expect("failed to flush output");

    fn find_matches(lines: &[&str], query: &str) -> Vec<usize> {
        if query.is_empty() {
            return Vec::new();
        }
        let mut matches = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            // Strip ANSI escape codes for searching
            let clean = strip_ansi(line);
            if clean.to_lowercase().contains(&query.to_lowercase()) {
                matches.push(i);
            }
        }
        matches
    }

    fn strip_ansi(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip ESC sequence until 'm'
                for nc in chars.by_ref() {
                    if nc == 'm' {
                        break;
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
    }

    loop {
        // Calculate visible range (in logical lines)
        let start = scroll_offset;
        let end = (scroll_offset + visible_lines).min(total_lines);

        // Clear and redraw
        execute!(io::stdout(), Clear(ClearType::All)).expect("failed to clear");
        execute!(io::stdout(), crossterm::cursor::MoveTo(0, 0)).expect("failed to move cursor");
        io::stdout().flush().expect("failed to flush after clear");

        // Header
        let header = if search_mode || (!search_query.is_empty() && !search_matches.is_empty()) {
            format!("=== Search: {} ===", search_query)
        } else if let Some(p) = path {
            format!("=== Blad: {} ===", p.display())
        } else {
            "=== Blad Markdown Viewer ===".to_string()
        };
        write!(io::stdout(), "{}\r\n", header).expect("failed to write header");

        // Render visible lines with search highlighting and line numbers
        let visible = &lines[start..end];
        let max_line_digits = total_lines.to_string().len();
        for (i, line) in visible.iter().enumerate() {
            let actual_line = start + i + 1; // 1-indexed
            let line_prefix = format!("{:>width$} ", actual_line, width = max_line_digits);

            if search_matches.contains(&(start + i)) {
                // Highlight matching line
                execute!(
                    io::stdout(),
                    crossterm::style::SetForegroundColor(crossterm::style::Color::Yellow),
                    crossterm::style::SetBackgroundColor(crossterm::style::Color::Blue)
                )
                .expect("failed to set colors");
            }

            // Apply horizontal scroll by slicing the line
            let line_chars: Vec<char> = line.chars().collect();
            let max_x = line_chars.len().saturating_sub((cols - 5) as usize);
            if scroll_x > max_x {
                scroll_x = max_x;
            }
            let visible_chars: String = line_chars[scroll_x..].iter().collect();

            // Print line with line number
            write!(io::stdout(), "{}{}\r\n", line_prefix, visible_chars)
                .expect("failed to write line");

            execute!(io::stdout(), crossterm::style::ResetColor).expect("failed to reset colors");

            // Pad empty lines to ensure footer is in fixed position
            if i == visible.len() - 1 && end < total_lines {
                for _ in 0..(visible_lines - visible.len()) {
                    println!();
                }
            }
        }

        // Position footer in fixed position at bottom row
        execute!(io::stdout(), crossterm::cursor::MoveTo(0, rows - 1))
            .expect("failed to move cursor");

        // Calculate meaningful percentage based on scroll position
        let pct = if total_lines > visible_lines {
            ((scroll_offset as f64) / ((total_lines - visible_lines) as f64)) * 100.0
        } else {
            100.0
        };

        let search_info = if search_mode && !search_matches.is_empty() {
            format!(
                " {}/{}/{} matches ",
                search_index + 1,
                search_matches.len(),
                search_query
            )
        } else {
            "".to_string()
        };

        let line_info = format!(" L{}-{} of {}", start + 1, end, total_lines);
        let _ = write!(
            io::stdout(),
            "  [{:5.1}%]{}{}{}",
            pct,
            line_info,
            search_info,
            if scroll_offset == 0 {
                " TOP"
            } else if end >= total_lines {
                " BOTTOM"
            } else {
                " SCROLL"
            }
        );
        execute!(io::stdout(), crossterm::cursor::MoveTo(0, 0)).expect("failed to move cursor");

        // Draw scrollbar on the right
        if total_lines > visible_lines {
            let scrollbar_size = visible_lines;
            let thumb_size = if scrollbar_size > 0 {
                ((visible_lines as f64) / (total_lines as f64)) * scrollbar_size as f64
            } else {
                0.0
            };
            let thumb_top = ((scroll_offset as f64) / (total_lines as f64)) * scrollbar_size as f64;

            // Draw scrollbar track
            for i in 0..scrollbar_size {
                let y = i + 1; // Row offset (after header)
                if (i as f64) >= thumb_top && (i as f64) < thumb_top + thumb_size {
                    // Thumb
                    execute!(
                        io::stdout(),
                        crossterm::style::SetForegroundColor(crossterm::style::Color::White)
                    )
                    .expect("failed to set color");
                    let _ = write!(io::stdout(), "█");
                    execute!(
                        io::stdout(),
                        crossterm::style::SetForegroundColor(crossterm::style::Color::Reset)
                    )
                    .expect("failed to reset color");
                } else {
                    // Track
                    let _ = write!(io::stdout(), "░");
                }
                // Move to next row of scrollbar
                execute!(
                    io::stdout(),
                    crossterm::cursor::MoveTo(scrollbar_col, (y + 1) as u16)
                )
                .expect("failed to move cursor");
            }
        }

        // Event handling
        match event::read() {
            Ok(Event::Key(KeyEvent { code, .. })) => {
                if search_mode {
                    match code {
                        KeyCode::Char(c) => {
                            search_query.push(c);
                            search_matches = find_matches(&lines, &search_query);
                            if !search_matches.is_empty() {
                                search_index = 0;
                                scroll_to_match(
                                    scroll_offset,
                                    search_matches[search_index],
                                    visible_lines,
                                );
                            }
                        }
                        KeyCode::Backspace => {
                            search_query.pop();
                            search_matches = find_matches(&lines, &search_query);
                            if !search_matches.is_empty() {
                                search_index = 0;
                                scroll_to_match(
                                    scroll_offset,
                                    search_matches[search_index],
                                    visible_lines,
                                );
                            }
                        }
                        KeyCode::Enter => {
                            search_mode = false;
                            if search_matches.is_empty() {
                                search_query.clear();
                            }
                        }
                        KeyCode::Esc => {
                            search_mode = false;
                            search_query.clear();
                            search_matches.clear();
                        }
                        _ => {}
                    }
                } else {
                    match code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            scroll_offset = scroll_offset.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if scroll_offset < total_lines.saturating_sub(visible_lines) {
                                scroll_offset += 1;
                            }
                        }
                        KeyCode::PageUp | KeyCode::Char('u') => {
                            scroll_offset = scroll_offset.saturating_sub(visible_lines);
                        }
                        KeyCode::PageDown | KeyCode::Char('d') => {
                            scroll_offset = (scroll_offset + visible_lines)
                                .min(total_lines.saturating_sub(visible_lines));
                        }
                        KeyCode::Home => {
                            scroll_offset = 0;
                            scroll_x = 0;
                        }
                        KeyCode::End => {
                            scroll_offset = total_lines.saturating_sub(visible_lines);
                        }
                        KeyCode::Left => {
                            scroll_x = scroll_x.saturating_sub(1);
                        }
                        KeyCode::Right => {
                            scroll_x += 1;
                        }
                        KeyCode::Char('g') | KeyCode::Char('H') => {
                            scroll_offset = 0;
                            scroll_x = 0;
                        }
                        KeyCode::Char('G') | KeyCode::Char('L') => {
                            scroll_offset = total_lines.saturating_sub(visible_lines);
                        }
                        KeyCode::Char('/') => {
                            search_mode = true;
                            search_query.clear();
                            search_matches.clear();
                        }
                        KeyCode::Char('n') if !search_matches.is_empty() => {
                            search_index = (search_index + 1).min(search_matches.len() - 1);
                            scroll_to_match(
                                scroll_offset,
                                search_matches[search_index],
                                visible_lines,
                            );
                        }
                        KeyCode::Char('p') if !search_matches.is_empty() => {
                            search_index = search_index.saturating_sub(1);
                            scroll_to_match(
                                scroll_offset,
                                search_matches[search_index],
                                visible_lines,
                            );
                        }
                        KeyCode::Char('e') if path.is_some() => {
                            // Edit mode: exit viewer, edit file, re-render
                            disable_raw_mode().expect("failed to disable raw mode");
                            execute!(io::stdout(), LeaveAlternateScreen)
                                .expect("failed to leave alternate screen");
                            edit_and_rerender(path.unwrap(), &lines);
                            return;
                        }
                        KeyCode::Char('q') | KeyCode::Esc => {
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Mouse(MouseEvent { kind, .. })) => match kind {
                MouseEventKind::ScrollUp => {
                    scroll_offset = scroll_offset.saturating_sub(1);
                }
                MouseEventKind::ScrollDown
                    if scroll_offset < total_lines.saturating_sub(visible_lines) =>
                {
                    scroll_offset += 1;
                }
                _ => {}
            },
            Ok(_) => {}
            Err(_) => break,
        }
    }

    // Show cursor before leaving
    write!(io::stdout(), "\x1b[?25h").expect("failed to show cursor");
    io::stdout().flush().expect("failed to flush output");
    disable_raw_mode().expect("failed to disable raw mode");
    execute!(io::stdout(), LeaveAlternateScreen).expect("failed to leave alternate screen");
}

fn scroll_to_match(current_offset: usize, target_line: usize, visible_lines: usize) -> usize {
    if target_line < current_offset {
        target_line
    } else if target_line >= current_offset + visible_lines {
        target_line - visible_lines / 2
    } else {
        current_offset
    }
}

fn edit_and_rerender(path: &std::path::Path, _lines: &[&str]) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());
    let args: Vec<&str> = editor.split_whitespace().collect();
    let result = std::process::Command::new(args[0])
        .args(&args[1..])
        .arg(path)
        .status();

    match result {
        Ok(status) if status.success() => {
            // Re-read and re-render
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    let mut output = Vec::new();
                    render_markdown(&content, &mut output).unwrap();
                    let rendered = String::from_utf8(output).unwrap();
                    interactive_viewer(Some(path), &rendered);
                }
                Err(e) => eprintln!("Failed to re-read file: {}", e),
            }
        }
        Ok(status) => eprintln!("Editor exited with status: {}", status),
        Err(e) => eprintln!("Failed to run editor: {}", e),
    }
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
