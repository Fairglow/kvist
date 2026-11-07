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
/// read-only native data. Canonical checks are process-free; otherwise the
/// operator check uses acquisition-free native offline Cargo admission.
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
        let home = rust::cache::home().map_err(from_error)?;
        let sources = rust::cache::sources(project, &home, cancellation).map_err(from_error)?;
        let report = match sources.admission {
            rust::cache::SourceAdmission::Catalogue(report) => report,
            rust::cache::SourceAdmission::NoLock => {
                return Err(failure(
                    &"Cargo.lock is required for an explicit locked dependency check",
                ));
            }
            rust::cache::SourceAdmission::CargoOfflineRequired { .. } => {
                eprintln!(
                    "Explicit host Rust source admission for `{}`: metadata --offline --locked --format-version 1 --all-features. Cargo may read host configuration, update native cache administration or extract cached sources; no acquisition, compilation, lock resolution or receipt minting.",
                    crate::error::terminal_text(&project.to_string_lossy()),
                );
                let cargo = crate::toolchain::rust_environment::host_cargo(project, cancellation)?;
                rust::cache::admit(
                    project,
                    &home,
                    &rust::ProvisionOptions {
                        cargo,
                        offline: true,
                        vendored_dir: None,
                        timeout: Duration::from_secs(limits.timeout_secs),
                        force: false,
                    },
                    cancellation,
                )
                .map_err(from_error)?
                .report
            }
        };
        if !report.ready() {
            return Err(failure(&report.missing_human_readable()));
        }
        return Ok(report);
    }
    // Require a bounded lock before resolving a toolchain or creating state.
    let lock = project.join("Cargo.lock");
    let metadata = std::fs::symlink_metadata(&lock).map_err(|error| {
        failure(&format!(
            "cannot read existing Cargo.lock: {error}; provisioning currently supports Rust only"
        ))
    })?;
    if !metadata.is_file() || metadata.len() > limits.max_lockfile_bytes {
        return Err(failure(
            &"Cargo.lock must be a bounded regular file; resolve the Rust lock explicitly on the host first",
        ));
    }
    let cargo = crate::toolchain::rust_environment::host_cargo(project, cancellation)?;
    rust::cache::provision(
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
    .map(|view| view.report)
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
            "{error}. Resolve Cargo.lock explicitly on the host if required by this explicit provision/check; run `skott --vendor [--cwd DIR]` (or add --offline for cached-only acquisition). `--check` never acquires sources: canonical checks are process-free, while Git/alternate admission uses explicitly disclosed offline Cargo metadata. Admission failure does not gate live workspace resources or independent tools. Host changes refresh on subsequent invocations without restart or automatic replay. Inspect partial native acquisition before explicitly retrying; never delete operator locks or cache data as a recovery shortcut"
        ),
    }
}
