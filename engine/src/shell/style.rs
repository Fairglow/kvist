//! Terminal theming and layout helpers for the workspace shell.
//!
//! Color is a presentation layer that degrades to plain text:
//! [`Theme::detect`] honors `NO_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`,
//! `TERM=dumb`, and terminal detection, so captured, piped, or dumb-terminal
//! output never contains escape sequences. All renderers are pure functions
//! of their inputs (including the theme), so they are testable without a
//! terminal.
//!
//! Layouts adapt to the terminal width: boxes are sized to their content,
//! capped by the probed terminal width, and fall back to 80 columns when the
//! size cannot be determined.

use std::io::IsTerminal;

/// A resolved terminal theme: ANSI-styled or plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Theme {
    /// Whether ANSI styling is emitted.
    enabled: bool,
}

impl Theme {
    /// A plain-text theme: every style helper returns its input unchanged.
    pub const fn plain() -> Self {
        Self { enabled: false }
    }

    /// A styling theme: every style helper emits ANSI SGR sequences.
    pub const fn enabled() -> Self {
        Self { enabled: true }
    }

    /// Detects the theme from the environment and terminal state.
    ///
    /// `NO_COLOR` (present with any value) disables styling, as do
    /// `CLICOLOR=0` and `TERM=dumb`; `CLICOLOR_FORCE` (anything but `0`)
    /// enables styling even when stdout is not a terminal; otherwise styling
    /// requires a terminal stdout.
    pub fn detect() -> Self {
        if std::env::var_os("NO_COLOR").is_some()
            || std::env::var_os("CLICOLOR").is_some_and(|value| value == "0")
            || std::env::var_os("TERM").is_some_and(|value| value == "dumb")
        {
            return Self::plain();
        }
        if std::env::var_os("CLICOLOR_FORCE").is_some_and(|value| value != "0") {
            return Self { enabled: true };
        }
        Self {
            enabled: std::io::stdout().is_terminal(),
        }
    }

    /// Whether ANSI styling is emitted.
    pub const fn is_enabled(self) -> bool {
        self.enabled
    }

    /// Applies one SGR code, or returns the text unchanged when styling is
    /// disabled.
    pub fn style(self, code: &str, text: &str) -> String {
        if !self.enabled {
            return text.to_owned();
        }
        format!("\x1b[{code}m{text}\x1b[0m")
    }

    /// Bold text.
    pub fn bold(self, text: &str) -> String {
        self.style("1", text)
    }

    /// Faint (dimmed) text.
    pub fn dim(self, text: &str) -> String {
        self.style("2", text)
    }

    /// Red text (errors, failures, stale state).
    pub fn red(self, text: &str) -> String {
        self.style("31", text)
    }

    /// Green text (success, live state, the prompt marker).
    pub fn green(self, text: &str) -> String {
        self.style("32", text)
    }

    /// Yellow text (caution, the next-ready marker).
    pub fn yellow(self, text: &str) -> String {
        self.style("33", text)
    }

    /// Blue text (the prompt verb).
    pub fn blue(self, text: &str) -> String {
        self.style("34", text)
    }

    /// Magenta text (component context).
    pub fn magenta(self, text: &str) -> String {
        self.style("35", text)
    }

    /// Cyan text (in-progress state).
    pub fn cyan(self, text: &str) -> String {
        self.style("36", text)
    }
}

/// Removes ANSI SGR escape sequences so widths can be computed on visible
/// text.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The visible (ANSI-stripped) column width of a line.
pub fn visible_len(text: &str) -> usize {
    strip_ansi(text).chars().count()
}

/// Pads `text` on the right to a visible `width` in columns, so styled
/// fragments stay column-aligned in tables.
pub(crate) fn pad_right(text: &str, width: usize) -> String {
    let pad = width.saturating_sub(visible_len(text));
    let mut out = text.to_owned();
    out.push_str(&" ".repeat(pad));
    out
}

/// One styled span of a line: a text fragment with its SGR code (`None` for
/// plain text). Shell styling is a single open plus a reset per span.
struct Span {
    code: Option<String>,
    text: String,
}

/// One breakable unit with the SGR code of the span it came from.
struct Token {
    code: Option<String>,
    /// Break point after the word prefix (0 = one unbreakable unit).
    break_width: usize,
    text: String,
}

