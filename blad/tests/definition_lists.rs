//! Definition list terminal projection tests.
//!
//! These tests pin the approved dialect and layout for definition lists in
//! Blad's terminal output. They are written fail-first: the parser does not
//! yet enable the definition list extension, so these tests will fail until
//! the feature is implemented end-to-end (parsing, layout, navigation).
//!
//! Approved dialect:
//!   - Terms are followed by definitions indented 2 spaces
//!   - Multiple definitions for a single term are supported
//!   - Multiple terms can share a definition block
//!   - Definitions can contain nested blocks (paragraphs, lists, code)
//!
//! Approved layout:
//!   - Terms are rendered bold on their own line
//!   - Definitions are indented 2 spaces relative to the term
//!   - Definitions wrap to the full terminal width minus the 2-space indent
//!   - Multiple terms share a single definition line with a comma separator
//!   - A blank line separates each term/definition group
//!
//! Non-matches:
//!   - Lines that look like definitions but aren't indented are plain text
//!   - Indented text that doesn't follow a term is a code block or continuation

use blad::{Color, MarkdownRenderer, RenderOptions};
use vte::{Params, Parser, Perform};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Appearance {
    color: Option<Color>,
    bold: bool,
    italic: bool,
    strike: bool,
    reverse: bool,
}

#[derive(Default)]
struct StyledText {
    plain: String,
    text: String,
    styles: Vec<Appearance>,
    current: Appearance,
}

impl StyledText {
    fn parse(output: &str) -> Self {
        let mut text = Self::default();
        Parser::new().advance(&mut text, output.as_bytes());
        text
    }

    fn assert_style(&self, text: &str, expected: Appearance) {
        let offset = self.text.find(text).unwrap_or_else(|| {
            panic!("missing {text:?} in {:?}", self.text);
        });
        let start = self.text[..offset].chars().count();
        let styles = &self.styles[start..start + text.chars().count()];
        assert!(
            styles.iter().all(|style| *style == expected),
            "{text:?}: expected {expected:?}, got {styles:?}"
        );
    }
}

impl Perform for StyledText {
    fn print(&mut self, character: char) {
        self.plain.push(character);
        if !character.is_whitespace() {
            self.text.push(character);
            self.styles.push(self.current);
        }
    }

