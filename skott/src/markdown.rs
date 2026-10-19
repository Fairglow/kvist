//! Markdown rendering for the transcript using Blad's parsing and rendering.
//! Wraps Blad's implementation to produce ratatui-compatible output.

use blad::markdown::MarkdownRenderer as BladRenderer;
use blad::markdown::TerminalBg;
use ratatui::style::{Color, Modifier, Style as RStyle};
use ratatui::text::{Line, Span};
use std::io::Cursor;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// One visual transcript row plus a flag marking whether it is code.
#[derive(Debug, Clone)]
pub struct RenderedLine {
    pub line: Line<'static>,
    pub is_code: bool,
}

/// Styling for each Markdown construct. Used by the theme system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownStyles {
    pub paragraph: RStyle,
    pub heading: [RStyle; 6],
    pub code_inline: RStyle,
    pub code_bg: RStyle,
    pub code_label: RStyle,
    pub code_gutter: RStyle,
    pub quote: RStyle,
    pub quote_gutter: RStyle,
    pub link: RStyle,
    pub link_url: RStyle,
    pub html: RStyle,
    pub rule: RStyle,
    pub list_bullet: RStyle,
    pub task_open: RStyle,
    pub task_closed: RStyle,
    pub table_header: RStyle,
    pub table_sep: RStyle,
}

impl Default for MarkdownStyles {
    fn default() -> Self {
        Self {
            paragraph: RStyle::default(),
            heading: [
                RStyle::default().fg(Color::Red),
                RStyle::default().fg(Color::Yellow),
                RStyle::default().fg(Color::Green),
                RStyle::default().fg(Color::Cyan),
                RStyle::default().fg(Color::Blue),
                RStyle::default().fg(Color::Magenta),
            ],
            code_inline: RStyle::default()
                .fg(Color::Rgb(222, 214, 208))
                .bg(Color::Rgb(20, 20, 28)),
            code_bg: RStyle::default().bg(Color::Rgb(20, 20, 28)),
            code_label: RStyle::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
            code_gutter: RStyle::default().fg(Color::Gray),
            quote: RStyle::default().fg(Color::Gray),
            quote_gutter: RStyle::default().fg(Color::DarkGray),
            link: RStyle::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            link_url: RStyle::default().fg(Color::DarkGray),
            html: RStyle::default().fg(Color::Gray),
            rule: RStyle::default().fg(Color::DarkGray),
            list_bullet: RStyle::default().fg(Color::DarkGray),
            task_open: RStyle::default().fg(Color::Gray),
            task_closed: RStyle::default().fg(Color::Green),
            table_header: RStyle::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            table_sep: RStyle::default().fg(Color::DarkGray),
        }
    }
}

/// A configured markdown renderer that holds rendering options.
///
/// Configure it once with your theme and width preferences, then call
/// `render` multiple times with different markdown content. Both theme and
/// width are configuration that's stable during a session and updated in
/// centralized handlers (theme picker, resize event).
#[derive(Debug, Clone)]
pub struct MarkdownRenderer {
    syntax_theme: Option<String>,
    width: usize,
    /// The TUI's actual panel background. When set, it is forwarded to Blad
    /// so the table zebra striping and inverted header use colors that match
    /// the real panel, not the ambient terminal background. `None` means the
    /// TUI didn't declare a panel, so Blad auto-detects from the environment.
    panel_bg: Option<TerminalBg>,
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self {
            syntax_theme: None,
            width: 80,
            panel_bg: None,
        }
    }
}

impl MarkdownRenderer {
    /// Create a new renderer with default options (no syntax theme, 80 columns).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the syntax highlighting theme (e.g., "Monokai", "Dracula").
    /// Pass `None` to disable syntax highlighting.
    pub fn set_syntax_theme(&mut self, theme: Option<&str>) {
        self.syntax_theme = theme.map(|s| s.to_owned());
    }

    /// Set the render width. Call this from the centralized resize handler.
    pub fn set_width(&mut self, width: usize) {
        self.width = width;
    }

