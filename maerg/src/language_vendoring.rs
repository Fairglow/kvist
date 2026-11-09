//! Language-aware vendoring enforcement (ADR-0011).
//!
//! Kvist owns vendoring for every supported language. Each strategy treats its
//! lock file as the *authoritative catalogue* of the exact locked content:
//!
//! - Host provisioning uses native Cargo/npm/Conan caches, Python distribution
//!   archives and installed environments, or a language's native vendor output.
//! - Default Rust inspection uses the shared native Cargo cache catalogue.
//!   Explicit directory exports remain a separate interface.
//!
//! The lock-file digest is the identity of every read-only vendored-directory
//! mount. This is deliberate: the lock file already catalogs every locked entry,
//! so hashing every vendored file would cost hundreds of thousands of extra hash
//! operations for no additional guarantee. Read-only grant identities are a
//! build-time claim bound to the approved mount plan and are not re-verified at
//! execution, so the lock-file digest is the catalogue and is sufficient.
//!
//! Native Rust catalogue inspection is not integrity attestation; protected
//! readiness is a separate caller obligation. Other languages check coarse
//! material presence, without a persisted lock-match or per-package guarantee.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{KvistError, Result};
use crate::vendoring as rust_vendoring;

mod config;
#[path = "vendor_command/go_cache.rs"]
pub(crate) mod go_cache;

/// Identity prefix, matching the digest shape used everywhere else in Kvist.
pub const DIGEST_PREFIX: &str = "sha256:";
/// Vendored package-cache directory name for JavaScript projects (`.kvist/`).
pub const JS_PACKAGE_CACHE_DIRNAME: &str = "vendored-js";
/// Vendored wheels directory name for Python projects (`.kvist/`).
pub const PYTHON_WHEELS_DIRNAME: &str = "vendored-python";
/// Project-local Conan home directory name for C/C++ projects (`.kvist/`).
pub const CONAN_HOME_DIRNAME: &str = "vendored-conan";
/// Project-local vcpkg root directory name for C/C++ projects (`.kvist/`).
pub const VCPKG_ROOT_DIRNAME: &str = "vendored-vcpkg";

/// SHA-256 of the lock file, prefixed. The lock file is the authoritative
/// catalogue of the locked content, so this is both the manifest identity and
/// the read-only mount identity.
pub fn lockfile_digest(contents: &[u8]) -> String {
    format!("{DIGEST_PREFIX}{}", hex::encode(Sha256::digest(contents)))
}

/// A read-only host→sandbox directory mount produced by vendoring. Every mount's
/// identity is the lock-file digest, not a per-file hash of the directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendoredMount {
    /// Absolute host directory that is mounted read-only.
    pub source: PathBuf,
    /// Fixed sandbox destination of the read-only mount.
    pub destination: String,
    /// Lock-file digest identity for the mount (a build-time claim).
    pub identity: String,
}

/// The outcome of enforcing that a locked project can build offline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendoringReport {
    /// Supported language the report is about.
    pub language: String,
    /// Project directory the report is about.
    pub project_root: String,
    /// Lock file path that was checked.
    pub lockfile_path: String,
    /// Digest of the lock file that was checked.
    pub lockfile_digest: String,
    /// Vendored-content directory that was inspected.
    pub vendored_dir: String,
    /// Observed dependency count; non-Rust strategies report approximate counts.
    pub present_dependencies: usize,
    /// Locked dependencies not found in the vendored material.
    pub missing_dependencies: Vec<String>,
}

impl VendoringReport {
    /// Whether inspection reported no missing material. This does not attest
    /// package integrity or exact non-Rust completeness.
    pub fn ready(&self) -> bool {
        self.missing_dependencies.is_empty()
    }

    /// Human-readable, non-secret explanation of the missing material.
    pub fn missing_human_readable(&self) -> String {
        self.missing_dependencies.join(", ")
    }
}

impl std::fmt::Display for VendoringReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ready() {
            write!(
                f,
                "{} vendored and ready for offline builds ({} locked entr{} present) at {}",
                self.language,
                self.present_dependencies,
                if self.present_dependencies == 1 {
                    "y"
                } else {
                    "ies"
                },
                self.vendored_dir
            )
        } else {
            write!(
                f,
                "{} not ready for offline builds: {}",
                self.language,
                self.missing_human_readable()
            )
        }
    }
}

/// A language's offline-vendoring model.
pub trait LanguageStrategy {
    /// Stable language identifier.
    fn id(&self) -> &'static str;
    /// Lock file names that catalogue the locked content, in priority order.
    fn lockfile_names(&self) -> &'static [&'static str];
    /// Vendored-content directory name under `<project>/.kvist/`.
    fn vendored_dirname(&self) -> &'static str;
    /// Fixed sandbox destination of the vendored-content directory.
    fn vendored_mount_dest(&self) -> &'static str;
    /// Fixed sandbox destination of the offline-config directory.
    fn sandbox_config_dest(&self) -> &'static str;
    /// The on-disk export/artifact directory. Defaults to the Kvist-owned
    /// `.kvist/<vendored_dirname>`; Go uses its conventional project `vendor/`
    /// export location. Default Rust/Go runtime storage instead comes from
    /// their reports and mount plans.
    fn vendored_dir(&self, project_dir: &Path) -> PathBuf {
        project_dir.join(".kvist").join(self.vendored_dirname())
    }
    /// The lock file that selects this strategy (first name present).
    fn lockfile_path(&self, project_dir: &Path) -> Option<PathBuf> {
        self.lockfile_names()
            .iter()
            .map(|name| project_dir.join(name))
            .find(|path| path.is_file())
    }
    /// Whether this strategy detects a locked project.
    fn detects(&self, project_dir: &Path) -> bool {
        self.lockfile_path(project_dir).is_some()
    }
    /// Inspect native/artifact readiness. Missing material may be reported in
    /// the result; unreadable or unsupported inspection inputs return errors.
    fn enforce(&self, project_dir: &Path) -> Result<VendoringReport>;
    /// Inspect with the caller's selected sandbox context. Native Rust source
    /// admission uses this context rather than falling back to host execution.
    fn enforce_with_sandbox(
        &self,
        project_dir: &Path,
        _sandbox: Option<&crate::config::SandboxConfig>,
    ) -> Result<VendoringReport> {
        self.enforce(project_dir)
    }
    /// The read-only directory mounts an offline build needs, each identified by
    /// the lock-file digest.
    fn mounts(&self, project_dir: &Path, lockfile_digest: &str) -> Result<Vec<VendoredMount>>;
}

