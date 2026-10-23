use std::collections::BTreeMap;
use std::io;
use std::ops::Range;

use crate::links::NavigationId;
use crate::terminal_text::TerminalLine;

#[derive(Default)]
pub(crate) struct TableLinks {
    cells: BTreeMap<(usize, usize), Vec<CellLink>>,
}

#[derive(Clone)]
pub(crate) struct CellLink {
    pub source: NavigationId,
    pub range: Range<usize>,
}

impl TableLinks {
    pub fn append(
        &mut self,
        cells: &mut [String],
        row: usize,
        source: Option<NavigationId>,
        text: &str,
    ) {
        let column = cells.len().saturating_sub(1);
        if let Some(cell) = cells.last_mut() {
            if let Some(source) = source {
                self.cells.entry((row, column)).or_default().push(CellLink {
                    source,
                    range: cell.len()..cell.len() + text.len(),
                });
            }
            cell.push_str(text);
        }
    }

    pub fn wrapped(
        &self,
        row: usize,
        column: usize,
        original: &str,
        wrapped: &[String],
    ) -> io::Result<Vec<Vec<CellLink>>> {
        let mut result = vec![Vec::new(); wrapped.len()];
        let Some(marks) = self.cells.get(&(row, column)) else {
            return Ok(result);
        };
        let mut source_cursor = 0;
        let mut mark_cursor = 0;
        for (line, output) in wrapped.iter().zip(&mut result) {
            let mut column = 0;
            for word in line.split_whitespace() {
                // Cell wrapping preserves tokens and changes only whitespace.
                let offset = original[source_cursor..].find(word).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "table link token lost during wrapping",
                    )
                })?;
                let start = source_cursor + offset;
                let end = start + word.len();
                while marks
                    .get(mark_cursor)
                    .is_some_and(|mark| mark.range.end <= start)
                {
                    mark_cursor += 1;
                }
                let word = TerminalLine::new(word);
                for mark in marks
                    .iter()
                    .skip(mark_cursor)
                    .take_while(|mark| mark.range.start < end)
                {
                    let overlap = start.max(mark.range.start)..end.min(mark.range.end);
                    if overlap.start < overlap.end {
                        let bytes = overlap.start - start..overlap.end - start;
                        for range in word.column_ranges(std::slice::from_ref(&bytes)) {
                            output.push(CellLink {
                                source: mark.source,
                                range: column + range.start..column + range.end,
                            });
                        }
                    }
                }
                column += word.width() + 1;
                source_cursor = end;
            }
        }
        Ok(result)
    }
}
