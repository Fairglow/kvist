//! Terminal UI application state: transcript rows, selectors, input, and keys.

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use agent_runtime::ReasoningEffort;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use tui_textarea::TextArea;

use crate::error::Result;
use crate::history::{self, SessionEntry};
use crate::markdown::render_document;
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

/// A single pre-wrapped, styled transcript row.
///
/// A row holds one or more styled spans so Markdown inline formatting
/// (bold, italic, inline code, links) can mix on a single line. [`ScreenLine::md`]
/// carries the raw Markdown that produced the row; it is `Some` only on the
/// first row of a Markdown block, which lets [`App::resize`] re-render the whole
/// block at a new width instead of reflowing already-styled spans.
#[derive(Debug, Clone)]
pub struct ScreenLine {
    pub line: Line<'static>,
    pub kind: LineKind,
    pub md: Option<String>,
}

impl ScreenLine {
    /// A plain, single-span row styled uniformly.
    fn plain(text: impl Into<String>, style: Style, kind: LineKind) -> Self {
        ScreenLine {
            line: Line::from(vec![Span::styled(text.into(), style)]),
            kind,
            md: None,
        }
    }
}

/// Maximum number of transcript lines retained before dropping the oldest.
const MAX_LINES: usize = 5000;
/// The braille spinner frames cycled in the header while a turn generates.
const SPINNER_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
/// Milliseconds per spinner frame; the loop redraws each tick, so this sets the
/// visible spin rate.
const SPINNER_FRAME_MS: u128 = 100;
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

/// A modal overlay that takes over the transcript box. While any overlay other
/// than [`Overlay::None`] is active, navigation keys drive the overlay and the
/// prompt editor is inert, so menus and replays never mix with live input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// The normal transcript, editor, and live session.
    None,
    /// The `Esc` action menu: new session, session history, and quit.
    Menu,
    /// The scrollable list of past sessions.
    History,
    /// A read-only replay of one past session's transcript.
    Replay,
}

/// The `Esc` menu actions, in display order, with their single-letter hotkeys.
pub const MENU_ITEMS: [&str; 3] = ["New session", "Session history", "Quit"];
pub const MENU_HOTKEYS: [char; 3] = ['n', 'h', 'q'];