/// Detects the language strategy that owns a project (Rust first, so a mixed
/// Rust+JS project such as Kvist itself verifies against its Rust lock file).
pub fn detect_language_strategy(project_dir: &Path) -> Result<Box<dyn LanguageStrategy>> {
    for strategy in language_strategies() {
        if strategy.detects(project_dir) {
            return Ok(strategy);
        }
    }
    let path = project_dir.to_string_lossy().into_owned();
    Err(KvistError::VendoringUnavailable {
        path: path.clone(),
        reason: "no supported lock file (Cargo.lock, go.sum, requirements.lock.txt, \
                 uv.lock, package-lock.json, yarn.lock, pnpm-lock.yaml, a conanfile, \
                 or vcpkg.json) found; run the language's acquisition command so \
                 vendoring matches the exact locked versions"
            .to_owned(),
    })
}

/// One supported language strategy, in detection priority order.
pub fn language_strategies() -> [Box<dyn LanguageStrategy>; 6] {
    [
        Box::new(RustStrategy),
        Box::new(GoStrategy),
        Box::new(PythonStrategy),
        Box::new(JavaScriptStrategy),
        // Conan is listed before vcpkg so a project with both a conanfile and a
        // vcpkg.json keeps the established Conan detection; a vcpkg.json-only
        // project falls through to the vcpkg strategy.
        Box::new(CConanStrategy),
        Box::new(CvcpkgStrategy),
    ]
}

/// The enforceable, offline-build-ready state of a project's vendoring.
pub struct VendoringEnforcement {
    /// Supported language that was enforced.
    pub language: String,
    /// Lock-file digest identity of every vendored mount.
    pub lockfile_digest: String,
    /// Vendored-content directory on the host.
    pub vendored_dir: PathBuf,
    /// Read-only directory mounts an offline build needs.
    pub mounts: Vec<VendoredMount>,
}

/// Enforce that a project's vendored material satisfies its lock file and return
/// what verification must mount for an offline build. If `auto_vendor` is true and
/// vendoring is not ready, automatically runs the vendor command to fix it.
/// That flag is explicit operator acquisition authority, including host non-Rust
/// installation; sandbox selection does not mediate those installers.
/// Offline verification must pass `false`.
pub fn enforce_vendoring(
    project_dir: &Path,
    auto_vendor: bool,
    sandbox_config: Option<&crate::config::SandboxConfig>,
) -> Result<VendoringEnforcement> {
    let strategy = detect_language_strategy(project_dir)?;
    let mut report = strategy.enforce_with_sandbox(project_dir, sandbox_config)?;
    if !report.ready() {
        if auto_vendor {
            tracing::warn!(
                project = %project_dir.to_string_lossy(),
                missing = report.missing_human_readable(),
                "vendoring is out of date, running `kvist vendor` to fix"
            );
            // Run the vendor command to fix vendoring
            crate::vendor_command::vendor_project(
                project_dir,
                crate::vendor_command::VendorOptions {
                    vendored_dir: None,
                    populate: true,
                    force: false,
                    sandbox: sandbox_config.cloned(),
                },
            )
            .map_err(|source| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!("automatic vendoring failed: {source}"),
            })?;
            // Re-check after vendoring
            report = strategy.enforce_with_sandbox(project_dir, sandbox_config)?;
            if !report.ready() {
                return Err(KvistError::VendoringIncomplete {
                    path: report.project_root.clone(),
                    count: report.missing_dependencies.len(),
                    missing: report.missing_human_readable(),
                });
            }
        } else {
            return Err(KvistError::VendoringIncomplete {
                path: report.project_root.clone(),
                count: report.missing_dependencies.len(),
                missing: report.missing_human_readable(),
            });
        }
    }
    let mounts = strategy.mounts(project_dir, &report.lockfile_digest)?;
    Ok(VendoringEnforcement {
        language: report.language,
        lockfile_digest: report.lockfile_digest,
        vendored_dir: PathBuf::from(report.vendored_dir),
        mounts,
    })
}

/// Whether a directory exists and holds at least one entry.
fn directory_non_empty(dir: &Path) -> bool {
    match fs::read_dir(dir) {
        Ok(mut entries) => entries.any(|entry| entry.is_ok()),
        Err(_) => false,
    }
}

/// Read a selected strategy's lock file and return its path and digest.
fn lockfile_digest_for(
    strategy: &dyn LanguageStrategy,
    project_dir: &Path,
) -> Result<(PathBuf, String)> {
    let path =
        strategy
            .lockfile_path(project_dir)
            .ok_or_else(|| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!("no {} lock file found", strategy.id()),
            })?;
    let contents = fs::read(&path).map_err(|source| KvistError::Io {
        operation: "read lock file",
        path: path.clone(),
        source,
    })?;
    Ok((path, lockfile_digest(&contents)))
}

