//! `kvist vendor`: prepare native offline dependencies (ADR-0020).
//!
//! This is the operator-facing entry point that makes builds and tests run with
//! the sandbox network denied. It detects the project's language strategy and
//! dispatches the host-authorized provisioning pass for it:
//!
//! - **Rust** — reuses selected native registry/Git data, acquiring only missing
//!   material. Configured fetch/admission uses the closed sandbox topology;
//!   explicit portable exports remain separate.
//! - **Go** — `go mod download` populates the selected native module cache.
//! - **JavaScript** — frozen native installs populate `node_modules` and a
//!   persistent project package store.
//! - **Python** — downloads the locked wheels and builds a provisioned
//!   virtualenv that makes them importable offline.
//! - **C/C++ (Conan)** — fills a project-local Conan home with the exact locked
//!   binaries and generates the build files the project build system consumes.
//!
//! Native catalogues are presence checks, not source-integrity attestations.
//! Verification exposes data read-only with private administrative/build state
//! and denied networking. Acquisition requires explicit operator authority.

use std::path::{Path, PathBuf};

use crate::error::{KvistError, Result};
use crate::sandbox::SandboxAllowedSource;
use crate::vendoring::VendorManifest;

#[cfg(test)]
#[path = "vendor_command/protected_tests.rs"]
mod protected_tests;

/// The readiness outcome of a `kvist vendor` pass, per language.
///
/// Rust reports the exact registry/Git/path dependency counts from its manifest
/// machinery; the other languages report the lock-file catalogue plus
/// vendored-content presence. Both carry the same core identity facts.
#[derive(Debug, Clone)]
pub enum VendorReport {
    Rust(crate::vendoring::VendoringReport),
    Language(crate::language_vendoring::VendoringReport),
}

impl VendorReport {
    /// The vendored-content directory the pass provisioned or verified.
    pub fn vendored_dir(&self) -> &str {
        match self {
            Self::Rust(report) => &report.vendored_dir,
            Self::Language(report) => &report.vendored_dir,
        }
    }

    /// Whether the pass left the project ready for offline builds.
    pub fn ready(&self) -> bool {
        match self {
            Self::Rust(report) => report.ready(),
            Self::Language(report) => report.ready(),
        }
    }
}

impl std::fmt::Display for VendorReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rust(report) => write!(f, "{report}"),
            Self::Language(report) => write!(f, "{report}"),
        }
    }
}

/// Maximum bytes of a captured package-manager diagnostic retained in an error.
const MAX_VENDOR_ERROR_BYTES: usize = 4096;

/// Options controlling a vendoring pass.
#[derive(Debug, Clone, Default)]
pub struct VendorOptions {
    /// Refresh the native receipt, or rebuild an explicitly selected export.
    /// This does not guarantee repair of undetected native cache corruption.
    pub force: bool,
    /// Explicit portable Cargo directory export. `None` selects native cache reuse.
    pub vendored_dir: Option<PathBuf>,
    /// Authorize Rust acquisition of missing native material or an explicit export.
    /// Canonical complete native cache reuse does not invoke Cargo.
    pub populate: bool,
    /// Optional sandbox for bounded native fetch/admission, never a host fallback.
    /// Only explicit portable exports repack acquired material with offline Cargo.
    pub sandbox: Option<crate::config::SandboxConfig>,
}