/// The interactive application state driven by events and key input.
pub struct App {
    pub lines: Vec<ScreenLine>,
    pub scroll: u16,
    /// Auto-follow pin: while true the transcript stays pinned to the newest
    /// line as output arrives. Scrolling up unpins; Ctrl+End re-pins.
    following: bool,
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
    /// Wall-clock time the current turn began generating, so the header can
    /// derive an animated spinner frame and the UI can tell "working" from
    /// "done"/"awaiting". `None` while idle, set on `TurnStart`, cleared on any
    /// terminal event (`Finished`, `PromptEnd`, `Failed`).
    pub running_since: Option<Instant>,
    pub show_help: bool,
    pub overlay: Overlay,
    pub status: String,
    pub should_quit: bool,
    /// The highlighted index into [`MENU_ITEMS`] while the action menu is open.
    pub menu_selection: usize,
    /// The past sessions available when the history overlay is open.
    pub history_items: Vec<SessionEntry>,
    /// The scroll offset of the history list.
    pub history_scroll: u16,
    /// The highlighted index into `history_items`.
    pub history_selection: usize,
    /// The transcript lines shown while the replay overlay is open.
    pub replay_lines: Vec<String>,
    /// The session id labelled on the replay overlay.
    pub replay_title: String,
    /// The scroll offset of the replay transcript.
    pub replay_scroll: u16,
    /// Where to look for past-session transcripts, resolved from overrides.
    log_dir: Option<PathBuf>,
    pub width: u16,
    pub height: u16,
    /// The staged prompt awaiting dispatch, if any.
    pending_prompt: Option<String>,
    /// Streamed answer text not yet flushed to the transcript (incomplete
    /// paragraphs); flushed at paragraph boundaries and on any non-text event.
    pending_text: String,
    /// Streamed reasoning text not yet flushed to the transcript, kept
    /// separately from the answer and tagged as reasoning on flush.
    pending_reasoning: String,
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
    /// Whether the transcript shows a right-edge scrollbar. Toggled with
    /// Ctrl+S. The scrollbar is purely on-screen: it never affects the transcript
    /// content or what copy emits.
    pub show_scrollbar: bool,
    /// The OSC 52 escape that copies the cleaned transcript to the terminal, if
    /// a copy is pending but the event loop has not emitted it yet. Kept here so
    /// a double copy cannot resend a stale sequence.
    pending_osc_52: Option<String>,
    /// The last escape `copy_to_clipboard` produced, used to skip re-staging an
    /// identical copy (so a repeat Ctrl+Insert with no new content does not spam
    /// the terminal with another OSC 52 sequence).
    last_copied_escape: Option<String>,
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
                    " prompt · Enter/Ctrl+Enter send · Shift+Enter · Tab model · Shift+Tab effort · Ctrl+Insert copy · Ctrl+S scroll · Ctrl-Q quit · Esc menu · Ctrl+H help ",
                    Style::default().fg(Color::DarkGray),
                )),
        );
        let mut app = App {
            lines: Vec::new(),
            scroll: 0,
            following: true,
            help_scroll: 0,
            editor,
            history: Vec::new(),
            history_index: None,
            models: models.to_vec(),
            model: model_id.to_owned(),
            effort,
            running: false,
            running_since: None,
            show_help: false,
            overlay: Overlay::None,
            status: "idle".to_owned(),
            should_quit: false,
            menu_selection: 0,
            history_items: Vec::new(),
            history_scroll: 0,
            history_selection: 0,
            replay_lines: Vec::new(),
            replay_title: String::new(),
            replay_scroll: 0,
            log_dir: None,
            width,
            height,
            pending_prompt: None,
            pending_text: String::new(),
            pending_reasoning: String::new(),
            config_path,
            stats: None,
            last_growth: None,
            collapse_reasoning: false,
            hidden_reasoning: Vec::new(),
            show_scrollbar: false,
            pending_osc_52: None,
            last_copied_escape: None,
        };
        app.note(
            Style::default().fg(Color::Cyan),
            "agent-runner ready. Type a prompt and press Ctrl+Enter. Ctrl-Q quits; Esc opens the menu; Ctrl+H helps.",
        );
        app
    }

    /// Stages a prefilled, auto-started prompt for a caller (for example
    /// `kvist prompt`) that wants the agent to begin working immediately without
    /// the user typing. The text is shown in the editor so it stays editable,
    /// announced in the transcript, and queued for dispatch on the next loop
    /// tick. The same length guard as interactive submission keeps the agent
    /// transcript valid; an over-long prompt is shown but left editable rather
    /// than submitted.
    pub fn stage_initial_prompt(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        if text.chars().count() > MAX_PROMPT_CHARS {
            self.note(
                Style::default().fg(Color::White).bold(),
                &format!("You: {text} (too long; shown but not started)"),
            );
            self.note(
                Style::default().fg(Color::Yellow),
                &format!(
                    "prompt is longer than {MAX_PROMPT_CHARS} characters; shorten it before sending"
                ),
            );
            return;
        }
        self.editor.set_lines(vec![text.clone()], (0, 0));
        self.note(
            Style::default().fg(Color::White).bold(),
            &format!("You: {text} (auto-started)"),
        );
        self.pending_prompt = Some(text);
        self.status = "queued".to_owned();
    }

    /// Feeds one loop event into the transcript, wrapping at the current width.
    pub fn push_event(&mut self, event: Event) {
        match event {
            Event::TurnStart { model } => {
                self.flush_pending();
                self.running = true;
                self.running_since = Some(Instant::now());
                self.status = "thinking".to_owned();
                self.note(
                    Style::default().fg(Color::Magenta),
                    &format!("▍ {model} is working…"),
                );
            }
            Event::Reasoning(text) => self.stream_reasoning(&text),
            // Streamed answer text arrives fragment-by-fragment. A model often
            // emits a trailing newline after each token or line; a lone newline
            // is a soft break (a space), while a blank line is a real paragraph
            // boundary. Accumulate and flush only at paragraph boundaries so a
            // sentence wraps horizontally instead of one token per line.
            Event::Text(text) => {
                // Reasoning generated before this answer text is temporally
                // older, so flush any pending reasoning first to keep the
                // transcript in generation order.
                self.flush_pending_reasoning();
                let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
                self.pending_text.push_str(&normalized);
                self.flush_paragraphs();
            }
            Event::ToolCall { description, .. } => {
                self.flush_pending();
                self.note(
                    Style::default().fg(Color::Blue),
                    &format!("→ tool: {description}"),
                );
            }
            Event::ToolResult {
                description,
                failed,
                ..
            } => {
                self.flush_pending();
                let style = if failed {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                };
                self.note(style, &format!("✓ tool {description} finished"));
            }
            Event::Finished { message } => {
                self.flush_pending();
                self.quit_running();
                self.status = "done".to_owned();
                self.note(Style::default().fg(Color::Green), &format!("✓ {message}"));
            }
            // The prompt loop exited with no answer. Without this the UI would
            // keep showing "working…" forever once the single-turn cap (or the
            // multi-turn limit) returns control, so the user cannot tell the
            // agent stopped. A clear, distinct state removes that ambiguity.
            Event::PromptEnd {
                exhausted,
                cancelled,
            } => {
                self.quit_running();
                if cancelled {
                    self.status = "cancelled".to_owned();
                    self.note(
                        Style::default().fg(Color::Yellow),
                        "cancelled — type a prompt to continue",
                    );
                } else if exhausted {
                    self.status = "exhausted".to_owned();
                    self.note(
                        Style::default().fg(Color::Yellow),
                        "single-turn cap reached with no answer — send a follow-up to continue",
                    );
                } else {
                    self.status = "awaiting".to_owned();
                    self.note(
                        Style::default().fg(Color::Yellow),
                        "no answer this turn — send a follow-up to continue",
                    );
                }
            }
            Event::Failed(text) => {
                self.flush_pending();
                self.quit_running();
                self.status = "error".to_owned();
                self.note(Style::default().fg(Color::Red), &text);
            }
            Event::Note(text) => {
                self.flush_pending();
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

    /// Accumulates streamed reasoning fragments and flushes completed paragraphs.
    /// Like streamed answer text, a lone newline is a soft break (a space) while
    /// a blank line is a real paragraph boundary, so thinking flows on rows
    /// instead of one short row per delta.
    fn stream_reasoning(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.pending_reasoning.push_str(&normalized);
        self.flush_reasoning_paragraphs();
    }

    /// Flushes every completed reasoning paragraph in the pending buffer.
    fn flush_reasoning_paragraphs(&mut self) {
        while let Some(pos) = self.pending_reasoning.find("\n\n") {
            let head = self.pending_reasoning[..pos].to_owned();
            let tail = self.pending_reasoning[pos + 2..].to_owned();
            self.push_reasoning_lines(&normalize_soft_breaks(&head));
            self.pending_reasoning = tail;
        }
    }

    /// Flushes all pending reasoning, normalizing soft breaks to spaces so the
    /// trailing fragment of thinking reads horizontally.
    fn flush_pending_reasoning(&mut self) {
        if !self.pending_reasoning.is_empty() {
            let text = std::mem::take(&mut self.pending_reasoning);
            self.push_reasoning_lines(&normalize_soft_breaks(&text));
        }
    }

    /// Pushes wrapped reasoning lines, dimmed and tagged so they can be
    /// collapsed later, then keeps the transcript bounded and followed.
    fn push_reasoning_lines(&mut self, text: &str) {
        let style = Style::default()
            .fg(Color::Gray)
            .add_modifier(ratatui::style::Modifier::DIM);
        for line in wrap(text, self.content_width()) {
            self.lines
                .push(ScreenLine::plain(line, style, LineKind::Reasoning));
        }
        self.maybe_truncate();
        self.follow();
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
        self.follow();
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
        // Modal overlays intercept their own keys before the editor or live keys.
        match self.overlay {
            Overlay::Menu => return self.handle_menu_key(key),
            Overlay::History => return self.handle_history_key(key),
            Overlay::Replay => return self.handle_replay_key(key),
            Overlay::None => {}
        }
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
            // Ctrl-Q is the reliable, immediate exit: it quits regardless of a
            // running turn or a staged prompt, unlike Ctrl+C which needs a
            // second press once idle and Ctrl-D only quits with an empty prompt.
            (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                self.should_quit = true;
                KeyAction::Quit
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
                // Esc closes the help overlay, otherwise it opens the action menu.
                if self.show_help {
                    self.show_help = false;
                } else {
                    self.open_menu();
                }
                KeyAction::Idle
            }
            (KeyCode::Char('h'), KeyModifiers::CONTROL) => {
                self.show_help = true;
                KeyAction::Idle
            }
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                self.show_scrollbar = !self.show_scrollbar;
                KeyAction::Idle
            }
            (KeyCode::PageUp, _) => {
                self.following = false;
                self.scroll_back(self.visible_rows());
                KeyAction::Idle
            }
            (KeyCode::PageDown, _) => {
                self.scroll_forward(self.visible_rows());
                self.clamp_scroll();
                if self.scroll == self.bottom_offset() {
                    self.following = true;
                }
                KeyAction::Idle
            }
            // Ctrl+End jumps to the newest output and resumes auto-follow;
            // plain End stays with the editor for cursor motions.
            (KeyCode::End, KeyModifiers::CONTROL) => {
                self.scroll = self.bottom_offset();
                self.following = true;
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
            // Ctrl+Insert copies the transcript to the clipboard via the OSC 52
            // escape (see copy_to_clipboard). Ctrl+C is handled above: a running
            // turn cancels, an idle Ctrl+C quits.
            (KeyCode::Char('c'), KeyModifiers::SHIFT)
            | (KeyCode::Insert, KeyModifiers::CONTROL) => {
                self.copy_to_clipboard();
                KeyAction::Idle
            }
            // Everything else goes to the multiline editor, which handles Enter
            // as a newline, characters, Backspace, and cursor motions.
            _ => {
                self.editor.input(key);
                KeyAction::Idle
            }
        }
    }

    /// Builds the OSC 52 clipboard escape that copies the transcript to the
    /// terminal's primary selection.
    ///
    /// The transcript lines are pre-wrapped at the *inner* width (width minus the
    /// two border columns), so they never contain the box's `│` characters — the
    /// frame is drawn solely by the terminal widget and never reaches the buffer.
    /// Copy therefore emits the stored lines verbatim, then strips trailing
    /// whitespace from each line so nothing but real content lands on the
    /// clipboard. The content is base64-encoded and wrapped in the OSC 52 escape
    /// (ST via the ANSI bell); a terminal that speaks OSC 52 (Alacritty, Kitty,
    /// iTerm2, WezTerm, GNOME Terminal) then places it on the clipboard.
    fn clipboard_escape(&self) -> Option<String> {
        // Strip trailing whitespace per line, drop fully-empty trailing lines so
        // pasted content reads cleanly, and join without a trailing newline so a
        // pasted block has no dangling line break.
        let mut cleaned: Vec<String> = Vec::new();
        for line in &self.lines {
            let text = line.line.to_string();
            let trimmed = text.trim_end().to_owned();
            // Keep internal blank lines (blank paragraphs) but remember whether
            // the very last lines were empty so we can trim the tail.
            cleaned.push(trimmed);
        }
        while cleaned.last().is_some_and(|line| line.is_empty()) {
            cleaned.pop();
        }
        if cleaned.is_empty() {
            return None;
        }
        let content = cleaned.join("\n");
        let encoded = base64_encode(content.as_bytes());
        let mut escape = String::with_capacity(encoded.len() + 8);
        escape.push('\u{1b}');
        escape.push_str("]52;c;");
        escape.push_str(&encoded);
        escape.push('\u{7}');
        Some(escape)
    }

    /// Stages a copy of the transcript to the clipboard escape. The escape is
    /// emitted to stdout by the event loop via [`App::emit_pending_osc_52`] so the
    /// write stays out of the key handler's synchronous path.
    fn copy_to_clipboard(&mut self) {
        let escape = self.clipboard_escape();
        if escape.as_deref() == self.last_copied_escape.as_deref() {
            return;
        }
        self.last_copied_escape = escape.clone();
        self.pending_osc_52 = escape;
    }

    /// The OSC 52 escape awaiting emission, if any. Called by the event loop so
    /// the write goes through the loop's stdout handle and the escape is cleared
    /// after being sent.
    pub fn take_pending_osc_52(&mut self) -> Option<String> {
        self.pending_osc_52.take()
    }

    /// Emits any staged OSC 52 clipboard escape to the terminal's stdout,
    /// clearing the pending copy once it has been sent.
    pub fn emit_pending_osc_52(&mut self) -> Result<()> {
        if let Some(escape) = self.take_pending_osc_52() {
            self.pending_osc_52 = None;
            std::io::stdout().write_all(escape.as_bytes())?;
        }
        Ok(())
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

    /// Flushes accumulated streamed text into the transcript. Whatever remains
    /// (an incomplete paragraph or an unclosed fence) is rendered as Markdown so
    /// a trailing fragment still reads horizontally.
    fn flush_pending_text(&mut self) {
        if !self.pending_text.is_empty() {
            let block = std::mem::take(&mut self.pending_text);
            self.push_markdown_block(block);
        }
    }

    /// Flushes every complete block in the pending buffer. A block is either a
    /// paragraph (terminated by a blank line, `\n\n`) or a fenced code block
    /// (terminated by its closing fence). Lone newlines are soft breaks kept in
    /// the buffer until a flush trigger, so a streamed sentence flows on rows
    /// rather than one short row per token. Fences are matched so a code block
    /// is never split across flushes even when it contains blank lines.
    fn flush_paragraphs(&mut self) {
        while let Some(end) = next_block_end(&self.pending_text) {
            let block = self.pending_text[..end].to_owned();
            let tail = self.pending_text[end..].to_owned();
            self.push_markdown_block(block);
            self.pending_text = tail;
        }
    }

    /// Renders one Markdown block and appends its styled rows to the transcript.
    /// Blank-only blocks (e.g. the separator consumed with a paragraph) add
    /// nothing, and the raw source is retained on the first row so resize can
    /// re-render the block at a new width.
    fn push_markdown_block(&mut self, md: String) {
        if md.trim().is_empty() {
            return;
        }
        let rows = render_document(&md, self.content_width());
        for (index, row) in rows.into_iter().enumerate() {
            self.lines.push(ScreenLine {
                line: row.line,
                kind: LineKind::Normal,
                md: if index == 0 { Some(md.clone()) } else { None },
            });
        }
        self.maybe_truncate();
        self.follow();
    }

    /// Flushes accumulated streamed text and reasoning together so the transcript
    /// advances at the same boundaries (tool events, turn finish, notes, ...).
    fn flush_pending(&mut self) {
        self.flush_pending_text();
        self.flush_pending_reasoning();
    }

    /// Evicts the oldest lines past MAX_LINES so the transcript stays bounded,
    /// keeping the scroll offset anchored to the bottom while doing so.
    fn maybe_truncate(&mut self) {
        if self.lines.len() > MAX_LINES {
            let overflow = self.lines.len() - MAX_LINES;
            self.lines.drain(0..overflow);
            self.scroll = self.scroll.saturating_sub(overflow as u16);
        }
    }

    /// The scroll offset that views the newest (last) line, i.e. the bottom.
    fn bottom_offset(&self) -> u16 {
        self.lines
            .len()
            .saturating_sub(self.visible_rows() as usize) as u16
    }

    /// While the view is pinned to the bottom (auto-follow), keep it pinned as
    /// new lines are appended. Escaping upward with a scroll unpins this.
    fn follow(&mut self) {
        if self.following {
            self.scroll = self.bottom_offset();
        }
    }

    fn clear_screen(&mut self) {
        self.lines.clear();
        self.scroll = 0;
    }

    fn note(&mut self, style: Style, text: &str) {
        self.push_wrapped(style, text);
    }

    /// Leaves the "working" state: clears the running flag and its spinner
    /// timer so the header stops animating once control has returned to the
    /// caller. Shared by every terminal event so the UI never looks stuck.
    fn quit_running(&mut self) {
        self.running = false;
        self.running_since = None;
    }

    /// The animated spinner frame for the header, or `None` while idle. The
    /// frame advances with wall-clock time (the loop redraws each tick), so a
    /// running turn shows motion an idle one does not — a quick way to tell the
    /// agent is generating versus having stopped.
    pub fn spinner(&self) -> Option<&'static str> {
        let since = self.running_since?;
        let frame = since.elapsed().as_millis() / SPINNER_FRAME_MS;
        let index = (frame % SPINNER_FRAMES.len() as u128) as usize;
        Some(SPINNER_FRAMES[index])
    }

    fn push_wrapped(&mut self, style: Style, text: &str) {
        for line in wrap(text, self.content_width()) {
            self.lines
                .push(ScreenLine::plain(line, style, LineKind::Normal));
        }
        self.maybe_truncate();
        self.follow();
    }

    /// Scrolls the transcript up (toward older lines) by `amount` rows.
    /// Scrolls up. While the help overlay is open this scrolls the help;
    /// otherwise it scrolls the transcript.
    /// Scrolls up (toward older lines). Moving away from the bottom unpins
    /// auto-follow so the user can read without the view being yanked down.
    pub fn scroll_up(&mut self, amount: u16) {
        match self.overlay {
            Overlay::History => self.history_scroll_up(),
            Overlay::Replay => {
                self.following = false;
                let bottom = self.replay_bottom();
                self.replay_scroll = self.replay_scroll.saturating_sub(amount).min(bottom);
            }
            _ => {
                if self.show_help {
                    self.help_scroll = self.help_scroll.saturating_sub(self.help_visible_rows());
                } else {
                    self.following = false;
                    self.scroll_back(amount);
                }
            }
        }
    }

    /// Scrolls down (toward newer lines). While a history overlay is open this
    /// scrolls the list; while a replay is open it scrolls the transcript;
    /// while the help overlay is open it scrolls the help; otherwise reaching
    /// the bottom re-pins auto-follow.
    pub fn scroll_down(&mut self, amount: u16) {
        match self.overlay {
            Overlay::History => self.history_scroll_down(),
            Overlay::Replay => {
                self.following = false;
                let bottom = self.replay_bottom();
                self.replay_scroll = self.replay_scroll.saturating_add(amount).min(bottom);
            }
            _ => {
                if self.show_help {
                    self.help_scroll += self.help_visible_rows();
                } else {
                    self.scroll_forward(amount);
                    self.clamp_scroll();
                    if self.scroll == self.bottom_offset() {
                        self.following = true;
                    }
                }
            }
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

    /// The transcript's inner character width, subtracting the box's left and
    /// right borders so pre-wrapped lines never extend past the visible area.
    fn content_width(&self) -> usize {
        self.width.saturating_sub(2).max(1) as usize
    }

    /// The number of content rows visible inside the transcript box, used for
    /// scroll bounds and page-scroll steps. Equal to the full height minus the
    /// header, stats, input rows, and the box's two borders.
    fn visible_rows(&self) -> u16 {
        self.height.saturating_sub(8).max(1)
    }

    fn clamp_scroll(&mut self) {
        let max = self.bottom_offset();
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

    /// Opens the action overlay menu.
    pub fn open_menu(&mut self) {
        self.menu_selection = 0;
        self.overlay = Overlay::Menu;
    }

    /// Opens the session-history overlay, listing past transcripts.
    fn open_history(&mut self) {
        let dir = self
            .log_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(crate::session_log::DEFAULT_LOG_DIR));
        self.history_items = history::list_sessions(&dir);
        self.history_selection = 0;
        self.history_scroll = 0;
        if self.history_items.is_empty() {
            self.note(
                Style::default().fg(Color::Yellow),
                "no past sessions found in the log directory",
            );
        }
        self.overlay = Overlay::History;
    }

    /// Loads one past session's transcript for a read-only replay.
    fn start_replay(&mut self, entry: &SessionEntry) {
        match history::transcript_lines(&entry.path) {
            Ok(lines) => {
                self.replay_lines = lines;
                self.replay_title = entry.id.clone();
                self.replay_scroll = self.replay_bottom();
                self.following = false;
                self.overlay = Overlay::Replay;
            }
            Err(error) => {
                self.note(
                    Style::default().fg(Color::Red),
                    &format!("could not load transcript: {error}"),
                );
            }
        }
    }

    /// Starts a fresh slate: clears the transcript and returns to idle while
    /// keeping the selected model and effort.
    fn new_session(&mut self) {
        self.clear_screen();
        self.following = true;
        self.status = "idle".to_owned();
        self.note(
            Style::default().fg(Color::Cyan),
            "new session — transcript cleared; type a prompt below",
        );
    }

    /// Runs the action chosen by the highlighted menu item.
    fn dispatch_menu(&mut self) {
        match self.menu_selection {
            0 => self.new_session(),
            1 => self.open_history(),
            _ => self.should_quit = true,
        }
    }

    /// Handles keys while the action menu is open.
    fn handle_menu_key(&mut self, key: KeyEvent) -> KeyAction {
        match key.code {
            KeyCode::Esc | KeyCode::Char('r') => {
                self.overlay = Overlay::None;
                KeyAction::Idle
            }
            KeyCode::Char('q') => {
                self.should_quit = true;
                KeyAction::Quit
            }
            KeyCode::Char('n') => {
                self.new_session();
                KeyAction::Idle
            }
            KeyCode::Char('h') => {
                self.open_history();
                KeyAction::Idle
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.menu_selection = self
                    .menu_selection
                    .saturating_add(1)
                    .min(MENU_ITEMS.len() - 1);
                KeyAction::Idle
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.menu_selection = self.menu_selection.saturating_sub(1);
                KeyAction::Idle
            }
            KeyCode::Enter => {
                self.dispatch_menu();
                KeyAction::Idle
            }
            _ => KeyAction::Idle,
        }
    }

    /// Handles keys while the session-history list is open.
    fn handle_history_key(&mut self, key: KeyEvent) -> KeyAction {
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::Menu;
                KeyAction::Idle
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.history_scroll_up();
                KeyAction::Idle
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.history_scroll_down();
                KeyAction::Idle
            }
            KeyCode::PageUp => {
                for _ in 0..self.visible_rows() {
                    self.history_scroll_up();
                }
                KeyAction::Idle
            }
            KeyCode::PageDown => {
                for _ in 0..self.visible_rows() {
                    self.history_scroll_down();
                }
                KeyAction::Idle
            }
            KeyCode::Enter => {
                if let Some(entry) = self.history_items.get(self.history_selection).cloned() {
                    self.start_replay(&entry);
                }
                KeyAction::Idle
            }
            _ => KeyAction::Idle,
        }
    }

    /// Handles keys while a past session's transcript is replayed. The editor is
    /// inert here: navigation scrolls the transcript, Esc returns to the list.
    fn handle_replay_key(&mut self, key: KeyEvent) -> KeyAction {
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::History;
                KeyAction::Idle
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.replay_scroll = self.replay_scroll.saturating_sub(1);
                KeyAction::Idle
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.replay_scroll = self.replay_scroll.min(self.replay_bottom());
                KeyAction::Idle
            }
            KeyCode::PageUp => {
                self.replay_scroll = self.replay_scroll.saturating_sub(self.visible_rows());
                KeyAction::Idle
            }
            KeyCode::PageDown => {
                self.replay_scroll = self.replay_scroll.min(self.replay_bottom());
                KeyAction::Idle
            }
            _ => KeyAction::Idle,
        }
    }

    /// Scrolls the history list up by one row, keeping the selection and bounds
    /// in step. A short list needs no scrolling.
    fn history_scroll_up(&mut self) {
        if self.history_items.is_empty() {
            return;
        }
        self.history_scroll = self
            .history_scroll
            .saturating_sub(1)
            .min(self.history_max_scroll());
        self.history_selection = self.history_selection.saturating_sub(1);
    }

    /// Scrolls the history list down by one row, clamping to the last item.
    fn history_scroll_down(&mut self) {
        if self.history_items.is_empty() {
            return;
        }
        self.history_scroll = self
            .history_scroll
            .saturating_add(1)
            .min(self.history_max_scroll());
        self.history_selection = self
            .history_selection
            .saturating_add(1)
            .min(self.history_items.len().saturating_sub(1));
    }

    /// The largest scroll offset that still shows the last history row.
    fn history_max_scroll(&self) -> u16 {
        self.history_items
            .len()
            .saturating_sub(self.visible_rows() as usize) as u16
    }

    /// The largest scroll offset that still shows the last replay row.
    fn replay_bottom(&self) -> u16 {
        self.replay_lines
            .len()
            .saturating_sub(self.visible_rows() as usize) as u16
    }

    /// Sets the directory the history overlay reads past transcripts from. The
    /// run loop resolves this from the overrides so the history overlay and the
    /// durable worker log the same directory.
    pub fn set_log_dir(&mut self, dir: PathBuf) {
        self.log_dir = Some(dir);
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
        let target = self.content_width();
        let mut wrapped: Vec<ScreenLine> = Vec::new();
        let mut index = 0;
        while index < self.lines.len() {
            if let Some(source) = &self.lines[index].md.clone() {
                // Re-render a whole Markdown block at the new width, preserving
                // its block marker on the first row for any later resize.
                let rows = render_document(source, target);
                let mut next = index + 1;
                while next < self.lines.len() && self.lines[next].md.is_none() {
                    next += 1;
                }
                for (offset, row) in rows.into_iter().enumerate() {
                    wrapped.push(ScreenLine {
                        line: row.line,
                        kind: LineKind::Normal,
                        md: if offset == 0 {
                            Some(source.clone())
                        } else {
                            None
                        },
                    });
                }
                index = next;
            } else {
                // A plain row reflows at the new width using its style and kind.
                let style = self.lines[index]
                    .line
                    .spans
                    .first()
                    .map(|span| span.style)
                    .unwrap_or_default();
                let text = self.lines[index].line.to_string();
                for line in wrap(&text, target) {
                    wrapped.push(ScreenLine::plain(line, style, self.lines[index].kind));
                }
                index += 1;
            }
        }
        self.lines = wrapped;
        self.clamp_scroll();
    }
}

