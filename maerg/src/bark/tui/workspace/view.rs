use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{Overlay, Workspace};
use crate::presentation::sanitize;

fn centered(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).min(100);
    let height = area.height.saturating_sub(4);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

impl Workspace {
    pub(super) fn draw(&mut self, frame: &mut Frame<'_>) -> io::Result<()> {
        let area = frame.area();
        let (base, accent) = super::super::render::chrome(self.theme);
        frame.render_widget(Block::default().style(base), area);
        let parts = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);
        let heading = format!(
            " Kvist | {} | {} | {}",
            self.focus().as_deref().unwrap_or("(project)"),
            self.state_label,
            self.progress
        );
        frame.render_widget(
            Paragraph::new(sanitize(&heading)).style(base.fg(accent).add_modifier(Modifier::BOLD)),
            parts[0],
        );
        let run = self
            .running
            .as_ref()
            .map(|run| format!(" | RUNNING {}s", run.started.elapsed().as_secs()))
            .unwrap_or_default();
        let stats = format!(
            " {} | sandbox: {} | model: {} | locks: {} live / {} stale{}",
            self.state.branch().unwrap_or("no-vcs"),
            self.status.sandbox_backend,
            self.status.default_model,
            self.status.live_locks,
            self.status.stale_locks,
            run,
        );
        frame.render_widget(Paragraph::new(sanitize(&stats)).style(base), parts[1]);
        let body = parts[2];
        let title = format!(
            " Transcript {}/{} | {}{} ",
            self.segment + 1,
            self.transcript.latest() + 1,
            if self.follow { "following" } else { "paused" },
            if self.last_failed {
                " | last command failed"
            } else {
                ""
            },
        );
        let panel = Block::default()
            .borders(Borders::TOP)
            .title(title)
            .border_style(Style::default().fg(accent))
            .style(base);
        let mut inner = panel.inner(body);
        inner.width = inner.width.saturating_sub(1);
        frame.render_widget(panel, body);
        self.viewport = usize::from(inner.height).max(1);
        let bottom = self.rendered.lines.len().saturating_sub(self.viewport);
        if self.follow {
            self.scroll = bottom;
        } else {
            self.scroll = self.scroll.min(bottom);
        }
        let end = (self.scroll + self.viewport).min(self.rendered.lines.len());
        let lines = self
            .rendered
            .lines
            .get(self.scroll..end)
            .unwrap_or_default()
            .to_vec();
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .style(base)
                .scroll((0, self.horizontal)),
            inner,
        );
        let segments = self.transcript.latest() as f64 + 1.0;
        let lines = self.rendered.lines.len().max(self.viewport) as f64;
        let position = (self.segment as f64 + self.scroll as f64 / lines) / segments;
        let visible = self.viewport as f64 / lines / segments;
        let mut scrollbar = ScrollbarState::new(65_536)
            .position((position * 65_536.0) as usize)
            .viewport_content_length((visible * 65_536.0).max(1.0) as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::default().fg(accent))
                .track_style(Style::default().fg(Color::DarkGray)),
            body,
            &mut scrollbar,
        );
        let footer = parts[3];
        let label = if self.running.is_some() {
            "answer> "
        } else {
            "> "
        };
        let input = Block::default()
            .borders(Borders::TOP)
            .title(" Command ")
            .style(base)
            .border_style(Style::default().fg(accent));
        let input_area = input.inner(footer);
        frame.render_widget(input, footer);
        let display = if self.running.is_some() {
            "*".repeat(self.input.text[..self.input.cursor].graphemes(true).count())
        } else {
            self.input.text.clone()
        };
        let cursor = if self.running.is_some() {
            display.len()
        } else {
            self.input.cursor
        };
        let available = usize::from(input_area.width)
            .saturating_sub(label.len())
            .max(1);
        let mut start = cursor;
        let mut columns = 0;
        for (index, grapheme) in display[..cursor].grapheme_indices(true).rev() {
            let width = grapheme.width();
            if columns + width >= available {
                break;
            }
            start = index;
            columns += width;
        }
        let text = format!("{label}{}", &display[start..]);
        frame.render_widget(
            Paragraph::new(sanitize(&text)).style(base),
            Rect {
                height: 1,
                ..input_area
            },
        );
        if self.overlay.is_none() {
            frame.set_cursor_position((
                input_area
                    .x
                    .saturating_add((label.len() + columns) as u16)
                    .min(area.right().saturating_sub(1)),
                input_area.y.min(area.bottom().saturating_sub(1)),
            ));
        }
        let hints = if self.notice.is_empty() {
            "Tab complete  F1 help  F2 settings  PgUp/PgDn scroll  Ctrl+C cancel  Ctrl+D EOF/exit"
        } else {
            &self.notice
        };
        frame.render_widget(
            Paragraph::new(sanitize(hints)).style(base.fg(accent)),
            Rect {
                y: input_area.y.saturating_add(1),
                height: 1,
                ..input_area
            },
        );
        if !self.candidates.is_empty() && self.overlay.is_none() {
            let height = (self.candidates.len().min(8) as u16 + 2).min(body.height);
            let popup = Rect {
                x: body.x,
                y: body.bottom().saturating_sub(height),
                width: body.width.min(100),
                height,
            };
            frame.render_widget(Clear, popup);
            let list = List::new(self.candidates.iter().map(|candidate| {
                ListItem::new(Line::from(vec![
                    Span::styled(sanitize(&candidate.value), Style::default().fg(accent)),
                    Span::raw(format!(
                        "  {}",
                        sanitize(candidate.description.as_deref().unwrap_or(""))
                    )),
                ]))
            }))
            .block(
                Block::bordered().title(" Completion: Tab / arrows; Enter selects; Esc dismisses "),
            )
            .style(base)
            .highlight_style(base.add_modifier(Modifier::REVERSED));
            let mut state = ListState::default().with_selected(Some(self.candidate));
            frame.render_stateful_widget(list, popup, &mut state);
        }
        if let Some(overlay) = &self.overlay {
            let popup = centered(area);
            frame.render_widget(Clear, popup);
            match overlay {
                Overlay::Menu {
                    title,
                    choices,
                    selected,
                } => {
                    let list = List::new(
                        choices
                            .iter()
                            .map(|(label, _)| ListItem::new(sanitize(label))),
                    )
                    .block(Block::bordered().title(format!(" {title} | arrows / Enter / Esc ")))
                    .style(base)
                    .highlight_style(base.add_modifier(Modifier::REVERSED));
                    let mut state = ListState::default().with_selected(Some(*selected));
                    frame.render_stateful_widget(list, popup, &mut state);
                }
                Overlay::Confirm { prompt, .. } => {
                    frame.render_widget(
                        Paragraph::new(format!(
                            "Confirm {}?\n\ny: proceed     n / Esc: cancel",
                            sanitize(prompt)
                        ))
                        .block(Block::bordered().title(" Explicit confirmation "))
                        .style(base),
                        popup,
                    );
                }
                Overlay::Help { scroll } => {
                    let block = Block::bordered()
                        .title(" Keyboard help | arrows / PgUp / PgDn | Esc closes ")
                        .style(base);
                    let inner = block.inner(popup);
                    let help = super::super::render::markdown(
                        super::actions::view_help(),
                        inner.width,
                        self.theme,
                    )?;
                    let start =
                        (*scroll).min(help.lines.len().saturating_sub(usize::from(inner.height)));
                    let end = (start + usize::from(inner.height)).min(help.lines.len());
                    frame.render_widget(block, popup);
                    frame.render_widget(
                        Paragraph::new(Text::from(help.lines[start..end].to_vec())).style(base),
                        inner,
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn layout_has_status_transcript_scrollbar_and_command_footer() {
        let dir = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::new(dir.path()).unwrap();
        workspace.notice.clear();
        workspace.source = "# Rendered heading\n\n**Markdown** transcript.\n".into();
        workspace.rendered = super::super::super::render::markdown(
            &workspace.source,
            77,
            crate::bark::Theme::dark(),
        )
        .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| workspace.draw(frame).unwrap())
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text = (0..24)
            .map(|row| {
                (0..80)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        for expected in [
            "Kvist",
            "(project)",
            "sandbox:",
            "model:",
            "Transcript",
            "Rendered heading",
            "Command",
            "F1 help",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        assert!(!text.contains("**Markdown**"));
    }

    #[test]
    fn history_seek_spans_all_segments_and_focus_reopens_private_history() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::initialize(dir.path()).unwrap();
        let mut workspace = Workspace::new(dir.path()).unwrap();
        workspace.append(&"first\n\n".repeat(160_000)).unwrap();
        let last = workspace.transcript.latest();
        assert!(last > 0);
        workspace.seek_history(0, 100).unwrap();
        assert_eq!(workspace.segment, 0);
        workspace.seek_history(100, 100).unwrap();
        assert_eq!(workspace.segment, last);
        workspace.change_focus(Some(".".into())).unwrap();
        workspace.append("# Component only\n").unwrap();
        workspace.history = vec!["status".into()];
        workspace
            .transcript
            .save_history(&workspace.history)
            .unwrap();
        workspace.change_focus(None).unwrap();
        assert!(!workspace.source.contains("# Component only"));
        workspace.change_focus(Some(".".into())).unwrap();
        assert!(workspace.source.contains("# Component only"));
        assert_eq!(workspace.history, ["status"]);
    }

    #[test]
    fn settings_expose_choices_and_failed_theme_changes_leave_preference_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::new(dir.path()).unwrap();
        workspace.menu("Settings");
        let Some(super::super::Overlay::Menu { choices, .. }) = &workspace.overlay else {
            panic!("expected settings menu");
        };
        assert_eq!(choices.len(), 6);
        workspace.set_theme("light").unwrap();
        assert_eq!(workspace.theme.name(), "light");
        let saved = std::fs::read_to_string(dir.path().join(".kvist/theme")).unwrap();
        assert!(
            workspace
                .set_theme("/does-not-exist/kvist-theme.toml")
                .is_err()
        );
        assert_eq!(workspace.theme.name(), "light");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".kvist/theme")).unwrap(),
            saved
        );
    }
}