pub(crate) fn configured_sandbox(project: &Path) -> Result<Option<crate::config::SandboxConfig>> {
    match crate::config::load(project) {
        Ok(config) => Ok(config.sandbox),
        Err(KvistError::ProjectConfigurationMissing { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Task-owned authority and controls for protected dependency provisioning.
///
/// This is deliberately separate from [`VendorOptions`]: selecting protected
/// execution cannot authorize the explicit human command's host acquisition.
pub struct ProtectedVendorOptions<'a> {
    pub sandbox: &'a crate::config::SandboxConfig,
    pub vcs_selection: crate::config::VcsSelection,
    /// The task's already approved runner, never a freshly selected identity.
    pub expected_runner: &'a crate::sandbox::RunnerIdentity,
    /// The task's authenticated approval digest, not a derived replacement.
    pub policy_identity: &'a str,
    pub cancellation: &'a sav::CancellationToken,
    /// Remaining whole-task budget, not a fresh provisioning timeout.
    pub remaining_budget: std::time::Duration,
}

/// Refuse protected model dependency requests that cannot be bound exactly.
///
/// This entry point has no exact requested-dependency identity binding and
/// therefore always returns an error, even when existing vendoring is ready.
/// Readiness is not evidence that a fresh requested dependency is present.
/// No paths are inspected, state staged, or commands executed. There is no
/// fallback to [`vendor_project`]. Read-only readiness inspection is a separate
/// operation: [`check_protected_vendor_readiness`].
pub fn vendor_project_protected(
    project_dir: &Path,
    options: ProtectedVendorOptions<'_>,
) -> Result<VendorReport> {
    if options.cancellation.is_cancelled() {
        return Err(KvistError::Sav(sav::Error::Cancelled));
    }
    let reason = if options.remaining_budget.is_zero() {
        "the remaining task wall-time budget is exhausted"
    } else {
        "the model-requested dependency identity is not bound to the existing lock/manifest; \
         protected request fulfillment is unsupported even when existing vendoring is ready"
    };
    Err(KvistError::VendoringUnavailable {
        path: project_dir.to_string_lossy().into_owned(),
        reason: format!(
            "protected dependency provisioning refused: {reason}; host fallback is disabled"
        ),
    })
}

/// Inspect ready Rust vendoring without fulfilling a model dependency request.
///
/// A successful report only describes the existing lockfile and registry. It
/// does not bind, fetch, or establish the presence of any requested dependency.
/// Other languages and Rust
/// state needing changes are refused without provisioning writes or package
/// manager execution.
pub fn check_protected_vendor_readiness(
    project_dir: &Path,
    options: ProtectedVendorOptions<'_>,
) -> Result<VendorReport> {
    let started = std::time::Instant::now();
    let refused = |reason: String| KvistError::VendoringUnavailable {
        path: project_dir.to_string_lossy().into_owned(),
        reason: format!(
            "protected dependency provisioning refused: {reason}; host fallback is disabled; \
             provision dependencies with an explicit human `kvist vendor` invocation outside \
             the protected task"
        ),
    };
    let check_control = || -> Result<()> {
        if options.cancellation.is_cancelled() {
            return Err(KvistError::Sav(sav::Error::Cancelled));
        }
        if started.elapsed() >= options.remaining_budget {
            return Err(refused(
                "the remaining task wall-time budget is exhausted".into(),
            ));
        }
        Ok(())
    };
    check_control()?;
    crate::acquisition::ContentIdentity::new(options.policy_identity)
        .map_err(|error| refused(format!("invalid approved policy identity: {error}")))?;
    let project_dir = canonical_or(project_dir)?;
    let current_runner =
        crate::sandbox::runner_identity(options.sandbox, &project_dir, options.vcs_selection)?;
    if &current_runner != options.expected_runner {
        return Err(KvistError::SandboxUnavailable {
            runner: options.sandbox.runner.clone(),
            reason: "trusted sandbox runner identity or content changed after approval".into(),
        });
    }
    check_control()?;
    let strategy = crate::language_vendoring::detect_language_strategy(&project_dir)?;
    if strategy.id() != "rust" {
        return Err(refused(format!(
            "language `{}` has no mediated provisioning implementation",
            strategy.id()
        )));
    }
    let home = roetter::rust::cache::home()?;
    let view = native_cargo_admission_controlled(
        &project_dir,
        &home,
        options.sandbox,
        options.cancellation,
        Some(options.remaining_budget.saturating_sub(started.elapsed())),
    )?;
    view.report.enforce()?;
    check_control()?;
    Ok(VendorReport::Rust(view.report))
}

/// Ensure a locked project can build offline and record what was produced.
///
/// Detects the project's language strategy, performs the host-authorized
/// provisioning pass for it, and returns the readiness report the strategy
/// enforces. Native Rust writes a small lock-bound cache receipt, not a
/// generated source-replacement configuration; other languages are enforced from their
/// lock file plus the provisioned material on disk.
pub fn vendor_project(project_dir: &Path, options: VendorOptions) -> Result<VendorReport> {
    let project_dir = canonical_or(project_dir)?;
    let strategy = crate::language_vendoring::detect_language_strategy(&project_dir)?;
    match strategy.id() {
        language if options.force && language != "rust" => Err(KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: "--force rebuild is currently supported for Rust only".into(),
        }),
        "rust" => Ok(VendorReport::Rust(vendor_rust_project(
            &project_dir,
            options,
        )?)),
        "go" => {
            populate_go_vendor(&project_dir)?;
            Ok(VendorReport::Language(enforce_language_report(
                &project_dir,
                "go",
            )?))
        }
        "javascript" => {
            populate_javascript(&project_dir)?;
            Ok(VendorReport::Language(enforce_language_report(
                &project_dir,
                "javascript",
            )?))
        }
        "python" => {
            populate_python(&project_dir)?;
            Ok(VendorReport::Language(enforce_language_report(
                &project_dir,
                "python",
            )?))
        }
        "c" => {
            populate_conan(&project_dir)?;
            Ok(VendorReport::Language(enforce_language_report(
                &project_dir,
                "c",
            )?))
        }
        "c-vcpkg" => {
            populate_vcpkg(&project_dir)?;
            Ok(VendorReport::Language(enforce_language_report(
                &project_dir,
                "c-vcpkg",
            )?))
        }
        other => Err(KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: format!("no provisioning pass for language `{other}`"),
        }),
    }
}

/// Enforce a non-Rust strategy's vendoring readiness and return its report.
fn enforce_language_report(
    project_dir: &Path,
    language: &str,
) -> Result<crate::language_vendoring::VendoringReport> {
    let strategy = crate::language_vendoring::detect_language_strategy(project_dir)?;
    if strategy.id() != language {
        return Err(KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: format!(
                "project language changed during vendoring (expected {language}, detected {})",
                strategy.id()
            ),
        });
    }
    let report = strategy.enforce(project_dir)?;
    if !report.ready() {
        return Err(KvistError::VendoringIncomplete {
            path: report.project_root.clone(),
            count: report.missing_dependencies.len(),
            missing: report.missing_human_readable(),
        });
    }
    tracing::info!(
        project = %report.project_root,
        language,
        present = report.present_dependencies,
        "vendored dependencies for offline builds"
    );
    Ok(report)
}

/// Prepare native Cargo data, or deliberately export a portable directory source.
fn vendor_rust_project(
    project_dir: &Path,
    options: VendorOptions,
) -> Result<crate::vendoring::VendoringReport> {
    let cancellation = sav::CancellationToken::new();
    let limits = roetter::VendorLimits::load(project_dir)?;
    if options.vendored_dir.is_none() {
        let home = roetter::rust::cache::home()?;
        tracing::info!(project = %project_dir.display(), cache = %home.display(), "checking native locked Cargo cache");
        let acquire = |home: &Path| {
            if !options.populate {
                return Err(roetter::Error::VendoringUnavailable {
                    path: project_dir.to_string_lossy().into_owned(),
                    reason: "locked native dependencies are missing; acquisition was not authorized; run explicit `kvist vendor`".into(),
                });
            }
            tracing::info!(project = %project_dir.display(), "acquiring missing locked Cargo dependencies; completed downloads will be retained");
            if let Some(sandbox) = &options.sandbox {
                provision_rust_via_acquisition_sandbox(project_dir, sandbox, home).map_err(
                    |error| roetter::Error::VendoringUnavailable {
                        path: project_dir.to_string_lossy().into_owned(),
                        reason: format!("{error}; configured acquisition stopped; cached downloads retained; no host fallback"),
                    },
                )
            } else {
                let cargo = crate::toolchain::resolve_pinned_toolchain(
                    project_dir,
                    "explicit native Cargo acquisition",
                )
                .map_err(|error| roetter::Error::VendoringUnavailable {
                    path: project_dir.to_string_lossy().into_owned(),
                    reason: error.to_string(),
                })?
                .cargo;
                roetter::rust::cache::populate(
                    project_dir,
                    home,
                    &roetter::rust::ProvisionOptions {
                        cargo,
                        offline: false,
                        vendored_dir: None,
                        timeout: std::time::Duration::from_secs(limits.timeout_secs),
                        force: options.force,
                    },
                    &cancellation,
                )
            }
        };
        let view = if let Some(sandbox) = &options.sandbox {
            roetter::rust::cache::provision_with_validation(
                project_dir,
                &home,
                &cancellation,
                options.force,
                acquire,
                |home| native_metadata(project_dir, home, sandbox, &cancellation, None),
            )?
        } else {
            roetter::rust::cache::provision_with(
                project_dir,
                &home,
                &cancellation,
                options.force,
                acquire,
            )?
        };
        tracing::info!(project = %project_dir.display(), "native locked Cargo cache ready; no source copying or per-file inventory");
        return Ok(view.report);
    }
    if !options.populate {
        return roetter::rust::provision_with(
            project_dir,
            options.vendored_dir.as_deref(),
            &cancellation,
            options.force,
            |_| Err(roetter::Error::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: "locked dependency acquisition is required but population was not authorized; run the explicit operator vendor command".into(),
            }),
        ).map_err(Into::into);
    }
    if let Some(sandbox) = &options.sandbox {
        return roetter::rust::provision_with(
            project_dir,
            options.vendored_dir.as_deref(),
            &cancellation,
            options.force,
            |staged| {
                let home = roetter::rust::cache::home()?;
                roetter::rust::cache::provision_with_validation(
                    project_dir,
                    &home,
                    &cancellation,
                    false,
                    |home| {
                        provision_rust_via_acquisition_sandbox(project_dir, sandbox, home).map_err(
                            |error| roetter::Error::VendoringUnavailable {
                                path: project_dir.to_string_lossy().into_owned(),
                                reason: format!(
                                    "{error}; configured acquisition stopped; no host fallback"
                                ),
                            },
                        )
                    },
                    |home| native_metadata(project_dir, home, sandbox, &cancellation, None),
                )?;
                let cargo = crate::toolchain::resolve_pinned_toolchain(
                    project_dir,
                    "explicit portable Cargo export",
                )
                .map_err(|error| roetter::Error::VendoringUnavailable {
                    path: project_dir.to_string_lossy().into_owned(),
                    reason: error.to_string(),
                })?
                .cargo;
                roetter::rust::populate(
                    project_dir,
                    staged,
                    &roetter::rust::ProvisionOptions {
                        cargo,
                        offline: true,
                        vendored_dir: None,
                        timeout: std::time::Duration::from_secs(limits.timeout_secs),
                        force: false,
                    },
                    Some(&home),
                    &cancellation,
                )
            },
        )
        .map_err(Into::into);
    }
    let cargo =
        crate::toolchain::resolve_pinned_toolchain(project_dir, "explicit Rust vendoring")?.cargo;
    roetter::rust::provision(
        project_dir,
        roetter::rust::ProvisionOptions {
            cargo,
            offline: false,
            vendored_dir: options.vendored_dir,
            timeout: std::time::Duration::from_secs(limits.timeout_secs),
            force: options.force,
        },
        &cancellation,
    )
    .map_err(Into::into)
}

