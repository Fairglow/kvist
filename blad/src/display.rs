//! Session and file display for Blad.
//!
//! Parses Markdown message bodies and applies syntax highlighting to fenced
//! code blocks. Unsupported languages (e.g. hemlock) fall back to plain text
//! without crashing.

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use crate::RenderOptions;
use crate::links::{Link, extract_links};
use crate::markdown::{MarkdownRenderer, inert_source};
use crate::search::Search;
#[cfg(feature = "sessions")]
pub use crate::session_display::{
    dump_session, export_session, list_sessions, show_session, show_session_with_options,
};
use crate::stream_input::{REFRESH_INTERVAL, Refresh, StdinStream};
use crate::viewer_files::{OpenFile, Position, edit_file, load_files, tab_title};
use crate::viewer_navigation::{HELP, follow_key, link_spans, reveal, spatial_link};
use crate::viewer_reload::{FileWatch, read_document_source, read_reload_source};
use crate::viewer_terminal::{TerminalGuard, require_controlling_terminal, wait_event};
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
        tracing::error!("Failed to display Markdown: {error}");
    }
}

/// View a file with settings retained through resize and editing.
pub fn view_markdown_file_with_options(
    path: Option<&std::path::Path>,
    content: &str,
    line_numbers: bool,
    options: &RenderOptions,
) -> io::Result<()> {
    if is_tty() && is_stdin_tty() && !options.plain {
        run_viewer(path, content, line_numbers, None, options)
    } else {
        MarkdownRenderer::with_options(options.clone())
            .render_markdown(content, &mut io::stdout().lock())
    }
}

