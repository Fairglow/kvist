//! Integration tests for table rendering with wrapping and column width adaptation.

use blad::{MarkdownRenderer, RenderOptions};
use unicode_width::UnicodeWidthStr;

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
    s.width()
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

    // Data rows — no separator line: the header is distinguished by its
    // inverted background, so data starts immediately at index 1.
    assert!(lines[1].contains("Alice"));
    assert!(lines[2].contains("Bob"));

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
fn delimiter_alignment_applies_to_headers_and_body_in_styled_and_plain_output() {
    let markdown = "| L | C | R | U |\n| :--- | :---: | ---: | --- |\n| x | y | 7 | z |\n| 123456 | 123456 | 123456 | 123456 |\n| | y |";
    let expected = [
        ["L     ", "  C   ", "     R", "U     "].join(" | "),
        ["x     ", "  y   ", "     7", "z     "].join(" | "),
        ["123456"; 4].join(" | "),
        ["      ", "  y   ", "      ", "      "].join(" | "),
    ]
    .join("\n")
        + "\n\n";
    for width in [None, Some(0), Some(1), Some(80)] {
        for options in [RenderOptions::default(), RenderOptions::plain()] {
            let plain = options.plain;
            let mut renderer = MarkdownRenderer::with_options(options);
            renderer.set_width(width);
            let mut output = Vec::new();
            renderer.render_markdown(markdown, &mut output).unwrap();
            let output = String::from_utf8(output).unwrap();
            assert_eq!(strip_ansi(&output), expected, "width {width:?}");
            if plain {
                assert!(!output.contains('\x1b'));
            } else {
                assert!(output.starts_with("\x1b[7m"));
            }
        }
    }
}

#[test]
fn delimiter_alignment_measures_unicode_display_columns() {
    let output = strip_ansi(&render_width(
        "| L | C | R |\n| :--- | :---: | ---: |\n| 界 | e\u{301} | 👩‍💻 |\n| 123456 | 123456 | 123456 |",
        80,
    ));
    let row: Vec<_> = output.lines().nth(1).unwrap().split(" | ").collect();
    assert_eq!(row, ["界    ", "  e\u{301}   ", "    👩‍💻"]);
    assert!(
        output
            .lines()
            .filter(|line| !line.is_empty())
            .all(|line| line.width() == 24)
    );
}

#[test]
fn delimiter_alignment_preserves_explicit_and_empty_cell_lines() {
    let output = strip_ansi(&render_width(
        "| L | C | R |\n| :--- | :---: | ---: |\n| a<br><br/>b | c<br><br />d | e<br><BR>f |\n| 123456 | 123456 | 123456 |",
        80,
    ));
    let rows: Vec<_> = output.lines().collect();
    assert_eq!(rows[1], ["a     ", "  c   ", "     e"].join(" | "));
    assert_eq!(rows[2], ["      "; 3].join(" | "));
    assert_eq!(rows[3], ["b     ", "  d   ", "     f"].join(" | "));
    assert_eq!(rows[4], ["123456"; 3].join(" | "));
}

#[test]
fn delimiter_alignment_pads_each_wrapped_line_without_changing_geometry() {
    let markdown = "| Left | Center | Right |\n| :--- | :---: | ---: |\n| a b c d e f g h i j | one two three four five six seven eight | alpha beta gamma delta epsilon zeta |\n| x | y<br><br>z | 7 |";
    let unaligned = markdown
        .replace(":---:", "---")
        .replace(":---", "---")
        .replace("---:", "---");
    let natural_rows = strip_ansi(&render_width(markdown, 1000)).lines().count();
    for width in [0, 1, 30, 45, 80, 1000] {
        let baseline = strip_ansi(&render_width(&unaligned, width));
        let output = strip_ansi(&render_width(markdown, width));
        assert_eq!(output.lines().count(), baseline.lines().count());
        if width == 30 {
            assert!(output.lines().count() > natural_rows);
        }
        for (actual, reference) in output.lines().zip(baseline.lines()) {
            let expected = reference
                .split(" | ")
                .enumerate()
                .map(|(column, cell)| {
                    let text = cell.trim();
                    let spare = cell.width() - text.width();
                    let leading = match column {
                        1 => spare / 2,
                        2 => spare,
                        _ => 0,
                    };
                    format!(
                        "{}{text}{}",
                        " ".repeat(leading),
                        " ".repeat(spare - leading)
                    )
                })
                .collect::<Vec<_>>()
                .join(" | ");
            assert_eq!(actual, expected, "viewport {width}");
            assert_eq!(actual.width(), reference.width());
        }
    }
}

#[test]
fn delimiter_alignment_is_local_to_each_table() {
    let output = strip_ansi(&render_width(
        "| First |\n| ---: |\n| x |\n\n| Second |\n| --- |\n| y |",
        80,
    ));
    assert_eq!(output, "First\n    x\n\n\nSecond\ny     \n\n");
}