/// Re-provision a Rust vendored registry *inside the sandbox*: run a
/// network-allow dependency-acquisition sandbox (`cargo fetch`) with allowlisted
/// package sources into persistent native data without repacking or host fallback.
fn provision_rust_via_acquisition_sandbox(
    project_dir: &Path,
    sandbox_config: &crate::config::SandboxConfig,
    cargo_home: &Path,
) -> Result<()> {
    let result = run_native_cargo_preparation(
        project_dir,
        sandbox_config,
        cargo_home,
        NativePreparation::Fetch,
        &sav::CancellationToken::new(),
        None,
    )?;
    let limits = roetter::VendorLimits::load(project_dir)?;
    check_acquisition_result(
        project_dir,
        &result,
        &limits,
        &sandbox_redactions(sandbox_config),
    )
}

#[derive(Clone, Copy)]
enum NativePreparation {
    Fetch,
    Metadata,
}

pub(crate) fn native_cargo_admission(
    project: &Path,
    home: &Path,
    sandbox: Option<&crate::config::SandboxConfig>,
    cancellation: &sav::CancellationToken,
) -> Result<roetter::rust::cache::CacheView> {
    if let Some(sandbox) = sandbox {
        native_cargo_admission_controlled(project, home, sandbox, cancellation, None)
    } else {
        let limits = roetter::VendorLimits::load(project)?;
        let toolchain = crate::toolchain::resolve_pinned_toolchain(
            project,
            "explicit host native Cargo admission",
        )?;
        roetter::rust::cache::admit(
            project,
            home,
            &roetter::rust::ProvisionOptions {
                cargo: toolchain.cargo,
                offline: true,
                vendored_dir: None,
                timeout: std::time::Duration::from_secs(limits.timeout_secs.min(30)),
                force: false,
            },
            cancellation,
        )
        .map_err(Into::into)
    }
}

fn native_cargo_admission_controlled(
    project: &Path,
    home: &Path,
    sandbox: &crate::config::SandboxConfig,
    cancellation: &sav::CancellationToken,
    remaining: Option<std::time::Duration>,
) -> Result<roetter::rust::cache::CacheView> {
    roetter::rust::cache::admit_with(project, home, cancellation, |home| {
        native_metadata(project, home, sandbox, cancellation, remaining)
    })
    .map_err(Into::into)
}

fn native_metadata(
    project: &Path,
    home: &Path,
    sandbox: &crate::config::SandboxConfig,
    cancellation: &sav::CancellationToken,
    remaining: Option<std::time::Duration>,
) -> roetter::Result<Vec<u8>> {
    let result = run_native_cargo_preparation(
        project,
        sandbox,
        home,
        NativePreparation::Metadata,
        cancellation,
        remaining,
    )
    .map_err(|error| roetter::Error::VendoringUnavailable {
        path: project.to_string_lossy().into_owned(),
        reason: format!("{error}; configured native admission stopped; no host fallback"),
    })?;
    if result.cancelled || cancellation.is_cancelled() {
        return Err(roetter::Error::Cancelled);
    }
    let failure = if result.timed_out {
        Some(roetter::ProcessFailure::Timeout)
    } else if result.output_limit_exceeded {
        Some(roetter::ProcessFailure::OutputLimit)
    } else if !result.output.status.success() {
        Some(roetter::ProcessFailure::Exit)
    } else {
        None
    };
    if let Some(failure) = failure {
        let error = roetter::rust::cache::classify_metadata_failure(
            project,
            failure,
            result.output.status.code(),
            &result.output.stderr,
            cancellation,
        );
        if matches!(
            error,
            roetter::Error::VendoringIncomplete { .. } | roetter::Error::Cancelled
        ) {
            return Err(error);
        }
        return Err(roetter::Error::VendoringUnavailable {
            path: project.to_string_lossy().into_owned(),
            reason: format!(
                "{error}; configured native Cargo metadata failed ({}); no host fallback\n{}",
                result.output.status,
                vendor_diagnostic(
                    &result.output.stderr,
                    &result.output.stdout,
                    &sandbox_redactions(sandbox)
                ),
            ),
        });
    }
    roetter::rust::cache::normalize_metadata_paths(
        project,
        Path::new(crate::sandbox::NATIVE_CARGO_HOME),
        &result.output.stdout,
        cancellation,
    )
}

fn sandbox_redactions(config: &crate::config::SandboxConfig) -> Vec<String> {
    config
        .environment_allowlist
        .iter()
        .filter_map(std::env::var_os)
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
        .collect()
}

