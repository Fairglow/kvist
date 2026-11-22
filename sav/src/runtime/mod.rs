//! Host-neutral native conversation, bounded execution and correlated controls.
//!
//! Tools and durable recording are injected by the host. This module neither
//! authorizes effects nor supplies an execution fallback.

pub mod context;
pub mod contracts;
pub mod control;
mod diagnostics;
mod driver;
mod error;
pub mod execution_backend;
mod outcome;
pub mod retry;
mod session;

pub use context::{
    Compaction, ContextManager, DEFAULT_CONTEXT_TOKENS, estimate_messages, estimate_tokens,
};
pub use contracts::{
    BackendClass, CapabilityDescriptor, CapabilityState, MAX_EXTENSION_PAYLOAD_BYTES,
    ProviderExtension, RedactedEvent, redact_event,
};
pub use control::{
    ControlMailbox, Guidance, GuidanceId, GuidanceNotice, GuidanceState, Submission, SubmissionKind,
};
pub use diagnostics::{
    BudgetResource, EffectCertainty, RecoveryAction, StopReason, ToolFailure, ToolFailureCategory,
};
pub use driver::{NativeRuntime, Operation};
pub use error::{Error, Result};
pub use execution_backend::{
    BackendIdentity, Capabilities, ExecutionBackend, Namespaces, RunnerIdentity,
};
pub use outcome::ToolOutcome;
pub use retry::{
    DEFAULT_MAX_ATTEMPTS, DEFAULT_RETRY_BASE_DELAY, DEFAULT_RETRY_MAX_DELAY, RetryPolicy,
};
pub use session::validate_turn;
pub use session::{
    AgentSession, Event, EventSink, MAX_TOOL_RESULT_BYTES, MAX_TURNS, ModelIdentity, Recorder,
    RunLimits, RunSummary, TokenAccounting, ToolExecutor,
};
