//! Markdown rendering with syntax highlighting for terminal output.
//!
//! Parses markdown using pulldown-cmark and applies syntax highlighting to
//! fenced code blocks using syntect. Tables are rendered with manual alignment.
//! Outputs ANSI-colored text to the terminal.
//!
//! Supported GitHub-flavored Markdown extensions:
//! - Tables
//! - Strikethrough
//! - Task lists
//! - Fenced code blocks with language tags
//! - Autolinks

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::io::Write;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SColor, FontStyle, Style as SStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Render markdown with syntax-highlighted code blocks to the terminal.
/// If terminal_width is provided, tables will be wrapped to fit within that width.
pub fn render_markdown(md: &str, w: &mut impl Write) -> std::io::Result<()> {
    render_markdown_width(md, w, None)
}

pub fn render_markdown_width(
    md: &str,
    w: &mut impl Write,
    terminal_width: Option<usize>,
) -> std::io::Result<()> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = get_terminal_theme(&ts);

    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_GFM;

    let parser = Parser::new_ext(md, options);

    let mut in_code_block = false;
    let mut code_language = String::new();
    let mut code_content = String::new();
    let mut hl: Option<HighlightLines> = None;

    let mut bold = false;
    let mut italic = false;
    let mut strikethrough = false;

    let mut list_item_count = 0;
    let mut list_level: usize = 0;

    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_row_cells: Vec<String> = Vec::new();

    let mut at_start = true;
    let mut need_blank = false;

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                }
                in_code_block = true;
                code_content.clear();
                match kind {
                    CodeBlockKind::Fenced(s) => code_language = s.to_string(),
                    CodeBlockKind::Indented => code_language.clear(),
                }
                if code_language == "mermaid" {
                    // Mermaid diagrams: collect content, render later
                    // Try to use mmdc if available, fallback to plain text
                } else {
                    let syntax = find_syntax(&ss, &code_language);
                    hl = Some(HighlightLines::new(syntax, &theme));
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                if code_language == "mermaid" {
                    // Try to render mermaid diagram
                    let result = render_mermaid(&code_content);
                    match result {
                        Ok(ascii) => {
                            writeln!(w, "{}", ascii)?;
                        }
                        Err(_) => {
                            // Fallback: show as plain text
                            writeln!(w, "Mermaid diagram:")?;
                            for line in code_content.lines() {
                                writeln!(w, "    {}", line)?;
                            }
                        }
                    }
                    writeln!(w)?;
                } else if let Some(mut highlighter) = hl.take() {
                    for line in code_content.lines() {
                        if let Ok(ranges) = highlighter.highlight_line(line, &ss) {
                            let styled = to_ansi_line(&ranges);
                            writeln!(w, "{}", styled)?;
                        } else {
                            writeln!(w, "{}", line)?;
                        }
                    }
                    writeln!(w)?;
                }
            }
            Event::Start(Tag::Strong) => {
                bold = true;
            }
            Event::End(TagEnd::Strong) => {
                bold = false;
            }
            Event::Start(Tag::Emphasis) => {
                italic = true;
            }
            Event::End(TagEnd::Emphasis) => {
                italic = false;
            }
            Event::Start(Tag::Strikethrough) => {
                strikethrough = true;
            }
            Event::End(TagEnd::Strikethrough) => {
                strikethrough = false;
            }
            Event::Text(text) => {
                if in_code_block {
                    code_content.push_str(&text);
                } else if in_table {
                    let cleaned = text.replace('\n', " ").replace('\r', "");
                    if let Some(cell) = table_row_cells.last_mut()
                        && !cleaned.is_empty()
                    {
                        if !cell.is_empty() {
                            cell.push(' ');
                        }
                        cell.push_str(&cleaned);
                    }
                } else {
                    write_emphasized(w, bold, italic, strikethrough, &text)?;
                }
            }
            Event::Code(text) => {
                if in_table {
                    if let Some(cell) = table_row_cells.last_mut() {
                        if !cell.is_empty() {
                            cell.push(' ');
                        }
                        cell.push_str(&text);
                    }
                } else {
                    // Inline code: use reverse video but preserve surrounding formatting
                    // Use \x1b[27m to turn off reverse video without resetting other attributes
                    write!(w, "\x1b[7m")?;
                    write_emphasized(w, bold, italic, strikethrough, &text)?;
                    write!(w, "\x1b[27m")?;
                }
            }
            Event::End(TagEnd::Paragraph) => {
                writeln!(w)?;
                need_blank = true;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                } else if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
                let prefix = match level {
                    HeadingLevel::H1 => "# ",
                    HeadingLevel::H2 => "## ",
                    HeadingLevel::H3 => "### ",
                    HeadingLevel::H4 => "#### ",
                    HeadingLevel::H5 => "##### ",
                    HeadingLevel::H6 => "###### ",
                };
                write!(w, "\x1b[1m\x1b[34m{}", prefix)?;
            }
            Event::End(TagEnd::Heading(_)) => {
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
                need_blank = true;
            }
            Event::SoftBreak => {
                write!(w, " ")?;
            }
            Event::HardBreak => {
                writeln!(w)?;
            }
            Event::Start(Tag::List(_)) => {
                if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                } else if !at_start {
                    writeln!(w)?;
                }
                list_level += 1;
            }
            Event::End(TagEnd::List(_)) => {
                list_level = list_level.saturating_sub(1);
                writeln!(w)?;
            }
            Event::Start(Tag::Item) => {
                list_item_count += 1;
                let indent = "  ".repeat(list_level - 1);
                if list_item_count > 1 {
                    write!(w, "\r\n")?;
                }
                write!(w, "{}\x1b[33m•\x1b[0m ", indent)?;
            }
            Event::End(TagEnd::Item) => {}
            Event::Rule => {
                if !at_start {
                    writeln!(w)?;
                }
                write!(w, "\x1b[90m")?;
                for _ in 0..80 {
                    write!(w, "─")?;
                }
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
                need_blank = true;
            }
            Event::Start(Tag::Table(_)) => {
                if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                } else if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
                in_table = true;
                table_rows.clear();
                table_row_cells.clear();
            }
            Event::End(TagEnd::Table) => {
                in_table = false;
                render_table(w, &table_rows, terminal_width)?;
                writeln!(w)?;
                need_blank = true;
                table_rows.clear();
                table_row_cells.clear();
            }
            Event::Start(Tag::TableRow) => {
                table_row_cells.clear();
            }
            Event::Start(Tag::TableHead) => {
                table_row_cells.clear();
            }
            Event::End(TagEnd::TableRow) | Event::End(TagEnd::TableHead) => {
                if !table_row_cells.is_empty() {
                    table_rows.push(table_row_cells.clone());
                }
                table_row_cells = Vec::new();
            }
            Event::Start(Tag::TableCell) => {
                table_row_cells.push(String::new());
            }
            Event::Start(Tag::BlockQuote(Some(kind))) => {
                if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
                let (color, label) = match kind {
                    pulldown_cmark::BlockQuoteKind::Note => ("\x1b[96m", "NOTE"),
                    pulldown_cmark::BlockQuoteKind::Tip => ("\x1b[92m", "TIP"),
                    pulldown_cmark::BlockQuoteKind::Important => ("\x1b[95m", "IMPORTANT"),
                    pulldown_cmark::BlockQuoteKind::Warning => ("\x1b[93m", "WARNING"),
                    pulldown_cmark::BlockQuoteKind::Caution => ("\x1b[91m", "CAUTION"),
                };
                write!(w, "{}[{}] ", color, label)?;
            }
            Event::Start(Tag::Superscript) => {
                write!(w, "\x1b[2m")?;
            }
            Event::End(TagEnd::Superscript) => {
                write!(w, "\x1b[0m")?;
            }
            Event::Start(Tag::Subscript) => {
                write!(w, "\x1b[2m")?;
            }
            Event::End(TagEnd::Subscript) => {
                write!(w, "\x1b[0m")?;
            }
            Event::FootnoteReference(id) => {
                write!(w, "\x1b[96m[{}\x1b[0m]", id)?;
            }
            Event::Start(Tag::FootnoteDefinition(_)) => {
                if !at_start {
                    writeln!(w)?;
                }
                write!(w, "\x1b[90m")?;
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
            }
            Event::InlineMath(tex) => {
                write!(w, "\x1b[95m${}\x1b[0m", tex)?;
            }
            Event::DisplayMath(tex) => {
                if !at_start {
                    writeln!(w)?;
                }
                write!(w, "\x1b[95m${} $\x1b[0m", tex)?;
                writeln!(w)?;
                need_blank = true;
            }
            Event::Start(Tag::Paragraph) => {
                if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                } else if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
            }
            Event::Start(Tag::BlockQuote(_)) => {
                if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                } else if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
                write!(w, "\x1b[90m")?;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
                need_blank = true;
            }
            _ => {}
        }
    }

    Ok(())
}

