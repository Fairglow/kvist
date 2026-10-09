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
use crate::vendoring::{
    VendorManifest, enforce_offline_readiness, find_stale_vendored_packages, now_unix_secs,
    offline_cargo_config, remove_stale_vendored_packages,
};

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
    // A dependency-free lockfile needs no registry material, but the offline
    // topology still mounts the vendored registry directory; it must exist on
    // disk even when empty.
    std::fs::create_dir_all(&vendored_dir).map_err(|source| KvistError::Io {
        operation: "create vendored registry directory",
        path: vendored_dir.clone(),
        source,
    })?;

    // Populate or reconcile the vendored registry on the host using the locked
    // lockfile. `cargo vendor` reconciles an existing registry, so it is only
    // invoked when a locked dependency is absent and the registry does not yet
    // satisfy the lock; a complete registry is reused and cargo is never invoked.
    // This is the only step that may touch the network: it resolves crate sources
    // into an attempt-local, content-addressed-on-disk registry.
    let mut report = enforce_offline_readiness(project_dir, &vendored_dir)?;

    // Check for stale packages (present in vendored dir but not in Cargo.lock).
    // If any are found, we need to re-vendor to ensure the vendored directory
    // exactly matches the locked dependencies.
    let stale = find_stale_vendored_packages(project_dir, &vendored_dir)?;
    if !stale.is_empty() {
        tracing::info!(
            project = %project_dir.to_string_lossy(),
            stale_count = stale.len(),
            "found stale vendored packages, will re-vendor"
        );
    }

    if options.populate && (!report.ready() || !stale.is_empty()) {
        if let Some(sandbox) = &options.sandbox {
            let in_sandbox =
                provision_rust_via_acquisition_sandbox(project_dir, sandbox, &vendored_dir)?;
            match in_sandbox {
                ProvisionOutcome::Ready => {
                    report = enforce_offline_readiness(project_dir, &vendored_dir)?;
                }
                ProvisionOutcome::Fallback(reason) => {
                    tracing::warn!(
                        project = %project_dir.to_string_lossy(),
                        %reason,
                        "falling back to the host cargo vendor pass"
                    );
                    populate_with_cargo_vendor(project_dir, &vendored_dir)?;
                    report = enforce_offline_readiness(project_dir, &vendored_dir)?;
                }
            }
        } else {
            populate_with_cargo_vendor(project_dir, &vendored_dir)?;
            report = enforce_offline_readiness(project_dir, &vendored_dir)?;
        }

        // After populating, check for and remove any remaining stale packages.
        // This handles the case where a dependency was removed or its version
        // changed, leaving an old vendored copy behind.
        let stale_after = find_stale_vendored_packages(project_dir, &vendored_dir)?;
        if !stale_after.is_empty() {
            remove_stale_vendored_packages(&vendored_dir, &stale_after)?;
        }
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
    let sandbox_cargo_dir = ensure_sandbox_cargo_config(project_dir)?;

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
    manifest.save(project_dir)?;

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

/// The outcome of an in-sandbox acquisition provisioning pass.
///
/// `Ready` means the vendored registry now satisfies the lock and the manifest
/// can be recorded. `Fallback` means the network-allow acquisition sandbox could
/// not run here (no runner, backend, toolchain, or a failed fetch), so the
/// caller should use the classic host `cargo vendor` pass.
#[derive(Debug)]
enum ProvisionOutcome {
    /// The vendored registry was re-provisioned from the acquisition sandbox.
    Ready,
    /// The in-sandbox path could not run; use the host `cargo vendor` pass.
    Fallback(String),
}

/// Re-provision a Rust vendored registry *inside the sandbox*: run a
/// network-allow dependency-acquisition sandbox (`cargo fetch`) with allowlisted
/// package sources before the effect sandbox executes, then repack the fetched
/// material into the project's vendored registry offline.
///
/// This is the in-sandbox provisioning step ADR-0011 describes as running the
/// acquisition outside the effect sandbox. The one step that may contact the
/// network is the allowlisted `cargo fetch` inside the acquisition sandbox; the
/// repack (`cargo vendor --offline`) runs on the host with network denied, so
/// the host never resolves crate sources. When the sandbox cannot run in this
/// environment (no runner, backend, or toolchain, or the fetch fails), the
/// result is `Fallback` so the caller can use the classic host `cargo vendor`
/// pass.
fn provision_rust_via_acquisition_sandbox(
    project_dir: &Path,
    sandbox_config: &crate::config::SandboxConfig,
    vendored_dir: &Path,
) -> Result<ProvisionOutcome> {
    use crate::config::{PackageSource, VcsSelection};
    use crate::sandbox::{SandboxProbe, backend_identity, ensure_available, runner_identity};

    // The VCS selection is a property of the project; load it from the project
    // configuration. A missing/invalid configuration is not a reason to fail the
    // vendoring pass, so fall back to the host `cargo vendor` path.
    let vcs = match crate::config::load(project_dir) {
        Ok(config) => config.vcs,
        Err(_) => VcsSelection::Git,
    };

    // Resolve the pinned toolchain (immutable root + exact cargo beneath it).
    let toolchain = match crate::toolchain::resolve_pinned_toolchain(
        project_dir,
        "dependency-acquisition sandbox",
    ) {
        Ok(toolchain) => toolchain,
        Err(_) => {
            return Ok(ProvisionOutcome::Fallback(
                "no resolvable Rust toolchain for the acquisition sandbox".to_owned(),
            ));
        }
    };

    // Rehash the trusted runner and backend identities; a changed runner fails
    // closed before any network contact.
    let expected_runner = match runner_identity(sandbox_config, project_dir, vcs) {
        Ok(identity) => identity,
        Err(_) => {
            return Ok(ProvisionOutcome::Fallback(
                "the trusted sandbox runner is not available".to_owned(),
            ));
        }
    };
    let backend = match backend_identity(sandbox_config, project_dir, vcs) {
        Ok(identity) => identity,
        Err(_) => {
            return Ok(ProvisionOutcome::Fallback(
                "the sandbox enforcement backend is not available".to_owned(),
            ));
        }
    };
    // Probe the live sandbox; it must be able to run before the fetch is
    // attempted.
    let probe: SandboxProbe =
        match ensure_available(sandbox_config, project_dir, vcs, &expected_runner, &backend) {
            Ok(probe) => probe,
            Err(_) => {
                return Ok(ProvisionOutcome::Fallback(
                    "the sandbox runner is not available in this environment".to_owned(),
                ));
            }
        };

    // Provision the writable acquisition inputs under the Kvist-owned
    // `.kvist/acquisition/` directory, regenerated on every pass.
    let acquisition = project_dir.join(".kvist").join("acquisition");
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
    // The sandbox working directory is the lockfile workspace; `cargo fetch`
    // resolves the workspace from `Cargo.lock` there.
    let lockfile_src = project_dir.join(crate::vendoring::CARGO_LOCK_FILENAME);
    let lockfile_dst = lockfile_workspace.join(crate::vendoring::CARGO_LOCK_FILENAME);
    std::fs::copy(&lockfile_src, &lockfile_dst).map_err(|source| KvistError::Io {
        operation: "copy lockfile into acquisition workspace",
        path: lockfile_dst,
        source,
    })?;
    let lockfile_before_identity =
        crate::vendoring::lockfile_digest(&std::fs::read(&lockfile_src).map_err(|source| {
            KvistError::Io {
                operation: "read lockfile for acquisition identity",
                path: lockfile_src.clone(),
                source,
            }
        })?);

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
            timeout: Some(std::time::Duration::from_secs(900)),
            output_limit: None,
            live_stdout: None,
        },
    );
    let result = match result {
        Ok(result) => result,
        Err(_) => {
            return Ok(ProvisionOutcome::Fallback(
                "the dependency-acquisition sandbox could not run".to_owned(),
            ));
        }
    };
    if result.timed_out || result.output_limit_exceeded || result.cancelled {
        return Ok(ProvisionOutcome::Fallback(
            "the dependency-acquisition sandbox did not complete cleanly".to_owned(),
        ));
    }
    if !result.output.status.success() {
        let stderr = String::from_utf8_lossy(&result.output.stderr);
        let truncated: String = stderr.chars().take(MAX_VENDOR_ERROR_BYTES).collect();
        return Ok(ProvisionOutcome::Fallback(format!(
            "cargo fetch in the acquisition sandbox failed: {truncated}"
        )));
    }

    // The fetched material lives in the writable acquisition cargo home (the
    // runner also promotes it into a sibling `project-cache`); repack it into
    // the project's vendored registry offline. This is the only host step and
    // it runs with network denied.
    repack_fetched_cache(project_dir, vendored_dir, &cargo_home)
}