#[test]
fn test_table_empty_cells() {
    let md = "| Name | Age |\n|------|-----|\n| Alice |\n| Bob | 25 |";
    let output = strip_ansi(&render_width(md, 80));
    let lines: Vec<&str> = output.lines().collect();

    // No separator line: header at 0, data rows at 1 and 2. The header is
    // distinguished by its inverted background, not a border, so the table is
    // just header + N data rows (plus trailing blank from writeln).
    assert!(lines.len() >= 3);

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

    // No separator line: header at 0, data rows at 1 and 2.
    assert!(lines[0].contains("Col1"));
    assert!(lines[0].contains("Col2"));
    assert!(lines[2].contains("A"));
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
    let second_column = lines
        .iter()
        .filter_map(|line| line.split_once(" | ").map(|(_, cell)| cell.trim()))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(second_column.contains("This cell has a very long value that should also wrap"));
}

#[test]
fn test_table_no_wrap_when_fits() {
    let md = "| A | B |\n|---|---|\n| 1 | 2 |";
    let width = 80;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    // No separator line: header + 1 data row + trailing blank = 3 lines.
    assert!(lines.len() >= 3);
}

#[test]
fn test_table_wrapped_rows_have_no_internal_separators() {
    let md = "| Item | Description |\n|------|-------------|\n| A | This is a very long description that will wrap to multiple lines when the table is rendered |\n| B | Short |";
    let width = 35;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    // A wrapped cell spans multiple lines within the SAME cell, so it must not
    // be broken up by internal row borders. No separator lines at all: the
    // header is distinguished by its inverted background and the data rows by
    // the zebra tint, so borders are unnecessary even when cells wrap.
    let separator_count = lines.iter().filter(|l| l.starts_with("─")).count();
    assert_eq!(
        separator_count, 0,
        "No borders are drawn, so no separator lines should appear. Output: {:?}",
        output
    );

    assert!(lines.iter().all(|line| display_width(line) <= width));
}

#[test]
fn test_table_column_min_width_ratio() {
    let md = "| A | B |\n|---|---|\n| x | This is a long description that will wrap to multiple lines when rendered |";
    for width in [0, 1, 2, 10, 30, 40, 80, usize::MAX] {
        let output = strip_ansi(&render_width(md, width));
        let lines: Vec<_> = output.lines().filter(|line| !line.is_empty()).collect();
        let widths: Vec<_> = lines[0].split(" | ").map(display_width).collect();
        let height = lines.len() - 1;
        assert_eq!(widths.len(), 2);
        assert_eq!(widths[0], 1, "{output}");
        assert!(widths[1] >= 4 * height, "{output}");
        assert!(lines.iter().all(|line| line.width() == lines[0].width()));
    }
}

#[test]
fn test_table_long_words_remain_whole_even_when_the_table_overflows() {
    let md = "| Item | Description |\n|------|-------------|\n| A | ThisIsAVeryLongWordWithoutSpaces |\n| B | Normal text |";
    let width = 30;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    let long_word = "ThisIsAVeryLongWordWithoutSpaces";
    assert!(output.contains(long_word), "{output}");
    assert!(lines.iter().any(|line| display_width(line) > width));
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

    // No border between header and body: the header is distinguished by its
    // inverted background, so the body starts immediately at index 1.
    assert!(
        lines[1].contains("Alice"),
        "Header must be followed directly by the body"
    );
    assert!(!lines[1].contains("─"), "No border between header and body");
}

#[test]
fn test_table_no_extra_separator_when_not_wrapped() {
    let md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
    let output = strip_ansi(&render_width(md, 80));
    let lines: Vec<&str> = output.lines().collect();

    // No separator line: header + 2 data rows + trailing blank = 3 content
    // lines. The header and body are distinguished by background, not a
    // border, so the line count drops by one versus the old layout.
    assert!(lines.len() >= 3);

    // No borders at all: the inverted header and zebra tint replace the
    // separator line entirely.
    let separator_count = lines.iter().filter(|l| l.starts_with("─")).count();
    assert_eq!(
        separator_count, 0,
        "No borders are drawn, so no separator lines should appear, got {}",
        separator_count
    );
}