/// Render a table with proper column alignment and soft-wrapping.
/// Column widths are adapted to fit the terminal width while preserving
/// readability. Cells wrap to multiple lines when needed.
fn render_table(
    w: &mut impl Write,
    rows: &[Vec<String>],
    terminal_width: Option<usize>,
) -> std::io::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let num_cols = rows[0].len();

    // Determine natural column widths based on content
    let mut col_widths = vec![0; num_cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < num_cols {
                let width = cell.chars().count();
                if width > col_widths[i] {
                    col_widths[i] = width;
                }
            }
        }
    }

    // If terminal width is provided, adapt column widths to fit
    if let Some(term_width) = terminal_width {
        let separator_space = (num_cols - 1) * 2;
        let available_width = term_width.saturating_sub(separator_space);
        let total_natural: usize = col_widths.iter().sum();

        if total_natural > available_width {
            // Need to shrink columns. Use iterative proportional fit
            // with minimum width constraints.
            let min_width = 3;
            let mut remaining = available_width;
            let mut fitted = vec![false; num_cols];
            let mut iterations = 0;

            loop {
                iterations += 1;
                if iterations > num_cols * 2 {
                    break;
                }

                // Calculate unfitted total
                let unfitted_total: usize = col_widths
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !fitted[*i])
                    .map(|(_, w)| *w)
                    .sum();
                let unfitted_count = fitted.iter().filter(|&f| !f).count();

                if unfitted_count == 0 || remaining == 0 {
                    break;
                }

                // Try to fit remaining unfitted columns
                let mut any_fitted = false;
                for i in 0..num_cols {
                    if fitted[i] {
                        continue;
                    }

                    // Calculate proportional share
                    let share = if unfitted_total > 0 {
                        ((col_widths[i] as f64) / (unfitted_total as f64)) * remaining as f64
                    } else {
                        (remaining as f64) / (unfitted_count as f64)
                    };
                    let fitted_width = share.ceil() as usize;

                    if fitted_width < min_width {
                        // Can't fit this column with minimum width
                        fitted[i] = true;
                        col_widths[i] = min_width;
                        remaining = remaining.saturating_sub(min_width);
                        any_fitted = true;
                    } else if fitted_width >= col_widths[i] {
                        // Column fits at natural width - keep it
                        fitted[i] = true;
                        remaining = remaining.saturating_sub(col_widths[i]);
                        any_fitted = true;
                    } else {
                        // Column needs to be shrunk
                        fitted[i] = true;
                        col_widths[i] = fitted_width;
                        remaining = remaining.saturating_sub(fitted_width);
                        any_fitted = true;
                    }
                }

                if !any_fitted {
                    break;
                }
            }

            // If we still have remaining space, distribute it proportionally
            if remaining > 0 {
                let unfitted_total: usize = col_widths
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !fitted[*i])
                    .map(|(_, w)| *w)
                    .sum();
                if unfitted_total > 0 {
                    for i in 0..num_cols {
                        if !fitted[i] {
                            let share = ((col_widths[i] as f64) / (unfitted_total as f64))
                                * remaining as f64;
                            col_widths[i] += share.ceil() as usize;
                            fitted[i] = true;
                        }
                    }
                }
            }

            // Any still-unfitted columns get minimum width (will overflow)
            for i in 0..num_cols {
                if !fitted[i] {
                    col_widths[i] = min_width;
                }
            }
        }
    }

    // Render all rows with proper alignment and soft-wrapping
    for (row_idx, row) in rows.iter().enumerate() {
        // First, wrap all cells in this row
        let mut wrapped_row: Vec<Vec<String>> = Vec::new();
        for (i, cell) in row.iter().enumerate() {
            let wrapped = wrap_cell(cell, col_widths[i]);
            wrapped_row.push(wrapped);
        }

        // Find the maximum number of wrapped lines in this row
        let max_lines = wrapped_row
            .iter()
            .map(|lines| lines.len())
            .max()
            .unwrap_or(1);

        // Render each line of the wrapped row
        for line_idx in 0..max_lines {
            for (i, wrapped_lines) in wrapped_row.iter().enumerate() {
                if i > 0 {
                    write!(w, " | ")?;
                }
                if line_idx < wrapped_lines.len() {
                    let padded = pad_cell(&wrapped_lines[line_idx], col_widths[i]);
                    write!(w, "{}", padded)?;
                } else {
                    // Pad with spaces for shorter cells
                    write!(w, "{}", " ".repeat(col_widths[i]))?;
                }
            }
            writeln!(w)?;
        }

        // Separator after header row (dashes)
        if row_idx == 0 {
            for (i, &width) in col_widths.iter().enumerate() {
                if i > 0 {
                    write!(w, " | ")?;
                }
                write!(w, "{}", "-".repeat(width))?;
            }
            writeln!(w)?;
        }
    }

    Ok(())
}

