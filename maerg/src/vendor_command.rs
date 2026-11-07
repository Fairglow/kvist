//! `kvist vendor`: provision and enforce offline vendored dependencies
//! (ADR-0011, ADR-0013).
//!
//! This is the operator-facing entry point that makes builds and tests run with
//! the sandbox network denied. It detects the project's language strategy and
//! dispatches the host-authorized provisioning pass for it:
//!
//! - **Rust** — populates or reconciles a vendored registry with the host cargo,
//!   records a versioned manifest under the project, and writes the sandbox
//!   cargo configuration. The recorded manifest is re-enforced by verification
//!   before an offline build is allowed.
//! - **Go** — `go mod vendor` commits the exact module material into the
//!   project's `vendor/` directory.
//! - **JavaScript** — `npm ci` (or `yarn install --frozen-lockfile`) builds
//!   `node_modules` from the exact locked versions into a vendored package
//!   cache.
//! - **Python** — downloads the locked wheels and builds a provisioned
//!   virtualenv that makes them importable offline.
//! - **C/C++ (Conan)** — fills a project-local Conan home with the exact locked
//!   binaries and generates the build files the project build system consumes.
//!
//! Kvist controls vendoring: the vendored material is produced from the exact
//! locked versions, and every subsequent verification re-checks that the lockfile
//! still matches and that the material is present before it allows an offline
//! build. Provisioning is the only step that may contact the network; it runs on
//! the host, outside the effect sandbox.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{KvistError, Result};
use crate::sandbox::SandboxAllowedSource;
use crate::vendoring::{VendorManifest, find_stale_vendored_packages};

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

/// Maximum bytes of a failed `cargo vendor` stderr message retained in an error.
const MAX_VENDOR_ERROR_BYTES: usize = 4096;

/// Options controlling a vendoring pass.
#[derive(Debug, Clone, Default)]
pub struct VendorOptions {
    /// Explicitly rebuild otherwise reusable Rust material.
    pub force: bool,
    /// An explicit vendored-registry directory. Defaults to
    /// `<project>/.kvist/vendored` when `None`.
    pub vendored_dir: Option<PathBuf>,
    /// Whether to populate or reconcile a vendored registry with the host
    /// cargo. `cargo vendor` reconciles an existing registry, so cargo is
    /// invoked only when a locked dependency is absent and the registry does not
    /// yet satisfy the lock; a complete registry is reused and cargo is never
    /// invoked.
    pub populate: bool,
    /// The project's sandbox configuration. When present, a stale registry is
    /// re-provisioned by a network-allow dependency-acquisition sandbox
    /// (`cargo fetch`) run before the effect sandbox executes; the host
    /// `cargo vendor` is then used only to repack the fetched material into the
    /// vendored registry offline (never contacting the network). When absent
    /// (no sandbox configured), the classic host `cargo vendor` pass is used.
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
/// The acquisition runner permits `cargo fetch`, but not the `cargo vendor`
/// repack needed to provision registry material. Other languages and Rust
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
    let manifest = VendorManifest::load(&project_dir)?.ok_or_else(|| {
        refused(
            "Rust has no existing vendoring manifest and the sandbox cannot perform the \
             required vendor repack"
                .into(),
        )
    })?;
    if manifest.schema_version != crate::vendoring::VENDOR_SCHEMA_VERSION {
        return Err(refused(
            "the existing Rust vendoring receipt has an unsupported version".into(),
        ));
    }
    let current = VendorManifest::current_digest(&project_dir)?;
    if current != manifest.lockfile_digest {
        return Err(KvistError::VendoringStale {
            path: project_dir.to_string_lossy().into_owned(),
            current,
            expected: manifest.lockfile_digest,
        });
    }
    let report = kvist_dependencies::rust::check(&project_dir, options.cancellation)?;
    let vendored_dir = manifest.vendored_registry(&project_dir);
    if !vendored_dir.is_dir()
        || !find_stale_vendored_packages(&project_dir, &vendored_dir)?.is_empty()
    {
        return Err(refused(
            "the Rust registry requires a vendor repack that the sandbox cannot perform".into(),
        ));
    }
    check_control()?;
    Ok(VendorReport::Rust(report))
}

/// Ensure a locked project can build offline and record what was produced.
///
/// Detects the project's language strategy, performs the host-authorized
/// provisioning pass for it, and returns the readiness report the strategy
/// enforces. Rust additionally writes the sandbox cargo configuration and the
/// versioned manifest; the other languages are enforced statelessly from their
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

