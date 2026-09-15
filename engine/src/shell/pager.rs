//! Output paging with a testable, terminal-height-aware policy.
//!
//! [`pager_policy`] is a pure function of the text and the environment/
//! terminal state, so the decision can be unit-tested without a terminal.
//! [`display_output`] applies it and falls back to `println!` whenever the
//! pager is unavailable or fails.

#[cfg(not(test))]
use std::io::IsTerminal;

#[cfg(not(test))]
use minus::Pager;

/// Fallback line threshold when the terminal height cannot be determined.
pub const DEFAULT_PAGER_THRESHOLD: usize = 15;

/// Decides whether `text` should be paged.
///
/// Output is paged only when an interactive pager is possible (stdout is a
/// terminal and `KVIST_NO_PAGER` is unset) and the output would scroll
/// past the screen: it exceeds the terminal height minus the prompt line,
/// or the 15-line fallback when the height is unknown.
pub fn pager_policy(
    text: &str,
    no_pager: bool,
    is_terminal: bool,
    terminal_rows: Option<u16>,
) -> bool {
    if no_pager || !is_terminal {
        return false;
    }
    let threshold = terminal_rows
        .map(|rows| rows.saturating_sub(1) as usize)
        .unwrap_or(DEFAULT_PAGER_THRESHOLD);
    text.lines().count() > threshold
}

/// Displays output text to the user.
///
/// If stdout is an interactive terminal and the output would scroll past the
/// terminal height, the output is paginated cleanly using the `minus` pager;
/// otherwise (and whenever the pager is unavailable or fails) it is printed
/// directly, so output is never lost.
pub fn display_output(text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    // Never enter an interactive pager during automated tests.
    #[cfg(test)]
    {
        println!("{text}");
    }

    #[cfg(not(test))]
    {
        let no_pager = std::env::var_os("KVIST_NO_PAGER").is_some();
        let is_terminal = std::io::stdout().is_terminal();
        let rows =
            super::style::terminal_size().map(|(_, rows)| u16::try_from(rows).unwrap_or(u16::MAX));
        if pager_policy(text, no_pager, is_terminal, rows) {
            let pager = Pager::new();
            if pager.push_str(text).is_ok() && minus::page_all(pager).is_ok() {
                return;
            }
            // A pager that cannot start or page must not swallow output.
        }
        println!("{text}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long_lines(n: usize) -> String {
        (0..n)
            .map(|i| format!("line {i}: component details and metadata"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn display_output_handles_empty_input() {
        display_output("");
        display_output("   \n\t  ");
    }

    #[test]
    fn display_output_handles_short_text() {
        display_output("simple status output");
    }

    #[test]
    fn display_output_handles_long_text() {
        display_output(&long_lines(50));
    }

    #[test]
    fn policy_never_pages_without_a_terminal_or_with_the_env_override() {
        let text = long_lines(100);
        assert!(!pager_policy(&text, false, false, Some(40)));
        assert!(!pager_policy(&text, true, true, Some(40)));
        assert!(!pager_policy(&text, true, false, None));
    }

    #[test]
    fn policy_pages_only_when_the_output_would_scroll() {
        // Known height: 24 rows keep the last row for the prompt, so 23
        // lines fit and the 24th already overflows.
        assert!(!pager_policy(&long_lines(23), false, true, Some(24)));
        assert!(pager_policy(&long_lines(24), false, true, Some(24)));
        assert!(pager_policy(&long_lines(25), false, true, Some(24)));
        // A one-row terminal degrades to a zero threshold: anything pages.
        assert!(pager_policy(&long_lines(1), false, true, Some(1)));
        // Unknown height: the 15-line fallback applies.
        assert!(!pager_policy(&long_lines(15), false, true, None));
        assert!(pager_policy(&long_lines(16), false, true, None));
        assert!(!pager_policy(&long_lines(3), false, true, None));
    }
}