/// A manifest stamping helper shared across strategies.
pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

/// A locked Rust project's native cache catalogue; protected readiness is
/// enforced separately by its execution owner.
struct RustStrategy;

enum RustAdmission<'a> {
    InspectionOnly,
    Selected(Option<&'a crate::config::SandboxConfig>),
}

impl RustStrategy {
    fn native_sources(project_dir: &Path) -> Result<roetter::rust::cache::SourceView> {
        let home =
            roetter::rust::cache::home().map_err(|source| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!(
                    "cannot resolve native Cargo home (CARGO_HOME or HOME/.cargo): {source}"
                ),
            })?;
        roetter::rust::cache::sources(project_dir, &home, &sav::CancellationToken::new()).map_err(
            |source| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!("cannot inspect native Cargo cache: {source}"),
            },
        )
    }

    fn report(project_dir: &Path, context: RustAdmission<'_>) -> Result<VendoringReport> {
        use roetter::rust::cache::SourceAdmission;

        let sources = Self::native_sources(project_dir)?;
        let report = match sources.admission {
            SourceAdmission::Catalogue(report) => report,
            SourceAdmission::CargoOfflineRequired { .. } => match context {
                RustAdmission::InspectionOnly => {
                    return Err(KvistError::VendoringUnavailable {
                        path: project_dir.to_string_lossy().into_owned(),
                        reason: "noncanonical Rust sources require an explicit native source admission context; use configured offline admission or explicit operator provisioning".into(),
                    });
                }
                RustAdmission::Selected(sandbox) => {
                    crate::vendor_command::native_cargo_admission(
                        project_dir,
                        &sources.cargo_home,
                        sandbox,
                        &sav::CancellationToken::new(),
                    )?
                    .report
                }
            },
            SourceAdmission::NoLock => {
                return Err(KvistError::VendoringUnavailable {
                    path: project_dir.to_string_lossy().into_owned(),
                    reason: "Cargo.lock is missing; source-only selection is not a locked readiness report".into(),
                });
            }
        };
        let present_dependencies =
            (report.registry_dependencies + report.git_dependencies + report.path_dependencies)
                .saturating_sub(report.missing_dependencies.len());
        Ok(VendoringReport {
            language: "rust".to_owned(),
            project_root: report.project_root,
            lockfile_path: report.lockfile_path,
            lockfile_digest: report.lockfile_digest,
            vendored_dir: report.vendored_dir,
            present_dependencies,
            missing_dependencies: report.missing_dependencies,
        })
    }
}

impl LanguageStrategy for RustStrategy {
    fn id(&self) -> &'static str {
        "rust"
    }
    fn lockfile_names(&self) -> &'static [&'static str] {
        &["Cargo.lock"]
    }
    fn vendored_dirname(&self) -> &'static str {
        rust_vendoring::DEFAULT_VENDORED_DIRNAME
    }
    fn vendored_mount_dest(&self) -> &'static str {
        "/workspace/cargo-registry"
    }
    fn sandbox_config_dest(&self) -> &'static str {
        ""
    }

    fn enforce(&self, project_dir: &Path) -> Result<VendoringReport> {
        Self::report(project_dir, RustAdmission::InspectionOnly)
    }

    fn enforce_with_sandbox(
        &self,
        project_dir: &Path,
        sandbox: Option<&crate::config::SandboxConfig>,
    ) -> Result<VendoringReport> {
        Self::report(project_dir, RustAdmission::Selected(sandbox))
    }

    fn mounts(&self, project_dir: &Path, lockfile_digest: &str) -> Result<Vec<VendoredMount>> {
        let view = Self::native_sources(project_dir)?;
        let current = view
            .lockfile_digest
            .ok_or_else(|| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: "Cargo.lock disappeared before native source mount preparation".into(),
            })?;
        if current != lockfile_digest {
            return Err(KvistError::VendoringStale {
                path: project_dir.to_string_lossy().into_owned(),
                current,
                expected: lockfile_digest.to_owned(),
            });
        }
        Ok([
            (view.registry, "/workspace/cargo-registry"),
            (view.git, "/workspace/cargo-git"),
        ]
        .into_iter()
        .filter_map(|(source, destination)| {
            source.map(|source| VendoredMount {
                source,
                destination: destination.to_owned(),
                identity: lockfile_digest.to_owned(),
            })
        })
        .collect())
    }
}

/// A locked Go project's native module cache and declared-module catalogue.
struct GoStrategy;

impl LanguageStrategy for GoStrategy {
    fn id(&self) -> &'static str {
        "go"
    }
    fn lockfile_names(&self) -> &'static [&'static str] {
        &["go.sum"]
    }
    fn vendored_dirname(&self) -> &'static str {
        "vendor"
    }
    fn vendored_mount_dest(&self) -> &'static str {
        go_cache::MOUNT
    }
    fn sandbox_config_dest(&self) -> &'static str {
        ""
    }
    /// The conventional Go export location, not the native runtime cache.
    fn vendored_dir(&self, project_dir: &Path) -> PathBuf {
        project_dir.join(self.vendored_dirname())
    }

    fn enforce(&self, project_dir: &Path) -> Result<VendoringReport> {
        go_cache::report(project_dir)
    }

    fn mounts(&self, project_dir: &Path, lockfile_digest: &str) -> Result<Vec<VendoredMount>> {
        let current = go_cache::identity(project_dir)?;
        if current != lockfile_digest {
            return Err(KvistError::VendoringStale {
                path: project_dir.to_string_lossy().into_owned(),
                current,
                expected: lockfile_digest.to_owned(),
            });
        }
        let cache = go_cache::cache_root(project_dir)?;
        Ok(cache
            .is_dir()
            .then(|| VendoredMount {
                source: cache,
                destination: go_cache::MOUNT.into(),
                identity: lockfile_digest.to_owned(),
            })
            .into_iter()
            .collect())
    }
}