/// Open initial files in stable argument order, reusing canonical duplicates.
///
/// Named files validate before terminal entry. With terminal stdout, redirected
/// stdin streams into one tab using the controlling terminal for keyboard input.
/// Plain settings or nonterminal stdout read all inputs through EOF before output.
/// Interactive stdin is exclusively consumed: fd 0 becomes the controlling
/// terminal. Do not concurrently read or pre-buffer stdin around this helper.
pub fn view_markdown_files_with_options(
    paths: &[std::path::PathBuf],
    line_numbers: bool,
    options: &RenderOptions,
) -> io::Result<()> {
    let interactive = is_tty() && !options.plain;
    let keyboard = if interactive && !is_stdin_tty() {
        Some(require_controlling_terminal()?)
    } else {
        None
    };
    let files = load_files(paths, interactive)?;
    if interactive {
        let stream = if files.iter().any(|file| file.path.is_none()) {
            let keyboard = keyboard.as_ref().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "stdin paging requires redirected input",
                )
            })?;
            Some(StdinStream::take_stdin(keyboard)?)
        } else {
            None
        };
        run_open_files(files, line_numbers, None, options, stream)
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
            tracing::error!("Failed to display Markdown: {error}");
        }
    } else {
        let mut output = Vec::new();
        let mut renderer = MarkdownRenderer::with_options(RenderOptions::plain());
        renderer.set_width(Some(terminal_width));
        if let Err(error) = renderer
            .render_markdown(content, &mut output)
            .and_then(|()| io::stdout().write_all(&output))
        {
            tracing::error!("Failed to display Markdown: {error}");
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
    run_open_files(files, line_numbers, width_limit, options, None)
}

fn run_open_files(
    mut files: Vec<OpenFile>,
    line_numbers: bool,
    width_limit: Option<usize>,
    options: &RenderOptions,
    mut stream: Option<StdinStream>,
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
            &mut stream,
        )?;
        match action {
            ViewerAction::Quit => {
                terminal.restore()?;
                return stream.map_or(Ok(()), StdinStream::finish);
            }
            ViewerAction::Close => {
                if files.len() == 1 {
                    terminal.restore()?;
                    return stream.map_or(Ok(()), StdinStream::finish);
                }
                if files[active].path.is_none()
                    && let Some(stream) = &mut stream
                {
                    stream.stop();
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
    stream: &mut Option<StdinStream>,
) -> io::Result<ViewerAction> {
    use crossterm::event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };

    let (mut cols, mut rows) = crossterm::terminal::size()?;
    let stdin_tab = path.is_none() && stream.is_some();
    let source = current_source(content, stream, stdin_tab);
    let mut document =
        Document::render_with_options(source, cols, line_numbers, width_limit, options)?;

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
    let mut links = extract_links(&inert_source(source));
    let mut spans: Vec<_> = links
        .iter()
        .map(|link| link_spans(&document, link))
        .collect();
    let mut link_index = 0;
    let mut selected_link: Option<usize> = None;
    let mut selected_part = 0;
    let mut watch = FileWatch::new();
    let mut refresh = Refresh::new(stream.as_ref().map_or(0, |stream| stream.revision));
    loop {
        let input_ready = stream.as_mut().is_some_and(StdinStream::pump);
        if stdin_tab
            && let Some(stream) = stream.as_ref()
            && (refresh.due(stream.revision, Instant::now())
                || (!stream.live() && refresh.pending(stream.revision)))
        {
            let started = Instant::now();
            document = Document::render_with_options(
                stream.source(),
                cols,
                line_numbers,
                width_limit,
                options,
            )?;
            links = extract_links(&inert_source(stream.source()));
            spans = links
                .iter()
                .map(|link| link_spans(&document, link))
                .collect();
            selected_link = None;
            selected_part = 0;
            link_index = 0;
            search.update(&document.lines, scroll_offset);
            refresh.completed(stream.revision, started.elapsed());
        }
        if path.is_some_and(|path| watch.changed(path)) {
            let candidate = read_reload_source(path).and_then(|source| {
                if source == *content {
                    return Ok(None);
                }
                let updated = Document::render_with_options(
                    &source,
                    cols,
                    line_numbers,
                    width_limit,
                    options,
                )?;
                Ok(Some((source, updated)))
            });
            match candidate {
                Ok(Some((source, updated))) => {
                    *content = source;
                    document = updated;
                    scroll_offset =
                        scroll_offset.min(document.lines.len().saturating_sub(visible_lines));
                    scroll_x = scroll_x.min(document.max_x());
                    links = extract_links(&inert_source(content));
                    spans = links
                        .iter()
                        .map(|link| link_spans(&document, link))
                        .collect();
                    selected_link = None;
                    selected_part = 0;
                    link_index = 0;
                    search.update(&document.lines, scroll_offset);
                    if status.starts_with("Error reloading file:") {
                        status.clear();
                    }
                }
                Ok(None) => {
                    if status.starts_with("Error reloading file:") {
                        status.clear();
                    }
                }
                Err(error) => *status = format!("Error reloading file: {error}"),
            }
        }
        let mut pending_follow = None;
        let lines = &document.lines;
        let total_lines = lines.len();
        if stdin_tab && position.following {
            scroll_offset = total_lines.saturating_sub(visible_lines);
        }
        scroll_offset = scroll_offset.min(total_lines.saturating_sub(visible_lines));
        scroll_x = scroll_x.min(document.max_x());
        position.y = scroll_offset;
        position.x = scroll_x;
        let start = scroll_offset;
        let end = scroll_offset.saturating_add(visible_lines).min(total_lines);

        // Header
        let context = if selected_link.is_some() && !links.is_empty() {
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

        let search_info = if search.active() {
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
        let mut footer = if !status.is_empty() {
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
        if let Some(stream) = stream.as_ref()
            && (stdin_tab || stream.failed())
        {
            footer = format!("{} {footer}", stream.footer(position.following));
        }
        let help_footer = stream
            .as_ref()
            .filter(|stream| stdin_tab || stream.failed())
            .map_or_else(
                || "Any key returns to the document".to_string(),
                |stream| {
                    format!(
                        "{} Any key returns to the document",
                        stream.footer(position.following)
                    )
                },
            );
        let first_match = search.matches.partition_point(|span| span.row < start);
        let last_match = search.matches.partition_point(|span| span.row < end);
        let highlights = &search.matches[first_match..last_match];
        let active = if let Some(index) = selected_link {
            spans[index].as_slice()
        } else {
            search.current().map_or(&[][..], std::slice::from_ref)
        };
        painter.paint(
            &mut io::stdout().lock(),
            if help { &help_document } else { &document },
            Frame {
                columns: cols,
                rows,
                y: if help { 0 } else { scroll_offset },
                x: if help { 0 } else { scroll_x },
                header: if help { "Blad help" } else { title },
                footer: if help { &help_footer } else { &footer },
                matches: if help { &[] } else { highlights },
                active: if help { &[] } else { active },
            },
        )?;

        let input_live = stream.as_ref().is_some_and(StdinStream::live);
        let input_pending = stdin_tab
            && stream
                .as_ref()
                .is_some_and(|stream| refresh.pending(stream.revision));
        if path.is_some() || input_live || input_pending {
            let wait = if input_ready {
                Duration::ZERO
            } else if path.is_some() && (input_live || input_pending) {
                REFRESH_INTERVAL.min(watch.wait())
            } else if input_live || input_pending {
                REFRESH_INTERVAL
            } else {
                watch.wait()
            };
            if !wait_event(stream.as_ref(), wait)? {
                continue;
            }
        }

        // Event handling
        let mut scroll_down = false;
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
                if stdin_tab && !search.editing {
                    scroll_down = follow_key(code, modifiers, &mut position.following);
                }
                if modifiers.contains(KeyModifiers::ALT)
                    && !search.editing
                    && matches!(code, KeyCode::Left | KeyCode::Right)
                {
                    status.clear();
                    return Ok(ViewerAction::Switch(if code == KeyCode::Left {
                        -1
                    } else {
                        1
                    }));
                }
                if modifiers.contains(KeyModifiers::CONTROL)
                    && !search.editing
                    && matches!(code, KeyCode::PageUp | KeyCode::PageDown)
                {
                    status.clear();
                    return Ok(ViewerAction::Switch(if code == KeyCode::PageUp {
                        -1
                    } else {
                        1
                    }));
                }
                if !matches!(code, KeyCode::Enter) {
                    status.clear();
                }
                if modifiers.contains(KeyModifiers::CONTROL)
                    && code == KeyCode::Char('q')
                    && !search.editing
                {
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
                        KeyCode::Char('r') => {
                            search.begin(false);
                            search.regex = true;
                            selected_link = None;
                        }
                        KeyCode::Home => {
                            scroll_offset = 0;
                            scroll_x = 0;
                        }
                        KeyCode::End => {
                            scroll_offset = total_lines.saturating_sub(visible_lines);
                        }
                        _ => {}
                    }
                } else {
                    match code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            scroll_offset = scroll_offset.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j')
                            if scroll_offset < total_lines.saturating_sub(visible_lines) =>
                        {
                            scroll_offset += 1;
                        }
                        KeyCode::PageUp if modifiers.contains(KeyModifiers::SHIFT) => {
                            scroll_offset =
                                scroll_offset.saturating_sub((visible_lines / 2).max(1));
                        }
                        KeyCode::PageUp => {
                            scroll_offset = scroll_offset.saturating_sub(visible_lines);
                        }
                        KeyCode::PageDown if modifiers.contains(KeyModifiers::SHIFT) => {
                            scroll_offset =
                                scroll_offset.saturating_add((visible_lines / 2).max(1));
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
                            scroll_x = 0;
                        }
                        KeyCode::End => {
                            scroll_x = document.max_x();
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
                            search.begin(false);
                            selected_link = None;
                        }
                        KeyCode::Char('n' | '>') | KeyCode::Enter if search.active() => {
                            position.following = false;
                            search.advance(false);
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
                        KeyCode::Char('N' | '<') if !search.matches.is_empty() => {
                            search.advance(true);
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
                        KeyCode::Char('b') => {
                            return Ok(ViewerAction::Back);
                        }
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
                        KeyCode::Char('?' | 'h') | KeyCode::F(1) => help = true,
                        KeyCode::Char('Q') => return Ok(ViewerAction::Quit),
                        KeyCode::Char('q') | KeyCode::Esc if search.active() => {
                            search = Search::default();
                        }
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
                    position.following = false;
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
                    position.following = false;
                    scroll_offset = scroll_offset.saturating_sub(1);
                }
                MouseEventKind::ScrollDown
                    if scroll_offset < total_lines.saturating_sub(visible_lines) =>
                {
                    scroll_down = true;
                    scroll_offset += 1;
                }
                _ => {}
            },
            Ok(Event::Resize(columns, height)) => {
                let started = Instant::now();
                let reveal_search = search.current().is_some_and(|span| {
                    (scroll_offset..scroll_offset.saturating_add(visible_lines)).contains(&span.row)
                        && span.start < scroll_x.saturating_add(document.content_width)
                        && span.end > scroll_x
                });
                cols = columns;
                rows = height;
                visible_lines = usize::from(rows.saturating_sub(2));
                document = Document::render_with_options(
                    current_source(content, stream, stdin_tab),
                    cols,
                    line_numbers,
                    width_limit,
                    options,
                )?;
                if stdin_tab
                    && let Some(stream) = stream.as_ref()
                    && refresh.pending(stream.revision)
                {
                    links = extract_links(&inert_source(stream.source()));
                    selected_link = None;
                    link_index = 0;
                }
                let search_index = search.index;
                search.update(&document.lines, scroll_offset);
                search.index = search_index.min(search.matches.len().saturating_sub(1));
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
                } else if reveal_search && let Some(span) = search.current() {
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
                if stdin_tab && let Some(stream) = stream.as_ref() {
                    refresh.completed(stream.revision, started.elapsed());
                }
                painter.invalidate();
            }
            Ok(_) => {}
            Err(error) => return Err(error),
        }
        if stdin_tab && scroll_down {
            position.following = scroll_offset >= total_lines.saturating_sub(visible_lines);
        }
        if let Some(index) = pending_follow {
            position.following = false;
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

fn current_source<'a>(
    content: &'a str,
    stream: &'a Option<StdinStream>,
    stdin_tab: bool,
) -> &'a str {
    if stdin_tab && let Some(stream) = stream {
        stream.source()
    } else {
        content
    }
}
