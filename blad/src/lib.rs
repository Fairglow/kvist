//! Blad: first-class Markdown-native transcripts for agent sessions.
//!
//! Blad reads and displays Skott's structured session transcripts.

pub mod display;
pub mod markdown;
pub mod session;

pub use markdown::MarkdownRenderer;
pub use markdown::render_markdown;

mod inline;
mod terminal_text;
mod viewport;
