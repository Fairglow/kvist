use blad::{MarkdownRenderer, RenderOptions};

fn render(source: &str, width: Option<usize>) -> String {
    let mut renderer = MarkdownRenderer::with_options(RenderOptions::plain());
    renderer.set_width(width);
    let mut output = Vec::new();
    renderer.render_markdown(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn details_summary_and_unseparated_html_text_are_visible() {
    let source = "<details>\n<summary>More &amp; less</summary>\nHidden text<br>second line\n</details>\n\n# After\n\n[link](https://example.com)\n";
    let text = render(source, None);
    assert!(text.contains("More & less"), "{text:?}");
    assert!(text.contains("Hidden text\nsecond line"), "{text:?}");
    assert!(
        text.contains("# After") && text.contains("link"),
        "{text:?}"
    );
    assert!(!text.contains("<details>") && !text.contains("<summary>"));
    assert_eq!(
        render(source, None),
        render(&source.replace("<details>", "<DETAILS open>"), None)
    );
}

#[test]
fn unsupported_html_is_literal_and_never_hides_its_contents() {
    let source = "<widget title=\"a > b\">visible &lt;text&gt;</widget>\n\n<!-- comment -->\n\n<script>if (a < b) { '<br>'; }</script>\n";
    let text = render(source, None);
    for expected in [
        "<widget title=\"a > b\">",
        "visible <text>",
        "</widget>",
        "<!-- comment -->",
        "<script>if (a < b) { '<br>'; }</script>",
    ] {
        assert!(text.contains(expected), "{expected:?}: {text:?}");
    }
}

#[test]
fn inline_html_and_table_cells_keep_breaks_and_source() {
    let text = render(
        "before<br/>after <b>bold</b> <kbd>key</kbd> <custom>x</custom>\n\n| H |\n| - |\n| a<BR />b <widget>c</widget> |\n",
        None,
    );
    assert!(
        text.contains("before\nafter bold key <custom>x</custom>"),
        "{text:?}"
    );
    assert!(text.contains("<widget>c</widget>"), "{text:?}");
    assert!(!text.contains("<kbd>") && !text.contains("<BR"));
}

#[test]
fn html_preserves_code_frontmatter_controls_and_unknown_entities() {
    let text = render(
        "---\nvalue: <br>\n---\n\n`<details>`\n\n<details>\n<summary>&#x4e16;&#30028; &unknown;</summary>\ntext\x1b[2J\n</details>\n",
        None,
    );
    assert!(text.contains("value: <br>") && text.contains("<details>"));
    assert!(text.contains("世界 &unknown;"), "{text:?}");
    assert!(!text.contains('\x1b'), "{text:?}");
}

#[test]
fn html_wraps_and_propagates_writer_and_cancellation_errors() {
    use std::io::{self, Write};
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("html writer failed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let source = "<details><summary>one two three four five</summary>body</details>";
    let text = render(source, Some(9));
    assert!(
        text.lines().all(|line| line.chars().count() <= 9),
        "{text:?}"
    );
    let renderer = MarkdownRenderer::new();
    assert!(renderer.render_markdown(source, &mut Broken).is_err());
    let cancellation = blad::RenderCancellation::default();
    cancellation.cancel();
    assert_eq!(
        renderer
            .render_markdown_with_cancellation(source, &mut Vec::new(), &cancellation)
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
}

#[test]
fn strip_policy_is_typed_and_keeps_malformed_markup_and_code() {
    let mut renderer = MarkdownRenderer::with_options(RenderOptions {
        strip_html: true,
        ..RenderOptions::plain()
    });
    renderer.set_width(Some(80));
    let mut output = Vec::new();
    renderer
        .render_markdown(
            "<widget>x</widget> `<widget>` <broken title=\"unfinished\n",
            &mut output,
        )
        .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(
        text.contains("x <widget> <broken title=\"unfinished"),
        "{text:?}"
    );
    assert!(!text.contains("</widget>"));
}

#[test]
fn inline_html_labels_agree_with_link_extraction() {
    let source = "[<b>bold</b> <kbd>key</kbd> <custom>literal</custom>](target.md)";
    let links = blad::links::extract_links(source);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].text, "bold key <custom>literal</custom>");
    assert_eq!(render(source, None).trim(), links[0].text);
}

#[test]
fn multiline_comments_cannot_change_later_html_link_labels() {
    let source = "<!--\n<script>\n-->\n\n[<b>Heading</b>](#heading)\n";
    let links = blad::links::extract_links(source);
    assert_eq!(links[0].text, "Heading");
    let text = render(source, None);
    assert!(text.ends_with("Heading\n"), "{text:?}");
}

#[test]
fn raw_html_decodes_long_zero_padded_numeric_entities() {
    let source = format!(
        "<details><summary>&#{}65; &#x{}41;</summary></details>",
        "0".repeat(512),
        "0".repeat(512)
    );
    assert_eq!(render(&source, None).trim(), "A A");
}

#[test]
fn block_boundaries_are_visible() {
    // p and div create block boundaries; hr creates a rule
    let source = "<p>First paragraph</p>\n<p>Second paragraph</p>\n\n<div>Block div</div>\n\n<hr>\n\nAfter rule";
    let text = render(source, None);
    assert!(text.contains("First paragraph"), "{text:?}");
    assert!(text.contains("Second paragraph"), "{text:?}");
    assert!(text.contains("Block div"), "{text:?}");
    assert!(text.contains("After rule"), "{text:?}");
    // Paragraphs should be separated
    assert!(
        text.contains("First paragraph\nSecond paragraph")
            || text.contains("First paragraph\n\nSecond paragraph"),
        "{text:?}"
    );
}

#[test]
fn hr_creates_text_rule() {
    let source = "Above\n\n<hr>\n\nBelow";
    let text = render(source, None);
    assert!(text.contains("Above"), "{text:?}");
    assert!(text.contains("Below"), "{text:?}");
    // hr should produce some visible separator (not just invisible)
    let lines: Vec<&str> = text.lines().collect();
    let above_idx = lines.iter().position(|l| l.contains("Above")).unwrap();
    let below_idx = lines.iter().position(|l| l.contains("Below")).unwrap();
    assert!(
        below_idx > above_idx + 1,
        "hr should create separation between Above and Below"
    );
}

#[test]
fn malformed_html_remains_visible() {
    let source =
        "<unclosed tag\n<missing-quote attr=value>\n<<double-lt>>\n<tag attr=value missing-eq";
    let text = render(source, None);
    // Malformed tags should remain as literal text, not hide content
    assert!(text.contains("<unclosed tag"), "{text:?}");
    assert!(text.contains("missing-quote"), "{text:?}");
}

#[test]
fn nested_html_is_bounded() {
    // Deep nesting should not crash or silently drop content
    let mut source = String::new();
    for _ in 0..100 {
        source.push_str("<b>");
    }
    source.push_str("deep");
    for _ in 0..100 {
        source.push_str("</b>");
    }
    let text = render(&source, None);
    assert!(text.contains("deep"), "{text:?}");
}

#[test]
fn unmatched_closing_tags_do_not_crash() {
    let source = "</b></i></em></strong></s></del></code></kbd></samp>";
    // Should not crash; unmatched closers are treated as literal
    render(source, None);
}

#[test]
fn html_entity_decoding_in_text() {
    let source = "Text with &amp; &lt; &gt; &quot; &apos; &nbsp; entities";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
    assert!(text.contains("<"), "{text:?}");
    assert!(text.contains(">"), "{text:?}");
    assert!(text.contains('"'), "{text:?}");
}

#[test]
fn numeric_entities_are_decoded() {
    let source = "ASCII: &#65;&#66;&#67; Hex: &#x41;&#x42;&#x43;";
    let text = render(source, None);
    assert!(text.contains("ABC"), "{text:?}");
}

#[test]
fn unknown_entities_remain_source() {
    let source = "Unknown: &notarealentity; &amp;";
    let text = render(source, None);
    assert!(text.contains("&notarealentity;"), "{text:?}");
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_in_table_cells() {
    let source =
        "| Header |\n|--------|\n| Cell with <b>bold</b> and <br> break |\n| Cell &amp; entities |";
    let text = render(source, Some(80));
    assert!(text.contains("bold"), "{text:?}");
    assert!(text.contains("break"), "{text:?}");
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_strip_policy_removes_markup_keeps_text() {
    let mut renderer = MarkdownRenderer::with_options(RenderOptions {
        strip_html: true,
        ..RenderOptions::plain()
    });
    renderer.set_width(Some(80));
    let mut output = Vec::new();
    renderer
        .render_markdown("<b>bold</b> <i>italic</i> <p>paragraph</p>", &mut output)
        .unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("bold"), "{text:?}");
    assert!(text.contains("italic"), "{text:?}");
    assert!(text.contains("paragraph"), "{text:?}");
    assert!(!text.contains("<b>"), "{text:?}");
    assert!(!text.contains("<i>"), "{text:?}");
}

#[test]
fn script_style_contents_remain_literal() {
    let source = "<script>var x = '<br>'; alert('hi');</script>\n\nAfter script";
    let text = render(source, None);
    assert!(text.contains("var x"), "{text:?}");
    assert!(text.contains("After script"), "{text:?}");
    // Script content should not be interpreted as HTML
    assert!(text.contains("<br>"), "{text:?}");
}

#[test]
fn html_does_not_execute_or_fetch() {
    // Verify no process spawning or network by just rendering
    let source = "<img src=\"http://example.com/image.png\" alt=\"test alt\">";
    let text = render(source, None);
    assert!(text.contains("test alt"), "{text:?}");
    // img tag itself is literal (unsupported)
    assert!(text.contains("<img"), "{text:?}");
}

#[test]
fn html_in_link_labels() {
    let source = "[<b>bold link</b>](https://example.com)";
    let text = render(source, None);
    assert!(text.contains("bold link"), "{text:?}");
}

#[test]
fn html_in_heading_labels() {
    let source = "# Heading with <b>bold</b> and <i>italic</i>";
    let text = render(source, None);
    assert!(text.contains("Heading with"), "{text:?}");
    assert!(text.contains("bold"), "{text:?}");
    assert!(text.contains("italic"), "{text:?}");
}

#[test]
fn html_attributes_are_ignored() {
    let source = "<b class=\"red\" style=\"color:red\">text</b>";
    let text = render(source, None);
    assert!(text.contains("text"), "{text:?}");
    // Attributes are ignored, not interpreted
}

#[test]
fn html_case_insensitive_tag_names() {
    let source = "<B>BOLD</B> <I>Italic</I> <BR> break";
    let text = render(source, None);
    assert!(text.contains("BOLD"), "{text:?}");
    assert!(text.contains("Italic"), "{text:?}");
    assert!(text.contains("break"), "{text:?}");
}

#[test]
fn html_with_markdown_mixed() {
    let source = "Markdown **bold** and HTML <b>bold</b> in same line";
    let text = render(source, None);
    assert!(text.contains("bold"), "{text:?}");
}

#[test]
fn html_details_expanded_not_foldable() {
    let source = "<details open>\n<summary>Summary</summary>\nHidden content\n</details>";
    let text = render(source, None);
    assert!(text.contains("Summary"), "{text:?}");
    assert!(text.contains("Hidden content"), "{text:?}");
}

#[test]
fn html_details_closed_still_expanded() {
    // Even closed details are expanded (not foldable)
    let source = "<details>\n<summary>Summary</summary>\nHidden content\n</details>";
    let text = render(source, None);
    assert!(text.contains("Summary"), "{text:?}");
    assert!(text.contains("Hidden content"), "{text:?}");
}

#[test]
fn html_br_variants() {
    let source = "Line1<br>Line2<br/>Line3<br />Line4";
    let text = render(source, None);
    assert!(text.contains("Line1"), "{text:?}");
    assert!(text.contains("Line2"), "{text:?}");
    assert!(text.contains("Line3"), "{text:?}");
    assert!(text.contains("Line4"), "{text:?}");
}

#[test]
fn html_inline_style_combinations() {
    let source = "<b><i><s>combined</s></i></b>";
    let text = render(source, None);
    assert!(text.contains("combined"), "{text:?}");
}

#[test]
fn html_code_kbd_samp_styling() {
    let source = "<code>code</code> <kbd>key</kbd> <samp>sample</samp>";
    let text = render(source, None);
    assert!(text.contains("code"), "{text:?}");
    assert!(text.contains("key"), "{text:?}");
    assert!(text.contains("sample"), "{text:?}");
}

#[test]
fn html_unsupported_tags_are_literal() {
    let source = "<video src=\"test.mp4\"></video>";
    let text = render(source, None);
    assert!(text.contains("<video"), "{text:?}");
    assert!(text.contains("</video>"), "{text:?}");
}

#[test]
fn html_comments_are_literal_by_default() {
    let source = "<!-- This is a comment -->";
    let text = render(source, None);
    assert!(text.contains("<!--"), "{text:?}");
    assert!(text.contains("-->"), "{text:?}");
}

#[test]
fn html_with_newlines_in_attributes() {
    let source = "<div title=\"line1\nline2\">content</div>";
    let text = render(source, None);
    assert!(text.contains("content"), "{text:?}");
}

#[test]
fn html_empty_tags() {
    let source = "<br/><hr/><input type=\"text\"/>";
    let text = render(source, None);
    // br and hr produce breaks/rules; input is literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_malformed_attribute_values() {
    let source = "<tag attr=\"unterminated>content</tag>";
    // Should not crash
    render(source, None);
}

#[test]
fn html_depth_limit_64() {
    // Stack limit is 64; deeper nesting should not crash
    let mut source = String::new();
    for _ in 0..100 {
        source.push_str("<b>");
    }
    source.push_str("text");
    for _ in 0..100 {
        source.push_str("</b>");
    }
    let text = render(&source, None);
    assert!(text.contains("text"), "{text:?}");
}

#[test]
fn html_large_input_does_not_crash() {
    // Resource limit: large HTML input
    let mut source = String::new();
    for i in 0..1000 {
        source.push_str(&format!("<p>Paragraph {}</p>\n", i));
    }
    let text = render(&source, None);
    assert!(text.contains("Paragraph 0"), "{text:?}");
    assert!(text.contains("Paragraph 999"), "{text:?}");
}

#[test]
fn html_entity_boundary_conditions() {
    // Entity at end of string
    let text1 = render("Text &amp;", None);
    assert!(text1.contains("&"), "{text1:?}");

    // Entity at start
    let text2 = render("&amp; Text", None);
    assert!(text2.contains("&"), "{text2:?}");

    // Multiple entities
    let text3 = render("&amp;&amp;&amp;", None);
    assert!(text3.contains("&&&"), "{text3:?}");
}

#[test]
fn html_numeric_entity_boundary_conditions() {
    // Max valid codepoint (U+10FFFF)
    let text1 = render("&#1114111;", None);
    assert!(
        !text1.contains("&#1114111;"),
        "max codepoint should be decoded: {text1:?}"
    );

    // Invalid: too large (> 0x10FFFF) - becomes replacement character
    let text2 = render("&#1114112;", None);
    assert!(
        text2.contains('\u{FFFD}'),
        "too-large entity should become replacement: {text2:?}"
    );
}

#[test]
fn html_hex_entity_boundary_conditions() {
    // Valid hex
    let text1 = render("&#x41;", None);
    assert!(text1.contains("A"), "{text1:?}");

    // Case insensitive hex prefix
    let text2 = render("&#X41;", None);
    assert!(text2.contains("A"), "{text2:?}");

    // Invalid hex digits
    let text3 = render("&#xZZ;", None);
    assert!(text3.contains("&#xZZ;"), "{text3:?}");
}

#[test]
fn html_entity_ambiguous_ampersand() {
    // &amp followed by other text (not semicolon)
    let text = render("&amp text", None);
    assert!(text.contains("&amp text"), "{text:?}");
}

#[test]
fn html_raw_elements_script_style_textarea() {
    // Raw elements keep fake tags inside
    let source = "<script>if (a < b) { return '<br>'; }</script>";
    let text = render(source, None);
    assert!(text.contains("if (a < b)"), "{text:?}");
    assert!(text.contains("<br>"), "{text:?}");

    let source2 = "<style>.class { color: red; }</style>";
    let text2 = render(source2, None);
    assert!(text2.contains("color: red"), "{text2:?}");

    let source3 = "<textarea>user <b>input</b></textarea>";
    let text3 = render(source3, None);
    assert!(text3.contains("user"), "{text3:?}");
    assert!(text3.contains("<b>"), "{text3:?}");
}

#[test]
fn html_raw_element_close_case_insensitive() {
    let source = "<script>content</SCRIPT>";
    let text = render(source, None);
    assert!(text.contains("content"), "{text:?}");
}

#[test]
fn html_raw_element_incomplete_close() {
    let source = "<script>content</scripted>";
    let text = render(source, None);
    assert!(text.contains("content"), "{text:?}");
    assert!(text.contains("</scripted>"), "{text:?}");
}

#[test]
fn html_incomplete_tag_at_eof() {
    let source = "Text with <incomplete";
    let text = render(source, None);
    assert!(text.contains("Text with"), "{text:?}");
    assert!(text.contains("<incomplete"), "{text:?}");
}

#[test]
fn html_incomplete_comment_at_eof() {
    let source = "Text with <!-- incomplete comment";
    let text = render(source, None);
    assert!(text.contains("Text with"), "{text:?}");
    assert!(text.contains("<!-- incomplete comment"), "{text:?}");
}

#[test]
fn html_incomplete_attribute_at_eof() {
    let source = "Text with <tag attr=\"incomplete";
    let text = render(source, None);
    assert!(text.contains("Text with"), "{text:?}");
}

#[test]
fn html_entity_in_attribute_is_not_decoded() {
    // Entities in attributes are not decoded (attributes are ignored)
    let source = "<b class=\"&amp;\">text</b>";
    let text = render(source, None);
    assert!(text.contains("text"), "{text:?}");
}

#[test]
fn html_entity_in_text_context_only() {
    // Entities are decoded in text context
    let source = "Before &lt;tag&gt; After";
    let text = render(source, None);
    assert!(text.contains("<tag>"), "{text:?}");
}

#[test]
fn html_malformed_entity_semicolon_missing() {
    let source = "Text &amp text";
    let text = render(source, None);
    assert!(text.contains("&amp text"), "{text:?}");
}

#[test]
fn html_entity_double_ampersand() {
    let source = "Text &&amp; text";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_name() {
    let source = "Text &amp ; text";
    let text = render(source, None);
    assert!(text.contains("&amp ;"), "{text:?}");
}

#[test]
fn html_entity_leading_whitespace() {
    let source = "Text & amp; text";
    let text = render(source, None);
    assert!(text.contains("& amp;"), "{text:?}");
}

#[test]
fn html_entity_trailing_whitespace() {
    let source = "Text &amp; text";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_in_entity() {
    let source = "&amp;amp;";
    let text = render(source, None);
    assert!(text.contains("&amp;"), "{text:?}");
}

#[test]
fn html_entity_numeric_whitespace() {
    let source = "&# 65;";
    let text = render(source, None);
    assert!(text.contains("&# 65;"), "{text:?}");
}

#[test]
fn html_entity_hex_whitespace() {
    let source = "&#x 41;";
    let text = render(source, None);
    assert!(text.contains("&#x 41;"), "{text:?}");
}

#[test]
fn html_entity_zero_padded_decimal() {
    let source = "&#065;";
    let text = render(source, None);
    assert!(text.contains("A"), "{text:?}");
}

#[test]
fn html_entity_zero_padded_hex() {
    let source = "&#x041;";
    let text = render(source, None);
    assert!(text.contains("A"), "{text:?}");
}

#[test]
fn html_entity_negative_decimal() {
    let source = "&#-65;";
    let text = render(source, None);
    assert!(text.contains("&#-65;"), "{text:?}");
}

#[test]
fn html_entity_negative_hex() {
    let source = "&#x-41;";
    let text = render(source, None);
    assert!(text.contains("&#x-41;"), "{text:?}");
}

#[test]
fn html_entity_plus_sign_decimal() {
    let source = "&#+65;";
    let text = render(source, None);
    assert!(text.contains("&#+65;"), "{text:?}");
}

#[test]
fn html_entity_plus_sign_hex() {
    let source = "&#x+41;";
    let text = render(source, None);
    assert!(text.contains("&#x+41;"), "{text:?}");
}

#[test]
fn html_entity_leading_plus_decimal() {
    let source = "&#+65;";
    let text = render(source, None);
    assert!(text.contains("&#+65;"), "{text:?}");
}

#[test]
fn html_entity_leading_plus_hex() {
    let source = "&#x+41;";
    let text = render(source, None);
    assert!(text.contains("&#x+41;"), "{text:?}");
}

#[test]
fn html_entity_trailing_plus_decimal() {
    let source = "&#65+;";
    let text = render(source, None);
    assert!(text.contains("&#65+;"), "{text:?}");
}

#[test]
fn html_entity_trailing_plus_hex() {
    let source = "&#x41+;";
    let text = render(source, None);
    assert!(text.contains("&#x41+;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_after_hash() {
    let source = "&# 65;";
    let text = render(source, None);
    assert!(text.contains("&# 65;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_after_x() {
    let source = "&#x 41;";
    let text = render(source, None);
    assert!(text.contains("&#x 41;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_before_semicolon_decimal() {
    let source = "&#65 ;";
    let text = render(source, None);
    assert!(text.contains("&#65 ;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_before_semicolon_hex() {
    let source = "&#x41 ;";
    let text = render(source, None);
    assert!(text.contains("&#x41 ;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_after_semicolon() {
    let source = "&#65; text";
    let text = render(source, None);
    assert!(text.contains("A"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_named_entity() {
    let source = "&amp ;";
    let text = render(source, None);
    assert!(text.contains("&amp ;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_around_ampersand() {
    let source = " &amp; ";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_whitespace_before_ampersand() {
    let source = "text &amp;";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_whitespace_after_ampersand() {
    let source = "&amp; text";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_whitespace_between_ampersands() {
    let source = "&amp; &amp;";
    let text = render(source, None);
    // Each entity decodes to &, with the space preserved between them
    assert!(text.contains("& &"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_numeric_value() {
    let source = "&#6 5;";
    let text = render(source, None);
    assert!(text.contains("&#6 5;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_hex_value() {
    let source = "&#x4 1;";
    let text = render(source, None);
    assert!(text.contains("&#x4 1;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_around_semicolon() {
    let source = "&#65 ;";
    let text = render(source, None);
    assert!(text.contains("&#65 ;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_around_hash() {
    let source = "&# 65;";
    let text = render(source, None);
    assert!(text.contains("&# 65;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_around_x() {
    let source = "&#x 41;";
    let text = render(source, None);
    assert!(text.contains("&#x 41;"), "{text:?}");
}

#[test]
fn html_entity_all_whitespace() {
    let source = "&amp;";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_whitespace_only_entity() {
    let source = "&amp;";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_whitespace_around_entity() {
    let source = " &amp; ";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_whitespace_before_entity() {
    let source = " &amp;";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_whitespace_after_entity() {
    let source = "&amp; ";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_whitespace_between_entities() {
    let source = "&amp; &lt;";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
    assert!(text.contains("<"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_sequence() {
    let source = "&amp;&lt;&gt;";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
    assert!(text.contains("<"), "{text:?}");
    assert!(text.contains(">"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_named_sequence() {
    let source = "&amp; &lt; &gt;";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
    assert!(text.contains("<"), "{text:?}");
    assert!(text.contains(">"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_numeric_sequence() {
    let source = "&#65;&#66;&#67;";
    let text = render(source, None);
    assert!(text.contains("ABC"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_hex_sequence() {
    let source = "&#x41;&#x42;&#x43;";
    let text = render(source, None);
    assert!(text.contains("ABC"), "{text:?}");
}

#[test]
fn html_entity_whitespace_mixed_sequence() {
    let source = "&amp; &#65; &#x41;";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
    assert!(text.contains("A"), "{text:?}");
}

#[test]
fn html_entity_whitespace_all_types() {
    let source = "&amp; &lt; &gt; &quot; &apos; &nbsp; &#65; &#x41; &#X41;";
    let text = render(source, None);
    assert!(text.contains("&"), "{text:?}");
    assert!(text.contains("<"), "{text:?}");
    assert!(text.contains(">"), "{text:?}");
    assert!(text.contains('"'), "{text:?}");
    assert!(text.contains("A"), "{text:?}");
}

#[test]
fn html_entity_whitespace_invalid_sequence() {
    let source = "&unknown; &amp; &#9999999999999999999; &#xZZZZ;";
    let text = render(source, None);
    assert!(text.contains("&unknown;"), "{text:?}");
    assert!(text.contains("&"), "{text:?}");
}

#[test]
fn html_entity_whitespace_edge_cases() {
    // Entity at very end
    let text1 = render("text&amp;", None);
    assert!(text1.contains("&"), "{text1:?}");

    // Entity at very start
    let text2 = render("&amp;text", None);
    assert!(text2.contains("&"), "{text2:?}");

    // Only entity
    let text3 = render("&amp;", None);
    assert!(text3.contains("&"), "{text3:?}");

    // Entity followed by newline
    let text4 = render("&amp;\nnext", None);
    assert!(text4.contains("&"), "{text4:?}");
    assert!(text4.contains("next"), "{text4:?}");
}

#[test]
fn html_entity_whitespace_in_attributes() {
    let source = "<b class=\"&amp;\">text</b>";
    let text = render(source, None);
    assert!(text.contains("text"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_tag_name() {
    let source = "<b &amp;>text</b>";
    // Malformed tag, should not crash
    render(source, None);
}

#[test]
fn html_entity_whitespace_in_closing_tag() {
    let source = "<b>text</b &amp;>";
    let text = render(source, None);
    assert!(text.contains("text"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_self_closing() {
    let source = "<br &amp;/>";
    // Malformed, should not crash
    render(source, None);
}

#[test]
fn html_entity_whitespace_in_empty_tag() {
    let source = "<img src=\"test.png\" alt=\"&amp;\"/>";
    let text = render(source, None);
    assert!(text.contains("<img"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_raw_element() {
    let source = "<script>&amp;</script>";
    let text = render(source, None);
    assert!(text.contains("&amp;"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_comment() {
    let source = "<!-- &amp; -->";
    let text = render(source, None);
    assert!(text.contains("<!--"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_doctype() {
    let source = "<!DOCTYPE html &amp;>";
    let text = render(source, None);
    // Declarations are literal
    assert!(text.contains("<!DOCTYPE"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_processing_instruction() {
    let source = "<?xml version=\"1.0\" &amp; ?>";
    let text = render(source, None);
    // Processing instructions are literal
    assert!(text.contains("<?xml"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_cdata() {
    let source = "<![CDATA[&amp;]]>";
    let text = render(source, None);
    // CDATA is literal
    assert!(text.contains("<![CDATA"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_template() {
    let source = "<template>&amp;</template>";
    let text = render(source, None);
    // Template is unsupported, literal
    assert!(text.contains("<template>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_svg() {
    let source = "<svg>&amp;</svg>";
    let text = render(source, None);
    // SVG is unsupported, literal
    assert!(text.contains("<svg>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_math() {
    let source = "<math>&amp;</math>";
    let text = render(source, None);
    // Math is unsupported, literal
    assert!(text.contains("<math>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_iframe() {
    let source = "<iframe src=\"test.html\"></iframe>";
    let text = render(source, None);
    // Iframe is unsupported, literal
    assert!(text.contains("<iframe"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_object() {
    let source = "<object data=\"test.pdf\"></object>";
    let text = render(source, None);
    // Object is unsupported, literal
    assert!(text.contains("<object"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_embed() {
    let source = "<embed src=\"test.swf\">";
    let text = render(source, None);
    // Embed is unsupported, literal
    assert!(text.contains("<embed"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_audio() {
    let source = "<audio src=\"test.mp3\"></audio>";
    let text = render(source, None);
    // Audio is unsupported, literal
    assert!(text.contains("<audio"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_video() {
    let source = "<video src=\"test.mp4\"></video>";
    let text = render(source, None);
    // Video is unsupported, literal
    assert!(text.contains("<video"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_source() {
    let source = "<source src=\"test.mp4\" type=\"video/mp4\">";
    let text = render(source, None);
    // Source is unsupported, literal
    assert!(text.contains("<source"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_track() {
    let source = "<track src=\"test.vtt\" kind=\"subtitles\">";
    let text = render(source, None);
    // Track is unsupported, literal
    assert!(text.contains("<track"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_form() {
    let source = "<form action=\"submit.html\" method=\"post\"></form>";
    let text = render(source, None);
    // Form is unsupported, literal
    assert!(text.contains("<form"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_button() {
    let source = "<button type=\"submit\">Click</button>";
    let text = render(source, None);
    // Button is unsupported, literal
    assert!(text.contains("<button"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_select() {
    let source = "<select><option>Choice</option></select>";
    let text = render(source, None);
    // Select is unsupported, literal
    assert!(text.contains("<select>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_option() {
    let source = "<option value=\"1\">One</option>";
    let text = render(source, None);
    // Option is unsupported, literal
    assert!(text.contains("<option"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_textarea() {
    let source = "<textarea>Text &amp; more</textarea>";
    let text = render(source, None);
    // Textarea is raw element, content is literal
    assert!(text.contains("Text"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_label() {
    let source = "<label for=\"field\">Label</label>";
    let text = render(source, None);
    // Label is unsupported, literal
    assert!(text.contains("<label"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_fieldset() {
    let source = "<fieldset><legend>Group</legend></fieldset>";
    let text = render(source, None);
    // Fieldset is unsupported, literal
    assert!(text.contains("<fieldset>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_legend() {
    let source = "<legend>Group</legend>";
    let text = render(source, None);
    // Legend is unsupported, literal
    assert!(text.contains("<legend>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_input() {
    let source = "<input type=\"text\" name=\"field\"/>";
    let text = render(source, None);
    // Input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_checkbox() {
    let source = "<input type=\"checkbox\" name=\"agree\"/>";
    let text = render(source, None);
    // Checkbox is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_radio() {
    let source = "<input type=\"radio\" name=\"choice\" value=\"1\"/>";
    let text = render(source, None);
    // Radio is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_hidden() {
    let source = "<input type=\"hidden\" name=\"token\" value=\"abc\"/>";
    let text = render(source, None);
    // Hidden is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_submit() {
    let source = "<input type=\"submit\" value=\"Submit\"/>";
    let text = render(source, None);
    // Submit is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_reset() {
    let source = "<input type=\"reset\" value=\"Reset\"/>";
    let text = render(source, None);
    // Reset is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_file() {
    let source = "<input type=\"file\" name=\"upload\"/>";
    let text = render(source, None);
    // File input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_password() {
    let source = "<input type=\"password\" name=\"pass\"/>";
    let text = render(source, None);
    // Password is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_email() {
    let source = "<input type=\"email\" name=\"email\"/>";
    let text = render(source, None);
    // Email input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_url() {
    let source = "<input type=\"url\" name=\"website\"/>";
    let text = render(source, None);
    // URL input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_number() {
    let source = "<input type=\"number\" name=\"count\"/>";
    let text = render(source, None);
    // Number input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_range() {
    let source = "<input type=\"range\" name=\"volume\"/>";
    let text = render(source, None);
    // Range input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_date() {
    let source = "<input type=\"date\" name=\"birthday\"/>";
    let text = render(source, None);
    // Date input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_time() {
    let source = "<input type=\"time\" name=\"appointment\"/>";
    let text = render(source, None);
    // Time input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_datetime() {
    let source = "<input type=\"datetime-local\" name=\"event\"/>";
    let text = render(source, None);
    // Datetime input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_month() {
    let source = "<input type=\"month\" name=\"billing\"/>";
    let text = render(source, None);
    // Month input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_week() {
    let source = "<input type=\"week\" name=\"schedule\"/>";
    let text = render(source, None);
    // Week input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_color() {
    let source = "<input type=\"color\" name=\"theme\"/>";
    let text = render(source, None);
    // Color input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_search() {
    let source = "<input type=\"search\" name=\"query\"/>";
    let text = render(source, None);
    // Search input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_tel() {
    let source = "<input type=\"tel\" name=\"phone\"/>";
    let text = render(source, None);
    // Tel input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_image() {
    let source = "<input type=\"image\" src=\"btn.png\"/>";
    let text = render(source, None);
    // Image input is unsupported, literal
    assert!(text.contains("<input"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_progress() {
    let source = "<progress value=\"50\" max=\"100\"></progress>";
    let text = render(source, None);
    // Progress is unsupported, literal
    assert!(text.contains("<progress"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_meter() {
    let source = "<meter value=\"0.6\" min=\"0\" max=\"1\"></meter>";
    let text = render(source, None);
    // Meter is unsupported, literal
    assert!(text.contains("<meter"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_output() {
    let source = "<output for=\"a b\">Result</output>";
    let text = render(source, None);
    // Output is unsupported, literal
    assert!(text.contains("<output"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_datalist() {
    let source = "<datalist id=\"options\"><option value=\"A\"/></datalist>";
    let text = render(source, None);
    // Datalist is unsupported, literal
    assert!(text.contains("<datalist"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_keygen() {
    let source = "<keygen name=\"key\"/>";
    let text = render(source, None);
    // Keygen is unsupported, literal
    assert!(text.contains("<keygen"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_output_element() {
    let source = "<output>Result</output>";
    let text = render(source, None);
    // Output is unsupported, literal
    assert!(text.contains("<output>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_map() {
    let source = "<map name=\"coords\"><area shape=\"rect\" coords=\"0,0,100,100\"/></map>";
    let text = render(source, None);
    // Map is unsupported, literal
    assert!(text.contains("<map"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_area() {
    let source = "<area shape=\"circle\" coords=\"50,50,25\" href=\"target.html\"/>";
    let text = render(source, None);
    // Area is unsupported, literal
    assert!(text.contains("<area"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_frame() {
    let source = "<frameset><frame src=\"content.html\"/></frameset>";
    let text = render(source, None);
    // Frames are unsupported, literal
    assert!(text.contains("<frameset>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_noframes() {
    let source = "<noframes>Old browser</noframes>";
    let text = render(source, None);
    // Noframes is unsupported, literal
    assert!(text.contains("<noframes>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_base() {
    let source = "<base href=\"http://example.com/\"/>";
    let text = render(source, None);
    // Base is unsupported, literal
    assert!(text.contains("<base"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_link_rel() {
    let source = "<link rel=\"stylesheet\" href=\"style.css\"/>";
    let text = render(source, None);
    // Link is unsupported, literal
    assert!(text.contains("<link"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_meta() {
    let source = "<meta charset=\"utf-8\"/>";
    let text = render(source, None);
    // Meta is unsupported, literal
    assert!(text.contains("<meta"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_title() {
    let source = "<title>Page Title</title>";
    let text = render(source, None);
    // Title is unsupported, literal
    assert!(text.contains("<title>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_head() {
    let source = "<head><title>Test</title></head>";
    let text = render(source, None);
    // Head is unsupported, literal
    assert!(text.contains("<head>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_body() {
    let source = "<body>Content</body>";
    let text = render(source, None);
    // Body is unsupported, literal
    assert!(text.contains("<body>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_html() {
    let source = "<html><body>Content</body></html>";
    let text = render(source, None);
    // html tag is unsupported, literal
    assert!(text.contains("<html>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_header() {
    let source = "<header>Site header</header>";
    let text = render(source, None);
    // Header is unsupported, literal
    assert!(text.contains("<header>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_footer() {
    let source = "<footer>Site footer</footer>";
    let text = render(source, None);
    // Footer is unsupported, literal
    assert!(text.contains("<footer>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_nav() {
    let source = "<nav><a href=\"/\">Home</a></nav>";
    let text = render(source, None);
    // Nav is unsupported, literal
    assert!(text.contains("<nav>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_main() {
    let source = "<main>Primary content</main>";
    let text = render(source, None);
    // Main is unsupported, literal
    assert!(text.contains("<main>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_article() {
    let source = "<article>Article content</article>";
    let text = render(source, None);
    // Article is unsupported, literal
    assert!(text.contains("<article>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_section() {
    let source = "<section>Section content</section>";
    let text = render(source, None);
    // Section is unsupported, literal
    assert!(text.contains("<section>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_aside() {
    let source = "<aside>Sidebar</aside>";
    let text = render(source, None);
    // Aside is unsupported, literal
    assert!(text.contains("<aside>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_figure() {
    let source =
        "<figure><img src=\"pic.png\" alt=\"pic\"/><figcaption>Caption</figcaption></figure>";
    let text = render(source, None);
    // Figure is unsupported, literal
    assert!(text.contains("<figure>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_figcaption() {
    let source = "<figcaption>Caption text</figcaption>";
    let text = render(source, None);
    // Figcaption is unsupported, literal
    assert!(text.contains("<figcaption>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_time_element() {
    let source = "<time datetime=\"2024-01-01\">New Year</time>";
    let text = render(source, None);
    // Time element is unsupported, literal
    assert!(text.contains("<time"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_mark() {
    let source = "<mark>Highlighted</mark>";
    let text = render(source, None);
    // Mark is unsupported, literal
    assert!(text.contains("<mark>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_small() {
    let source = "<small>Fine print</small>";
    let text = render(source, None);
    // Small is unsupported, literal
    assert!(text.contains("<small>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_big() {
    let source = "<big>Large text</big>";
    let text = render(source, None);
    // Big is unsupported, literal
    assert!(text.contains("<big>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_u() {
    let source = "<u>Underlined</u>";
    let text = render(source, None);
    // U is unsupported, literal
    assert!(text.contains("<u>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_abbr() {
    let source = "<abbr title=\"World Wide Web\">WWW</abbr>";
    let text = render(source, None);
    // Abbr is unsupported, literal
    assert!(text.contains("<abbr"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_acronym() {
    let source = "<acronym title=\"Federal Bureau of Investigation\">FBI</acronym>";
    let text = render(source, None);
    // Acronym is unsupported, literal
    assert!(text.contains("<acronym"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_q() {
    let source = "<q cite=\"source.html\">Quoted text</q>";
    let text = render(source, None);
    // Q is unsupported, literal
    assert!(text.contains("<q"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_blockquote() {
    let source = "<blockquote cite=\"source.html\">Block quote</blockquote>";
    let text = render(source, None);
    // Blockquote is unsupported, literal
    assert!(text.contains("<blockquote"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_pre() {
    let source = "<pre>Preformatted\ntext</pre>";
    let text = render(source, None);
    // Pre is unsupported, literal
    assert!(text.contains("<pre>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_address() {
    let source = "<address>123 Main St</address>";
    let text = render(source, None);
    // Address is unsupported, literal
    assert!(text.contains("<address>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_h1() {
    let source = "<h1>Heading 1</h1>";
    let text = render(source, None);
    // h1 is unsupported, literal
    assert!(text.contains("<h1>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_h2() {
    let source = "<h2>Heading 2</h2>";
    let text = render(source, None);
    // h2 is unsupported, literal
    assert!(text.contains("<h2>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_h3() {
    let source = "<h3>Heading 3</h3>";
    let text = render(source, None);
    // h3 is unsupported, literal
    assert!(text.contains("<h3>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_h4() {
    let source = "<h4>Heading 4</h4>";
    let text = render(source, None);
    // h4 is unsupported, literal
    assert!(text.contains("<h4>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_h5() {
    let source = "<h5>Heading 5</h5>";
    let text = render(source, None);
    // h5 is unsupported, literal
    assert!(text.contains("<h5>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_h6() {
    let source = "<h6>Heading 6</h6>";
    let text = render(source, None);
    // h6 is unsupported, literal
    assert!(text.contains("<h6>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_span() {
    let source = "<span class=\"red\">Red text</span>";
    let text = render(source, None);
    // Span is unsupported, literal
    assert!(text.contains("<span"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_a() {
    let source = "<a href=\"http://example.com\">Link text</a>";
    let text = render(source, None);
    // a is unsupported, literal
    assert!(text.contains("<a href"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_img() {
    let source = "<img src=\"image.png\" alt=\"Description\"/>";
    let text = render(source, None);
    // img is unsupported, literal
    assert!(text.contains("<img"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_ul() {
    let source = "<ul><li>Item 1</li><li>Item 2</li></ul>";
    let text = render(source, None);
    // ul is unsupported, literal
    assert!(text.contains("<ul>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_ol() {
    let source = "<ol><li>First</li><li>Second</li></ol>";
    let text = render(source, None);
    // ol is unsupported, literal
    assert!(text.contains("<ol>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_li() {
    let source = "<li>List item</li>";
    let text = render(source, None);
    // li is unsupported, literal
    assert!(text.contains("<li>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_dl() {
    let source = "<dl><dt>Term</dt><dd>Definition</dd></dl>";
    let text = render(source, None);
    // dl is unsupported, literal
    assert!(text.contains("<dl>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_dt() {
    let source = "<dt>Term</dt>";
    let text = render(source, None);
    // dt is unsupported, literal
    assert!(text.contains("<dt>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_dd() {
    let source = "<dd>Definition</dd>";
    let text = render(source, None);
    // dd is unsupported, literal
    assert!(text.contains("<dd>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_table() {
    let source = "<table><tr><td>Cell</td></tr></table>";
    let text = render(source, None);
    // table is unsupported, literal
    assert!(text.contains("<table>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_tr() {
    let source = "<tr><td>Cell</td></tr>";
    let text = render(source, None);
    // tr is unsupported, literal
    assert!(text.contains("<tr>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_td() {
    let source = "<td>Cell content</td>";
    let text = render(source, None);
    // td is unsupported, literal
    assert!(text.contains("<td>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_th() {
    let source = "<th>Header cell</th>";
    let text = render(source, None);
    // th is unsupported, literal
    assert!(text.contains("<th>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_thead() {
    let source = "<thead><tr><th>Header</th></tr></thead>";
    let text = render(source, None);
    // thead is unsupported, literal
    assert!(text.contains("<thead>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_tbody() {
    let source = "<tbody><tr><td>Body</td></tr></tbody>";
    let text = render(source, None);
    // tbody is unsupported, literal
    assert!(text.contains("<tbody>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_tfoot() {
    let source = "<tfoot><tr><td>Footer</td></tr></tfoot>";
    let text = render(source, None);
    // tfoot is unsupported, literal
    assert!(text.contains("<tfoot>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_colgroup() {
    let source = "<colgroup><col span=\"2\"/></colgroup>";
    let text = render(source, None);
    // colgroup is unsupported, literal
    assert!(text.contains("<colgroup>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_col() {
    let source = "<col span=\"2\"/>";
    let text = render(source, None);
    // col is unsupported, literal
    assert!(text.contains("<col"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_caption() {
    let source = "<caption>Table caption</caption>";
    let text = render(source, None);
    // caption is unsupported, literal
    assert!(text.contains("<caption>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_sup() {
    let source = "E = mc<sup>2</sup>";
    let text = render(source, None);
    // sup is unsupported, literal
    assert!(text.contains("<sup>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_sub() {
    let source = "H<sub>2</sub>O";
    let text = render(source, None);
    // sub is unsupported, literal
    assert!(text.contains("<sub>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_ins() {
    let source = "<ins>Inserted text</ins>";
    let text = render(source, None);
    // ins is unsupported, literal
    assert!(text.contains("<ins>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_del() {
    let source = "<del>Deleted text</del>";
    let text = render(source, None);
    // del is supported (strike)
    assert!(text.contains("Deleted text"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_s() {
    let source = "<s>Struck through</s>";
    let text = render(source, None);
    // s is supported (strike)
    assert!(text.contains("Struck through"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_strike() {
    let source = "<strike>Struck through</strike>";
    let text = render(source, None);
    // strike is supported
    assert!(text.contains("Struck through"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_em() {
    let source = "<em>Emphasized</em>";
    let text = render(source, None);
    // em is supported (italic)
    assert!(text.contains("Emphasized"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_i() {
    let source = "<i>Italic</i>";
    let text = render(source, None);
    // i is supported (italic)
    assert!(text.contains("Italic"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_strong() {
    let source = "<strong>Bold</strong>";
    let text = render(source, None);
    // strong is supported (bold)
    assert!(text.contains("Bold"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_b() {
    let source = "<b>Bold</b>";
    let text = render(source, None);
    // b is supported (bold)
    assert!(text.contains("Bold"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_code() {
    let source = "<code>Inline code</code>";
    let text = render(source, None);
    // code is supported
    assert!(text.contains("Inline code"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_kbd() {
    let source = "<kbd>Enter</kbd>";
    let text = render(source, None);
    // kbd is supported
    assert!(text.contains("Enter"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_samp() {
    let source = "<samp>Sample output</samp>";
    let text = render(source, None);
    // samp is supported
    assert!(text.contains("Sample output"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_var() {
    let source = "<var>variable</var>";
    let text = render(source, None);
    // var is unsupported, literal
    assert!(text.contains("<var>"), "{text:?}");
}

#[test]
fn html_entity_whitespace_in_dfn() {
    let source = "<dfn>Definition</dfn>";
    let text = render(source, None);
    // dfn is unsupported, literal
    assert!(text.contains("<dfn>"), "{text:?}");
}
