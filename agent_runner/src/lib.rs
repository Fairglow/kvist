//! The agent-runner library.
//!
//! Agent-runner is a first-class, sandbox-integrated interactive agent shell.
//! It shares Kvist's bounded `agent-runtime` mechanisms and its independently
//! installed sandbox enforcement boundary. The library exposes configuration,
//! the tool model, the sandbox request/execution layer, the transport-agnostic
//! agent loop, rolling context management, and the durable session log; the
//! binary wires these to a terminal UI.

#![forbid(unsafe_code)]

mod cli;
mod kvist_import;
mod logging;
mod markdown;
mod run;

pub mod config;
pub mod context;
pub mod error;
pub mod executor;
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
pub use kvist_import::{KVIST_CONFIG_FILE, import_models, resolve_kvist_config_path};
pub use markdown::{MarkdownStyles, RenderedLine, render_document};
pub use retry::{
    DEFAULT_MAX_ATTEMPTS, DEFAULT_RETRY_BASE_DELAY, DEFAULT_RETRY_MAX_DELAY, RetryPolicy,
};
pub use run::system_prompt;
pub use sandbox::ToolOutcome;
pub use session::{
    AgentRunner, AgentSession, Event, EventSink, MAX_TURNS, Recorder, RunSummary, ToolExecutor,
};
pub use session_log::{DEFAULT_LOG_DIR, SessionLog};
pub use toolchain::{ProfileSetting, ToolProfile, ToolchainProbe};
pub use tools::{ExecContext, RenderedTool, StagedWrite, ToolRegistry, describe_tool_call};

/// Initializes logging for the binary.
pub fn init_logging() {
    logging::init_logging();
}
