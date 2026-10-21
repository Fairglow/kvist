use std::io::{self, Write};
use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub reverse: bool,
    pub dim: bool,
    pub color: Option<crate::theme::Color>,
}

impl Style {
    fn write(self, w: &mut impl Write, text: &str) -> io::Result<()> {
        if self == Self::default() {
            return write!(w, "{text}");
        }
        write!(w, "\x1b[0m")?;
        for (enabled, code) in [
            (self.bold, 1),
            (self.italic, 3),
            (self.strike, 9),
            (self.reverse, 7),
            (self.dim, 2),
        ] {
            if enabled {
                write!(w, "\x1b[{code}m")?;
            }
        }
        if let Some(color) = self.color {
            color.write(w, false)?;
        }
        write!(w, "{text}\x1b[0m")
    }
}

#[derive(Default)]
struct Word {
    text: String,
    spans: Vec<(Range<usize>, Style)>,
    atomic: bool,
}

impl Word {
    fn push(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        if let Some((range, previous)) = self.spans.last_mut()
            && *previous == style
        {
            range.end = self.text.len();
        } else {
            self.spans.push((start..self.text.len(), style));
        }
    }

    fn write_part(
        &self,
        w: &mut impl Write,
        range: Range<usize>,
        span_index: &mut usize,
    ) -> io::Result<()> {
        while let Some((span, style)) = self.spans.get(*span_index) {
            let start = span.start.max(range.start);
            let end = span.end.min(range.end);
            if start < end {
                style.write(w, &self.text[start..end])?;
            }
            if span.end > range.end {
                break;
            }
            *span_index += 1;
        }
        Ok(())
    }
}

pub(crate) struct InlineWriter {
    width: Option<usize>,
    column: usize,
    indent: usize,
    has_text: bool,
    pending_space: bool,
    word: Word,
}

impl InlineWriter {
    pub fn new(width: Option<usize>) -> Self {
        Self {
            width: width.map(|width| width.max(1)),
            column: 0,
            indent: 0,
            has_text: false,
            pending_space: false,
            word: Word::default(),
        }
    }

    pub fn reset(&mut self, column: usize, indent: usize) {
        self.column = column;
        self.indent = self.width.map_or(indent, |width| indent.min(width - 1));
        self.has_text = false;
        self.pending_space = false;
    }

    pub fn has_prefix(&self) -> bool {
        self.column > 0
    }

    pub fn has_content(&self) -> bool {
        self.has_text || !self.word.text.is_empty()
    }

    pub fn set_indent(&mut self, indent: usize) {
        self.indent = self.width.map_or(indent, |width| indent.min(width - 1));
    }

    pub fn text(&mut self, w: &mut impl Write, text: &str, style: Style) -> io::Result<()> {
        for piece in text.split_inclusive([' ', '\t', '\r', '\n']) {
            let body = piece.trim_end_matches([' ', '\t', '\r', '\n']);
            self.word.push(body, style);
            if body.len() != piece.len() {
                self.soft_break(w)?;
            }
        }
        Ok(())
    }

    pub fn code(&mut self, text: &str, style: Style) {
        self.word.push(text, style);
        self.word.atomic = true;
    }

    pub fn soft_break(&mut self, w: &mut impl Write) -> io::Result<()> {
        self.flush_word(w)?;
        self.pending_space = true;
        Ok(())
    }

    pub fn finish(&mut self, w: &mut impl Write) -> io::Result<()> {
        self.flush_word(w)?;
        self.pending_space = false;
        Ok(())
    }

    pub fn hard_break(&mut self, w: &mut impl Write) -> io::Result<()> {
        self.finish(w)?;
        self.newline(w)
    }

    fn newline(&mut self, w: &mut impl Write) -> io::Result<()> {
        writeln!(w)?;
        self.column = 0;
        self.has_text = false;
        self.pending_space = false;
        Ok(())
    }

    fn write_indent(&mut self, w: &mut impl Write) -> io::Result<()> {
        if self.column == 0 {
            write!(w, "{}", " ".repeat(self.indent))?;
            self.column = self.indent;
        }
        Ok(())
    }

    fn flush_word(&mut self, w: &mut impl Write) -> io::Result<()> {
        if self.word.text.is_empty() {
            return Ok(());
        }
        let word_width = self.word.text.width();
        let separator = usize::from(self.pending_space && self.has_text);
        if let Some(width) = self.width
            && self.has_text
            && self
                .column
                .saturating_add(separator)
                .saturating_add(word_width)
                > width
        {
            self.newline(w)?;
        }
        self.write_indent(w)?;
        if self.pending_space && self.has_text {
            write!(w, " ")?;
            self.column += 1;
        }
        self.pending_space = false;

        let mut span_index = 0;
        if let Some(width) = self.width
            && !self.word.atomic
            && self.column.saturating_add(word_width) > width
        {
            let mut start = 0;
            // One span cursor avoids rescanning all styles for each wrapped slice.
            let boundaries: Vec<(usize, usize)> = self
                .word
                .text
                .grapheme_indices(true)
                .map(|(offset, grapheme)| (offset, grapheme.width()))
                .collect();
            for (offset, grapheme_width) in boundaries {
                if self.has_text && self.column.saturating_add(grapheme_width) > width {
                    self.word.write_part(w, start..offset, &mut span_index)?;
                    self.newline(w)?;
                    self.write_indent(w)?;
                    start = offset;
                }
                self.column += grapheme_width;
                self.has_text = true;
            }
            self.word
                .write_part(w, start..self.word.text.len(), &mut span_index)?;
        } else {
            self.word
                .write_part(w, 0..self.word.text.len(), &mut span_index)?;
            self.column += word_width;
            self.has_text = true;
        }
        self.word.text.clear();
        self.word.spans.clear();
        self.word.atomic = false;
        Ok(())
    }
}