fn run_native_cargo_preparation(
    project_dir: &Path,
    sandbox_config: &crate::config::SandboxConfig,
    cargo_home: &Path,
    operation: NativePreparation,
    cancellation: &sav::CancellationToken,
    remaining: Option<std::time::Duration>,
) -> Result<crate::sandbox::ExecutionResult> {
    use crate::config::{PackageSource, VcsSelection};
    use crate::sandbox::{SandboxProbe, backend_identity, ensure_available, runner_identity};
    let started = std::time::Instant::now();
    if cancellation.is_cancelled() {
        return Err(KvistError::Sav(sav::Error::Cancelled));
    }
    let limits = roetter::VendorLimits::load(project_dir)?;

    // Explicit standalone paths may not have project configuration.
    let vcs = match crate::config::load(project_dir) {
        Ok(config) => config.vcs,
        Err(KvistError::ProjectConfigurationMissing { .. }) => VcsSelection::Git,
        Err(error) => return Err(error),
    };

    // Resolve the pinned toolchain (immutable root + exact cargo beneath it).
    let toolchain =
        crate::toolchain::resolve_pinned_toolchain(project_dir, "dependency-acquisition sandbox")?;

    // Rehash the trusted runner and backend identities; a changed runner fails
    // closed before any network contact.
    let expected_runner = runner_identity(sandbox_config, project_dir, vcs)?;
    let backend = backend_identity(sandbox_config, project_dir, vcs)?;
    // Probe the live sandbox; it must be able to run before the fetch is
    // attempted.
    let probe: SandboxProbe =
        ensure_available(sandbox_config, project_dir, vcs, &expected_runner, &backend)?;

    let runtime_bin = crate::sandbox::provision_toolchain_bin(project_dir, &sandbox_config.runner)?;
    // Inputs live inside this transaction's private staging, never a reusable
    // writable workspace alias that could redirect host writes through links.
    with_acquisition_scratch(project_dir, |acquisition| {
        let scratch = acquisition.join("scratch");
        let lockfile_workspace = acquisition.join("lockfile-workspace");
        std::fs::create_dir_all(&scratch).map_err(|source| KvistError::Io {
            operation: "create acquisition scratch",
            path: scratch.clone(),
            source,
        })?;
        std::fs::create_dir_all(&lockfile_workspace).map_err(|source| KvistError::Io {
            operation: "create acquisition lockfile workspace",
            path: lockfile_workspace.clone(),
            source,
        })?;
        let lockfile_before_identity = match operation {
            NativePreparation::Fetch => crate::sandbox::stage_acquisition_workspace(
                sandbox_config,
                project_dir,
                &lockfile_workspace,
            )?,
            NativePreparation::Metadata => crate::sandbox::stage_native_admission_workspace(
                sandbox_config,
                project_dir,
                &lockfile_workspace,
            )?,
        };

        // The allowlisted package sources the sandbox may contact: canonical
        // crates.io plus any project-approved additional sources.
        let mut sources: Vec<SandboxAllowedSource> = vec![SandboxAllowedSource::CargoRegistry {
            name: crate::acquisition::CANONICAL_CRATES_IO_NAME.to_owned(),
            index_origin: crate::acquisition::CANONICAL_CRATES_IO_INDEX_ORIGIN.to_owned(),
            download_origin: crate::acquisition::CANONICAL_CRATES_IO_DOWNLOAD_ORIGIN.to_owned(),
            identity: crate::acquisition::crates_io_identity().as_str().to_owned(),
        }];
        for source in &sandbox_config.acquisition.additional_sources {
            match source {
                PackageSource::CargoRegistry {
                    name,
                    index_origin,
                    download_origin,
                } => sources.push(SandboxAllowedSource::CargoRegistry {
                    name: name.clone(),
                    index_origin: index_origin.clone(),
                    download_origin: download_origin.clone(),
                    identity: crate::acquisition::registry_identity(
                        name,
                        index_origin,
                        download_origin,
                    )
                    .as_str()
                    .to_owned(),
                }),
                PackageSource::CargoGit {
                    repository,
                    revision,
                } => sources.push(SandboxAllowedSource::CargoGit {
                    repository: repository.clone(),
                    revision: revision.clone(),
                    identity: crate::acquisition::git_identity(repository, revision)
                        .as_str()
                        .to_owned(),
                }),
            }
        }

        // A valid-format policy identity: this pass is a host-authorized,
        // non-interactive provisioning step, so the identity is derived from the
        // runner digest it ran under.
        let policy_identity = crate::vendoring::lockfile_digest(expected_runner.digest.as_bytes());

        let timeout = std::time::Duration::from_secs(limits.timeout_secs);
        let timeout = match operation {
            NativePreparation::Fetch => timeout,
            NativePreparation::Metadata => timeout
                .min(std::time::Duration::from_secs(30))
                .min(remaining.unwrap_or(std::time::Duration::from_secs(30)))
                .saturating_sub(started.elapsed()),
        };
        if timeout.is_zero() {
            return Err(KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: "native Cargo preparation exhausted the remaining time budget before execution; no admission or receipt publication occurred".into(),
            });
        }
        let options = crate::sandbox::ExecutionOptions {
            timeout: Some(timeout),
            output_limit: Some(limits.max_output_bytes),
            live_stdout: None,
        };
        let result = match operation {
            NativePreparation::Fetch => crate::sandbox::execute_dependency_acquisition(
                sandbox_config,
                &crate::sandbox::DependencyAcquisition {
                    project_root: project_dir,
                    vcs_selection: vcs,
                    toolchain_root: &toolchain.root,
                    cargo_path: &toolchain.cargo,
                    cargo_home,
                    runtime_bin: runtime_bin.path(),
                    scratch_host_dir: &scratch,
                    lockfile_workspace: &lockfile_workspace,
                    lockfile_before_identity: &lockfile_before_identity,
                    sources: &sources,
                    policy_identity: &policy_identity,
                    backend: &probe.backend,
                    config: sandbox_config,
                    expected_runner: &expected_runner,
                },
                options,
            )?,
            NativePreparation::Metadata => {
                let (registry, git) =
                    crate::sandbox::native_cargo_data(sandbox_config, cargo_home)?;
                crate::sandbox::execute_native_cargo_metadata(
                    sandbox_config,
                    &crate::sandbox::OfflineCargoVerification {
                        project_root: project_dir,
                        vcs_selection: vcs,
                        toolchain_root: &toolchain.root,
                        cargo_path: &toolchain.cargo,
                        sources: crate::sandbox::CargoSources::Native {
                            registry: registry.as_deref(),
                            git: git.as_deref(),
                        },
                        runtime_bin: runtime_bin.path(),
                        scratch_host_dir: &scratch,
                        component_dir: &lockfile_workspace,
                        lockfile_digest: lockfile_before_identity.clone(),
                        policy_identity: &policy_identity,
                        backend: &probe.backend,
                        config: sandbox_config,
                        expected_runner: &expected_runner,
                    },
                    options,
                    cancellation,
                )?
            }
        };
        if VendorManifest::current_digest(&lockfile_workspace)? != lockfile_before_identity
            || VendorManifest::current_digest(project_dir)? != lockfile_before_identity
        {
            return Err(KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: "locked catalogue changed during configured native Cargo preparation; no receipt publication occurred; reconcile Cargo.lock before a new explicit attempt; no host fallback".into(),
            });
        }

        Ok(result)
    })
}

