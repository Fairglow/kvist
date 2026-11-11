//! Interactive workspace state and event loop.

mod actions;
mod bindings;
#[cfg(test)]
mod menu_tests;
mod menus;
#[cfg(test)]
mod model_menu_tests;
mod model_menus;
mod preferences;
mod view;

use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, text::Text};

use super::{
    editor::Input,
    process::{Output, Running, Stream},
    render,
    storage::Transcript,
};
use crate::bark::{
    DynamicState, StatusContext,
    completion::{Candidate, KvistCompleter},
    prefill::ComponentPrefill,
    tree::{CommandNode, build_root},
};
use crate::skott_embedded::{ContinuationDecision, EmbeddedSkott, SkottMetrics, SkottResult};

/// The maximum automatic continuations Kvist submits for one agent session
/// before it stops forcing progress and surfaces the result for a decision.
const MAX_SKOTT_CONTINUATIONS: u32 = 5;
use crate::{KvistError, Result};

enum Overlay {
    Help {
        scroll: usize,
    },
    Menu(menus::Menu),
    Confirm {
        prompt: String,
        arguments: Vec<String>,
        scroll: usize,
    },
    Details {
        title: String,
        markdown: String,
        scroll: usize,
    },
    Quit,
}

pub(super) struct Workspace {
    project: PathBuf,
    focus: Arc<Mutex<Option<String>>>,
    tree: CommandNode,
    state: DynamicState,
    prefill: ComponentPrefill,
    input: Input,
    candidates: Vec<Candidate>,
    candidate: usize,
    transcript: Transcript,
    segment: u64,
    source: String,
    rendered: Text<'static>,
    scroll: usize,
    horizontal: u16,
    viewport: usize,
    follow: bool,
    mask_answers: bool,
    dirty: bool,
    width: u16,
    theme: crate::bark::Theme,
    history: Vec<String>,
    history_index: Option<usize>,
    history_draft: String,
    overlay: Option<Overlay>,
    quit_return: Option<Overlay>,
    menu_stack: Vec<menus::Menu>,
    pending_page: Option<menus::Page>,
    preferences: preferences::Preferences,
    running: Option<Running>,
    streams: [Stream; 2],
    redactions: Vec<String>,
    status: StatusContext,
    state_label: String,
    progress: String,
    notice: String,
    last_failed: bool,
    capture_failed: bool,
    quit: bool,
    prompt_file: Option<tempfile::NamedTempFile>,
    skott: Option<EmbeddedSkott>,
    skott_continuations: u32,
    skott_result_reported: bool,
}

pub(crate) fn available() -> bool {
    io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var_os("CLICOLOR").is_none_or(|value| value != "0")
        && std::env::var_os("TERM").is_some_and(|value| value != "dumb")
        && crossterm::terminal::size().is_ok_and(|(width, height)| width >= 40 && height >= 10)
}

type Screen = Terminal<CrosstermBackend<io::Stdout>>;

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        ) {
            let _ = disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                LeaveAlternateScreen,
                DisableMouseCapture,
                DisableBracketedPaste
            );
            return Err(error);
        }
        Ok(Self)
    }

    fn restore() -> io::Result<()> {
        let raw = disable_raw_mode();
        let screen = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        raw.and(screen)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = Self::restore();
    }
}

