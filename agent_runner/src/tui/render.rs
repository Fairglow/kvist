//! Rendering for the terminal UI.

use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use super::app::{App, MENU_HOTKEYS, MENU_ITEMS, Overlay};

/// Rows reserved for the multiline prompt editor around the transcript.
const INPUT_ROWS: u16 = 4;

/// Renders one frame of the application.
pub fn render(f: &mut ratatui::Frame, app: &App) {
    let area = f.area();
    let vertical = ratatui::layout::Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(INPUT_ROWS),
    ]);
    let areas = vertical.split(area);
    let mut split = areas.iter();
    let header_area = *split.next().expect("header area");
    let stats_area = *split.next().expect("stats area");
    let transcript_area = *split.next().expect("transcript area");
    let input_area = *split.next().expect("input area");

    render_header(f, app, header_area);
    render_stats(f, app, stats_area);
    match (app.show_help, app.overlay) {
        // The help overlay keeps its own dedicated rendering and scroll state.
        (true, _) => render_help(f, app, transcript_area),
        (false, Overlay::History) => render_history(f, app, transcript_area),
        (false, Overlay::Replay) => render_replay(f, app, transcript_area),
        (false, Overlay::Menu) => render_menu(f, app, transcript_area),
        (false, Overlay::None) => render_transcript(f, app, transcript_area),
    }
    render_input(f, app, input_area);
}

/// The live stats bar: working speed, context utilization, and compaction
/// progress. Shown even before the first turn so the overview is always present.
fn render_stats(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let stats_text = app.stats_line();
    let text = if stats_text.is_empty() {
        " ⚡ — tok/s   context —    ·   compaction — ".to_owned()
    } else {
        stats_text
    };
    let block = Block::default();
    let paragraph = Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(Color::Green),
    )))
    .block(block);
    f.render_widget(paragraph, area);
}

fn render_header(f: &mut ratatui::Frame, app: &App, area: Rect) {
    // A leading spinner that advances with wall-clock time gives a motion cue
    // an idle header lacks, so "is it working?" is answerable at a glance: it
    // spins only while a turn generates and stops on any terminal event.
    let spinner = match app.spinner() {
        Some(frame) => Span::styled(frame, Style::default().fg(Color::Magenta).bold()),
        None => Span::from(" "),
    };
    let title = format!(
        "{} agent-runner  ·  model: {}  ·  thinking: {}  ·  status: {}",
        spinner,
        highlight(&app.model, app.running),
        app.effort.as_str(),
        app.status
    );
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .title(Span::styled(title, Style::default().fg(Color::Cyan).bold()));
    f.render_widget(block, area);
}

fn render_transcript(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = app
        .lines
        .iter()
        .map(|screen_line| screen_line.line.clone())
        .collect();
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        format!(" transcript [{}] ", app.lines.len()),
        Style::default().fg(Color::DarkGray),
    ));
    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .scroll((app.scroll, 0));
    f.render_widget(paragraph, area);
    // The help and other overlays share the transcript box, so they must not
    // draw the scrollbar on the right edge (it would overwrite the last column
    // of text). Overlays manage their own visible window instead.
    if app.show_scrollbar && !app.show_help && matches!(app.overlay, Overlay::None) {
        // The box's left and right borders consume 2 columns; the remaining
        // width is the inner content area the scrollbar represents.
        let viewport = area.height.saturating_sub(2).max(1) as usize;
        let content_length = app.lines.len().max(viewport);
        let offset = (app.scroll as usize).min(app.lines.len().saturating_sub(viewport));
        let mut state = ScrollbarState::new(content_length)
            .position(offset)
            .viewport_content_length(viewport);
        // Ride the right border column (width 1) so the scrollbar never overlaps
        // the wrapped content, which fills columns 1..=area.right()-2.
        let bar_area = Rect::new(area.right() - 1, area.top(), 1, area.height);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .track_symbol(Some("│"))
                .style(ratatui::style::Style::default().fg(Color::DarkGray)),
            bar_area,
            &mut state,
        );
    }
}

fn render_input(f: &mut ratatui::Frame, app: &App, area: Rect) {
    f.render_widget(&app.editor, area);
    // The editor reports the terminal-relative cursor position from the most
    // recent render; park the real terminal cursor there so typing is visible.
    if let Some(position) = app.editor.rendered_cursor_position() {
        f.set_cursor_position(position);
    }
}

