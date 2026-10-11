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
pub fn render_markdown(md: &str, w: &mut impl Write) -> std::io::Result<()> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = get_terminal_theme(&ts);

    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES;

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
                let syntax = find_syntax(&ss, &code_language);
                hl = Some(HighlightLines::new(syntax, &theme));
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                if let Some(mut highlighter) = hl.take() {
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
                    write_emphasized(
                        w,
                        bold,
                        italic,
                        strikethrough,
                        &format!("\x1b[7m{}\x1b[0m", text),
                    )?;
                }
            }
            Event::End(TagEnd::Paragraph) => {
                writeln!(w)?;
                need_blank = true;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
                need_blank = false;
                let prefix = match level {
                    HeadingLevel::H1 => "### ",
                    HeadingLevel::H2 => "## ",
                    HeadingLevel::H3 => "# ",
                    HeadingLevel::H4 => "",
                    HeadingLevel::H5 => "",
                    HeadingLevel::H6 => "",
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
                list_level += 1;
            }
            Event::End(TagEnd::List(_)) => {
                list_level = list_level.saturating_sub(1);
                writeln!(w)?;
            }
            Event::Start(Tag::Item) => {
                list_item_count += 1;
                if list_item_count > 1 {
                    writeln!(w)?;
                }
                let indent = "  ".repeat(list_level - 1);
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
                if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
                need_blank = false;
                in_table = true;
                table_rows.clear();
                table_row_cells.clear();
            }
            Event::End(TagEnd::Table) => {
                in_table = false;
                render_table(w, &table_rows)?;
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
            Event::Start(Tag::BlockQuote(_)) => {
                if !at_start {
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

/// Render a table with proper column alignment.
fn render_table(w: &mut impl Write, rows: &[Vec<String>]) -> std::io::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let num_cols = rows[0].len();

    // Determine column widths
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

    // Render all rows with proper alignment
    for (row_idx, row) in rows.iter().enumerate() {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                write!(w, " | ")?;
            }
            let padded = pad_cell(cell, col_widths[i]);
            write!(w, "{}", padded)?;
        }
        writeln!(w)?;

        // Separator after header row
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
        let md = "# Hello\n\nWorld";
        let output = render(md);
        assert!(output.contains("### "));
        assert!(output.contains("World"));
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
        assert!(output.contains("### H1"));
        assert!(output.contains("## H2"));
        assert!(output.contains("# H3"));
        assert!(output.contains("H4"));
    }

    #[test]
    fn test_horizontal_rule() {
        let md = "Before\n\n---\n\nAfter";
        let output = render(md);
        assert!(output.contains("─"));
    }
}
