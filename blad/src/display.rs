//! Session and file display for Blad.
//!
//! Parses Markdown message bodies and applies syntax highlighting to fenced
//! code blocks. Unsupported languages (e.g. hemlock) fall back to plain text
//! without crashing.

use std::io::{self, IsTerminal, Write};

use crate::RenderOptions;
use crate::links::{Link, extract_links};
use crate::markdown::{MarkdownRenderer, inert_source};
#[cfg(feature = "sessions")]
pub use crate::session_display::{
    dump_session, export_session, list_sessions, show_session, show_session_with_options,
};
use crate::terminal_text::TerminalLine;
use crate::viewport::{Document, Frame, paint};

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

/// Open initial files in stable argument order, reusing canonical duplicates.
///
/// All files are read before terminal entry or output. Plain settings bypass
/// the interactive viewer; otherwise terminal inputs share one tabbed viewer.
pub fn view_markdown_files_with_options(
    paths: &[std::path::PathBuf],
    line_numbers: bool,
    options: &RenderOptions,
) -> io::Result<()> {
    let mut files: Vec<OpenFile> = Vec::new();
    for path in paths {
        let canonical = std::fs::canonicalize(path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot open {}: {error}", path.display()),
            )
        })?;
        if files
            .iter()
            .any(|file| file.path.as_ref() == Some(&canonical))
        {
            continue;
        }
        let content = std::fs::read_to_string(&canonical).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot read {}: {error}", path.display()),
            )
        })?;
        files.push(OpenFile {
            path: Some(canonical),
            content,
            position: Position::default(),
        });
    }
    if files.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "at least one file is required",
        ));
    }
    if is_tty() && is_stdin_tty() && !options.plain {
        run_open_files(files, line_numbers, None, options)
    } else {
        let renderer = MarkdownRenderer::with_options(options.clone());
        let mut stdout = io::stdout().lock();
        for file in files {
            renderer.render_markdown(&file.content, &mut stdout)?;
        }
        Ok(())
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
    mouse: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        let mut guard = Self {
            raw: false,
            alternate: false,
            hidden: false,
            mouse: false,
        };
        crossterm::terminal::enable_raw_mode()?;
        guard.raw = true;
        guard.alternate = true;
        crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
        guard.hidden = true;
        crossterm::execute!(io::stdout(), crossterm::cursor::Hide)?;
        guard.mouse = true;
        crossterm::execute!(io::stdout(), crossterm::event::EnableMouseCapture)?;
        Ok(guard)
    }

    fn restore(&mut self) -> io::Result<()> {
        let mut error = None;
        if self.mouse {
            match crossterm::execute!(io::stdout(), crossterm::event::DisableMouseCapture) {
                Ok(()) => self.mouse = false,
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
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

/// Follow a link in the viewer context.
///
/// URL links are handed to the default browser and the viewer stays put.
/// Fragments resolve to renderer-owned heading positions. File links are
/// resolved against the current document's directory.
fn follow_link(link: &Link, current_path: Option<&std::path::Path>) -> io::Result<ViewerAction> {
    match link.kind {
        crate::links::LinkKind::Url => Ok(ViewerAction::Notice(crate::browser::open(
            &link.destination,
        )?)),
        crate::links::LinkKind::Anchor => Ok(ViewerAction::Jump(crate::links::decode_destination(
            &link.destination[1..],
        )?)),
        crate::links::LinkKind::File => {
            let base = current_path.and_then(std::path::Path::parent);
            let (file, fragment) = link
                .destination
                .split_once('#')
                .map_or((link.destination.as_str(), None), |(file, fragment)| {
                    (file, Some(fragment))
                });
            let file = crate::links::decode_destination(file)?;
            let path =
                base.map_or_else(|| std::path::PathBuf::from(&file), |base| base.join(&file));
            Ok(ViewerAction::OpenFile(
                path,
                fragment.map(crate::links::decode_destination).transpose()?,
            ))
        }
    }
}

enum ViewerAction {
    Quit,
    Notice(String),
    Edit(std::path::PathBuf),
    OpenFile(std::path::PathBuf, Option<String>),
    Jump(String),
    Switch(isize),
    Back,
}

/// A file open in the viewer: its path (when it came from a file) and the
/// content currently displayed.
struct OpenFile {
    path: Option<std::path::PathBuf>,
    content: String,
    position: Position,
}

#[derive(Default)]
struct Position {
    y: usize,
    x: usize,
}

fn run_viewer(
    path: Option<&std::path::Path>,
    content: &str,
    line_numbers: bool,
    width_limit: Option<usize>,
    options: &RenderOptions,
) -> io::Result<()> {
    let files = vec![OpenFile {
        path: path.map(std::fs::canonicalize).transpose()?,
        content: content.to_string(),
        position: Position::default(),
    }];
    run_open_files(files, line_numbers, width_limit, options)
}

fn run_open_files(
    mut files: Vec<OpenFile>,
    line_numbers: bool,
    width_limit: Option<usize>,
    options: &RenderOptions,
) -> io::Result<()> {
    let mut active = 0;
    let mut history = Vec::new();
    let mut status = String::new();
    let mut anchor = None;

    loop {
        let title = tab_title(&files, active);
        let file = &mut files[active];
        let action = viewer_session(
            file.path.as_deref(),
            &file.content,
            line_numbers,
            width_limit,
            options,
            &title,
            &mut file.position,
            &mut status,
            anchor.take(),
        )?;
        match action {
            ViewerAction::Quit => return Ok(()),
            ViewerAction::Notice(message) => status = message,
            ViewerAction::Jump(_) => {}
            ViewerAction::Edit(path) => {
                files[active].content = edit_file(&path)?;
            }
            ViewerAction::OpenFile(path, fragment) => match std::fs::canonicalize(&path) {
                Ok(path) => {
                    let existing = files
                        .iter()
                        .position(|file| file.path.as_ref() == Some(&path));
                    let target = if let Some(index) = existing {
                        Ok(index)
                    } else {
                        std::fs::read_to_string(&path).map(|content| {
                            files.push(OpenFile {
                                path: Some(path),
                                content,
                                position: Position::default(),
                            });
                            files.len() - 1
                        })
                    };
                    match target {
                        Ok(index) => {
                            if index != active {
                                history.push(active);
                                active = index;
                            }
                            anchor = fragment;
                        }
                        Err(error) => status = format!("Error opening file: {error}"),
                    }
                }
                Err(error) => status = format!("Error opening file {}: {error}", path.display()),
            },
            ViewerAction::Back => {
                if let Some(previous) = history.pop() {
                    active = previous;
                }
            }
            ViewerAction::Switch(direction) => {
                let next = (active as isize + direction).rem_euclid(files.len() as isize) as usize;
                if next != active {
                    history.push(active);
                    active = next;
                }
            }
        }
    }
}

fn tab_title(files: &[OpenFile], active: usize) -> String {
    let label = |index: usize| {
        files[index]
            .path
            .as_deref()
            .and_then(std::path::Path::file_name)
            .map_or_else(
                || "Markdown".to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
    };
    let mut title = "=== Blad: ".to_string();
    for index in 0..files.len() {
        if index > 0 {
            title.push_str(" | ");
        }
        if index == active {
            title.push_str(&format!("[{}]", label(index)));
        } else {
            title.push_str(&format!(" {} ", label(index)));
        }
    }
    title
}

#[allow(clippy::too_many_arguments)]
fn viewer_session(
    path: Option<&std::path::Path>,
    content: &str,
    line_numbers: bool,
    width_limit: Option<usize>,
    options: &RenderOptions,
    title: &str,
    position: &mut Position,
    status: &mut String,
    anchor: Option<String>,
) -> io::Result<ViewerAction> {
    use crossterm::event::{
        self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    let (mut cols, mut rows) = crossterm::terminal::size()?;
    let mut document =
        Document::render_with_options(content, cols, line_numbers, width_limit, options)?;

    let mut scroll_offset = position.y;
    let mut scroll_x = position.x;
    if let Some(anchor) = anchor {
        if let Some(row) = anchor_row(&document, &anchor) {
            scroll_offset = row;
            scroll_x = 0;
        } else {
            *status = format!("anchor not found: #{anchor}");
        }
    }
    let mut visible_lines = usize::from(rows.saturating_sub(2));

    // Search state
    let mut search_mode = false;
    let mut search_query = String::new();
    let mut search_matches: Vec<usize> = Vec::new();
    let mut search_index = 0;

    // Source offsets identify actual rendered link spans, including duplicates.
    let safe_content = inert_source(content);
    let links = extract_links(&safe_content);
    let mut link_lines = link_rows(&document, &links);
    let mut link_mode = false;
    let mut link_index = 0;
    let mut selected_link = None;

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
        let mut pending_follow = None;
        let lines = &document.lines;
        let total_lines = lines.len();
        scroll_offset = scroll_offset.min(total_lines.saturating_sub(visible_lines));
        scroll_x = scroll_x.min(document.max_x());
        position.y = scroll_offset;
        position.x = scroll_x;
        let start = scroll_offset;
        let end = scroll_offset.saturating_add(visible_lines).min(total_lines);

        // Header
        let context = if search_mode || (!search_query.is_empty() && !search_matches.is_empty()) {
            format!("=== Search: {} ===", search_query)
        } else if (link_mode || selected_link.is_some()) && !links.is_empty() {
            format!(
                "=== Links: {}/{} - {} ===",
                link_index + 1,
                links.len(),
                links[link_index].text
            )
        } else {
            String::new()
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
        let footer = if !status.is_empty() {
            status.clone()
        } else if !context.is_empty() {
            context
        } else {
            format!(
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
            )
        };
        paint(
            &mut io::stdout().lock(),
            &document,
            Frame {
                columns: cols,
                rows,
                y: scroll_offset,
                x: scroll_x,
                header: title,
                footer: &footer,
                matches: &highlight_rows(
                    &document,
                    &links,
                    selected_link.or_else(|| link_mode.then_some(link_index)),
                    &search_matches,
                ),
            },
        )?;

        // Event handling
        match event::read() {
            Ok(Event::Key(KeyEvent {
                code, modifiers, ..
            })) => {
                if modifiers.contains(KeyModifiers::ALT)
                    && matches!(code, KeyCode::Left | KeyCode::Right)
                {
                    status.clear();
                    return Ok(ViewerAction::Switch(if code == KeyCode::Left {
                        -1
                    } else {
                        1
                    }));
                }
                if !matches!(code, KeyCode::Enter) {
                    status.clear();
                }
                if search_mode {
                    match code {
                        KeyCode::Char(c) => {
                            search_query.push(c);
                            search_matches = find_matches(lines, &search_query);
                            if !search_matches.is_empty() {
                                search_index = 0;
                                scroll_offset = scroll_to_match(
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
                                scroll_offset = scroll_to_match(
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
                } else if link_mode {
                    match code {
                        KeyCode::Tab => {
                            link_index = (link_index + 1) % links.len();
                        }
                        KeyCode::BackTab => {
                            if link_index > 0 {
                                link_index -= 1;
                            } else {
                                link_index = links.len() - 1;
                            }
                        }
                        KeyCode::Enter => {
                            pending_follow = Some(link_index);
                            link_mode = false;
                        }
                        KeyCode::Esc => {
                            link_mode = false;
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
                        KeyCode::PageDown | KeyCode::Char('d') | KeyCode::Char(' ') => {
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
                            scroll_offset = scroll_to_match(
                                scroll_offset,
                                search_matches[search_index],
                                visible_lines,
                            );
                        }
                        KeyCode::Char('p') if !search_matches.is_empty() => {
                            search_index = search_index.saturating_sub(1);
                            scroll_offset = scroll_to_match(
                                scroll_offset,
                                search_matches[search_index],
                                visible_lines,
                            );
                        }
                        KeyCode::Char('f') => {
                            if !links.is_empty() {
                                link_mode = true;
                                link_index = 0;
                            }
                        }
                        KeyCode::Tab | KeyCode::BackTab if !links.is_empty() => {
                            link_index = match selected_link {
                                Some(index) if code == KeyCode::Tab => (index + 1) % links.len(),
                                Some(index) => (index + links.len() - 1) % links.len(),
                                None if code == KeyCode::BackTab => link_lines
                                    .iter()
                                    .rposition(|row| row.is_some_and(|row| row <= scroll_offset))
                                    .unwrap_or(links.len() - 1),
                                None => link_lines
                                    .iter()
                                    .position(|row| row.is_some_and(|row| row >= scroll_offset))
                                    .unwrap_or(0),
                            };
                            selected_link = Some(link_index);
                            if let Some(row) = link_lines[link_index] {
                                scroll_offset = scroll_to_match(scroll_offset, row, visible_lines);
                            }
                        }
                        KeyCode::Char('e') => {
                            if let Some(path) = path {
                                terminal.restore()?;
                                return Ok(ViewerAction::Edit(path.to_path_buf()));
                            }
                        }
                        KeyCode::Char('b') => {
                            return Ok(ViewerAction::Back);
                        }
                        // Enter follows the first link at or below the top of
                        // the viewport, so links can be jumped to without
                        // entering link mode.
                        KeyCode::Enter => {
                            pending_follow = selected_link.or_else(|| {
                                links
                                    .iter()
                                    .position(|link| {
                                        document
                                            .navigation
                                            .links
                                            .get(&link.source_offset)
                                            .is_some_and(|spans| {
                                                spans.iter().any(|span| span.row >= scroll_offset)
                                            })
                                    })
                                    .or_else(|| {
                                        link_lines.iter().position(|row| {
                                            row.is_some_and(|row| row >= scroll_offset)
                                        })
                                    })
                            });
                        }
                        KeyCode::Char('q') | KeyCode::Esc => {
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Mouse(MouseEvent {
                kind, column, row, ..
            })) => match kind {
                MouseEventKind::Down(MouseButton::Left)
                    if row > 0
                        && row < rows.saturating_sub(1)
                        && usize::from(column) >= document.gutter =>
                {
                    let target_row = scroll_offset + usize::from(row - 1);
                    let target_column = scroll_x + usize::from(column) - document.gutter;
                    pending_follow = links.iter().position(|link| {
                        document
                            .navigation
                            .links
                            .get(&link.source_offset)
                            .is_some_and(|spans| {
                                spans.iter().any(|span| {
                                    span.row == target_row
                                        && (span.start..span.end).contains(&target_column)
                                })
                            })
                    });
                }
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
                // Reflow moved links to new rendered lines.
                link_lines = link_rows(&document, &links);
            }
            Ok(_) => {}
            Err(error) => return Err(error),
        }
        if let Some(index) = pending_follow {
            status.clear();
            match follow_link(&links[index], path) {
                Ok(ViewerAction::Jump(anchor)) => {
                    if let Some(row) = anchor_row(&document, &anchor) {
                        scroll_offset = row;
                        scroll_x = 0;
                    } else {
                        *status = format!("anchor not found: #{anchor}");
                    }
                    selected_link = None;
                }
                Ok(ViewerAction::Notice(message)) => *status = message,
                Ok(action) => return Ok(action),
                Err(error) => *status = format!("Error following link: {error}"),
            }
        }
    }
    terminal.restore()?;
    Ok(ViewerAction::Quit)
}

fn link_rows(document: &Document, links: &[Link]) -> Vec<Option<usize>> {
    links
        .iter()
        .map(|link| {
            document
                .navigation
                .links
                .get(&link.source_offset)
                .and_then(|spans| spans.first().map(|span| span.row))
                .or_else(|| rendered_line_of(&document.lines, &link.text))
        })
        .collect()
}

fn anchor_row(document: &Document, anchor: &str) -> Option<usize> {
    if anchor.is_empty() {
        Some(0)
    } else {
        document.navigation.anchors.get(anchor).copied()
    }
}

fn highlight_rows(
    document: &Document,
    links: &[Link],
    selected: Option<usize>,
    search: &[usize],
) -> Vec<usize> {
    let mut rows = search.to_vec();
    if let Some(link) = selected.and_then(|index| links.get(index))
        && let Some(spans) = document.navigation.links.get(&link.source_offset)
    {
        rows.extend(spans.iter().map(|span| span.row));
    }
    rows.sort_unstable();
    rows.dedup();
    rows
}
/// Find the rendered line a link's display text first appears on, so Enter can
/// follow the link at the cursor. Returns `None` if the text is not found (for
/// example when a link label wraps across lines or is clipped).
fn rendered_line_of(lines: &[TerminalLine], text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    lines.iter().position(|line| line.plain().contains(text))
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
