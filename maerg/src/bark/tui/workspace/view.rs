use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::super::render;
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
        let raised = render::panel(self.theme);
        let selected = render::selection(self.theme);
        let border = render::border(self.theme);
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
            Paragraph::new(sanitize(&heading))
                .style(raised.fg(accent).add_modifier(Modifier::BOLD)),
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
        frame.render_widget(
            Paragraph::new(sanitize(&stats)).style(raised.patch(render::secondary(self.theme))),
            parts[1],
        );
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
            .border_style(border)
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
                .track_style(border),
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
            .style(raised)
            .border_style(border);
        let input_area = input.inner(footer);
        frame.render_widget(input, footer);
        let display = if self.running.is_some() && self.mask_answers {
            "*".repeat(self.input.text[..self.input.cursor].graphemes(true).count())
        } else {
            self.input.text.clone()
        };
        let cursor = if self.running.is_some() && self.mask_answers {
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
            Paragraph::new(sanitize(&text)).style(raised),
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
            super::bindings::footer()
        } else {
            self.notice.clone()
        };
        frame.render_widget(
            Paragraph::new(sanitize(&hints)).style(raised.patch(render::secondary(self.theme))),
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
            .style(raised)
            .highlight_style(selected);
            let mut state = ListState::default().with_selected(Some(self.candidate));
            frame.render_stateful_widget(list, popup, &mut state);
        }
        if let Some(overlay) = &mut self.overlay {
            let popup = centered(area);
            frame.render_widget(Clear, popup);
            match overlay {
                Overlay::Menu(menu) => {
                    let block = Block::bordered()
                        .title(format!(" {} ", sanitize(&menu.title)))
                        .title_bottom(" arrows / Enter | Esc back | F1 help ")
                        .border_style(border)
                        .style(raised);
                    let mut inner = block.inner(popup);
                    frame.render_widget(block, popup);
                    let visible = menu.visible();
                    if menu.page.searchable() && inner.height > 0 {
                        frame.render_widget(
                            Paragraph::new(format!(
                                "Filter: {}  ({} matches)",
                                sanitize(&menu.query),
                                visible.len()
                            ))
                            .style(raised),
                            Rect { height: 1, ..inner },
                        );
                        inner.y = inner.y.saturating_add(1);
                        inner.height = inner.height.saturating_sub(1);
                    }
                    let height = usize::from(inner.height);
                    let start = menu.selected.saturating_add(1).saturating_sub(height);
                    let end = (start + height).min(visible.len());
                    let list = List::new(visible[start.min(end)..end].iter().map(|&index| {
                        let entry = &menu.choices[index];
                        ListItem::new(format!(
                            "{}{}",
                            if entry.current { "[current] " } else { "" },
                            sanitize(&entry.label)
                        ))
                    }))
                    .style(raised)
                    .highlight_style(selected);
                    let mut state = ListState::default().with_selected(
                        (!visible.is_empty()).then_some(menu.selected.saturating_sub(start)),
                    );
                    frame.render_stateful_widget(list, inner, &mut state);
                    if visible.is_empty() {
                        frame.render_widget(
                            Paragraph::new("No matching entries").style(raised),
                            inner,
                        );
                    }
                }
                Overlay::Confirm { prompt, scroll, .. } => {
                    *scroll = document(
                        frame,
                        popup,
                        "Confirm: y / n / Esc",
                        &format!("{prompt}\n\n**y:** proceed. **n / Esc:** cancel."),
                        *scroll,
                        self.theme,
                    )?;
                }
                Overlay::Quit => {
                    frame.render_widget(
                        Paragraph::new("A command is running. Cancel it and quit?\n\nTask effects are not rolled back; review/recovery may be required.\n\ny: cancel and quit     n / Esc: keep running")
                            .block(Block::bordered().title(" Quit workspace ").border_style(border))
                            .style(raised).wrap(ratatui::widgets::Wrap { trim: false }), popup
                    );
                }
                Overlay::Help { scroll } => {
                    *scroll = document(
                        frame,
                        popup,
                        "Keyboard help",
                        &super::actions::view_help(),
                        *scroll,
                        self.theme,
                    )?;
                }
                Overlay::Details {
                    title,
                    markdown,
                    scroll,
                } => {
                    *scroll = document(frame, popup, title, markdown, *scroll, self.theme)?;
                }
            }
        }
        Ok(())
    }
}

