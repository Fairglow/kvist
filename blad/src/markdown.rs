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

use crate::inline::{InlineWriter, Style};

/// A configured markdown renderer that holds rendering options.
///
/// Configure it once with your theme and width preferences, then call
/// `render_markdown` multiple times with different markdown content.
#[derive(Debug, Clone, Default)]
pub struct MarkdownRenderer {
    syntax_theme: Option<String>,
    width: Option<usize>,
}

impl MarkdownRenderer {
    /// Create a new renderer with default options (no syntax theme, no width limit).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the syntax highlighting theme (e.g., "Monokai", "Dracula").
    /// Pass `None` to disable syntax highlighting.
    pub fn set_syntax_theme(&mut self, theme: Option<&str>) {
        self.syntax_theme = theme.map(|s| s.to_owned());
    }

    /// Set the render width. Call this from the centralized resize handler.
    pub fn set_width(&mut self, width: Option<usize>) {
        self.width = width;
    }

    /// Render markdown with the renderer's configured options.
    pub fn render_markdown(&self, md: &str, w: &mut impl Write) -> std::io::Result<()> {
        render_markdown_theme(md, w, self.width, self.syntax_theme.as_deref())
    }
}

/// Render markdown with syntax-highlighted code blocks to the terminal.
/// Use a `MarkdownRenderer` for repeated rendering with the same configuration.
pub fn render_markdown(md: &str, w: &mut impl Write) -> std::io::Result<()> {
    MarkdownRenderer::new().render_markdown(md, w)
}