pub(crate) fn run(project: &Path) -> Result<()> {
    sav::install_handler();
    let result = (|| {
        let (diagnostics, _logging) = crate::logging::WorkspaceDiagnostics::enter();
        let mut workspace = Workspace::new(project)?;
        let _guard = TerminalGuard::enter()?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        let mut last_render = Instant::now() - Duration::from_secs(1);
        loop {
            let diagnostic = diagnostics.drain();
            if !diagnostic.is_empty() {
                workspace.append(&format!(
                    "\n\n### Host diagnostics\n\n{}\n\n",
                    crate::presentation::fence(&diagnostic, "text")
                ))?;
            }
            workspace.pump()?;
            if sav::take_interrupted() {
                if let Some(run) = &mut workspace.running {
                    run.cancel()?;
                } else {
                    workspace.input.set("");
                    workspace.candidates.clear();
                }
            }
            let size = terminal.size()?;
            let width = size.width.saturating_sub(3).max(1);
            if workspace.width != width {
                workspace.width = width;
                workspace.dirty = true;
            }
            if workspace.dirty && last_render.elapsed() >= Duration::from_millis(100) {
                workspace.rendered = render::markdown(&workspace.source, width, workspace.theme)?;
                workspace.dirty = false;
                last_render = Instant::now();
            }
            let mut drawing = Ok(());
            terminal.draw(|frame| drawing = workspace.draw(frame))?;
            drawing?;
            if workspace.quit && workspace.running.is_none() {
                break;
            }
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        if let Err(error) = workspace.key(key, &mut terminal) {
                            workspace.record_error(&error.to_string())?;
                        }
                    }
                    Event::Paste(text) => workspace.paste(&text),
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                            if workspace.overlay.is_some() =>
                        {
                            let key = if mouse.kind == MouseEventKind::ScrollUp {
                                KeyCode::Up
                            } else {
                                KeyCode::Down
                            };
                            for _ in 0..3 {
                                workspace.overlay_key(key.into())?;
                            }
                        }
                        MouseEventKind::ScrollUp => workspace.navigate(-3)?,
                        MouseEventKind::ScrollDown => workspace.navigate(3)?,
                        MouseEventKind::Down(event::MouseButton::Left)
                        | MouseEventKind::Drag(event::MouseButton::Left)
                            if workspace.overlay.is_none()
                                && mouse.column == size.width.saturating_sub(1) =>
                        {
                            let height = size.height.saturating_sub(7).max(1);
                            let row = mouse.row.saturating_sub(3).min(height);
                            workspace.seek_history(row, height)?;
                        }
                        _ => {}
                    },
                    Event::Resize(_, _) => workspace.dirty = true,
                    _ => {}
                }
            }
        }
        terminal.show_cursor()?;
        TerminalGuard::restore()
    })();
    result.map_err(|source| KvistError::Io {
        operation: "run Markdown workspace (terminal restored; task effects are not rolled back)",
        path: project.to_path_buf(),
        source,
    })
}

impl Workspace {
    fn new(project: &Path) -> io::Result<Self> {
        let project = project.canonicalize()?;
        let transcript = Transcript::open(&project)?;
        let segment = transcript.latest();
        let source = transcript.read(segment)?;
        let history = transcript.history()?;
        let preferences = preferences::Preferences::load(&project)?;
        let tree = build_root();
        let focus = Arc::new(Mutex::new(None));
        let prefill = ComponentPrefill::new(tree.clone(), focus.clone());
        let mut workspace = Self {
            state: DynamicState::load(&project),
            theme: crate::bark::Theme::resolve(&project),
            status: StatusContext::load(&project),
            project,
            focus,
            tree,
            prefill,
            input: Input::default(),
            candidates: Vec::new(),
            candidate: 0,
            transcript,
            segment,
            source,
            rendered: Text::default(),
            scroll: 0,
            horizontal: 0,
            viewport: 1,
            follow: true,
            mask_answers: false,
            dirty: true,
            width: 80,
            history,
            history_index: None,
            history_draft: String::new(),
            overlay: None,
            quit_return: None,
            menu_stack: Vec::new(),
            pending_page: None,
            preferences,
            running: None,
            streams: Default::default(),
            redactions: Vec::new(),
            state_label: String::new(),
            progress: String::new(),
            notice: String::new(),
            last_failed: false,
            capture_failed: false,
            quit: false,
            prompt_file: None,
            skott: None,
            skott_continuations: 0,
            skott_result_reported: false,
        };
        workspace.refresh();
        workspace.append(&format!(
            "\n\n# Kvist workspace\n\nCommands are Markdown. {}.\n\n",
            bindings::footer()
        ))?;
        Ok(workspace)
    }

    fn focus(&self) -> Option<String> {
        self.focus
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn refresh(&mut self) {
        match crate::config::load(&self.project) {
            Ok(config) => {
                self.redactions = config
                    .agent
                    .roles
                    .values()
                    .chain([
                        &config.agent.developer,
                        &config.agent.architect,
                        &config.agent.security_reviewer,
                    ])
                    .flat_map(|role| role.redaction_values.clone())
                    .collect();
                if let Some(sandbox) = &config.sandbox {
                    self.redactions
                        .extend(crate::sandbox::allowed_environment(sandbox, None).into_values());
                }
                self.redactions.sort();
                self.redactions.dedup();
            }
            Err(error) => {
                self.notice = format!("Configuration unavailable: {error}");
            }
        }
        self.state = DynamicState::load(&self.project);
        self.status = StatusContext::load(&self.project);
        self.state_label = match crate::project_state::inspect(&self.project) {
            Ok(inspection) => {
                let focus = self.focus();
                if let Some(focus) = focus {
                    inspection
                        .components
                        .iter()
                        .find(|component| {
                            component.path == Path::new(&focus)
                                || focus == "." && component.path.as_os_str().is_empty()
                        })
                        .map(|component| component.state.name().to_owned())
                        .unwrap_or_else(|| "unavailable".into())
                } else {
                    inspection.state.name().to_owned()
                }
            }
            Err(error) => format!("unavailable: {error}"),
        };
        let focus = self.focus();
        let tasks: Vec<_> = self
            .state
            .scopes
            .iter()
            .filter(|(component, _)| focus.as_ref().is_none_or(|focus| focus == *component))
            .flat_map(|(_, scope)| &scope.tasks)
            .collect();
        let completed = tasks
            .iter()
            .filter(|task| task.status == crate::task_queue::TaskStatus::Completed)
            .count();
        self.progress = format!("{completed}/{} tasks", tasks.len());
    }

    fn append(&mut self, text: &str) -> io::Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.transcript.append(&crate::presentation::sanitize(
            &crate::presentation::redact(text, &self.redactions),
        ))?;
        if self.follow {
            self.segment = self.transcript.latest();
        }
        self.source = self.transcript.read(self.segment)?;
        self.dirty = true;
        Ok(())
    }

