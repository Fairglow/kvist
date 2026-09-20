//! Terminal UI application state: transcript rows, selectors, input, and keys.

use std::path::PathBuf;
use std::sync::mpsc;

use agent_runtime::ReasoningEffort;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use tui_textarea::TextArea;

use crate::error::Result;
use crate::session::Event;

/// Classifies a transcript line so it can be collapsed for a cleaner overview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Ordinary content (answer text, tool results, notes).
    Normal,
    /// A fragment of model reasoning / thinking, rendered dim.
    Reasoning,
    /// A collapsed placeholder standing in for hidden thinking.
    Placeholder,
}

/// A single pre-wrapped, styled transcript line.
#[derive(Debug, Clone)]
pub struct ScreenLine {
    pub style: Style,
    pub text: String,
    pub kind: LineKind,
}

/// Maximum number of transcript lines retained before dropping the oldest.
const MAX_LINES: usize = 5000;
/// Maximum total characters of a single prompt before it is rejected.
const MAX_PROMPT_CHARS: usize = 16_384;
/// The selectable thinking effort levels, in ascending order; the UI can only
/// present these valid choices.
const EFFORTS: [ReasoningEffort; 7] = [
    ReasoningEffort::None,
    ReasoningEffort::Minimal,
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Xhigh,
    ReasoningEffort::Max,
];

/// The latest context/token accounting from the loop, driving the stats bar.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    /// Session-wide tokens per second.
    pub tokens_per_sec: f64,
    /// Tokens the model currently holds in context.
    pub context_tokens: usize,
    /// The model context limit, in tokens.
    pub context_limit: usize,
    /// Context utilization against the limit (`0.0..=1.0+`).
    pub utilization: f64,
    /// How close compaction is to the hard limit (`0.0..=1.0`).
    pub compaction_progress: f64,
    /// Estimated seconds until the live context reaches the hard limit, forecast
    /// from the observed context growth rate. `None` when the estimate is not
    /// meaningful (no prior sample, flat or declining context, or growth below a
    /// floor), so a misleading ETA is never shown.
    pub eta_secs: Option<f64>,
    /// Cumulative provider tokens observed across the session.
    pub total_tokens: u64,
    /// Wall-clock seconds since the run started.
    pub elapsed_secs: f64,
}

/// What a key press asked the runner to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// Nothing beyond local UI state.
    Idle,
    /// A prompt was staged; the runner sends `App::take_pending_prompt()`.
    Submit,
    /// Cancel the running turn.
    Cancel,
    /// Quit the application.
    Quit,
}

/// The interactive application state driven by events and key input.
pub struct App {
    pub lines: Vec<ScreenLine>,
    pub scroll: u16,
    /// Scroll offset of the help overlay while it is shown, so the full
    /// (taller-than-the-transcript-box) help stays reachable on short terminals.
    pub help_scroll: u16,
    /// The multiline prompt editor.
    pub editor: TextArea<'static>,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    /// The model ids selectable in this session, in configuration order.
    pub models: Vec<String>,
    /// The currently selected model id.
    pub model: String,
    /// The currently selected thinking effort (a valid enum value).
    pub effort: ReasoningEffort,
    pub running: bool,
    pub show_help: bool,
    pub status: String,
    pub should_quit: bool,
    pub width: u16,
    pub height: u16,
    /// The staged prompt awaiting dispatch, if any.
    pending_prompt: Option<String>,
    /// Streamed answer text not yet flushed to the transcript (incomplete
    /// paragraphs); flushed on newlines and on any non-text event.
    pending_text: String,
    /// The resolved configuration path, shown in the help overlay.
    pub config_path: Option<PathBuf>,
    /// Latest stats, when the loop has reported any.
    pub stats: Option<Stats>,
    /// Most recent `(elapsed_secs, context_tokens)` progress sample, used to
    /// forecast the time until the live context reaches its hard limit.
    last_growth: Option<(f64, usize)>,
    /// Whether reasoning lines are collapsed in the transcript for an overview.
    pub collapse_reasoning: bool,
    /// Reasoning lines removed by a collapse, restored on reveal, in order.
    hidden_reasoning: Vec<Vec<ScreenLine>>,
}

impl App {
    /// Creates a new application for a selected model and thinking effort.
    pub fn new(
        models: &[String],
        model_id: &str,
        effort: ReasoningEffort,
        config_path: Option<PathBuf>,
        width: u16,
        height: u16,
    ) -> Self {
        let width = width.max(20);
        let mut editor = TextArea::new(vec![String::new()]);
        editor.set_wrap_mode(tui_textarea::WrapMode::Word);
        // The cursor line is underlined by default; disable it because the
        // underline looks odd and is unnecessary in a short prompt box.
        editor.set_cursor_line_style(Style::default());
        editor.set_block(
            ratatui::widgets::Block::default()
                .borders(ratatui::widgets::Borders::ALL)
                .title(Span::styled(
                    " prompt · Enter/Ctrl+Enter send · Shift+Enter · Tab model · Shift+Tab effort · Ctrl+H help ",
                    Style::default().fg(Color::DarkGray),
                )),
        );
        let mut app = App {
            lines: Vec::new(),
            scroll: 0,
            help_scroll: 0,
            editor,
            history: Vec::new(),
            history_index: None,
            models: models.to_vec(),
            model: model_id.to_owned(),
            effort,
            running: false,
            show_help: false,
            status: "idle".to_owned(),
            should_quit: false,
            width,
            height,
            pending_prompt: None,
            pending_text: String::new(),
            config_path,
            stats: None,
            last_growth: None,
            collapse_reasoning: false,
            hidden_reasoning: Vec::new(),
        };
        app.note(
            Style::default().fg(Color::Cyan),
            "agent-runner ready. Type a prompt and press Ctrl+Enter. Press Ctrl+H for help.",
        );
        app
    }

