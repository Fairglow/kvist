//! UI themes for the terminal application: one colour/style table per theme,
//! so every surface (header, transcript, prompt, thinking, code, overlays)
//! draws from the same palette.
//!
//! A theme is external data, not Rust code: `themes/dark.toml` and
//! `themes/light.toml` in the repository are the canonical built-in themes
//! (embedded into the binary as the fallback `Theme::dark()`/`Theme::light()`
//! so agent-runner always has a working theme), and [`file::discover`] lists
//! every theme available — the two built-ins plus any `*.toml` file found in
//! a user's themes directory (see [`file::themes_dir_for_config`]). Dropping
//! a new file there, or editing an existing one, takes effect the next time
//! the theme is picked (the in-app menu re-scans the directory when it
//! opens, and [`file::load`] re-reads the file on every selection): no
//! rebuild, no restart. See `themes/dark.toml` for the full file format.
//!
//! Themes are selected in the configuration (`theme = "dark"`), overridden by
//! the CLI `--theme` flag, cycled live with Ctrl+S, or picked from the
//! in-app menu, which lists every theme [`file::discover`] finds.

mod color;
pub mod file;

use ratatui::style::{Color, Style};

use crate::markdown::MarkdownStyles;

/// The built-in theme names, always available even with no themes directory.
pub const THEME_NAMES: [&str; 2] = ["dark", "light"];

/// The embedded `dark` theme file, compiled in as a fallback so agent-runner
/// always has a working default even when no external theme file exists.
const DARK_TOML: &str = include_str!("../../../themes/dark.toml");
/// The embedded `light` theme file; see [`DARK_TOML`].
const LIGHT_TOML: &str = include_str!("../../../themes/light.toml");

/// The full style table for one UI theme, built by [`file::parse_str`] from a
/// theme file's TOML text — nothing here is hand-written per theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// The theme's selector name: the theme file's name without `.toml`.
    pub name: String,
    /// Header title and overlay titles.
    pub title: Style,
    /// Muted, secondary text (transcript title, hints, dimmed notes).
    pub dim: Color,
    /// Success text (completed turns, finished tools).
    pub ok: Color,
    /// Warnings (cancelled turns, retries, queued notices).
    pub warn: Color,
    /// Errors and failed tool results.
    pub err: Color,
    /// Tool-call lines.
    pub info: Color,
    /// Accent notes (ready banner, new-session notice).
    pub accent: Color,
    /// The selected model name in the header while idle.
    pub model: Color,
    /// The selected model name in the header while a turn generates.
    pub model_active: Color,
    /// The animated spinner glyph in the header.
    pub spinner: Color,
    /// The live stats bar.
    pub stats: Color,
    /// The panel background of the transcript and prompt areas — the standard
    /// terminal background (black in the dark theme, white in the light one).
    pub panel_bg: Color,
    /// The single top edge of each panel (header underline, box top border).
    pub panel_border: Color,
    /// The background of the model-reasoning strip.
    pub reasoning_bg: Color,
    /// The full style of a thinking row: muted foreground plus the strip
    /// background, so reasoning reads as a distinct voice.
    pub reasoning: Style,
    /// The collapsed-thinking placeholder row.
    pub reasoning_placeholder: Style,
    /// The echoed prompt line ("You: …").
    pub prompt: Style,
    /// The right-edge scrollbar glyphs and track, with an explicit
    /// background so the track never falls back to the terminal's default.
    pub scrollbar: Style,
    /// The selected row in the action menu and history overlay, with an
    /// explicit background.
    pub menu_selected: Style,
    /// An unselected menu row, with an explicit background.
    pub menu_plain: Style,
    /// Markdown rendering (headings, inline code, code blocks, tables, …).
    pub markdown: MarkdownStyles,
}

impl Default for Theme {
    /// The default theme is `dark`.
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    /// Looks a built-in selector up; `None` when the name is not `dark` or
    /// `light`. Use [`file::load`] to also search a themes directory.
    pub fn by_name(name: &str) -> Option<Self> {
        let trimmed = name.trim();
        if trimmed.eq_ignore_ascii_case("dark") {
            Some(Self::dark())
        } else if trimmed.eq_ignore_ascii_case("light") {
            Some(Self::light())
        } else {
            None
        }
    }

    /// The next built-in theme in the cycle (`dark` <-> `light`). The live
    /// Ctrl+S toggle in the UI instead cycles every discovered theme; see
    /// `App::cycle_theme`.
    pub fn next(&self) -> Self {
        match self.name.as_str() {
            "dark" => Self::light(),
            _ => Self::dark(),
        }
    }

    /// The default dark theme, parsed from the embedded `themes/dark.toml`.
    pub fn dark() -> Self {
        file::parse_str(DARK_TOML, "dark")
            .expect("themes/dark.toml is a valid, checked-in built-in theme")
    }

    /// The light alternative, parsed from the embedded `themes/light.toml`.
    pub fn light() -> Self {
        file::parse_str(LIGHT_TOML, "light")
            .expect("themes/light.toml is a valid, checked-in built-in theme")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_themes_parse_and_differ() {
        let dark = Theme::dark();
        let light = Theme::light();
        assert_eq!(dark.name, "dark");
        assert_eq!(light.name, "light");
        assert_ne!(dark.panel_bg, light.panel_bg);
        assert_ne!(dark.reasoning_bg, dark.panel_bg);
        assert_ne!(light.reasoning_bg, light.panel_bg);
    }

    #[test]
    fn every_background_bearing_style_has_an_explicit_background() {
        for theme in [Theme::dark(), Theme::light()] {
            assert!(theme.scrollbar.bg.is_some(), "{}: scrollbar.bg", theme.name);
            assert!(
                theme.menu_selected.bg.is_some(),
                "{}: menu_selected.bg",
                theme.name
            );
            assert!(
                theme.menu_plain.bg.is_some(),
                "{}: menu_plain.bg",
                theme.name
            );
        }
    }
}