/// Writes the OSC 52 clipboard escape to stdout. Called by the event loop after
/// [`App::emit_pending_osc_52`] reports pending content, so the write happens on
/// the loop's stdout handle rather than from the synchronous key handler.
///
/// Returns `Ok(())` when nothing was staged, so the loop can call it unconditionally.
pub fn emit_pending_osc_52(app: &mut App) -> Result<()> {
    app.emit_pending_osc_52()
}

/// Minimal base64 encoder for the OSC 52 clipboard escape. Keeps `agent-runner`
/// free of an external clipboard/base64 dependency while remaining correct for
/// the small, UTF-8-encoded transcripts it copies.
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18 & 0x3f) as usize] as char);
        out.push(TABLE[(n >> 12 & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
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

/// Converts soft line breaks (lone `\r`/`\n`) to spaces so a streamed answer
/// reads horizontally. Blank lines already delimited paragraphs and never
/// reach here; paragraph breaks are handled by [`App::flush_paragraphs`].
fn normalize_soft_breaks(text: &str) -> String {
    text.replace('\n', " ")
}

fn split_long(word: &str, width: usize) -> Vec<String> {
    word.chars()
        .collect::<Vec<_>>()
        .chunks(width.max(1))
        .map(|chunk| chunk.iter().collect::<String>())
        .collect()
}

/// Returns the byte length of the next complete Markdown block in `pending`, or
/// `None` when the pending buffer holds no *closed* block yet (only an open
/// paragraph without a blank line, or an unmatched code fence). The caller
/// removes that prefix before flushing the next block.
///
/// A paragraph block ends at the next blank line (`\n\n`); a fenced code block
/// ends at a matching closer fence. Blank lines between the opening fence and
/// its closer are part of the code, never a paragraph boundary.
fn next_block_end(pending: &str) -> Option<usize> {
    let stripped = pending.trim_start_matches('\n');
    let lead = pending.len() - stripped.len();
    if stripped.is_empty() {
        // Only trailing newlines remain; nothing to flush.
        return None;
    }
    let first_line_end = stripped.find('\n').unwrap_or(stripped.len());
    let first_line = &stripped[..first_line_end];
    if let Some((ch, len, _info)) = fence_split(first_line) {
        // Scan for a matching closer fence; only fence characters, no content.
        let mut rest = &stripped[first_line_end + 1..];
        let mut consumed = first_line_end + 1;
        loop {
            let line_end = rest.find('\n').unwrap_or(rest.len());
            let line = &rest[..line_end];
            consumed += line_end;
            if let Some((closer, clo_len, rest_str)) = fence_split(line)
                && closer == ch
                && clo_len >= len
                && rest_str.trim().is_empty()
            {
                return Some(consumed);
            }
            if line_end >= rest.len() {
                // Reached the end without a matching closer.
                return None;
            }
            consumed += 1; // the newline after this line
            rest = &rest[line_end + 1..];
        }
    } else {
        // A paragraph: flush up to and including the next blank line.
        stripped.find("\n\n").map(|pos| lead + pos + 2)
    }
}

/// Recognises a code fence opener or closer and returns its character, run
/// length, and the (possibly empty) info string that follows the run. Opening
/// fences allow an info string (`rust`, `python 3`, ...); closers must contain
/// only fence characters.
fn fence_split(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let ch = chars[0];
    if ch != '`' && ch != '~' {
        return None;
    }
    let run = chars.iter().take_while(|c| **c == ch).count();
    if run < 3 {
        return None;
    }
    Some((ch, run, &trimmed[run..]))
}

/// The dim placeholder line shown where thinking has been collapsed.
fn collapse_placeholder() -> ScreenLine {
    ScreenLine::plain(
        "▸ thinking hidden — press T to reveal",
        Style::default().fg(Color::DarkGray),
        LineKind::Placeholder,
    )
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
    use super::{App, KeyAction, LineKind, MENU_ITEMS, Overlay, Style};
    use crate::session::Event;
    use agent_runtime::ReasoningEffort;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn models() -> Vec<String> {
        vec!["local".to_owned(), "ollama".to_owned()]
    }

    fn app() -> App {
        App::new(&models(), "local", ReasoningEffort::Medium, None, 40, 24)
    }

    /// An app with the startup welcome note cleared, so markdown-rendering
    /// tests can index the transcript from the first rendered row.
    fn clean_app() -> App {
        let mut app = app();
        app.lines.clear();
        app
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
            description: "ls".to_owned(),
            name: "shell".to_owned(),
        });
        app.push_event(Event::ToolResult {
            description: "ls".to_owned(),
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
        assert!(
            !app.lines
                .iter()
                .any(|line| line.line.to_string().contains("You:")),
            "user prompt lines should be plain text"
        );

        // Ctrl+Enter with text stages the prompt as a line and a pending send.
        app.editor.insert_str("hello");
        assert_eq!(app.on_key(ctrl(KeyCode::Enter)), KeyAction::Submit);
        assert_eq!(app.take_pending_prompt().as_deref(), Some("hello"));
        assert!(
            app.lines
                .iter()
                .any(|line| line.line.to_string().contains("You: hello"))
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
        assert!(
            app.lines
                .iter()
                .any(|line| line.line.to_string().contains("first"))
        );
        assert!(
            app.lines
                .iter()
                .any(|line| line.line.to_string().contains("second"))
        );
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
                .any(|line| line.line.to_string().contains("one two three"))
        );
    }

    #[test]
    fn text_newlines_flush_complete_paragraphs() {
        let mut app = app();
        let before = app.lines.len();
        // A blank line is a paragraph boundary; the first paragraph flushes
        // immediately, the second stays pending until the next flush trigger.
        app.push_event(Event::Text("para one\n\npara two".to_owned()));
        assert!(
            app.lines
                .iter()
                .any(|line| line.line.to_string().contains("para one"))
        );
        assert!(
            !app.lines
                .iter()
                .any(|line| line.line.to_string().contains("para two"))
        );
        app.push_event(Event::Finished {
            message: "done".to_owned(),
        });
        assert!(
            app.lines
                .iter()
                .any(|line| line.line.to_string().contains("para two"))
        );
        assert!(app.lines.len() > before);
    }

    #[test]
    /// Regression: streamed fragments that each end with a newline (as a model
    /// frequently emits) must concatenate into flowing text rather than one
    /// short row per token.
    fn streamed_newlines_concatenate_into_flowing_text() {
        let mut app = app();
        for frag in ["The ", "quick\n", "brown\n", "fox"] {
            app.push_event(Event::Text(frag.to_owned()));
        }
        // No blank line yet: nothing flushed, still pending.
        app.flush_pending_text();
        assert!(
            app.lines
                .iter()
                .any(|line| line.line.to_string() == "The quick brown fox"),
            "tokens should concatenate horizontally, not one per line:\n{:?}",
            app.lines
                .iter()
                .map(|l| l.line.to_string())
                .collect::<Vec<_>>()
        );
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
                .any(|line| line.line.to_string().contains("shorten it"))
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

    #[test]
    fn streamed_reasoning_concatenates_into_flowing_lines() {
        // Regression: each reasoning delta used to wrap into its own row, so
        // thinking broke into one short line per fragment.
        let mut app = app();
        let before = app.lines.len();
        for frag in ["The ", "quick ", "brown ", "fox"] {
            app.push_event(Event::Reasoning(frag.to_owned()));
        }
        // Still pending: no blank line to flush a reasoning paragraph yet.
        assert_eq!(app.lines.len(), before);
        // A non-reasoning event flushes the accumulated reasoning, concatenated.
        app.push_event(Event::Finished {
            message: "done".to_owned(),
        });
        let reasoning: Vec<String> = app
            .lines
            .iter()
            .filter(|line| line.kind == LineKind::Reasoning)
            .map(|line| line.line.to_string())
            .collect();
        assert!(
            reasoning.iter().any(|line| line == "The quick brown fox"),
            "reasoning should concatenate horizontally, not one row per delta:\n{:?}",
            app.lines
                .iter()
                .map(|l| l.line.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn auto_follows_new_output_to_the_bottom() {
        // By default the transcript is pinned to the newest line: appending far
        // more rows than fit must keep the view at the bottom.
        let mut app = app();
        assert!(app.following);
        for i in 0..50 {
            app.push_event(Event::Note(format!("line {i}")));
        }
        assert_eq!(app.scroll, app.bottom_offset());
        assert!(app.following);
    }

    #[test]
    fn scrolling_unpins_and_stays_put() {
        let mut app = app();
        for i in 0..50 {
            app.push_event(Event::Note(format!("line {i}")));
        }
        assert_eq!(app.scroll, app.bottom_offset());
        // Scroll up to read older output: auto-follow must unpin.
        app.scroll_up(10);
        assert!(!app.following);
        let pinned = app.scroll;
        // New output arrives; the view must not jump back down.
        app.push_event(Event::Note("newest line".to_owned()));
        assert_eq!(app.scroll, pinned);
        assert!(!app.following);
    }

    #[test]
    fn ctrl_end_refollows_to_the_bottom() {
        let mut app = app();
        for i in 0..50 {
            app.push_event(Event::Note(format!("line {i}")));
        }
        app.scroll_up(15);
        assert!(!app.following);
        let above_bottom = app.scroll;
        assert!(above_bottom < app.bottom_offset());
        // Ctrl+End jumps to the newest output and resumes auto-follow.
        app.on_key(ctrl(KeyCode::End));
        assert_eq!(app.scroll, app.bottom_offset());
        assert!(app.following);
    }

    #[test]
    fn ctrl_s_toggles_scrollbar() {
        let mut app = app();
        assert!(!app.show_scrollbar);
        app.on_key(ctrl(KeyCode::Char('s')));
        assert!(app.show_scrollbar);
        app.on_key(ctrl(KeyCode::Char('s')));
        assert!(!app.show_scrollbar);
    }

    #[test]
    fn copy_strips_frame_and_trailing_whitespace_from_transcript() {
        let mut app = clean_app();
        // `push_wrapped` appends one pre-wrapped row per line, so each argument
        // line becomes a distinct transcript row without depending on markdown
        // (where a single `\n` is a soft break, i.e. a space).
        app.push_wrapped(Style::default(), "alpha");
        app.push_wrapped(Style::default(), "beta");
        // A final row that ends with genuine trailing spaces, to prove copy
        // trims the padding that sits between the content and the box border.
        app.push_wrapped(Style::default(), "trailing spaces   ");
        app.copy_to_clipboard();
        let escape = app
            .take_pending_osc_52()
            .expect("a clipboard escape is staged");
        // The escape is OSC 52: `\x1b]52;c;<base64>\x07`.
        assert!(escape.starts_with("\u{1b}]52;c;"));
        assert!(escape.ends_with('\u{7}'));
        assert!(
            !escape.contains('\u{2502}'),
            "no box frame char: {escape:?}"
        );
        assert!(
            !escape.ends_with('\n'),
            "no trailing newline in escape: {escape:?}"
        );
        // The base64 payload starts at byte 7 (after the ESC byte and `]52;c;`)
        // and runs to the final byte before the ST bell.
        let payload = &escape.as_bytes()[7..escape.len() - 1];
        let decoded = base64_decode(std::str::from_utf8(payload).unwrap());
        assert_eq!(
            decoded, "alpha\nbeta\ntrailing spaces",
            "trailing whitespace is stripped and no trailing line break is kept"
        );
        // A second copy with nothing new staged must not resend a stale escape.
        app.copy_to_clipboard();
        assert!(app.take_pending_osc_52().is_none());
    }

    #[test]
    fn copy_with_empty_transcript_stages_nothing() {
        let mut app = clean_app();
        app.copy_to_clipboard();
        assert!(app.take_pending_osc_52().is_none());
    }

    fn rendered_text(app: &App) -> Vec<String> {
        app.lines.iter().map(|line| line.line.to_string()).collect()
    }

    #[test]
    fn markdown_fence_renders_a_marked_code_block() {
        let mut app = clean_app();
        let before = app.lines.len();
        app.push_event(Event::Text("```rust\nfn main() {}\n```".to_owned()));
        app.flush_pending_text();
        // A fenced block renders two rows: a marked language label, then the source.
        let lines = rendered_text(&app);
        assert_eq!(
            lines.len() - before,
            2,
            "a fence label and one source row: {lines:?}"
        );
        assert!(
            lines[0].contains("rust"),
            "the fence is marked with its language: {lines:?}"
        );
        assert!(
            lines[1].contains("fn main() {}"),
            "the source is shown: {lines:?}"
        );
    }

    #[test]
    fn markdown_fence_streamed_across_fragments_flushes_whole() {
        let mut app = clean_app();
        // The closing fence arrives later; an open fence must stay pending.
        app.push_event(Event::Text("```sh\necho hi".to_owned()));
        assert_eq!(
            app.lines.len(),
            0,
            "an unclosed fence renders nothing: {:?}",
            rendered_text(&app)
        );
        app.push_event(Event::Text("\n```".to_owned()));
        app.flush_pending_text();
        let lines = rendered_text(&app);
        assert!(
            lines.iter().any(|line| line.contains("echo hi")),
            "the block renders whole once the fence closes: {lines:?}"
        );
    }

    #[test]
    fn markdown_heading_and_bold_render_as_formatted_rows() {
        let mut app = clean_app();
        app.push_event(Event::Text("# Title\n\n**bold** text".to_owned()));
        app.flush_pending_text();
        let lines = rendered_text(&app);
        assert_eq!(lines[0], "Title", "the heading text: {lines:?}");
        let bold_row = app
            .lines
            .iter()
            .find(|line| line.line.to_string().contains("bold"))
            .expect("the bold paragraph rendered");
        assert!(
            bold_row.line.spans.len() > 1,
            "bold text is a distinct styled span, not plain text: {:?}",
            bold_row.line.spans
        );
    }

    #[test]
    fn markdown_table_renders_header_separator_and_row() {
        let mut app = clean_app();
        app.push_event(Event::Text(
            "| a | b |\n| --- | --- |\n| 1 | 2 |\n".to_owned(),
        ));
        app.flush_pending_text();
        let lines = rendered_text(&app);
        assert_eq!(lines.len(), 3, "a GFM table is three rows: {lines:?}");
        assert!(
            lines[1].chars().all(|c| matches!(c, '-' | ':' | ' ')),
            "a separator row: {lines:?}"
        );
        assert!(lines[2].contains("1") && lines[2].contains("2"));
    }

    /// Groups are decoded four characters at a time into bytes. A full group of
    /// four chars yields three bytes; a trailing group of three (`XXX=`) yields
    /// two, and of two (`XX==`) yields one. Sextets are zero-padded to four so
    /// the 24-bit layout is fixed and the real bytes occupy the top bits in
    /// order; the trailing padding sextets decode to zeros that the match discards.
    fn base64_decode(input: &str) -> String {
        const TABLE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let vals: Vec<u32> = input
            .bytes()
            .filter(|&b| b != b'=' && b != b'\n' && b != b'\r')
            .map(|b| {
                TABLE
                    .find(b as char)
                    .map(|i| i as u32)
                    .expect("non-base64 character in escape")
            })
            .collect();
        let mut out: Vec<u8> = Vec::new();
        for group in vals.chunks(4) {
            // Zero-pad to four sextets so the 24-bit layout is fixed. The padding
            // sextets decode to zero bytes that the match discards, so a 2-byte tail
            // yields 1 output byte and a 3-byte tail yields 2.
            let mut sextets = [0u32; 4];
            for (slot, value) in sextets.iter_mut().zip(group.iter()) {
                *slot = *value;
            }
            let n: u32 = (sextets[0] << 18) | (sextets[1] << 12) | (sextets[2] << 6) | sextets[3];
            match group.len() {
                4 => {
                    out.push((n >> 16) as u8);
                    out.push((n >> 8) as u8);
                    out.push(n as u8);
                }
                3 => {
                    out.push((n >> 16) as u8);
                    out.push((n >> 8) as u8);
                }
                2 => out.push((n >> 16) as u8),
                _ => {}
            }
        }
        String::from_utf8(out).expect("base64 payload is utf-8")
    }

    #[test]
    fn ctrl_q_quits_even_when_a_turn_is_running() {
        // Ctrl-Q is the reliable exit: it quits whether or not a turn runs,
        // unlike Ctrl+C which needs a second press once idle.
        let mut running = app();
        running.running = true;
        running.status = "thinking".to_owned();
        assert_eq!(running.on_key(ctrl(KeyCode::Char('q'))), KeyAction::Quit);
        assert!(running.should_quit);
    }

    #[test]
    fn ctrl_q_quits_when_idle() {
        let mut app = app();
        assert_eq!(app.on_key(ctrl(KeyCode::Char('q'))), KeyAction::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn esc_when_idle_opens_the_action_menu() {
        let mut app = app();
        assert_eq!(app.on_key(ch(KeyCode::Esc)), KeyAction::Idle);
        assert_eq!(app.overlay, Overlay::Menu);
        assert_eq!(app.menu_selection, 0);
    }

    #[test]
    fn esc_when_help_is_open_closes_help_and_skips_the_menu() {
        // Help keeps priority: Esc closes it and returns to the prompt, rather
        // than dropping straight into the menu.
        let mut app = app();
        app.show_help = true;
        assert_eq!(app.on_key(ch(KeyCode::Esc)), KeyAction::Idle);
        assert!(!app.show_help);
        assert_eq!(app.overlay, Overlay::None);
    }

    #[test]
    fn menu_esc_and_r_return_to_the_prompt() {
        let mut app = app();
        app.open_menu();
        assert_eq!(app.on_key(ch(KeyCode::Esc)), KeyAction::Idle);
        assert_eq!(app.overlay, Overlay::None);
        app.open_menu();
        assert_eq!(app.on_key(ch(KeyCode::Char('r'))), KeyAction::Idle);
        assert_eq!(app.overlay, Overlay::None);
    }

    #[test]
    fn menu_navigation_clamps_at_both_ends() {
        let mut app = app();
        app.open_menu();
        for _ in 0..8 {
            app.on_key(ch(KeyCode::Down));
        }
        assert_eq!(app.menu_selection, MENU_ITEMS.len() - 1);
        for _ in 0..8 {
            app.on_key(ch(KeyCode::Up));
        }
        assert_eq!(app.menu_selection, 0);
    }

    #[test]
    fn menu_n_hotkey_starts_a_new_session() {
        let mut app = app();
        app.note(Style::default(), "earlier output that should vanish");
        assert!(!app.lines.is_empty());
        app.open_menu();
        app.on_key(ch(KeyCode::Char('n')));
        assert!(
            !app.lines
                .iter()
                .any(|l| l.line.to_string().contains("earlier output")),
            "old transcript is cleared"
        );
        assert_eq!(app.status, "idle");
    }

    #[test]
    fn menu_enter_on_quit_selection_quits() {
        let mut app = app();
        app.open_menu();
        // Move to the Quit action (index 2) and activate with Enter.
        for _ in 0..2 {
            app.on_key(ch(KeyCode::Down));
        }
        assert_eq!(app.menu_selection, 2);
        app.on_key(ch(KeyCode::Enter));
        assert!(app.should_quit);
    }

    #[test]
    fn menu_history_hotkey_opens_the_history_overlay() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("session-1.log"),
            "== session refac finished: 2 turns, 20 tokens, 1s, ok ==\n",
        )
        .unwrap();
        let mut app = app();
        app.set_log_dir(dir.path().to_path_buf());
        app.open_menu();
        app.on_key(ch(KeyCode::Char('h')));
        assert_eq!(app.overlay, Overlay::History);
        assert_eq!(app.history_items.len(), 1);
    }

    #[test]
    fn history_esc_returns_to_the_menu() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("session-1.log"),
            "== session s finished: cancelled/failed ==\n",
        )
        .unwrap();
        let mut app = app();
        app.set_log_dir(dir.path().to_path_buf());
        app.open_history();
        assert_eq!(app.overlay, Overlay::History);
        assert_eq!(app.on_key(ch(KeyCode::Esc)), KeyAction::Idle);
        assert_eq!(app.overlay, Overlay::Menu);
    }

    #[test]
    fn history_enter_replays_the_selected_transcript() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("session-1.log"),
            "== session demo started 0s ago ==\nyou: refactor the parser\n\u{2192} tool: shell\n\u{2713} tool shell finished\n",
        )
        .unwrap();
        let mut app = app();
        app.set_log_dir(dir.path().to_path_buf());
        app.open_history();
        app.on_key(ch(KeyCode::Enter));
        assert_eq!(app.overlay, Overlay::Replay);
        assert_eq!(app.replay_title, "demo");
        assert!(
            app.replay_lines
                .iter()
                .any(|l| l.contains("refactor the parser")),
            "replay shows the transcript content:\n{:?}",
            app.replay_lines
        );
    }

    #[test]
    fn replay_esc_returns_to_the_history_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("session-1.log"),
            "== session demo started 0s ago ==\nhello there\n",
        )
        .unwrap();
        let mut app = app();
        app.set_log_dir(dir.path().to_path_buf());
        app.open_history();
        app.on_key(ch(KeyCode::Enter));
        assert_eq!(app.overlay, Overlay::Replay);
        assert_eq!(app.on_key(ch(KeyCode::Esc)), KeyAction::Idle);
        assert_eq!(app.overlay, Overlay::History);
    }

    #[test]
    fn open_history_notes_when_no_sessions_exist() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.set_log_dir(dir.path().to_path_buf());
        app.open_history();
        assert_eq!(app.overlay, Overlay::History);
        assert!(app.history_items.is_empty());
        assert!(
            app.lines
                .iter()
                .any(|line| line.line.to_string().contains("no past sessions found")),
            "a helpful note appears when history is empty"
        );
    }
}
