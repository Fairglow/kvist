use blad::markdown::MarkdownRenderer;
use unicode_width::UnicodeWidthStr;

fn plain(md: &str, width: Option<usize>) -> String {
    let mut bytes = Vec::new();
    let mut renderer = MarkdownRenderer::new();
    if let Some(w) = width {
        renderer.set_width(Some(w));
    }
    renderer.render_markdown(md, &mut bytes).unwrap();
    let rendered = String::from_utf8(bytes).unwrap();
    let mut output = String::new();
    let mut chars = rendered.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
            }
        } else {
            output.push(c);
        }
    }
    output.replace("\r\n", "\n")
}

#[test]
fn paragraphs_and_hard_breaks_start_without_spaces() {
    assert_eq!(
        plain("First paragraph.\n\nSecond paragraph.", Some(40)),
        "First paragraph.\n\nSecond paragraph.\n"
    );
    assert_eq!(
        plain("First line.  \nSecond line.", Some(40)),
        "First line.\nSecond line.\n"
    );
    assert_eq!(
        plain("Before.\n\n# Heading\n\nAfter.", Some(40)),
        "Before.\n\n# Heading\n\nAfter.\n"
    );
}

#[test]
fn inline_events_preserve_adjacency_and_source_spaces() {
    for width in [None, Some(80)] {
        assert_eq!(plain("a**b**c and `x`y.", width), "abc and xy.\n");
        assert_eq!(
            plain("one **two** three `four` five\nsix.", width),
            "one two three four five six.\n"
        );
        assert_eq!(plain("**one**,*two*! `three`.", width), "one,two! three.\n");
    }
}

#[test]
fn wrapping_uses_the_complete_width_and_separator_budget() {
    assert_eq!(plain("12345 6789", Some(10)), "12345 6789\n");
    assert_eq!(plain("12345 67890", Some(10)), "12345\n67890\n");
    assert_eq!(plain("ab**cd**ef gh", Some(6)), "abcdef\ngh\n");
    assert_eq!(plain("12345 `ab` xy", Some(8)), "12345 ab\nxy\n");
    assert_eq!(plain("12345\n67890", Some(10)), "12345\n67890\n");
}

#[test]
fn long_words_and_headings_wrap_without_initial_empty_lines() {
    assert_eq!(plain("abcdefghijkl", Some(5)), "abcde\nfghij\nkl\n");
    let output = plain("# A long heading with **styled** words", Some(12));
    assert!(output.starts_with("# A long"));
    assert!(output.lines().all(|line| line.width() <= 12));
    assert_eq!(plain("---", Some(12)).trim().width(), 12);
}

#[test]
fn unicode_width_and_graphemes_are_preserved() {
    assert_eq!(plain("世界 世界", Some(4)), "世界\n世界\n");
    assert_eq!(
        plain("e\u{301}e\u{301}e\u{301}", Some(2)),
        "e\u{301}e\u{301}\ne\u{301}\n"
    );
    assert_eq!(plain("👩‍💻👩‍💻", Some(2)), "👩‍💻\n👩‍💻\n");
}

#[test]
fn inline_code_and_code_block_whitespace_are_not_reformatted() {
    assert_eq!(
        plain("before `a  b` after", Some(80)),
        "before a  b after\n"
    );
    assert_eq!(plain("`abcdefghijk`", Some(4)), "abcdefghijk\n");
    assert_eq!(
        plain("```text\n  abcdefghijkl\n\n\tindent\n```", Some(4)),
        // The code panel's background must reach the right edge, so every
        // line — including the blank one — is padded to the terminal width
        // with spaces that carry the background. The *content* whitespace
        // (the leading spaces on the first line, the tab on the last) is
        // still preserved verbatim; only trailing background padding is added.
        // A full-width frame line is drawn directly above and below the block
        // (the bottom frame replaces the block's trailing blank line).
        "    \n  abcdefghijkl\n    \n\tindent\n    \n"
    );
}

#[test]
fn tiny_widths_and_partial_markdown_are_panic_free() {
    for width in [0, 1, 2, 20] {
        for md in [
            "",
            "hello",
            "# heading",
            "- item",
            "```text\nunfinished",
            "**open",
        ] {
            let result = plain(md, Some(width));
            assert!(md.is_empty() || !result.is_empty());
        }
    }
    let md = "# Heading\n\nText **with** `code` and [link](target).\n\n```text\nunfinished";
    for end in (0..=md.len()).filter(|end| md.is_char_boundary(*end)) {
        plain(&md[..end], Some(15));
    }
}

#[test]
fn heading_and_list_prefixes_fit_even_narrow_prose_budgets() {
    let md = "###### Heading with **styles**\n\nParagraph text.\n\n- A long item\n  - Nested text\n\n> [!NOTE]\n> Alert text";
    for width in 1..=30 {
        let output = plain(md, Some(width));
        for line in output.lines() {
            assert!(line.width() <= width, "width {width}: {line:?}");
        }
    }
}