fn render_markdown_theme(
    md: &str,
    w: &mut impl Write,
    terminal_width: Option<usize>,
    theme_name: Option<&str>,
) -> std::io::Result<()> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = if let Some(name) = theme_name {
        ts.themes.get(name).cloned()
    } else {
        get_terminal_theme(&ts)
    };

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

    let mut bold = 0_usize;
    let mut italic = 0_usize;
    let mut strikethrough = 0_usize;
    let mut dim = 0_usize;
    let mut quote_colors = Vec::new();

    let mut list_item_count = 0;
    let mut list_level: usize = 0;
    let mut in_heading = false;

    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_row_cells: Vec<String> = Vec::new();

    let mut at_start = true;
    let mut need_blank = false;

    let mut inline = InlineWriter::new(terminal_width);

    for event in parser {
        let style = Style {
            bold: bold > 0 || in_heading,
            italic: italic > 0,
            strike: strikethrough > 0,
            dim: dim > 0,
            color: if in_heading {
                Some(34)
            } else {
                quote_colors.last().copied()
            },
            ..Style::default()
        };
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                inline.finish(w)?;
                inline.reset(0, 0);
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
                } else if let (Some(syntax), Some(theme)) =
                    (find_syntax(&ss, &code_language), theme.as_ref())
                {
                    hl = Some(HighlightLines::new(syntax, theme));
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
                } else {
                    // No highlighter (missing syntax or theme): keep the code
                    // visible as plain text instead of dropping it.
                    for line in code_content.lines() {
                        writeln!(w, "{}", line)?;
                    }
                    writeln!(w)?;
                }
                inline.reset(0, 0);
            }
            Event::Start(Tag::Strong) => {
                bold += 1;
            }
            Event::End(TagEnd::Strong) => {
                bold = bold.saturating_sub(1);
            }
            Event::Start(Tag::Emphasis) => {
                italic += 1;
            }
            Event::End(TagEnd::Emphasis) => {
                italic = italic.saturating_sub(1);
            }
            Event::Start(Tag::Strikethrough) => {
                strikethrough += 1;
            }
            Event::End(TagEnd::Strikethrough) => {
                strikethrough = strikethrough.saturating_sub(1);
            }
            Event::Text(text) => {
                if in_code_block {
                    code_content.push_str(&text);
                } else if in_table {
                    let cleaned = text.replace('\n', " ").replace('\r', "");
                    if let Some(cell) = table_row_cells.last_mut()
                        && !cleaned.is_empty()
                    {
                        cell.push_str(&cleaned);
                    }
                } else {
                    inline.text(w, &text, style)?;
                }
            }
            Event::Code(text) => {
                if in_table {
                    if let Some(cell) = table_row_cells.last_mut() {
                        cell.push_str(&text);
                    }
                } else {
                    inline.code(
                        &text,
                        Style {
                            reverse: true,
                            ..style
                        },
                    );
                }
            }
            Event::End(TagEnd::Paragraph) => {
                inline.finish(w)?;
                writeln!(w)?;
                inline.reset(
                    0,
                    if list_level > 0 {
                        2 * list_level + 2
                    } else {
                        0
                    },
                );
                need_blank = true;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                inline.finish(w)?;
                if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                } else if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
                in_heading = true;
                let prefix = match level {
                    HeadingLevel::H1 => "# ",
                    HeadingLevel::H2 => "## ",
                    HeadingLevel::H3 => "### ",
                    HeadingLevel::H4 => "#### ",
                    HeadingLevel::H5 => "##### ",
                    HeadingLevel::H6 => "###### ",
                };
                inline.reset(0, 0);
                inline.text(
                    w,
                    prefix,
                    Style {
                        bold: true,
                        color: Some(34),
                        ..Style::default()
                    },
                )?;
            }
            Event::End(TagEnd::Heading(_)) => {
                inline.finish(w)?;
                in_heading = false;
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
                inline.reset(0, 0);
                need_blank = true;
            }
            Event::SoftBreak => {
                if in_table {
                    if let Some(cell) = table_row_cells.last_mut() {
                        cell.push(' ');
                    }
                } else {
                    inline.soft_break(w)?;
                }
            }
            Event::HardBreak => {
                if in_table {
                    if let Some(cell) = table_row_cells.last_mut() {
                        cell.push(' ');
                    }
                } else {
                    inline.hard_break(w)?;
                }
            }
            Event::Start(Tag::List(_)) => {
                inline.finish(w)?;
                inline.reset(0, 0);
                if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                } else if !at_start {
                    writeln!(w)?;
                }
                list_level += 1;
            }
            Event::End(TagEnd::List(_)) => {
                inline.finish(w)?;
                list_level = list_level.saturating_sub(1);
                writeln!(w)?;
                inline.reset(
                    0,
                    if list_level > 0 {
                        2 * list_level + 2
                    } else {
                        0
                    },
                );
            }
            Event::Start(Tag::Item) => {
                inline.finish(w)?;
                list_item_count += 1;
                let indent_width = 2 * list_level.saturating_sub(1);
                let indent_width =
                    terminal_width.map_or(indent_width, |width| indent_width.min(width.max(1) - 1));
                let indent = " ".repeat(indent_width);
                if list_item_count > 1 {
                    write!(w, "\r\n")?;
                }
                write!(w, "{indent}")?;
                inline.reset(indent_width, 0);
                inline.text(
                    w,
                    "• ",
                    Style {
                        color: Some(33),
                        ..Style::default()
                    },
                )?;
                inline.set_indent(2 * list_level + 2);
            }
            Event::End(TagEnd::Item) => inline.finish(w)?,
            Event::Rule => {
                inline.finish(w)?;
                inline.reset(0, 0);
                if !at_start {
                    writeln!(w)?;
                }
                write!(w, "\x1b[90m")?;
                for _ in 0..terminal_width.unwrap_or(80).max(1) {
                    write!(w, "─")?;
                }
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
                need_blank = true;
            }
            Event::Start(Tag::Table(_)) => {
                inline.finish(w)?;
                inline.reset(0, 0);
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
                inline.reset(0, 0);
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
                inline.finish(w)?;
                if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
                let (color, label) = match kind {
                    pulldown_cmark::BlockQuoteKind::Note => (96, "NOTE"),
                    pulldown_cmark::BlockQuoteKind::Tip => (92, "TIP"),
                    pulldown_cmark::BlockQuoteKind::Important => (95, "IMPORTANT"),
                    pulldown_cmark::BlockQuoteKind::Warning => (93, "WARNING"),
                    pulldown_cmark::BlockQuoteKind::Caution => (91, "CAUTION"),
                };
                quote_colors.push(color);
                inline.reset(0, 0);
                inline.text(
                    w,
                    &format!("[{label}] "),
                    Style {
                        color: Some(color),
                        ..Style::default()
                    },
                )?;
            }
            Event::Start(Tag::Superscript) => {
                dim += 1;
            }
            Event::End(TagEnd::Superscript) => {
                dim = dim.saturating_sub(1);
            }
            Event::Start(Tag::Subscript) => {
                dim += 1;
            }
            Event::End(TagEnd::Subscript) => {
                dim = dim.saturating_sub(1);
            }
            Event::FootnoteReference(id) => {
                inline.code(
                    &format!("[{id}]"),
                    Style {
                        color: Some(96),
                        ..style
                    },
                );
            }
            Event::Start(Tag::FootnoteDefinition(_)) => {
                inline.finish(w)?;
                inline.reset(0, 0);
                if !at_start {
                    writeln!(w)?;
                }
                write!(w, "\x1b[90m")?;
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                inline.finish(w)?;
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
                inline.reset(0, 0);
            }
            Event::InlineMath(tex) => {
                inline.code(
                    &format!("${tex}"),
                    Style {
                        color: Some(95),
                        ..style
                    },
                );
            }
            Event::DisplayMath(tex) => {
                inline.finish(w)?;
                inline.reset(0, 0);
                if !at_start {
                    writeln!(w)?;
                }
                write!(w, "\x1b[95m${} $\x1b[0m", tex)?;
                writeln!(w)?;
                need_blank = true;
            }
            Event::Start(Tag::Paragraph) => {
                if inline.has_prefix() {
                    need_blank = false;
                } else if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                } else if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
            }
            Event::Start(Tag::BlockQuote(_)) => {
                inline.finish(w)?;
                inline.reset(0, 0);
                if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                } else if !at_start {
                    writeln!(w)?;
                }
                at_start = false;
                write!(w, "\x1b[90m")?;
                quote_colors.push(90);
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                inline.finish(w)?;
                quote_colors.pop();
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
                inline.reset(0, 0);
                need_blank = true;
            }
            _ => {}
        }
    }
    inline.finish(w)?;

    Ok(())
}

