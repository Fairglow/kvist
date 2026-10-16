use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    terminal::{Clear, ClearType},
};

use crate::markdown::render_markdown_width;
use crate::terminal_text::TerminalLine;

pub(crate) struct Document {
    pub lines: Vec<TerminalLine>,
    pub content_width: usize,
    pub gutter: usize,
    max_width: usize,
}

impl Document {
    pub fn render(
        source: &str,
        columns: u16,
        numbers: bool,
        width_limit: Option<usize>,
    ) -> io::Result<Self> {
        let available = usize::from(columns.saturating_sub(1));
        let mut gutter = if numbers { 2 } else { 0 };
        loop {
            let content_width = width_limit.map_or(available.saturating_sub(gutter), |limit| {
                limit.min(available.saturating_sub(gutter))
            });
            let mut output = Vec::new();
            render_markdown_width(source, &mut output, Some(content_width), None)?;
            let rendered = String::from_utf8(output)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let lines: Vec<_> = rendered.lines().map(TerminalLine::new).collect();
            let required = if numbers {
                lines.len().to_string().len() + 1
            } else {
                0
            };
            if required <= gutter {
                let max_width = lines.iter().map(TerminalLine::width).max().unwrap_or(0);
                return Ok(Self {
                    lines,
                    content_width,
                    gutter,
                    max_width,
                });
            }
            gutter = required;
        }
    }

    pub fn max_x(&self) -> usize {
        self.max_width.saturating_sub(self.content_width)
    }
}

pub(crate) struct Frame<'a> {
    pub columns: u16,
    pub rows: u16,
    pub y: usize,
    pub x: usize,
    pub header: &'a str,
    pub footer: &'a str,
    pub matches: &'a [usize],
}

pub(crate) fn paint(w: &mut impl Write, document: &Document, frame: Frame<'_>) -> io::Result<()> {
    queue!(w, Clear(ClearType::All))?;
    if frame.columns == 0 || frame.rows == 0 {
        return w.flush();
    }
    queue!(w, MoveTo(0, 0))?;
    write!(
        w,
        "{}",
        TerminalLine::new(frame.header).clip(0, usize::from(frame.columns))
    )?;
    let height = usize::from(frame.rows.saturating_sub(2));
    for (index, line) in document.lines.iter().enumerate().skip(frame.y).take(height) {
        let row = (index - frame.y + 1) as u16;
        queue!(w, MoveTo(0, row))?;
        if document.gutter > 0 {
            let prefix = format!("{:>width$} ", index + 1, width = document.gutter - 1);
            write!(
                w,
                "{}",
                TerminalLine::new(&prefix).clip(0, usize::from(frame.columns.saturating_sub(1)))
            )?;
        }
        if frame.matches.contains(&index) {
            write!(w, "\x1b[33;44m")?;
        }
        write!(w, "{}", line.clip(frame.x, document.content_width))?;
    }
    if frame.rows > 1 {
        queue!(w, MoveTo(0, frame.rows - 1))?;
        write!(
            w,
            "{}",
            TerminalLine::new(frame.footer).clip(0, usize::from(frame.columns))
        )?;
    }
    if height > 0 && document.lines.len() > height {
        let thumb = (height * height / document.lines.len()).max(1).min(height);
        let top = frame.y * (height - thumb) / (document.lines.len() - height);
        for index in 0..height {
            queue!(w, MoveTo(frame.columns - 1, (index + 1) as u16))?;
            write!(
                w,
                "{}",
                if (top..top + thumb).contains(&index) {
                    '█'
                } else {
                    '░'
                }
            )?;
        }
    }
    write!(w, "\x1b[0m")?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gutter_and_width_limit_are_reserved_before_wrapping() {
        let source = "123456789 123456789 123456789\n\n".repeat(100);
        let document = Document::render(&source, 30, true, None).unwrap();
        assert_eq!(document.gutter, 4);
        assert_eq!(document.content_width, 25);
        assert!(document.lines.iter().all(|line| line.width() <= 25));
        let limited = Document::render(&source, 30, false, Some(10)).unwrap();
        assert_eq!(limited.content_width, 10);
        assert!(limited.lines.iter().all(|line| line.width() <= 10));
    }

    #[test]
    fn writer_failures_are_propagated() {
        let document = Document::render("text", 20, false, None).unwrap();
        struct Failed;
        impl Write for Failed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let frame = Frame {
            columns: 20,
            rows: 5,
            y: 0,
            x: 0,
            header: "title",
            footer: "footer",
            matches: &[],
        };
        assert_eq!(
            paint(&mut Failed, &document, frame).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}