/// The Rust provisioning pass: populate or reconcile the vendored registry,
/// write the sandbox cargo configuration, and persist the versioned manifest.
fn vendor_rust_project(
    project_dir: &Path,
    options: VendorOptions,
) -> Result<crate::vendoring::VendoringReport> {
    let cancellation = sav::CancellationToken::new();
    let limits = kvist_dependencies::VendorLimits::load(project_dir)?;
    if !options.populate {
        return kvist_dependencies::rust::provision_with(
            project_dir,
            options.vendored_dir.as_deref(),
            &cancellation,
            options.force,
            |_| Err(kvist_dependencies::Error::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: "locked dependency acquisition is required but population was not authorized; run the explicit operator vendor command".into(),
            }),
        ).map_err(Into::into);
    }
    if let Some(sandbox) = &options.sandbox {
        return kvist_dependencies::rust::provision_with(
            project_dir,
            options.vendored_dir.as_deref(),
            &cancellation,
            options.force,
            |staged| {
                provision_rust_via_acquisition_sandbox(project_dir, sandbox, staged).map_err(
                    |error| kvist_dependencies::Error::VendoringUnavailable {
                        path: project_dir.to_string_lossy().into_owned(),
                        reason: format!(
                            "{error}; configured acquisition stopped; no host fallback"
                        ),
                    },
                )
            },
        )
        .map_err(Into::into);
    }
    let cargo =
        crate::toolchain::resolve_pinned_toolchain(project_dir, "explicit Rust vendoring")?.cargo;
    kvist_dependencies::rust::provision(
        project_dir,
        kvist_dependencies::rust::ProvisionOptions {
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
/// package sources before the effect sandbox executes, then repack the fetched
/// material into the project's vendored registry offline.
///
/// This is the in-sandbox provisioning step ADR-0011 describes as running the
/// acquisition outside the effect sandbox. The one step that may contact the
/// network is the allowlisted `cargo fetch` inside the acquisition sandbox; the
/// repack (`cargo vendor --offline --locked`) uses only the fetched cache.
/// This is Cargo-level offline mode, not OS confinement of the host process.
/// Configured acquisition failure stops explicitly without host fallback.
fn provision_rust_via_acquisition_sandbox(
    project_dir: &Path,
    sandbox_config: &crate::config::SandboxConfig,
    vendored_dir: &Path,
) -> Result<String> {
    use crate::config::{PackageSource, VcsSelection};
    use crate::sandbox::{SandboxProbe, backend_identity, ensure_available, runner_identity};
    let limits = kvist_dependencies::VendorLimits::load(project_dir)?;

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
    with_acquisition_scratch(project_dir, vendored_dir, |acquisition| {
        let cargo_home = acquisition.join("cargo-home");
        let scratch = acquisition.join("scratch");
        let lockfile_workspace = acquisition.join("lockfile-workspace");
        std::fs::create_dir_all(&cargo_home).map_err(|source| KvistError::Io {
            operation: "create writable acquisition cargo home",
            path: cargo_home.clone(),
            source,
        })?;
        for child in ["registry/cache", "git/db"] {
            std::fs::create_dir_all(cargo_home.join(child)).map_err(|source| KvistError::Io {
                operation: "create acquisition cargo home layout",
                path: cargo_home.clone(),
                source,
            })?;
        }
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
        let lockfile_before_identity = crate::sandbox::stage_acquisition_workspace(
            sandbox_config,
            project_dir,
            &lockfile_workspace,
        )?;

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

        // Run the network-allow acquisition sandbox: `cargo fetch` with the
        // allowlisted package sources.
        let result = crate::sandbox::execute_dependency_acquisition(
            sandbox_config,
            &crate::sandbox::DependencyAcquisition {
                project_root: project_dir,
                vcs_selection: vcs,
                toolchain_root: &toolchain.root,
                cargo_path: &toolchain.cargo,
                cargo_home: &cargo_home,
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
            crate::sandbox::ExecutionOptions {
                // A fetch may download a large dependency set; give it ample time.
                timeout: Some(std::time::Duration::from_secs(limits.timeout_secs)),
                output_limit: Some(limits.max_output_bytes),
                live_stdout: None,
            },
        )?;
        if result.timed_out || result.output_limit_exceeded || result.cancelled {
            return Err(KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: "configured dependency acquisition exceeded its output/time allowance or was cancelled; inspect before a new explicit attempt; no host fallback".into(),
        });
        }
        if !result.output.status.success() {
            return Err(KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: "configured sandbox Cargo fetch failed; inspect acquisition policy and locked inputs; no host fallback (package-manager output withheld)".into(),
        });
        }
        if VendorManifest::current_digest(&lockfile_workspace)? != lockfile_before_identity
            || VendorManifest::current_digest(project_dir)? != lockfile_before_identity
        {
            return Err(KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: "configured Cargo fetch changed the locked catalogue; no repack or publication occurred; reconcile Cargo.lock before a new explicit attempt; no host fallback".into(),
            });
        }

        // The fetched material lives in the writable acquisition cargo home (the
        // runner also promotes it into a sibling `project-cache`); repack it into
        // the project's vendored registry offline. This is the only host step and
        // enforces Cargo-level offline mode, not host OS network confinement.
        let options = kvist_dependencies::rust::ProvisionOptions {
            cargo: toolchain.cargo,
            offline: true,
            vendored_dir: None,
            timeout: std::time::Duration::from_secs(limits.timeout_secs),
            force: false,
        };
        kvist_dependencies::rust::populate(
            project_dir,
            vendored_dir,
            &options,
            Some(&cargo_home),
            &sav::CancellationToken::new(),
        )
        .map_err(Into::into)
    })
}