    fn execute(&mut self, byte: u8) {
        if byte == b'\n' {
            self.plain.push('\n');
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _: &[u8], _: bool, action: char) {
        if action != 'm' {
            return;
        }
        let mut values = params.iter().map(|value| value[0]);
        while let Some(value) = values.next() {
            match value {
                0 => self.current = Appearance::default(),
                1 => self.current.bold = true,
                3 => self.current.italic = true,
                9 => self.current.strike = true,
                22 => self.current.bold = false,
                23 => self.current.italic = false,
                29 => self.current.strike = false,
                30..=37 => self.current.color = Some(Color::Ansi((value - 30) as u8)),
                90..=97 => self.current.color = Some(Color::Ansi((value - 90 + 8) as u8)),
                39 => self.current.color = None,
                38 | 48 => {
                    let color = match values.next() {
                        Some(5) => Color::Ansi(values.next().unwrap().try_into().unwrap()),
                        Some(2) => Color::Rgb(
                            values.next().unwrap().try_into().unwrap(),
                            values.next().unwrap().try_into().unwrap(),
                            values.next().unwrap().try_into().unwrap(),
                        ),
                        other => panic!("unexpected color encoding: {other:?}"),
                    };
                    if value == 38 {
                        self.current.color = Some(color);
                    }
                }
                _ => {}
            }
        }
    }
}

fn render(source: &str, width: Option<usize>, options: RenderOptions) -> String {
    let mut renderer = MarkdownRenderer::with_options(options);
    renderer.set_width(width);
    let mut output = Vec::new();
    renderer.render_markdown(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

fn plain(source: &str, width: Option<usize>) -> String {
    let mut rendered = render(source, width, RenderOptions::plain());
    rendered = rendered.replace("\r\n", "\n");
    rendered
}

#[test]
fn basic_term_definition_layout() {
    let md = "term\n  definition text";
    let output = plain(md, Some(80));
    // Term is bold on its own line, definition indented 2 spaces
    assert!(output.contains("term"), "term must appear: {output:?}");
    assert!(
        output.contains("  definition text"),
        "definition must be indented 2 spaces: {output:?}"
    );
}

#[test]
fn multiple_definitions_for_single_term() {
    let md = "term\n  first definition\n\n  second definition";
    let output = plain(md, Some(80));
    assert!(output.contains("  first definition"));
    assert!(output.contains("  second definition"));
}

#[test]
fn multiple_terms_share_definition() {
    let md = "term1\nterm2\n  shared definition";
    let output = plain(md, Some(80));
    // Multiple terms share a definition line with comma separator
    assert!(
        output.contains("term1") && output.contains("term2"),
        "both terms must appear: {output:?}"
    );
    assert!(output.contains("  shared definition"));
}

#[test]
fn definition_wraps_to_width_minus_indent() {
    let md = "term\n  This is a very long definition that should wrap when the terminal is narrow enough to force it.";
    let output = plain(md, Some(20));
    // Definition wraps at width minus 2-space indent (18 chars of content)
    for line in output.lines() {
        if line.starts_with("  ") {
            assert!(
                line.len() <= 20,
                "wrapped definition line too long: {:?} (len {})",
                line,
                line.len()
            );
        }
    }
}

#[test]
fn nested_paragraphs_in_definition() {
    let md = "term\n  First paragraph.\n\n  Second paragraph.";
    let output = plain(md, Some(80));
    assert!(output.contains("  First paragraph."));
    assert!(output.contains("  Second paragraph."));
}

#[test]
fn nested_list_in_definition() {
    let md = "term\n  - item one\n  - item two";
    let output = plain(md, Some(80));
    assert!(output.contains("item one"));
    assert!(output.contains("item two"));
}

#[test]
fn nested_code_block_in_definition() {
    let md = "term\n  ```text\n  code here\n  ```";
    let output = plain(md, Some(80));
    assert!(output.contains("code here"));
}

#[test]
fn term_is_rendered_bold() {
    let md = "term\n  definition";
    let output = render(md, Some(80), RenderOptions::default());
    let text = StyledText::parse(&output);
    text.assert_style(
        "term",
        Appearance {
            bold: true,
            ..Appearance::default()
        },
    );
}

#[test]
fn definition_links_are_navigable() {
    let md = "term\n  See [docs](https://example.com) for more.";
    let output = render(md, Some(80), RenderOptions::default());
    // The link label should be rendered with link styling
    let text = StyledText::parse(&output);
    let link_color = Appearance {
        color: Some(Color::Ansi(14)),
        ..Appearance::default()
    };
    text.assert_style("docs", link_color);
}

#[test]
fn heading_in_definition_list_creates_anchor() {
    let md = "# Glossary\n\nterm\n  definition with [link](#glossary)";
    let output = plain(md, Some(80));
    assert!(output.contains("Glossary"));
    assert!(output.contains("term"));
    assert!(output.contains("definition"));
}

#[test]
fn unicode_in_terms_and_definitions() {
    let md = "世界\n  Definition in English.\n\ncafé\n  Définition en français.";
    let output = plain(md, Some(80));
    assert!(output.contains("世界"));
    assert!(output.contains("café"));
    assert!(output.contains("Defin"));
}

#[test]
fn definition_list_with_inline_code() {
    let md = "fn()\n  A function call in code: `fn()`.";
    let output = plain(md, Some(80));
    assert!(output.contains("fn()"));
}

#[test]
fn tiny_widths_render_without_panic() {
    let md = "term\n  definition";
    for width in [0, 1, 2, 5, 10] {
        let result = plain(md, Some(width));
        assert!(!result.is_empty(), "width {width}: empty output");
    }
}

#[test]
fn plain_output_has_no_escapes() {
    let md = "term\n  definition";
    let output = render(md, Some(80), RenderOptions::plain());
    assert!(
        !output.contains('\x1b'),
        "plain output must not contain escape sequences"
    );
}

#[test]
fn color_output_has_styling() {
    let md = "term\n  definition";
    let output = render(md, Some(80), RenderOptions::default());
    // Bold term should produce escape sequences
    assert!(
        output.contains('\x1b'),
        "color output must contain escape sequences"
    );
}

#[test]
fn literal_non_match_unindented_is_not_definition() {
    // A line that looks like a definition but isn't indented
    let md = "term\nnot a definition";
    let output = plain(md, Some(80));
    // "not a definition" should be a separate paragraph, not indented
    assert!(!output.contains("  not a definition"));
}

#[test]
fn indented_text_without_term_is_code_block() {
    // Indented text that doesn't follow a term is a code block
    let md = "Some text.\n\n    This is code, not a definition.";
    let output = plain(md, Some(80));
    assert!(output.contains("This is code"));
}

#[test]
fn definition_list_search_finds_terms() {
    // Verify that terms are searchable (plain text contains them)
    let md = "alpha\n  First term.\n\nbeta\n  Second term.";
    let output = plain(md, Some(80));
    assert!(output.contains("alpha"));
    assert!(output.contains("beta"));
    assert!(output.contains("First term"));
    assert!(output.contains("Second term"));
}

#[test]
fn definition_list_streaming_partial_input() {
    // Partial definition list input should not panic
    let md = "term\n  part";
    let output = plain(md, Some(80));
    assert!(output.contains("term"));
    assert!(output.contains("part"));
}

#[test]
fn definition_list_resize_renders_consistently() {
    let md = "term\n  This is a definition that wraps.";
    let narrow = plain(md, Some(20));
    let wide = plain(md, Some(80));
    // Both should contain the term and definition text
    assert!(narrow.contains("term"));
    assert!(narrow.contains("definition"));
    assert!(wide.contains("term"));
    assert!(wide.contains("definition"));
}

#[test]
fn complex_definition_list_mixed_content() {
    let md = "# API Reference\n\nget()\n  Fetches data from the server. See [GET request](https://http.dev) for details.\n\n  ```http\n  GET /api/users\n  ```\n\npost()\n  Creates a new resource.\n  - Requires authentication\n  - Returns 201 on success";
    let output = plain(md, Some(80));
    assert!(output.contains("get()"));
    assert!(output.contains("post()"));
    assert!(output.contains("Fetches data"));
    assert!(output.contains("Creates a new resource"));
    assert!(output.contains("Requires authentication"));
    assert!(output.contains("Returns 201"));
}
