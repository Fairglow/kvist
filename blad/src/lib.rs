//! Blad: first-class Markdown-native transcripts for agent sessions.
//!
//! Blad reads and displays Skott's structured session transcripts.

pub mod display;
pub mod markdown;
pub mod session;
pub mod theme;

pub use markdown::MarkdownRenderer;
pub use markdown::render_markdown;
pub use theme::{
    Color, ColorDepth, DiagramCharset, MathMode, MermaidMode, Palette, RenderOptions, Theme,
};

mod inline;
mod output;
mod scripts;
mod table;
mod terminal_text;
mod viewport;