/// A locked Python project (`pip`/`uv`). Coarse presence inspection of a
/// wheels/sdists directory, a `pip.conf` that resolves offline, and a
/// provisioned virtualenv that makes the locked material importable in the
/// sandbox (the venv is the offline runtime; the wheels catalogue it).
struct PythonStrategy;

/// Kvist-owned virtualenv directory name under a project's `.kvist/`.
pub const PYTHON_VENV_DIRNAME: &str = "venv";

impl LanguageStrategy for PythonStrategy {
    fn id(&self) -> &'static str {
        "python"
    }
    fn lockfile_names(&self) -> &'static [&'static str] {
        &["requirements.lock.txt", "uv.lock"]
    }
    fn vendored_dirname(&self) -> &'static str {
        PYTHON_WHEELS_DIRNAME
    }
    fn vendored_mount_dest(&self) -> &'static str {
        "/workspace/vendored-python"
    }
    fn sandbox_config_dest(&self) -> &'static str {
        "/workspace/.pip"
    }

    fn enforce(&self, project_dir: &Path) -> Result<VendoringReport> {
        let (lockfile, digest) = lockfile_digest_for(self, project_dir)?;
        let vendored = self.vendored_dir(project_dir);
        let wheels_ready = directory_non_empty(&vendored);
        let venv_ready = venv_interpreter(project_dir).is_some();
        if !wheels_ready || !venv_ready {
            let mut missing_dependencies = Vec::new();
            if !wheels_ready {
                missing_dependencies.push(
                    "python: vendored wheels/sdists not provisioned; run `kvist vendor` on the host".to_owned(),
                );
            }
            if !venv_ready {
                missing_dependencies.push(
                    "python: virtualenv not provisioned; run `kvist vendor` on the host".to_owned(),
                );
            }
            return Ok(VendoringReport {
                language: "python".to_owned(),
                project_root: project_dir.to_string_lossy().into_owned(),
                lockfile_path: lockfile.to_string_lossy().into_owned(),
                lockfile_digest: digest,
                vendored_dir: vendored.to_string_lossy().into_owned(),
                present_dependencies: 0,
                missing_dependencies,
            });
        }
        Ok(VendoringReport {
            language: "python".to_owned(),
            project_root: project_dir.to_string_lossy().into_owned(),
            lockfile_path: lockfile.to_string_lossy().into_owned(),
            lockfile_digest: digest,
            vendored_dir: vendored.to_string_lossy().into_owned(),
            present_dependencies: count_lock_entries(&lockfile),
            missing_dependencies: Vec::new(),
        })
    }

    fn mounts(&self, project_dir: &Path, lockfile_digest: &str) -> Result<Vec<VendoredMount>> {
        let vendored = self.vendored_dir(project_dir);
        let contents = format!(
            "# Managed by `kvist vendor` (ADR-0011). Do not edit by hand.\n\
             [global]\n\
             no-index = true\n\
             find-links = {}\n",
            self.vendored_mount_dest()
        );
        let config_dir = config::prepare(project_dir, "sandbox-pip", "pip.conf", &contents)?;
        let venv = project_dir.join(".kvist").join(PYTHON_VENV_DIRNAME);
        Ok(vec![
            VendoredMount {
                source: vendored,
                destination: self.vendored_mount_dest().to_owned(),
                identity: lockfile_digest.to_owned(),
            },
            VendoredMount {
                source: config_dir,
                destination: self.sandbox_config_dest().to_owned(),
                identity: lockfile_digest.to_owned(),
            },
            // The provisioned virtualenv is the offline runtime: it is mounted
            // read-only and made the interpreter's site via `VIRTUAL_ENV`.
            VendoredMount {
                source: venv,
                destination: "/workspace/venv".to_owned(),
                identity: lockfile_digest.to_owned(),
            },
        ])
    }
}

/// The provisioned venv interpreter, when the venv exists and is complete.
fn venv_interpreter(project_dir: &Path) -> Option<PathBuf> {
    let venv = project_dir
        .join(".kvist")
        .join(PYTHON_VENV_DIRNAME)
        .join("bin")
        .join("python");
    venv.is_file().then_some(venv)
}

/// Which JavaScript/Node package manager owns this project, by lock file.
/// pnpm is preferred over the lock-file priority order because a `pnpm-lock.yaml`
/// requires the pnpm store, which is a distinct catalogue shape from the
/// npm/yarn tarball cache.
fn javascript_package_manager(project_dir: &Path) -> &'static str {
    if project_dir.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if project_dir.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm"
    }
}

/// A locked JavaScript/Node project's native cache/store presence and offline
/// `.npmrc`; this is not package-by-package completeness verification.
struct JavaScriptStrategy;

