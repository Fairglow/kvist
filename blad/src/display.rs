//! Session and file display for Blad.
//!
//! Parses Markdown message bodies and applies syntax highlighting to fenced
//! code blocks. Unsupported languages (e.g. hemlock) fall back to plain text
//! without crashing.

use std::io::{self, IsTerminal, Write};

use crate::markdown::{render_markdown, MarkdownRenderer};
use crate::session::{Message, Session, load_messages};
use crate::terminal_text::TerminalLine;
use crate::viewport::{Document, Frame, paint};

/// Show a completed session transcript.
pub fn show_session(session: &Session) -> io::Result<()> {
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
        print_message(msg, &mut stdout)?;
    }
    Ok(())
}

/// Dump a completed session transcript to plain text (no ANSI).
pub fn dump_session(session: &Session) -> io::Result<()> {
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
        dump_message(msg, &mut stdout)?;
    }
    Ok(())
}

/// Stream a live or recently-completed session.
pub fn stream_session(session: &Session) -> io::Result<()> {
    let messages = load_messages(session);
    let mut stdout = io::stdout();
    for msg in &messages {
        print_message(msg, &mut stdout)?;
    }

    if !session.completed {
        println!("\n(session still active - re-run to see updates)");
    }
    Ok(())
}

/// Check if stdout is a TTY (interactive terminal).
fn is_tty() -> bool {
    io::stdout().is_terminal()
}

/// Check if stdin is a TTY (interactive terminal).
fn is_stdin_tty() -> bool {
    io::stdin().is_terminal()
}

/// View a standalone markdown file interactively.
pub fn view_markdown(content: &str) {
    view_markdown_file(None, content, false);
}

/// View a markdown file with optional edit support.
pub fn view_markdown_file(path: Option<&std::path::Path>, content: &str, line_numbers: bool) {
    // Pass original markdown to interactive viewer - it handles rendering internally
    if is_tty() && is_stdin_tty() {
        interactive_viewer(path, content, line_numbers);
    } else {
        // Non-interactive: render to stdout and exit
        if let Err(error) = dump_markdown(content) {
            eprintln!("Failed to display Markdown: {error}");
        }
    }
}

/// View a markdown file with terminal width awareness.
pub fn view_markdown_width(path: Option<&std::path::Path>, content: &str, terminal_width: usize) {
    if is_tty() && is_stdin_tty() {
        if let Err(error) = run_viewer(path, content, false, Some(terminal_width)) {
            eprintln!("Failed to display Markdown: {error}");
        }
    } else {
        let mut output = Vec::new();
        let mut renderer = MarkdownRenderer::new();
        renderer.set_width(Some(terminal_width));
        if let Err(error) = renderer
            .render_markdown(content, &mut output)
            .and_then(|()| io::stdout().write_all(&output))
        {
            eprintln!("Failed to display Markdown: {error}");
        }
    }
}

/// Dump a standalone markdown file to plain text (no ANSI).
pub fn dump_markdown(content: &str) -> io::Result<()> {
    // Render markdown to a buffer, output directly
    let mut output = Vec::new();
    render_markdown(content, &mut output)?;
    let rendered = String::from_utf8(output)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    io::stdout().write_all(rendered.as_bytes())
}

/// Interactive viewer with navigation and search.
fn interactive_viewer(path: Option<&std::path::Path>, content: &str, line_numbers: bool) {
    if let Err(error) = run_viewer(path, content, line_numbers, None) {
        eprintln!("Failed to display Markdown: {error}");
    }
}

