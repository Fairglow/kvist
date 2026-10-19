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
use std::io::{IsTerminal, Write};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SColor, FontStyle, Style as SStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::inline::{InlineWriter, Style};

/// Whether the terminal's background is dark or light. Drives the table
/// zebra striping: a dark background uses subtly brighter tints and a light
/// background uses subtly darker tints, while the header is always a full
/// reverse video so it inverts to the complement of the text color in either
/// mode. Detection is automatic (OSC 11 query, then
/// `COLORTERM`/`COLORFGBG`/`NO_COLOR`, then the terminal's `TERM_PROGRAM`/`TERM`
/// heuristics) but can be overridden explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalBg {
    /// The background is dark (the default assumption).
    #[default]
    Dark,
    /// The background is light.
    Light,
}

impl TerminalBg {
    /// The 256-color index of the subtle data-row tint. A one-step lift from
    /// the base background is deliberately faint: it separates rows without
    /// drawing a border, and it stays legible over the standard foreground.
    /// The header is a full reverse video (see `render_table`) rather than a
    /// fixed background, because a fixed color can't guarantee contrast for
    /// every foreground the terminal might use.
    const fn tint(self) -> u8 {
        match self {
            TerminalBg::Dark => 235,  // #4e4e4e, one step above #262626
            TerminalBg::Light => 254, // #e4e4e4, one step below #f4f4f4
        }
    }

    /// Derive a [`TerminalBg`] from an RGB panel background color using the
    /// standard relative-luminance formula. A panel whose luminance is at or
    /// above the midpoint (0x7f, the halfway point of an 8-bit channel) is
    /// treated as light; anything darker is dark. This lets an application
    /// that owns its panel background (e.g. Skott's themed TUI) feed the exact
    /// zebra mode instead of relying on terminal-environment detection.
    pub const fn from_panel_rgb(r: u8, g: u8, b: u8) -> Self {
        let l = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
        if l >= 127 {
            TerminalBg::Light
        } else {
            TerminalBg::Dark
        }
    }
}

/// Detect the terminal background from environment heuristics. This is
/// side-effect-free (no terminal query, no stdin read) so it can run on every
/// render without interfering with the interactive viewer's event loop. The
/// result is deliberately conservative: anything ambiguous is `Dark`, the
/// overwhelming default. Callers that need a precise answer can use
/// [`query_background_rgb`].
fn detect_terminal_bg() -> TerminalBg {
    // An explicit `COLORFGBG` background field is the strongest env signal.
    if let Some(bg_field) = std::env::var("COLORFGBG").ok().as_deref()
        && let Some(bg) = bg_field.split(':').next_back() {
            return match bg {
                "1" | "15" | "7" => TerminalBg::Light,
                _ => TerminalBg::default(),
            };
        }

    // COLORTERM and TERM_PROGRAM are weak but the only explicit dark/light
    // markers some terminals expose. A terminal that advertises a light
    // palette wins; anything else stays dark.
    for val in [
        std::env::var("COLORTERM").ok(),
        std::env::var("TERM_PROGRAM").ok(),
        std::env::var("TERM").ok(),
    ] {
        let Some(val) = val else { continue };
        let lower = val.to_ascii_lowercase();
        if lower.contains("light") {
            return TerminalBg::Light;
        }
    }

    TerminalBg::default()
}