/// Wrap a cell to fit within the given width.
fn wrap_cell(cell: &str, width: usize) -> Vec<String> {
    if cell.chars().count() <= width {
        return vec![cell.to_string()];
    }

    let mut result = Vec::new();
    let mut current = String::new();

    for word in cell.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            result.push(current);
            current = word.to_string();
        }
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

/// Pad a cell to the right width.
fn pad_cell(cell: &str, width: usize) -> String {
    let cell_len = cell.chars().count();
    if cell_len >= width {
        cell.to_string()
    } else {
        format!("{}{}", cell, " ".repeat(width - cell_len))
    }
}

fn write_emphasized(
    w: &mut impl Write,
    bold: bool,
    italic: bool,
    strikethrough: bool,
    text: &str,
) -> std::io::Result<()> {
    let mut escape = String::new();
    if bold {
        escape.push_str("\x1b[1m");
    }
    if italic {
        escape.push_str("\x1b[3m");
    }
    if strikethrough {
        escape.push_str("\x1b[9m");
    }
    if !escape.is_empty() {
        write!(w, "{}{}\x1b[0m", escape, text)?;
    } else {
        write!(w, "{}", text)?;
    }
    Ok(())
}

fn find_syntax<'a>(ss: &'a SyntaxSet, lang: &str) -> &'a syntect::parsing::SyntaxReference {
    let lang = lang.trim();
    if lang.is_empty() {
        return ss
            .find_syntax_by_name("Plain Text")
            .unwrap_or(ss.syntaxes().first().unwrap());
    }

    let aliases = [
        ("rs", "Rust"),
        ("rust", "Rust"),
        ("py", "Python"),
        ("python", "Python"),
        ("js", "JavaScript"),
        ("javascript", "JavaScript"),
        ("ts", "TypeScript"),
        ("typescript", "TypeScript"),
        ("sh", "Bash"),
        ("bash", "Bash"),
        ("zsh", "Bash"),
        ("hemlock", "Hemlock"),
    ];

    for (alias, name) in &aliases {
        if lang == *alias
            && let Some(s) = ss.find_syntax_by_name(name)
        {
            return s;
        }
    }

    if let Some(s) = ss.find_syntax_by_name(lang) {
        return s;
    }

    if let Some(s) = ss.find_syntax_by_extension(lang) {
        return s;
    }

    let upper = lang.to_uppercase();
    if let Some(s) = ss.find_syntax_by_name(&upper) {
        return s;
    }

    ss.find_syntax_by_name("Plain Text")
        .unwrap_or(ss.syntaxes().first().unwrap())
}

