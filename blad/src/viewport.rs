use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate},
};

use crate::RenderOptions;
use crate::markdown::MarkdownRenderer;
use crate::terminal_text::TerminalLine;

pub(crate) struct Document {
    pub navigation: crate::render_writer::Navigation,
    pub lines: Vec<TerminalLine>,
    pub content_width: usize,
    pub gutter: usize,
    max_width: usize,
    chrome: String,
    search: String,
    selection: String,
}

impl Document {
    #[cfg(test)]
    pub fn render(
        source: &str,
        columns: u16,
        numbers: bool,
        width_limit: Option<usize>,
    ) -> io::Result<Self> {
        Self::render_with_options(
            source,
            columns,
            numbers,
            width_limit,
            &RenderOptions::default(),
        )
    }

    pub fn render_with_options(
        source: &str,
        columns: u16,
        numbers: bool,
        width_limit: Option<usize>,
        options: &RenderOptions,
    ) -> io::Result<Self> {
        let available = usize::from(columns.saturating_sub(1));
        let mut gutter = if numbers { 2 } else { 0 };
        loop {
            let content_width = width_limit.map_or(available.saturating_sub(gutter), |limit| {
                limit.min(available.saturating_sub(gutter))
            });
            let mut output = Vec::new();
            let mut renderer = MarkdownRenderer::with_options(options.clone());
            renderer.set_width(Some(content_width));
            let navigation = renderer.render_navigation(source, &mut output)?;
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
                    navigation,
                    lines,
                    content_width,
                    gutter,
                    max_width,
                    chrome: Self::style(options, options.palette().chrome, None)?,
                    search: Self::style(options, options.palette().search, Some(false))?,
                    selection: Self::style(options, options.palette().search, Some(true))?,
                });
            }
            gutter = required;
        }
    }

    fn style(
        options: &RenderOptions,
        color: crate::Color,
        reverse: Option<bool>,
    ) -> io::Result<String> {
        let mut bytes = Vec::new();
        let mut output = crate::output::Output::new(&mut bytes, options);
        color.write(&mut output, false)?;
        if let Some(reverse) = reverse {
            write!(output, "\x1b[{}m\x1b[4m", if reverse { 7 } else { 27 })?;
        }
        output.flush()?;
        String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn max_x(&self) -> usize {
        self.max_width.saturating_sub(self.content_width)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Frame<'a> {
    pub columns: u16,
    pub rows: u16,
    pub y: usize,
    pub x: usize,
    pub header: &'a str,
    pub footer: &'a str,
    pub matches: &'a [crate::render_writer::LinkSpan],
    pub active: &'a [crate::render_writer::LinkSpan],
}

#[derive(Default)]
pub(crate) struct Painter {
    previous: Vec<String>,
    size: Option<(u16, u16)>,
}

impl Painter {
    pub fn invalidate(&mut self) {
        self.size = None;
    }

    pub fn paint(
        &mut self,
        w: &mut impl Write,
        document: &Document,
        frame: Frame<'_>,
    ) -> io::Result<()> {
        let size = (frame.columns, frame.rows);
        let rows = compose(document, &frame);
        let mut output = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            if self.size != Some(size) || self.previous.get(index) != Some(row) {
                if output.is_empty() {
                    queue!(output, BeginSynchronizedUpdate)?;
                }
                queue!(output, MoveTo(0, index as u16))?;
                write!(output, "{row}")?;
            }
        }
        if !output.is_empty() {
            queue!(output, EndSynchronizedUpdate)?;
            w.write_all(&output)?;
            w.flush()?;
        }
        self.previous = rows;
        self.size = Some(size);
        Ok(())
    }
}

fn pad(row: &mut String, width: usize) {
    let used = TerminalLine::new(row).width();
    row.push_str("\x1b[0m");
    row.extend(std::iter::repeat_n(' ', width.saturating_sub(used)));
}

