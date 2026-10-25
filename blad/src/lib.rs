//! Standalone terminal Markdown rendering and interactive file viewing.
//!
//! The planned registry package is `kvist-blad` 0.3.0 (not yet published);
//! the library and executable are named `blad`. Rust 1.95 is the minimum
//! supported compiler, not an exact toolchain pin. Linux x86_64 is the verified
//! executable target.
//!
//! # Render without a terminal
//!
//! [`MarkdownRenderer`] writes to any [`std::io::Write`] and propagates errors.
//! It does not inspect environment variables, call `is_terminal()`, read stdin,
//! query terminal capabilities or launch processes. Interactive helpers in
//! [`display`] have separate terminal, filesystem and explicit editor/browser
//! behavior.
//!
//! ```
//! use blad::{MarkdownRenderer, RenderOptions};
//!
//! fn main() -> std::io::Result<()> {
//!     let mut renderer = MarkdownRenderer::with_options(RenderOptions::plain());
//!     renderer.set_width(Some(80));
//!     let mut output = Vec::new();
//!     renderer.render_markdown("# Hello\n\nReadable **Markdown**.\n", &mut output)?;
//!     assert!(!output.contains(&0x1b));
//!     Ok(())
//! }
//! ```
//!
//! # Host-controlled rendering policy
//!
//! [`RenderOptions::default()`] is deterministic: [`Theme::Dark`],
//! [`ColorDepth::TrueColor`], styling enabled, [`MathMode::Unicode`],
//! [`MermaidMode::Text`] and [`DiagramCharset::Unicode`]. The default renderer
//! has no width limit. Hosts may explicitly choose [`ColorDepth::None`],
//! [`ColorDepth::Ansi16`], [`ColorDepth::Ansi256`] or [`ColorDepth::TrueColor`],
//! and supply a semantic [`Palette`]. Colorless output may retain emphasis;
//! [`RenderOptions::plain()`] suppresses all styling for escape-free text.
//! Width and syntax-theme choices persist when options are replaced.
//!
//! CommonMark parsing and selected extensions are terminal projections, not
//! full CommonMark/GFM conformance: raw HTML is omitted (table-cell `<br>` is a
//! line-break exception), images show alternative text only, and table delimiter
//! alignment is not implemented. Math and spatial Mermaid diagrams have bounded
//! subset profiles and visible source fallback, not full TeX/Mermaid support.
//! Authored terminal controls are made inert.
//!
//! [`MarkdownRenderer::render_markdown_with_cancellation`] accepts a cloneable
//! [`RenderCancellation`]. Cancellation and cooperative deadlines return
//! [`std::io::ErrorKind::Interrupted`]; writer errors propagate. Output may
//! already have been written on either error. Rendering starts no worker thread;
//! cooperative limits are not hard time or aggregate-memory guarantees.
//!
//! The optional published `sessions` feature adds Kvist/Skott file-format
//! integration; it is disabled by default and requires no Kvist/Skott runtime.
//! Caller-supplied strings are not subject to the CLI's 16 MiB per-file
//! document-read limit.

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
mod file_input;
mod inline;
pub mod logging;
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
mod viewer_reload;
mod viewer_terminal;
mod viewport;