fn get_terminal_theme(ts: &ThemeSet) -> Theme {
    let candidates = [
        "base16-ocean.dark",
        "base16-default.dark",
        "solarized-dark",
        "monokai",
        "gruvbox-dark",
    ];

    for name in candidates {
        if let Some(theme) = ts.themes.get(name) {
            return theme.clone();
        }
    }

    ts.themes.values().next().cloned().unwrap_or_else(|| {
        ThemeSet::load_defaults()
            .themes
            .values()
            .next()
            .cloned()
            .unwrap()
    })
}

/// Try to render a mermaid diagram to ASCII using mmdc.
fn render_mermaid(content: &str) -> std::io::Result<String> {
    // Try to use mmdc (mermaid-cli) if available
    let output = std::process::Command::new("mmdc")
        .args(["-i", "-", "-o", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn();

    match output {
        Ok(mut child) => {
            // Write mermaid content to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin.write_all(content.as_bytes())?;
            }

            // Read output
            let result = child.wait_with_output()?;
            if result.status.success() {
                let _svg = String::from_utf8_lossy(&result.stdout).to_string();
                // For now, just return a placeholder since we can't easily
                // convert SVG to ASCII in Rust without dependencies
                Ok("[Mermaid diagram rendered via mmdc]".to_string())
            } else {
                Err(std::io::Error::other(format!(
                    "mmdc failed: {}",
                    String::from_utf8_lossy(&result.stderr)
                )))
            }
        }
        Err(e) => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("mmdc not found: {}", e),
        )),
    }
}