fn render_help(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let config_path = app
        .config_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "(none found)".to_owned());
    let config_hint = format!(
        "  {} models are configured; Tab / Shift+Tab switch model and effort per prompt.",
        app.models.len()
    );
    let lines = vec![
        Line::from(Span::styled(
            "agent-runner help",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(""),
        Line::from("  Ctrl+Enter       submit the prompt"),
        Line::from("  Enter            submit on a blank line, else a newline"),
        Line::from("  Shift+Enter      always insert a blank line"),
        Line::from("  Tab / Shift+Tab  next model / next thinking effort (per prompt)"),
        Line::from("  Ctrl+P / Ctrl+N  previous / next prompt history"),
        Line::from("  Ctrl+C           cancel a running turn, or quit when idle"),
        Line::from("  Ctrl+D           quit when the prompt is empty"),
        Line::from("  PageUp / PageDown scroll the transcript"),
        Line::from("  Ctrl+L           clear the transcript"),
        Line::from("  Ctrl+T           collapse/reveal reasoning in the transcript"),
        Line::from("  Ctrl+H / Esc     toggle this help"),
        Line::from(""),
        Line::from("  The top bar shows model, thinking effort, and status."),
        Line::from(""),
        Line::from("  Configuration (add agents / models here):"),
        Line::from(format!("    {config_path}")),
        Line::from("    Add a [[models]] entry: id, provider (llama-server or"),
        Line::from("    ollama), base_url, provider model name, deadline_secs."),
        Line::from("    Reuse agents declared in Kvist's kvist.toml:"),
        Line::from("    agent-runner --import-kvist  (prints [[models]] to paste)."),
        Line::from(config_hint),
        Line::from(""),
        Line::from("  The stats bar shows: working speed (tok/s), context"),
        Line::from("  utilization of the model window, and how close we are to"),
        Line::from("  an automatic compaction. A compaction 'in ...' ETA is"),
        Line::from("  shown while the live context steadily climbs toward the"),
        Line::from("  limit; it is suppressed when the context is flat or was"),
        Line::from("  just rolled back by a compaction, so the number is honest"),
        Line::from(""),
        Line::from("  Compaction keeps the model's context bounded by rolling"),
        Line::from("  older turns into a summary. Full reasoning is always"),
        Line::from("  saved to the session journal + transcript; Ctrl+T only"),
        Line::from("  hides it in the live view. The agent runs inside the"),
        Line::from("  sandbox. Writes stay inside the working directory."),
    ];
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        " help",
        Style::default().fg(Color::Cyan).bold(),
    ));
    // Clamp the scroll to the rendered content so a short terminal never shows
    // a blank box: the help is taller than the transcript box, so it scrolls,
    // but the offset must stay within the help's own lines.
    let content_height = lines.len() as u16;
    let inner_height = area.height.saturating_sub(2).max(1);
    let scroll = app
        .help_scroll
        .min(content_height.saturating_sub(inner_height));
    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .alignment(Alignment::Left)
        .scroll((scroll, 0));
    f.render_widget(paragraph, area);
}

/// The action overlay menu, reached with Esc. Items are navigated with the
/// arrow keys or `j`/`k`, activated with Enter, or via their single-letter
/// hotkeys; Esc or `r` returns to the prompt.
fn render_menu(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " agent-runner menu ",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(""),
        Line::from("  up/down or j/k select · enter act · esc / r return"),
        Line::from(""),
    ];
    for (index, item) in MENU_ITEMS.iter().enumerate() {
        let selected = index == app.menu_selection;
        let style = if selected {
            Style::default().fg(Color::Yellow).bold()
        } else {
            Style::default().fg(Color::White)
        };
        let marker = if selected { "> " } else { "  " };
        let hotkey = MENU_HOTKEYS[index];
        lines.push(Line::from(Span::styled(
            format!("  {marker}{item} ({hotkey})"),
            style,
        )));
    }
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        " menu",
        Style::default().fg(Color::Cyan).bold(),
    ));
    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .alignment(Alignment::Left);
    f.render_widget(paragraph, area);
}