fn compose(document: &Document, frame: &Frame<'_>) -> Vec<String> {
    if frame.columns == 0 || frame.rows == 0 {
        return Vec::new();
    }
    let mut rows = vec![String::new(); usize::from(frame.rows)];
    rows[0] = format!(
        "{}{}",
        document.chrome,
        TerminalLine::new(&crate::markdown::inert_source(frame.header))
            .clip(0, usize::from(frame.columns))
    );
    let height = usize::from(frame.rows.saturating_sub(2));
    let mut match_index = frame.matches.partition_point(|span| span.row < frame.y);
    let mut active_index = frame.active.partition_point(|span| span.row < frame.y);
    for (index, line) in document.lines.iter().enumerate().skip(frame.y).take(height) {
        let row = &mut rows[index - frame.y + 1];
        if document.gutter > 0 {
            let prefix = format!("{:>width$} ", index + 1, width = document.gutter - 1);
            row.push_str(
                &TerminalLine::new(&prefix).clip(0, usize::from(frame.columns.saturating_sub(1))),
            );
        }
        let ranges = row_ranges(frame.matches, &mut match_index, index);
        let active = row_ranges(frame.active, &mut active_index, index);
        row.push_str(&line.clip_with_overlays(
            frame.x,
            document.content_width,
            &ranges,
            &document.search,
            &active,
            &document.selection,
        ));
    }
    if frame.rows > 1 {
        rows[usize::from(frame.rows - 1)] = format!(
            "{}{}",
            document.chrome,
            TerminalLine::new(&crate::markdown::inert_source(frame.footer))
                .clip(0, usize::from(frame.columns))
        );
    }
    for row in &mut rows {
        pad(row, usize::from(frame.columns));
    }
    if height > 0 && document.lines.len() > height {
        let thumb = (height * height / document.lines.len()).max(1).min(height);
        let y = frame.y.min(document.lines.len() - height);
        let top = y * (height - thumb) / (document.lines.len() - height);
        for index in 0..height {
            let row = &mut rows[index + 1];
            // The final column was reserved for the scrollbar.
            row.pop();
            row.push(if (top..top + thumb).contains(&index) {
                '█'
            } else {
                '░'
            });
        }
    }
    rows
}