/// Parses a line into styled spans. The shell emits one SGR open per span
/// plus a reset, so the parser tracks the open code until `\x1b[0m`.
fn parse_spans(line: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut rest = line;
    while let Some(offset) = rest.find('\x1b') {
        let (plain, after) = rest.split_at(offset);
        if !plain.is_empty() {
            spans.push(Span {
                code: None,
                text: plain.to_owned(),
            });
        }
        // `after` starts with ESC; try to parse `ESC[<params>m ... ESC[0m`.
        if after.get(1..2) == Some("[") {
            let params = &after[2..];
            if let Some(code_end) = params.find(|c: char| c.is_ascii_alphabetic())
                && params.get(code_end..code_end + 1) == Some("m")
            {
                let start = 2 + code_end + 1;
                // The span runs to its reset: the text stops where the reset
                // begins, and the reset itself is consumed so a following span
                // (or plain text) parses cleanly.
                let (text_end, rest_start) = match after[start..].find("\x1b[0m") {
                    Some(found) => {
                        let reset_at = start + found;
                        (reset_at, reset_at + "\x1b[0m".len())
                    }
                    None => (after.len(), after.len()),
                };
                spans.push(Span {
                    code: Some(params[..code_end].to_owned()),
                    text: after[start..text_end].to_owned(),
                });
                rest = &after[rest_start..];
                continue;
            }
        }
        // Not a recognized SGR sequence: keep the escape as plain text.
        let first = after.chars().next().unwrap();
        spans.push(Span {
            code: None,
            text: first.to_string(),
        });
        rest = &after[first.len_utf8()..];
    }
    if !rest.is_empty() {
        spans.push(Span {
            code: None,
            text: rest.to_owned(),
        });
    }
    spans
}

/// Characters that stay attached to a word when wrapping (identifiers, paths,
/// command fragments, key combos like `Ctrl+L`, and non-ASCII symbols);
/// anything else that is not whitespace is punctuation.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || !c.is_ascii() || matches!(c, '-' | '_' | '.' | '/' | '\\' | '+')
}

/// The visible width of the token's word prefix: where a line may break
/// before the token's trailing punctuation (the colon of `ready:`), or zero
/// when the token is one unbreakable unit.
fn break_width(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut end = chars.len();
    while end > 0 && !is_word_char(chars[end - 1]) {
        end -= 1;
    }
    (end > 0 && end < chars.len() && is_word_char(chars[end - 1])) as usize * end
}

impl Token {
    /// The width used when deciding where the token may start on a line.
    fn decision_width(&self) -> usize {
        if self.break_width > 0 {
            self.break_width
        } else {
            self.text.chars().count()
        }
    }

    /// The trailing punctuation of a breakable token, if the break point
    /// separates it from the word (`ready:` -> `:`).
    fn suffix(&self) -> Option<String> {
        if self.break_width > 0 && self.break_width < self.text.chars().count() {
            let at = self
                .text
                .char_indices()
                .nth(self.break_width)
                .map(|(i, _)| i)?;
            Some(self.text[at..].to_owned())
        } else {
            None
        }
    }

    /// The word prefix of a breakable token (`ready:` -> `ready`).
    fn prefix(&self) -> Option<String> {
        if self.break_width > 0 && self.break_width < self.text.chars().count() {
            let at = self
                .text
                .char_indices()
                .nth(self.break_width)
                .map(|(i, _)| i)?;
            Some(self.text[..at].to_owned())
        } else {
            None
        }
    }
}

/// Tokenizes the spans: a maximal non-whitespace chunk is one token (so
/// `a:b` and `Ctrl+L` never split apart); whitespace is dropped and
/// re-emitted between tokens. A chunk's trailing punctuation (the colon of
/// `ready:`) records a break point after the word.
fn tokenize(spans: &[Span]) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut chunk = String::new();
    let mut chunk_code: Option<String> = None;
    for span in spans {
        for c in span.text.chars() {
            if c.is_whitespace() {
                if !chunk.is_empty() {
                    tokens.push(Token {
                        code: chunk_code.clone(),
                        break_width: break_width(&chunk),
                        text: std::mem::take(&mut chunk),
                    });
                }
            } else {
                if chunk.is_empty() {
                    chunk_code = span.code.clone();
                } else if chunk_code.as_ref() != span.code.as_ref() {
                    // A span boundary mid-chunk: keep the pieces separate.
                    tokens.push(Token {
                        code: chunk_code.clone(),
                        break_width: break_width(&chunk),
                        text: std::mem::take(&mut chunk),
                    });
                    chunk_code = span.code.clone();
                }
                chunk.push(c);
            }
        }
    }
    if !chunk.is_empty() {
        tokens.push(Token {
            code: chunk_code,
            break_width: break_width(&chunk),
            text: chunk,
        });
    }
    tokens
}

