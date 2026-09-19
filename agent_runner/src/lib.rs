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
mod logging;
mod run;

pub mod config;
pub mod context;
pub mod error;
pub mod executor;
pub mod sandbox;
pub mod session;
pub mod session_log;
pub mod tools;
pub mod tui;

pub use cli::{Cli, parse_effort, resolve_config_path};
pub use config::{Config, DEFAULT_WRITE_ROOT, Model, ModelProvider, SandboxPaths, ToolPolicy};
pub use context::{
    Compaction, ContextManager, DEFAULT_CONTEXT_TOKENS, estimate_messages, estimate_tokens,
};
pub use error::{Error, Result};
pub use executor::SandboxExecutor;
pub use run::system_prompt;
pub use sandbox::ToolOutcome;
pub use session::{
    AgentRunner, AgentSession, Event, EventSink, MAX_TURNS, Recorder, RunSummary, ToolExecutor,
};
pub use session_log::{DEFAULT_LOG_DIR, SessionLog};
pub use tools::{ExecContext, RenderedTool, StagedWrite, ToolProfile, ToolRegistry};

/// Initializes logging for the binary.
pub fn init_logging() {
    logging::init_logging();
}