/// Repack a fetched Cargo home into the project's vendored registry using
/// `cargo vendor --offline`. This is a pure, network-denied host step: it
/// re-lays the exact locked material the acquisition sandbox already fetched
/// into the directory the offline build resolves from.
fn repack_fetched_cache(
    project_dir: &Path,
    vendored_dir: &Path,
    fetched_cargo_home: &Path,
) -> Result<ProvisionOutcome> {
    let output = Command::new("cargo")
        .arg("vendor")
        .arg(vendored_dir)
        .arg("--offline")
        .current_dir(project_dir)
        .env("CARGO_HOME", fetched_cargo_home)
        .env("CARGO_NET_OFFLINE", "true")
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
                    reason: format!("cannot invoke cargo to repack fetched dependencies: {source}"),
                }
            }
        })?;
    if output.status.success() {
        return Ok(ProvisionOutcome::Ready);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let truncated: String = stderr.chars().take(MAX_VENDOR_ERROR_BYTES).collect();
    // A repack failure means the fetched material did not yield a complete
    // registry; the host `cargo vendor` pass can recover it.
    Ok(ProvisionOutcome::Fallback(format!(
        "offline repack of the fetched registry did not complete: {truncated}"
    )))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendoring::{find_stale_vendored_packages, remove_stale_vendored_packages};
    use tempfile::tempdir;

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

    /// The in-sandbox acquisition path must degrade to a `Fallback` (not a hard
    /// error) when the sandbox cannot run here — e.g. no resolvable toolchain —
    /// so the caller can use the classic host `cargo vendor` pass. This is the
    /// property that keeps `kvist vendor` working in environments without a
    /// bubblewrap runner.
    #[test]
    fn acquisition_fallback_when_no_toolchain_is_resolvable() {
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
        let outcome = provision_rust_via_acquisition_sandbox(project, &sandbox, &vendored)
            .expect("provisioning must not hard-fail");
        assert!(
            matches!(outcome, ProvisionOutcome::Fallback(_)),
            "without a resolvable toolchain the in-sandbox path must fall back to \
             the host cargo vendor pass"
        );
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
        remove_stale_vendored_packages(&vendored, &stale).expect("remove stale");

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