/// The session-history overlay: a scrollable list of past transcripts. Enter
/// replays the highlighted session; Esc returns to the action menu.
fn render_history(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " session history ",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(""),
        Line::from("  up/down or j/k select · enter replay · esc back"),
        Line::from(""),
    ];
    if app.history_items.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no past sessions found",
            Style::default().fg(Color::Yellow),
        )));
    } else {
        for (index, item) in app.history_items.iter().enumerate() {
            let selected = index == app.history_selection;
            let style = if selected {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default().fg(Color::White)
            };
            let marker = if selected { "> " } else { "  " };
            lines.push(Line::from(Span::styled(
                format!("  {marker}{}  {}", item.id, item.describe()),
                style,
            )));
        }
    }
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        " sessions",
        Style::default().fg(Color::Cyan).bold(),
    ));
    let inner_height = area.height.saturating_sub(2).max(1);
    let max_scroll = lines.len().saturating_sub(inner_height as usize) as u16;
    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .alignment(Alignment::Left)
        .scroll((app.history_scroll.min(max_scroll), 0));
    f.render_widget(paragraph, area);
}

/// A read-only replay of one past session's transcript. Esc returns to the
/// session-history list; navigation scrolls the transcript.
fn render_replay(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!(" replay {} ", app.replay_title),
        Style::default().fg(Color::Cyan).bold(),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from("  esc back to history"));
    lines.push(Line::from(""));
    for line in &app.replay_lines {
        lines.push(Line::from(Span::styled(
            line.clone(),
            Style::default().fg(Color::White),
        )));
    }
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        " replay",
        Style::default().fg(Color::Cyan).bold(),
    ));
    let inner_height = area.height.saturating_sub(2).max(1);
    let max_scroll = lines.len().saturating_sub(inner_height as usize) as u16;
    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .alignment(Alignment::Left)
        .scroll((app.replay_scroll.min(max_scroll), 0));
    f.render_widget(paragraph, area);
}

