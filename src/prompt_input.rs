//! Kvist compatibility surface for standalone prompt acquisition.

use std::path::Path;

use crate::Result;

pub use supervised_agent::MAX_PROMPT_BYTES;

/// Resolves one prompt through the standalone supervised-agent library.
pub fn resolve(prompt: Option<String>, file: Option<&Path>, use_editor: bool) -> Result<String> {
    supervised_agent::resolve_prompt(prompt, file, use_editor).map_err(Into::into)
}