/// Re-emits one physical line: the indentation, then the tokens with one
/// space between them, grouping consecutive same-style tokens under a single
/// SGR open and reset.
fn render_tokens(indent: &str, tokens: &[Token]) -> String {
    let mut out = String::from(indent);
    let mut open: Option<&str> = None;
    for (i, token) in tokens.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let code = token.code.as_deref();
        if code != open {
            if open.is_some() {
                out.push_str("\x1b[0m");
            }
            if let Some(code) = code {
                out.push_str(&format!("\x1b[{code}m"));
            }
            open = code;
        }
        out.push_str(&token.text);
    }
    if open.is_some() {
        out.push_str("\x1b[0m");
    }
    out
}

/// Wraps one line to a visible `width`, splitting at word boundaries and
/// hard-splitting unbreakable words. Every physical line keeps the source
/// line's leading whitespace and the styling of its spans, so text blocks
/// keep their indentation and ANSI styling survives wrapping. A line that
/// already fits (or a width of zero) is returned unchanged.
/// Places `token` onto the current physical line, starting a new line (or
/// hard-splitting an over-long token) as needed. `used` tracks the current
/// line's visible width; `physical` is appended in place.
fn place_token(physical: &mut Vec<Vec<Token>>, used: &mut usize, avail: usize, token: Token) {
    let len = token.text.chars().count();
    let dw = token.decision_width();
    let current_empty = physical.last().unwrap().is_empty();
    if !current_empty && *used + 1 + dw <= avail {
        // The word prefix fits on the current line.
        if token.suffix().is_none() || *used + 1 + len <= avail {
            physical.last_mut().unwrap().push(token);
            *used += 1 + len;
            return;
        }
        // The word fits, its trailing punctuation does not: the line ends
        // after the word and the punctuation starts the next line.
        let (prefix, suffix) = (token.prefix().unwrap(), token.suffix().unwrap());
        let suffix_len = suffix.chars().count();
        physical.last_mut().unwrap().push(Token {
            code: token.code.clone(),
            break_width: 0,
            text: prefix,
        });
        *used += 1 + dw;
        physical.push(vec![Token {
            code: token.code,
            break_width: 0,
            text: suffix,
        }]);
        *used = suffix_len;
        return;
    }
    // The token does not fit on the current line (or starts a new one).
    if !current_empty {
        physical.push(Vec::new());
    }
    if len <= avail {
        physical.last_mut().unwrap().push(token);
        *used = len;
        return;
    }
    if token.suffix().is_some() && dw <= avail {
        // The word prefix fills the new line; the punctuation goes after it.
        let (prefix, suffix) = (token.prefix().unwrap(), token.suffix().unwrap());
        let suffix_len = suffix.chars().count();
        physical.last_mut().unwrap().push(Token {
            code: token.code.clone(),
            break_width: 0,
            text: prefix,
        });
        *used = dw;
        physical.push(vec![Token {
            code: token.code,
            break_width: 0,
            text: suffix,
        }]);
        *used = suffix_len;
        return;
    }
    // Hard-split the unbreakable token at the available width: the first
    // chunk fills the current line, each further chunk gets its own line.
    let code = token.code.clone();
    let mut text = token.text;
    let mut first = true;
    while text.chars().count() > avail {
        let cut = text
            .char_indices()
            .nth(avail)
            .map(|(i, _)| i)
            .unwrap_or(text.len());
        let (head, tail) = text.split_at(cut);
        if first {
            physical.last_mut().unwrap().push(Token {
                code: code.clone(),
                break_width: 0,
                text: head.to_owned(),
            });
            first = false;
        } else {
            physical.push(vec![Token {
                code: code.clone(),
                break_width: 0,
                text: head.to_owned(),
            }]);
        }
        text = tail.to_owned();
    }
    if text.is_empty() {
        *used = avail;
    } else {
        physical.push(vec![Token {
            code,
            break_width: 0,
            text,
        }]);
        *used = physical.last().unwrap()[0].text.chars().count();
    }
}

