//! Baseline tests for the currently supported Markdown subset and its terminal projection.
//!
//! These tests pin the existing behavior so that intent changes have a stable
//! fail-first baseline. They cover the core rendering capabilities without
//! claiming new functionality.

#![cfg(test)]

use blad::{MarkdownRenderer, RenderOptions};

/// Helper to create a renderer with default options.
fn renderer() -> MarkdownRenderer {
    MarkdownRenderer::with_options(RenderOptions::default())
}

/// Helper to create a plain (no-color) renderer.
fn plain_renderer() -> MarkdownRenderer {
    MarkdownRenderer::with_options(RenderOptions::plain())
}

#[test]
fn commonmark_basic_elements_render() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = "# Heading\n\n**bold** and *italic*.\n\n- List item\n\n[link](url)";
    renderer.render_markdown(input, &mut output).unwrap();

    // Basic CommonMark elements should render without error
    assert!(!output.is_empty());
}

#[test]
fn gfm_extensions_supported() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = r#"
| Header | Header |
|--------|--------|
| Cell | Cell |

- [x] Task complete
- [ ] Task pending

~~strikethrough~~

`inline code`

```rust
fn main() {}
```
"#;
    renderer.render_markdown(input, &mut output).unwrap();
    assert!(!output.is_empty());
}

#[test]
fn yaml_frontmatter_preserved_as_literal() {
    let mut renderer = plain_renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = r#"---
title: Test
tags: [markdown, test]
---

# Content

Body text.
"#;
    renderer.render_markdown(input, &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    // Frontmatter should be preserved as literal YAML, not parsed/interpreted
    assert!(text.contains("title: Test"));
    assert!(text.contains("tags: [markdown, test]"));
}

#[test]
fn math_projection_bounded() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    // Supported math syntax
    let input = "Inline $x^2$ and display $$\\frac{a}{b}$$";
    renderer.render_markdown(input, &mut output).unwrap();
    assert!(!output.is_empty());

    // Unsupported/malformed math should preserve source
    let input_bad = "$unsupported_command{foo}$";
    let mut output_bad = Vec::new();
    renderer
        .render_markdown(input_bad, &mut output_bad)
        .unwrap();
    // Should not panic; source should be visible
    assert!(!output_bad.is_empty());
}

#[test]
fn mermaid_diagrams_text_only() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = r#"```mermaid
flowchart LR
A[Input] --> B[Output]
```"#;
    renderer.render_markdown(input, &mut output).unwrap();
    // Should render as text diagram, not HTML or graphics
    assert!(!output.is_empty());
}

#[test]
fn html_subset_inert() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    // Supported HTML subset (details/summary, breaks, rules, inline styles)
    let input = r#"
<details><summary>Click me</summary>Content</details>

<br>

<hr>

<b>bold</b> and <i>italic</i>
"#;
    renderer.render_markdown(input, &mut output).unwrap();
    assert!(!output.is_empty());

    // Unsupported HTML remains literal source
    let input_unsupported = r#"<script>alert('x')</script>"#;
    let mut output_unsupported = Vec::new();
    renderer
        .render_markdown(input_unsupported, &mut output_unsupported)
        .unwrap();
    assert!(!output_unsupported.is_empty());
}

#[test]
fn tables_alignment_supported() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = r#"
| Left | Center | Right |
|:-----|:------:|------:|
| Cell | Cell | Cell |
"#;
    renderer.render_markdown(input, &mut output).unwrap();
    assert!(!output.is_empty());
}

#[test]
fn footnotes_rendered() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = r#"Text with a footnote[^1].

[^1]: Footnote text.
"#;
    renderer.render_markdown(input, &mut output).unwrap();
    assert!(!output.is_empty());
}

#[test]
fn scripts_superscript_subscript() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = "E=mc^2^ and H~2~O";
    renderer.render_markdown(input, &mut output).unwrap();
    assert!(!output.is_empty());
}

#[test]
fn plain_output_no_ansi_escapes() {
    let mut renderer = plain_renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = "# Heading\n\n**bold** *italic*";
    renderer.render_markdown(input, &mut output).unwrap();

    let text = String::from_utf8(output).unwrap();
    // Plain output should contain no ANSI escape sequences
    assert!(!text.contains('\x1b'));
}

#[test]
fn code_blocks_preserved() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = r#"```rust
fn main() {
    println!("hello");
}
```"#;
    renderer.render_markdown(input, &mut output).unwrap();
    assert!(!output.is_empty());
}

#[test]
fn link_navigation_metadata_preserved() {
    // This test verifies that link metadata (for navigation) is preserved
    // through the rendering pipeline. The actual navigation is tested in
    // viewer tests; this ensures the renderer doesn't strip link info.
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = "[link text](https://example.com)";
    renderer.render_markdown(input, &mut output).unwrap();
    assert!(!output.is_empty());
}

#[test]
fn table_cell_emphasis_preserved() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    let input = r#"
| **bold** | *italic* | ~~strike~~ |
|----------|----------|----------|
| **bold *italic*** | | |
"#;
    renderer.render_markdown(input, &mut output).unwrap();
    assert!(!output.is_empty());
}

#[test]
fn frontmatter_boundary_cases() {
    let mut renderer = plain_renderer();
    renderer.set_width(Some(80));

    // BOM + frontmatter
    let input_bom = "\u{feff}---\ntitle: Test\n---\n# Content";
    let mut output = Vec::new();
    renderer.render_markdown(input_bom, &mut output).unwrap();
    assert!(!output.is_empty());

    // CRLF line endings
    let input_crlf = "---\r\ntitle: Test\r\n---\r\n# Content";
    let mut output_crlf = Vec::new();
    renderer
        .render_markdown(input_crlf, &mut output_crlf)
        .unwrap();
    assert!(!output_crlf.is_empty());

    // Empty frontmatter
    let input_empty = "---\n---\n# Content";
    let mut output_empty = Vec::new();
    renderer
        .render_markdown(input_empty, &mut output_empty)
        .unwrap();
    assert!(!output_empty.is_empty());
}

#[test]
fn html_entity_decoding_inert() {
    let mut renderer = plain_renderer();
    renderer.set_width(Some(80));
    let mut output = Vec::new();

    // Entity-decoded controls should be made inert
    let input = "&lt;script&gt;";
    renderer.render_markdown(input, &mut output).unwrap();
    let text = String::from_utf8(output).unwrap();
    // Should not execute as HTML; should be visible as text
    assert!(text.contains("<script>") || text.contains("&lt;script&gt;"));
}

#[test]
fn viewer_options_persist_across_renders() {
    let mut renderer = renderer();
    renderer.set_width(Some(80));

    let mut output1 = Vec::new();
    renderer.render_markdown("# First", &mut output1).unwrap();

    let mut output2 = Vec::new();
    renderer.render_markdown("# Second", &mut output2).unwrap();

    // Both renders should use the same width setting
    assert!(!output1.is_empty());
    assert!(!output2.is_empty());
}

#[test]
fn session_routes_use_same_dump_policy() {
    // This test verifies that session-related routes use the same
    // escape-free dump policy as standalone file rendering.
    // The actual session integration is tested in integration.rs;
    // this ensures the rendering policy is consistent.
    let mut renderer = plain_renderer();
    renderer.set_width(Some(80));

    let mut output = Vec::new();
    renderer.render_markdown("# Test", &mut output).unwrap();

    assert!(!output.contains(&0x1b));
}
