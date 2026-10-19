use super::{MarkdownRenderer, render_markdown};

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

fn render_width(md: &str, width: usize) -> String {
    let mut buf = Vec::new();
    let mut renderer = MarkdownRenderer::new();
    renderer.set_width(Some(width));
    renderer.render_markdown(md, &mut buf).unwrap();
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
    let output = render_width(md, 40);
    // Check that table renders without error
    assert!(output.contains("Col1"));
    assert!(output.contains("Col2"));
}

#[test]
fn test_table_narrow_terminal() {
    // Table should wrap cells when terminal is narrow
    let md = "| Short | Very Long Column Header That Should Wrap |\n|-------|------------------------------------------|\n| A     | B                                        |";
    let output = render_width(md, 30);
    let plain = strip_ansi(&output);
    let lines: Vec<&str> = plain.lines().collect();

    // Every line must fit within the terminal width.
    for line in &lines {
        let w = unicode_width::UnicodeWidthStr::width(*line);
        assert!(
            w <= 30,
            "Line exceeds terminal width: {:?} ({} chars)",
            line,
            w
        );
    }

    // The long header must be present in the output, wrapped across lines.
    assert!(plain.contains("Very Long"));
    assert!(plain.contains("Header"));

    // The short column must also be present (possibly hard-wrapped if the
    // proportional allocation gives it less than 5 columns).
    let joined: String = lines
        .iter()
        .map(|l| {
            // Take the first column of each line.
            match l.find(" | ") {
                Some(pos) => &l[..pos],
                None => l,
            }
        })
        .collect::<Vec<_>>()
        .join("");
    assert!(
        joined.contains("Short"),
        "Short column must be recoverable from wrapped chunks, got {:?}",
        joined
    );
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
    let output = render_width(md, 40);
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
    let md = "```hemlock\nlet x = 1;\nlet y = 2;\nlet z = x + y;\n```";
    let output = render_width(md, 40);
    let lines: Vec<&str> = output.lines().collect();
    // Should have 3 code lines
    let code_lines: Vec<&str> = lines
        .iter()
        .filter(|l| l.contains("let"))
        .copied()
        .collect();
    assert_eq!(code_lines.len(), 3, "Should have 3 code lines");
}

#[test]
fn test_wrap_preserves_indentation() {
    // Verify wrapped list items are indented
    let md = "- This is a long list item that should wrap to the next line with proper indentation";
    let output = render_width(md, 40);
    let lines: Vec<&str> = output.lines().collect();
    // Find the bullet point
    let bullet_idx = lines.iter().position(|l| l.contains("•"));
    assert!(bullet_idx.is_some());
    let bullet_idx = bullet_idx.unwrap();
    // Next line (wrapped part) should be indented
    assert!(bullet_idx + 1 < lines.len());
    let wrapped = lines[bullet_idx + 1];
    assert!(
        wrapped.starts_with("  ") || wrapped.starts_with("    "),
        "Long item should wrap"
    );
}

/// Render at a fixed width and explicit background mode, returning the raw
/// ANSI output (unstripped) so tests can inspect the emitted SGR codes.
fn render_width_bg(md: &str, width: usize, bg: super::TerminalBg) -> String {
    let mut buf = Vec::new();
    let mut renderer = MarkdownRenderer::new();
    renderer.set_width(Some(width));
    renderer.set_background(bg);
    renderer.render_markdown(md, &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn test_code_panel_background_is_mode_appropriate() {
    // Dark mode uses the dark code panel (256-color index 236 = #262626);
    // light mode uses the light code panel (index 254 = #e4e4e4). The
    // background is applied per line, not taken from the syntect theme.
    let dark = render_width_bg("```rust\nlet x = 1;\n```", 40, super::TerminalBg::Dark);
    assert!(
        dark.contains("\x1b[48;5;236m"),
        "dark code block must carry the dark panel background"
    );
    let light = render_width_bg("```rust\nlet x = 1;\n```", 40, super::TerminalBg::Light);
    assert!(
        light.contains("\x1b[48;5;254m"),
        "light code block must carry the light panel background"
    );
    // The syntect theme's own per-token truecolor background (48;2;...) must
    // not leak through: the panel is a single 256-color background per line.
    assert!(
        !dark.contains("48;2;") && !light.contains("48;2;"),
        "the syntect theme background must not be emitted"
    );
}

#[test]
fn test_code_panel_background_reaches_right_edge() {
    // Every code line — including blank lines inside the block — must be
    // padded to the terminal width so the background spans the whole line.
    let output = render_width_bg(
        "```text\nshort\n\nlonger line of text\n```",
        20,
        super::TerminalBg::Dark,
    );
    for line in output.lines() {
        if line.contains("\x1b[48;5;236m") {
            let clean = strip_ansi(line);
            let w = unicode_width::UnicodeWidthStr::width(clean.as_str());
            assert_eq!(
                w, 20,
                "backgrounded code line must span the full width, got {} ({} chars)",
                w, clean
            );
        }
    }
}

#[test]
fn test_code_block_preserves_syntect_foreground() {
    // The syntect theme must still color the tokens: the output keeps
    // per-token truecolor foreground codes (38;2;...) alongside the panel
    // background. A highlighted line has at least one 38;2;... code.
    let output = render_width_bg("```rust\nlet x = 1;\n```", 40, super::TerminalBg::Dark);
    assert!(
        output.contains("38;2;"),
        "syntect foreground colors must be preserved in code blocks"
    );
}

#[test]
fn test_code_block_blank_line_keeps_background() {
    // A blank line inside a fenced block is still part of the panel and must
    // carry the background (via padding) rather than being a bare empty line.
    let output = render_width_bg("```text\nalpha\n\nbeta\n```", 12, super::TerminalBg::Dark);
    let lines: Vec<&str> = output.lines().collect();
    // The blank line sits between "alpha" and "beta"; it must carry the panel
    // background, not be a truly empty line.
    let blank = lines
        .iter()
        .position(|l| strip_ansi(l).trim().is_empty() && !l.is_empty())
        .unwrap_or_else(|| {
            panic!(
                "expected a padded blank line inside the code block, got: {:?}",
                lines.iter().map(|l| strip_ansi(l)).collect::<Vec<_>>()
            )
        });
    assert!(
        lines[blank].contains("\x1b[48;5;236m"),
        "the blank line must carry the panel background"
    );
}
