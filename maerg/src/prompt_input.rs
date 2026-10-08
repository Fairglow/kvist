//! Kvist compatibility surface for standalone prompt acquisition.

use std::path::Path;

use crate::Result;

pub use sav::MAX_PROMPT_BYTES;

/// Resolves one prompt through the standalone sav library.
pub fn resolve(prompt: Option<String>, file: Option<&Path>, use_editor: bool) -> Result<String> {
    sav::resolve_prompt(prompt, file, use_editor).map_err(Into::into)
}