pub fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 || visible_len(line) <= width {
        return vec![line.to_owned()];
    }
    let indent_len = line.len() - line.trim_start().len();
    let indent = &line[..indent_len];
    let avail = width.saturating_sub(indent.chars().count()).max(1);
    let tokens = tokenize(&parse_spans(line));
    let mut physical: Vec<Vec<Token>> = vec![Vec::new()];
    let mut used = 0usize;
    for token in tokens {
        place_token(&mut physical, &mut used, avail, token);
    }
    physical
        .iter()
        .map(|tokens| render_tokens(indent, tokens))
        .collect()
}

/// Wraps one framed status-report line so every physical line keeps the frame
/// at exactly `width` visible columns: a fully framed `│content│` line is
/// re-drawn with both borders, a left-framed `│  content` overview line keeps
/// its left border and right padding, and a plain line falls back to
/// [`wrap_line`]. A line that already fits is returned unchanged.
pub fn wrap_bordered_line(line: &str, width: usize) -> Vec<String> {
    if visible_len(line) <= width {
        return vec![line.to_owned()];
    }
    let Some(rest) = line.strip_prefix('│') else {
        return wrap_line(line, width);
    };
    let framed_right = rest.ends_with('│');
    let inner_end = if framed_right {
        rest.len().saturating_sub('│'.len_utf8())
    } else {
        rest.len()
    };
    let inner = rest[..inner_end].trim_end();
    let frame = if framed_right { 2 } else { 1 };
    let inner_width = width.saturating_sub(frame).max(1);
    wrap_line(inner, inner_width)
        .into_iter()
        .map(|wrapped| {
            if framed_right {
                format!("{}{}{}", "│", pad_right(&wrapped, inner_width), "│")
            } else {
                format!("{}{}", "│", pad_right(&wrapped, inner_width))
            }
        })
        .collect()
}

/// A rendered titled box: its top rule, content rows, and bottom rule, all
/// the same visible width.
pub struct TitledBox {
    /// The top rule with the bold title: `╭── <title> ────╮`.
    pub top: String,
    /// One `│  <row> │` line per content row.
    pub rows: Vec<String>,
    /// The bottom rule: `╰─────╯`.
    pub bottom: String,
}

/// Renders a titled box that fits its content, capped by the terminal width.
///
/// The box is at least 40 columns wide and never wider than the terminal
/// width minus a margin, or 100 columns, whichever is smaller. Content wider
/// than the cap (e.g. a very long command on a narrow terminal) wraps to the
/// box's inner width, so the border stays intact on every physical line.
pub fn titled_box(
    theme: Theme,
    title: &str,
    rows: &[String],
    terminal_width: Option<usize>,
) -> TitledBox {
    let content_width = rows.iter().map(|row| visible_len(row)).max().unwrap_or(0);
    let min_width = content_width.max(visible_len(title)).saturating_add(4);
    let cap = terminal_width
        .map(|width| width.saturating_sub(2))
        .unwrap_or(80)
        .min(100);
    // The box grows with its content up to the terminal cap (and never below
    // the 40-column floor); wider rows wrap to the inner width below.
    let width = min_width.clamp(40, cap.max(40));

    let title_len = visible_len(title);
    // The top rule is `╭── <title> ──╮`; when the title nearly fills the box
    // the double dash collapses to a single one so every line stays the same
    // visible width.
    let (prefix, fill) = if title_len + 5 <= width {
        ("╭── ", width.saturating_sub(5).saturating_sub(title_len))
    } else {
        ("╭ ", width.saturating_sub(3).saturating_sub(title_len))
    };
    let top = format!(
        "{}{}{}{}",
        theme.dim(prefix),
        theme.bold(title),
        theme.dim(&"─".repeat(fill)),
        theme.dim("╮")
    );
    let bottom = theme.dim(&format!("╰{}╯", "─".repeat(width.saturating_sub(2))));
    // Wrap rows to the inner width before sizing the frame, so a wide row
    // becomes several bordered physical lines instead of a broken frame.
    let inner = width.saturating_sub(4);
    let rows = rows
        .iter()
        .flat_map(|row| wrap_line(row, inner))
        .map(|row| {
            format!(
                "{}{}{}",
                theme.dim("│  "),
                pad_right(&row, inner),
                theme.dim("│")
            )
        })
        .collect();
    TitledBox { top, rows, bottom }
}

/// The probed terminal size in (columns, rows), when it can be determined.
pub fn terminal_size() -> Option<(usize, usize)> {
    terminal_size::terminal_size().map(|(cols, rows)| (cols.0 as usize, rows.0 as usize))
}

