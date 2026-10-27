use crossterm::event::{KeyCode, KeyModifiers};

use crate::links::Link;
use crate::render_writer::LinkSpan;
use crate::viewport::Document;

pub(crate) const HELP: &str = "\
# Blad keyboard help

/ literal search; Ctrl+R starts regex or toggles while editing
FIND: Enter confirms search; Esc cancels; q is query text
SEARCH: n / > / Enter next; N / < previous; Esc/q exits
Current match: reversed + underlined; others: underlined

Tab / Shift+Tab: next / previous link; Enter: follow
Ctrl+Arrows: spatial links; [ / ]: previous / next heading

j/k or arrows: line; Space / PageDown / Ctrl+F: page down
PageUp / Ctrl+B: page up; d/u or Ctrl+D/U: half page
g/G or Ctrl+Home/End: top/bottom; Home/End: horizontal start/end

Alt+Left/Right: tabs; Ctrl+PageUp/Down also work
b: back; e: editor; file saves reload automatically

stdin streams automatically; scrolling up/search pauses following
G / Ctrl+End resumes following; EOF keeps the document open

q/Esc: close tab outside search; Ctrl+Q/Ctrl+C quit; Ctrl+L redraw
? / F1 / h: this help; Ctrl+C quits from any mode
";

/// Update the live pin, returning whether downward navigation can re-pin it.
pub(crate) fn follow_key(code: KeyCode, modifiers: KeyModifiers, following: &mut bool) -> bool {
    let control = modifiers.contains(KeyModifiers::CONTROL);
    if (modifiers.contains(KeyModifiers::ALT) && matches!(code, KeyCode::Left | KeyCode::Right))
        || (control && matches!(code, KeyCode::PageUp | KeyCode::PageDown))
    {
        return false;
    }
    if matches!(code, KeyCode::Char('G' | 'L')) || (control && code == KeyCode::End) {
        *following = true;
    } else if matches!(
        code,
        KeyCode::Up
            | KeyCode::PageUp
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Char('k' | 'g' | 'H' | 'u' | '/' | '[' | ']' | 'n' | 'N' | '<' | '>')
    ) || (control
        && matches!(
            code,
            KeyCode::Home
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Char('b' | 'u' | 'r')
        ))
    {
        *following = false;
    }
    matches!(code, KeyCode::PageDown | KeyCode::Char('j' | ' ' | 'd'))
        || (code == KeyCode::Down && !control)
        || (code == KeyCode::Char('f') && control)
}

pub(crate) fn link_spans(document: &Document, link: &Link) -> Vec<LinkSpan> {
    document
        .navigation
        .links
        .get(&link.navigation_id())
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn spatial_link(
    spans: &[Vec<LinkSpan>],
    selected: Option<(usize, usize)>,
    origin: (usize, usize),
    direction: KeyCode,
) -> Option<(usize, usize)> {
    let (row, column) = selected
        .and_then(|(index, span)| spans.get(index)?.get(span))
        .map_or(origin, |span| (span.row, span.start));
    spans
        .iter()
        .enumerate()
        .filter(|(index, _)| selected.is_none_or(|(selected, _)| selected != *index))
        .flat_map(|(index, spans)| {
            spans.iter().enumerate().filter_map(move |(part, span)| {
                let eligible = match direction {
                    KeyCode::Left => span.row == row && span.start < column,
                    KeyCode::Right => span.row == row && span.start > column,
                    KeyCode::Up => span.row < row,
                    KeyCode::Down => span.row > row,
                    _ => false,
                };
                eligible.then_some((
                    (
                        span.row.abs_diff(row),
                        span.start.abs_diff(column),
                        index,
                        part,
                    ),
                    (index, part),
                ))
            })
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, target)| target)
}

pub(crate) fn reveal(span: &LinkSpan, y: &mut usize, x: &mut usize, height: usize, width: usize) {
    if span.row < *y {
        *y = span.row;
    } else if span.row >= y.saturating_add(height) {
        *y = span.row.saturating_sub(height / 2);
    }
    if span.start < *x || span.end.saturating_sub(span.start) > width {
        *x = span.start;
    } else if span.end > x.saturating_add(width) {
        *x = span.end.saturating_sub(width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_keys_pause_browsing_resume_bottom_and_leave_tab_switches_alone() {
        for (code, modifiers) in [
            (KeyCode::Up, KeyModifiers::NONE),
            (KeyCode::Home, KeyModifiers::CONTROL),
            (KeyCode::Right, KeyModifiers::CONTROL),
            (KeyCode::Char('/'), KeyModifiers::NONE),
            (KeyCode::Char('n'), KeyModifiers::NONE),
            (KeyCode::Tab, KeyModifiers::NONE),
        ] {
            let mut following = true;
            assert!(!follow_key(code, modifiers, &mut following));
            assert!(!following);
        }
        for code in [KeyCode::PageUp, KeyCode::PageDown] {
            let mut following = true;
            assert!(!follow_key(code, KeyModifiers::CONTROL, &mut following));
            assert!(following);
        }
        let mut following = false;
        follow_key(KeyCode::End, KeyModifiers::CONTROL, &mut following);
        assert!(following);
        assert!(follow_key(
            KeyCode::Down,
            KeyModifiers::NONE,
            &mut following
        ));
    }

    fn span(row: usize, start: usize) -> LinkSpan {
        LinkSpan {
            row,
            start,
            end: start + 4,
        }
    }

    #[test]
    fn spatial_direction_distance_and_wrapped_selection_are_deterministic() {
        let spans = vec![
            vec![span(0, 2), span(1, 0)],
            vec![span(0, 20)],
            vec![span(2, 19)],
        ];
        assert_eq!(
            spatial_link(&spans, Some((0, 0)), (0, 0), KeyCode::Right),
            Some((1, 0))
        );
        assert_eq!(
            spatial_link(&spans, Some((1, 0)), (0, 0), KeyCode::Down),
            Some((0, 1))
        );
        assert_eq!(
            spatial_link(&spans, Some((0, 1)), (0, 0), KeyCode::Down),
            Some((2, 0))
        );
        assert_eq!(
            spatial_link(&spans, Some((0, 0)), (0, 0), KeyCode::Left),
            None
        );
        let (mut y, mut x) = (0, 0);
        reveal(&span(20, 40), &mut y, &mut x, 10, 20);
        assert_eq!((y, x), (15, 24));
    }
}