impl LanguageStrategy for JavaScriptStrategy {
    fn id(&self) -> &'static str {
        "javascript"
    }
    fn lockfile_names(&self) -> &'static [&'static str] {
        &["pnpm-lock.yaml", "yarn.lock", "package-lock.json"]
    }
    fn vendored_dirname(&self) -> &'static str {
        JS_PACKAGE_CACHE_DIRNAME
    }
    fn vendored_mount_dest(&self) -> &'static str {
        "/workspace/vendored-node"
    }
    fn sandbox_config_dest(&self) -> &'static str {
        "/workspace/.npm"
    }

    fn enforce(&self, project_dir: &Path) -> Result<VendoringReport> {
        let (lockfile, digest) = lockfile_digest_for(self, project_dir)?;
        let vendored = self.vendored_dir(project_dir);
        // The native catalogue shape differs per manager (npm cache, yarn
        // cache, pnpm content-addressable store) but its presence check is
        // the same: non-empty and lock-file digest identified.
        let catalogue = match javascript_package_manager(project_dir) {
            "pnpm" => "pnpm store",
            "yarn" => "yarn cache",
            _ => "npm cache",
        };
        if !directory_non_empty(&vendored) {
            return Ok(VendoringReport {
                language: "javascript".to_owned(),
                project_root: project_dir.to_string_lossy().into_owned(),
                lockfile_path: lockfile.to_string_lossy().into_owned(),
                lockfile_digest: digest,
                vendored_dir: vendored.to_string_lossy().into_owned(),
                present_dependencies: 0,
                missing_dependencies: vec![format!(
                    "javascript: {catalogue} not provisioned; run `kvist vendor` on the host"
                )],
            });
        }
        Ok(VendoringReport {
            language: "javascript".to_owned(),
            project_root: project_dir.to_string_lossy().into_owned(),
            lockfile_path: lockfile.to_string_lossy().into_owned(),
            lockfile_digest: digest,
            vendored_dir: vendored.to_string_lossy().into_owned(),
            present_dependencies: count_lock_entries(&lockfile),
            missing_dependencies: Vec::new(),
        })
    }

    fn mounts(&self, project_dir: &Path, lockfile_digest: &str) -> Result<Vec<VendoredMount>> {
        let vendored = self.vendored_dir(project_dir);
        // npm/yarn consume `cache=` for the offline tarball cache; pnpm uses a
        // content-addressable store referenced by `store-path`. The file is
        // bound to the lock-file digest identity and documents the offline
        // catalogue; the canonical `node --test` consumes the committed
        // node_modules, not this config.
        let contents = match javascript_package_manager(project_dir) {
            "pnpm" => format!(
                "# Managed by `kvist vendor` (ADR-0011). Do not edit by hand.\n\
                 offline=true\n\
                 store-path={}\n",
                self.vendored_mount_dest()
            ),
            _ => format!(
                "# Managed by `kvist vendor` (ADR-0011). Do not edit by hand.\n\
                 offline=true\n\
                 prefer-offline=true\n\
                 cache={}\n",
                self.vendored_mount_dest()
            ),
        };
        let config_dir = config::prepare(project_dir, "sandbox-npm", ".npmrc", &contents)?;
        Ok(vec![
            VendoredMount {
                source: vendored,
                destination: self.vendored_mount_dest().to_owned(),
                identity: lockfile_digest.to_owned(),
            },
            VendoredMount {
                source: config_dir,
                destination: self.sandbox_config_dest().to_owned(),
                identity: lockfile_digest.to_owned(),
            },
        ])
    }
}

/// A locked C/C++ project (`Conan`). The vendored material is a project-local
/// Conan home (`CONAN_HOME=.kvist/vendored-conan`) holding the exact locked
/// binaries, plus the generated build files under `.kvist/conan-build` that
/// the project build system consumes. The Conan home is mounted read-only at
/// its own host path in the sandbox so the absolute cache paths embedded in
/// the generated toolchain file resolve unchanged.
struct CConanStrategy;

impl LanguageStrategy for CConanStrategy {
    fn id(&self) -> &'static str {
        "c"
    }
    fn lockfile_names(&self) -> &'static [&'static str] {
        &["conanfile.lock"]
    }
    /// A Conan project is marked by its conanfile even before the lock file
    /// exists: `kvist vendor` generates `conanfile.lock` during provisioning
    /// (`conan install --lockfile-out`), so lock-file-only detection would
    /// make a fresh C/C++ project unprovisionable.
    fn detects(&self, project_dir: &Path) -> bool {
        project_dir.join("conanfile.txt").is_file()
            || project_dir.join("conanfile.py").is_file()
            || self.lockfile_path(project_dir).is_some()
    }
    fn vendored_dirname(&self) -> &'static str {
        CONAN_HOME_DIRNAME
    }
    fn vendored_mount_dest(&self) -> &'static str {
        "/workspace/vendored-conan"
    }
    fn sandbox_config_dest(&self) -> &'static str {
        "/workspace/.conan"
    }

    fn enforce(&self, project_dir: &Path) -> Result<VendoringReport> {
        let (lockfile, digest) = lockfile_digest_for(self, project_dir)?;
        let vendored = self.vendored_dir(project_dir);
        // Conan 2 keeps its native package cache under the home's `p/`.
        if !directory_non_empty(&vendored.join("p")) {
            return Ok(VendoringReport {
                language: "c".to_owned(),
                project_root: project_dir.to_string_lossy().into_owned(),
                lockfile_path: lockfile.to_string_lossy().into_owned(),
                lockfile_digest: digest,
                vendored_dir: vendored.to_string_lossy().into_owned(),
                present_dependencies: 0,
                missing_dependencies: vec![
                    "c: conan home not provisioned; run `kvist vendor` on the host \
                     (CONAN_HOME=.kvist/vendored-conan conan install --build=missing)"
                        .to_owned(),
                ],
            });
        }
        Ok(VendoringReport {
            language: "c".to_owned(),
            project_root: project_dir.to_string_lossy().into_owned(),
            lockfile_path: lockfile.to_string_lossy().into_owned(),
            lockfile_digest: digest,
            vendored_dir: vendored.to_string_lossy().into_owned(),
            present_dependencies: count_lock_entries(&lockfile),
            missing_dependencies: Vec::new(),
        })
    }

    fn mounts(&self, project_dir: &Path, lockfile_digest: &str) -> Result<Vec<VendoredMount>> {
        let vendored = self.vendored_dir(project_dir);
        let contents = format!(
            "# Managed by `kvist vendor` (ADR-0011/ADR-0013). Do not edit by hand.\n\n\
             Offline provisioning for this locked project:\n\n\
             - Host: `CONAN_HOME={} conan install --lockfile=conanfile.lock \\\n--build=missing -of .kvist/conan-build`\n\
             - Sandbox: the Conan home is mounted read-only at its host path;\
 the generated build files under `.kvist/conan-build` are inside the\
 component mount.\n",
            vendored.display()
        );
        let config_dir = config::prepare(project_dir, "sandbox-conan", "OFFLINE.md", &contents)?;
        // The Conan home is mounted at its own absolute host path (source ==
        // destination): the generated conan toolchain file embeds absolute
        // cache paths, so the sandbox sees them unchanged.
        let canonical = vendored.canonicalize().map_err(|source| KvistError::Io {
            operation: "canonicalize conan home for offline verification mount",
            path: vendored.clone(),
            source,
        })?;
        let destination = canonical
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| KvistError::Io {
                operation: "canonicalize conan home for offline verification mount",
                path: canonical.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "conan home path is not valid UTF-8",
                ),
            })?;
        Ok(vec![
            VendoredMount {
                source: canonical,
                destination,
                identity: lockfile_digest.to_owned(),
            },
            VendoredMount {
                source: config_dir,
                destination: self.sandbox_config_dest().to_owned(),
                identity: lockfile_digest.to_owned(),
            },
        ])
    }
}