    /// Set the TUI panel background mode. Call this from the theme handler so
    /// Blad's table zebra striping and inverted header pick colors that match
    /// the real panel. Pass `None` to fall back to Blad's auto-detection.
    pub fn set_panel_bg(&mut self, bg: Option<TerminalBg>) {
        self.panel_bg = bg;
    }

    /// Render markdown with the renderer's configured options.
    ///
    /// Uses the configured width and theme. For one-off rendering with
    /// different settings, construct a new `MarkdownRenderer` instance.
    pub fn render(&self, md: &str) -> Vec<RenderedLine> {
        // Use Blad's markdown rendering, capture the output, and convert to
        // ratatui-compatible lines.
        let mut output = Cursor::new(Vec::new());
        let mut blade = BladRenderer::new();
        blade.set_width(Some(self.width));
        if let Some(ref theme) = self.syntax_theme {
            blade.set_syntax_theme(Some(theme.as_str()));
        }
        if let Some(bg) = self.panel_bg {
            blade.set_background(bg);
        }
        let _ = blade.render_markdown(md, &mut output);
        let ansi_bytes = output.into_inner();
        let ansi_text = String::from_utf8(ansi_bytes).unwrap_or_default();

        // Parse the ANSI output into styled spans for ratatui
        parse_ansi_to_lines(&ansi_text)
    }
}

/// Render markdown with default options (no syntax theme, 80-column width).
///
/// For repeated rendering with the same configuration, use a
/// `MarkdownRenderer` instance instead.
pub fn render_document(md: &str) -> Vec<RenderedLine> {
    MarkdownRenderer::new().render(md)
}

/// Derive the Blad [`TerminalBg`] mode from a theme's panel background color.
///
/// Theme files always resolve `panel.bg` to a truecolor `Color::Rgb` triple
/// (see `theme::color::parse_color`), so the luminance of that triple decides
/// dark versus light. The `terminal` theme, however, declares `panel.bg` as
/// `Color::Reset` ("use the terminal's own background"), for which the TUI
/// does not know the panel color — that case returns `None` so Blad falls
/// back to its own environment detection. Any other non-`Rgb` color likewise
/// has no luminance to measure, so it also yields `None` and Blad auto-detects
/// (the conservative choice when the panel color is not an explicit RGB triple).
pub fn terminal_bg_for_panel(panel_bg: Color) -> Option<TerminalBg> {
    match panel_bg {
        Color::Rgb(r, g, b) => Some(TerminalBg::from_panel_rgb(r, g, b)),
        _ => None,
    }
}

/// Parse ANSI escape sequences into ratatui-compatible styled lines.
fn parse_ansi_to_lines(ansi_text: &str) -> Vec<RenderedLine> {
    let mut lines = Vec::new();

    for line_text in ansi_text.lines() {
        let spans = parse_ansi_line(line_text);
        let line = Line::from(spans);
        lines.push(RenderedLine {
            line,
            is_code: false,
        });
    }

    lines
}

/// Parse ANSI escape sequences in a single line into styled spans.
fn parse_ansi_line(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current_text = String::new();
    let mut current_style = RStyle::default();
    let mut i = 0;

    while i < line.len() {
        if line.as_bytes()[i] == 0x1b {
            // Found escape sequence
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }

            // Parse the escape sequence
            i = parse_escape_sequence(line, i, &mut current_style);
        } else {
            // `i` is a byte index sitting on a char boundary; consume the
            // whole character so multibyte text is neither misindexed nor
            // split across bytes.
            let ch = line[i..].chars().next().unwrap_or('\u{FFFD}');
            current_text.push(ch);
            i += ch.len_utf8();
        }
    }

    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    if spans.is_empty() {
        spans.push(Span::from(""));
    }

    spans
}

