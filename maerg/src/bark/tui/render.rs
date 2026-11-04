//! Blad rendering and the sole terminal drawing boundary.

use std::io;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

#[derive(Default)]
struct Styled {
    lines: Vec<Line<'static>>,
    text: String,
    style: Style,
}

impl Styled {
    fn flush(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(Line::default());
        }
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            if let Some(line) = self.lines.last_mut() {
                line.spans.push(Span::styled(text, self.style));
            }
        }
    }
}

impl vte::Perform for Styled {
    fn print(&mut self, ch: char) {
        self.text.push(ch);
    }
    fn execute(&mut self, byte: u8) {
        if byte == b'\n' {
            self.flush();
            self.lines.push(Line::default());
        } else if byte == b'\t' {
            self.text.push_str("    ");
        }
    }
    fn csi_dispatch(&mut self, params: &vte::Params, _: &[u8], _: bool, action: char) {
        if action != 'm' {
            return;
        }
        self.flush();
        let params: Vec<u16> = params
            .iter()
            .map(|p| p.first().copied().unwrap_or(0))
            .collect();
        let mut params = params.iter().copied();
        while let Some(code) = params.next() {
            match code {
                0 => self.style = Style::default(),
                1 => self.style = self.style.add_modifier(Modifier::BOLD),
                2 => self.style = self.style.add_modifier(Modifier::DIM),
                3 => self.style = self.style.add_modifier(Modifier::ITALIC),
                4 => self.style = self.style.add_modifier(Modifier::UNDERLINED),
                7 => self.style = self.style.add_modifier(Modifier::REVERSED),
                9 => self.style = self.style.add_modifier(Modifier::CROSSED_OUT),
                22 => self.style = self.style.remove_modifier(Modifier::BOLD | Modifier::DIM),
                23 => self.style = self.style.remove_modifier(Modifier::ITALIC),
                24 => self.style = self.style.remove_modifier(Modifier::UNDERLINED),
                27 => self.style = self.style.remove_modifier(Modifier::REVERSED),
                29 => self.style = self.style.remove_modifier(Modifier::CROSSED_OUT),
                30..=37 => self.style = self.style.fg(Color::Indexed((code - 30) as u8)),
                40..=47 => self.style = self.style.bg(Color::Indexed((code - 40) as u8)),
                90..=97 => self.style = self.style.fg(Color::Indexed((code - 90 + 8) as u8)),
                100..=107 => self.style = self.style.bg(Color::Indexed((code - 100 + 8) as u8)),
                39 => self.style.fg = None,
                49 => self.style.bg = None,
                38 | 48 => {
                    let color = match params.next() {
                        Some(5) => params
                            .next()
                            .and_then(|index| u8::try_from(index).ok())
                            .map(Color::Indexed),
                        Some(2) => match (params.next(), params.next(), params.next()) {
                            (Some(r), Some(g), Some(b)) => {
                                match (u8::try_from(r), u8::try_from(g), u8::try_from(b)) {
                                    (Ok(r), Ok(g), Ok(b)) => Some(Color::Rgb(r, g, b)),
                                    _ => None,
                                }
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(color) = color {
                        self.style = if code == 38 {
                            self.style.fg(color)
                        } else {
                            self.style.bg(color)
                        };
                    }
                }
                _ => {}
            }
        }
    }
}

fn sgr(code: &str) -> Style {
    let mut styled = Styled::default();
    vte::Parser::new().advance(&mut styled, format!("\x1b[{code}m").as_bytes());
    styled.style
}

pub(super) fn chrome(theme: crate::bark::Theme) -> (Style, Color) {
    let palette = theme.palette();
    let background = sgr(&palette.agent_background).bg.unwrap_or(Color::Black);
    let foreground = sgr(&palette.agent_foreground).fg.unwrap_or(Color::White);
    let accent = sgr(&palette.blue).fg.unwrap_or(Color::Cyan);
    (Style::default().bg(background).fg(foreground), accent)
}

fn blad_color(color: Color) -> blad::Color {
    match color {
        Color::Rgb(r, g, b) => blad::Color::Rgb(r, g, b),
        Color::Indexed(index) => blad::Color::Ansi(index),
        _ => blad::Color::Ansi(7),
    }
}

pub(super) fn markdown(
    source: &str,
    width: u16,
    host: crate::bark::Theme,
) -> io::Result<Text<'static>> {
    let (base, _) = chrome(host);
    let theme = match base.bg {
        Some(Color::Indexed(7 | 15)) => blad::Theme::Light,
        Some(Color::Rgb(r, g, b)) if u16::from(r) + u16::from(g) + u16::from(b) > 384 => {
            blad::Theme::Light
        }
        _ => blad::Theme::Dark,
    };
    let colors = host.palette();
    let background = base.bg.map(blad_color);
    let mut palette = blad::Palette::for_theme(theme);
    palette.heading = blad_color(sgr(&colors.blue).fg.unwrap_or(Color::Cyan));
    palette.link = blad_color(sgr(&colors.cyan).fg.unwrap_or(Color::Cyan));
    palette.math = blad_color(sgr(&colors.magenta).fg.unwrap_or(Color::Magenta));
    palette.code_background = background;
    palette.table_background = background;
    palette.quote_background = background;
    let renderer = {
        let mut renderer = blad::MarkdownRenderer::with_options(blad::RenderOptions {
            theme,
            palette: Some(palette),
            ..Default::default()
        });
        renderer.set_width(Some(usize::from(width.max(1))));
        renderer
    };
    let mut bytes = Vec::new();
    renderer.render_markdown(source, &mut bytes)?;
    let mut styled = Styled::default();
    let mut parser = vte::Parser::new();
    parser.advance(&mut styled, &bytes);
    styled.flush();
    Ok(Text::from(styled.lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_uses_blad_for_tables_code_math_and_diagrams() {
        let source = "# Hello\n\n| A | B |\n|---|---|\n| x | y |\n\n```rust\nlet value = 42;\n```\n\n$\\alpha$\n\n```mermaid\nflowchart LR\n A --> B\n```\n";
        let text = markdown(source, 72, crate::bark::Theme::dark()).unwrap();
        let plain = text
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            plain.contains("Hello") && plain.contains("value") && plain.contains('α'),
            "{plain}"
        );
        assert!(
            !plain.contains("```") && !plain.contains("flowchart LR"),
            "{plain}"
        );
        assert!(
            text.lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.style.fg.is_some())
        );
    }

    #[test]
    fn authored_controls_do_not_become_terminal_actions() {
        let text = markdown(
            "# Hello\x1b]52;c;secret\x07",
            80,
            crate::bark::Theme::dark(),
        )
        .unwrap();
        assert!(
            text.lines
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| !span.content.contains('\x1b'))
        );
    }

    #[test]
    fn custom_palette_reaches_both_chrome_and_markdown() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("theme.toml");
        std::fs::write(&path,
            "name = \"workspace-render-test\"\nmode = \"light\"\n[colors]\nblue = \"#123456\"\nagent_background = \"#eeeeee\"\nagent_foreground = \"#112233\"\n",
        ).unwrap();
        let theme = crate::bark::Theme::by_id(crate::bark::theme::load_theme_file(&path).unwrap());
        let (base, accent) = chrome(theme);
        assert_eq!(base.bg, Some(Color::Rgb(238, 238, 238)));
        assert_eq!(base.fg, Some(Color::Rgb(17, 34, 51)));
        assert_eq!(accent, Color::Rgb(18, 52, 86));
        let text = markdown("# Custom heading", 80, theme).unwrap();
        assert!(
            text.lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.style.fg == Some(accent))
        );
    }
}
