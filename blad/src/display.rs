//! Session and file display for Blad.
//!
//! Parses Markdown message bodies and applies syntax highlighting to fenced
//! code blocks. Unsupported languages (e.g. hemlock) fall back to plain text
//! without crashing.

use std::io::{self, IsTerminal, Write};

use crate::RenderOptions;
use crate::links::{Link, extract_links};
use crate::markdown::{MarkdownRenderer, inert_source};
use crate::search::Search;
#[cfg(feature = "sessions")]
pub use crate::session_display::{
    dump_session, export_session, list_sessions, show_session, show_session_with_options,
};
use crate::viewer_navigation::{HELP, link_spans, reveal, spatial_link};
use crate::viewer_terminal::TerminalGuard;
use crate::viewport::{Document, Frame, Painter};

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
        let content = read_document_source(&canonical).map_err(|error| {
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
    Close,
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
    let mut terminal = TerminalGuard::enter()?;
    let mut painter = Painter::default();

    loop {
        let title = tab_title(&files, active);
        let file = &mut files[active];
        let action = viewer_session(
            file.path.as_deref(),
            &mut file.content,
            line_numbers,
            width_limit,
            options,
            &title,
            &mut file.position,
            &mut status,
            anchor.take(),
            &mut painter,
        )?;
        match action {
            ViewerAction::Quit => return terminal.restore(),
            ViewerAction::Close => {
                if files.len() == 1 {
                    return terminal.restore();
                }
                files.remove(active);
                history.retain_mut(|index| {
                    if *index == active {
                        false
                    } else {
                        if *index > active {
                            *index -= 1;
                        }
                        true
                    }
                });
                active = active.min(files.len() - 1);
                history.retain(|index| *index != active);
                status.clear();
            }
            ViewerAction::Notice(message) => status = message,
            ViewerAction::Jump(_) => {}
            ViewerAction::Edit(path) => {
                terminal.restore()?;
                files[active].content = edit_file(&path)?;
                terminal = TerminalGuard::enter()?;
                painter.invalidate();
            }
            ViewerAction::OpenFile(path, fragment) => match std::fs::canonicalize(&path) {
                Ok(path) => {
                    let existing = files
                        .iter()
                        .position(|file| file.path.as_ref() == Some(&path));
                    let target = if let Some(index) = existing {
                        Ok(index)
                    } else {
                        read_document_source(&path).map(|content| {
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
    content: &mut String,
    line_numbers: bool,
    width_limit: Option<usize>,
    options: &RenderOptions,
    title: &str,
    position: &mut Position,
    status: &mut String,
    anchor: Option<String>,
    painter: &mut Painter,
) -> io::Result<ViewerAction> {
    use crossterm::event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
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

    let mut search = Search::default();
    let mut help = false;
    let mut help_document = Document::render_with_options(HELP, cols, false, width_limit, options)?;

    // Source offsets identify actual rendered link spans, including duplicates.
    let mut links = extract_links(&inert_source(content));
    let mut spans: Vec<_> = links
        .iter()
        .map(|link| link_spans(&document, link))
        .collect();
    let mut link_mode = false;
    let mut link_index = 0;
    let mut selected_link = None;
    let mut selected_part = 0;

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
        let context = if (link_mode || selected_link.is_some()) && !links.is_empty() {
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

        let search_info = if search.editing || !search.query.is_empty() {
            search.footer()
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
        } else if !search_info.is_empty() {
            search_info
        } else if !context.is_empty() {
            context
        } else {
            format!(
                "  [{:5.1}%]{}{}  h:help",
                pct,
                line_info,
                if scroll_offset == 0 {
                    " TOP"
                } else if end >= total_lines {
                    " BOTTOM"
                } else {
                    " SCROLL"
                }
            )
        };
        let first_match = search.matches.partition_point(|span| span.row < start);
        let last_match = search.matches.partition_point(|span| span.row < end);
        let mut highlights = search.matches[first_match..last_match].to_vec();
        if let Some(index) = selected_link.or_else(|| link_mode.then_some(link_index)) {
            highlights.extend(
                spans[index]
                    .iter()
                    .filter(|span| (start..end).contains(&span.row))
                    .cloned(),
            );
        }
        highlights.sort_by_key(|span| (span.row, span.start, span.end));
        painter.paint(
            &mut io::stdout().lock(),
            if help { &help_document } else { &document },
            Frame {
                columns: cols,
                rows,
                y: if help { 0 } else { scroll_offset },
                x: if help { 0 } else { scroll_x },
                header: if help { "Blad help" } else { title },
                footer: if help {
                    "Any key returns to the document"
                } else {
                    &footer
                },
                matches: if help { &[] } else { &highlights },
            },
        )?;

        // Event handling
        match event::read() {
            Ok(Event::Key(KeyEvent {
                kind: KeyEventKind::Release,
                ..
            })) => continue,
            Ok(Event::Key(KeyEvent {
                code, modifiers, ..
            })) => {
                if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
                    return Ok(ViewerAction::Quit);
                }
                if help {
                    help = false;
                    continue;
                }
                if modifiers.contains(KeyModifiers::ALT)
                    && !search.editing
                    && !link_mode
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
                if code == KeyCode::Char('Q') && !search.editing {
                    return Ok(ViewerAction::Quit);
                }
                if search.editing {
                    let mut changed = true;
                    match code {
                        KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
                            search.regex = !search.regex;
                        }
                        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                            search.query.clear()
                        }
                        KeyCode::Char(c)
                            if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                        {
                            changed = search.push(c);
                        }
                        KeyCode::Backspace => {
                            search.query.pop();
                        }
                        KeyCode::Enter => {
                            search.editing = false;
                            changed = false;
                        }
                        KeyCode::Esc => {
                            search = Search::default();
                        }
                        _ => changed = false,
                    }
                    if changed {
                        search.update(lines, scroll_offset);
                        if let Some(span) = search.current() {
                            reveal(
                                span,
                                &mut scroll_offset,
                                &mut scroll_x,
                                visible_lines,
                                document.content_width,
                            );
                        }
                    }
                } else if modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(
                        code,
                        KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
                    )
                {
                    if let Some((index, part)) = spatial_link(
                        &spans,
                        selected_link.map(|index| (index, selected_part)),
                        (scroll_offset, scroll_x),
                        code,
                    ) {
                        link_index = index;
                        selected_link = Some(index);
                        selected_part = part;
                        search = Search::default();
                        reveal(
                            &spans[index][part],
                            &mut scroll_offset,
                            &mut scroll_x,
                            visible_lines,
                            document.content_width,
                        );
                    }
                } else if link_mode && !modifiers.contains(KeyModifiers::CONTROL) {
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
                            selected_link = None;
                        }
                        _ => {}
                    }
                    if link_mode {
                        selected_link = Some(link_index);
                        selected_part = 0;
                        if let Some(span) = spans[link_index].first() {
                            reveal(
                                span,
                                &mut scroll_offset,
                                &mut scroll_x,
                                visible_lines,
                                document.content_width,
                            );
                        }
                    }
                } else if modifiers.contains(KeyModifiers::CONTROL) {
                    match code {
                        KeyCode::Char('f') => {
                            scroll_offset = scroll_offset.saturating_add(visible_lines)
                        }
                        KeyCode::Char('b') => {
                            scroll_offset = scroll_offset.saturating_sub(visible_lines)
                        }
                        KeyCode::Char('d') => {
                            scroll_offset = scroll_offset.saturating_add((visible_lines / 2).max(1))
                        }
                        KeyCode::Char('u') => {
                            scroll_offset = scroll_offset.saturating_sub((visible_lines / 2).max(1))
                        }
                        KeyCode::Char('l') => painter.invalidate(),
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
                        KeyCode::PageUp => {
                            scroll_offset = scroll_offset.saturating_sub(visible_lines);
                        }
                        KeyCode::PageDown | KeyCode::Char(' ') => {
                            scroll_offset = (scroll_offset + visible_lines)
                                .min(total_lines.saturating_sub(visible_lines));
                        }
                        KeyCode::Char('u') => {
                            scroll_offset = scroll_offset.saturating_sub((visible_lines / 2).max(1))
                        }
                        KeyCode::Char('d') => {
                            scroll_offset = scroll_offset.saturating_add((visible_lines / 2).max(1))
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
                        KeyCode::Char('/') | KeyCode::Char('?') => {
                            search.begin(code == KeyCode::Char('?'));
                            selected_link = None;
                        }
                        KeyCode::Char('n' | 'N' | 'p') if !search.matches.is_empty() => {
                            search.advance(code != KeyCode::Char('n'));
                            if let Some(span) = search.current() {
                                reveal(
                                    span,
                                    &mut scroll_offset,
                                    &mut scroll_x,
                                    visible_lines,
                                    document.content_width,
                                );
                            }
                        }
                        KeyCode::Char('f') => {
                            if !links.is_empty() {
                                link_mode = true;
                                link_index = 0;
                                selected_link = Some(0);
                                selected_part = 0;
                                search = Search::default();
                                if let Some(span) = spans[0].first() {
                                    reveal(
                                        span,
                                        &mut scroll_offset,
                                        &mut scroll_x,
                                        visible_lines,
                                        document.content_width,
                                    );
                                }
                            }
                        }
                        KeyCode::Tab | KeyCode::BackTab if !links.is_empty() => {
                            link_index = match selected_link {
                                Some(index) if code == KeyCode::Tab => (index + 1) % links.len(),
                                Some(index) => (index + links.len() - 1) % links.len(),
                                None if code == KeyCode::BackTab => spans
                                    .iter()
                                    .rposition(|spans| {
                                        spans.first().is_some_and(|span| span.row <= scroll_offset)
                                    })
                                    .unwrap_or(links.len() - 1),
                                None => spans
                                    .iter()
                                    .position(|spans| {
                                        spans.first().is_some_and(|span| span.row >= scroll_offset)
                                    })
                                    .unwrap_or(0),
                            };
                            selected_link = Some(link_index);
                            selected_part = 0;
                            search = Search::default();
                            if let Some(span) = spans[link_index].first() {
                                reveal(
                                    span,
                                    &mut scroll_offset,
                                    &mut scroll_x,
                                    visible_lines,
                                    document.content_width,
                                );
                            }
                        }
                        KeyCode::Char('e') => {
                            if let Some(path) = path {
                                return Ok(ViewerAction::Edit(path.to_path_buf()));
                            }
                        }
                        KeyCode::Char('r') => {
                            let candidate = read_reload_source(path).and_then(|source| {
                                let document = Document::render_with_options(
                                    &source,
                                    cols,
                                    line_numbers,
                                    width_limit,
                                    options,
                                )?;
                                Ok((source, document))
                            });
                            match candidate {
                                Ok((source, updated)) => {
                                    *content = source;
                                    document = updated;
                                    scroll_offset = scroll_offset
                                        .min(document.lines.len().saturating_sub(visible_lines));
                                    scroll_x = scroll_x.min(document.max_x());
                                    links = extract_links(&inert_source(content));
                                    spans = links
                                        .iter()
                                        .map(|link| link_spans(&document, link))
                                        .collect();
                                    selected_link = None;
                                    selected_part = 0;
                                    search.update(&document.lines, scroll_offset);
                                    *status = "Reloaded file".into();
                                }
                                Err(error) => *status = format!("Error reloading file: {error}"),
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
                                            .get(&link.navigation_id())
                                            .is_some_and(|spans| {
                                                spans.iter().any(|span| span.row >= scroll_offset)
                                            })
                                    })
                                    .or_else(|| {
                                        spans.iter().position(|spans| {
                                            spans
                                                .first()
                                                .is_some_and(|span| span.row >= scroll_offset)
                                        })
                                    })
                            });
                        }
                        KeyCode::Char('[' | ']') => {
                            let mut headings: Vec<_> =
                                document.navigation.anchors.values().copied().collect();
                            headings.sort_unstable();
                            headings.dedup();
                            let target = if code == KeyCode::Char(']') {
                                headings.into_iter().find(|row| *row > scroll_offset)
                            } else {
                                headings.into_iter().rev().find(|row| *row < scroll_offset)
                            };
                            if let Some(row) = target {
                                scroll_offset = row;
                                scroll_x = 0;
                            }
                            selected_link = None;
                            search = Search::default();
                        }
                        KeyCode::Char('h') => help = true,
                        KeyCode::Char('Q') => return Ok(ViewerAction::Quit),
                        KeyCode::Char('q') | KeyCode::Esc => {
                            return Ok(ViewerAction::Close);
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Mouse(MouseEvent {
                kind, column, row, ..
            })) if !help => match kind {
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
                            .get(&link.navigation_id())
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
                search.update(&document.lines, scroll_offset);
                spans = links
                    .iter()
                    .map(|link| link_spans(&document, link))
                    .collect();
                selected_part = 0;
                if let Some(index) = selected_link
                    && let Some(span) = spans[index].first()
                {
                    reveal(
                        span,
                        &mut scroll_offset,
                        &mut scroll_x,
                        visible_lines,
                        document.content_width,
                    );
                }
                help_document =
                    Document::render_with_options(HELP, cols, false, width_limit, options)?;
                painter.invalidate();
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
}

fn anchor_row(document: &Document, anchor: &str) -> Option<usize> {
    if anchor.is_empty() {
        Some(0)
    } else {
        document.navigation.anchors.get(anchor).copied()
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
    read_document_source(path)
}

fn read_document_source(path: &std::path::Path) -> io::Result<String> {
    let bytes = crate::file_input::read_regular_file(path, 16 * 1024 * 1024)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn read_reload_source(path: Option<&std::path::Path>) -> io::Result<String> {
    let path = path.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "reload unavailable: this document has no file path",
        )
    })?;
    read_document_source(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reload_source_has_an_inclusive_16_mib_byte_limit() {
        const LIMIT: u64 = 16 * 1024 * 1024;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("boundary.md");
        let file = std::fs::File::create(&path).unwrap();
        for size in [LIMIT - 1, LIMIT, LIMIT + 1] {
            file.set_len(size).unwrap();
            let result = super::read_reload_source(Some(&path));
            if size <= LIMIT {
                assert_eq!(result.unwrap().len() as u64, size);
            } else {
                let Err(error) = result else {
                    panic!("oversized reload must fail");
                };
                assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge);
                assert!(error.to_string().contains("16777216"));
            }
        }
    }

    #[test]
    fn reload_invalid_utf8_does_not_return_a_valid_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("invalid.md");
        std::fs::write(&path, [b'x', 0xff]).unwrap();
        assert_eq!(
            super::read_reload_source(Some(&path)).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn reload_without_a_file_path_is_an_explicit_error() {
        let error = super::read_reload_source(None).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("reload unavailable"));
    }
}
