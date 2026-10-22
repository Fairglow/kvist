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
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SColor, FontStyle, Style as SStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::inline::{InlineWriter, Style};
pub(crate) use crate::terminal_text::inert_source;
use crate::theme::{ColorDepth, RenderOptions, Theme};

fn syntax_assets() -> (&'static SyntaxSet, &'static ThemeSet) {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    static THEMES: OnceLock<ThemeSet> = OnceLock::new();
    (
        SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines),
        THEMES.get_or_init(ThemeSet::load_defaults),
    )
}

/// Whether the terminal's background is dark or light. Drives the table
/// zebra striping: a dark background uses subtly brighter tints and a light
/// background uses subtly darker tints, while the header is always a full
/// reverse video so it inverts to the complement of the text color in either
/// mode. Embedding applications provide their panel appearance explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalBg {
    /// The background is dark (the default assumption).
    #[default]
    Dark,
    /// The background is light.
    Light,
}

impl TerminalBg {
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

/// A configured markdown renderer that holds rendering options.
///
/// Configure it once with your theme and width preferences, then call
/// `render_markdown` multiple times with different markdown content.
#[derive(Debug, Clone, Default)]
pub struct MarkdownRenderer {
    syntax_theme: Option<String>,
    width: Option<usize>,
    options: RenderOptions,
}

impl MarkdownRenderer {
    /// Create a new renderer with default options (no syntax theme, no width
    /// limit, dark background).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a renderer using explicit host-resolved settings.
    pub fn with_options(options: RenderOptions) -> Self {
        Self {
            options,
            ..Self::default()
        }
    }

    /// Read the host-resolved settings.
    pub fn options(&self) -> &RenderOptions {
        &self.options
    }

    /// Replace settings without losing width or syntax-theme selection.
    pub fn set_options(&mut self, options: RenderOptions) {
        self.options = options;
    }

    /// Select an installed syntect theme. `None` restores the appearance default.
    /// An unknown explicit name produces an error when rendering.
    pub fn set_syntax_theme(&mut self, theme: Option<&str>) {
        self.syntax_theme = theme.map(|s| s.to_owned());
    }

    /// Set the render width. Call this from the centralized resize handler.
    pub fn set_width(&mut self, width: Option<usize>) {
        self.width = width;
    }

    /// Set the terminal background mode explicitly. Call this when the
    /// application already knows the background (e.g. Skott, which owns the
    /// theme). Library defaults are deterministic and never query the terminal.
    pub fn set_background(&mut self, bg: TerminalBg) {
        self.options.theme = match bg {
            TerminalBg::Dark => Theme::Dark,
            TerminalBg::Light => Theme::Light,
        };
    }

    /// Render markdown with the renderer's configured options.
    pub fn render_markdown(&self, md: &str, w: &mut impl Write) -> std::io::Result<()> {
        self.render_controlled(md, w, None)
    }

    /// Render with host-controlled cooperative cancellation.
    ///
    /// Cancellation and deadlines return `Interrupted`; output may already
    /// have been written. No thread, subprocess or terminal query is started.
    pub fn render_markdown_with_cancellation(
        &self,
        md: &str,
        w: &mut impl Write,
        cancellation: &crate::RenderCancellation,
    ) -> std::io::Result<()> {
        self.render_controlled(md, w, Some(cancellation))
    }

