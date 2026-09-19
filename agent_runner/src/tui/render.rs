//! Rendering for the terminal UI.

use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::app::App;

/// The number of header + input rows reserved around the transcript.
const FRAME_ROWS: u16 = 2;

/// Renders one frame of the application.
pub fn render(f: &mut ratatui::Frame, app: &App) {
    let area = f.area();
    let vertical = ratatui::layout::Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ]);
    let areas = vertical.split(area);
    let mut split = areas.iter();
    let header_area = *split.next().expect("header area");
    let stats_area = *split.next().expect("stats area");
    let transcript_area = *split.next().expect("transcript area");
    let input_area = *split.next().expect("input area");

    render_header(f, app, header_area);
    render_stats(f, app, stats_area);
    if app.show_help {
        render_help(f, app, transcript_area);
    } else {
        render_transcript(f, app, transcript_area);
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
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .title(Span::styled(
            " stats ",
            Style::default().fg(Color::DarkGray),
        ));
    let paragraph = Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(Color::Green),
    )))
    .block(block);
    f.render_widget(paragraph, area);
}

fn render_header(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let running = if app.running { " · running" } else { "" };
    let title = format!(
        " agent-runner  ·  model: {}  ·  thinking: {}  ·  status: {}{}",
        highlight(&app.model, app.running),
        app.effort,
        app.status,
        running
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
        .map(|screen_line| Line::from(Span::styled(screen_line.text.clone(), screen_line.style)))
        .collect();
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        format!(" transcript [{}] ", app.lines.len()),
        Style::default().fg(Color::DarkGray),
    ));
    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .scroll((app.scroll, 0));
    f.render_widget(paragraph, area);
}

fn render_input(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::styled("› ", Style::default().fg(Color::Cyan).bold())];
    spans.push(Span::styled(
        app.input.clone(),
        Style::default().fg(Color::White),
    ));
    let toggle = if app.can_reveal_reasoning() {
        "Ctrl+T reveal thinking"
    } else {
        "Ctrl+T collapse thinking"
    };
    let hint = format!("  Ctrl+C cancel/quit · {toggle} · ? help · ↑/↓ history · scroll");
    let block = Block::default()
        .borders(Borders::TOP)
        .title(Span::styled(hint, Style::default().fg(Color::DarkGray)));
    let paragraph = Paragraph::new(Line::from(spans)).block(block);
    f.render_widget(paragraph, area);
}

fn render_help(f: &mut ratatui::Frame, _app: &App, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "agent-runner help",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(""),
        Line::from("  Enter            submit your prompt"),
        Line::from("  Ctrl+C           cancel a running turn, or quit when idle"),
        Line::from("  Ctrl+D           quit when idle"),
        Line::from("  ↑ / ↓            previous / next prompt history"),
        Line::from("  PageUp / PageDown scroll the transcript"),
        Line::from("  Ctrl+L           clear the transcript"),
        Line::from("  Ctrl+T           collapse/reveal reasoning in the transcript"),
        Line::from("  ? / Esc          toggle this help"),
        Line::from(""),
        Line::from("  The top bar shows model, thinking effort, and status."),
        Line::from("  The stats bar shows: working speed (tok/s), context"),
        Line::from("  utilization of the model window, and how close we are to"),
        Line::from("  an automatic compaction."),
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
    let paragraph = Paragraph::new(Text::from(lines))
        .block(block)
        .alignment(Alignment::Left);
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

// Retained to document the frame row budget; unused directly.
#[allow(dead_code)]
const _FRAME_ROWS: u16 = FRAME_ROWS;