fn check_acquisition_result(
    project: &Path,
    result: &crate::sandbox::ExecutionResult,
    limits: &roetter::VendorLimits,
    redactions: &[String],
) -> Result<()> {
    let mut causes = Vec::new();
    if result.cancelled {
        causes.push("cancelled".into());
    }
    if result.timed_out {
        causes.push(format!("timed out (limit {} seconds)", limits.timeout_secs));
    }
    if result.output_limit_exceeded {
        causes.push(format!(
            "capture exceeded its limit of {} bytes",
            limits.max_output_bytes
        ));
    }
    if causes.is_empty() {
        if result.output.status.success() {
            return Ok(());
        }
        causes.push(format!("unsuccessful {}", result.output.status));
    }
    let detail = vendor_diagnostic(&result.output.stderr, &result.output.stdout, redactions);
    Err(KvistError::VendoringUnavailable {
        path: project.to_string_lossy().into_owned(),
        reason: format!(
            "configured sandbox Cargo fetch failed: {}; inspect acquisition policy and locked inputs before a new explicit attempt; completed downloads retained; no host fallback\n{detail}",
            causes.join("; ")
        ),
    })
}

fn vendor_diagnostic(stderr: &[u8], stdout: &[u8], redactions: &[String]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let stdout = String::from_utf8_lossy(stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    let mut detail = if detail.is_empty() {
        "no stdout/stderr was captured".into()
    } else {
        crate::presentation::sanitize(&crate::presentation::redact(detail, redactions))
    };
    if detail.len() > MAX_VENDOR_ERROR_BYTES {
        let marker = "\n[truncated]";
        let mut end = MAX_VENDOR_ERROR_BYTES - marker.len();
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        detail.truncate(end);
        detail.push_str(marker);
    }
    detail
}

fn with_acquisition_scratch<T>(
    project: &Path,
    operation: impl FnOnce(&Path) -> Result<T>,
) -> Result<T> {
    let parent = Path::new("/tmp");
    let acquisition = tempfile::Builder::new()
        .prefix(".acquisition-")
        .tempdir_in(parent)
        .map_err(|source| KvistError::Io {
            operation: "create private acquisition staging",
            path: parent.to_path_buf(),
            source,
        })?;
    let result = operation(acquisition.path());
    let path = acquisition.path().to_path_buf();
    let cleanup = acquisition.close().map_err(|source| KvistError::Io {
        operation: "remove owned administrative acquisition staging",
        path,
        source,
    });
    match (result, cleanup) {
        (Ok(config), Ok(())) => Ok(config),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(KvistError::VendoringUnavailable {
            path: project.to_string_lossy().into_owned(),
            reason: format!(
                "{error}; acquisition cleanup also failed: {cleanup}; inspect before a new action"
            ),
        }),
    }
}

/// Run a host provisioning tool at the project directory with extra
/// environment variables, failing closed with a bounded, actionable error.
fn run_host_tool(
    program: &str,
    args: &[&str],
    project_dir: &Path,
    context: &str,
    env: &[(&str, &str)],
) -> Result<()> {
    run_host_tool_capture(program, args, project_dir, context, env).map(|_| ())
}

fn run_host_tool_capture(
    program: &str,
    args: &[&str],
    project_dir: &Path,
    context: &str,
    env: &[(&str, &str)],
) -> Result<Vec<u8>> {
    let limits = roetter::VendorLimits::load(project_dir)?;
    let command = if env.is_empty() {
        sav::CommandSpec::new(program, args.iter().copied())
    } else {
        let mut arguments: Vec<String> = env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        arguments.push(program.into());
        arguments.extend(args.iter().map(|arg| (*arg).to_owned()));
        sav::CommandSpec::new("/usr/bin/env", arguments)
    }
    .in_directory(project_dir);
    let policy = sav::SupervisionPolicy {
        idle_timeout: std::time::Duration::from_secs(limits.timeout_secs.min(3600)),
        attempt_timeout: Some(std::time::Duration::from_secs(limits.timeout_secs)),
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: limits.max_output_bytes,
    };
    let redactions: Vec<String> = env
        .iter()
        .map(|(_, value)| (*value).to_owned())
        .filter(|value| !value.is_empty())
        .collect();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    tracing::info!(project = %project_dir.display(), program, phase = context, "preparing native dependency artifacts");
    sav::run_supervised_observed(
        &policy,
        &command,
        &sav::CancellationToken::new(),
        &mut |stream, bytes| {
            match stream {
                sav::OutputStream::Stdout => stdout.extend_from_slice(bytes),
                sav::OutputStream::Stderr => stderr.extend_from_slice(bytes),
            }
            Ok(())
        },
    ).map(|output| output.stdout).map_err(|error| KvistError::VendoringUnavailable {
        path: project_dir.to_string_lossy().into_owned(),
        reason: format!(
            "`{program}` could not {context}: {error}; acquisition limits: {} seconds, {} output bytes; completed native downloads/artifacts retained; inspect the source/tool installation before another explicit attempt\n{}",
            limits.timeout_secs, limits.max_output_bytes,
            vendor_diagnostic(&stderr, &stdout, &redactions),
        ),
    })
}

/// Go provisioning: acquire missing native module-cache material without a
/// source vendor copy, preserving the locked manifest/checksum inputs.
fn populate_go_vendor(project_dir: &Path) -> Result<()> {
    crate::language_vendoring::go_cache::populate(project_dir)
}

/// JavaScript provisioning: build `node_modules` from the exact locked
/// versions and vend the resolved material under `.kvist/`. `npm ci`
/// reconciles from `package-lock.json` into a tarball cache; yarn projects
/// reconcile from `yarn.lock`; pnpm uses a persistent project-native store.
/// Required `node_modules` installation is separate useful work. Only install steps may touch
/// the network; they run on the host during provisioning.
fn populate_javascript(project_dir: &Path) -> Result<()> {
    let vendored = project_dir
        .join(".kvist")
        .join(crate::language_vendoring::JS_PACKAGE_CACHE_DIRNAME);
    std::fs::create_dir_all(&vendored).map_err(|source| KvistError::Io {
        operation: "create vendored javascript package cache",
        path: vendored.clone(),
        source,
    })?;
    let vendored_str = vendored.to_str().ok_or_else(invalid_utf8_path_error)?;
    if project_dir.join("pnpm-lock.yaml").is_file() {
        populate_pnpm(project_dir)
    } else if project_dir.join("yarn.lock").is_file() {
        run_host_tool(
            "yarn",
            &[
                "install",
                "--frozen-lockfile",
                "--cache-folder",
                vendored_str,
            ],
            project_dir,
            "install JavaScript packages with yarn",
            &[],
        )
    } else {
        run_host_tool(
            "npm",
            &["ci", "--cache", vendored_str],
            project_dir,
            "install JavaScript packages with npm",
            &[],
        )
    }
}

/// Python provisioning: download the locked wheels, build the provisioned
/// virtualenv, and install the locked material into it offline. Both
/// `requirements.lock.txt` (pip) and `uv.lock` (uv) are supported lock forms.
fn populate_python(project_dir: &Path) -> Result<()> {
    // `requirements.lock.txt` is preferred; a `uv.lock` project is provisioned
    // by first exporting its exact resolved graph to a pip-format file so the
    // locked catalogue can be vendored and installed identically to pip.
    let pip_lock = project_dir.join("requirements.lock.txt");
    let uv_lock = project_dir.join("uv.lock");
    let use_uv = uv_lock.is_file() && !pip_lock.is_file();
    let requirements: PathBuf = if use_uv {
        project_dir.join(".kvist").join("uv-requirements.txt")
    } else {
        pip_lock.clone()
    };
    let requirements_str = requirements.to_str().ok_or_else(invalid_utf8_path_error)?;

    let wheels = project_dir
        .join(".kvist")
        .join(crate::language_vendoring::PYTHON_WHEELS_DIRNAME);
    std::fs::create_dir_all(&wheels).map_err(|source| KvistError::Io {
        operation: "create vendored python wheels directory",
        path: wheels.clone(),
        source,
    })?;

    if use_uv {
        // `uv export --locked` prints the pip-format graph to stdout; progress
        // goes to stderr, so only stdout is persisted as the vendored catalogue.
        export_uv_lock(project_dir, &requirements)?;
    }

    // Download the locked wheels (the only network step).
    run_host_tool(
        "pip",
        &[
            "download",
            "-r",
            requirements_str,
            "-d",
            wheels.to_str().ok_or_else(invalid_utf8_path_error)?,
        ],
        project_dir,
        "download locked Python wheels",
        &[],
    )?;

    // Build the virtualenv (uv preferred for speed, stdlib venv fallback).
    let venv = project_dir.join(".kvist").join("venv");
    let venv_python = venv.join("bin").join("python");
    let uv_available = optional_host_tool("uv", &["--version"], project_dir)?;
    if !venv_python.exists() {
        if uv_available {
            run_host_tool(
                "uv",
                &["venv", venv.to_str().ok_or_else(invalid_utf8_path_error)?],
                project_dir,
                "create the provisioned Python virtualenv",
                &[],
            )?;
        } else {
            run_host_tool(
                "python3",
                &[
                    "-m",
                    "venv",
                    venv.to_str().ok_or_else(invalid_utf8_path_error)?,
                ],
                project_dir,
                "create the provisioned Python virtualenv",
                &[],
            )?;
        }
    }

    // Install the locked material into the venv fully offline.
    let find_links = wheels.to_str().ok_or_else(invalid_utf8_path_error)?;
    let venv_python_str = venv_python.to_str().ok_or_else(invalid_utf8_path_error)?;
    if uv_available {
        run_host_tool(
            "uv",
            &[
                "pip",
                "install",
                "--python",
                venv_python_str,
                "--no-index",
                "--find-links",
                find_links,
                "-r",
                requirements_str,
            ],
            project_dir,
            "install the locked Python wheels into the virtualenv",
            &[],
        )?;
    } else {
        run_host_tool(
            venv_python_str,
            &[
                "-m",
                "pip",
                "install",
                "--no-index",
                "--find-links",
                find_links,
                "-r",
                requirements_str,
            ],
            project_dir,
            "install the locked Python wheels into the virtualenv",
            &[],
        )?;
    }
    Ok(())
}

/// Derive a pip-format requirements file from a `uv.lock` so its exact resolved
/// graph (including transitive dependencies) can be vendored with `pip download`
/// and installed offline into the venv, exactly like a `requirements.lock.txt`.
///
/// `uv export --locked` prints the pip-format catalogue to stdout; progress
/// messages go to stderr. Failures retain bounded native diagnostics.
fn export_uv_lock(project_dir: &Path, exported: &Path) -> Result<()> {
    let output = run_host_tool_capture(
        "uv",
        &["export", "--locked"],
        project_dir,
        "export the locked Python graph (install uv for uv.lock projects)",
        &[],
    )?;
    use std::io::Write as _;
    let parent = exported.parent().ok_or_else(invalid_utf8_path_error)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|source| KvistError::Io {
        operation: "stage uv-exported requirements",
        path: parent.to_path_buf(),
        source,
    })?;
    staged
        .write_all(&output)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|source| KvistError::Io {
            operation: "write uv-exported requirements",
            path: exported.to_path_buf(),
            source,
        })?;
    staged.persist(exported).map_err(|error| KvistError::Io {
        operation: "publish uv-exported requirements",
        path: exported.to_path_buf(),
        source: error.error,
    })?;
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| KvistError::Io {
            operation: "sync uv-exported requirements publication",
            path: parent.to_path_buf(),
            source,
        })?;
    Ok(())
}

