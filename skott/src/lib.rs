//! The skott library.
//!
//! Skott is a first-class, sandbox-integrated interactive agent shell.
//! It shares Kvist's bounded `sav` mechanisms and its independently
//! installed sandbox enforcement boundary. The library exposes configuration,
//! the tool model, the sandbox request/execution layer, the transport-agnostic
//! agent loop, rolling context management, and the durable session log; the
//! binary wires these to a terminal UI.

#![forbid(unsafe_code)]

mod cli;
mod host;
mod kvist_import;
mod logging;
mod markdown;
mod process;
mod run;

pub mod config;
pub mod context;
pub mod error;
pub mod executor;
pub mod file_tools;
pub mod headless;
pub mod history;
pub mod retry;
pub mod sandbox;
pub mod session;
pub mod session_log;
pub mod toolchain;
pub mod tools;
pub mod tui;

pub use cli::{Cli, parse_effort, resolve_config_path};
pub use config::{Config, DEFAULT_WRITE_ROOT, Model, ModelProvider, SandboxPaths, ToolPolicy};
pub use context::{
    Compaction, ContextManager, DEFAULT_CONTEXT_TOKENS, estimate_messages, estimate_tokens,
};
pub use error::{Error, Result};
pub use executor::SandboxExecutor;
pub use host::HostExecutor;
pub use kvist_import::{KVIST_CONFIG_FILE, import_models, resolve_kvist_config_path};
pub use markdown::{MarkdownStyles, RenderedLine, render_document};
pub use retry::{
    DEFAULT_MAX_ATTEMPTS, DEFAULT_RETRY_BASE_DELAY, DEFAULT_RETRY_MAX_DELAY, RetryPolicy,
};
pub use run::system_prompt;
pub use sandbox::ToolOutcome;
pub use session::{
    Skott, AgentSession, Event, EventSink, MAX_TURNS, Recorder, RunLimits, RunSummary,
    ToolExecutor,
};
pub use session_log::{DEFAULT_LOG_DIR, SessionLog};
pub use toolchain::{ProfileSetting, ToolProfile, ToolchainProbe};
pub use tools::{ExecContext, RenderedTool, ToolRegistry, describe_tool_call};

/// Initializes logging for the binary.
pub fn init_logging() {
    logging::init_logging();
}