fn with_acquisition_scratch(
    project: &Path,
    vendored: &Path,
    operation: impl FnOnce(&Path) -> Result<String>,
) -> Result<String> {
    let parent = vendored
        .parent()
        .ok_or_else(|| KvistError::VendoringUnavailable {
            path: project.to_string_lossy().into_owned(),
            reason: "acquisition staging has no private parent".into(),
        })?;
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
        operation: "remove owned acquisition staging before publication",
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
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(project_dir)
        .envs(env.iter().copied());
    let output = command.output().map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!(
                    "the `{program}` executable was not found on PATH; install the \
                     toolchain before vendoring dependencies"
                ),
            }
        } else {
            KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!("cannot invoke {program} to {context}: {source}"),
            }
        }
    })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    let truncated: String = detail.chars().take(MAX_VENDOR_ERROR_BYTES).collect();
    Err(KvistError::VendoringUnavailable {
        path: project_dir.to_string_lossy().into_owned(),
        reason: format!(
            "`{program} {}` did not complete:\n{truncated}",
            args.join(" ")
        ),
    })
}

/// Go provisioning: `go mod vendor` commits the exact locked module material
/// into the project's `vendor/` directory (the only step that may fetch).
fn populate_go_vendor(project_dir: &Path) -> Result<()> {
    run_host_tool(
        "go",
        &["mod", "vendor"],
        project_dir,
        "vendor Go modules",
        &[],
    )
}

/// JavaScript provisioning: build `node_modules` from the exact locked
/// versions and vend the resolved material under `.kvist/`. `npm ci`
/// reconciles from `package-lock.json` into a tarball cache; yarn projects
/// reconcile from `yarn.lock`; pnpm projects are vendored as the resolved
/// `node_modules` tree (see `populate_pnpm`). Only the install steps may touch
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
    if !venv_python.exists() {
        let uv = run_host_tool_quiet("uv", &["--version"], project_dir);
        if uv {
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
    let installed = run_host_tool_quiet(
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
    );
    if !installed {
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
/// messages go to stderr and are discarded. Fails closed with an actionable
/// message when uv is absent or the export does not complete.
fn export_uv_lock(project_dir: &Path, exported: &Path) -> Result<()> {
    let output = Command::new("uv")
        .args(["export", "--locked"])
        .current_dir(project_dir)
        .output()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                KvistError::VendoringUnavailable {
                    path: project_dir.to_string_lossy().into_owned(),
                    reason: "uv executable was not found on PATH; install uv so `uv.lock`
                             projects can be vendored"
                        .to_owned(),
                }
            } else {
                KvistError::VendoringUnavailable {
                    path: project_dir.to_string_lossy().into_owned(),
                    reason: "cannot invoke uv to export the locked Python graph".to_owned(),
                }
            }
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let truncated: String = stderr.chars().take(MAX_VENDOR_ERROR_BYTES).collect();
        return Err(KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: format!("`uv export --locked` did not complete:\n{truncated}"),
        });
    }
    std::fs::write(exported, &output.stdout).map_err(|source| KvistError::Io {
        operation: "write uv-exported requirements",
        path: exported.to_path_buf(),
        source,
    })
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

/// Probe a host tool without treating a failure as an error (for optional
/// accelerators such as `uv`).
fn run_host_tool_quiet(program: &str, args: &[&str], project_dir: &Path) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// pnpm provisioning: build `node_modules` from the locked graph and vend its
/// resolved material. pnpm resolves dependencies into a content-addressable
/// store (its canonical offline catalogue), so that store is copied into
/// `.kvist/`, bound to the lock-file digest, and mounted read-only in the
/// sandbox. Only `pnpm install` touches the network, and only on the host.
fn populate_pnpm(project_dir: &Path) -> Result<()> {
    run_host_tool(
        "pnpm",
        &["install"],
        project_dir,
        "install JavaScript packages with pnpm",
        &[],
    )?;
    let vendored = project_dir
        .join(".kvist")
        .join(crate::language_vendoring::JS_PACKAGE_CACHE_DIRNAME);
    let store = pnpm_store_path(project_dir)?;
    copy_tree(&store, &vendored)
}

/// Resolve pnpm's content-addressable store path via `pnpm store path` and
/// confirm it exists and is therefore populated by the install above.
fn pnpm_store_path(project_dir: &Path) -> Result<PathBuf> {
    let output = run_capture_host_tool(
        "pnpm",
        &["store", "path"],
        project_dir,
        "locate the pnpm store",
    )?;
    let text = String::from_utf8_lossy(&output.stdout);
    let store = text
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .ok_or_else(|| KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: "pnpm did not report a store path; check the pnpm installation".to_owned(),
        })?;
    let store_path = PathBuf::from(store);
    if !store_path.is_dir() {
        return Err(KvistError::VendoringUnavailable {
            path: project_dir.to_string_lossy().into_owned(),
            reason: format!(
                "the pnpm store at `{store}` is not present after install; run \
                 `pnpm install` on the host"
            ),
        });
    }
    Ok(store_path)
}