    /// Feeds one loop event into the transcript, wrapping at the current width.
    pub fn push_event(&mut self, event: Event) {
        match event {
            Event::TurnStart { model } => {
                self.flush_pending_text();
                self.running = true;
                self.status = "thinking".to_owned();
                self.note(
                    Style::default().fg(Color::Magenta),
                    &format!("▍ {model} is working…"),
                );
            }
            Event::Reasoning(text) => self.plain_reasoning(&text),
            // Streamed answer text arrives in fragments; accumulate it and only
            // wrap at paragraph boundaries so a sentence stays on its lines
            // instead of one word per line.
            Event::Text(text) => {
                self.pending_text.push_str(&text);
                if let Some(pos) = self.pending_text.rfind('\n') {
                    let ready = self.pending_text[..pos].to_owned();
                    self.pending_text = self.pending_text[pos + 1..].to_owned();
                    self.push_wrapped(Style::default().fg(Color::Reset), &ready);
                }
            }
            Event::ToolCall { name } => {
                self.flush_pending_text();
                self.note(Style::default().fg(Color::Blue), &format!("→ tool: {name}"));
            }
            Event::ToolResult { name, failed } => {
                self.flush_pending_text();
                let style = if failed {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                };
                self.note(style, &format!("✓ tool {name} finished"));
            }
            Event::Finished { message } => {
                self.flush_pending_text();
                self.running = false;
                self.status = "done".to_owned();
                self.note(Style::default().fg(Color::Green), &format!("✓ {message}"));
            }
            Event::Failed(text) => {
                self.flush_pending_text();
                self.running = false;
                self.status = "error".to_owned();
                self.note(Style::default().fg(Color::Red), &text);
            }
            Event::Note(text) => {
                self.flush_pending_text();
                self.note(Style::default().fg(Color::DarkGray), &text);
            }
            Event::Progress {
                tokens_per_sec,
                context_tokens,
                context_limit,
                context_utilization,
                compaction_progress,
                total_tokens,
                elapsed_secs,
                ..
            } => {
                let eta_secs = estimate_eta(
                    self.last_growth,
                    elapsed_secs,
                    context_tokens,
                    context_limit,
                );
                self.last_growth = Some((elapsed_secs, context_tokens));
                self.stats = Some(Stats {
                    tokens_per_sec,
                    context_tokens,
                    context_limit,
                    utilization: context_utilization,
                    compaction_progress,
                    eta_secs,
                    total_tokens,
                    elapsed_secs,
                });
                if context_limit > 0 && context_utilization >= 1.0 {
                    self.status = "context-full".to_owned();
                }
            }
        }
        self.clamp_scroll();
    }

    /// Pushes a reasoning fragment, tagged so it can be collapsed later.
    fn plain_reasoning(&mut self, text: &str) {
        let wrapped = wrap(text, self.width as usize);
        for line in wrapped {
            self.lines.push(ScreenLine {
                style: Style::default()
                    .fg(Color::Gray)
                    .add_modifier(ratatui::style::Modifier::DIM),
                text: line,
                kind: LineKind::Reasoning,
            });
        }
        if self.lines.len() > MAX_LINES {
            let overflow = self.lines.len() - MAX_LINES;
            self.lines.drain(0..overflow);
            self.scroll = self.scroll.saturating_sub(overflow as u16);
        }
    }

    /// Collapses or reveals reasoning lines in the transcript for a cleaner
    /// overview. The removed thinking is retained (in memory here, and always in
    /// the session log), so reveal restores it exactly.
    pub fn set_collapse_reasoning(&mut self, collapse: bool) {
        if collapse == self.collapse_reasoning {
            return;
        }
        self.collapse_reasoning = collapse;
        if collapse {
            let mut next: Vec<ScreenLine> = Vec::with_capacity(self.lines.len());
            let mut current_run: Vec<ScreenLine> = Vec::new();
            for line in std::mem::take(&mut self.lines) {
                if line.kind == LineKind::Reasoning {
                    current_run.push(line);
                } else if !current_run.is_empty() {
                    self.hidden_reasoning.push(std::mem::take(&mut current_run));
                    next.push(collapse_placeholder());
                }
            }
            if !current_run.is_empty() {
                self.hidden_reasoning.push(std::mem::take(&mut current_run));
                next.push(collapse_placeholder());
            }
            self.lines = next;
        } else {
            let mut next: Vec<ScreenLine> = Vec::with_capacity(self.lines.len());
            for line in std::mem::take(&mut self.lines) {
                if line.kind == LineKind::Placeholder {
                    if let Some(run) = self.hidden_reasoning.first().cloned() {
                        self.hidden_reasoning.remove(0);
                        next.extend(run);
                    }
                } else {
                    next.push(line);
                }
            }
            self.lines = next;
        }
        self.clamp_scroll();
    }

    /// Whether any reasoning has been collapsed and can be revealed.
    #[allow(dead_code)]
    pub fn can_reveal_reasoning(&self) -> bool {
        self.collapse_reasoning && !self.hidden_reasoning.is_empty()
    }

