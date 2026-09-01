//! Kvist compatibility surface for standalone prompt acquisition.

use std::path::Path;

use crate::Result;

pub use agent_runtime::MAX_PROMPT_BYTES;

/// Resolves one prompt through the standalone agent-runtime library.
pub fn resolve(prompt: Option<String>, file: Option<&Path>, use_editor: bool) -> Result<String> {
    agent_runtime::resolve_prompt(prompt, file, use_editor).map_err(Into::into)
}