    fn render_controlled(
        &self,
        md: &str,
        w: &mut impl Write,
        cancellation: Option<&crate::RenderCancellation>,
    ) -> std::io::Result<()> {
        if let Some(control) = cancellation {
            control.checkpoint()?;
        }
        let mut output = crate::output::Output::new(w, &self.options);
        render_markdown_theme(
            md,
            &mut output,
            self.width,
            self.syntax_theme.as_deref(),
            &self.options,
            cancellation,
        )?;
        output.flush()?;
        if let Some(control) = cancellation {
            control.checkpoint()?;
        }
        Ok(())
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
    settings: &RenderOptions,
    cancellation: Option<&crate::RenderCancellation>,
) -> std::io::Result<()> {
    let (ss, ts) = syntax_assets();
    let palette = settings.palette();
    let code_bg = palette
        .code_background
        .map(|color| color.sequence(true))
        .transpose()?
        .unwrap_or_default();
    let theme = if let Some(name) = theme_name {
        Some(ts.themes.get(name).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unknown syntax theme: {name}"),
            )
        })?)
    } else if settings.theme == Theme::Light {
        ts.themes
            .get("InspiredGitHub")
            .or_else(|| ts.themes.get("Solarized (light)"))
    } else {
        get_terminal_theme(ts, settings.color_depth)
    };

    let safe_source = inert_source(md);
    let md = safe_source.as_ref();
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_GFM;

    let parser = crate::scripts::events(md, Parser::new_ext(md, options).into_offset_iter());

    let mut in_code_block = false;
    let mut code_language = String::new();
    let mut code_content = String::new();
    let mut hl: Option<HighlightLines> = None;

    let mut bold = 0_usize;
    let mut italic = 0_usize;
    let mut strikethrough = 0_usize;
    let mut links = 0_usize;
    let mut quote_colors = Vec::new();

    let mut list_item_count = 0;
    let mut list_level: usize = 0;
    // The start number of each open list, so an ordered list that does not
    // begin at 1 is enumerated from its actual first number.
    let mut list_starts: Vec<Option<usize>> = Vec::new();
    let mut in_heading = false;

    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_row_cells: Vec<String> = Vec::new();

    let mut at_start = true;
    let mut need_blank = false;
    let mut last_display = false;

    let mut inline = InlineWriter::new(terminal_width);

    for (event, range) in parser {
        if let Some(control) = cancellation {
            control.checkpoint()?;
        }
        if last_display
            && matches!(
                &event,
                Event::Text(_)
                    | Event::Code(_)
                    | Event::InlineMath(_)
                    | Event::FootnoteReference(_)
            )
            && !in_table
        {
            writeln!(w)?;
            last_display = false;
            need_blank = false;
        }
        let style = Style {
            bold: bold > 0 || in_heading,
            italic: italic > 0,
            strike: strikethrough > 0,
            color: if in_heading {
                Some(palette.heading)
            } else {
                quote_colors
                    .last()
                    .copied()
                    .or_else(|| (links > 0).then_some(palette.link))
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
                if code_language != "mermaid"
                    && let (Some(syntax), Some(theme)) = (find_syntax(ss, &code_language), theme)
                {
                    hl = Some(HighlightLines::new(syntax, theme));
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                if code_language == "mermaid" {
                    crate::diagram::write(
                        &code_content,
                        w,
                        settings,
                        terminal_width,
                        cancellation,
                    )?;
                    writeln!(w)?;
                } else {
                    // A full-width frame line directly above and below the
                    // block gives the code a distinct, closed shape. It is
                    // only drawn when a width budget exists (the viewer); with
                    // no width there is no panel to frame. The bottom frame
                    // replaces the block's trailing blank line, so it sits
                    // flush against the last code line.
                    if terminal_width.is_some() {
                        write_code_frame(w, &code_bg, terminal_width)?;
                    }
                    let reset = "\x1b[0m";
                    for line in code_content.lines() {
                        // Measure the raw line's display width (full-width
                        // characters count double) before ANSI codes are
                        // added, so the padding is exact.
                        let content_width = crate::terminal_text::width_with_tabs(line);
                        write!(w, "{}", code_bg)?;
                        if let Some(highlighter) = hl.as_mut() {
                            match highlighter.highlight_line(line, ss) {
                                Ok(ranges) => {
                                    // A blank line yields no ranges, so the
                                    // body is empty and only the padding
                                    // fills it.
                                    let styled = to_ansi_line(&ranges, &code_bg);
                                    let padded =
                                        pad_code_line(&styled, content_width, terminal_width);
                                    write!(w, "{}", padded)?;
                                }
                                Err(_) => {
                                    let padded =
                                        pad_code_line(line, content_width, terminal_width);
                                    write!(w, "{}", padded)?;
                                }
                            }
                        } else {
                            // No highlighter (missing syntax or theme): keep
                            // the code visible as plain text, but still on
                            // the mode-appropriate code panel.
                            let padded = pad_code_line(line, content_width, terminal_width);
                            write!(w, "{}", padded)?;
                        }
                        write!(w, "{}", reset)?;
                        writeln!(w)?;
                    }
                    hl = None;
                    if terminal_width.is_some() {
                        write_code_frame(w, &code_bg, terminal_width)?;
                    } else {
                        writeln!(w)?;
                    }
                }
                inline.reset(0, 0);
            }
            Event::Start(Tag::Strong) => {
                bold += 1;
            }
            Event::End(TagEnd::Strong) => {
                bold = bold.saturating_sub(1);
            }
            Event::Start(Tag::Link { .. }) => links += 1,
            Event::End(TagEnd::Link) => links = links.saturating_sub(1),
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
                let projected = if in_code_block {
                    std::borrow::Cow::Borrowed(text.as_ref())
                } else {
                    crate::scripts::text(&text, md, range)
                };
                let text = inert_source(&projected);
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
            Event::InlineHtml(html) if in_table => {
                if ["<br>", "<br/>", "<br />"]
                    .iter()
                    .any(|tag| html.trim().eq_ignore_ascii_case(tag))
                    && let Some(cell) = table_row_cells.last_mut()
                {
                    cell.push('\n');
                }
            }
            Event::Code(text) => {
                let text = inert_source(&text);
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
                if !last_display {
                    writeln!(w)?;
                }
                last_display = false;
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
                        color: Some(palette.heading),
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
                        cell.push('\n');
                    }
                } else {
                    inline.hard_break(w)?;
                }
            }
            Event::Start(Tag::List(start)) => {
                inline.finish(w)?;
                inline.reset(0, 0);
                if need_blank {
                    writeln!(w)?;
                    need_blank = false;
                } else if !at_start {
                    writeln!(w)?;
                }
                list_level += 1;
                list_starts.push(start.map(|n| n as usize));
                list_item_count = 0;
            }
            Event::End(TagEnd::List(_)) => {
                inline.finish(w)?;
                list_level = list_level.saturating_sub(1);
                list_starts.pop();
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
                    writeln!(w)?;
                }
                write!(w, "{indent}")?;
                inline.reset(indent_width, 0);
                // Ordered items are enumerated from the list's start number;
                // unordered items alternate bullet glyph by nesting level so
                // depth reads at a glance.
                let marker = match list_starts.last().copied().flatten() {
                    Some(start) => format!("{}. ", start.saturating_add(list_item_count - 1)),
                    None => match list_level % 3 {
                        1 => "• ",
                        2 => "◦ ",
                        _ => "– ",
                    }
                    .to_string(),
                };
                inline.text(
                    w,
                    &marker,
                    Style {
                        color: Some(palette.list_marker),
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
                // Differentiate rule styles by their delimiter so `***` reads
                // as stronger than `---` and `~~~` as a squiggle.
                let (glyph, strong) = rule_style(md, range);
                if strong {
                    write!(w, "\x1b[1m")?;
                }
                palette.muted.write(w, false)?;
                for _ in 0..terminal_width.unwrap_or(80).max(1) {
                    write!(w, "{glyph}")?;
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
                render_table(w, &table_rows, terminal_width, settings)?;
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
                    pulldown_cmark::BlockQuoteKind::Note => (palette.alerts[0], "NOTE"),
                    pulldown_cmark::BlockQuoteKind::Tip => (palette.alerts[1], "TIP"),
                    pulldown_cmark::BlockQuoteKind::Important => (palette.alerts[2], "IMPORTANT"),
                    pulldown_cmark::BlockQuoteKind::Warning => (palette.alerts[3], "WARNING"),
                    pulldown_cmark::BlockQuoteKind::Caution => (palette.alerts[4], "CAUTION"),
                };
                quote_colors.push(color);
                inline.reset(0, 0);
                // The alert shares the blockquote's left-edge bar, tinted by
                // the alert's own color, so it stands out from prose.
                inline.set_border(Some('│'));
                inline.text(
                    w,
                    &format!("[{label}] "),
                    Style {
                        color: Some(color),
                        ..Style::default()
                    },
                )?;
            }
            Event::FootnoteReference(id) => {
                let id = inert_source(&id);
                // A superscript marker, sharing the exact tag of its
                // definition so the two are visually linked. Falls back to
                // `[n]` when the id has no single superscript glyph.
                inline.code(
                    &footnote_marker(&id),
                    Style {
                        color: Some(palette.link),
                        ..style
                    },
                );
            }
            Event::Start(Tag::FootnoteDefinition(id)) => {
                inline.finish(w)?;
                inline.reset(0, 0);
                if !at_start {
                    writeln!(w)?;
                }
                // The definition line is prefixed with the same tag as the
                // reference, so the reader can match them without color.
                let id = inert_source(&id);
                inline.text(
                    w,
                    &format!("{} ", footnote_marker(&id)),
                    Style {
                        color: Some(palette.muted),
                        ..Style::default()
                    },
                )?;
                palette.muted.write(w, false)?;
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                inline.finish(w)?;
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
                inline.reset(0, 0);
            }
            Event::InlineMath(tex) => {
                let original = md.get(range.clone()).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "math source range is invalid",
                    )
                })?;
                let currency = crate::math::currency(&tex, md[range.end..].chars().next());
                let text = crate::math::present(&tex, original, settings.math, currency);
                if in_table {
                    if let Some(cell) = table_row_cells.last_mut() {
                        cell.push_str(&text);
                    }
                } else {
                    inline.code(
                        &text,
                        Style {
                            color: Some(palette.math),
                            ..style
                        },
                    );
                }
            }
            Event::DisplayMath(tex) => {
                let original = md.get(range).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "math source range is invalid",
                    )
                })?;
                let text = crate::math::present(&tex, original, settings.math, false);
                if in_table {
                    if let Some(cell) = table_row_cells.last_mut() {
                        cell.push_str(&text);
                    }
                } else {
                    let had_content = inline.has_content();
                    inline.finish(w)?;
                    inline.reset(0, 0);
                    if had_content {
                        writeln!(w)?;
                        writeln!(w)?;
                    } else if need_blank {
                        writeln!(w)?;
                    }
                    palette.math.write(w, false)?;
                    writeln!(w, "{text}\x1b[0m")?;
                    need_blank = true;
                    last_display = true;
                }
            }
            Event::Start(Tag::Paragraph) => {
                last_display = false;
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
                // A left-edge bar makes the quote read as a distinct block
                // rather than just indented text.
                inline.set_border(Some('│'));
                palette.muted.write(w, false)?;
                quote_colors.push(palette.muted);
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                inline.finish(w)?;
                quote_colors.pop();
                inline.set_border(None);
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
                inline.reset(0, 0);
                // The bar already separates the quote from surrounding text;
                // do not also force a blank line, which produced double
                // spacing above and below the block.
                need_blank = false;
            }
            _ => {}
        }
    }
    inline.finish(w)?;

    Ok(())
}