    /// The compact stats footer, or an empty string when no stats exist yet.
    pub fn stats_line(&self) -> String {
        let Some(stats) = self.stats else {
            return String::new();
        };
        let speed = format!("⚡ {:.0} tok/s  ·  ", stats.tokens_per_sec);
        let context = format!(
            "▁▃▅▇{} {:+.0}% {}/{}  ·  ",
            bargraph(stats.utilization, 8),
            stats.utilization.clamp(0.0, 1.0) * 100.0,
            stats.context_tokens,
            stats.context_limit
        );
        let eta_suffix = match stats.eta_secs {
            Some(secs) if stats.compaction_progress > 0.0 => {
                format!(" (in {})", format_eta(secs))
            }
            _ => String::new(),
        };
        let compaction = format!(
            "compaction {}{:.0}%{}",
            bargraph(stats.compaction_progress, 8),
            stats.compaction_progress.clamp(0.0, 1.0) * 100.0,
            eta_suffix
        );
        let progress = format!(
            " · {total} tok · {elapsed}",
            total = stats.total_tokens,
            elapsed = format_elapsed(stats.elapsed_secs)
        );
        format!("{speed}{context}{compaction}{progress}")
    }

    /// Processes one key event and reports what the runner should do.
    ///
    /// Editing keys (characters, Enter as newline, Backspace, arrows, Home/
    /// End, Delete, word motions) are handled by the embedded multiline editor;
    /// the control keys below are intercepted first.
    pub fn on_key(&mut self, key: KeyEvent) -> KeyAction {
        if self.show_help {
            if self.handle_help_key(key) {
                self.show_help = false;
            }
            return KeyAction::Idle;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.running {
                    self.status = "cancelling".to_owned();
                    KeyAction::Cancel
                } else {
                    self.should_quit = true;
                    KeyAction::Quit
                }
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                if self.editor.is_empty() {
                    self.should_quit = true;
                    KeyAction::Quit
                } else {
                    KeyAction::Idle
                }
            }
            // Ctrl+Enter always submits. Enter submits when the cursor is on a
            // blank line below the first (so a double-Enter sends) and otherwise
            // inserts a newline. Shift+Enter always inserts a newline, which is
            // the safe way to add blank lines inside a multi-line prompt.
            (KeyCode::Enter, KeyModifiers::CONTROL) => {
                self.submit();
                KeyAction::Submit
            }
            (KeyCode::Enter, KeyModifiers::SHIFT) => {
                self.editor.input(key);
                KeyAction::Idle
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                if self.submit_on_blank_line() {
                    KeyAction::Submit
                } else {
                    self.editor.input(key);
                    KeyAction::Idle
                }
            }
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.cycle_model(true);
                KeyAction::Idle
            }
            // Shift+Tab arrives as `BackTab` on most terminals (or as Tab with
            // SHIFT on others); both cycle the thinking effort.
            (KeyCode::BackTab, _) | (KeyCode::Tab, KeyModifiers::SHIFT) => {
                self.cycle_effort(true);
                KeyAction::Idle
            }
            (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                self.history_prev();
                KeyAction::Idle
            }
            (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                self.history_next();
                KeyAction::Idle
            }
            (KeyCode::Esc, _) => {
                self.show_help = false;
                KeyAction::Idle
            }
            (KeyCode::Char('h'), KeyModifiers::CONTROL) => {
                self.show_help = true;
                KeyAction::Idle
            }
            (KeyCode::PageUp, _) => {
                self.scroll_back(self.visible_rows());
                KeyAction::Idle
            }
            (KeyCode::PageDown, _) => {
                self.scroll_forward(self.visible_rows());
                KeyAction::Idle
            }
            (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                self.clear_screen();
                KeyAction::Idle
            }
            (KeyCode::Char('t'), KeyModifiers::CONTROL) => {
                if self.running {
                    KeyAction::Idle
                } else {
                    let reveal = self.collapse_reasoning && !self.hidden_reasoning.is_empty();
                    self.set_collapse_reasoning(!reveal);
                    KeyAction::Idle
                }
            }
            // Everything else goes to the multiline editor, which handles Enter
            // as a newline, characters, Backspace, and cursor motions.
            _ => {
                self.editor.input(key);
                KeyAction::Idle
            }
        }
    }

    fn submit(&mut self) {
        let text: String = self.editor.lines().join("\n");
        if text.trim().is_empty() {
            return;
        }
        if text.chars().count() > MAX_PROMPT_CHARS {
            self.note(
                Style::default().fg(Color::Yellow),
                &format!(
                    "prompt is longer than {MAX_PROMPT_CHARS} characters; shorten it before sending"
                ),
            );
            return;
        }
        self.history.push(text.clone());
        self.history_index = None;
        self.note(
            Style::default().fg(Color::White).bold(),
            &format!("You: {text}"),
        );
        self.editor.clear();
        if self.running {
            self.status = "queued".to_owned();
        }
        self.pending_prompt = Some(text);
    }

    /// Inserts a newline unless the cursor sits on a blank line below the first
    /// line, in which case it submits the prompt and returns true. This makes a
    /// double-Enter send while still allowing multi-line prompts to be edited.
    fn submit_on_blank_line(&mut self) -> bool {
        let (row, _) = self.editor.cursor();
        let blank = self
            .editor
            .lines()
            .get(row)
            .is_none_or(|line| line.is_empty());
        if row >= 1 && blank {
            self.submit();
            true
        } else {
            false
        }
    }

    /// Takes the staged prompt out of the app state, if one is pending.
    pub fn take_pending_prompt(&mut self) -> Option<String> {
        self.pending_prompt.take()
    }

    /// Cycles the selected model to the next configured id (Tab). The change
    /// applies to the next prompt; the runner restarts the session for it.
    fn cycle_model(&mut self, forward: bool) {
        if self.models.len() <= 1 {
            return;
        }
        let current = self
            .models
            .iter()
            .position(|id| id == &self.model)
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % self.models.len()
        } else {
            if current == 0 {
                self.models.len() - 1
            } else {
                current - 1
            }
        };
        self.model = self.models[next].clone();
        self.note(
            Style::default().fg(Color::DarkGray),
            &format!(
                "model → {} (applies to the next prompt; the session restarts)",
                self.model
            ),
        );
    }

    /// Cycles the thinking effort through the valid enum values (Shift+Tab).
    fn cycle_effort(&mut self, forward: bool) {
        let current = EFFORTS
            .iter()
            .position(|effort| *effort == self.effort)
            .unwrap_or(3);
        let next = if forward {
            (current + 1) % EFFORTS.len()
        } else {
            if current == 0 {
                EFFORTS.len() - 1
            } else {
                current - 1
            }
        };
        self.effort = EFFORTS[next];
        self.note(
            Style::default().fg(Color::DarkGray),
            &format!(
                "thinking effort → {} (applies to the next prompt; the session restarts)",
                self.effort.as_str()
            ),
        );
    }

    /// Flushes accumulated streamed text into the transcript as one paragraph.
    fn flush_pending_text(&mut self) {
        if !self.pending_text.is_empty() {
            let text = std::mem::take(&mut self.pending_text);
            self.push_wrapped(Style::default().fg(Color::Reset), &text);
        }
    }

    fn clear_screen(&mut self) {
        self.lines.clear();
        self.scroll = 0;
    }

    fn note(&mut self, style: Style, text: &str) {
        self.push_wrapped(style, text);
    }

    fn push_wrapped(&mut self, style: Style, text: &str) {
        for line in wrap(text, self.width as usize) {
            self.lines.push(ScreenLine {
                style,
                text: line,
                kind: LineKind::Normal,
            });
        }
        if self.lines.len() > MAX_LINES {
            let overflow = self.lines.len() - MAX_LINES;
            self.lines.drain(0..overflow);
            self.scroll = self.scroll.saturating_sub(overflow as u16);
        }
    }

    /// Scrolls the transcript up (toward older lines) by `amount` rows.
    /// Scrolls up. While the help overlay is open this scrolls the help;
    /// otherwise it scrolls the transcript.
    pub fn scroll_up(&mut self, amount: u16) {
        if self.show_help {
            self.help_scroll = self.help_scroll.saturating_sub(self.help_visible_rows());
        } else {
            self.scroll_back(amount);
        }
    }

    /// Scrolls down. While the help overlay is open this scrolls the help;
    /// otherwise it scrolls the transcript.
    pub fn scroll_down(&mut self, amount: u16) {
        if self.show_help {
            self.help_scroll += self.help_visible_rows();
        } else {
            self.scroll_forward(amount);
        }
    }

    /// The number of help rows visible inside the transcript box, used as the
    /// per-page help scroll step (full height minus the header, stats, input,
    /// and the box's two borders).
    fn help_visible_rows(&self) -> u16 {
        self.height.saturating_sub(8).max(1)
    }

    fn scroll_back(&mut self, amount: u16) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    fn scroll_forward(&mut self, amount: u16) {
        self.scroll += amount;
    }

    fn visible_rows(&self) -> u16 {
        self.height.saturating_sub(3).max(1)
    }

    fn clamp_scroll(&mut self) {
        let max = self
            .lines
            .len()
            .saturating_sub(self.visible_rows() as usize) as u16;
        if self.scroll > max {
            self.scroll = max;
        }
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let index = match self.history_index {
            Some(i) => i.saturating_sub(1),
            None => self.history.len() - 1,
        };
        self.history_index = Some(index);
        if let Some(text) = self.history.get(index).cloned() {
            self.restore_editor(&text);
        }
    }

    fn history_next(&mut self) {
        match self.history_index {
            Some(i) if i + 1 < self.history.len() => {
                self.history_index = Some(i + 1);
                if let Some(text) = self.history.get(i + 1).cloned() {
                    self.restore_editor(&text);
                }
            }
            _ => {
                self.history_index = None;
                self.editor.clear();
            }
        }
    }

    /// Replaces the editor contents with a history entry, preserving the
    /// multiline shape.
    fn restore_editor(&mut self, text: &str) {
        self.editor
            .set_lines(text.split('\n').map(str::to_owned).collect(), (0, 0));
    }

    fn handle_help_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => true,
            // The help is taller than the transcript box on short terminals, so
            // PageUp/PageDown (and Ctrl+P/Ctrl+N) scroll through it.
            KeyCode::PageDown => {
                self.help_scroll += self.help_visible_rows();
                false
            }
            KeyCode::PageUp => {
                self.help_scroll = self.help_scroll.saturating_sub(self.help_visible_rows());
                false
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.help_scroll += self.help_visible_rows();
                false
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.help_scroll = self.help_scroll.saturating_sub(self.help_visible_rows());
                false
            }
            _ => false,
        }
    }

    /// Drains available loop events from `rx` without blocking. Returns true if
    /// any event was processed.
    pub fn pump(&mut self, rx: &mpsc::Receiver<Event>) -> Result<bool> {
        let mut processed = false;
        while let Ok(event) = rx.try_recv() {
            self.push_event(event);
            processed = true;
        }
        Ok(processed)
    }

    /// The number of transcript lines currently displayed.
    #[allow(dead_code)]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Updates the terminal size and re-wraps the transcript at the new width.
    pub fn resize(&mut self, width: u16, height: u16) {
        let width = width.max(20);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        let wrapped: Vec<ScreenLine> = self
            .lines
            .iter()
            .flat_map(|screen_line| {
                wrap(&screen_line.text, self.width as usize)
                    .into_iter()
                    .map(|text| ScreenLine {
                        style: screen_line.style,
                        text,
                        kind: screen_line.kind,
                    })
            })
            .collect();
        self.lines = wrapped;
        self.clamp_scroll();
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(10);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_owned()
            } else {
                format!(" {}", word)
            };
            if candidate.chars().count() > width {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                for chunk in split_long(word, width) {
                    lines.push(chunk);
                }
            } else if current.chars().count() + candidate.chars().count() > width
                && !current.is_empty()
            {
                lines.push(std::mem::take(&mut current));
                current = candidate;
            } else {
                current.push_str(&candidate);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

fn split_long(word: &str, width: usize) -> Vec<String> {
    word.chars()
        .collect::<Vec<_>>()
        .chunks(width.max(1))
        .map(|chunk| chunk.iter().collect::<String>())
        .collect()
}

/// The dim placeholder line shown where thinking has been collapsed.
fn collapse_placeholder() -> ScreenLine {
    ScreenLine {
        style: Style::default().fg(Color::DarkGray),
        text: "▸ thinking hidden — press T to reveal".to_owned(),
        kind: LineKind::Placeholder,
    }
}

/// Estimates seconds until the live context reaches `context_limit`, forecast
/// from the growth between the previous and current progress samples. Returns
/// `None` when the estimate is not meaningful: no prior sample, a non-positive
/// time gap, flat or declining context (as after a compaction reset), or growth
/// below the floor. Never fabricates a number.
fn estimate_eta(
    prev: Option<(f64, usize)>,
    elapsed: f64,
    context_tokens: usize,
    context_limit: usize,
) -> Option<f64> {
    let (prev_elapsed, prev_tokens) = prev?;
    let dt = elapsed - prev_elapsed;
    if dt <= 0.0 || context_tokens <= prev_tokens {
        return None;
    }
    let rate = (context_tokens - prev_tokens) as f64 / dt;
    if rate < ETA_MIN_RATE {
        return None;
    }
    Some(context_limit.saturating_sub(context_tokens) as f64 / rate)
}

/// The minimum observed context fill rate (tokens/sec) below which an extrapolated
/// ETA would be too noisy to trust and is suppressed instead.
const ETA_MIN_RATE: f64 = 1.0;

/// Formats an estimated time-to-limit as `45s`, `2m12s`, or `3h05m`.
fn format_eta(secs: f64) -> String {
    if !secs.is_finite() {
        return "∞".to_owned();
    }
    let secs = secs.round().clamp(0.0, f64::MAX) as u64;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m{seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

/// Formats elapsed session time as `m:ss`, or `h:mm:ss` past an hour.
fn format_elapsed(secs: f64) -> String {
    let secs = secs.round().clamp(0.0, f64::MAX) as u64;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Renders a filled/empty block bargraph for a fraction in `0.0..=1.0+`.
fn bargraph(fraction: f64, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    const BLOCKS: [char; 9] = ['░', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let total = (fraction.clamp(0.0, 1.0) * width as f64).clamp(0.0, width as f64);
    let full = total.floor() as usize;
    let remainder = total - full as f64;
    let mut s = String::with_capacity(width as usize);
    for i in 0..width as usize {
        if i < full {
            s.push('█');
        } else if i == full {
            let level = (remainder * 8.0).round() as usize;
            s.push(BLOCKS[level.clamp(0, 8)]);
        } else {
            s.push('░');
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::{App, KeyAction, LineKind};
    use crate::session::Event;
    use agent_runtime::ReasoningEffort;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn models() -> Vec<String> {
        vec!["local".to_owned(), "ollama".to_owned()]
    }

    fn app() -> App {
        App::new(&models(), "local", ReasoningEffort::Medium, None, 40, 24)
    }

    fn app_at(width: u16) -> App {
        App::new(&models(), "local", ReasoningEffort::Medium, None, width, 24)
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn ch(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::CONTROL)
    }

    fn editor_text(app: &App) -> String {
        app.editor.lines().join("\n")
    }

    #[test]
    fn push_event_folds_every_event_type_into_lines() {
        let mut app = app();
        app.push_event(Event::TurnStart {
            model: "local".to_owned(),
        });
        app.push_event(Event::Reasoning("because".to_owned()));
        app.push_event(Event::Text("answer here".to_owned()));
        app.push_event(Event::ToolCall {
            name: "shell".to_owned(),
        });
        app.push_event(Event::ToolResult {
            name: "shell".to_owned(),
            failed: false,
        });
        app.push_event(Event::Note("compacted".to_owned()));
        app.push_event(Event::Finished {
            message: "done".to_owned(),
        });
        assert!(app.lines.len() >= 7, "every event produced transcript rows");
    }

    #[test]
    fn status_transitions_follow_events() {
        let mut app = app();
        app.push_event(Event::TurnStart {
            model: "local".to_owned(),
        });
        assert_eq!(app.status, "thinking");
        assert!(app.running);
        app.push_event(Event::Finished {
            message: "done".to_owned(),
        });
        assert_eq!(app.status, "done");
        assert!(!app.running);
    }

    #[test]
    fn failed_event_sets_error_status() {
        let mut app = app();
        app.push_event(Event::Failed("boom".to_owned()));
        assert_eq!(app.status, "error");
        assert!(!app.running);
    }

    #[test]
    fn scroll_is_clamped_to_line_count() {
        let mut app = app();
        for i in 0..200 {
            app.push_event(Event::Text(format!("line {i}")));
        }
        app.scroll += 10_000;
        app.clamp_scroll();
        let max = (app.lines.len() as u16).saturating_sub(app.visible_rows());
        assert!(app.scroll <= max);
    }

    #[test]
    fn help_overlay_toggles_with_ctrl_h_and_esc() {
        let mut app = app();
        assert!(!app.show_help);
        // Ctrl+H opens help (consumed, not submitted) while idle, so a literal
        // `?` reaches the editor instead.
        assert_eq!(app.on_key(ctrl(KeyCode::Char('h'))), KeyAction::Idle);
        assert!(app.show_help);
        // While help is open, `Esc` closes it and is reported as idle (not a
        // submit); typing is ignored while help is open.
        assert_eq!(app.on_key(ch(KeyCode::Esc)), KeyAction::Idle);
        assert!(!app.show_help);
    }

    #[test]
    fn ctrl_c_quits_when_idle_but_cancels_when_running() {
        let mut idle = app();
        assert_eq!(idle.on_key(ctrl(KeyCode::Char('c'))), KeyAction::Quit);
        assert!(idle.should_quit);

        let mut running = app();
        running.running = true;
        running.status = "thinking".to_owned();
        assert_eq!(running.on_key(ctrl(KeyCode::Char('c'))), KeyAction::Cancel);
        assert_eq!(running.status, "cancelling");
        assert!(running.running);
    }

    #[test]
    fn empty_prompt_is_not_submitted_but_text_is() {
        let mut app = app();
        // Ctrl+Enter on an empty editor records nothing in the transcript.
        assert_eq!(app.on_key(ctrl(KeyCode::Enter)), KeyAction::Submit);
        assert!(app.take_pending_prompt().is_none());
        assert!(!app.lines.iter().any(|line| line.text.contains("You:")));

        // Ctrl+Enter with text stages the prompt as a line and a pending send.
        app.editor.insert_str("hello");
        assert_eq!(app.on_key(ctrl(KeyCode::Enter)), KeyAction::Submit);
        assert_eq!(app.take_pending_prompt().as_deref(), Some("hello"));
        assert!(
            app.lines
                .iter()
                .any(|line| line.text.contains("You: hello"))
        );
    }

    #[test]
    fn enter_inserts_a_newline_and_ctrl_enter_sends_multiline() {
        let mut app = app();
        app.editor.insert_str("first");
        app.on_key(ch(KeyCode::Enter));
        app.editor.insert_str("second");
        assert_eq!(editor_text(&app), "first\nsecond");
        assert!(app.take_pending_prompt().is_none());

        assert_eq!(app.on_key(ctrl(KeyCode::Enter)), KeyAction::Submit);
        assert_eq!(app.take_pending_prompt().as_deref(), Some("first\nsecond"));
        assert!(app.lines.iter().any(|line| line.text.contains("first")));
        assert!(app.lines.iter().any(|line| line.text.contains("second")));
    }

    #[test]
    fn enter_submits_on_a_blank_line_and_shift_enter_adds_one() {
        let mut app = app();

        // First Enter on the empty first line inserts a newline (no submit).
        assert_eq!(app.on_key(ch(KeyCode::Enter)), KeyAction::Idle);
        assert_eq!(editor_text(&app), "\n");

        // Second Enter is on a blank line below the first: it submits, but the
        // prompt is empty so nothing is staged.
        assert_eq!(app.on_key(ch(KeyCode::Enter)), KeyAction::Submit);
        assert!(app.take_pending_prompt().is_none());

        // Type a prompt, then Shift+Enter inserts a blank line instead of
        // submitting.
        app.editor.insert_str("note");
        assert_eq!(editor_text(&app), "\nnote");
        assert_eq!(
            app.on_key(key(KeyCode::Enter, KeyModifiers::SHIFT)),
            KeyAction::Idle
        );
        assert!(app.take_pending_prompt().is_none());
        assert_eq!(editor_text(&app), "\nnote\n");

        // Type on the new line, then Enter on a line that already has text
        // inserts a newline rather than submitting.
        app.editor.insert_str("more");
        assert_eq!(editor_text(&app), "\nnote\nmore");
        assert_eq!(app.on_key(ch(KeyCode::Enter)), KeyAction::Idle);
        assert!(app.take_pending_prompt().is_none());
        assert_eq!(editor_text(&app), "\nnote\nmore\n");

        // Now on a blank line below the first; Enter submits the whole prompt.
        assert_eq!(app.on_key(ch(KeyCode::Enter)), KeyAction::Submit);
        assert_eq!(app.take_pending_prompt().as_deref(), Some("\nnote\nmore\n"));
    }

    #[test]
    fn tab_cycles_models_and_shift_tab_cycles_effort() {
        let mut app = app();
        assert_eq!(app.model, "local");
        assert_eq!(app.effort, ReasoningEffort::Medium);

        // Tab wraps forward through the configured models.
        app.on_key(ch(KeyCode::Tab));
        assert_eq!(app.model, "ollama");
        app.on_key(ch(KeyCode::Tab));
        assert_eq!(app.model, "local");

        // Shift+Tab (reported as BackTab on most terminals) walks the effort
        // enum in order; only valid enum values are ever selectable.
        app.on_key(key(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(app.effort, ReasoningEffort::High);
        app.on_key(key(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.effort, ReasoningEffort::Xhigh);
        app.on_key(key(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.effort, ReasoningEffort::Max);
        app.on_key(key(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.effort, ReasoningEffort::None);
    }

    #[test]
    fn ctrl_p_and_ctrl_n_navigate_prompt_history() {
        let mut app = app();
        for prompt in ["alpha", "beta\ngamma", "delta"] {
            app.editor.insert_str(prompt);
            app.on_key(ctrl(KeyCode::Enter));
            app.take_pending_prompt();
        }
        app.on_key(ctrl(KeyCode::Char('p')));
        assert_eq!(editor_text(&app), "delta");
        app.on_key(ctrl(KeyCode::Char('p')));
        assert_eq!(editor_text(&app), "beta\ngamma");
        app.on_key(ctrl(KeyCode::Char('p')));
        assert_eq!(editor_text(&app), "alpha");
        app.on_key(ctrl(KeyCode::Char('n')));
        assert_eq!(editor_text(&app), "beta\ngamma");
        app.on_key(ctrl(KeyCode::Char('n')));
        assert_eq!(editor_text(&app), "delta");
        app.on_key(ctrl(KeyCode::Char('n')));
        assert_eq!(editor_text(&app), "");
    }

    #[test]
    fn streamed_text_fragments_accumulate_into_one_line() {
        // Regression: streamed word fragments used to become one line each.
        let mut app = app();
        let before = app.lines.len();
        app.push_event(Event::Text("one ".to_owned()));
        app.push_event(Event::Text("two ".to_owned()));
        app.push_event(Event::Text("three".to_owned()));
        // The fragments are still pending (no newline, no flush trigger yet),
        // and once flushed they form a single transcript line.
        app.flush_pending_text();
        assert_eq!(
            app.lines.len() - before,
            1,
            "the streamed sentence occupies exactly one line"
        );
        assert!(
            app.lines
                .iter()
                .any(|line| line.text.contains("one two three"))
        );
    }

    #[test]
    fn text_newlines_flush_complete_paragraphs() {
        let mut app = app();
        let before = app.lines.len();
        app.push_event(Event::Text("para one\npara two".to_owned()));
        // "para one" is complete and wrapped immediately; "para two" stays
        // pending until the next flush trigger.
        assert!(app.lines.iter().any(|line| line.text.contains("para one")));
        assert!(!app.lines.iter().any(|line| line.text.contains("para two")));
        app.push_event(Event::Finished {
            message: "done".to_owned(),
        });
        assert!(app.lines.iter().any(|line| line.text.contains("para two")));
        assert!(app.lines.len() > before);
    }

    #[test]
    fn oversized_prompt_is_rejected() {
        let mut app = app();
        let long = "x".repeat(super::MAX_PROMPT_CHARS + 1);
        app.editor.insert_str(&long);
        assert_eq!(app.on_key(ctrl(KeyCode::Enter)), KeyAction::Submit);
        assert!(app.take_pending_prompt().is_none());
        assert!(
            app.lines
                .iter()
                .any(|line| line.text.contains("shorten it"))
        );
        // The editor keeps the text so the user can trim it.
        assert_eq!(editor_text(&app), long);
    }

    #[test]
    fn collapse_then_reveal_reasoning_restores_lines() {
        let mut app = app();
        app.push_event(Event::Reasoning("a reason".to_owned()));
        app.push_event(Event::Text("an answer".to_owned()));
        assert!(
            app.lines
                .iter()
                .any(|line| line.kind == LineKind::Reasoning)
        );

        app.set_collapse_reasoning(true);
        assert!(app.can_reveal_reasoning());
        assert!(
            !app.lines
                .iter()
                .any(|line| line.kind == LineKind::Reasoning)
        );
        assert!(
            app.lines
                .iter()
                .any(|line| line.kind == LineKind::Placeholder)
        );

        app.set_collapse_reasoning(false);
        assert!(
            app.lines
                .iter()
                .any(|line| line.kind == LineKind::Reasoning)
        );
    }

    #[test]
    fn stats_line_is_empty_until_progress_then_reports_values() {
        let idle = app();
        assert!(idle.stats_line().is_empty());

        let mut app = app();
        app.push_event(Event::Progress {
            input_tokens: 0,
            output_tokens: 0,
            context_tokens: 100,
            context_limit: 8192,
            context_utilization: 0.5,
            compaction_progress: 0.2,
            tokens_per_sec: 42.0,
            total_tokens: 512,
            elapsed_secs: 225.5,
        });
        let line = app.stats_line();
        assert!(line.contains("tok/s"));
        assert!(line.contains("8192"));
        assert!(line.contains("512 tok"));
        assert!(line.contains("3:46"));
    }

    #[test]
    fn eta_is_none_without_a_prior_sample() {
        let mut app = app();
        app.push_event(Event::Progress {
            input_tokens: 0,
            output_tokens: 0,
            context_tokens: 4000,
            context_limit: 8192,
            context_utilization: 0.5,
            compaction_progress: 0.4,
            tokens_per_sec: 50.0,
            total_tokens: 1024,
            elapsed_secs: 20.0,
        });
        assert_eq!(app.stats.unwrap().eta_secs, None);
    }

    #[test]
    fn eta_estimates_time_to_limit_when_climbing_past_warmup() {
        let mut app = app();
        // First sample: establishes the growth baseline.
        app.push_event(Event::Progress {
            input_tokens: 0,
            output_tokens: 0,
            context_tokens: 3000,
            context_limit: 8192,
            context_utilization: 0.4,
            compaction_progress: 0.3,
            tokens_per_sec: 50.0,
            total_tokens: 1024,
            elapsed_secs: 10.0,
        });
        // Second sample, 10s later: 1000 tokens added => 100 tok/s.
        app.push_event(Event::Progress {
            input_tokens: 0,
            output_tokens: 0,
            context_tokens: 4000,
            context_limit: 8192,
            context_utilization: 0.5,
            compaction_progress: 0.45,
            tokens_per_sec: 50.0,
            total_tokens: 2048,
            elapsed_secs: 20.0,
        });
        let eta = app.stats.unwrap().eta_secs;
        // 4192 tokens remain at 100 tok/s => ~42s.
        let secs = eta.expect("an ETA should be forecast while climbing");
        assert!((40.0..44.0).contains(&secs), "eta was {secs}");
    }

    #[test]
    fn eta_is_none_when_growth_is_flat() {
        let mut app = app();
        let push = |app: &mut App, ctx: usize, elapsed: f64| {
            app.push_event(Event::Progress {
                input_tokens: 0,
                output_tokens: 0,
                context_tokens: ctx,
                context_limit: 8192,
                context_utilization: 0.5,
                compaction_progress: 0.4,
                tokens_per_sec: 50.0,
                total_tokens: 1024,
                elapsed_secs: elapsed,
            });
        };
        push(&mut app, 4000, 10.0);
        push(&mut app, 4000, 20.0);
        assert_eq!(app.stats.unwrap().eta_secs, None);
    }

    #[test]
    fn eta_is_none_after_a_compaction_reset() {
        let mut app = app();
        let push = |app: &mut App, ctx: usize, elapsed: f64| {
            app.push_event(Event::Progress {
                input_tokens: 0,
                output_tokens: 0,
                context_tokens: ctx,
                context_limit: 8192,
                context_utilization: 0.5,
                compaction_progress: 0.4,
                tokens_per_sec: 50.0,
                total_tokens: 1024,
                elapsed_secs: elapsed,
            });
        };
        push(&mut app, 4000, 10.0);
        // A compaction rolled the context back down.
        push(&mut app, 2500, 20.0);
        assert_eq!(app.stats.unwrap().eta_secs, None);
    }

    #[test]
    fn eta_suffix_appears_in_stats_line_only_when_climbing() {
        let mut app = app_at(60);
        let push = |app: &mut App, ctx: usize, elapsed: f64| {
            app.push_event(Event::Progress {
                input_tokens: 0,
                output_tokens: 0,
                context_tokens: ctx,
                context_limit: 8192,
                context_utilization: 0.5,
                compaction_progress: 0.4,
                tokens_per_sec: 50.0,
                total_tokens: 1024,
                elapsed_secs: elapsed,
            });
        };
        push(&mut app, 6292, 10.0);
        assert!(!app.stats_line().contains("(in"));
        // 100s later: 1000 tokens added => 10 tok/s; 900 remain => 90s ETA.
        push(&mut app, 7292, 110.0);
        assert!(app.stats_line().contains("(in 1m30s"));
    }

    #[test]
    fn wrap_keeps_fitting_words_on_one_line() {
        // Regression: a previously broken append path replaced the accumulated
        // line with each new word, collapsing a sentence to its last word.
        assert_eq!(
            super::wrap("one two three", 20),
            vec!["one two three".to_owned()]
        );
    }

    #[test]
    fn wrap_wraps_when_a_line_overflows() {
        let rendered = super::wrap("alpha beta gamma", 10);
        assert_eq!(rendered, vec!["alpha beta".to_owned(), " gamma".to_owned()]);
    }

    #[test]
    fn narrower_width_rewraps_the_transcript_into_more_lines() {
        // Same content at two widths: the narrower view must wrap into more
        // transcript rows. Using two independent apps avoids confounding the
        // init note and re-wrap with a resize measurement.
        let mut wide = app();
        let mut narrow = app();
        narrow.resize(20, 24);
        let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa".to_owned();
        wide.push_event(Event::Text(text.clone()));
        wide.flush_pending_text();
        narrow.push_event(Event::Text(text));
        narrow.flush_pending_text();
        let wide_lines = wide.line_count();
        let narrow_lines = narrow.line_count();
        assert!(
            narrow_lines > wide_lines,
            "a narrower width should rewrap into more lines (wide={wide_lines}, narrow={narrow_lines})"
        );
    }
}
