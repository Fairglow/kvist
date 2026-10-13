//! A minimal full-screen pager with a right-hand scrollbar.
//!
//! The pager takes over the terminal (alternate screen, raw mode, mouse
//! capture), renders the pre-styled lines it is given, and shows a
//! one-column scrollbar on the right whose thumb size reflects how large
//! the output is and whose position reflects where the viewer is. Keys:
//! `q`/`Esc` quit, arrows and `j`/`k` scroll one line, `PageUp`/`PageDown`
//! scroll a page, `g`/`G` (and Home/End) jump to the ends, `d`/`u` scroll a
//! half page, and the mouse wheel scrolls. On exit the last visible line is
//! reprinted (like `less`) so the shell's next prompt follows the content.
//!
//! [`page`] is the only entry point; a pager that cannot take the terminal
//! returns `false` and the caller prints the output directly, so output is
//! never lost.

#![cfg_attr(test, allow(dead_code))]
use std::io::Write;
use std::time::Duration;

use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseEventKind, poll, read,
    },
    execute,
    style::{Attribute, Print, ResetColor, SetAttribute},
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

/// Columns reserved on the right of every line: one gutter space plus the
/// one-column scrollbar.
const GUTTER_COLUMNS: usize = 2;

/// Lines scrolled per mouse wheel notch.
const WHEEL_STEP: usize = 3;

/// The largest valid top offset for a view of `height` rows over `lines`.
fn max_offset(lines: &[String], height: usize) -> usize {
    lines.len().saturating_sub(height.max(1))
}

/// The scrollbar thumb geometry, derived purely so it is testable.
///
/// Returns `(thumb_top, thumb_height)` in track rows for `total` content
/// rows, `visible` track rows, and a top offset of `offset`. A document that
/// fits fills the track; a thumb is never shorter than two rows.
pub fn scrollbar_geometry(total: usize, visible: usize, offset: usize) -> (usize, usize) {
    if visible == 0 {
        return (0, 0);
    }
    if total <= visible {
        return (0, visible);
    }
    let mut thumb = (visible as u64 * visible as u64 / total as u64) as usize;
    if thumb < 2 {
        thumb = 2;
    }
    if thumb > visible {
        thumb = visible;
    }
    let track = visible - thumb;
    let range = total - visible;
    let top = if range == 0 {
        0
    } else {
        (offset as u64 * track as u64 / range as u64) as usize
    };
    (top, thumb)
}

/// Pages `text` full-screen with a scrollbar.
///
/// Returns `true` when the pager ran (the user quit it) and `false` when the
/// pager could not take the terminal, in which case the caller must print the
/// output directly.
pub fn page(text: &str) -> bool {
    let Some((mut width, mut height)) = super::style::terminal_size() else {
        return false;
    };
    if width < 10 || height < 3 {
        return false;
    }
    let mut stdout = std::io::stdout();
    if crossterm::terminal::enable_raw_mode().is_err() {
        return false;
    }
    if execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        Clear(ClearType::All)
    )
    .is_err()
    {
        let _ = crossterm::terminal::disable_raw_mode();
        return false;
    }
    let mut offset: usize = 0;
    let mut lines = wrap(text, width.saturating_sub(GUTTER_COLUMNS));
    render(&mut stdout, width, height, &lines, offset);
    while let Ok(event) = read_event() {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Up | KeyCode::Char('k') => offset = offset.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    offset = (offset + 1).min(max_offset(&lines, height))
                }
                KeyCode::PageUp => offset = offset.saturating_sub(height.saturating_sub(1).max(1)),
                KeyCode::PageDown => {
                    let page = height.saturating_sub(1).max(1);
                    offset = (offset + page).min(max_offset(&lines, height))
                }
                KeyCode::Home | KeyCode::Char('g') => offset = 0,
                KeyCode::End | KeyCode::Char('G') => offset = max_offset(&lines, height),
                KeyCode::Char('d') => {
                    let half = height / 2;
                    offset = (offset + half).min(max_offset(&lines, height))
                }
                KeyCode::Char('u') => offset = offset.saturating_sub(height / 2),
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => offset = offset.saturating_sub(WHEEL_STEP),
                MouseEventKind::ScrollDown => {
                    offset = (offset + WHEEL_STEP).min(max_offset(&lines, height))
                }
                _ => {}
            },
            Event::Resize(new_width, new_height) => {
                if (new_width as usize) < 10 || (new_height as usize) < 3 {
                    continue;
                }
                width = new_width as usize;
                height = new_height as usize;
                lines = wrap(text, width.saturating_sub(GUTTER_COLUMNS));
                offset = offset.min(max_offset(&lines, height));
            }
            // Polling timeouts and other events leave the view unchanged.
            _ => {}
        }
        render(&mut stdout, width, height, &lines, offset);
    }
    let _ = execute!(stdout, DisableMouseCapture, LeaveAlternateScreen);
    let _ = crossterm::terminal::disable_raw_mode();
    // Reprint the final line so the following prompt reads continuously.
    let last = lines.last().cloned().unwrap_or_default();
    if !last.is_empty() {
        let _ = writeln!(std::io::stdout(), "{last}");
    }
    true
}

/// Reads one terminal event, waiting at most one second.
fn read_event() -> std::io::Result<Event> {
    poll(Duration::from_millis(200))?;
    read()
}

/// Wraps `text` to `width` visible columns, keeping ANSI styling intact.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![];
    }
    super::pager::prepare_display(text, width)
        .lines()
        .map(|l| l.to_owned())
        .collect()
}

