//! Presentation-only attachment to a live diagnostic journal.

use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};
use sav::observation::{ControlRequest, JournalFollower, JournalRecord, send_control};

use crate::error::{Error, Result, terminal_text};

const MAX_VIEW_BYTES: usize = 1024 * 1024;
const MAX_VIEW_LINES: usize = 2048;
const MAX_LINE_BYTES: usize = 4096;
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

/// Inspects a run without provider configuration or any executor.
pub fn inspect(
    path: &Path,
    follow: bool,
    control: Option<&Path>,
    cancel: bool,
    guidance: Option<&str>,
) -> Result<()> {
    let mut journal = JournalFollower::open(path).map_err(observation_error)?;
    if cancel || guidance.is_some() {
        let control = control.ok_or_else(|| Error::Config {
            path: None,
            reason: "run control requires an explicit --control capability".into(),
        })?;
        let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        let request = match guidance {
            Some(text) => ControlRequest::guide(id, text),
            None => ControlRequest::cancel(id),
        };
        let reply =
            send_control(control, &request, Duration::from_secs(2)).map_err(observation_error)?;
        eprintln!(
            "control {}: {}{}",
            reply.id,
            terminal_text(&reply.status),
            reply
                .message
                .as_ref()
                .map_or(String::new(), |message| format!(
                    " ({})",
                    terminal_text(message)
                ))
        );
        if reply.status == "rejected" {
            return Err(Error::Config {
                path: None,
                reason: "run owner rejected the control request".into(),
            });
        }
    }
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        terminal_view(&mut journal, None, control)
    } else {
        plain_view(&mut journal, follow)
    }
}

fn observation_error(error: sav::Error) -> Error {
    Error::Recording {
        reason: error.to_string(),
    }
}

fn plain_view(journal: &mut JournalFollower, follow: bool) -> Result<()> {
    sav::install_handler();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    loop {
        let records = journal.poll().map_err(observation_error)?;
        let has_more = records.len() == 256;
        for record in records {
            writeln!(writer, "{}", display_record(&record))?;
        }
        writer.flush()?;
        if journal.is_finished() {
            journal.end_of_input().map_err(observation_error)?;
            return Ok(());
        }
        if !follow && !has_more {
            writeln!(
                writer,
                "[incomplete run; snapshot only, no terminal disposition observed]"
            )?;
            return Ok(());
        }
        if sav::take_interrupted() {
            writeln!(writer, "[detached; execution was not cancelled]")?;
            return Ok(());
        }
        if !has_more {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn display_record(record: &JournalRecord) -> String {
    // JSON string rendering escapes untrusted terminal control bytes.
    let event = record.event.to_string();
    let mut end = event.len().min(MAX_LINE_BYTES);
    while !event.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{} {}{}",
        record.sequence,
        &event[..end],
        if end < event.len() {
            " [display truncated]"
        } else {
            ""
        }
    )
}

struct Screen {
    active: bool,
}

impl Screen {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut screen = Self { active: true };
        if let Err(error) = execute!(std::io::stdout(), EnterAlternateScreen) {
            screen.restore()?;
            return Err(error.into());
        }
        Ok(screen)
    }

    fn restore(&mut self) -> Result<()> {
        if self.active {
            self.active = false;
            let leave = execute!(std::io::stdout(), LeaveAlternateScreen);
            let raw = disable_raw_mode();
            leave.and(raw)?;
        }
        Ok(())
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            tracing::error!(%error, "inspection terminal restoration failed");
        }
    }
}

/// Observes a terminal-owned opaque process with an explicit in-process cancel action.
pub fn inspect_owned(path: &Path, cancellation: &sav::CancellationToken) -> Result<()> {
    let mut journal = JournalFollower::open(path).map_err(observation_error)?;
    terminal_view(&mut journal, Some(cancellation), None)
}

