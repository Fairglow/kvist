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
mod skott_view;
mod task_progress;
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
use crate::config::CommitFrequency;
use crate::skott_embedded::{ActivePane, EmbeddedSkott};
use crate::{KvistError, Result};
use skott_view::{TaskView, idle_app};
use task_progress::RunProgress;

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
    task_progress: Option<RunProgress>,
    commit_frequency: Option<CommitFrequency>,
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
    free_skott: skott::tui::app::App,
    task_skott: TaskView,
    pane: ActivePane,
    split_skott_focus: bool,
    skott_area: ratatui::layout::Rect,
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
        // Seed the transcript wrap width from the real terminal size so the
        // first frame already wraps at the right width; the frame sync in the
        // loop below keeps it tracking resizes and split-view panes after.
        let size = terminal.size()?;
        workspace.width = size.width.saturating_sub(2).max(1);
        let mut last_render = Instant::now() - Duration::from_secs(1);
        let mut last_error = String::new();
        loop {
            let quit = (|| -> io::Result<bool> {
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
                if workspace.dirty && last_render.elapsed() >= Duration::from_millis(100) {
                    workspace.rendered = workspace.render_transcript()?;
                    workspace.dirty = false;
                    last_render = Instant::now();
                }
                let mut drawing = Ok(());
                terminal.draw(|frame| drawing = workspace.draw(frame))?;
                drawing?;
                if workspace.quit && workspace.running.is_none() {
                    return Ok(true);
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
                            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                                let in_skott = workspace
                                    .skott_area
                                    .contains((mouse.column, mouse.row).into());
                                workspace.mouse_scroll(
                                    mouse.kind == MouseEventKind::ScrollUp,
                                    in_skott,
                                )?;
                            }
                            MouseEventKind::Down(event::MouseButton::Left)
                                if workspace.overlay.is_none()
                                    && workspace.pane == ActivePane::Both =>
                            {
                                workspace.split_skott_focus = workspace
                                    .skott_area
                                    .contains((mouse.column, mouse.row).into());
                            }
                            MouseEventKind::Down(event::MouseButton::Left)
                            | MouseEventKind::Drag(event::MouseButton::Left)
                                if workspace.overlay.is_none()
                                    && workspace.pane != ActivePane::Skott
                                    && !workspace
                                        .skott_area
                                        .contains((mouse.column, mouse.row).into())
                                    && mouse.column == size.width.saturating_sub(1) =>
                            {
                                let height = size.height.saturating_sub(7).max(1);
                                let row = mouse.row.saturating_sub(3).min(height);
                                workspace.seek_history(row, height)?;
                            }
                            _ => {}
                        },
                        // The frame carries the authoritative size (it can
                        // change between loop iterations); re-derive the
                        // transcript width from it and re-wrap when it moved.
                        Event::Resize(width, _) => {
                            let transcript_width = width.saturating_sub(2).max(1);
                            if workspace.width != transcript_width {
                                workspace.width = transcript_width;
                                workspace.dirty = true;
                            }
                        }
                        _ => {}
                    }
                }
                Ok(false)
            })();
            match quit {
                Ok(true) => break,
                Ok(false) => last_error.clear(),
                Err(error) => {
                    // Kvist must stay running through in-loop failures: record the
                    // error so it is visible in the notice bar and transcript, then
                    // continue. Only a dead terminal forces a clean exit, never an
                    // unhandled crash. A persistent error with a live terminal keeps
                    // the shell alive and surfaces the error so the user can decide.
                    if error.to_string() != last_error {
                        if let Err(record_error) = workspace.record_error(&error.to_string()) {
                            eprintln!("kvist: could not record workspace error: {record_error}");
                        }
                        last_error = error.to_string();
                    }
                    let terminal_gone = matches!(
                        error.kind(),
                        io::ErrorKind::BrokenPipe
                            | io::ErrorKind::NotConnected
                            | io::ErrorKind::UnexpectedEof
                            | io::ErrorKind::WriteZero
                    );
                    if terminal_gone {
                        break;
                    }
                    // Throttle the retry so a persistent failure does not spin the
                    // loop at full CPU. The shell stays alive and the error stays
                    // visible so the user can decide how to proceed.
                    std::thread::sleep(Duration::from_millis(100));
                    // If the user pressed Ctrl+C during the retry, cancel the
                    // stuck Skott session so the
                    // loop can recover and reach the normal event handling.
                    if sav::take_interrupted() {
                        if let Some(skott) = &mut workspace.skott {
                            skott.cancel();
                        }
                        workspace.skott = None;
                        workspace.dirty = true;
                        workspace.notice =
                            "Interrupted; stuck session cleared. Try again or inspect the task."
                                .into();
                    }
                }
            }
        }
        // Best-effort terminal restore: a broken terminal must not turn a clean
        // exit into an unhandled crash.
        let _ = terminal.show_cursor();
        if let Err(error) = TerminalGuard::restore() {
            eprintln!("kvist: could not fully restore the terminal: {error}");
        }
        Ok(())
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
            task_progress: None,
            commit_frequency: None,
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
            free_skott: idle_app(),
            task_skott: TaskView::default(),
            pane: ActivePane::Kvist,
            split_skott_focus: false,
            skott_area: ratatui::layout::Rect::ZERO,
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
                self.commit_frequency = Some(config.task_run.commit);
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
                self.commit_frequency = None;
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
        // The notice bar always carries the error. A transcript append failure
        // is reported to stderr but never propagated, so recording an error can
        // never itself force Kvist to exit: the failure is always visible in the
        // notice bar even when the transcript cannot be written.
        if let Err(append_error) = self.append(&format!(
            "\n\n## Error\n\n{}\n\n",
            crate::presentation::report(error)
        )) {
            eprintln!("kvist: could not append error to transcript: {append_error}");
        }
        Ok(())
    }

    fn load_segment(&mut self, segment: u64, bottom: bool) -> io::Result<()> {
        let source = self.transcript.read(segment)?;
        self.segment = segment;
        self.source = source;
        self.rendered = self.render_transcript()?;
        self.scroll = if bottom {
            self.rendered.lines.len().saturating_sub(self.viewport)
        } else {
            0
        };
        self.dirty = false;
        Ok(())
    }

    /// Re-renders the Kvist transcript at its current width. The width is
    /// read back from the last frame (which tracks the terminal), so a
    /// resize or a split-view pane change rewraps the markdown.
    fn render_transcript(&mut self) -> io::Result<Text<'static>> {
        render::markdown(&self.source, self.width, self.theme)
    }

    fn skott_focused(&self) -> bool {
        self.pane == ActivePane::Skott || self.pane == ActivePane::Both && self.split_skott_focus
    }

    fn select_pane(&mut self, pane: ActivePane) {
        self.pane = pane;
        self.candidates.clear();
        self.dirty = true;
        if pane != ActivePane::Kvist {
            self.prepare_skott_view();
        }
    }

    /// Applies one mouse-wheel tick to the overlay or the pane under the pointer.
    fn mouse_scroll(&mut self, up: bool, skott: bool) -> io::Result<()> {
        if self.overlay.is_some() {
            let key = if up { KeyCode::Up } else { KeyCode::Down };
            for _ in 0..3 {
                self.overlay_key(key.into())?;
            }
            return Ok(());
        }
        if skott {
            let app = self.skott_app();
            if up {
                app.scroll_up(3);
            } else {
                app.scroll_down(3);
            }
            return Ok(());
        }
        self.navigate(if up { -3 } else { 3 })?;
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
        if let Some(skott) = &mut self.skott {
            skott.pump();
        }
        let more_feed = self.pump_task_view()?;

        let Some(run) = &mut self.running else {
            return Ok(());
        };
        if self
            .task_progress
            .as_mut()
            .is_some_and(RunProgress::refresh)
        {
            self.dirty = true;
        }
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
            for item in &mut self.streams {
                batch.push_str(&item.flush(status.is_some()));
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
            if more_feed || self.pump_task_view()? {
                return Ok(());
            }
            if let Some(stats) = self.task_skott.finish(None) {
                self.append(&stats)?;
            }
            self.task_skott.feed.take();
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
            self.task_progress = None;
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

    fn key(&mut self, key: KeyEvent, terminal: &mut Screen) -> io::Result<()> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('q') {
            self.request_quit();
            return Ok(());
        }
        if matches!(key.code, KeyCode::F(3..=5))
            && let Some(pane) = self.pane.for_key(key)
        {
            self.select_pane(pane);
            return Ok(());
        }
        if key.code == KeyCode::F(6) && self.pane == ActivePane::Both {
            self.split_skott_focus = !self.split_skott_focus;
            return Ok(());
        }
        if key.code == KeyCode::F(7) && self.running.is_none() {
            self.task_skott.shown = !self.task_skott.shown;
            self.prepare_skott_view();
            return Ok(());
        }
        if self.overlay.is_none() && self.skott_focused() && key.code != KeyCode::F(2) {
            return self.skott_key(key);
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
        if self.skott_focused() {
            let app = self.skott_app();
            if app.overlay == skott::tui::app::Overlay::None && !app.show_help {
                app.editor.insert_str(text);
            }
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