/// Render a table with proper column alignment and soft-wrapping.
/// Column widths are adapted to fit the terminal width while preserving
/// readability. Cells wrap to multiple lines when needed.
/// Target cell aspect ratio is 4:3 (width:height) for readability.
fn render_table(
    w: &mut impl Write,
    rows: &[Vec<String>],
    terminal_width: Option<usize>,
) -> std::io::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    // Normalize ragged rows: use the widest row so cell access never runs
    // past a shorter row's actual cell count.
    let num_cols = rows.iter().map(|row| row.len()).max().unwrap_or(0);
    if num_cols == 0 {
        return Ok(());
    }

    // Determine natural column widths based on content.
    let mut col_widths = vec![0usize; num_cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < num_cols {
                let width = unicode_width::UnicodeWidthStr::width(cell.as_str());
                if width > col_widths[i] {
                    col_widths[i] = width;
                }
            }
        }
    }

    // Enforce minimum column width: 4:3 width:height ratio means a column
    // that wraps to H lines must be at least ceil(H * 4/3) columns wide.
    // Iterate until stable because wrapping changes based on column widths.
    let mut stable = false;
    while !stable {
        stable = true;
        for row in rows {
            let mut row_wrapped = Vec::new();
            for (i, &width) in col_widths.iter().enumerate() {
                let cell = row.get(i).map_or("", |cell| cell.as_str());
                row_wrapped.push(wrap_cell(cell, width));
            }
            let max_lines = row_wrapped
                .iter()
                .map(|lines| lines.len())
                .max()
                .unwrap_or(1);
            for (i, _wrapped_lines) in row_wrapped.iter().enumerate() {
                let needed = ceil_div(max_lines * 4, 3);
                if col_widths[i] < needed {
                    col_widths[i] = needed;
                    stable = false;
                }
            }
        }
    }

    // If terminal width is provided, adapt column widths to fit
    if let Some(term_width) = terminal_width {
        let separator_space = (num_cols - 1) * 3;
        let available_width = term_width.saturating_sub(separator_space);
        let total_natural: usize = col_widths.iter().sum();

        if total_natural > available_width {
            // Need to shrink columns. Strategy:
            // 1. First, try to fit all columns at their natural width
            // 2. If that doesn't work, shrink the largest columns first
            // 3. Use iterative proportional fit with minimum width constraints
            let min_width = 3;
            let mut remaining = available_width;
            let mut fitted = vec![false; num_cols];

            // First pass: fit columns that can fit at natural width
            for i in 0..num_cols {
                if col_widths[i] <= remaining {
                    fitted[i] = true;
                    remaining = remaining.saturating_sub(col_widths[i]);
                }
            }

            // Second pass: shrink remaining columns proportionally
            loop {
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
        for (i, &width) in col_widths.iter().enumerate() {
            let cell = row.get(i).map_or("", |cell| cell.as_str());
            let wrapped = wrap_cell(cell, width);
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

            // Add separator between wrapped lines within the same row
            if line_idx < max_lines - 1 {
                for (i, &width) in col_widths.iter().enumerate() {
                    if i > 0 {
                        write!(w, " | ")?;
                    }
                    write!(w, "{}", "─".repeat(width))?;
                }
                writeln!(w)?;
            }
        }

        // Separator after header row (using Unicode box-drawing)
        if row_idx == 0 {
            for (i, &width) in col_widths.iter().enumerate() {
                if i > 0 {
                    write!(w, " | ")?;
                }
                write!(w, "{}", "─".repeat(width))?;
            }
            writeln!(w)?;
        }
    }

    Ok(())
}

/// Wrap a cell to fit within the given width.
/// Uses word wrapping, with hard wrapping for words that exceed the width.
fn wrap_cell(cell: &str, width: usize) -> Vec<String> {
    // Quick check: if cell fits, return as-is
    if unicode_width::UnicodeWidthStr::width(cell) <= width {
        return vec![cell.to_string()];
    }

    let mut result = Vec::new();
    let mut current = String::new();

    for word in cell.split_whitespace() {
        let word_width = unicode_width::UnicodeWidthStr::width(word);
        
        // If the word itself is wider than the column, hard-wrap it
        if word_width > width {
            if !current.is_empty() {
                result.push(current);
                current = String::new();
            }
            // Split the long word into chunks of the given width
            let word_chars: Vec<char> = word.chars().collect();
            let mut i = 0;
            while i < word_chars.len() {
                let end = i + width;
                let chunk: String = if end < word_chars.len() {
                    word_chars[i..end].iter().collect()
                } else {
                    word_chars[i..].iter().collect()
                };
                result.push(chunk);
                i = end;
            }
        } else if current.is_empty() {
            current = word.to_string();
        } else if unicode_width::UnicodeWidthStr::width(current.as_str()) + 1 + word_width <= width {
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

    if result.is_empty() {
        result.push(String::new());
    }

    result
}

/// Pad a cell to the right width.
fn pad_cell(cell: &str, width: usize) -> String {
    let cell_width = unicode_width::UnicodeWidthStr::width(cell);
    if cell_width >= width {
        cell.to_string()
    } else {
        format!("{}{}", cell, " ".repeat(width - cell_width))
    }
}

fn ceil_div(a: usize, b: usize) -> usize {
    a / b + if a % b != 0 { 1 } else { 0 }
}


fn find_syntax<'a>(ss: &'a SyntaxSet, lang: &str) -> Option<&'a syntect::parsing::SyntaxReference> {
    let lang = lang.trim();
    if lang.is_empty() {
        return ss
            .find_syntax_by_name("Plain Text")
            .or_else(|| ss.syntaxes().first());
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
            return Some(s);
        }
    }

    if let Some(s) = ss.find_syntax_by_name(lang) {
        return Some(s);
    }

    if let Some(s) = ss.find_syntax_by_extension(lang) {
        return Some(s);
    }

    let upper = lang.to_uppercase();
    if let Some(s) = ss.find_syntax_by_name(&upper) {
        return Some(s);
    }

    ss.find_syntax_by_name("Plain Text")
        .or_else(|| ss.syntaxes().first())
}

fn get_terminal_theme(ts: &ThemeSet) -> Option<Theme> {
    let candidates = [
        "base16-ocean.dark",
        "base16-default.dark",
        "solarized-dark",
        "monokai",
        "gruvbox-dark",
    ];

    for name in candidates {
        if let Some(theme) = ts.themes.get(name) {
            return Some(theme.clone());
        }
    }

    ts.themes
        .values()
        .next()
        .cloned()
        .or_else(|| ThemeSet::load_defaults().themes.values().next().cloned())
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
#[path = "markdown_tests.rs"]
mod tests;