fn terminal_view(
    journal: &mut JournalFollower,
    owner_cancel: Option<&sav::CancellationToken>,
    control: Option<&Path>,
) -> Result<()> {
    let mut screen = Screen::enter()?;
    let result = (|| {
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        let mut lines: VecDeque<String> = VecDeque::new();
        let mut retained = 0usize;
        let mut offset = 0usize;
        let mut horizontal = 0u16;
        let mut following = true;
        let mut dropped = 0usize;
        let mut guidance: Option<String> = None;
        let mut status = String::new();
        loop {
            for record in journal.poll().map_err(observation_error)? {
                let text = display_record(&record);
                retained += text.len();
                lines.push_back(text);
                while retained > MAX_VIEW_BYTES || lines.len() > MAX_VIEW_LINES {
                    if let Some(line) = lines.pop_front() {
                        retained -= line.len();
                        dropped += 1;
                        offset = offset.saturating_sub(1);
                    }
                }
            }
            let height = usize::from(terminal.size()?.height.saturating_sub(4)).max(1);
            let last = lines.len().saturating_sub(height);
            offset = if following { last } else { offset.min(last) };
            terminal.draw(|frame| {
                let areas = Layout::vertical([Constraint::Min(1), Constraint::Length(2)])
                    .split(frame.area());
                let content = lines
                    .iter()
                    .map(|line| Line::raw(line.as_str()))
                    .collect::<Vec<_>>();
                let title = format!(
                    "Inspection ({}) | {} | {dropped} older display records omitted",
                    if control.is_some() || owner_cancel.is_some() {"explicit controls enabled"} else {"read-only"},
                    if journal.is_finished() {
                        "terminal disposition recorded"
                    } else {
                        "live / incomplete"
                    }
                );
                frame.render_widget(
                    Paragraph::new(content)
                        .block(Block::default().borders(Borders::ALL).title(title))
                        .scroll((offset as u16, horizontal)),
                    areas[0],
                );
                frame.render_widget(
                    Paragraph::new(guidance.as_ref().map_or_else(
                        ||format!("{}\n{}",if control.is_some() {"c request cancellation | g compose guidance | q/Esc/Ctrl+C detach"} else if owner_cancel.is_some() {"c cancel owned run | q/Esc/Ctrl+C detach viewer | arrows scroll"} else {"q/Esc/Ctrl+C detach | Up/Down/Pg/Home/End scroll | Left/Right pan"},status),
                        |text|format!("Guidance (Enter queue, Esc discard): {}\n{}",terminal_text(text),status))),
                    areas[1],
                );
            })?;
            if event::poll(Duration::from_millis(50))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                if let Some(text) = &mut guidance {
                    match key.code {
                        KeyCode::Esc => guidance = None,
                        KeyCode::Backspace => {
                            text.pop();
                        }
                        KeyCode::Enter => {
                            if let Some(path) = control {
                                status = exchange(path, Some(text));
                                guidance = None;
                            }
                        }
                        KeyCode::Char(character) if !character.is_control() => {
                            if text.len() + character.len_utf8() <= 16 * 1024 {
                                text.push(character);
                            } else {
                                status = "guidance exceeds 16384 bytes; shorten it before sending"
                                    .into();
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('c') if owner_cancel.is_some() => {
                        if let Some(token) = owner_cancel {
                            token.cancel();
                        }
                        status =
                            "cancellation requested; previous/unknown effects are not rolled back"
                                .into();
                    }
                    KeyCode::Char('c') if control.is_some() => {
                        if let Some(path) = control {
                            status = exchange(path, None);
                        }
                    }
                    KeyCode::Char('g') if control.is_some() => guidance = Some(String::new()),
                    KeyCode::Up => {
                        following = false;
                        offset = offset.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        following = false;
                        offset = offset.saturating_add(1).min(last);
                    }
                    KeyCode::PageUp => {
                        following = false;
                        offset = offset.saturating_sub(height);
                    }
                    KeyCode::PageDown => {
                        following = false;
                        offset = offset.saturating_add(height).min(last);
                    }
                    KeyCode::Home => {
                        following = false;
                        offset = 0;
                    }
                    KeyCode::End => following = true,
                    KeyCode::Left => horizontal = horizontal.saturating_sub(8),
                    KeyCode::Right => {
                        horizontal = horizontal.saturating_add(8).min(MAX_LINE_BYTES as u16)
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    result.and(screen.restore())
}

fn exchange(path: &Path, guidance: Option<&str>) -> String {
    let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    let request = match guidance {
        Some(text) => ControlRequest::guide(id, text),
        None => ControlRequest::cancel(id),
    };
    match send_control(path, &request, Duration::from_secs(2)) {
        Ok(reply) => format!(
            "control #{}: {}{}",
            reply.id,
            terminal_text(&reply.status),
            reply
                .message
                .as_ref()
                .map_or(String::new(), |message| format!(
                    " ({})",
                    terminal_text(message)
                ))
        ),
        Err(error) => format!(
            "control request failed: {}",
            terminal_text(&error.to_string())
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interactive_control_exchange_reports_queueing_rejection_and_cancellation_honestly() {
        let mailbox = sav::runtime::ControlMailbox::default();
        let token = sav::CancellationToken::new();
        let server = sav::observation::bind_runtime_controls(&mailbox, &token).expect("server");
        assert!(exchange(server.path(), Some("late guidance")).contains("rejected"));
        mailbox.begin_run().expect("owner");
        let reply = exchange(server.path(), Some("retain the objective"));
        assert!(reply.contains("queued"));
        assert!(reply.contains("not yet delivered"));
        assert_eq!(mailbox.claim().expect("claimed").len(), 1);
        assert!(exchange(server.path(), Some("")).contains("failed"));
        assert!(exchange(server.path(), None).contains("cancellation_requested"));
        assert!(token.is_cancelled());
        mailbox.end_run().expect("end");
        assert!(mailbox.next_task().expect("tasks").is_none());
    }
}
