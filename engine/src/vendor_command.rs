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
use crate::vendoring::{
    VendorManifest, enforce_offline_readiness, now_unix_secs, offline_cargo_config,
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
    if options.populate && !report.ready() {
        populate_with_cargo_vendor(project_dir, &vendored_dir)?;
        report = enforce_offline_readiness(project_dir, &vendored_dir)?;
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
/// versions into the vendored package cache. `npm ci` reconciles from
/// `package-lock.json`; yarn projects reconcile from `yarn.lock`.
fn populate_javascript(project_dir: &Path) -> Result<()> {
    let vendored = project_dir
        .join(".kvist")
        .join(crate::language_vendoring::JS_PACKAGE_CACHE_DIRNAME);
    std::fs::create_dir_all(&vendored).map_err(|source| KvistError::Io {
        operation: "create vendored javascript package cache",
        path: vendored.clone(),
        source,
    })?;
    if project_dir.join("yarn.lock").is_file() {
        run_host_tool(
            "yarn",
            &[
                "install",
                "--frozen-lockfile",
                "--cache-folder",
                vendored.to_str().ok_or_else(invalid_utf8_path_error)?,
            ],
            project_dir,
            "install JavaScript packages with yarn",
            &[],
        )
    } else {
        run_host_tool(
            "npm",
            &[
                "ci",
                "--cache",
                vendored.to_str().ok_or_else(invalid_utf8_path_error)?,
            ],
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