fn document(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    source: &str,
    scroll: usize,
    theme: crate::bark::Theme,
) -> io::Result<usize> {
    let style = render::panel(theme);
    let block = Block::bordered()
        .title(format!(
            " {} | arrows / PgUp / PgDn | Esc back ",
            sanitize(title)
        ))
        .border_style(render::border(theme))
        .style(style);
    let inner = block.inner(area);
    let text = render::markdown(source, inner.width, theme)?;
    let start = scroll.min(text.lines.len().saturating_sub(usize::from(inner.height)));
    let end = (start + usize::from(inner.height)).min(text.lines.len());
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(Text::from(text.lines[start..end].to_vec())).style(style),
        inner,
    );
    Ok(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn long_confirmations_can_scroll_to_all_source_and_consumer_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::new(dir.path()).unwrap();
        workspace.overlay = Some(super::super::Overlay::Confirm {
            prompt: format!(
                "Remove model?\n\n{}\nUnknown consumers.",
                "Source details continue here.\n".repeat(40)
            ),
            arguments: vec![],
            scroll: 0,
        });
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        let mut capture = |workspace: &mut Workspace| {
            terminal
                .draw(|frame| workspace.draw(frame).unwrap())
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };
        assert!(!capture(&mut workspace).contains("Unknown consumers."));
        workspace
            .overlay_key(crossterm::event::KeyCode::End.into())
            .unwrap();
        assert!(capture(&mut workspace).contains("Unknown consumers."));
        let Some(super::super::Overlay::Confirm { scroll: bottom, .. }) = &workspace.overlay else {
            panic!("expected confirmation");
        };
        let bottom = *bottom;
        assert!(bottom > 0 && bottom < usize::MAX);
        workspace
            .overlay_key(crossterm::event::KeyCode::Up.into())
            .unwrap();
        let Some(super::super::Overlay::Confirm { scroll, .. }) = &workspace.overlay else {
            panic!("expected confirmation");
        };
        assert_eq!(*scroll, bottom - 1);
    }

    #[test]
    fn nested_settings_back_restores_parent_cursor_and_command_draft() {
        let dir = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::new(dir.path()).unwrap();
        workspace.input.set("status --format json");
        workspace
            .open_menu(super::super::menus::Page::Root)
            .unwrap();
        for _ in 0..3 {
            workspace
                .overlay_key(crossterm::event::KeyCode::Down.into())
                .unwrap();
        }
        workspace
            .overlay_key(crossterm::event::KeyCode::Enter.into())
            .unwrap();
        workspace
            .overlay_key(crossterm::event::KeyCode::Esc.into())
            .unwrap();
        let Some(super::super::Overlay::Menu(menu)) = &workspace.overlay else {
            panic!("expected parent menu");
        };
        assert_eq!(menu.selected, 3);
        assert_eq!(workspace.input.text, "status --format json");
    }

    #[test]
    fn theme_picker_preselects_current_value() {
        let dir = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::new(dir.path()).unwrap();
        workspace.set_theme("light").unwrap();
        workspace
            .open_menu(super::super::menus::Page::Theme)
            .unwrap();
        let Some(super::super::Overlay::Menu(menu)) = &workspace.overlay else {
            panic!("expected theme picker");
        };
        assert_eq!(menu.choices[menu.selected].label, "light");
        assert!(menu.choices[menu.selected].current);
    }

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
        workspace
            .open_menu(super::super::menus::Page::Root)
            .unwrap();
        let Some(super::super::Overlay::Menu(menu)) = &workspace.overlay else {
            panic!("expected settings menu");
        };
        assert_eq!(menu.choices.len(), 7);
        assert!(menu.choices.iter().any(|entry| entry.label == "Quit"));
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