    fn record_error(&mut self, error: &str) -> io::Result<()> {
        self.last_failed = true;
        self.notice =
            crate::presentation::sanitize(&crate::presentation::redact(error, &self.redactions));
        self.append(&format!(
            "\n\n## Error\n\n{}\n\n",
            crate::presentation::report(error)
        ))
    }

    fn load_segment(&mut self, segment: u64, bottom: bool) -> io::Result<()> {
        let source = self.transcript.read(segment)?;
        self.segment = segment;
        self.source = source;
        self.rendered = render::markdown(&self.source, self.width, self.theme)?;
        self.scroll = if bottom {
            self.rendered.lines.len().saturating_sub(self.viewport)
        } else {
            0
        };
        self.dirty = false;
        Ok(())
    }

    fn navigate(&mut self, delta: isize) -> io::Result<()> {
        self.follow = false;
        if delta < 0 {
            let amount = delta.unsigned_abs();
            if self.scroll == 0 && self.segment > 0 {
                self.load_segment(self.segment - 1, true)?;
            } else {
                self.scroll = self.scroll.saturating_sub(amount);
            }
        } else {
            let bottom = self.rendered.lines.len().saturating_sub(self.viewport);
            if self.scroll >= bottom && self.segment < self.transcript.latest() {
                self.load_segment(self.segment + 1, false)?;
            } else {
                self.scroll = self.scroll.saturating_add(delta as usize).min(bottom);
                self.follow = self.scroll == bottom && self.segment == self.transcript.latest();
            }
        }
        Ok(())
    }

    fn seek_history(&mut self, row: u16, height: u16) -> io::Result<()> {
        self.follow = false;
        let fraction = f64::from(row) / f64::from(height.max(1));
        let position = fraction * (self.transcript.latest() as f64 + 1.0);
        let segment = (position.floor() as u64).min(self.transcript.latest());
        self.load_segment(segment, false)?;
        let local = if fraction >= 1.0 {
            1.0
        } else {
            position.fract()
        };
        self.scroll =
            (local * self.rendered.lines.len().saturating_sub(self.viewport) as f64) as usize;
        Ok(())
    }

    fn capture_error(&mut self, error: &str) -> io::Result<()> {
        self.capture_failed = true;
        if let Some(run) = &mut self.running {
            run.cancel()?;
        }
        self.record_error(&format!(
            "Command capture failed; cancellation requested: {error}"
        ))
    }

