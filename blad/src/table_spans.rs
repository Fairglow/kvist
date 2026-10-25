use std::collections::BTreeMap;
use std::io::{self, Write};
use std::ops::Range;

use crate::inline::Style;
use crate::render_writer::RenderWriter;
use crate::terminal_text::TerminalLine;

#[derive(Default)]
pub(crate) struct TableSpans {
    cells: BTreeMap<(usize, usize), Vec<CellSpan>>,
}

struct CellSpan {
    style: Style,
    bytes: Range<usize>,
}

pub(crate) struct WrappedSpan {
    style: Style,
    bytes: Range<usize>,
    columns: Range<usize>,
}

impl TableSpans {
    pub fn append(&mut self, cells: &mut [String], row: usize, style: Style, text: &str) {
        let column = cells.len().saturating_sub(1);
        if let Some(cell) = cells.last_mut() {
            let start = cell.len();
            cell.push_str(text);
            let style = Style {
                bold: style.bold,
                italic: style.italic,
                strike: style.strike,
                link: style.link,
                ..Style::default()
            };
            if text.is_empty() || style == Style::default() {
                return;
            }
            let spans = self.cells.entry((row, column)).or_default();
            if let Some(previous) = spans.last_mut()
                && previous.style == style
                && previous.bytes.end == start
            {
                previous.bytes.end = cell.len();
            } else {
                spans.push(CellSpan {
                    style,
                    bytes: start..cell.len(),
                });
            }
        }
    }

    pub fn wrapped(
        &self,
        row: usize,
        column: usize,
        original: &str,
        wrapped: &[String],
    ) -> io::Result<Vec<Vec<WrappedSpan>>> {
        let mut result: Vec<Vec<WrappedSpan>> = (0..wrapped.len()).map(|_| Vec::new()).collect();
        let Some(marks) = self.cells.get(&(row, column)) else {
            return Ok(result);
        };
        let mut source_cursor = 0;
        let mut mark_cursor = 0;
        for (line, output) in wrapped.iter().zip(&mut result) {
            let mut column = 0;
            let mut byte = 0;
            for word in line.split_whitespace() {
                // Cell wrapping preserves tokens and changes only whitespace.
                let offset = original[source_cursor..].find(word).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "table span token lost during wrapping",
                    )
                })?;
                let start = source_cursor + offset;
                let end = start + word.len();
                while marks
                    .get(mark_cursor)
                    .is_some_and(|mark| mark.bytes.end <= source_cursor)
                {
                    mark_cursor += 1;
                }
                if byte > 0
                    && let Some(mark) = marks
                        .get(mark_cursor)
                        .filter(|mark| mark.bytes.start <= source_cursor)
                {
                    push_span(output, mark.style, byte - 1..byte, column - 1..column);
                }
                while marks
                    .get(mark_cursor)
                    .is_some_and(|mark| mark.bytes.end <= start)
                {
                    mark_cursor += 1;
                }
                let length = word.len();
                let word = TerminalLine::new(word);
                let overlaps: Vec<_> = marks
                    .iter()
                    .skip(mark_cursor)
                    .take_while(|mark| mark.bytes.start < end)
                    .filter_map(|mark| {
                        let overlap = start.max(mark.bytes.start)..end.min(mark.bytes.end);
                        (overlap.start < overlap.end)
                            .then(|| (mark.style, overlap.start - start..overlap.end - start))
                    })
                    .collect();
                let ranges: Vec<_> = overlaps.iter().map(|(_, bytes)| bytes.clone()).collect();
                let columns = word.column_ranges(&ranges);
                if columns.len() != overlaps.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "table span has no wrapped grapheme",
                    ));
                }
                for ((style, bytes), range) in overlaps.into_iter().zip(columns) {
                    push_span(
                        output,
                        style,
                        byte + bytes.start..byte + bytes.end,
                        column + range.start..column + range.end,
                    );
                }
                column += word.width() + 1;
                byte += length + 1;
                source_cursor = end;
            }
        }

        Ok(result)
    }
}