/// Render a table with proper column alignment and soft-wrapping.
/// Styling is independent of the integer width/height solver. Geometry takes
/// precedence over viewport fit; horizontally overflowing tables remain valid.
fn render_table(
    w: &mut impl Write,
    rows: &[Vec<String>],
    terminal_width: Option<usize>,
    settings: &RenderOptions,
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
    let tint_code = settings
        .palette()
        .table_background
        .map(|color| color.sequence(true))
        .transpose()?
        .unwrap_or_default();
    // The header is a full reverse video (SGR 7), which inverts both the
    // foreground and the background together. That makes the header bar the
    // exact complement of the text color in *any* theme, so contrast is
    // guaranteed without the renderer having to know the panel's fg or bg —
    // and it's the only inversion that stays legible when the user swaps
    // light for dark (or vice versa) mid-session.
    let header_code = "\x1b[7m";
    let reset = "\x1b[0m";

    let table = crate::table::Table::new(rows);
    let col_widths = table.widths(terminal_width);
    if col_widths.is_empty() {
        return Ok(());
    }

    // Render all rows with proper alignment and soft-wrapping. The header is
    // a reverse video (a full inversion that auto-adapts to dark or light and
    // always keeps great contrast), and the data rows alternate between the
    // panel background and the subtle tint. Wrapping the *whole* visual line
    // in the row's background means a multiline (wrapped) cell stays inside one
    // colored block, so the background itself reads as the row separator and
    // no border line between rows is needed.
    for (row_idx, row) in table.rows.iter().enumerate() {
        // First, wrap all cells in this row
        let mut wrapped_row: Vec<Vec<String>> = Vec::new();
        for (cell, &width) in row.iter().zip(&col_widths) {
            wrapped_row.push(cell.wrap(width));
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

/// Choose the rule glyph and strength from the delimiter used in the source.
/// `***` is a stronger double line; `---` (and `___`) is a thin line.
/// (`~~~` is a code-fence delimiter in CommonMark, never a rule.)
fn rule_style(md: &str, range: std::ops::Range<usize>) -> (char, bool) {
    let source = md.get(range.clone()).unwrap_or("");
    if source.trim().starts_with('*') {
        ('━', true)
    } else {
        ('─', false)
    }
}

/// Render a footnote id as a superscript marker, falling back to `[id]` when
/// the id is not a plain number (only digits 0-9 have superscript glyphs).
fn footnote_marker(id: &str) -> String {
    const SUPER: &[char] = &['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
        id.chars().map(|c| SUPER[(c as u8 - b'0') as usize]).collect()
    } else {
        format!("[{id}]")
    }
}

fn get_terminal_theme(ts: &ThemeSet, depth: ColorDepth) -> Option<&syntect::highlighting::Theme> {
    // base16-ocean is a subtle-gray truecolor theme: at 256-color and truecolor
    // depth its token colors are distinct, but quantized to 16 colors almost
    // every token collapses to the same white, so syntax highlighting appears
    // to "not work". Solarized (dark) keeps its keyword/string/comment colors
    // distinct even at 16 colors, so it is the right default there.
    let candidates: &[&str] = match depth {
        ColorDepth::None | ColorDepth::Ansi16 => &[
            "Solarized (dark)",
            "base16-ocean.dark",
            "base16-eighties.dark",
        ],
        _ => &[
            "base16-ocean.dark",
            "base16-default.dark",
            "Solarized (dark)",
            "base16-eighties.dark",
        ],
    };
    for name in candidates {
        if let Some(theme) = ts.themes.get(*name) {
            return Some(theme);
        }
    }
    ts.themes.values().next()
}

fn syntect_color_to_ansi(c: SColor) -> String {
    format!("\x1b[38;2;{};{};{}m", c.r, c.g, c.b)
}

fn to_ansi_line(ranges: &[(SStyle, &str)], code_bg: &str) -> String {
    // Emit the syntect-highlighted text for one line: foreground colors and
    // bold/italic/underline per token. A full reset (`\x1b[0m`) is needed
    // between ranges of differing style to clear modifiers that the next
    // range does not set, but that reset also wipes the code-panel background.
    // So the background (`code_bg`) is re-asserted after every reset; it is
    // a constant for the whole line, so this is safe and cheap. No trailing
    // reset is emitted — the caller closes the background after the padding
    // so the panel reaches the right edge and blank lines stay solid.
    let mut output = String::new();
    let mut current_style: Option<SStyle> = None;

    for (style, text) in ranges {
        if current_style != Some(*style) {
            if current_style.is_some() {
                output.push_str("\x1b[0m");
                // The reset above cleared the panel background; re-assert it.
                output.push_str(code_bg);
            }
            output.push_str(&syntect_color_to_ansi(style.foreground));
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

    output
}

/// Write a full-width frame line (the code panel's background, no text) above
/// or below a code block so the block reads as a closed rectangle.
fn write_code_frame(
    w: &mut impl Write,
    code_bg: &str,
    terminal_width: Option<usize>,
) -> std::io::Result<()> {
    let width = terminal_width.unwrap_or(80).max(1);
    write!(w, "{code_bg}")?;
    write!(w, "{}", " ".repeat(width))?;
    write!(w, "\x1b[0m")?;
    writeln!(w)
}

/// Pad a code line with spaces up to `terminal_width` so the background
/// wrapping it reaches the right edge. `content_width` is the line's display
/// width in columns (measured on the raw text, before any ANSI codes are
/// added), so full-width characters count double and the padding is exact.
/// Long lines are left alone rather than wrapped or clipped, preserving
/// verbatim content.
fn pad_code_line(line: &str, content_width: usize, terminal_width: Option<usize>) -> String {
    let Some(width) = terminal_width else {
        return line.to_string();
    };
    let width = width.max(1);
    if content_width >= width {
        return line.to_string();
    }
    let mut out = line.to_string();
    out.push_str(&" ".repeat(width - content_width));
    out
}

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod tests;