/// C/C++ (Conan) provisioning: fill a project-local Conan home with the exact
/// locked binaries and generate the build files under `.kvist/conan-build`.
/// The Conan home under `.kvist/` keeps the cache Kvist-owned and inspectable.
fn populate_conan(project_dir: &Path) -> Result<()> {
    let conan_home = project_dir
        .join(".kvist")
        .join(crate::language_vendoring::CONAN_HOME_DIRNAME);
    let env = [(
        "CONAN_HOME",
        conan_home.to_str().ok_or_else(invalid_utf8_path_error)?,
    )];

    // Ensure a default profile exists in the project-local home.
    let has_profile = conan_home.join("profiles").join("default").is_file();
    if !has_profile {
        run_host_tool(
            "conan",
            &["profile", "detect"],
            project_dir,
            "detect the Conan default profile",
            &env,
        )?;
    }

    let output_path = project_dir.join(".kvist").join("conan-build");
    let output_folder = output_path.to_str().ok_or_else(invalid_utf8_path_error)?;
    let mut args: Vec<&str> = vec!["install", "."];
    if project_dir.join("conanfile.lock").is_file() {
        args.push("--lockfile=conanfile.lock");
    } else {
        args.push("--lockfile-out=conanfile.lock");
    }
    args.push("--build=missing");
    // The CMake generators are not Conan 2 defaults, so the toolchain file
    // and find_package material the project build system consumes must be
    // requested explicitly.
    args.push("-g");
    args.push("CMakeToolchain");
    args.push("-g");
    args.push("CMakeDeps");
    args.push("-of");
    args.push(output_folder);
    run_host_tool(
        "conan",
        &args,
        project_dir,
        "install locked Conan packages",
        &env,
    )
}