fn push_span(
    output: &mut Vec<WrappedSpan>,
    style: Style,
    bytes: Range<usize>,
    columns: Range<usize>,
) {
    if let Some(previous) = output.last_mut()
        && previous.style == style
        && previous.bytes.end == bytes.start
    {
        previous.bytes.end = bytes.end;
        previous.columns.end = previous.columns.end.max(columns.end);
    } else {
        output.push(WrappedSpan {
            style,
            bytes,
            columns,
        });
    }
}

pub(crate) fn write_cell(
    writer: &mut RenderWriter<'_, impl Write>,
    text: &str,
    spans: &[WrappedSpan],
) -> io::Result<()> {
    for span in spans {
        if let Some(source) = span.style.link {
            writer.mark_link_columns(source, span.columns.start, span.columns.end);
        }
    }
    let mut cursor = 0;
    for span in spans {
        let plain = text
            .get(cursor..span.bytes.start)
            .ok_or_else(invalid_span)?;
        let styled = text.get(span.bytes.clone()).ok_or_else(invalid_span)?;
        write!(writer, "{plain}")?;
        write_emphasis(writer, span.style, true)?;
        write!(writer, "{styled}")?;
        write_emphasis(writer, span.style, false)?;
        cursor = span.bytes.end;
    }
    write!(writer, "{}", text.get(cursor..).ok_or_else(invalid_span)?)
}

fn write_emphasis(writer: &mut impl Write, style: Style, enabled: bool) -> io::Result<()> {
    for (active, on, off) in [
        (style.bold, 1, 22),
        (style.italic, 3, 23),
        (style.strike, 9, 29),
    ] {
        if active {
            write!(writer, "\x1b[{}m", if enabled { on } else { off })?;
        }
    }
    Ok(())
}

fn invalid_span() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid wrapped table span")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_spaces_keep_the_first_source_whitespace_style() {
        let mut spans = TableSpans::default();
        let mut cells = vec![String::new()];
        spans.append(
            &mut cells,
            0,
            Style {
                bold: true,
                reverse: true,
                color: Some(crate::Color::Ansi(1)),
                ..Style::default()
            },
            "a ",
        );
        spans.append(
            &mut cells,
            0,
            Style {
                italic: true,
                ..Style::default()
            },
            " b",
        );
        let wrapped = spans.wrapped(0, 0, &cells[0], &["a b".into()]).unwrap();
        assert_eq!(wrapped[0].len(), 2);
        assert_eq!(wrapped[0][0].bytes, 0..2);
        assert!(wrapped[0][0].style.bold);
        assert!(!wrapped[0][0].style.reverse);
        assert_eq!(wrapped[0][0].style.color, None);
        assert_eq!(wrapped[0][1].bytes, 2..3);
        assert!(wrapped[0][1].style.italic);
    }

    #[test]
    fn split_grapheme_styles_keep_safe_bytes_and_whole_link_columns() {
        let mut spans = TableSpans::default();
        let mut cells = vec![String::new()];
        for (text, bold) in [("e", true), ("\u{301}", false)] {
            spans.append(
                &mut cells,
                0,
                Style {
                    bold,
                    italic: !bold,
                    link: Some(crate::links::NavigationId::Source(0)),
                    ..Style::default()
                },
                text,
            );
        }
        let wrapped = spans.wrapped(0, 0, &cells[0], &[cells[0].clone()]).unwrap();
        assert_eq!(wrapped[0].len(), 2);
        assert_eq!(wrapped[0][0].bytes, 0..1);
        assert_eq!(wrapped[0][1].bytes, 1..3);
        assert!(wrapped[0].iter().all(|span| span.columns == (0..1)));
    }

    #[test]
    fn projection_and_slice_failures_are_explicit() {
        let mut spans = TableSpans::default();
        let mut cells = vec![String::new()];
        spans.append(
            &mut cells,
            0,
            Style {
                bold: true,
                ..Style::default()
            },
            "source",
        );
        assert_eq!(
            spans
                .wrapped(0, 0, &cells[0], &["missing".into()])
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let mut output = Vec::new();
        let mut writer = RenderWriter::new(&mut output, None, &crate::RenderOptions::plain());
        let invalid = [WrappedSpan {
            style: Style::default(),
            bytes: 0..100,
            columns: 0..100,
        }];
        assert_eq!(
            write_cell(&mut writer, "short", &invalid)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