    fn pump(&mut self) -> io::Result<()> {
        // Process embedded Skott events; a new event marks the view dirty.
        if let Some(skott) = &mut self.skott {
            if skott.pump() {
                self.dirty = true;
            }
            // Kvist owns the completion policy. When the agent returns control
            // without finishing and without a legitimate block, resubmit a
            // focused continuation prompt (bounded, so a stuck agent surfaces
            // for a human decision instead of looping forever). A terminal
            // result is reported to the Kvist view exactly once.
            if skott.result().finished && !self.skott_result_reported {
                match skott.result().decision() {
                    Some(ContinuationDecision::Continue)
                        if self.skott_continuations < MAX_SKOTT_CONTINUATIONS =>
                    {
                        self.skott_continuations = self.skott_continuations.saturating_add(1);
                        let attempt = self.skott_continuations;
                        let turns = skott.metrics().turns;
                        match skott.submit(format!(
                            "Continue the task. You returned control after {turns} turn(s) \
                             without finishing and without a blocking reason. Review what \
                             remains and keep working until the task is complete, you are \
                             blocked for a concrete reason, or a human decision is required. \
                             (automatic continuation {attempt}/{MAX_SKOTT_CONTINUATIONS})"
                        )) {
                            Ok(()) => {
                                self.dirty = true;
                                self.notice = format!(
                                    "Agent stopped without finishing; Kvist continued the task (attempt {attempt})."
                                );
                            }
                            Err(error) => {
                                self.skott_result_reported = true;
                                self.record_error(&format!("Skott continuation failed: {error}"))?;
                            }
                        }
                    }
                    _ => {
                        // Finished, blocked, or out of continuations: surface it.
                        self.skott_result_reported = true;
                        let result = skott.result().clone();
                        let metrics = skott.metrics().clone();
                        self.report_skott_result(&result, &metrics)?;
                    }
                }
            }
        }

        let Some(run) = &mut self.running else {
            return Ok(());
        };
        let events = run.drain();
        let status = run.poll()?;
        let mut batch = String::new();
        let mut capture_error = None;
        for event in events {
            if self.capture_failed || capture_error.is_some() {
                continue;
            }
            match event {
                Output::Text { stderr, text } => {
                    let index = usize::from(stderr);
                    match self.streams[index].push(&text) {
                        Ok(markdown) => batch.push_str(&markdown),
                        Err(error) => capture_error = Some(error.to_string()),
                    }
                }
                Output::Error(error) => capture_error = Some(error.to_string()),
                Output::Eof => {}
            }
        }
        if !self.capture_failed && capture_error.is_none() {
            for index in 0..2 {
                batch.push_str(&self.streams[index].flush(status.is_some()));
            }
        }
        if !batch.is_empty()
            && let Err(error) = self.append(&batch)
        {
            if error.kind() != io::ErrorKind::FileTooLarge {
                return Err(error);
            }
            self.transcript.new_document()?;
            capture_error = Some(error.to_string());
        }
        if let Some(error) = capture_error {
            self.capture_error(&error)?;
        }
        if let Some(status) = status {
            self.last_failed = self.capture_failed || !status.success();
            self.append(&format!(
                "\n\n**Command {}** ({status}).\n\n",
                if !self.last_failed {
                    "finished"
                } else {
                    "failed"
                }
            ))?;
            self.running.take();
            self.prompt_file.take();
            self.refresh();
            if !self.capture_failed {
                self.notice.clear();
            }
            if !self.quit
                && let Some(page) = self.pending_page.take()
                && self.overlay.is_none()
            {
                match self.build_menu(page) {
                    Ok(menu) => self.overlay = Some(Overlay::Menu(menu)),
                    Err(error) => self.record_error(&format!("refresh settings: {error}"))?,
                }
            }
        }
        Ok(())
    }

    /// Appends a terminal Skott result to the Kvist transcript so the Kvist
    /// view shows the answer (or failure) and the session metrics.
    fn report_skott_result(
        &mut self,
        result: &SkottResult,
        metrics: &SkottMetrics,
    ) -> io::Result<()> {
        let heading = match result.decision() {
            Some(ContinuationDecision::Finished) => "Agent result",
            Some(ContinuationDecision::Blocked) => "Agent blocked",
            Some(ContinuationDecision::Continue) => "Agent paused",
            None => "Agent",
        };
        let mut markdown = format!("\n\n## {heading}\n\n");
        if let Some(answer) = &result.answer {
            markdown.push_str(answer);
            markdown.push_str("\n\n");
        }
        if let Some(failure) = &result.failure {
            markdown.push_str(&format!("**Failure:** {failure}\n\n"));
        }
        markdown.push_str(&format!("**Metrics:** {}\n\n", metrics.summary_line()));
        self.append(&markdown)
    }

    fn key(&mut self, key: KeyEvent, terminal: &mut Screen) -> io::Result<()> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('q') {
            self.request_quit();
            return Ok(());
        }
        if control && key.code == KeyCode::Char('c') {
            if let Some(run) = &mut self.running {
                run.cancel()?;
                self.notice =
                    "Cancellation requested; observed effects remain for review/recovery".into();
            } else {
                self.input.set("");
                self.candidates.clear();
                self.overlay = None;
                self.menu_stack.clear();
                self.prefill.begin_submission();
            }
            return Ok(());
        }
        if control && key.code == KeyCode::Char('d') && self.input.text.is_empty() {
            if let Some(run) = &mut self.running {
                run.close_input();
                self.notice = "Command stdin closed (EOF); Ctrl+C cancels, Ctrl+Q exits".into();
            } else {
                self.quit = true;
            }
            return Ok(());
        }
        if key.code == KeyCode::Esc {
            return self.escape();
        }
        if self.overlay.is_some() {
            return self.overlay_key(key);
        }