fn highlight(text: &str, active: bool) -> Span<'static> {
    Span::styled(
        text.to_owned(),
        if active {
            Style::default().fg(Color::Yellow).bold()
        } else {
            Style::default().fg(Color::Green)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::session::Event;
    use crate::tui::app::App;
    use agent_runtime::ReasoningEffort;
    use crossterm::event::KeyCode;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Draws one frame and returns the test backend for inspection.
    fn draw(app: &App) -> TestBackend {
        // Frame the app in its own reported size so tall frames actually render
        // tall (a fixed size would ignore the app's height).
        let backend = TestBackend::new(app.width, app.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, app))
            .expect("frame draws");
        terminal.backend().clone()
    }

    #[test]
    fn stats_row_shows_live_content_when_populated() {
        // The stats bar must render real statistics on its single row, not just a
        // border or heading, so the user sees working speed, context, and total
        // tokens while the session runs.
        let mut app = App::new(
            &["local".to_owned()],
            "local",
            ReasoningEffort::Medium,
            None,
            100,
            30,
        );
        app.push_event(Event::Progress {
            input_tokens: 70000,
            output_tokens: 6350,
            context_tokens: 76350,
            context_limit: 81920,
            context_utilization: 0.93,
            compaction_progress: 0.88,
            tokens_per_sec: 62.0,
            total_tokens: 45200,
            elapsed_secs: 332.5,
        });
        let backend = draw(&app);
        let text = buffer_text(&backend);
        // Row 0 is the header, row 1 is the stats bar.
        let stats_row = text.lines().nth(1).expect("stats row");
        assert!(
            stats_row.contains("tok/s")
                && stats_row.contains("81920")
                && stats_row.contains("45200 tok"),
            "stats content must be visible, got: {stats_row:?}"
        );
    }

    #[test]
    fn stats_row_is_placeholder_when_idle() {
        // Before any progress the stats bar shows a static placeholder hint.
        let app = App::new(
            &["local".to_owned()],
            "local",
            ReasoningEffort::Medium,
            None,
            100,
            30,
        );
        let backend = draw(&app);
        let text = buffer_text(&backend);
        let stats_row = text.lines().nth(1).expect("stats row");
        assert!(
            stats_row.contains("tok/s") && stats_row.contains("compaction"),
            "placeholder:\n{stats_row:?}"
        );
    }

    fn buffer_text(backend: &TestBackend) -> String {
        let buffer = backend.buffer();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                match buffer.cell((x, y)) {
                    Some(cell) => out.push_str(cell.symbol()),
                    None => out.push(' '),
                }
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn frame_shows_header_editor_text_and_parked_cursor() {
        let mut app = App::new(
            &["local".to_owned(), "ollama".to_owned()],
            "local",
            ReasoningEffort::Medium,
            None,
            60,
            20,
        );
        app.editor.insert_str("hello world");
        let mut backend = draw(&app);
        let text = buffer_text(&backend);
        // Header reports the selection; the editor shows the prompt text.
        assert!(
            text.contains("model: local"),
            "header shows the model:\n{text}"
        );
        assert!(
            text.contains("thinking: medium"),
            "header shows the effort:\n{text}"
        );
        assert!(
            text.contains("hello world"),
            "editor shows the typed text:\n{text}"
        );
        // The terminal cursor is parked exactly where the editor says it is.
        let expected = app
            .editor
            .rendered_cursor_position()
            .expect("cursor visible after render");
        backend.assert_cursor_position(expected);
    }

    #[test]
    fn help_overlay_documents_configuration_and_import() {
        let mut app = App::new(
            &["local".to_owned()],
            "local",
            ReasoningEffort::Medium,
            Some(std::path::PathBuf::from("/cfg/agent-runner.toml")),
            60,
            52,
        );
        app.on_key(crossterm::event::KeyEvent::new(
            KeyCode::Char('h'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert!(app.show_help);
        let backend = draw(&app);
        let text = buffer_text(&backend);
        assert!(text.contains("Ctrl+Enter"), "help lists the send key:");
        assert!(
            text.contains("/cfg/agent-runner.toml"),
            "help names the config path:"
        );
        assert!(
            text.contains("--import-kvist"),
            "help documents reusing Kvist agents:"
        );
    }

    #[test]
    fn help_overlay_is_scrollable_top_and_bottom() {
        // On a short frame the help overflows the transcript box; scrolling
        // reaches content that is not visible at scroll zero.
        let mut app = App::new(
            &["local".to_owned()],
            "local",
            ReasoningEffort::Medium,
            Some(std::path::PathBuf::from("/cfg/agent-runner.toml")),
            60,
            20,
        );
        app.on_key(crossterm::event::KeyEvent::new(
            KeyCode::Char('h'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        // Top of the help is visible without scrolling.
        assert!(
            buffer_text(&draw(&app)).contains("agent-runner help"),
            "top of help visible at scroll 0"
        );
        // Scrolling down several pages reaches the bottom of the help.
        for _ in 0..8 {
            app.on_key(crossterm::event::KeyEvent::new(
                KeyCode::PageDown,
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        let text = buffer_text(&draw(&app));
        assert!(app.help_scroll > 0, "help scrolled");
        assert!(
            text.contains("sandbox. Writes stay inside the working directory."),
            "bottom of the help is reachable:\n{text}"
        );
    }

    #[test]
    fn repro_streaming_dump() {
        use crate::session::Event;

        eprintln!("=== SCENARIO A: fragments WITHOUT newlines, width 80 ===");
        let mut a = App::new(
            &["local".to_owned()],
            "local",
            agent_runtime::ReasoningEffort::Medium,
            None,
            80,
            24,
        );
        for frag in ["The ", "quick ", "brown ", "fox "] {
            a.push_event(Event::Text(frag.to_owned()));
        }
        a.push_event(Event::Finished {
            message: "done".to_owned(),
        });
        for line in &a.lines {
            let text = line.line.to_string();
            eprintln!("  A LINE[{}] {:?}", text.chars().count(), text);
        }

        eprintln!("=== SCENARIO B: each fragment has a trailing newline, width 80 ===");
        let mut b = App::new(
            &["local".to_owned()],
            "local",
            agent_runtime::ReasoningEffort::Medium,
            None,
            80,
            24,
        );
        for frag in ["Hel\n", "lo \n", "wor\n", "ld\n"] {
            b.push_event(Event::Text(frag.to_owned()));
        }
        b.push_event(Event::Finished {
            message: "done".to_owned(),
        });
        for line in &b.lines {
            let text = line.line.to_string();
            eprintln!("  B LINE[{}] {:?}", text.chars().count(), text);
        }
    }
}
