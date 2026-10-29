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
