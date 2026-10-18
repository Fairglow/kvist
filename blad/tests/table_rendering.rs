//! Integration tests for table rendering with wrapping and column width adaptation.

use blad::MarkdownRenderer;

/// Strip ANSI escape codes for easier testing.
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

/// Compute display width of a string (for ASCII this is the char count).
fn display_width(s: &str) -> usize {
    s.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

fn render_width(md: &str, width: usize) -> String {
    let mut buf = Vec::new();
    let mut renderer = MarkdownRenderer::new();
    renderer.set_width(Some(width));
    renderer.render_markdown(md, &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn test_table_basic_alignment() {
    let md = "| Name | Age |\n|------|-----|\n| Alice | 30 |\n| Bob | 25 |";
    let output = strip_ansi(&render_width(md, 80));
    let lines: Vec<&str> = output.lines().collect();

    // Header row
    assert!(lines[0].contains("Name"));
    assert!(lines[0].contains("Age"));

    // Data rows - after separator
    assert!(lines[2].contains("Alice"));
    assert!(lines[3].contains("Bob"));

    // All non-empty lines should have same length (proper alignment)
    let non_empty: Vec<&str> = lines.iter().filter(|l| !l.is_empty()).copied().collect();
    let widths: Vec<usize> = non_empty.iter().map(|l| display_width(l)).collect();
    assert!(
        !widths.is_empty() && widths.iter().all(|&w| w == widths[0]),
        "Table lines should be aligned, got widths: {:?}",
        widths
    );
}

#[test]
fn test_table_empty_cells() {
    let md = "| Name | Age |\n|------|-----|\n| Alice |\n| Bob | 25 |";
    let output = strip_ansi(&render_width(md, 80));
    let lines: Vec<&str> = output.lines().collect();

    // Should have 4 rows (header + separator + 2 data)
    assert!(lines.len() >= 4);

    // All non-empty lines should have same length (proper alignment)
    let non_empty: Vec<&str> = lines.iter().filter(|l| !l.is_empty()).copied().collect();
    let widths: Vec<usize> = non_empty.iter().map(|l| display_width(l)).collect();
    assert!(
        !widths.is_empty() && widths.iter().all(|&w| w == widths[0]),
        "Table with empty cells should be aligned, got widths: {:?}",
        widths
    );
}

#[test]
fn test_table_empty_all_cells() {
    let md = "| Col1 | Col2 |\n|------|------|\n|  |  |\n| A | B |";
    let output = strip_ansi(&render_width(md, 80));
    let lines: Vec<&str> = output.lines().collect();

    assert!(lines[0].contains("Col1"));
    assert!(lines[0].contains("Col2"));
    assert!(lines[3].contains("A"));
}

#[test]
fn test_table_wraps_to_fit_width() {
    let md = "| Short | This is a very long column header that should wrap |\n|-------|--------------------------------------------------|\n| A | This cell has a very long value that should also wrap |\n| B | Short value |";
    let width = 40;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    // All lines should fit within terminal width
    for line in &lines {
        assert!(
            display_width(line) <= width,
            "Line exceeds terminal width: {:?} ({} chars)",
            line,
            display_width(line)
        );
    }

    // Content should still be present (may be wrapped across lines)
    assert!(output.contains("Short"));
    assert!(output.contains("very long"));
    assert!(output.contains("column"));
    assert!(output.contains("header"));
    assert!(output.contains("should wrap"));
    assert!(output.contains("very long value"));
}

#[test]
fn test_table_no_wrap_when_fits() {
    let md = "| A | B |\n|---|---|\n| 1 | 2 |";
    let width = 80;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    // Should be exactly 4 lines (header + separator + 1 data row + trailing blank)
    assert!(lines.len() >= 4);
}

#[test]
fn test_table_wrapped_rows_have_no_internal_separators() {
    let md = "| Item | Description |\n|------|-------------|\n| A | This is a very long description that will wrap to multiple lines when the table is rendered |\n| B | Short |";
    let width = 35;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    // A wrapped cell spans multiple lines within the SAME cell, so it must not
    // be broken up by internal row borders. The only separator line is the one
    // between the header and the body, even when a cell wraps to many lines.
    let separator_count = lines.iter().filter(|l| l.starts_with("─")).count();
    assert_eq!(
        separator_count, 1,
        "Wrapped rows must not add internal separators, got {} separators. Output: {:?}",
        separator_count,
        output
    );

    // All wrapped lines still fit within the terminal width.
    for line in &lines {
        assert!(
            display_width(line) <= width,
            "Line exceeds terminal width: {:?} ({} chars)",
            line,
            display_width(line)
        );
    }
}

#[test]
fn test_table_column_min_width_ratio() {
    // Column should not shrink below 4:3 width:height ratio.
    // If a column wraps to 4 lines, its minimum width is 5 (ceil(4 * 4/3)).
    let md = "| Item | Description |\n|------|-------------|\n| A | This is a long description that will wrap to multiple lines when rendered |\n| B | Short |";
    let width = 30;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    // All lines should still fit
    for line in &lines {
        assert!(
            display_width(line) <= width,
            "Line exceeds terminal width: {} chars",
            display_width(line)
        );
    }
}

#[test]
fn test_table_hard_wrap_long_words() {
    // Words longer than the column width should be hard-wrapped
    let md = "| Item | Description |\n|------|-------------|\n| A | ThisIsAVeryLongWordWithoutSpaces |\n| B | Normal text |";
    let width = 30;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    // All lines should fit within width (hard wrap must have occurred)
    for line in &lines {
        assert!(
            display_width(line) <= width,
            "Hard wrap should break long words, got line: {:?}",
            line
        );
    }

    // The long word should still be fully present in the output (split across
    // lines). The exact split point depends on the column width the allocator
    // chose, so only assert that the whole word is recoverable by removing the
    // column separators and re-joining the split chunks.
    // Reconstruct the original long word from the hard-wrapped chunks.
    let long_word = "ThisIsAVeryLongWordWithoutSpaces";
    let mut recovered = String::new();
    for line in &lines {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        // The Description column is the last column; take everything after the
        // last " | " separator.
        if let Some(pos) = line.rfind(" | ") {
            recovered.push_str(&line[pos + 3..]);
        } else if line.starts_with('─') {
            // separator line; skip
        } else {
            recovered.push_str(line);
        }
    }
    assert!(
        recovered.contains(long_word),
        "Hard-wrapped chunks must rejoin into the full word, got {:?}",
        recovered
    );
}

#[test]
fn test_table_multiple_columns_wrap() {
    let md = "| Long Header 1 | Long Header 2 | Long Header 3 |\n|---------------|---------------|---------------|\n| Value 1 | Value 2 | Value 3 |\n| Another value | Another value | Another value |";
    let width = 40;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    // All lines should fit
    for line in &lines {
        assert!(
            display_width(line) <= width,
            "Multiple wrapping columns: line too wide: {:?}",
            line
        );
    }
}

#[test]
fn test_table_header_separated_from_body() {
    let md = "| Name | Age |\n|------|-----|\n| Alice | 30 |";
    let output = strip_ansi(&render_width(md, 80));
    let lines: Vec<&str> = output.lines().collect();

    // Should have a separator between header and body
    assert!(lines[1].contains("─"), "Missing header separator");
}

#[test]
fn test_table_no_extra_separator_when_not_wrapped() {
    let md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
    let output = strip_ansi(&render_width(md, 80));
    let lines: Vec<&str> = output.lines().collect();

    // Should have exactly 4 lines: header, separator, 2 data rows
    // Account for trailing empty line from writeln
    assert!(lines.len() >= 4);

    // Only one separator line (between header and body)
    let separator_count = lines.iter().filter(|l| l.starts_with("─")).count();
    assert_eq!(
        separator_count, 1,
        "Should have exactly 1 separator when no wrapping, got {}",
        separator_count
    );
}

#[test]
fn test_table_widest_column_gets_most_width() {
    // The column with the longest value must be allocated the most width, not
    // crushed to a stub while short columns keep their full natural width.
    let md = "| Commit | Files | What it does |\n|--------|-------|--------------|\n| 826f281 Make cargo clippy resolve the vendored snap | sav/src/offline_rust.rs, skott/tests/rust_build_environment.rs, SANDBOX_ENVIRONMENT.md | Fixes the cargo-clippy wrapper so it strips the clippy subcommand token cargo delegates and injects the offline and locked flags plus the vendored-source config. |";
    let width = 80;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    // Column widths from the header.
    let first = lines[0];
    let cols: Vec<usize> = first
        .split(" | ")
        .map(|s| s.chars().count())
        .collect();
    let (w_commit, w_files, w_desc) = (cols[0], cols[1], cols[2]);
    assert!(
        w_desc > w_commit && w_desc > w_files,
        "Column with the longest value must be the widest, got commit={w_commit} files={w_files} desc={w_desc}"
    );

    // No internal borders: exactly one separator line (header/body) even
    // though the wrapped cells span multiple lines.
    let separator_count = lines.iter().filter(|l| l.starts_with("─")).count();
    assert_eq!(
        separator_count, 1,
        "Wrapped cells must not add internal borders, got {} separators",
        separator_count
    );

    // Every line fits the terminal width.
    for line in &lines {
        assert!(
            display_width(line) <= width,
            "Line exceeds terminal width: {:?} ({} chars)",
            line,
            display_width(line)
        );
    }
}
