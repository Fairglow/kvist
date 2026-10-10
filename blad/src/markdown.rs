//! Markdown rendering with syntax highlighting for terminal output.
//!
//! Parses markdown using pulldown-cmark and applies syntax highlighting to
//! fenced code blocks using syntect. Outputs ANSI-colored text to the terminal.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;
use std::io::Write;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SColor, FontStyle, Style as SStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Render markdown with syntax-highlighted code blocks to the terminal.
pub fn render_markdown(md: &str, w: &mut impl Write) -> std::io::Result<()> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = get_terminal_theme(&ts);

    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(md, options);

    let mut in_code_block = false;
    let mut code_language = String::new();
    let mut code_content = String::new();
    let mut hl: Option<HighlightLines> = None;

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
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
                // Process accumulated code
                if let Some(mut highlighter) = hl.take() {
                    for line in code_content.lines() {
                        if let Ok(ranges) = highlighter.highlight_line(line, &ss) {
                            let styled = to_ansi_line(&ranges, &theme);
                            writeln!(w, "{}", styled)?;
                        } else {
                            writeln!(w, "{}", line)?;
                        }
                    }
                    writeln!(w)?;
                }
            }
            Event::Text(text) => {
                if in_code_block {
                    code_content.push_str(&text);
                } else {
                    write!(w, "{}", text)?;
                }
            }
            Event::Code(text) => {
                // Inline code
                write!(w, "\x1b[7m{}\x1b[0m", text)?;
            }
            Event::End(TagEnd::Paragraph) => {
                writeln!(w)?;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let prefix = match level {
                    HeadingLevel::H1 => "### ",
                    HeadingLevel::H2 => "## ",
                    HeadingLevel::H3 => "# ",
                    _ => "",
                };
                write!(w, "\x1b[1m\x1b[34m{}", prefix)?;
            }
            Event::End(TagEnd::Heading(_)) => {
                write!(w, "\x1b[0m")?;
                writeln!(w)?;
            }
            _ => {}
        }
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

    // Language name aliases
    let aliases: HashMap<&str, &str> = HashMap::from([
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
    ]);

    if let Some(name) = aliases.get(lang)
        && let Some(s) = ss.find_syntax_by_name(name)
    {
        return s;
    }

    // Try by name
    if let Some(s) = ss.find_syntax_by_name(lang) {
        return s;
    }

    // Try by extension
    if let Some(s) = ss.find_syntax_by_extension(lang) {
        return s;
    }

    // Try uppercase
    let upper = lang.to_uppercase();
    if let Some(s) = ss.find_syntax_by_name(&upper) {
        return s;
    }

    ss.find_syntax_by_name("Plain Text")
        .unwrap_or(ss.syntaxes().first().unwrap())
}

fn get_terminal_theme(ts: &ThemeSet) -> Theme {
    // Try to find a dark theme suitable for terminals
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

    // Fall back to first theme
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
    // Convert RGB to nearest ANSI 256 color or use true color escape
    format!("\x1b[38;2;{};{};{}m", c.r, c.g, c.b)
}

fn syntect_bg_color_to_ansi(c: SColor) -> String {
    format!("\x1b[48;2;{};{};{}m", c.r, c.g, c.b)
}

fn to_ansi_line(ranges: &[(SStyle, &str)], theme: &Theme) -> String {
    let mut result = String::new();
    let bg = theme.settings.background;

    for (style, text) in ranges {
        let mut escape = String::new();

        escape.push_str(&syntect_color_to_ansi(style.foreground));
        if let Some(ref bg_color) = bg {
            escape.push_str(&syntect_bg_color_to_ansi(*bg_color));
        }
        if style.font_style.contains(FontStyle::BOLD) {
            escape.push_str("\x1b[1m"); // bold
        }

        if !escape.is_empty() {
            result.push_str(&escape);
        }
        result.push_str(text);
        result.push_str("\x1b[0m");
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Strip ANSI escape sequences from a string for testing purposes.
    fn strip_ansi(s: &str) -> String {
        let mut result = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip until 'm' (end of CSI sequence)
                while let Some(&next) = chars.peek() {
                    if next == 'm' {
                        chars.next();
                        break;
                    }
                    chars.next();
                }
            } else {
                result.push(c);
            }
        }
        result
    }

    #[test]
    fn test_find_syntax_rust() {
        let ss = SyntaxSet::load_defaults_newlines();
        let s = find_syntax(&ss, "rust");
        assert_eq!(s.name, "Rust");
    }

    #[test]
    fn test_find_syntax_rs() {
        let ss = SyntaxSet::load_defaults_newlines();
        let s = find_syntax(&ss, "rs");
        assert_eq!(s.name, "Rust");
    }

    #[test]
    fn test_find_syntax_python() {
        let ss = SyntaxSet::load_defaults_newlines();
        let s = find_syntax(&ss, "python");
        assert_eq!(s.name, "Python");
    }

    #[test]
    fn test_find_syntax_js() {
        let ss = SyntaxSet::load_defaults_newlines();
        let s = find_syntax(&ss, "js");
        assert_eq!(s.name, "JavaScript");
    }

    #[test]
    fn test_find_syntax_unknown_falls_back() {
        let ss = SyntaxSet::load_defaults_newlines();
        let s = find_syntax(&ss, "hemlock");
        // Should not panic, returns some syntax (Plain Text or fallback)
        assert!(!s.name.is_empty());
    }

    #[test]
    fn test_render_code_block() {
        let md = "```rust\nlet x = 42;\n```";
        let mut buf = Cursor::new(Vec::new());
        render_markdown(md, &mut buf).unwrap();
        let output = String::from_utf8(buf.into_inner()).unwrap();
        // Should contain ANSI escapes for syntax highlighting
        assert!(output.contains("\x1b[38;2;"));
        let plain = strip_ansi(&output);
        assert!(plain.contains("let x = 42;"));
    }

    #[test]
    fn test_render_multiple_languages() {
        let md = "```rust\nfn main() {}\n```\n\n```python\nx = 1\n```";
        let mut buf = Cursor::new(Vec::new());
        render_markdown(md, &mut buf).unwrap();
        let output = String::from_utf8(buf.into_inner()).unwrap();
        let plain = strip_ansi(&output);
        assert!(plain.contains("fn main()"));
        assert!(plain.contains("x = 1"));
        assert!(output.contains("\x1b[38;2;"));
    }
}