/// Parse a single ANSI escape sequence and update the style.
/// Returns the index after the escape sequence.
fn parse_escape_sequence(line: &str, start: usize, style: &mut RStyle) -> usize {
    let mut i = start + 1;
    if i >= line.len() {
        return i;
    }

    if line.as_bytes()[i] != b'[' {
        return i;
    }
    i += 1;

    // Collect the escape code
    let mut code = String::new();
    while i < line.len() {
        let c = line.as_bytes()[i];
        if c.is_ascii_uppercase() || c.is_ascii_lowercase() {
            code.push(c as char);
            i += 1;
            break;
        }
        code.push(c as char);
        i += 1;
    }

    // Parse the code. The terminating letter is stripped so its digits
    // reach the parameter parser; a truncated escape (an empty code, or a
    // code with no terminating letter) simply sets no style instead of
    // panicking.
    let params: Vec<&str> = code
        .trim_end_matches(|c: char| c.is_ascii_alphabetic())
        .split(';')
        .collect();

    if code.ends_with('m') {
        // SGR (Select Graphic Rendition)
        if params.is_empty() || params[0].trim().is_empty() {
            // Reset
            *style = RStyle::default();
        } else {
            let mut p = 0;
            while p < params.len() {
                let param = params[p].trim();
                if param.is_empty() {
                    *style = RStyle::default();
                    p += 1;
                    continue;
                }
                let Ok(code) = param.parse::<u16>() else {
                    p += 1;
                    continue;
                };
                match code {
                    0 => *style = RStyle::default(),
                    1 => *style = style.add_modifier(Modifier::BOLD),
                    2 => *style = style.add_modifier(Modifier::DIM),
                    3 => *style = style.add_modifier(Modifier::ITALIC),
                    4 => *style = style.add_modifier(Modifier::UNDERLINED),
                    7 => *style = style.add_modifier(Modifier::REVERSED),
                    22 => *style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
                    23 => *style = style.remove_modifier(Modifier::ITALIC),
                    24 => *style = style.remove_modifier(Modifier::UNDERLINED),
                    27 => *style = style.remove_modifier(Modifier::REVERSED),
                    30..=37 => {
                        *style = style.fg(ansi_color_to_ratatui(code - 30));
                    }
                    39 => *style = style.fg(Color::Reset),
                    40..=47 => {
                        *style = style.bg(ansi_color_to_ratatui(code - 40));
                    }
                    49 => *style = style.bg(Color::Reset),
                    90..=97 => {
                        let base = ansi_color_to_ratatui(code - 90);
                        *style = style.fg(match base {
                            Color::Black => Color::DarkGray,
                            Color::Red => Color::LightRed,
                            Color::Green => Color::LightGreen,
                            Color::Yellow => Color::LightYellow,
                            Color::Blue => Color::LightBlue,
                            Color::Magenta => Color::LightMagenta,
                            Color::Cyan => Color::LightCyan,
                            Color::White => Color::White,
                            _ => base,
                        });
                    }
                    100..=107 => {
                        let base = ansi_color_to_ratatui(code - 100);
                        *style = style.bg(match base {
                            Color::Black => Color::DarkGray,
                            Color::Red => Color::LightRed,
                            Color::Green => Color::LightGreen,
                            Color::Yellow => Color::LightYellow,
                            Color::Blue => Color::LightBlue,
                            Color::Magenta => Color::LightMagenta,
                            Color::Cyan => Color::LightCyan,
                            Color::White => Color::White,
                            _ => base,
                        });
                    }
                    38 | 48 => {
                        // Extended color: `38;5;n` / `48;5;n` (256-color) or
                        // `38;2;r;g;b` / `48;2;r;g;b` (truecolor). The mode and
                        // its components live in the following parameters, so
                        // they are consumed here together.
                        let is_bg = code == 48;
                        p += 1;
                        let mode = params.get(p).and_then(|v| v.trim().parse::<u16>().ok());
                        p += 1;
                        let color = match mode {
                            Some(5) => params
                                .get(p)
                                .and_then(|v| v.trim().parse::<u16>().ok())
                                .and_then(|idx| (idx <= 255).then_some(Color::Indexed(idx as u8))),
                            Some(2) => {
                                let r = params.get(p).and_then(|v| v.trim().parse::<u8>().ok());
                                p += 1;
                                let g = params.get(p).and_then(|v| v.trim().parse::<u8>().ok());
                                p += 1;
                                let b = params.get(p).and_then(|v| v.trim().parse::<u8>().ok());
                                match (r, g, b) {
                                    (Some(r), Some(g), Some(b)) => Some(Color::Rgb(r, g, b)),
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        if let Some(color) = color {
                            if is_bg {
                                *style = style.bg(color);
                            } else {
                                *style = style.fg(color);
                            }
                        }
                    }
                    _ => {}
                }
                p += 1;
            }
        }
    }

    i
}

fn ansi_color_to_ratatui(code: u16) -> Color {
    match code {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::White,
        _ => Color::Reset,
    }
}

/// Split a string into chunks whose displayed cell width never exceeds
/// `width`, breaking only on grapheme-cluster boundaries so combining marks,
/// wide characters, and ZWJ sequences are never split. A single grapheme wider
/// than the budget still occupies its own chunk: the terminal will clip or
/// fold it, but a mid-cluster fragment is never emitted. Empty input yields
/// one empty chunk.
pub fn split_cell_chunks(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for grapheme in text.graphemes(true) {
        let grapheme_width = grapheme.width();
        if current_width.saturating_add(grapheme_width) > width && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push_str(grapheme);
        current_width = current_width.saturating_add(grapheme_width);
    }
    if !current.is_empty() || chunks.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_heading() {
        let out = MarkdownRenderer::new().render("# Title\n\nsome text\n");
        assert!(!out.is_empty());
    }

    #[test]
    fn inline_bold() {
        let out = MarkdownRenderer::new().render("a **b** c");
        assert!(!out.is_empty());
    }

    #[test]
    fn inline_code() {
        let out = MarkdownRenderer::new().render("use `cargo build` now");
        assert!(!out.is_empty());
    }

    #[test]
    fn code_block() {
        let out = MarkdownRenderer::new().render("```rust\nfn main() {}\n```\n");
        assert!(!out.is_empty());
    }

    #[test]
    fn table() {
        let out = MarkdownRenderer::new().render("| a | b |\n| --- | --- |\n| 1 | 2 |\n");
        assert!(!out.is_empty());
    }

    #[test]
    fn multibyte_text_is_not_misindexed() {
        // A byte index must not be treated as a char index: 'é' is two
        // bytes but one character, and must survive parsing intact.
        let spans = parse_ansi_line("é");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "é");
        // Multibyte text inside a styled span.
        let spans = parse_ansi_line("\x1b[1mé\x1b[0m");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "é");
    }

    #[test]
    fn truncated_escape_at_end_of_line_is_dropped() {
        // A line ending right after `\x1b[` has no terminating letter: the
        // escape is consumed, no style is set, and nothing panics.
        let spans = parse_ansi_line("\x1b[");
        assert_eq!(spans.len(), 1);
        assert!(spans[0].content.as_ref().is_empty());
        let spans = parse_ansi_line("a\x1b[");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "a");
    }

    #[test]
    fn sgr_codes_apply_style_to_the_following_text() {
        // The terminating `m` must be stripped from the last parameter, or
        // its digits never parse and the style is silently dropped.
        let spans = parse_ansi_line("\x1b[31mred\x1b[0m");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "red");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        let spans = parse_ansi_line("\x1b[1;31merr\x1b[0m");
        assert_eq!(spans[0].content.as_ref(), "err");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn sgr_extended_colors_resolve_background_and_foreground() {
        // Blad's zebra table emits `48;5;n` for the row tint and header bar.
        // The parser must resolve that 256-color background to a ratatui
        // `Color::Indexed`, not drop it (which would flatten the zebra away).
        let spans = parse_ansi_line("\x1b[48;5;235mrow\x1b[0m");
        assert_eq!(spans[0].content.as_ref(), "row");
        assert_eq!(spans[0].style.bg, Some(Color::Indexed(235)));
        // `38;5;n` foreground, `48;2;r;g;b` truecolor background, and a
        // combined bold + indexed-background span (the header row shape).
        let spans = parse_ansi_line("\x1b[38;5;33mf\x1b[0m");
        assert_eq!(spans[0].style.fg, Some(Color::Indexed(33)));
        let spans = parse_ansi_line("\x1b[48;2;10;20;30mc\x1b[0m");
        assert_eq!(spans[0].style.bg, Some(Color::Rgb(10, 20, 30)));
        let spans = parse_ansi_line("\x1b[1;48;5;254mh\x1b[0m");
        assert_eq!(spans[0].content.as_ref(), "h");
        assert_eq!(spans[0].style.bg, Some(Color::Indexed(254)));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn terminal_bg_for_panel_maps_theme_panel_colors() {
        // The dark theme's panel is #000010 (near-black) and the light
        // theme's is white; the conversion must classify each accordingly.
        use blad::markdown::TerminalBg;
        assert_eq!(
            terminal_bg_for_panel(Color::Rgb(0, 0, 16)),
            Some(TerminalBg::Dark),
            "#000010 is a dark panel"
        );
        assert_eq!(
            terminal_bg_for_panel(Color::Rgb(255, 255, 255)),
            Some(TerminalBg::Light),
            "white is a light panel"
        );
        // The `terminal` theme declares panel.bg as Reset (use the terminal's
        // own background), for which the TUI does not know the color, so the
        // conversion returns None and Blad auto-detects.
        assert_eq!(terminal_bg_for_panel(Color::Reset), None);
        // Any other non-Rgb color (e.g. a legacy named color) has no luminance
        // to measure, so it also yields None and Blad auto-detects.
        assert_eq!(terminal_bg_for_panel(Color::Black), None);
    }

    #[test]
    fn split_cell_chunks_is_ascii_stable() {
        assert_eq!(
            split_cell_chunks("abcdefghij", 4),
            vec!["abcd", "efgh", "ij"]
        );
        assert_eq!(split_cell_chunks("abcd", 4), vec!["abcd"]);
    }

    #[test]
    fn split_cell_chunks_empty_yields_one_empty_chunk() {
        assert_eq!(split_cell_chunks("", 4), vec![""]);
    }

    #[test]
    fn split_cell_chunks_never_breaks_grapheme_clusters() {
        // Combining mark, wide CJK, ZWJ emoji, and a plain ASCII tail: 8
        // cells in one word. At budget 4 it splits only on cluster edges.
        let text = "a\u{301}\u{4f60}\u{597d}\u{1f469}\u{200d}\u{1f4bb}b";
        let chunks = split_cell_chunks(text, 4);
        assert_eq!(
            chunks,
            vec![
                "a\u{301}\u{4f60}",
                "\u{597d}\u{1f469}\u{200d}\u{1f4bb}",
                "b"
            ]
        );
        assert!(chunks.iter().all(|c| Span::raw(c).width() <= 4));
        assert!(!chunks.iter().any(|c| c.starts_with('\u{301}')));
    }

    #[test]
    fn split_cell_chunks_combining_mark_stays_with_its_base() {
        // A combining mark may not start a chunk, even at a 1-cell budget.
        assert_eq!(split_cell_chunks("a\u{301}b", 1), vec!["a\u{301}", "b"]);
    }

    #[test]
    fn split_cell_chunks_wide_grapheme_wider_than_budget_stays_whole() {
        // A ZWJ sequence is two cells wide; at a 1-cell budget it cannot be
        // split without breaking the cluster, so it keeps its own chunk.
        assert_eq!(
            split_cell_chunks("\u{1f469}\u{200d}\u{1f4bb}", 1),
            vec!["\u{1f469}\u{200d}\u{1f4bb}"]
        );
    }
}