/// Draws one full screen: content lines, the gutter, and the scrollbar.
///
/// Every content line is drawn at column 0 of its own terminal row (crossterm's
/// `MoveTo` takes the column first), so the full terminal height carries
/// content and scrolling reveals it line by line; the scrollbar symbols ride
/// the last column. The pass writes to any `Write`, so the emitted cursor
/// geometry is unit-testable without a terminal.
fn render(writer: &mut impl Write, width: usize, height: usize, lines: &[String], offset: usize) {
    let (thumb_top, thumb_height) = scrollbar_geometry(lines.len(), height, offset);
    for row in 0..height {
        let line = lines.get(offset + row).map(String::as_str).unwrap_or("");
        let _ = execute!(
            writer,
            crossterm::cursor::MoveTo(0, row as u16),
            Clear(ClearType::CurrentLine),
            Print(line),
            Print(" "),
        );
        if row >= thumb_top && row < thumb_top + thumb_height {
            let _ = execute!(
                writer,
                crossterm::cursor::MoveTo(width as u16 - 1, row as u16),
                Print("█"),
            );
        } else {
            let _ = execute!(
                writer,
                crossterm::cursor::MoveTo(width as u16 - 1, row as u16),
                SetAttribute(Attribute::Dim),
                Print("·"),
                ResetColor,
            );
        }
    }
    let _ = execute!(writer, crossterm::cursor::Hide);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The visible content of one rendered row: the cursor move to column 0
    /// of the row, the line clear, and the row's text.
    fn row_prefix(row: usize) -> String {
        format!("\x1b[{row};1H\x1b[2K")
    }

    #[test]
    fn render_places_each_content_line_at_column_zero_of_its_row() {
        let lines: Vec<String> = (0..4).map(|i| format!("line {i}")).collect();
        let mut out: Vec<u8> = Vec::new();
        render(&mut out, 40, 6, &lines, 0);
        let text = String::from_utf8(out).unwrap();
        for (row, line) in lines.iter().enumerate() {
            let prefix = row_prefix(row + 1);
            let idx = text
                .find(&prefix)
                .unwrap_or_else(|| panic!("missing cursor move to row {} column 0", row + 1));
            let after = &text[idx + prefix.len()..];
            assert!(
                after.starts_with(line.as_str()),
                "row {} must carry its own content, got {after:?}",
                row + 1
            );
        }
    }

    #[test]
    fn render_uses_the_full_terminal_height() {
        let lines: Vec<String> = vec!["first".to_owned(), "second".to_owned()];
        let mut out: Vec<u8> = Vec::new();
        render(&mut out, 30, 10, &lines, 0);
        let text = String::from_utf8(out).unwrap();
        // Every row of the view gets its own cursor move, content row or
        // blank row alike: the pager owns the full screen height.
        for row in 1..=10 {
            assert!(
                text.contains(&row_prefix(row)),
                "row {row} was never addressed"
            );
        }
        assert!(text.contains(&format!("{}first", row_prefix(1))));
        assert!(text.contains(&format!("{}second", row_prefix(2))));
    }

    #[test]
    fn render_scrolling_shifts_the_visible_window() {
        let lines: Vec<String> = (0..6).map(|i| format!("line {i}")).collect();
        let mut out: Vec<u8> = Vec::new();
        render(&mut out, 20, 3, &lines, 2);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains(&format!("{}line 2", row_prefix(1))));
        assert!(text.contains(&format!("{}line 3", row_prefix(2))));
        assert!(text.contains(&format!("{}line 4", row_prefix(3))));
    }

    #[test]
    fn render_keeps_the_scrollbar_in_the_last_column() {
        let lines: Vec<String> = (0..6).map(|i| format!("line {i}")).collect();
        let mut out: Vec<u8> = Vec::new();
        render(&mut out, 12, 4, &lines, 0);
        let text = String::from_utf8(out).unwrap();
        for row in 1..=4 {
            let prefix = format!("\x1b[{row};12H");
            assert!(text.contains(&prefix), "scrollbar missing on row {row}");
        }
    }

    #[test]
    fn scrollbar_fills_the_track_when_the_document_fits() {
        assert_eq!(scrollbar_geometry(5, 10, 0), (0, 10));
        assert_eq!(scrollbar_geometry(0, 10, 0), (0, 10));
    }

    #[test]
    fn scrollbar_thumb_is_proportional_to_the_page() {
        // A document twice the view gets a half-height thumb.
        assert_eq!(scrollbar_geometry(20, 10, 0), (0, 5));
        // A document four times the view gets a quarter-height thumb.
        assert_eq!(scrollbar_geometry(40, 10, 0), (0, 2));
        // A very large document keeps the two-row thumb floor.
        assert_eq!(scrollbar_geometry(100, 10, 0), (0, 2));
    }

    #[test]
    fn scrollbar_thumb_tracks_the_offset() {
        // 40 rows, 10 visible: the thumb (2 rows) slides 0..8.
        let (top, thumb) = scrollbar_geometry(40, 10, 0);
        assert_eq!((top, thumb), (0, 2));
        let (mid, _) = scrollbar_geometry(40, 10, 15);
        let (end, _) = scrollbar_geometry(40, 10, 30);
        assert_eq!(mid, 4);
        assert_eq!(end, 8);
    }

    #[test]
    fn scrollbar_never_exceeds_the_track() {
        let (top, thumb) = scrollbar_geometry(1000, 24, 976);
        assert!(top + thumb <= 24);
    }
}