/// A locked C/C++ project (`vcpkg`). The vendored material is a project-local
/// vcpkg root (`VCPKG_ROOT=.kvist/vendored-vcpkg`) holding the exact locked
/// ports plus the vcpkg tool, and the generated CMake toolchain files the
/// project build system consumes. The vcpkg root is mounted read-only at its
/// own canonical host path in the sandbox so the absolute cache paths embedded
/// in the generated toolchain file resolve unchanged.
struct CvcpkgStrategy;

impl LanguageStrategy for CvcpkgStrategy {
    fn id(&self) -> &'static str {
        "c-vcpkg"
    }
    fn lockfile_names(&self) -> &'static [&'static str] {
        &["vcpkg-lock.json"]
    }
    /// A vcpkg project is marked by its `vcpkg.json` manifest even before the
    /// generated `vcpkg-lock.json` exists: `kvist vendor` writes the lock file
    /// during provisioning (`vcpkg install --lockfile-out`), so lock-file-only
    /// detection would make a fresh C/C++ vcpkg project unprovisionable.
    fn detects(&self, project_dir: &Path) -> bool {
        project_dir.join("vcpkg.json").is_file() || self.lockfile_path(project_dir).is_some()
    }
    fn vendored_dirname(&self) -> &'static str {
        VCPKG_ROOT_DIRNAME
    }
    fn vendored_mount_dest(&self) -> &'static str {
        "/workspace/vendored-vcpkg"
    }
    fn sandbox_config_dest(&self) -> &'static str {
        "/workspace/.vcpkg"
    }

    fn enforce(&self, project_dir: &Path) -> Result<VendoringReport> {
        let (lockfile, digest) = lockfile_digest_for(self, project_dir)?;
        let vendored = self.vendored_dir(project_dir);
        // A provisioned vcpkg root holds the installed ports under `installed/`
        // (layout shared by the vcpkg release and ported binaries).
        if !directory_non_empty(&vendored.join("installed")) {
            return Ok(VendoringReport {
                language: "c-vcpkg".to_owned(),
                project_root: project_dir.to_string_lossy().into_owned(),
                lockfile_path: lockfile.to_string_lossy().into_owned(),
                lockfile_digest: digest,
                vendored_dir: vendored.to_string_lossy().into_owned(),
                present_dependencies: 0,
                missing_dependencies: vec![
                    "c: vcpkg root not provisioned; run `kvist vendor` on the host \
                     (VCPKG_ROOT=.kvist/vendored-vcpkg vcpkg install --locked --triplet x64-linux)"
                        .to_owned(),
                ],
            });
        }
        Ok(VendoringReport {
            language: "c-vcpkg".to_owned(),
            project_root: project_dir.to_string_lossy().into_owned(),
            lockfile_path: lockfile.to_string_lossy().into_owned(),
            lockfile_digest: digest,
            vendored_dir: vendored.to_string_lossy().into_owned(),
            present_dependencies: count_lock_entries(&lockfile),
            missing_dependencies: Vec::new(),
        })
    }

    fn mounts(&self, project_dir: &Path, lockfile_digest: &str) -> Result<Vec<VendoredMount>> {
        let vendored = self.vendored_dir(project_dir);
        let config_dir = project_dir.join(".kvist").join("sandbox-vcpkg");
        fs::create_dir_all(&config_dir).map_err(|source| KvistError::Io {
            operation: "create vcpkg sandbox config dir",
            path: config_dir.clone(),
            source,
        })?;
        let config_path = config_dir.join("OFFLINE.md");
        let contents = format!(
            "# Managed by `kvist vendor` (ADR-0011/ADR-0013). Do not edit by hand.\n\n\
             Offline provisioning for this locked project:\n\n\
             - Host: `VCPKG_ROOT={} vcpkg install --locked --triplet x64-linux`\n\
             - Sandbox: the vcpkg root is mounted read-only at its host path; the generated CMake toolchain file (`<VCPKG_ROOT>/scripts/buildsystems/vcpkg.cmake`) and the installed ports resolve unchanged inside the component mount.\n",
            vendored.display()
        );
        fs::write(&config_path, contents).map_err(|source| KvistError::Io {
            operation: "write vcpkg sandbox OFFLINE.md",
            path: config_path.clone(),
            source,
        })?;
        // The vcpkg root is mounted at its own absolute host path (source ==
        // destination): the generated toolchain file and the embedded install
        // paths resolve unchanged in the sandbox.
        let canonical = vendored.canonicalize().map_err(|source| KvistError::Io {
            operation: "canonicalize vcpkg root for offline verification mount",
            path: vendored.clone(),
            source,
        })?;
        let destination = canonical
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| KvistError::Io {
                operation: "canonicalize vcpkg root for offline verification mount",
                path: canonical.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "vcpkg root path is not valid UTF-8",
                ),
            })?;
        Ok(vec![
            VendoredMount {
                source: canonical,
                destination,
                identity: lockfile_digest.to_owned(),
            },
            VendoredMount {
                source: config_dir,
                destination: self.sandbox_config_dest().to_owned(),
                identity: lockfile_digest.to_owned(),
            },
        ])
    }
}