struct TerminalGuard {
    raw: bool,
    alternate: bool,
    hidden: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        let mut guard = Self {
            raw: false,
            alternate: false,
            hidden: false,
        };
        crossterm::terminal::enable_raw_mode()?;
        guard.raw = true;
        guard.alternate = true;
        crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
        guard.hidden = true;
        crossterm::execute!(io::stdout(), crossterm::cursor::Hide)?;
        Ok(guard)
    }

    fn restore(&mut self) -> io::Result<()> {
        let mut error = None;
        if self.hidden {
            match crossterm::execute!(io::stdout(), crossterm::cursor::Show) {
                Ok(()) => self.hidden = false,
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
        if self.alternate {
            match crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen) {
                Ok(()) => self.alternate = false,
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
        if self.raw {
            match crossterm::terminal::disable_raw_mode() {
                Ok(()) => self.raw = false,
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
        error.map_or(Ok(()), Err)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("Failed to restore terminal: {error}");
        }
    }
}

fn run_viewer(
    path: Option<&std::path::Path>,
    content: &str,
    line_numbers: bool,
    width_limit: Option<usize>,
) -> io::Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind};

    let (mut cols, mut rows) = crossterm::terminal::size()?;
    let mut document = Document::render(content, cols, line_numbers, width_limit)?;

    let mut scroll_offset: usize = 0;
    let mut scroll_x: usize = 0;
    let mut visible_lines = usize::from(rows.saturating_sub(2));

    // Search state
    let mut search_mode = false;
    let mut search_query = String::new();
    let mut search_matches: Vec<usize> = Vec::new();
    let mut search_index = 0;

    let mut terminal = TerminalGuard::enter()?;

    fn find_matches(lines: &[TerminalLine], query: &str) -> Vec<usize> {
        if query.is_empty() {
            return Vec::new();
        }
        let mut matches = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if line.plain().to_lowercase().contains(&query.to_lowercase()) {
                matches.push(i);
            }
        }
        matches
    }

    loop {
        let lines = &document.lines;
        let total_lines = lines.len();
        scroll_offset = scroll_offset.min(total_lines.saturating_sub(visible_lines));
        scroll_x = scroll_x.min(document.max_x());
        let start = scroll_offset;
        let end = scroll_offset.saturating_add(visible_lines).min(total_lines);

        // Header
        let header = if search_mode || (!search_query.is_empty() && !search_matches.is_empty()) {
            format!("=== Search: {} ===", search_query)
        } else if let Some(p) = path {
            format!("=== Blad: {} ===", p.display())
        } else {
            "=== Blad Markdown Viewer ===".to_string()
        };

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

        let line_info = format!(
            " L{}-{} of {}",
            if total_lines == 0 { 0 } else { start + 1 },
            end,
            total_lines
        );
        let footer = format!(
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
        paint(
            &mut io::stdout().lock(),
            &document,
            Frame {
                columns: cols,
                rows,
                y: scroll_offset,
                x: scroll_x,
                header: &header,
                footer: &footer,
                matches: &search_matches,
            },
        )?;

        // Event handling
        match event::read() {
            Ok(Event::Key(KeyEvent { code, .. })) => {
                if search_mode {
                    match code {
                        KeyCode::Char(c) => {
                            search_query.push(c);
                            search_matches = find_matches(lines, &search_query);
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
                            search_matches = find_matches(lines, &search_query);
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
                            scroll_x = scroll_x.saturating_add(1).min(document.max_x());
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
                            terminal.restore()?;
                            if let Some(path) = path {
                                edit_and_rerender(path, line_numbers, width_limit);
                            }
                            return Ok(());
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
            Ok(Event::Resize(columns, height)) => {
                cols = columns;
                rows = height;
                visible_lines = usize::from(rows.saturating_sub(2));
                document = Document::render(content, cols, line_numbers, width_limit)?;
                search_matches = find_matches(&document.lines, &search_query);
                search_index = search_index.min(search_matches.len().saturating_sub(1));
            }
            Ok(_) => {}
            Err(error) => return Err(error),
        }
    }

    terminal.restore()
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

fn edit_and_rerender(path: &std::path::Path, line_numbers: bool, width_limit: Option<usize>) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());
    let args: Vec<&str> = editor.split_whitespace().collect();
    let Some(program) = args.first() else {
        eprintln!("Cannot open editor: EDITOR is empty");
        return;
    };
    let result = std::process::Command::new(program)
        .args(&args[1..])
        .arg(path)
        .status();

    match result {
        Ok(status) if status.success() => {
            // Re-read and re-render
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    if let Err(error) = run_viewer(Some(path), &content, line_numbers, width_limit)
                    {
                        eprintln!("Failed to display edited Markdown: {error}");
                    }
                }
                Err(e) => eprintln!("Failed to re-read file: {}", e),
            }
        }
        Ok(status) => eprintln!("Editor exited with status: {}", status),
        Err(e) => eprintln!("Failed to run editor: {}", e),
    }
}

/// Export a session to a combined markdown document.
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
            let mut file = std::fs::File::create(path)?;
            file.write_all(doc.as_bytes())?;
            println!("Exported to {}", path.display());
        }
        None => {
            io::stdout().write_all(doc.as_bytes())?;
        }
    }
    Ok(())
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
