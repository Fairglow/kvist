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
/// than the cap (e.g. a very long command on a narrow terminal) extends the
/// box rather than truncating it.
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
    // Content (or the 40-column floor) wins over the terminal cap, so a box
    // is never truncated, and never narrower than 40 columns.
    let width = min_width.clamp(40, cap.max(40).max(min_width));

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
    let rows = rows
        .iter()
        .map(|row| {
            format!(
                "{}{}{}",
                theme.dim("│  "),
                pad_right(row, width.saturating_sub(4)),
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

/// Prints a shell-level error line with a styled `error:` prefix.
pub fn report_error(theme: Theme, message: &str) {
    eprintln!("{} {}", theme.style("1;31", "error:"), message);
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
    fn titled_box_fits_narrow_terminals_and_expands_for_wide_content() {
        let theme = Theme::plain();
        // Small content keeps the 40-column minimum on any terminal.
        let titled = titled_box(theme, "Title", &[], Some(60));
        assert_eq!(visible_len(&titled.top), 40);
        // Content wider than the terminal extends the box (never truncates).
        let wide = vec!["x".repeat(120)];
        let titled = titled_box(theme, "Title", &wide, Some(60));
        assert_eq!(visible_len(&titled.top), 124);
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
}