/// Count rough locked-package entries in a text lock file (one per non-empty,
/// non-comment, non-section line). Approximate but bounded, and only for reporting.
fn count_lock_entries(lockfile: &Path) -> usize {
    let Ok(text) = fs::read_to_string(lockfile) else {
        return 0;
    };
    text.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('['))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        for (name, contents) in files {
            fs::write(tmp.path().join(name), contents).expect("write lock");
        }
        tmp
    }

    fn seed_vendored(project: &Path, dir: &str) -> PathBuf {
        let dir = project.join(".kvist").join(dir);
        fs::create_dir_all(&dir).expect("vendored dir");
        fs::write(dir.join(".seed"), "x").expect("seed file");
        dir
    }

    fn seed_venv(project: &Path) -> PathBuf {
        let venv_python = project
            .join(".kvist")
            .join("venv")
            .join("bin")
            .join("python");
        fs::create_dir_all(venv_python.parent().expect("venv bin")).expect("venv bin dir");
        fs::write(&venv_python, "#!/usr/bin/python3\n").expect("venv python");
        venv_python
    }

    #[test]
    fn detects_rust_first_when_mixed_lock_files_present() {
        let tmp = project_with(&[
            (
                "Cargo.lock",
                "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.1.0\"\n",
            ),
            ("package-lock.json", "{\"name\":\"x\"}\n"),
        ]);
        assert_eq!(
            detect_language_strategy(tmp.path()).expect("detect").id(),
            "rust"
        );
    }

    #[test]
    fn detects_python_javascript_and_conan_lock_files() {
        assert_eq!(
            detect_language_strategy(project_with(&[("requirements.lock.txt", "")]).path())
                .expect("py")
                .id(),
            "python"
        );
        assert_eq!(
            detect_language_strategy(project_with(&[("package-lock.json", "{}")]).path())
                .expect("js")
                .id(),
            "javascript"
        );
        assert_eq!(
            detect_language_strategy(project_with(&[("yarn.lock", "# yarn lockfile\n")]).path())
                .expect("yarn")
                .id(),
            "javascript"
        );
        assert_eq!(
            detect_language_strategy(project_with(&[("conanfile.lock", "")]).path())
                .expect("conan")
                .id(),
            "c"
        );
        // A conanfile alone marks a C/C++ project before any lock file exists,
        // so kvist vendor can generate the lock file during provisioning.
        assert_eq!(
            detect_language_strategy(
                project_with(&[("conanfile.txt", "[requires]\nzlib/1.3.1\n")]).path()
            )
            .expect("conanfile.txt")
            .id(),
            "c"
        );
        assert_eq!(
            detect_language_strategy(project_with(&[("conanfile.py", "")]).path())
                .expect("conanfile.py")
                .id(),
            "c"
        );
    }

    #[test]
    fn detects_vcpkg_manifest_and_lock_file() {
        // A vcpkg.json manifest selects the vcpkg C/C++ strategy even before the
        // generated vcpkg-lock.json exists, so kvist vendor can generate it.
        assert_eq!(
            detect_language_strategy(project_with(&[("vcpkg.json", "{}")]).path())
                .expect("vcpkg.json")
                .id(),
            "c-vcpkg"
        );
        assert_eq!(
            detect_language_strategy(project_with(&[("vcpkg-lock.json", "{}")]).path())
                .expect("vcpkg-lock.json")
                .id(),
            "c-vcpkg"
        );
        // Conan is listed first: a project with both a conanfile and a vcpkg.json
        // keeps the established Conan detection.
        assert_eq!(
            detect_language_strategy(
                project_with(&[
                    ("conanfile.txt", "[requires]\nzlib/1.3.1\n"),
                    ("vcpkg.json", "{}"),
                ])
                .path()
            )
            .expect("conan + vcpkg")
            .id(),
            "c"
        );
    }

    #[test]
    fn vcpkg_mounts_document_offline_provisioning_as_a_single_line() {
        // The offline OFFLINE.md must describe vcpkg provisioning as one clean
        // line: the generated toolchain path stays inline and no space is
        // dropped between words (guards against a stray newline in the format
        // string splitting the description across lines).
        let tmp = project_with(&[
            (
                "vcpkg.json",
                "{\"name\":\"e2eminivcpkg\",\"dependencies\":[\"zlib\"]}\n",
            ),
            ("vcpkg-lock.json", "{}\n"),
        ]);
        let project = tmp.path();
        let strategy = detect_language_strategy(project).expect("vcpkg strategy");
        // A provisioned vcpkg root holds the installed ports under installed/.
        let vcpkg_root = project.join(".kvist").join("vendored-vcpkg");
        fs::create_dir_all(vcpkg_root.join("installed")).expect("installed dir");
        fs::write(vcpkg_root.join("installed").join(".seed"), "x").expect("seed installed");

        let report = strategy.enforce(project).expect("report ready");
        assert_eq!(report.language, "c-vcpkg");
        assert!(report.ready());

        let mounts = strategy
            .mounts(project, &report.lockfile_digest)
            .expect("mounts");
        // The vendored vcpkg root (mounted at its canonical host path) plus the
        // sandbox OFFLINE.md config dir, each bound to the lock-file digest.
        assert_eq!(mounts.len(), 2);
        for mount in &mounts {
            assert_eq!(mount.identity, report.lockfile_digest);
            assert!(mount.destination.starts_with('/'));
        }
        // The vcpkg root is mounted at its own canonical host path (source ==
        // destination) so the toolchain file's absolute paths resolve unchanged
        // inside the sandbox.
        let canonical_root = vcpkg_root.canonicalize().expect("canonical vcpkg root");
        let root_mount = mounts
            .iter()
            .find(|mount| mount.source == canonical_root)
            .expect("the vendored vcpkg root is mounted");
        assert_eq!(root_mount.source.to_string_lossy(), root_mount.destination);

        // The offline catalogue is a single, well-formed line: exactly one
        // "- Sandbox:" line, with the toolchain path inline and no dropped space.
        let config_mount = mounts
            .iter()
            .find(|mount| mount.destination == "/workspace/.vcpkg")
            .expect("the sandbox config dir is mounted");
        let offline = config_mount.source.join("OFFLINE.md");
        let text = fs::read_to_string(&offline).expect("OFFLINE.md");
        let sandbox_lines: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("- Sandbox:"))
            .collect();
        assert_eq!(
            sandbox_lines.len(),
            1,
            "the Sandbox description must be a single line"
        );
        assert_eq!(
            sandbox_lines[0].trim_start(),
            "- Sandbox: the vcpkg root is mounted read-only at its host path; the generated CMake toolchain file (`<VCPKG_ROOT>/scripts/buildsystems/vcpkg.cmake`) and the installed ports resolve unchanged inside the component mount."
        );
    }

    #[test]
    fn detects_pnpm_lock_file_and_reports_pnpm_package_manager() {
        // A pnpm-lock.yaml selects the JavaScript strategy, and the offline
        // catalogue is the pnpm store (a distinct shape from the npm/yarn cache).
        assert_eq!(
            detect_language_strategy(
                project_with(&[("pnpm-lock.yaml", "\"lockfileVersion\":\"6\"\n")]).path()
            )
            .expect("pnpm")
            .id(),
            "javascript"
        );
    }

    #[test]
    fn detects_python_uv_lock_and_prefers_pip_lock_when_both_present() {
        // `uv.lock` selects the Python strategy so `uv`-locked projects are
        // vendored through the same offline path as `requirements.lock.txt`.
        assert_eq!(
            detect_language_strategy(project_with(&[("uv.lock", "")]).path())
                .expect("uv.lock")
                .id(),
            "python"
        );
        // When both Python lock files are present, `requirements.lock.txt` is
        // preferred, so `uv.lock`-only provisioning is the only uv path taken.
        assert_eq!(
            detect_language_strategy(
                project_with(&[("requirements.lock.txt", ""), ("uv.lock", ""),]).path()
            )
            .expect("both")
            .id(),
            "python"
        );
    }

    #[test]
    fn rejects_project_without_any_supported_lock_file() {
        let tmp = project_with(&[("README.md", "no lock here")]);
        assert!(matches!(
            detect_language_strategy(tmp.path()),
            Err(KvistError::VendoringUnavailable { .. })
        ));
    }

    #[test]
    fn non_rust_strategy_reports_missing_then_ready() {
        let tmp = project_with(&[("requirements.lock.txt", "click==8.1.7\nflask==3.0.0\n")]);
        let project = tmp.path();
        let strategy = detect_language_strategy(project).unwrap();
        let report = strategy.enforce(project).expect("report");
        assert_eq!(report.language, "python");
        assert!(!report.ready());
        assert!(!report.missing_dependencies.is_empty());

        seed_vendored(project, "vendored-python");
        let report = strategy.enforce(project).expect("report");
        assert!(
            !report.ready(),
            "wheels alone are not enough; the venv is required"
        );

        seed_venv(project);
        let report = strategy.enforce(project).expect("report");
        assert!(report.ready());
        assert!(report.lockfile_digest.starts_with(DIGEST_PREFIX));
        assert_eq!(report.present_dependencies, 2);
    }

    #[test]
    fn enforce_vendoring_returns_digest_identified_monts() {
        let tmp = project_with(&[("requirements.lock.txt", "click==8.1.7\n")]);
        let project = tmp.path();
        seed_vendored(project, "vendored-python");
        seed_venv(project);
        let enforcement = enforce_vendoring(project, false, None).expect("enforce");
        assert_eq!(enforcement.language, "python");
        assert!(enforcement.lockfile_digest.starts_with(DIGEST_PREFIX));
        // Wheels, the pip config, and the provisioned virtualenv.
        assert_eq!(enforcement.mounts.len(), 3);
        assert!(
            enforcement
                .mounts
                .iter()
                .any(|mount| mount.destination == "/workspace/venv")
        );
        for mount in &enforcement.mounts {
            assert_eq!(mount.identity, enforcement.lockfile_digest);
            assert!(mount.destination.starts_with('/'));
        }
    }

    #[test]
    fn mount_identity_is_lockfile_digest_not_tree_hash() {
        let lock = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.1.0\"\n";
        let digest = lockfile_digest(lock.as_bytes());
        assert_eq!(digest, rust_vendoring::lockfile_digest(lock.as_bytes()));
        assert!(digest.starts_with("sha256:"));
    }
}