/// C/C++ (vcpkg) provisioning: fill a project-local vcpkg root with the exact
/// locked ports and generate the CMake toolchain files the project build system
/// consumes. The vcpkg root under `.kvist/` (`VCPKG_ROOT`) keeps the install
/// tree Kvist-owned and inspectable. Vendoring the root (the vcpkg tool plus the
/// installed ports) is what makes offline verification possible; the root must
/// already be a provisioned vcpkg installation, because vcpkg performs no
/// offline tool bootstrap.
fn populate_vcpkg(project_dir: &Path) -> Result<()> {
    let vcpkg_root = project_dir
        .join(".kvist")
        .join(crate::language_vendoring::VCPKG_ROOT_DIRNAME);
    let env = [(
        "VCPKG_ROOT",
        vcpkg_root.to_str().ok_or_else(invalid_utf8_path_error)?,
    )];

    // The vcpkg triple is project-defined: the manifest may pin it via
    // `x-triplet`; otherwise the Linux default `x64-linux` is used so the
    // offline build is reproducible and pinned.
    let triple = vcpkg_triple(project_dir)?;

    let mut args: Vec<&str> = vec!["install", "."];
    if project_dir.join("vcpkg-lock.json").is_file() {
        args.push("--locked");
    } else {
        args.push("--lockfile-out=vcpkg-lock.json");
    }
    args.push("--triplet");
    args.push(triple.as_str());
    run_host_tool(
        "vcpkg",
        &args,
        project_dir,
        "install locked vcpkg ports",
        &env,
    )
}

/// Read the pinned vcpkg triple from the manifest, falling back to the Linux
/// default so the offline build is reproducible.
fn vcpkg_triple(project_dir: &Path) -> Result<String> {
    let manifest = project_dir.join("vcpkg.json");
    let text =
        std::fs::read_to_string(&manifest).map_err(|source| KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: format!(
                "cannot read the vcpkg manifest `{}`: {source}",
                manifest.display()
            ),
        })?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|source| KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: format!(
                "the vcpkg manifest `{}` is not valid JSON: {source}",
                manifest.display()
            ),
        })?;
    let triple = value
        .get("x-triplet")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "x64-linux".to_owned());
    Ok(triple)
}

fn optional_host_tool(program: &str, args: &[&str], project_dir: &Path) -> Result<bool> {
    let path = std::env::var_os("PATH").ok_or_else(|| KvistError::VendoringUnavailable {
        path: project_dir.to_string_lossy().into_owned(),
        reason: format!(
            "PATH is not set; configure host tool discovery before probing `{program}`"
        ),
    })?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(program);
        match std::fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => {
                run_host_tool(
                    program,
                    args,
                    project_dir,
                    "probe the optional accelerator",
                    &[],
                )?;
                return Ok(true);
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect optional host accelerator",
                    path: candidate,
                    source,
                });
            }
        }
    }
    tracing::info!(
        program,
        "optional accelerator absent; using the standard installer"
    );
    Ok(false)
}

/// Reuse pnpm's project-native store without copying the global cache.
fn populate_pnpm(project_dir: &Path) -> Result<()> {
    let vendored = project_dir
        .join(".kvist")
        .join(crate::language_vendoring::JS_PACKAGE_CACHE_DIRNAME);
    run_host_tool(
        "pnpm",
        &[
            "install",
            "--frozen-lockfile",
            "--store-dir",
            vendored.to_str().ok_or_else(invalid_utf8_path_error)?,
        ],
        project_dir,
        "install JavaScript packages into the native pnpm store",
        &[],
    )
}

