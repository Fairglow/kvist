//! Blad: first-class Markdown-native transcripts for agent sessions.
//!
//! Blad reads and displays Skott's structured session transcripts.

pub mod display;
pub mod links;
pub mod markdown;
#[cfg(feature = "sessions")]
pub mod session;
pub mod theme;

pub use diagram::RenderCancellation;
pub use markdown::MarkdownRenderer;
pub use markdown::render_markdown;
pub use theme::{
    Color, ColorDepth, DiagramCharset, MathMode, MermaidMode, Palette, RenderOptions, Theme,
};

mod browser;
mod diagram;
mod inline;
mod math;
mod mermaid;
mod output;
mod render_writer;
mod scripts;
mod search;
#[cfg(feature = "sessions")]
mod session_display;
mod table;
mod table_navigation;
mod terminal_text;
mod viewer_navigation;
mod viewport;