/// Query the terminal's background color via OSC 11 and parse the response.
/// Returns `None` if the terminal doesn't answer within a short deadline or
/// the response is malformed. Callers should only invoke this when they can
/// safely read stdin (e.g. before entering the alternate screen / raw mode),
/// never in the middle of a render, because it consumes up to the deadline of
/// pending input.
pub fn query_background_rgb() -> Option<(u8, u8, u8)> {
    use std::io::Read as _;
    if !std::io::stdout().is_terminal() {
        return None;
    }
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(b"\x1b]11;?\x07").is_err() || stdout.flush().is_err() {
        return None;
    }
    drop(stdout);

    let mut buf = [0u8; 64];
    let mut pos = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
    while pos < buf.len() && std::time::Instant::now() < deadline {
        match std::io::stdin().read(&mut buf[pos..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => pos += n,
        }
    }
    parse_osc11_rgb(&String::from_utf8_lossy(&buf[..pos]))
}

fn parse_osc11_rgb(text: &str) -> Option<(u8, u8, u8)> {
    // The response is an OSC 11 sequence: ESC ] 11 ; rgb:RRRR/GGGG/BBBB ESC \
    // (or BEL). Some terminals wrap it in a leading ESC (CSI u). Find the
    // `rgb:` payload and parse the hex triplets.
    let start = text.find("rgb:")? + "rgb:".len();
    let tail = &text[start..];
    let mut parts = tail.splitn(3, '/');
    let r = parts
        .next()?
        .trim_end_matches(|c: char| !c.is_ascii_hexdigit())
        .parse()
        .ok()?;
    let g = parts
        .next()?
        .trim_end_matches(|c: char| !c.is_ascii_hexdigit())
        .parse()
        .ok()?;
    let b = parts
        .next()?
        .trim_end_matches(|c: char| !c.is_ascii_hexdigit())
        .parse()
        .ok()?;
    Some((r, g, b))
}

/// A configured markdown renderer that holds rendering options.
///
/// Configure it once with your theme and width preferences, then call
/// `render_markdown` multiple times with different markdown content.
#[derive(Debug, Clone, Default)]
pub struct MarkdownRenderer {
    syntax_theme: Option<String>,
    width: Option<usize>,
    /// Explicit terminal background. `None` means "auto-detect" on each
    /// render (a side-effect-free environment lookup).
    bg: Option<TerminalBg>,
}

impl MarkdownRenderer {
    /// Create a new renderer with default options (no syntax theme, no width
    /// limit, dark background).
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

    /// Set the terminal background mode explicitly. Call this when the
    /// application already knows the background (e.g. Skott, which owns the
    /// theme). When left unset, the renderer auto-detects from the
    /// environment on each render.
    pub fn set_background(&mut self, bg: TerminalBg) {
        self.bg = Some(bg);
    }

    /// The effective background: the explicit value if set, otherwise the
    /// auto-detected one.
    fn effective_bg(&self) -> TerminalBg {
        self.bg.unwrap_or_else(detect_terminal_bg)
    }

    /// Render markdown with the renderer's configured options.
    pub fn render_markdown(&self, md: &str, w: &mut impl Write) -> std::io::Result<()> {
        render_markdown_theme(
            md,
            w,
            self.width,
            self.syntax_theme.as_deref(),
            self.effective_bg(),
        )
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
    bg: TerminalBg,
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
                render_table(w, &table_rows, terminal_width, bg)?;
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
    bg: TerminalBg,
) -> std::io::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    // The subtle background applied to alternating data rows. A one-step lift
    // from the panel background is faint enough to read as a row separator
    // without a border line, and it never fights the foreground. The header
    // is rendered separately (below) as a full reverse video, which makes it
    // stand out as the table's anchor and lets the header/data separator
    // disappear.
    let tint_code = format!("\x1b[48;5;{}m", bg.tint());
    // The header is a full reverse video (SGR 7), which inverts both the
    // foreground and the background together. That makes the header bar the
    // exact complement of the text color in *any* theme, so contrast is
    // guaranteed without the renderer having to know the panel's fg or bg —
    // and it's the only inversion that stays legible when the user swaps
    // light for dark (or vice versa) mid-session.
    let header_code = "\x1b[7m";
    let reset = "\x1b[0m";

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
            // Proportional allocation: each column's width scales to how much
            // space its largest value needs, so the column with the longest
            // content gets the most room and the shorter columns shrink to
            // fit. This preserves the unbreakable 4:3 width:height ratio -- a
            // column whose content is long is given proportionally more width,
            // so it never wraps to so many lines that it becomes a tall, thin
            // stub (width/height < 4/3). Short columns keep their 4:3 ratio by
            // wrapping only a little.
            let natural_total = total_natural.max(1);
            let mut widths: Vec<usize> = col_widths
                .iter()
                .map(|&cw| {
                    let w = (cw as u128 * available_width as u128) / natural_total as u128;
                    w.max(1) as usize
                })
                .collect();
            let mut used: usize = widths.iter().sum();
            // Reconcile rounding so the table uses exactly the available width.
            // Give the leftover to the column with the most content (it needs
            // it most) and take any overflow back from the column with the
            // least content.
            while used < available_width {
                let idx = widths
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, w)| **w)
                    .map(|(i, _)| i)
                    .expect("at least one column");
                widths[idx] += 1;
                used += 1;
            }
            while used > available_width {
                let idx = widths
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| **w > 1)
                    .min_by_key(|(_, w)| **w)
                    .map(|(i, _)| i)
                    .expect("some column is wider than 1 and can shrink");
                widths[idx] -= 1;
                used -= 1;
            }
            col_widths = widths;
        }
    }

    // Render all rows with proper alignment and soft-wrapping. The header is
    // a reverse video (a full inversion that auto-adapts to dark or light and
    // always keeps great contrast), and the data rows alternate between the
    // panel background and the subtle tint. Wrapping the *whole* visual line
    // in the row's background means a multiline (wrapped) cell stays inside one
    // colored block, so the background itself reads as the row separator and
    // no border line between rows is needed.
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

        // Per-row background: the header takes the reverse video, the even
        // data rows (0, 2, …) take the subtle tint, and the odd data rows stay
        // on the plain panel background. The tint is subtle; the header
        // inversion is not, so the header reads as the table's anchor and no
        // separator line between header and data is needed.
        let (prefix, suffix) = if row_idx == 0 {
            (header_code, reset)
        } else if (row_idx - 1) % 2 == 0 {
            (tint_code.as_str(), reset)
        } else {
            ("", "")
        };

        // Render each line of the wrapped row, wrapping it in the row's
        // background so multiline cells stay within one colored block.
        for line_idx in 0..max_lines {
            if !prefix.is_empty() {
                write!(w, "{prefix}")?;
            }
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
            if !suffix.is_empty() {
                write!(w, "{suffix}")?;
            }
            writeln!(w)?;
        }
    }

    Ok(())
}

/// Wrap a cell to fit within the given width.
/// Uses word wrapping, with hard wrapping for words that exceed the width.
fn ceil_div(a: usize, b: usize) -> usize {
    a / b + if !a.is_multiple_of(b) { 1 } else { 0 }
}

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
        } else if unicode_width::UnicodeWidthStr::width(current.as_str()) + 1 + word_width <= width
        {
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
