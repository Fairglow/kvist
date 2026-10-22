//! Explicit appearance and output settings shared by CLI and embedding hosts.

use std::io::{self, Write};

/// A terminal color, independent of the output device's color depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// An xterm palette index; the first sixteen use basic ANSI encodings.
    Ansi(u8),
    /// An explicit sRGB color.
    Rgb(u8, u8, u8),
}

impl Color {
    pub(crate) fn sequence(self, background: bool) -> io::Result<String> {
        let mut bytes = Vec::new();
        self.write(&mut bytes, background)?;
        String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub(crate) fn write(self, w: &mut impl Write, background: bool) -> io::Result<()> {
        match self {
            Self::Ansi(index) => {
                if index >= 16 {
                    return write!(w, "\x1b[{};5;{index}m", if background { 48 } else { 38 });
                }
                let base = if background { 40 } else { 30 };
                write!(
                    w,
                    "\x1b[{}m",
                    base + index % 8 + if index >= 8 { 60 } else { 0 }
                )
            }
            Self::Rgb(r, g, b) => {
                write!(
                    w,
                    "\x1b[{};2;{r};{g};{b}m",
                    if background { 48 } else { 38 }
                )
            }
        }
    }
}

/// Built-in appearance. Library defaults never inspect the environment.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Dark panels and dark-compatible highlighting.
    #[default]
    Dark,
    /// Light panels and light-compatible highlighting.
    Light,
    /// Preserve the terminal's base foreground and background.
    Terminal,
}

/// Available color encoding, resolved by the host before rendering.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    /// No added foreground or background colors.
    None,
    /// The terminal's standard sixteen-color palette.
    Ansi16,
    /// The xterm 256-color palette.
    Ansi256,
    /// Twenty-four-bit RGB.
    #[default]
    TrueColor,
}

/// Mathematical expression presentation.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum MathMode {
    /// Project only the supported grammar into terminal text.
    #[default]
    Unicode,
    /// Preserve original expression source.
    Source,
}

/// Mermaid fence presentation.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum MermaidMode {
    /// Render supported diagrams as terminal text.
    #[default]
    Text,
    /// Preserve original diagram source.
    Source,
}

/// Structural glyphs used by diagrams; authored labels are unchanged.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum DiagramCharset {
    /// Unicode box drawing and arrows.
    #[default]
    Unicode,
    /// ASCII boxes and connectors.
    Ascii,
}

/// Semantic colors supplied by an embedding application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    /// Heading text.
    pub heading: Color,
    /// Link text.
    pub link: Color,
    /// Secondary text and rules.
    pub muted: Color,
    /// List markers.
    pub list_marker: Color,
    /// Mathematical notation.
    pub math: Color,
    /// NOTE, TIP, IMPORTANT, WARNING and CAUTION, in that order.
    pub alerts: [Color; 5],
    /// Viewer chrome.
    pub chrome: Color,
    /// Search indication.
    pub search: Color,
    /// Code-panel background, or the host's unchanged background.
    pub code_background: Option<Color>,
    /// Alternating table-row background, or no fixed fill.
    pub table_background: Option<Color>,
    /// Blockquote fill, or no fixed background.
    pub quote_background: Option<Color>,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            heading: Color::Ansi(4),
            link: Color::Ansi(14),
            muted: Color::Ansi(8),
            list_marker: Color::Ansi(3),
            math: Color::Ansi(13),
            alerts: [6, 2, 5, 3, 1].map(|color| Color::Ansi(color + 8)),
            chrome: Color::Ansi(6),
            search: Color::Ansi(3),
            code_background: Some(Color::Ansi(236)),
            table_background: Some(Color::Ansi(235)),
            quote_background: Some(Color::Ansi(235)),
        }
    }
}

impl Palette {
    /// Built-in semantic defaults for an explicitly resolved appearance.
    pub fn for_theme(theme: Theme) -> Self {
        let mut palette = Self::default();
        if theme != Theme::Dark {
            palette.alerts = [6, 2, 5, 3, 1].map(Color::Ansi);
            palette.math = Color::Ansi(5);
            palette.link = Color::Ansi(6);
        }
        if theme == Theme::Light {
            palette.code_background = Some(Color::Ansi(254));
            palette.table_background = Some(Color::Ansi(254));
            palette.quote_background = Some(Color::Ansi(254));
        } else if theme == Theme::Terminal {
            palette.code_background = None;
            palette.table_background = None;
            palette.quote_background = None;
        }
        palette
    }
}

/// Complete deterministic renderer configuration.
#[derive(Debug, Default, Clone)]
pub struct RenderOptions {
    /// Panel appearance and default semantic palette.
    pub theme: Theme,
    /// Host-resolved color capability.
    pub color_depth: ColorDepth,
    /// Suppress all styling, not just colors, for escape-free output.
    pub plain: bool,
    /// Optional host palette overriding built-in semantic colors.
    pub palette: Option<Palette>,
    /// Mathematical projection policy.
    pub math: MathMode,
    /// Diagram projection policy.
    pub mermaid: MermaidMode,
    /// Diagram structure encoding.
    pub diagram_charset: DiagramCharset,
}

impl RenderOptions {
    /// Escape-free rendered output suitable for dumps and pipelines.
    pub fn plain() -> Self {
        Self {
            plain: true,
            color_depth: ColorDepth::None,
            ..Self::default()
        }
    }

    pub(crate) fn palette(&self) -> Palette {
        self.palette
            .clone()
            .unwrap_or_else(|| Palette::for_theme(self.theme))
    }
}
