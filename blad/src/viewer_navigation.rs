use crossterm::event::KeyCode;

use crate::links::Link;
use crate::render_writer::LinkSpan;
use crate::viewport::Document;

pub(crate) const HELP: &str = "\
# Blad keyboard help

/ forward, ? backward; Ctrl+R toggles regex in search
n repeat, N/p reverse; Esc cancels search
Tab / Shift+Tab: next / previous link; Enter: follow
Ctrl+Arrows: spatial links; [ / ]: previous / next heading
j/k or arrows: line; Space / PageDown / Ctrl+F: page down
PageUp / Ctrl+B: page up; d/u or Ctrl+D/U: half page
g/G or Home/End: top/bottom; Left/Right: horizontal
Alt+Left/Right: tabs; b: back; e: editor; r: reload
q/Esc: close tab; Q/Ctrl+C: quit all; Ctrl+L: redraw

Any key returns to the document. No live follow or less shell commands.
";

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