#[test]
fn test_table_long_content_gets_space_without_splitting_file_tokens() {
    let md = "| Commit | Files | What it does |\n|--------|-------|--------------|\n| 826f281 Make cargo clippy resolve the vendored snap | sav/src/offline_rust.rs, skott/tests/rust_build_environment.rs, SANDBOX_ENVIRONMENT.md | Fixes the cargo-clippy wrapper so it strips the clippy subcommand token cargo delegates and injects the offline and locked flags plus the vendored-source config. |";
    let width = 80;
    let output = strip_ansi(&render_width(md, width));
    let lines: Vec<&str> = output.lines().collect();

    // Column widths from the header.
    let first = lines[0];
    let cols: Vec<usize> = first.split(" | ").map(|s| s.chars().count()).collect();
    let (w_commit, w_files, w_desc) = (cols[0], cols[1], cols[2]);
    assert!(
        w_desc > w_commit && w_files >= "skott/tests/rust_build_environment.rs,".width(),
        "Paragraphs need space and file tokens stay whole: commit={w_commit} files={w_files} desc={w_desc}"
    );

    // No internal borders: no separator lines at all, even though the
    // wrapped cells span multiple lines. The inverted header and zebra tint
    // carry the visual separation instead.
    let separator_count = lines.iter().filter(|l| l.starts_with("─")).count();
    assert_eq!(
        separator_count, 0,
        "No borders are drawn, so no separator lines should appear, got {}",
        separator_count
    );

    // A long file token can force overflow; no line loses alignment.
    assert!(
        lines
            .iter()
            .filter(|line| !line.is_empty())
            .all(|line| { display_width(line) == display_width(first) })
    );
}

#[test]
fn short_and_empty_columns_never_expand_beyond_their_text() {
    let output = strip_ansi(&render_width("| A | |\n|---|---|\n| | x |", 1));
    assert_eq!(output, "A |  \n  | x\n\n");
    let empty = strip_ansi(&render_width("| A | |\n|---|---|\n| x | |", 100));
    assert_eq!(empty, "A | \nx | \n\n");
}

#[test]
fn unicode_words_remain_intact_and_columns_use_display_width() {
    let md = "| A | B |\n|---|---|\n| e\u{301} 👩‍💻 | 世界 世界 世界 世界 世界 |";
    let output = strip_ansi(&render_width(md, 25));
    let lines: Vec<_> = output.lines().filter(|line| !line.is_empty()).collect();
    assert!(output.contains("e\u{301} 👩‍💻"));
    assert!(output.contains("世界"));
    let widths: Vec<_> = lines[0].split(" | ").map(display_width).collect();
    assert_eq!(widths[0], "e\u{301} 👩‍💻".width());
    assert!(widths[1] >= 4 * (lines.len() - 1));
    assert!(lines.iter().all(|line| line.width() == lines[0].width()));
}

#[test]
fn resizing_resolves_height_instead_of_scaling_below_the_floor() {
    let md =
        "| A | B |\n|---|---|\n| a b c d e f | one two three four five six seven eight nine ten |";
    let narrow = strip_ansi(&render_width(md, 10));
    let wide = strip_ansi(&render_width(md, 100));
    assert!(narrow.lines().count() > wide.lines().count());
    assert!(narrow.lines().next().unwrap().width() > 10);
    assert!(wide.lines().all(|line| line.width() <= 100));
    assert_eq!(wide, strip_ansi(&render_width(md, 100)));
}

#[test]
fn table_writer_failures_are_propagated() {
    struct BrokenWriter;
    impl std::io::Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("table output failed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let error = MarkdownRenderer::new()
        .render_markdown("| A |\n|---|\n| x |", &mut BrokenWriter)
        .unwrap_err();
    assert_eq!(error.to_string(), "table output failed");
}

#[test]
fn explicit_cell_breaks_preserve_empty_lines_without_inflating_widths() {
    let output = strip_ansi(&render_width(
        "| A | B |\n|---|---|\n| a<br><br />b<BR/> | x |",
        10,
    ));
    let lines: Vec<_> = output.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(lines.len(), 5, "{output}");
    let widths: Vec<_> = lines[0].split(" | ").map(display_width).collect();
    assert_eq!(widths, [1, 1]);
    assert!(lines[1].starts_with("a "));
    assert!(lines[2].split_once(" | ").unwrap().0.trim().is_empty());
    assert!(lines[3].starts_with("b "));
    assert!(lines[4].split_once(" | ").unwrap().0.trim().is_empty());
}

#[test]
fn component_model_columns_are_capped_at_their_text_widths() {
    let markdown = include_str!("fixtures/component_model.md");
    let mut natural = vec![0; 5];
    for (row, line) in markdown.lines().enumerate() {
        if row == 1 {
            continue;
        }
        for (column, text) in line.trim_matches('|').split('|').enumerate() {
            let text = text.replace('`', "");
            let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
            natural[column] = natural[column].max(normalized.width());
        }
    }
    for viewport in [0, 1, 80, 120, 160, 240, 1000, usize::MAX] {
        let output = strip_ansi(&render_width(markdown, viewport));
        let widths: Vec<_> = output
            .lines()
            .next()
            .unwrap()
            .split(" | ")
            .map(display_width)
            .collect();
        assert_eq!(&widths[..2], &natural[..2], "viewport {viewport}: {output}");
        assert!(widths.iter().zip(&natural).all(|(width, max)| width <= max));
        assert!(
            output.lines().filter(|line| !line.is_empty()).all(|line| {
                line.width() == widths.iter().sum::<usize>() + 3 * (widths.len() - 1)
            })
        );
        assert!(output.contains("kvist.maerg"));
        assert!(output.contains("maerg/"));
    }
}