fn syntect_color_to_ansi(c: SColor) -> String {
    format!("\x1b[38;2;{};{};{}m", c.r, c.g, c.b)
}

fn syntect_bg_color_to_ansi(c: SColor) -> String {
    format!("\x1b[48;2;{};{};{}m", c.r, c.g, c.b)
}

fn to_ansi_line(ranges: &[(SStyle, &str)]) -> String {
    let mut output = String::new();
    let mut current_style: Option<SStyle> = None;

    for (style, text) in ranges {
        if current_style != Some(*style) {
            if current_style.is_some() {
                output.push_str("\x1b[0m");
            }
            let fg = style.foreground;
            output.push_str(&syntect_color_to_ansi(fg));
            let bg = style.background;
            output.push_str(&syntect_bg_color_to_ansi(bg));
            if style.font_style.contains(FontStyle::BOLD) {
                output.push_str("\x1b[1m");
            }
            if style.font_style.contains(FontStyle::ITALIC) {
                output.push_str("\x1b[3m");
            }
            if style.font_style.contains(FontStyle::UNDERLINE) {
                output.push_str("\x1b[4m");
            }
            current_style = Some(*style);
        }
        output.push_str(text);
    }

    if current_style.is_some() {
        output.push_str("\x1b[0m");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(md: &str) -> String {
        let mut buf = Vec::new();
        render_markdown(md, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn test_render_basic() {
        let md = "# Hello\n\nThis is **bold** text.";
        let output = render(md);
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
        let output = render(md);
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
        let output = render(md);
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
        let output = render(md);
        // First line should be the heading, not blank
        let lines: Vec<&str> = output.lines().collect();
        assert!(!lines.is_empty());
        assert!(lines[0].contains("# First Heading"));
    }

    #[test]
    fn test_rendered_starts_at_line_one() {
        // Verify rendered output starts at line 1 with content, not blank lines
        let md = "# Title\n\nBody paragraph.\n\n- Item 1\n- Item 2\n\n| Col | Val |\n|-----|-----|\n| A | 1 |";
        let output = render(md);
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
}