/// Prints a shell-level error with a styled `error:` prefix; multi-line
/// messages keep their continuation lines aligned under the message text.
pub fn report_error(theme: Theme, message: &str) {
    let prefix = theme.style("1;31", "error:");
    let indent = " ".repeat(visible_len(&prefix) + 1);
    let mut first = true;
    for line in message.lines() {
        if first {
            eprintln!("{prefix} {line}");
            first = false;
        } else {
            eprintln!("{indent}{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_theme_is_the_identity() {
        let theme = Theme::plain();
        assert!(!theme.is_enabled());
        assert_eq!(theme.bold("x"), "x");
        assert_eq!(theme.dim("y"), "y");
        assert_eq!(theme.style("1;31", "z"), "z");
    }

    #[test]
    fn enabled_theme_wraps_in_sgr_and_resets() {
        let theme = Theme { enabled: true };
        assert_eq!(theme.bold("x"), "\x1b[1mx\x1b[0m");
        assert_eq!(theme.style("1;31", "err"), "\x1b[1;31merr\x1b[0m");
    }

    #[test]
    fn visible_len_ignores_ansi_escapes() {
        assert_eq!(visible_len("plain"), 5);
        assert_eq!(visible_len("\x1b[1m\x1b[31mred\x1b[0m"), 3);
        assert_eq!(visible_len("\x1b[2m[3]\x1b[0m"), 3);
        // A stray escape without an SGR terminator is dropped, not counted.
        assert_eq!(visible_len("a\x1bb"), 2);
    }

    #[test]
    fn strip_ansi_keeps_multibyte_text_intact() {
        assert_eq!(strip_ansi("\x1b[1möké\x1b[0m"), "öké");
    }

    #[test]
    fn titled_box_lines_share_one_visible_width() {
        let theme = Theme::plain();
        let rows = vec![
            "Branch:   main".to_owned(),
            "Locks:    1 active, 2 stale (run `locks clean`)".to_owned(),
        ];
        let titled = titled_box(theme, "Kvist Shell", &rows, Some(100));
        for line in [titled.top.as_str(), titled.bottom.as_str()]
            .into_iter()
            .chain(titled.rows.iter().map(String::as_str))
        {
            // The Locks row is 47 columns wide, so the box is 47 + 4 = 51.
            assert_eq!(visible_len(line), 51, "line `{line}`");
        }
        assert!(titled.top.starts_with("╭── Kvist Shell"));
        assert!(titled.top.ends_with('╮'));
        assert!(titled.bottom.starts_with('╰'));
        assert!(titled.bottom.ends_with('╯'));
        assert!(titled.rows[1].starts_with("│  Locks:    1 active"));
        assert!(titled.rows[0].starts_with("│  Branch:   main"));
    }

    #[test]
    fn titled_box_fits_narrow_terminals_and_wraps_wide_content() {
        let theme = Theme::plain();
        // Small content keeps the 40-column minimum on any terminal.
        let titled = titled_box(theme, "Title", &[], Some(60));
        assert_eq!(visible_len(&titled.top), 40);
        // Content wider than the terminal wraps at the cap (never extends
        // the box past the terminal).
        let wide = vec!["x".repeat(120)];
        let titled = titled_box(theme, "Title", &wide, Some(60));
        assert_eq!(visible_len(&titled.top), 58);
        assert!(titled.rows.len() >= 2, "wide row must wrap");
        // Unknown terminal size falls back to an 80-column cap.
        let titled = titled_box(theme, "Title", &[], None);
        assert_eq!(visible_len(&titled.top), 40);
    }

    #[test]
    fn titled_box_never_goes_below_40_columns() {
        let theme = Theme::plain();
        let titled = titled_box(theme, "S", &[], Some(10));
        assert_eq!(visible_len(&titled.top), 40);
    }

    #[test]
    fn report_error_prefix_is_plain_under_a_plain_theme() {
        // Smoke: must not panic and must contain the message.
        report_error(Theme::plain(), "boom");
    }

    #[test]
    fn wrap_line_is_the_identity_when_the_line_fits() {
        assert_eq!(wrap_line("hello world", 20), vec!["hello world"]);
        assert_eq!(wrap_line("hello", 5), vec!["hello"]);
        assert_eq!(wrap_line("  indented text", 20), vec!["  indented text"]);
        assert_eq!(wrap_line("", 10), vec![""]);
    }

    #[test]
    fn wrap_line_breaks_at_word_boundaries() {
        assert_eq!(wrap_line("hello world foo", 10), vec!["hello", "world foo"]);
        assert_eq!(
            wrap_line("one two three four", 8),
            vec!["one two", "three", "four"]
        );
    }

    #[test]
    fn wrap_line_keeps_the_line_indentation_on_continuations() {
        // A two-space text block keeps its indentation on every physical line.
        assert_eq!(
            wrap_line("  hello world foo bar", 12),
            vec!["  hello", "  world foo", "  bar"]
        );
        assert_eq!(
            wrap_line("    alpha beta gamma", 12),
            vec!["    alpha", "    beta", "    gamma"]
        );
    }

    #[test]
    fn wrap_line_hard_splits_unbreakable_words() {
        assert_eq!(wrap_line("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(
            wrap_line("ab longword cd", 6),
            vec!["ab", "longwo", "rd cd"]
        );
    }

    #[test]
    fn wrap_line_re_emits_styling_on_every_physical_line() {
        let styled = "\x1b[2mdim text here\x1b[0m";
        let wrapped = wrap_line(styled, 8);
        assert!(wrapped.len() > 1, "styled line must wrap");
        for line in &wrapped {
            assert!(
                line.starts_with("\x1b[2m"),
                "every physical line keeps the span: {line:?}"
            );
            assert!(
                line.ends_with("\x1b[0m"),
                "every physical line resets: {line:?}"
            );
        }
        // The physical lines keep the words in order (joined with the space
        // the wrap replaced by a line break).
        assert_eq!(strip_ansi(&wrapped.join(" ")), "dim text here");
        for line in &wrapped {
            assert!(visible_len(line) <= 8, "line overflows: {line:?}");
        }
    }

    #[test]
    fn wrap_line_handles_mixed_styled_and_plain_spans() {
        let mixed = "\x1b[1mnext ready:\x1b[0m ";
        let wrapped = wrap_line(&format!("{mixed}\x1b[1msome long task id\x1b[0m"), 10);
        // The visible content is preserved in order across the physical lines.
        let visible: Vec<String> = wrapped.iter().map(|line| strip_ansi(line)).collect();
        assert!(visible.iter().any(|line| line == "next ready"));
        assert!(visible.iter().any(|line| line.contains("some")));
        assert!(visible.iter().any(|line| line == "long task"));
        assert!(visible.iter().any(|line| line == "id"));
        for line in &wrapped {
            assert!(visible_len(line) <= 10, "line overflows: {line:?}");
        }
    }

    #[test]
    fn wrap_bordered_line_keeps_the_frame_on_every_physical_line() {
        let line = "│  hello world foo bar    │";
        let wrapped = wrap_bordered_line(line, 16);
        assert_eq!(wrapped.len(), 2);
        for physical in &wrapped {
            assert!(physical.starts_with('│'), "border left: {physical:?}");
            assert!(physical.ends_with('│'), "border right: {physical:?}");
            assert_eq!(visible_len(physical), 16, "frame width: {physical:?}");
        }
        assert!(wrapped[0].contains("hello world"));
        assert!(wrapped[1].contains("foo bar"));
    }

    #[test]
    fn wrap_bordered_line_is_the_identity_when_the_line_fits() {
        // Exactly 16 visible columns: frame (2) + inner padding (14).
        let line = "│  short       │";
        assert_eq!(visible_len(line), 16);
        assert_eq!(wrap_bordered_line(line, 16), vec![line]);
    }

    #[test]
    fn wrap_bordered_line_falls_back_to_plain_wrap_for_unframed_lines() {
        assert_eq!(
            wrap_bordered_line("plain long line here", 12),
            vec!["plain long", "line here"]
        );
    }

    #[test]
    fn titled_box_wraps_wide_content_instead_of_extending_past_the_terminal() {
        let theme = Theme::plain();
        let wide = vec!["x ".repeat(60).trim_end().to_owned()];
        let titled = titled_box(theme, "Title", &wide, Some(60));
        // The box stays within the terminal cap (58) and the content wraps.
        assert_eq!(visible_len(&titled.top), 58);
        assert!(titled.rows.len() >= 2, "wide row must wrap");
        for line in [titled.top.as_str(), titled.bottom.as_str()]
            .into_iter()
            .chain(titled.rows.iter().map(String::as_str))
        {
            assert_eq!(visible_len(line), 58, "line `{line}`");
        }
    }
}
