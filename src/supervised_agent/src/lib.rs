#![forbid(unsafe_code)]

//! Linux-first prompt acquisition, shell-free command rendering, and bounded
//! host-process supervision.
//!
//! This crate does not provide a sandbox. Callers must explicitly distinguish
//! host execution from future isolated execution backends.

#[cfg(not(target_os = "linux"))]
compile_error!("supervised-agent currently supports Linux only");

mod command;
mod error;
mod profile;
mod prompt;
mod setup;
mod supervisor;

pub use command::{render_command, split_raw_command};
pub use error::{Error, Result};
pub use profile::{
    MAX_PROFILE_CONFIG_BYTES, ModelProfile, default_profile_config_path, load_profile,
    load_profiles, upsert_profile,
};
pub use prompt::{MAX_PROMPT_BYTES, resolve_prompt};
pub use setup::{collect_profile, run_setup_wizard, verify_profile};
pub use supervisor::{
    AttemptContext, CommandSpec, ExecutionReport, RetryCause, SupervisionPolicy, run_supervised,
};
