//! `kvist vendor`: provision and enforce offline vendored dependencies (ADR-0011).
//!
//! This is the operator-facing entry point that makes Rust builds and tests run
//! with the sandbox network denied. It populates or reconciles a vendored
//! registry with the host cargo, records a versioned manifest under the
//! project, and writes the host and sandbox cargo configurations so an offline
//! build can resolve everything from vendored material. The recorded manifest is
//! re-enforced by verification before an offline build is allowed.
//!
//! Kvist controls vendoring: the vendored registry is produced from the exact
//! locked versions, and every subsequent verification re-checks that the lockfile
//! still matches and that every registry dependency is present before it allows
//! an offline build.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{KvistError, Result};
use crate::vendoring::{
    VendorManifest, VendoringReport, enforce_offline_readiness, now_unix_secs, offline_cargo_config,
};

/// Maximum bytes of a failed `cargo vendor` stderr message retained in an error.
const MAX_VENDOR_ERROR_BYTES: usize = 4096;

/// Options controlling a vendoring pass.
#[derive(Debug, Clone, Default)]
pub struct VendorOptions {
    /// An explicit vendored-registry directory. Defaults to
    /// `<project>/.kvist/vendored` when `None`.
    pub vendored_dir: Option<PathBuf>,
    /// Whether to populate or reconcile a vendored registry with the host
    /// cargo. `cargo vendor` reconciles an existing registry, so cargo is
    /// invoked only when a locked dependency is absent and the registry does not
    /// yet satisfy the lock; a complete registry is reused and cargo is never
    /// invoked.
    pub populate: bool,
}

/// Ensure a locked project can build offline and record what was produced.
///
/// The pass populates the vendored registry when required, enforces readiness,
/// writes the host and sandbox cargo configurations, and persists the manifest.
/// It returns a readiness report describing the resulting offline build state.
pub fn vendor_project(project_dir: &Path, options: VendorOptions) -> Result<VendoringReport> {
    let project_dir = canonical_or(project_dir)?;
    let lockfile_path = project_dir.join(crate::vendoring::CARGO_LOCK_FILENAME);
    if !lockfile_path.is_file() {
        return Err(KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: "no Cargo.lock found; run `cargo generate-lockfile` first so vendoring \
                     matches the exact locked versions"
                .to_owned(),
        });
    }

    let vendored_dir = match options.vendored_dir {
        Some(explicit) => canonical_or(&explicit)?,
        None => project_dir
            .join(".kvist")
            .join(crate::vendoring::DEFAULT_VENDORED_DIRNAME),
    };

    // Populate or reconcile the vendored registry on the host using the locked
    // lockfile. `cargo vendor` reconciles an existing registry, so it is only
    // invoked when a locked dependency is absent and the registry does not yet
    // satisfy the lock; a complete registry is reused and cargo is never invoked.
    // This is the only step that may touch the network: it resolves crate sources
    // into an attempt-local, content-addressed-on-disk registry.
    let mut report = enforce_offline_readiness(&project_dir, &vendored_dir)?;
    if options.populate && !report.ready() {
        populate_with_cargo_vendor(&project_dir, &vendored_dir)?;
        report = enforce_offline_readiness(&project_dir, &vendored_dir)?;
    }
    report.enforce()?;

    // The sandbox cargo configuration is what verification mounts at
    // `/workspace/.cargo`; it is the authoritative offline resolver config.
    // Kvist deliberately does NOT write source-replacement keys into the project's
    // own `.cargo/config.toml`: that file is carried into the sandbox through the
    // component mount, and cargo prefers the project-local config over parent
    // directories, so a host-path config there would shadow the mounted
    // `/workspace/.cargo` config and break the offline build. Host builds resolve
    // from the network as usual; only the sandbox build is required to be offline.
    let sandbox_cargo_dir = ensure_sandbox_cargo_config(&project_dir)?;

    let manifest = VendorManifest {
        schema_version: crate::vendoring::VENDOR_SCHEMA_VERSION,
        generated_at_unix_secs: now_unix_secs(),
        project_root: project_dir.to_string_lossy().into_owned(),
        lockfile_path: crate::vendoring::CARGO_LOCK_FILENAME.to_owned(),
        lockfile_digest: crate::vendoring::lockfile_digest(
            &std::fs::read(&lockfile_path).map_err(|source| KvistError::Io {
                operation: "read Cargo.lock for vendoring manifest",
                path: lockfile_path.clone(),
                source,
            })?,
        ),
        vendored_dir: vendored_dir.to_string_lossy().into_owned(),
        sandbox_cargo_dir: sandbox_cargo_dir.to_string_lossy().into_owned(),
        sandbox_vendored_mount: crate::vendoring::VENDOR_SANDBOX_MOUNT.to_owned(),
        verified: true,
    };
    manifest.save(&project_dir)?;

    tracing::info!(
        project = %project_dir.to_string_lossy(),
        vendored_dir = %vendored_dir.to_string_lossy(),
        registry_dependencies = report.registry_dependencies,
        git_dependencies = report.git_dependencies,
        path_dependencies = report.path_dependencies,
        "vendored dependencies for offline builds"
    );
    Ok(report)
}

