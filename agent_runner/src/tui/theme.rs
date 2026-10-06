//! UI themes for the terminal application: one colour/style table per theme,
//! so every surface (header, transcript, prompt, thinking, code, overlays)
//! draws from the same palette.
//!
//! The default `dark` theme keeps the classic look: black panel backgrounds
//! everywhere, a muted tint for model reasoning, and a darker patch under
//! highlighted code blocks. The `light` alternative inverts the panels to
//! white while keeping the same structure, so users on light terminals get the
//! same cues (top edge of each panel, thinking edge, indented code) with
//! appropriate contrast.
//!
//! Themes are selected in the configuration (`theme = "dark"`), overridden by
//! the CLI `--theme` flag, and can be cycled live with Ctrl+S.

use ratatui::style::{Color, Modifier, Style};

use crate::markdown::MarkdownStyles;

/// The selectable theme names, in display order.
pub const THEME_NAMES: [&str; 2] = ["dark", "light"];

/// The full style table for one UI theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// The theme's selector name (`dark`, `light`).
    pub name: &'static str,
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
    /// The right-edge scrollbar glyphs.
    pub scrollbar: Style,
    /// The selected row in the action menu and history overlay.
    pub menu_selected: Style,
    /// An unselected menu row.
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
    /// Looks a selector up; `None` when the name is not a known theme.
    pub fn by_name(name: &str) -> Option<Self> {
        THEME_NAMES
            .iter()
            .find(|&&known| known.eq_ignore_ascii_case(name.trim()))
            .map(|name| match *name {
                "dark" => Self::dark(),
                "light" => Self::light(),
                _ => unreachable!("name came from THEME_NAMES"),
            })
    }

    /// The next theme in the cycle, for the live Ctrl+S toggle.
    pub fn next(&self) -> Self {
        match self.name {
            "dark" => Self::light(),
            _ => Self::dark(),
        }
    }

    /// The default dark theme: black panels, a warm-gray reasoning strip, and
    /// a dark patch under highlighted code.
    pub fn dark() -> Self {
        Self {
            name: "dark",
            title: Style::default().fg(Color::Cyan).bold(),
            dim: Color::DarkGray,
            ok: Color::Green,
            warn: Color::Yellow,
            err: Color::Red,
            info: Color::Blue,
            accent: Color::Cyan,
            model: Color::Green,
            model_active: Color::Yellow,
            spinner: Color::Magenta,
            stats: Color::Green,
            panel_bg: Color::Black,
            panel_border: Color::Gray,
            reasoning_bg: Color::Rgb(38, 36, 44),
            reasoning: Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            reasoning_placeholder: Style::default().fg(Color::DarkGray),
            prompt: Style::default().fg(Color::White).bold(),
            scrollbar: Style::default().fg(Color::DarkGray),
            menu_selected: Style::default().fg(Color::Yellow).bold(),
            menu_plain: Style::default().fg(Color::White),
            markdown: MarkdownStyles::default(),
        }
    }

    /// The light alternative: white panels with the same structural cues —
    /// top panel edge, a left thinking edge, indented code — recoloured for a
    /// light terminal, keeping the muted-foreground contrast of the dark
    /// theme.
    pub fn light() -> Self {
        let ink = Color::Rgb(24, 26, 34);
        let muted = Color::Rgb(108, 112, 122);
        let code_bg = Color::Rgb(236, 236, 244);
        Self {
            name: "light",
            title: Style::default().fg(Color::Rgb(0, 70, 140)).bold(),
            dim: muted,
            ok: Color::Rgb(0, 110, 0),
            warn: Color::Rgb(140, 90, 0),
            err: Color::Rgb(170, 0, 0),
            info: Color::Rgb(0, 70, 160),
            accent: Color::Rgb(0, 90, 110),
            model: Color::Rgb(0, 110, 0),
            model_active: Color::Rgb(140, 90, 0),
            spinner: Color::Rgb(140, 0, 140),
            stats: Color::Rgb(0, 110, 0),
            panel_bg: Color::White,
            panel_border: Color::DarkGray,
            reasoning_bg: Color::Rgb(236, 233, 240),
            reasoning: Style::default()
                .fg(Color::Rgb(92, 92, 104))
                .add_modifier(Modifier::DIM),
            reasoning_placeholder: Style::default().fg(muted),
            prompt: Style::default().fg(Color::Rgb(20, 22, 70)).bold(),
            scrollbar: Style::default().fg(Color::Rgb(130, 132, 140)),
            menu_selected: Style::default().fg(Color::Rgb(140, 90, 0)).bold(),
            menu_plain: Style::default().fg(ink),
            markdown: MarkdownStyles {
                paragraph: Style::default().fg(ink),
                heading: [
                    Style::default().fg(Color::Rgb(170, 0, 0)),
                    Style::default().fg(Color::Rgb(140, 90, 0)),
                    Style::default().fg(Color::Rgb(0, 110, 0)),
                    Style::default().fg(Color::Rgb(0, 90, 110)),
                    Style::default().fg(Color::Rgb(20, 20, 160)),
                    Style::default().fg(Color::Rgb(120, 0, 120)),
                ],
                code_inline: Style::default().fg(Color::Rgb(130, 0, 90)).bg(code_bg),
                code_bg: Style::default().bg(code_bg),
                code_label: Style::default().fg(muted),
                code_gutter: Style::default().fg(Color::Rgb(130, 132, 140)),
                quote: Style::default().fg(Color::Rgb(92, 96, 106)),
                quote_gutter: Style::default().fg(Color::Rgb(130, 132, 140)),
                link: Style::default().fg(Color::Rgb(0, 60, 160)).bold(),
                link_url: Style::default().fg(muted),
                html: Style::default().fg(muted),
                rule: Style::default().fg(Color::Rgb(130, 132, 140)),
                list_bullet: Style::default().fg(muted),
                task_open: Style::default().fg(Color::Rgb(92, 96, 106)),
                task_closed: Style::default().fg(Color::Rgb(0, 110, 0)),
                table_header: Style::default().fg(Color::Rgb(0, 60, 160)).bold(),
                table_sep: Style::default().fg(Color::Rgb(130, 132, 140)),
            },
        }
    }
}