        // Skott pane switching: Ctrl+PageUp/PageDown or Alt+Left/Right
        if let Some(skott) = &mut self.skott
            && skott.handle_key(key)
        {
            self.dirty = true;
            return Ok(());
        }
        if self.running.is_some()
            && matches!(
                key.code,
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down
            )
        {
            return Ok(());
        }
        match key.code {
            KeyCode::F(1) => {
                self.candidates.clear();
                self.overlay = Some(Overlay::Help { scroll: 0 });
            }
            KeyCode::F(2) => self.open_menu(menus::Page::Root)?,
            KeyCode::PageUp => self.navigate(-(self.viewport.max(1) as isize))?,
            KeyCode::PageDown => self.navigate(self.viewport.max(1) as isize)?,
            KeyCode::Home if control => {
                self.follow = false;
                self.load_segment(0, false)?;
            }
            KeyCode::End if control => {
                self.follow = true;
                self.load_segment(self.transcript.latest(), true)?;
            }
            KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => {
                self.horizontal = self.horizontal.saturating_sub(8)
            }
            KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => {
                self.horizontal = self.horizontal.saturating_add(8)
            }
            KeyCode::Tab | KeyCode::BackTab => {
                if self.candidates.is_empty() {
                    self.candidates = KvistCompleter::resolve(
                        &self.tree,
                        &self.state,
                        self.focus().as_deref(),
                        &self.input.text,
                        self.input.cursor,
                    );
                    self.candidate = 0;
                    if self.candidates.len() == 1 {
                        self.accept_candidate();
                    }
                } else if key.code == KeyCode::BackTab {
                    self.candidate =
                        (self.candidate + self.candidates.len() - 1) % self.candidates.len();
                } else {
                    self.candidate = (self.candidate + 1) % self.candidates.len();
                }
            }
            KeyCode::Enter if !self.candidates.is_empty() => self.accept_candidate(),
            KeyCode::Up if !self.candidates.is_empty() => {
                self.candidate =
                    (self.candidate + self.candidates.len() - 1) % self.candidates.len()
            }
            KeyCode::Down if !self.candidates.is_empty() => {
                self.candidate = (self.candidate + 1) % self.candidates.len()
            }
            KeyCode::Up => self.recall(true),
            KeyCode::Down => self.recall(false),
            KeyCode::Enter => self.submit(terminal)?,
            _ => {
                self.candidates.clear();
                self.input.key(key);
                if self.running.is_none() {
                    self.input.prefill(&self.prefill);
                }
            }
        }
        Ok(())
    }

    fn escape(&mut self) -> io::Result<()> {
        if self.overlay.is_some() {
            self.overlay_key(KeyCode::Esc.into())
        } else if self.candidates.is_empty() {
            self.open_menu(menus::Page::Root)
        } else {
            self.candidates.clear();
            Ok(())
        }
    }

    fn paste(&mut self, text: &str) {
        if let Some(Overlay::Menu(menu)) = &mut self.overlay {
            if menu.page.searchable() && !menu.filter_insert(text) {
                self.notice = "Search exceeds 256 UTF-8 bytes".into();
            }
            return;
        }
        if self.overlay.is_some() {
            return;
        }
        if !self.input.insert(text) {
            self.notice = "Input exceeds 8192 UTF-8 bytes".into();
        }
        self.candidates.clear();
        if self.running.is_none() {
            self.input.prefill(&self.prefill);
        }
    }

    fn accept_candidate(&mut self) {
        if let Some(candidate) = self.candidates.get(self.candidate) {
            self.input.accept(candidate);
        }
        self.candidates.clear();
        self.input.prefill(&self.prefill);
    }

    fn recall(&mut self, previous: bool) {
        if self.history.is_empty() {
            return;
        }
        let index = match (self.history_index, previous) {
            (None, true) => {
                self.history_draft = self.input.text.clone();
                Some(self.history.len() - 1)
            }
            (Some(index), true) => Some(index.saturating_sub(1)),
            (Some(index), false) if index + 1 < self.history.len() => Some(index + 1),
            _ => None,
        };
        self.history_index = index;
        self.input.set(
            index
                .map(|i| self.history[i].as_str())
                .unwrap_or(&self.history_draft),
        );
        if self.running.is_none() {
            self.input.prefill(&self.prefill);
        }
    }
}