/// Run `cargo vendor <dir>` at the project directory, failing closed with a
/// bounded, actionable error when cargo is absent or the pass does not succeed.
fn populate_with_cargo_vendor(project_dir: &Path, vendored_dir: &Path) -> Result<()> {
    let output = Command::new("cargo")
        .arg("vendor")
        .arg(vendored_dir)
        .current_dir(project_dir)
        .output()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                KvistError::VendoringUnavailable {
                    path: project_dir.to_string_lossy().into_owned(),
                    reason: "cargo executable was not found on PATH; install a Rust \
                             toolchain before vendoring dependencies"
                        .to_owned(),
                }
            } else {
                KvistError::VendoringUnavailable {
                    path: project_dir.to_string_lossy().into_owned(),
                    reason: format!("cannot invoke cargo to vendor dependencies: {source}"),
                }
            }
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let truncated: String = stderr.chars().take(MAX_VENDOR_ERROR_BYTES).collect();
    Err(KvistError::VendoringUnavailable {
        path: project_dir.to_string_lossy().into_owned(),
        reason: format!("`cargo vendor` did not complete:\n{truncated}"),
    })
}

/// Write the sandbox cargo configuration (referencing the fixed sandbox mount at
/// `/workspace/vendored`) into a directory that verification mounts read-only at
/// `/workspace/.cargo`. Returns that directory.
///
/// This is the authoritative offline resolver config for sandbox verification. It
/// is mounted at `/workspace/.cargo`, a parent of the sandbox working directory,
/// and only takes effect because the project-local `.cargo/config.toml` carries no
/// source-replacement keys of its own (see `vendor_project`).
fn ensure_sandbox_cargo_config(project_dir: &Path) -> Result<PathBuf> {
    let sandbox_cargo_dir = project_dir
        .join(".kvist")
        .join(crate::vendoring::SANDBOX_CARGO_CONFIG_DIRNAME);
    std::fs::create_dir_all(&sandbox_cargo_dir).map_err(|source| KvistError::Io {
        operation: "create sandbox cargo config directory for vendoring",
        path: sandbox_cargo_dir.clone(),
        source,
    })?;
    let config_path = sandbox_cargo_dir.join("config.toml");
    let contents = offline_cargo_config(crate::vendoring::VENDOR_SANDBOX_MOUNT);
    // The sandbox config is regenerated on every pass and owned by Kvist, so an
    // existing copy may be replaced.
    std::fs::write(&config_path, contents.as_bytes()).map_err(|source| KvistError::Io {
        operation: "write sandbox cargo config for vendoring",
        path: config_path.clone(),
        source,
    })?;
    Ok(sandbox_cargo_dir)
}

/// Canonicalize a path when possible, falling back to the supplied path so
/// vendoring works even when the location does not exist yet.
fn canonical_or(path: &Path) -> Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(canonical) => Ok(canonical),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(source) => Err(KvistError::Io {
            operation: "canonicalize vendoring path",
            path: path.to_path_buf(),
            source,
        }),
    }
}