/// Recursively copy `src` into `dst`. The pnpm store is a content-addressable
/// tree of regular files, so no symlink handling is needed; `is_dir()` and
/// `copy` follow symlinks defensively if the layout ever changes. Host-side
/// provisioning only; the copy becomes the lock-file-digest-identified vendored
/// catalogue.
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|source| KvistError::Io {
        operation: "create vendored pnpm store",
        path: dst.to_path_buf(),
        source,
    })?;
    for entry in std::fs::read_dir(src).map_err(|source| KvistError::Io {
        operation: "read vendored pnpm store",
        path: src.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| KvistError::Io {
            operation: "list vendored pnpm store entry",
            path: src.to_path_buf(),
            source,
        })?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_tree(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|source| KvistError::Io {
                operation: "copy vendored pnpm store entry",
                path: src_path.clone(),
                source,
            })?;
        }
    }
    Ok(())
}

/// Run a host tool and capture its output for downstream parsing (e.g.
/// `pnpm store path`). Fails closed with an actionable message when the tool is
/// absent or does not complete.
fn run_capture_host_tool(
    program: &str,
    args: &[&str],
    project_dir: &Path,
    context: &str,
) -> Result<std::process::Output> {
    let output = Command::new(program)
        .args(args)
        .current_dir(project_dir)
        .output()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                KvistError::VendoringUnavailable {
                    path: project_dir.to_string_lossy().into_owned(),
                    reason: format!(
                        "the `{program}` executable was not found on PATH; install
                         {program} so it can {context}"
                    ),
                }
            } else {
                KvistError::VendoringUnavailable {
                    path: project_dir.to_string_lossy().into_owned(),
                    reason: format!("cannot invoke {program} to {context}: {source}"),
                }
            }
        })?;
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    let truncated: String = detail.chars().take(MAX_VENDOR_ERROR_BYTES).collect();
    Err(KvistError::VendoringUnavailable {
        path: project_dir.to_string_lossy().into_owned(),
        reason: format!(
            "`{program} {}` did not complete:\n{truncated}",
            args.join(" ")
        ),
    })
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
            let report = kvist_dependencies::rust::provision_with(
            project.path(), destination.as_deref(), &sav::CancellationToken::new(), false,
            |vendor| with_acquisition_scratch(project.path(), vendor, |scratch| {
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
                Ok(kvist_dependencies::vendoring::offline_cargo_config("/workspace/vendored"))
            }).map_err(|error| kvist_dependencies::Error::VendoringUnavailable {
                path: project.path().to_string_lossy().into_owned(), reason:error.to_string(),
            }),
        ).expect("scratch cleanup must leave a publishable vendor-only candidate");
            assert!(report.ready());
            let vendor = destination.unwrap_or_else(|| project.path().join(".kvist/vendored"));
            kvist_dependencies::rust::check_view(
                project.path(),
                &vendor,
                &sav::CancellationToken::new(),
            )
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
        remove_stale_vendored_packages(
            &vendored,
            &stale,
            &kvist_dependencies::VendorLimits::default(),
        )
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
