use std::collections::BTreeMap;
use std::io::{self, Write};

use crate::{Color, RenderOptions};

#[derive(Default)]
pub(crate) struct Navigation {
    pub links: BTreeMap<usize, Vec<LinkSpan>>,
    pub anchors: BTreeMap<String, usize>,
}

pub(crate) struct LinkSpan {
    pub row: usize,
    pub start: usize,
    pub end: usize,
}

pub(crate) trait InlineOutput: Write {
    fn mark_link(&mut self, source: usize, text: &str);
}

pub(crate) struct RenderWriter<'a, W> {
    output: &'a mut W,
    line: Vec<u8>,
    row: usize,
    quotes: Vec<Color>,
    width: Option<usize>,
    background: Option<Color>,
    pub navigation: Navigation,
}

impl<'a, W: Write> RenderWriter<'a, W> {
    pub fn new(output: &'a mut W, width: Option<usize>, options: &RenderOptions) -> Self {
        Self {
            output,
            line: Vec::new(),
            row: 0,
            quotes: Vec::new(),
            width,
            background: options.palette().quote_background,
            navigation: Navigation::default(),
        }
    }

    pub fn push_quote(&mut self, color: Color) {
        self.quotes.push(color);
    }

    pub fn pop_quote(&mut self) {
        self.quotes.pop();
    }

    pub fn inner_width(&self) -> Option<usize> {
        self.width
            .map(|width| width.saturating_sub(self.border_width()).max(1))
    }

    fn border_width(&self) -> usize {
        let desired = self.quotes.len().saturating_mul(2);
        self.width.map_or(desired, |width| {
            desired.min(width.saturating_sub(1) / 2 * 2)
        })
    }

    pub fn mark_anchor(&mut self, anchor: String) {
        self.navigation.anchors.entry(anchor).or_insert(self.row);
    }

    fn emit(&mut self, newline: bool) -> io::Result<()> {
        let text = std::str::from_utf8(&self.line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if self.quotes.is_empty() {
            self.output.write_all(&self.line)?;
        } else {
            let background = self
                .background
                .map(|color| color.sequence(true))
                .transpose()?
                .unwrap_or_default();
            write!(self.output, "{background}")?;
            for color in self.quotes.iter().take(self.border_width() / 2) {
                color.write(self.output, false)?;
                write!(self.output, "│ \x1b[0m{background}")?;
            }
            write!(
                self.output,
                "{}",
                text.replace("\x1b[0m", &format!("\x1b[0m{background}"))
            )?;
            let width = crate::terminal_text::TerminalLine::new(text).width() + self.border_width();
            if let Some(columns) = self.width {
                write!(self.output, "{}", " ".repeat(columns.saturating_sub(width)))?;
            }
            write!(self.output, "\x1b[0m")?;
        }
        self.line.clear();
        if newline {
            writeln!(self.output)?;
            self.row += 1;
        }
        Ok(())
    }
}

impl<W: Write> InlineOutput for RenderWriter<'_, W> {
    fn mark_link(&mut self, source: usize, text: &str) {
        let start = crate::terminal_text::TerminalLine::new(&String::from_utf8_lossy(&self.line))
            .width()
            + self.border_width();
        let end = start + crate::terminal_text::TerminalLine::new(text).width();
        let spans = self.navigation.links.entry(source).or_default();
        if let Some(span) = spans.last_mut().filter(|span| span.row == self.row) {
            span.end = span.end.max(end);
        } else {
            spans.push(LinkSpan {
                row: self.row,
                start,
                end,
            });
        }
    }
}

impl<W: Write> Write for RenderWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for part in bytes.split_inclusive(|byte| *byte == b'\n') {
            let newline = part.last() == Some(&b'\n');
            self.line.extend_from_slice(if newline {
                &part[..part.len() - 1]
            } else {
                part
            });
            if newline {
                self.emit(true)?;
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.line.is_empty() {
            self.emit(false)?;
        }
        self.output.flush()
    }
}
