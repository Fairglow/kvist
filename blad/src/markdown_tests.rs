use super::*;

/// Strip ANSI escape codes from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ESC sequence until 'm'
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

fn render(md: &str) -> String {
    let mut buf = Vec::new();
    render_markdown(md, &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn test_render_basic() {
    let md = "# Hello\n\nThis is **bold** text.";
    let output = strip_ansi(&render(md));
    assert!(output.contains("# "));
    assert!(output.contains("bold"));
}

#[test]
fn test_render_list() {
    let md = "- Item 1\n- Item 2";
    let output = render(md);
    let bullet_count = output.matches("•").count();
    assert_eq!(bullet_count, 2);
}

#[test]
fn test_render_table() {
    let md = "| Col1 | Col2 |\n|------|------|\n| A | B |\n| C | D |";
    let output = render(md);
    assert!(output.contains("Col1"));
    assert!(output.contains("Col2"));
    assert!(output.contains("A"));
    assert!(output.contains("B"));
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() >= 4);
}

#[test]
fn test_render_table_with_code() {
    let md = "| File | Count |\n|------|-------|\n| `A` | 1 |\n| `B` | 2 |";
    let output = render(md);
    assert!(output.contains("File"));
    assert!(output.contains("Count"));
    let lines: Vec<&str> = output.lines().collect();
    assert!(lines.len() >= 4);
}

#[test]
fn test_heading_levels() {
    let md = "# H1\n## H2\n### H3\n#### H4";
    let rendered = render(md);
    assert!(rendered.contains("\x1b[34m"));
    let output = strip_ansi(&rendered);
    assert!(output.contains("# H1"));
    assert!(output.contains("## H2"));
    assert!(output.contains("### H3"));
    assert!(output.contains("#### H4"));
}

#[test]
fn test_horizontal_rule() {
    let md = "Before\n\n---\n\nAfter";
    let output = render(md);
    assert!(output.contains("─"));
}

#[test]
fn test_gfm_alerts() {
    let md = "> [!NOTE]\n> This is a note";
    let output = render(md);
    assert!(output.contains("[NOTE]"));
}

#[test]
fn test_footnotes() {
    let md = "Text with footnote[^1].\n\n[^1]: Footnote definition";
    let output = render(md);
    // Footnote reference is rendered with ANSI color, check for parts
    assert!(output.contains("[1"));
    assert!(output.contains("]"));
    assert!(output.contains("Footnote definition"));
}

#[test]
fn test_superscript() {
    let md = "H~2~O and E=mc^2^";
    let output = render(md);
    assert!(output.contains("2"));
    assert!(output.contains("mc"));
}

#[test]
fn test_inline_math() {
    let md = "The formula is $E=mc^2$ here";
    let output = render(md);
    assert!(output.contains("$E=mc^2$") || output.contains("E=mc^2"));
}

#[test]
fn test_display_math() {
    let md = "$$\\frac{1}{2}$$";
    let output = render(md);
    assert!(output.contains("\\frac{1}{2}"));
}

#[test]
fn test_autolinks() {
    let md = "Visit https://example.com for more";
    let output = render(md);
    assert!(output.contains("https://example.com"));
}

#[test]
fn test_mermaid() {
    let md = "```mermaid\ngraph TD\n    A --> B\n```";
    let output = render(md);
    // Should render either via mmdc or as plain text fallback
    assert!(output.contains("A") || output.contains("B") || output.contains("Mermaid diagram"));
}

#[test]
fn test_heading_blank_line() {
    // Heading followed by paragraph with blank line in source should have
    // blank line in output
    let md = "# Heading\n\nParagraph";
    let output = strip_ansi(&render(md));
    let lines: Vec<&str> = output.lines().collect();
    // Find heading line
    let heading_idx = lines.iter().position(|l| l.contains("# Heading"));
    assert!(heading_idx.is_some());
    let heading_idx = heading_idx.unwrap();
    // Next line should be blank
    assert!(heading_idx + 1 < lines.len());
    assert!(lines[heading_idx + 1].is_empty());
}

#[test]
fn test_paragraph_blank_line() {
    // Two paragraphs should have blank line between them
    let md = "Para 1\n\nPara 2";
    let output = render(md);
    let lines: Vec<&str> = output.lines().collect();
    // Find "Para 1" line
    let para1_idx = lines.iter().position(|l| l.contains("Para 1"));
    assert!(para1_idx.is_some());
    let para1_idx = para1_idx.unwrap();
    // Next line should be blank
    assert!(para1_idx + 1 < lines.len());
    assert!(lines[para1_idx + 1].is_empty());
}

#[test]
fn test_table_width_adaptive() {
    // Table should adapt to terminal width
    let md = "| Col1 | Col2 |\n|------|------|\n| A | B |";
    let mut buf = Vec::new();
    render_markdown_width(md, &mut buf, Some(40)).unwrap();
    let output = String::from_utf8(buf).unwrap();
    // Check that table renders without error
    assert!(output.contains("Col1"));
    assert!(output.contains("Col2"));
}

#[test]
fn test_table_narrow_terminal() {
    // Table should wrap cells when terminal is narrow
    let md = "| Short | Very Long Column Header That Should Wrap |\n|-------|------------------------------------------|\n| A     | B                                        |";
    let mut buf = Vec::new();
    render_markdown_width(md, &mut buf, Some(30)).unwrap();
    let output = String::from_utf8(buf).unwrap();
    eprintln!("Output:");
    eprintln!("{}", output);
    // Check that table renders without error
    assert!(output.contains("Short"));
    assert!(output.contains("Very Long"));
}

#[test]
fn test_no_extra_blank_at_start() {
    // No blank line before first heading
    let md = "# First Heading\n\nContent";
    let output = strip_ansi(&render(md));
    // First line should be the heading, not blank
    let lines: Vec<&str> = output.lines().collect();
    assert!(!lines.is_empty());
    assert!(lines[0].contains("# First Heading"));
}

#[test]
fn test_rendered_starts_at_line_one() {
    // Verify rendered output starts at line 1 with content, not blank lines
    let md = "# Title\n\nBody paragraph.\n\n- Item 1\n- Item 2\n\n| Col | Val |\n|-----|-----|\n| A | 1 |";
    let output = strip_ansi(&render(md));
    let lines: Vec<&str> = output.lines().collect();

    // First line should be the heading
    assert!(!lines.is_empty());
    assert!(
        lines[0].contains("# Title"),
        "First line should be heading, got: {}",
        lines[0]
    );

    // Find where body content starts
    let body_idx = lines.iter().position(|l| l.contains("Body paragraph"));
    assert!(body_idx.is_some(), "Body paragraph not found");
    // Should be at line index 2 (heading, blank, body)
    assert_eq!(
        body_idx.unwrap(),
        2,
        "Body should be at index 2, got {:?}",
        body_idx
    );
}

#[test]
fn test_soft_wrap_paragraphs() {
    // Verify long paragraphs are wrapped at terminal width
    let md = "This is a very long paragraph that should be wrapped at the terminal width when the text exceeds the available space. It should wrap at word boundaries to maintain readability. The wrapping should happen automatically based on the terminal width setting.";
    let mut buf = Vec::new();
    render_markdown_width(md, &mut buf, Some(40)).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    // Should be wrapped into multiple lines
    assert!(
        lines.len() > 1,
        "Long paragraph should be wrapped into multiple lines"
    );
    // Each line should be at most 40 chars (approximately, ignoring ANSI codes)
    for line in &lines {
        let clean = strip_ansi(line);
        assert!(clean.len() <= 40, "Line too long: {} chars", clean.len());
    }
}

#[test]
fn test_no_wrap_code_blocks() {
    // Verify code blocks are not wrapped
    let md = "```hemlock\nlet x = 1;\nlet y = 2;\nlet z = x + y;\n```

This is a paragraph after the code block.";
    let mut buf = Vec::new();
    render_markdown_width(md, &mut buf, Some(40)).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    // Code block lines should be preserved as-is (not wrapped)
    let code_lines: Vec<&str> = lines
        .iter()
        .filter(|l| l.contains("let x") || l.contains("let y") || l.contains("let z"))
        .cloned()
        .collect();
    assert_eq!(code_lines.len(), 3, "Should have 3 code lines");
}

#[test]
fn test_wrap_preserves_indentation() {
    // Verify wrapped list items maintain indentation
    let md = "- This is a very long list item that should wrap to the next line while maintaining the bullet point indentation level. It should wrap at word boundaries.";
    let mut buf = Vec::new();
    render_markdown_width(md, &mut buf, Some(40)).unwrap();
    let output = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = output.lines().collect();

    // Find lines that are wrapped continuations (don't start with bullet)
    let continuation_lines: Vec<&str> = lines
        .iter()
        .filter(|l| !l.contains("•") && !l.trim().is_empty())
        .cloned()
        .collect();
    assert!(!continuation_lines.is_empty(), "Long item should wrap");

    // Wrapped lines should be indented (start with spaces)
    for line in &continuation_lines {
        let clean = strip_ansi(line);
        // Check that wrapped lines are indented (at least 4 spaces)
        assert!(
            clean.starts_with("    "),
            "Wrapped line should be indented: '{}'",
            clean
        );
    }
}