/// A non-UTF-8 path cannot cross into a tool argv or the sandbox wire format.
fn invalid_utf8_path_error() -> KvistError {
    KvistError::VendoringUnavailable {
        path: "<vendoring>".to_owned(),
        reason: "a vendoring path is not valid UTF-8 and cannot be used by the host tool"
            .to_owned(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendoring::{find_stale_vendored_packages, remove_stale_vendored_packages};
    use tempfile::tempdir;

    #[test]
    fn successful_acquisition_scratch_does_not_reject_shared_publication() {
        for custom in [false, true] {
            let project = tempdir().unwrap();
            let destination = custom.then(|| project.path().join("custom-vendor"));
            let checksum = "a".repeat(64);
            std::fs::write(project.path().join("Cargo.lock"), format!(
            "version=4\n[[package]]\nname=\"offline-fixture\"\nversion=\"0.1.0\"\nsource=\"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum=\"{checksum}\"\n"
        )).unwrap();
            let report = roetter::rust::provision_with(
            project.path(), destination.as_deref(), &sav::CancellationToken::new(), false,
            |vendor| with_acquisition_scratch(project.path(), |scratch| {
                std::fs::write(scratch.join("observed-fetch"), b"owned cache").unwrap();
                let package = vendor.join("offline-fixture");
                std::fs::create_dir_all(&package).unwrap();
                let manifest = "[package]\nname=\"offline-fixture\"\nversion=\"0.1.0\"\n[lib]\npath=\"lib.rs\"\n";
                let library = "pub fn value()->u8 {42}\n";
                std::fs::write(package.join("Cargo.toml"), manifest).unwrap();
                std::fs::write(package.join("lib.rs"), library).unwrap();
                use sha2::{Digest, Sha256};
                std::fs::write(package.join(".cargo-checksum.json"), serde_json::to_vec(
                    &serde_json::json!({"package":checksum,"files":{
                        "Cargo.toml":hex::encode(Sha256::digest(manifest.as_bytes())),
                        "lib.rs":hex::encode(Sha256::digest(library.as_bytes())),
                    }})
                ).unwrap()).unwrap();
                Ok(roetter::vendoring::offline_cargo_config("/workspace/vendored"))
            }).map_err(|error| roetter::Error::VendoringUnavailable {
                path: project.path().to_string_lossy().into_owned(), reason:error.to_string(),
            }),
        ).expect("scratch cleanup must leave a publishable vendor-only candidate");
            assert!(report.ready());
            let vendor = destination.unwrap_or_else(|| project.path().join(".kvist/vendored"));
            roetter::rust::check_view(project.path(), &vendor, &sav::CancellationToken::new())
                .unwrap();
        }
    }

    fn locked_rust_project(tmp: &Path) {
        std::fs::write(
            tmp.join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"serde_json\"\nversion = \"1.0.151\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        )
        .expect("write lock");
    }

    fn write_vendored_package(vendored: &Path, name: &str, version: &str) {
        let pkg_dir = vendored.join(name);
        std::fs::create_dir_all(&pkg_dir).expect("create vendored pkg dir");
        let cargo_toml = format!(
            "[package]\nname = \"{}\"\nversion = \"{}\"\n\n[dependencies]\n",
            name, version
        );
        std::fs::write(pkg_dir.join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");
    }

    fn write_vendored_package_numbered(vendored: &Path, name: &str, version: &str) {
        let pkg_dir = vendored.join(format!("{}-{}", name, version));
        std::fs::create_dir_all(&pkg_dir).expect("create vendored pkg dir");
        let cargo_toml = format!(
            "[package]\nname = \"{}\"\nversion = \"{}\"\n\n[dependencies]\n",
            name, version
        );
        std::fs::write(pkg_dir.join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");
    }

    #[test]
    fn configured_acquisition_failure_never_authorizes_host_fallback() {
        let tmp = tempfile::tempdir().expect("project");
        let project = tmp.path();
        locked_rust_project(project);
        let vendored = project.join(".kvist").join("vendored");
        std::fs::create_dir_all(&vendored).expect("vendored dir");

        let sandbox = crate::config::SandboxConfig {
            runner: "/nonexistent/runner".to_owned(),
            backend: "/nonexistent/bwrap".to_owned(),
            environment_allowlist: Vec::new(),
            acquisition: crate::config::AcquisitionConfig::default(),
        };
        let error = provision_rust_via_acquisition_sandbox(project, &sandbox, &vendored)
            .expect_err("configured acquisition failure must stop, never return host fallback");
        assert!(matches!(
            error,
            KvistError::VendoringUnavailable { .. }
                | KvistError::ToolchainUnavailable { .. }
                | KvistError::SandboxUnavailable { .. }
        ));
        assert_eq!(std::fs::read_dir(&vendored).unwrap().count(), 0);
        assert!(!project.join(".kvist/vendor.json").exists());
    }

    /// Stale packages (present in vendored dir but not in Cargo.lock) should
    /// be detected and reported.
    #[test]
    fn stale_packages_are_detected() {
        let project = tempdir().expect("project");
        let vendored = project.path().join("vendored");
        std::fs::create_dir_all(&vendored).expect("vendored dir");

        // Lockfile only has serde_json 1.0.151
        locked_rust_project(project.path());

        // Vendor serde_json (expected) and a stale package
        write_vendored_package(&vendored, "serde_json", "1.0.151");
        write_vendored_package(&vendored, "stale-pkg", "2.0.0");

        let stale = find_stale_vendored_packages(project.path(), &vendored).expect("find stale");
        assert!(
            stale.contains(&"stale-pkg".to_string()),
            "stale package should be detected"
        );
        assert!(
            !stale.contains(&"serde_json".to_string()),
            "expected package should not be detected as stale"
        );
    }

    /// Stale packages with numbered layout should be detected.
    #[test]
    fn stale_numbered_packages_are_detected() {
        let project = tempdir().expect("project");
        let vendored = project.path().join("vendored");
        std::fs::create_dir_all(&vendored).expect("vendored dir");

        locked_rust_project(project.path());

        write_vendored_package(&vendored, "serde_json", "1.0.151");
        write_vendored_package_numbered(&vendored, "stale-numbered", "1.5.0");

        let stale = find_stale_vendored_packages(project.path(), &vendored).expect("find stale");
        assert!(
            stale.contains(&"stale-numbered-1.5.0".to_string()),
            "stale numbered package should be detected"
        );
    }

    /// Packages with the same name but different versions: only the locked
    /// version should be kept, others should be detected as stale.
    #[test]
    fn wrong_version_detected_as_stale() {
        let project = tempdir().expect("project");
        let vendored = project.path().join("vendored");
        std::fs::create_dir_all(&vendored).expect("vendored dir");

        locked_rust_project(project.path());

        // Correct version
        write_vendored_package(&vendored, "serde_json", "1.0.151");
        // Wrong version (same name, different version)
        write_vendored_package_numbered(&vendored, "serde_json", "1.0.140");

        let stale = find_stale_vendored_packages(project.path(), &vendored).expect("find stale");
        assert!(
            stale.contains(&"serde_json-1.0.140".to_string()),
            "wrong version should be detected as stale"
        );
        assert!(
            !stale.contains(&"serde_json".to_string()),
            "correct version should not be stale"
        );
    }

    /// Multiple versions of the same package (both in Cargo.lock) should not
    /// be detected as stale.
    #[test]
    fn multiple_locked_versions_not_stale() {
        let project = tempdir().expect("project");
        let vendored = project.path().join("vendored");
        std::fs::create_dir_all(&vendored).expect("vendored dir");

        // Lockfile has two versions of base64
        std::fs::write(
            project.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"base64\"\nversion = \"0.22.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n[[package]]\nname = \"base64\"\nversion = \"0.23.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        ).expect("write lock");

        // Vendor both versions
        write_vendored_package(&vendored, "base64", "0.23.1");
        write_vendored_package_numbered(&vendored, "base64", "0.22.1");

        let stale = find_stale_vendored_packages(project.path(), &vendored).expect("find stale");
        assert!(stale.is_empty(), "both locked versions should not be stale");
    }

    /// Removing stale packages should clean up the vendored directory.
    #[test]
    fn removing_stale_packages_works() {
        let project = tempdir().expect("project");
        let vendored = project.path().join("vendored");
        std::fs::create_dir_all(&vendored).expect("vendored dir");

        locked_rust_project(project.path());

        write_vendored_package(&vendored, "serde_json", "1.0.151");
        write_vendored_package(&vendored, "stale-pkg", "2.0.0");

        let stale = find_stale_vendored_packages(project.path(), &vendored).expect("find stale");
        remove_stale_vendored_packages(&vendored, &stale, &roetter::VendorLimits::default())
            .expect("remove stale");

        assert!(
            !vendored.join("stale-pkg").exists(),
            "stale package should be removed"
        );
        assert!(
            vendored.join("serde_json").exists(),
            "expected package should remain"
        );
    }
}
