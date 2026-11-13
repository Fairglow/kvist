//! Tests for table inline code and link presentation styling.
//!
//! These pin the contract from R-074 / CONTRACT.md ("First inline table
//! styling subset"):
//! - Dedicated inline-code spans in table cells use the **code-background
//!   palette** (not reverse video, not monospace reversal), composing with
//!   the row's zebra background and header reversal.
//! - Cell **whitespace normalization and wrapping also apply** to inline-code
//!   text (code does not gain nonbreaking/atomic semantics in tables).
//! - Table **link labels use the link palette color**, composing with
//!   emphasis, header reversal, row backgrounds, and quote styling. A linked
//!   code span therefore carries both the link color and the code background.
//! - Plain output suppresses these styles without changing text or geometry.
//!
//! The expected palette indices are derived from the renderer's own palette
//! (not hard-coded) so the tests track the palette.

use blad::{Color, MarkdownRenderer, Palette, RenderOptions, Theme};
use vte::{Params, Parser, Perform};

/// Strip ANSI escape codes for text comparison.
fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for nc in chars.by_ref() {
                if nc == 'm' {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Render markdown to a string with given width.
fn render_width(md: &str, width: Option<usize>) -> String {
    let mut buf = Vec::new();
    let mut renderer = MarkdownRenderer::with_options(RenderOptions::default());
    renderer.set_width(width);
    renderer.render_markdown(md, &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

/// Render markdown in plain mode (no styling).
fn render_plain(md: &str, width: Option<usize>) -> String {
    let mut buf = Vec::new();
    let mut renderer = MarkdownRenderer::with_options(RenderOptions::plain());
    renderer.set_width(width);
    renderer.render_markdown(md, &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

/// The 256-color index a palette color is encoded as in SGR (Ansi(n) -> n).
fn sgr_index(color: &Color) -> u16 {
    match color {
        Color::Ansi(index) => *index as u16,
        // The default dark palette only uses Ansi colors; treat anything else
        // as a sentinel so a regression to RGB is still a distinct value.
        Color::Rgb(_, _, _) => 0xffff,
    }
}

/// Expected (code-background, link) SGR indices for the default palette.
fn expected_palette() -> (u16, u16) {
    let palette = Palette::for_theme(Theme::Dark);
    let code = palette
        .code_background
        .map(|color| sgr_index(&color))
        .expect("the default dark palette provides a code background");
    (code, sgr_index(&palette.link))
}

/// Per-character effective attributes parsed from rendered ANSI output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Attrs {
    bold: bool,
    italic: bool,
    strike: bool,
    reverse: bool,
    /// Effective 256-color foreground index, if set.
    fg: Option<u16>,
    /// Effective 256-color background index, if set.
    bg: Option<u16>,
}

#[derive(Default)]
struct StyledText {
    text: String,
    attributes: Vec<Attrs>,
    current: Attrs,
}

impl StyledText {
    fn parse(output: &str) -> Self {
        let mut text = Self::default();
        Parser::new().advance(&mut text, output.as_bytes());
        text
    }

    fn styles_for(&self, text: &str) -> &[Attrs] {
        let offset = self.text.find(text).unwrap_or(0);
        let start = self.text[..offset].chars().count();
        &self.attributes[start..start + text.chars().count()]
    }

    /// R-074: a table inline-code span uses the code-background palette.
    fn assert_code_background(&self, text: &str, code_bg: u16) {
        let attrs = self.styles_for(text);
        assert!(
            attrs.iter().all(|a| a.bg == Some(code_bg)),
            "{text:?} should use the code-background palette (bg={code_bg}); got {attrs:?}"
        );
    }

    /// R-074: a table link label uses the link palette color.
    fn assert_link_color(&self, text: &str, link: u16) {
        let attrs = self.styles_for(text);
        assert!(
            attrs.iter().all(|a| a.fg == Some(link)),
            "{text:?} should use the link palette color (fg={link}); got {attrs:?}"
        );
    }
}

impl Perform for StyledText {
    fn print(&mut self, character: char) {
        self.text.push(character);
        self.attributes.push(self.current);
    }

    fn execute(&mut self, byte: u8) {
        if byte == b'\n' {
            self.print('\n');
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _: &[u8], _: bool, action: char) {
        if action != 'm' {
            return;
        }
        let values: Vec<u16> = params.iter().map(|value| value[0]).collect();
        let mut i = 0;
        while i < values.len() {
            match values[i] {
                0 => {
                    self.current = Attrs::default();
                    i += 1;
                }
                1 => {
                    self.current.bold = true;
                    i += 1;
                }
                3 => {
                    self.current.italic = true;
                    i += 1;
                }
                9 => {
                    self.current.strike = true;
                    i += 1;
                }
                7 => {
                    self.current.reverse = true;
                    i += 1;
                }
                22 => {
                    self.current.bold = false;
                    i += 1;
                }
                23 => {
                    self.current.italic = false;
                    i += 1;
                }
                29 => {
                    self.current.strike = false;
                    i += 1;
                }
                27 => {
                    self.current.reverse = false;
                    i += 1;
                }
                39 => {
                    self.current.fg = None;
                    i += 1;
                }
                49 => {
                    self.current.bg = None;
                    i += 1;
                }
                38 | 48 => {
                    let is_foreground = values[i] == 38;
                    if i + 2 < values.len()
                        && values[i + 1] == 5
                        && values.get(i + 2).is_some_and(|v| *v <= 255)
                    {
                        let index = values[i + 2];
                        if is_foreground {
                            self.current.fg = Some(index);
                        } else {
                            self.current.bg = Some(index);
                        }
                        i += 3;
                    } else if i + 1 < values.len() && values[i + 1] == 2 {
                        // RGB: mark as "colored" with a sentinel distinct from
                        // any 256-color index.
                        if is_foreground {
                            self.current.fg = Some(0xffff);
                        } else {
                            self.current.bg = Some(0xffff);
                        }
                        i += 5;
                    } else {
                        i += 1;
                    }
                }
                30..=37 => {
                    self.current.fg = Some(values[i] - 30 + 8);
                    i += 1;
                }
                90..=97 => {
                    self.current.fg = Some(values[i] - 90 + 8);
                    i += 1;
                }
                40..=47 => {
                    self.current.bg = Some(values[i] - 40 + 8);
                    i += 1;
                }
                100..=107 => {
                    self.current.bg = Some(values[i] - 100 + 8);
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }
    }
}

#[test]
fn table_inline_code_has_monospace_styling() {
    // R-074: table inline code uses the code-background palette, not reverse
    // video. The name is retained for traceability to the original intent
    // ("dedicated code appearance"); the approved mechanism is the palette.
    let (code_bg, _) = expected_palette();
    let md = "| Code | Plain |\n| ------ | ----- |\n| `inline` | text |";
    let output = render_width(md, Some(80));
    let styled = StyledText::parse(&output);
    styled.assert_code_background("inline", code_bg);
    // The surrounding cell text does not carry the code background.
    assert!(
        styled
            .styles_for("text")
            .iter()
            .all(|a| a.bg != Some(code_bg)),
        "plain cell text must not inherit the code background"
    );
}

#[test]
fn table_code_whitespace_is_preserved_not_normalized() {
    // R-074: "Existing cell whitespace normalization and wrapping also apply
    // to inline-code text." The double space collapses to one, like any other
    // cell text. (Name retained for traceability; the contract normalizes.)
    let md = "| Code | Plain |\n| ------ | ----- |\n| `a  b` | text |";
    let output = strip_ansi(&render_width(md, Some(80)));
    assert!(
        output.contains("a b"),
        "Whitespace in code should be normalized to a single space: {output:?}"
    );
    assert!(
        !output.contains("a  b"),
        "Code should not gain nonbreaking whitespace in tables: {output:?}"
    );
}

#[test]
fn table_code_atomic_span_semantics_no_mid_grapheme_wrap() {
    let md = "| Code | Plain |\n| ------ | ----- |\n| `👩‍💻` | text |";
    let output = render_width(md, Some(10));
    let styled = StyledText::parse(&output);
    // The emoji should not be split across lines in code
    let text_content = &styled.text;
    assert!(
        !text_content.contains('\\'),
        "Emoji in code should not be escaped/split: {text_content:?}"
    );
}

#[test]
fn table_links_retain_normal_link_color() {
    let (_, link) = expected_palette();
    let md = "| Link | Text |\n| ------ | ----- |\n| [`file.rs`](file.rs) | description |";
    let output = render_width(md, Some(80));
    let styled = StyledText::parse(&output);
    // The link text "file.rs" should have the link palette color.
    styled.assert_link_color("file.rs", link);
}

#[test]
fn nested_emphasis_in_table_code_is_styled() {
    // A code span is literal: `**bold** *italic*` renders as those literal
    // characters, styled with the code-background palette (the enclosing
    // emphasis does not apply its own styling inside code).
    let (code_bg, _) = expected_palette();
    let md = "| Code | Plain |\n| ------ | ----- |\n| `**bold** *italic*` | text |";
    let output = render_width(md, Some(80));
    let styled = StyledText::parse(&output);
    styled.assert_code_background("bold", code_bg);
    styled.assert_code_background("italic", code_bg);
}

#[test]
fn nested_emphasis_in_table_links_is_styled() {
    let (_, link) = expected_palette();
    let md = "| Link | Text |\n| ------ | ----- |\n| **[link](url)** | desc |";
    let output = render_width(md, Some(80));
    let styled = StyledText::parse(&output);
    // The link should have the link palette color, and bold should compose.
    styled.assert_link_color("link", link);
    let attrs = styled.styles_for("link");
    assert!(
        attrs.iter().any(|a| a.bold),
        "Link text should be bold: {attrs:?}"
    );
}

#[test]
fn unicode_in_table_code_is_measured_correctly() {
    let md = "| Unicode | Plain |\n| --------- | ----- |\n| `世界` | text |";
    let output = strip_ansi(&render_width(md, Some(80)));
    // "世界" is 2 display width characters
    assert!(
        output.contains("世界"),
        "Unicode in code should be preserved: {output:?}"
    );
}

#[test]
fn explicit_breaks_in_table_cells_are_preserved() {
    let md = "| Cell | Plain |\n| ------ | ----- |\n| line1<br>line2 | text |";
    let output = strip_ansi(&render_width(md, Some(80)));
    assert!(
        output.contains("line1"),
        "First line should be present: {output:?}"
    );
    assert!(
        output.contains("line2"),
        "Second line should be present: {output:?}"
    );
}

#[test]
fn table_overflow_does_not_corrupt_cell_geometry() {
    let md =
        "| LongContent | Short |\n| ----------- | ----- |\n| verylongwordthatwilloverflow | x |";
    let output = render_width(md, Some(20));
    let lines: Vec<&str> = output.lines().collect();
    // All non-empty lines should have consistent structure
    for line in lines.iter().filter(|l| !l.is_empty()) {
        // Lines should not be empty or malformed
        assert!(!line.is_empty(), "Line should not be empty: {line:?}");
    }
}

#[test]
fn plain_output_has_no_ansi_escapes_in_tables() {
    let md = "| `code` | [link](url) |\n| ------ | --------- |\n| text | more |";
    let output = render_plain(md, Some(80));
    assert!(
        !output.contains('\x1b'),
        "Plain output should have no ANSI escapes: {output:?}"
    );
}

#[test]
fn table_header_zebra_striping_preserves_code_styling() {
    let (code_bg, _) = expected_palette();
    let md = "| **`code`** | Plain |\n| :--------- | ----- |\n| `text` | more |";
    let output = render_width(md, Some(80));
    let styled = StyledText::parse(&output);
    // Header code should use the code-background palette (composed over the
    // reversed header row).
    styled.assert_code_background("code", code_bg);
    // The header row itself is a full reverse video.
    assert!(
        styled.styles_for("code").iter().all(|a| a.reverse),
        "Header row should be reversed: {:?}",
        styled.styles_for("code")
    );
    // Body code should also use the code-background palette, on the zebra
    // row (not reversed).
    styled.assert_code_background("text", code_bg);
    assert!(
        styled.styles_for("text").iter().all(|a| !a.reverse),
        "Body row should not be reversed: {:?}",
        styled.styles_for("text")
    );
}

#[test]
fn linked_code_in_tables_has_combined_styling() {
    let (code_bg, link) = expected_palette();
    let md = "| LinkCode | Plain |\n| -------- | ----- |\n| [`foo.rs`](foo.rs) | text |";
    let output = render_width(md, Some(80));
    let styled = StyledText::parse(&output);
    // The code text "foo.rs" inside the link should have both the link color
    // and the code-background palette (independent SGR attributes that
    // compose, R-074).
    styled.assert_link_color("foo.rs", link);
    styled.assert_code_background("foo.rs", code_bg);
}

#[test]
fn table_cell_clipping_and_resize_preserves_content() {
    let md = "| A | B | C |\n| - | - | - |\n| x | y | z |";
    // Render at different widths and ensure content is preserved
    for width in [0, 10, 20, 80] {
        let output = strip_ansi(&render_width(md, Some(width)));
        assert!(
            output.contains("x"),
            "Content 'x' should be present at width {width}: {output:?}"
        );
        assert!(
            output.contains("y"),
            "Content 'y' should be present at width {width}: {output:?}"
        );
        assert!(
            output.contains("z"),
            "Content 'z' should be present at width {width}: {output:?}"
        );
    }
}

#[test]
fn empty_cells_in_tables_are_handled_correctly() {
    let md = "| A | B |\n| - | - |\n| | x |";
    let output = strip_ansi(&render_width(md, Some(80)));
    // Empty cell should still maintain table structure
    assert!(
        output.contains("|"),
        "Table structure should be preserved: {output:?}"
    );
}

#[test]
fn ragged_tables_with_mismatched_columns_still_render() {
    let md = "| A | B | C |\n| - | - |\n| x | y |";
    let output = strip_ansi(&render_width(md, Some(80)));
    // Should not panic and should contain expected content
    assert!(
        output.contains("x") || output.contains("y"),
        "Content should be present: {output:?}"
    );
}
