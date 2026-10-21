//! Session and file display for Blad.
//!
//! Parses Markdown message bodies and applies syntax highlighting to fenced
//! code blocks. Unsupported languages (e.g. hemlock) fall back to plain text
//! without crashing.

use std::io::{self, IsTerminal, Write};

use crate::RenderOptions;
use crate::markdown::MarkdownRenderer;
use crate::session::{Message, Session, load_messages};
use crate::terminal_text::TerminalLine;
use crate::viewport::{Document, Frame, paint};

/// List session metadata without interpreting authored terminal controls.
pub fn list_sessions(sessions: &[Session]) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    for session in sessions {
        writeln!(
            stdout,
            "{} - {} - {} messages - {}",
            crate::markdown::inert_source(&session.id),
            crate::markdown::inert_source(session.model.as_deref().unwrap_or("unknown")),
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

/// Show a completed session transcript.
pub fn show_session(session: &Session) -> io::Result<()> {
    show_session_with_options(session, &RenderOptions::default())
}

/// Show a session with one explicit rendering policy for every message.
pub fn show_session_with_options(session: &Session, options: &RenderOptions) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Session: {}",
        crate::markdown::inert_source(&session.id)
    )?;
    if let Some(model) = &session.model {
        writeln!(stdout, "Model: {}", crate::markdown::inert_source(model))?;
    }
    if let Some(created_at) = &session.created_at {
        writeln!(
            stdout,
            "Started: {}",
            crate::markdown::inert_source(created_at)
        )?;
    }
    writeln!(stdout, "Messages: {}\n", session.message_count)?;

    let messages = load_messages(session);
    let renderer = MarkdownRenderer::with_options(options.clone());
    for msg in &messages {
        print_message(msg, &mut stdout, &renderer)?;
    }
    Ok(())
}

/// Dump a completed session transcript to plain text (no ANSI).
pub fn dump_session(session: &Session) -> io::Result<()> {
    show_session_with_options(session, &RenderOptions::plain())
}

/// Stream a live or recently-completed session.
pub fn stream_session(session: &Session) -> io::Result<()> {
    stream_session_with_options(session, &RenderOptions::default())
}

/// Render the existing stream snapshot using explicit host settings.
pub fn stream_session_with_options(session: &Session, options: &RenderOptions) -> io::Result<()> {
    let messages = load_messages(session);
    let mut stdout = io::stdout().lock();
    let renderer = MarkdownRenderer::with_options(options.clone());
    for msg in &messages {
        print_message(msg, &mut stdout, &renderer)?;
    }

    if !session.completed {
        writeln!(stdout, "\n(session still active - re-run to see updates)")?;
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
    let options = if is_tty() {
        RenderOptions::default()
    } else {
        RenderOptions::plain()
    };
    if let Err(error) = view_markdown_file_with_options(path, content, line_numbers, &options) {
        eprintln!("Failed to display Markdown: {error}");
    }
}

/// View a file with settings retained through resize and editing.
pub fn view_markdown_file_with_options(
    path: Option<&std::path::Path>,
    content: &str,
    line_numbers: bool,
    options: &RenderOptions,
) -> io::Result<()> {
    if is_tty() && is_stdin_tty() {
        run_viewer(path, content, line_numbers, None, options)
    } else {
        MarkdownRenderer::with_options(options.clone())
            .render_markdown(content, &mut io::stdout().lock())
    }
}

/// View a markdown file with terminal width awareness.
pub fn view_markdown_width(path: Option<&std::path::Path>, content: &str, terminal_width: usize) {
    if is_tty() && is_stdin_tty() {
        if let Err(error) = run_viewer(
            path,
            content,
            false,
            Some(terminal_width),
            &RenderOptions::default(),
        ) {
            eprintln!("Failed to display Markdown: {error}");
        }
    } else {
        let mut output = Vec::new();
        let mut renderer = MarkdownRenderer::with_options(RenderOptions::plain());
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
    MarkdownRenderer::with_options(RenderOptions::plain())
        .render_markdown(content, &mut io::stdout().lock())
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
    options: &RenderOptions,
) -> io::Result<()> {
    let mut source = std::borrow::Cow::Borrowed(content);
    loop {
        match viewer_session(path, &source, line_numbers, width_limit, options)? {
            ViewerAction::Quit => return Ok(()),
            ViewerAction::Edit(path) => source = std::borrow::Cow::Owned(edit_file(path)?),
        }
    }
}

enum ViewerAction<'a> {
    Quit,
    Edit(&'a std::path::Path),
}

fn viewer_session<'a>(
    path: Option<&'a std::path::Path>,
    content: &str,
    line_numbers: bool,
    width_limit: Option<usize>,
    options: &RenderOptions,
) -> io::Result<ViewerAction<'a>> {
    use crossterm::event::{self, Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind};

    let (mut cols, mut rows) = crossterm::terminal::size()?;
    let mut document =
        Document::render_with_options(content, cols, line_numbers, width_limit, options)?;

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
                                return Ok(ViewerAction::Edit(path));
                            }
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
                document = Document::render_with_options(
                    content,
                    cols,
                    line_numbers,
                    width_limit,
                    options,
                )?;
                search_matches = find_matches(&document.lines, &search_query);
                search_index = search_index.min(search_matches.len().saturating_sub(1));
            }
            Ok(_) => {}
            Err(error) => return Err(error),
        }
    }

    terminal.restore()?;
    Ok(ViewerAction::Quit)
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

fn edit_file(path: &std::path::Path) -> io::Result<String> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());
    let args: Vec<&str> = editor.split_whitespace().collect();
    let Some(program) = args.first() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot open editor: EDITOR is empty",
        ));
    };
    let status = std::process::Command::new(program)
        .args(&args[1..])
        .arg(path)
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "editor exited with status: {status}"
        )));
    }
    std::fs::read_to_string(path)
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
fn print_message(
    msg: &Message,
    w: &mut impl Write,
    renderer: &MarkdownRenderer,
) -> std::io::Result<()> {
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

    writeln!(
        w,
        "--- {}{} ---",
        crate::markdown::inert_source(kind_label),
        disposition
    )?;
    // Parse markdown and highlight fenced code blocks
    renderer.render_markdown(&msg.content, w)?;
    if !msg.content.ends_with('\n') {
        writeln!(w)?;
    }
    Ok(())
}