fn row_ranges(
    spans: &[crate::render_writer::LinkSpan],
    index: &mut usize,
    row: usize,
) -> Vec<std::ops::Range<usize>> {
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    while let Some(span) = spans.get(*index).filter(|span| span.row == row) {
        if let Some(last) = ranges.last_mut().filter(|last| last.end >= span.start) {
            last.end = last.end.max(span.end);
        } else {
            ranges.push(span.start..span.end);
        }
        *index += 1;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(w: &mut impl Write, document: &Document, frame: Frame<'_>) -> io::Result<()> {
        Painter::default().paint(w, document, frame)
    }

    #[test]
    fn code_tabs_do_not_create_padding_only_horizontal_overflow() {
        for syntax in ["text", "rust"] {
            for numbers in [false, true] {
                for plain in [false, true] {
                    let source = format!("```{syntax}\n\tX\n\tX\tY\n```");
                    let options = RenderOptions {
                        plain,
                        ..RenderOptions::default()
                    };
                    let document =
                        Document::render_with_options(&source, 20, numbers, None, &options)
                            .unwrap();
                    assert_eq!(
                        document.max_x(),
                        0,
                        "{syntax} numbers={numbers} plain={plain}"
                    );
                    assert!(
                        document
                            .lines
                            .iter()
                            .all(|line| line.width() <= document.content_width)
                    );
                }
            }
        }
    }

    #[test]
    fn configured_chrome_search_and_reflow_keep_host_policy() {
        let palette = crate::Palette {
            chrome: crate::Color::Rgb(1, 2, 3),
            search: crate::Color::Rgb(4, 5, 6),
            ..crate::Palette::default()
        };
        let options = RenderOptions {
            palette: Some(palette),
            ..RenderOptions::default()
        };
        for columns in [12, 80] {
            let document =
                Document::render_with_options("# Match", columns, false, None, &options).unwrap();
            let mut output = Vec::new();
            paint(
                &mut output,
                &document,
                Frame {
                    columns,
                    rows: 5,
                    y: 0,
                    x: 0,
                    header: "title",
                    footer: "footer",
                    matches: &[],
                    active: &[crate::render_writer::LinkSpan {
                        row: 0,
                        start: 2,
                        end: 7,
                    }],
                },
            )
            .unwrap();
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains("\x1b[38;2;1;2;3m"), "{output:?}");
            let heading = output.find("Match").unwrap();
            let prior = &output[..heading];
            assert!(
                prior.rfind("\x1b[38;2;4;5;6m") > prior.rfind("\x1b[34m"),
                "{output:?}"
            );
            assert!(
                prior.rfind("\x1b[7m") > prior.rfind("\x1b[0m"),
                "{output:?}"
            );
        }
    }

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
            active: &[],
        };
        assert_eq!(
            paint(&mut Failed, &document, frame).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn duplicate_table_and_bare_links_have_source_owned_spans() {
        let source = "same\n\n| first | second |\n| --- | --- |\n| [same](#a) | [same](#b) |\n\nhttps://example.test https://example.test";
        let links = crate::links::extract_links(source);
        let document = Document::render(source, 80, false, None).unwrap();
        assert_eq!(links.len(), 4);
        for link in &links {
            let spans = document
                .navigation
                .links
                .get(&link.navigation_id())
                .unwrap();
            assert!(!spans.is_empty());
            assert!(spans.iter().all(|span| span.row > 0));
        }
        let first = &document.navigation.links[&links[0].navigation_id()][0];
        let second = &document.navigation.links[&links[1].navigation_id()][0];
        assert_eq!(first.row, second.row);
        assert!(first.end < second.start);
    }

    #[test]
    fn painter_suppresses_identical_frames_and_invalidates_after_failure_or_refresh() {
        let document = Document::render("long text\n\nsecond row", 20, false, None).unwrap();
        let frame = Frame {
            columns: 20,
            rows: 8,
            y: 0,
            x: 0,
            header: "header",
            footer: "footer",
            matches: &[],
            active: &[],
        };
        let mut painter = Painter::default();
        let mut output = Vec::new();
        painter.paint(&mut output, &document, frame).unwrap();
        assert!(!output.is_empty());
        output.clear();
        painter.paint(&mut output, &document, frame).unwrap();
        assert!(output.is_empty());
        painter.invalidate();
        painter.paint(&mut output, &document, frame).unwrap();
        assert!(String::from_utf8(output).unwrap().contains("header"));

        struct FlushFailure(Vec<u8>);
        impl Write for FlushFailure {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
        }
        painter.invalidate();
        let mut failure = FlushFailure(Vec::new());
        assert!(painter.paint(&mut failure, &document, frame).is_err());
        let mut retry = Vec::new();
        painter.paint(&mut retry, &document, frame).unwrap();
        assert!(!retry.is_empty());
        assert!(!String::from_utf8(retry).unwrap().contains("\x1b[2J"));
    }

    #[test]
    fn projected_text_and_math_labels_keep_distinct_navigation_identities() {
        let source = format!(
            "{} https://example.test [next](#next)\n\n[$x^2$](#math)",
            "~q~".repeat(40)
        );
        let document = Document::render(&source, 80, false, None).unwrap();
        let links = crate::links::extract_links(&source);
        assert_eq!(links.len(), 3);
        assert_eq!(document.navigation.links.len(), 3);
        for link in links {
            assert!(!document.navigation.links[&link.navigation_id()].is_empty());
        }
    }

    #[test]
    fn footnote_rows_restore_prose_color_after_link_and_search_overlays_on_reflow() {
        let source = "intro[^1]\n\n[^1]: muted words [linked label](#target) restored prose with enough words to wrap\n\nordinary\n\n# target";
        let link = crate::links::extract_links(source).remove(0);
        for columns in [14, 30, 80] {
            for numbers in [false, true] {
                let document = Document::render(source, columns, numbers, None).unwrap();
                let spans = &document.navigation.links[&link.navigation_id()];
                let labels: Vec<_> = spans
                    .iter()
                    .map(|span| {
                        let clipped =
                            document.lines[span.row].clip(span.start, span.end - span.start);
                        assert!(clipped.contains("\x1b[96m"), "{clipped:?}");
                        TerminalLine::new(&clipped).plain().to_owned()
                    })
                    .collect();
                assert_eq!(labels.join(" "), "linked label");
                let mut footnote_rows = 0;
                for line in document.lines.iter().skip(2) {
                    if line.plain() == "ordinary" {
                        let clipped = line.clip(0, document.content_width);
                        assert!(!clipped.contains("\x1b[90m"), "{clipped:?}");
                        break;
                    }
                    for word in line
                        .plain()
                        .split_whitespace()
                        .filter(|word| !matches!(*word, "linked" | "label"))
                    {
                        let offset = line.plain().find(word).unwrap();
                        let start = unicode_width::UnicodeWidthStr::width(&line.plain()[..offset]);
                        let width = unicode_width::UnicodeWidthStr::width(word);
                        let range = start..start + width;
                        let highlighted = line.clip_with_spans(
                            start,
                            width,
                            std::slice::from_ref(&range),
                            "\x1b[7m\x1b[4m",
                        );
                        let position = highlighted.find(word).unwrap();
                        let prefix = &highlighted[..position];
                        assert!(
                            prefix.rfind("\x1b[90m") > prefix.rfind("\x1b[0m"),
                            "{columns} columns, numbers {numbers}: {highlighted:?}"
                        );
                    }
                    if !line.plain().is_empty() {
                        footnote_rows += 1;
                    }
                }
                assert!(footnote_rows > 0);
                if columns == 14 {
                    assert!(footnote_rows > 1);
                }
            }
        }
    }

    #[test]
    fn wrapped_unicode_table_links_are_precise_in_quotes_and_after_reflow() {
        let source = "> | label | other |\n> | --- | --- |\n> | prefix [界 wide label **words**](#target) tail | x |\n\nE=mc^2^ https://example.test";
        let links = crate::links::extract_links(source);
        for columns in [25, 80] {
            let document = Document::render(source, columns, false, None).unwrap();
            for link in &links {
                let spans = &document.navigation.links[&link.navigation_id()];
                let parts: Vec<_> = spans
                    .iter()
                    .map(|span| {
                        TerminalLine::new(
                            &document.lines[span.row].clip(span.start, span.end - span.start),
                        )
                        .plain()
                        .to_owned()
                    })
                    .collect();
                let joined = parts.join(" ");
                assert!(
                    joined == "界 wide label words" || joined == "https://example.test",
                    "{joined}"
                );
            }
        }
    }

    #[test]
    fn frontmatter_excludes_navigation_but_preserves_body_spans_after_reflow() {
        let source = "---\n# Duplicate\nurl: https://metadata.test\nlink: '[hidden](#duplicate)'\n---\n\n# Duplicate\n\n[界 body link](#duplicate) https://body.test";
        let controlled = format!(
            "\u{feff}{}",
            source
                .replace("url:", "control: \x1b[2J\u{202e}\nurl:")
                .replace('\n', "\r\n")
        );
        for source in [source, controlled.as_str()] {
            let links = crate::links::extract_links(&crate::terminal_text::inert_source(source));
            assert_eq!(links.len(), 2);
            for columns in [20, 40, 80] {
                for numbers in [false, true] {
                    let document = Document::render(source, columns, numbers, None).unwrap();
                    assert_eq!(document.navigation.anchors.len(), 1);
                    let heading_row = document.navigation.anchors["duplicate"];
                    assert!(
                        document.lines[heading_row]
                            .plain()
                            .starts_with("# Duplicate")
                    );
                    assert!(heading_row > 5);
                    assert_eq!(document.navigation.links.len(), 2);
                    for link in &links {
                        let spans = &document.navigation.links[&link.navigation_id()];
                        let text: Vec<_> = spans
                            .iter()
                            .map(|span| {
                                assert!(span.row > heading_row);
                                TerminalLine::new(
                                    &document.lines[span.row]
                                        .clip(span.start, span.end - span.start),
                                )
                                .plain()
                                .to_owned()
                            })
                            .collect();
                        assert_eq!(
                            if link.is_url() {
                                text.concat()
                            } else {
                                text.join(" ")
                            },
                            link.text
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn aligned_table_link_spans_exclude_padding_after_numbered_quoted_reflow() {
        let source = "> | [界](#header-center) | [e\u{301}](#header-right) |\n> | :---: | ---: |\n> | prefix [界 wide label **words**](#target) tail | [短](#short) |\n> | longer heading value | longest second column value |";
        let links = crate::links::extract_links(source);
        let labels = ["界", "e\u{301}", "界 wide label words", "短"];
        for columns in [25, 40, 80] {
            for numbers in [false, true] {
                let document = Document::render(source, columns, numbers, None).unwrap();
                for (link, label) in links.iter().zip(labels) {
                    let spans = &document.navigation.links[&link.navigation_id()];
                    assert!(!spans.is_empty());
                    let parts: Vec<_> = spans
                        .iter()
                        .map(|span| {
                            let text = TerminalLine::new(
                                &document.lines[span.row].clip(span.start, span.end - span.start),
                            )
                            .plain()
                            .to_owned();
                            assert_eq!(text, text.trim(), "padding in link span: {text:?}");
                            assert!(span.start >= 2, "quote prefix included");
                            text
                        })
                        .collect();
                    assert_eq!(
                        parts.join(" "),
                        label,
                        "{columns} columns, numbers {numbers}"
                    );
                }
            }
        }
    }
}
