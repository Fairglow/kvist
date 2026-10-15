//! Markdown rendering for the transcript using Blad's parsing and rendering.
//! Wraps Blad's implementation to produce ratatui-compatible output.

use blad::markdown::render_markdown_width;
use ratatui::style::{Color, Modifier, Style as RStyle};
use ratatui::text::{Line, Span};
use std::io::Cursor;

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

/// Render markdown and return styled lines for the ratatui transcript.
pub fn render_document(md: &str, width: usize) -> Vec<RenderedLine> {
    render_document_styles(md, width, &MarkdownStyles::default())
}

/// Render markdown with custom styles.
pub fn render_document_styles(
    md: &str,
    width: usize,
    _styles: &MarkdownStyles,
) -> Vec<RenderedLine> {
    // Use Blad's markdown rendering, capture the output, and convert to
    // ratatui-compatible lines.
    let mut output = Cursor::new(Vec::new());
    let _ = render_markdown_width(md, &mut output, Some(width));
    let ansi_bytes = output.into_inner();
    let ansi_text = String::from_utf8(ansi_bytes).unwrap_or_default();

    // Parse the ANSI output into styled spans for ratatui
    parse_ansi_to_lines(&ansi_text)
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
            current_text.push(line.chars().nth(i).unwrap());
            i += 1;
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

    // Parse the code
    let params: Vec<&str> = code
        .trim_end_matches(|c: char| c.is_ascii_uppercase())
        .split(';')
        .collect();
    let final_char = code.chars().last().unwrap();

    if final_char == 'm' {
        // SGR (Select Graphic Rendition)
        if params.is_empty() || params[0].trim().is_empty() {
            // Reset
            *style = RStyle::default();
        } else {
            for param in params {
                let param = param.trim();
                if param.is_empty() {
                    *style = RStyle::default();
                } else if let Ok(code) = param.parse::<u16>() {
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
                            // Bright foreground
                            let base = ansi_color_to_ratatui(code - 90);
                            *style = style.fg(base);
                        }
                        100..=107 => {
                            // Bright background
                            let base = ansi_color_to_ratatui(code - 100);
                            *style = style.bg(base);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    i
}

/// Convert ANSI color code to ratatui Color.
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
        _ => Color::White,
    }
}

/// Split a string into cell-width chunks for terminal display.
pub fn split_cell_chunks(text: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < chars.len() {
        let end = std::cmp::min(start + width, chars.len());
        let chunk: String = chars[start..end].iter().collect();
        chunks.push(chunk);
        start = end;
    }

    if chunks.is_empty() {
        chunks.push(String::new());
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_heading() {
        let out = render_document("# Title\n\nsome text\n", 40);
        assert!(!out.is_empty());
    }

    #[test]
    fn inline_bold() {
        let out = render_document("a **b** c", 40);
        assert!(!out.is_empty());
    }

    #[test]
    fn inline_code() {
        let out = render_document("use `cargo build` now", 40);
        assert!(!out.is_empty());
    }

    #[test]
    fn code_block() {
        let out = render_document("```rust\nfn main() {}\n```\n", 40);
        assert!(!out.is_empty());
    }

    #[test]
    fn table() {
        let out = render_document("| a | b |\n| --- | --- |\n| 1 | 2 |\n", 40);
        assert!(!out.is_empty());
    }
}
