//! Explicit operator dependency provisioning, independent of model configuration.

use std::path::Path;
use std::time::Duration;

use kvist_dependencies::{rust, vendoring::VendoringReport};
use sav::CancellationToken;

use crate::error::{Error, Result};

/// Check or explicitly provision the selected Rust workspace.
///
/// This is not an agent tool. Acquisition uses host operator authority;
/// subsequent agent execution remains network-denied and uses the shared
/// read-only vendor area. `check` does not resolve a compiler or start a process.
pub fn run(
    project: &Path,
    check: bool,
    offline: bool,
    force: bool,
    cancellation: &CancellationToken,
) -> Result<VendoringReport> {
    let project = project.canonicalize().map_err(|error| {
        crate::error::io_error(
            "resolve dependency workspace",
            Some(&project.to_string_lossy()),
            error,
        )
    })?;
    let project = project.as_path();
    let limits = kvist_dependencies::VendorLimits::load(project).map_err(from_error)?;
    if check {
        return rust::check(project, cancellation).map_err(from_error);
    }
    // Require a bounded lock before resolving a toolchain or creating state.
    let lock = project.join("Cargo.lock");
    let metadata = std::fs::symlink_metadata(&lock).map_err(|error| {
        failure(&format!(
            "cannot read existing Cargo.lock: {error}; vendoring currently supports Rust only"
        ))
    })?;
    if !metadata.is_file() || metadata.len() > limits.max_lockfile_bytes {
        return Err(failure(
            &"Cargo.lock must be a bounded regular file; resolve the Rust lock explicitly on the host first",
        ));
    }
    let cargo = crate::toolchain::rust_environment::host_cargo(project, cancellation)?;
    rust::provision(
        project,
        rust::ProvisionOptions {
            cargo,
            offline,
            vendored_dir: None,
            timeout: Duration::from_secs(limits.timeout_secs),
            force,
        },
        cancellation,
    )
    .map_err(from_error)
}

pub(crate) fn from_error(error: kvist_dependencies::Error) -> Error {
    match error {
        kvist_dependencies::Error::Cancelled => Error::Cancelled,
        error => failure(&error),
    }
}

pub(crate) fn failure(error: &impl std::fmt::Display) -> Error {
    Error::ToolPrerequisite {
        tool: "Rust offline dependencies".into(),
        reason: format!(
            "{error}. Resolve Cargo.lock explicitly on the host if absent; run `skott --vendor [--cwd DIR]` (or add --offline for an existing cache), then `skott --vendor --check`.             After dependency changes restart the session to refresh its shared vendor view; interrupted publication requires inspection, never blind replay"
        ),
    }
}
