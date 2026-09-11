#![forbid(unsafe_code)]

//! Linux-first prompt acquisition, shell-free command rendering, and bounded
//! host-process supervision.
//!
//! This crate does not provide a sandbox. Callers must explicitly distinguish
//! host execution from future isolated execution backends.

#[cfg(not(target_os = "linux"))]
compile_error!("agent-runtime currently supports Linux only");

mod catalog;
mod command;
mod direct_transport;
mod error;
pub mod gbnf;
pub mod loop_detection;
mod model;
mod profile;
mod prompt;
#[cfg(feature = "rig-transport")]
mod rig_transport;
mod setup;
mod supervisor;
pub mod trajectory;

pub use catalog::{
    CatalogProvider, ModelCatalog, ModelDiscoveryOptions, ProviderModel, discover_models,
};
pub use command::{render_command, render_command_with_reasoning_effort, split_raw_command};
pub use direct_transport::DirectModelTransport;
pub use error::{Error, Result};
pub use gbnf::{compile_json_schema, compile_tools_schema};
pub use model::{
    CancellationToken, FinishReason, LocalModelProvider, ModelMessage, ModelRequest,
    ModelStreamEvent, ModelTransport, ModelTurn, ModelUsage, ReasoningEffort, ToolChoice,
    ToolDefinition, ToolIntent,
};
pub use profile::{
    MAX_PROFILE_CONFIG_BYTES, ModelProfile, default_profile_config_path, load_profile,
    load_profiles, upsert_profile,
};
pub use prompt::{MAX_PROMPT_BYTES, resolve_prompt};
#[cfg(feature = "rig-transport")]
pub use rig_transport::RigModelTransport;
pub use setup::{
    SetupOptions, collect_profile, collect_profile_with_options, run_setup_wizard,
    run_setup_wizard_with_options, verify_profile,
};
pub use supervisor::{
    ACTION_HASH_RING_CAPACITY, ActionHashRing, ActionRecord, AttemptContext,
    CONSECUTIVE_IDENTICAL_ACTION_LIMIT, CapturedExecutionReport, CommandSpec,
    DEFAULT_TEMPERATURE_JITTER, ExecutionReport, LoopDecision, MAX_STALLED_TURNS,
    OBSERVATION_INVARIANT_LIMIT, REASONING_NGRAM_SIZE, REASONING_SIMILARITY_THRESHOLD, RetryCause,
    SupervisionPolicy, WINDOW_IDENTICAL_ACTION_LIMIT, compute_action_hash,
    compute_ngram_jaccard_similarity, compute_observation_hash, normalize_json, run_supervised,
    run_supervised_capture,
};
pub use trajectory::{
    TrajectoryEvent, TrajectoryRecorder, TrajectoryReplayReport, replay_trajectory,
};
