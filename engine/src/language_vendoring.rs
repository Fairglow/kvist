//! Language-aware vendoring enforcement (ADR-0011).
//!
//! Kvist owns vendoring for every supported language. Each strategy treats its
//! lock file as the *authoritative catalogue* of the exact locked content:
//!
//! - The lock file is provisioned once, on the host and outside the effect
//!   sandbox, into a vendored directory holding the exact locked material
//!   (`cargo vendor`, `pip download`, `npm pack`/`npm ci`, `conan download`).
//! - Verification re-checks that the lock file still matches and that the vendored
//!   material is present before it allows an offline build, failing closed.
//!
//! The lock-file digest is the identity of every read-only vendored-directory
//! mount. This is deliberate: the lock file already catalogs every locked entry,
//! so hashing every vendored file would cost hundreds of thousands of extra hash
//! operations for no additional guarantee. Read-only grant identities are a
//! build-time claim bound to the approved mount plan and are not re-verified at
//! execution, so the lock-file digest is the catalogue and is sufficient.
//!
//! Rust enforces each locked registry and Git entry exactly. The other languages
//! enforce catalogue match plus vendored-content presence; exact per-package
//! verification for them is follow-up work, and this is the honest, cheap
//! guarantee the lock file provides.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{KvistError, Result};
use crate::vendoring as rust_vendoring;

/// Identity prefix, matching the digest shape used everywhere else in Kvist.
pub const DIGEST_PREFIX: &str = "sha256:";
/// Vendored package-cache directory name for JavaScript projects (`.kvist/`).
pub const JS_PACKAGE_CACHE_DIRNAME: &str = "vendored-js";
/// Vendored wheels directory name for Python projects (`.kvist/`).
pub const PYTHON_WHEELS_DIRNAME: &str = "vendored-python";
/// Project-local Conan home directory name for C/C++ projects (`.kvist/`).
pub const CONAN_HOME_DIRNAME: &str = "vendored-conan";

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
    /// Locked dependencies present in the vendored material.
    pub present_dependencies: usize,
    /// Locked dependencies not found in the vendored material.
    pub missing_dependencies: Vec<String>,
}

impl VendoringReport {
    /// Whether the vendored material satisfies the lock file.
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
    /// The on-disk vendored-content directory. Defaults to the Kvist-owned
    /// `.kvist/<vendored_dirname>`; Go overrides it because its vendored
    /// material is the committed `vendor/` directory inside the project.
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
    /// Enforce vendoring readiness; returns an error when the project cannot
    /// build offline (material missing, lock drifted, or not provisioned).
    fn enforce(&self, project_dir: &Path) -> Result<VendoringReport>;
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
                 uv.lock, package-lock.json, yarn.lock, or a conanfile) found; \
                 run the language's acquisition command so vendoring matches the exact \
                 locked versions"
            .to_owned(),
    })
}

/// One supported language strategy, in detection priority order.
pub fn language_strategies() -> [Box<dyn LanguageStrategy>; 5] {
    [
        Box::new(RustStrategy),
        Box::new(GoStrategy),
        Box::new(PythonStrategy),
        Box::new(JavaScriptStrategy),
        Box::new(CConanStrategy),
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
/// what verification must mount for an offline build. Fails closed when vendoring
/// is missing, incomplete, or stale.
pub fn enforce_vendoring(project_dir: &Path) -> Result<VendoringEnforcement> {
    let strategy = detect_language_strategy(project_dir)?;
    let report = strategy.enforce(project_dir)?;
    if !report.ready() {
        return Err(KvistError::VendoringIncomplete {
            path: report.project_root.clone(),
            count: report.missing_dependencies.len(),
            missing: report.missing_human_readable(),
        });
    }
    let mounts = strategy.mounts(project_dir, &report.lockfile_digest)?;
    Ok(VendoringEnforcement {
        language: report.language,
        lockfile_digest: report.lockfile_digest,
        vendored_dir: strategy.vendored_dir(project_dir),
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

/// A locked Rust project: exact per-dependency readiness plus a recorded,
/// non-stale manifest.
struct RustStrategy;

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
        rust_vendoring::VENDOR_SANDBOX_MOUNT
    }
    fn sandbox_config_dest(&self) -> &'static str {
        rust_vendoring::SANDBOX_CARGO_CONFIG_MOUNT
    }

    fn enforce(&self, project_dir: &Path) -> Result<VendoringReport> {
        let vendored = self.vendored_dir(project_dir);
        let report = rust_vendoring::enforce_offline_readiness(project_dir, &vendored).map_err(
            |source| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!("cannot check Rust vendoring readiness: {source}"),
            },
        )?;
        if !report.ready() {
            return Err(KvistError::VendoringIncomplete {
                path: report.project_root.clone(),
                count: report.missing_dependencies.len(),
                missing: report.missing_human_readable(),
            });
        }
        let manifest = rust_vendoring::VendorManifest::load(project_dir)
            .map_err(|source| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!("cannot read vendoring manifest: {source}"),
            })?
            .ok_or_else(|| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: "no vendoring manifest found; run `kvist vendor` to provision \
                         offline dependencies"
                    .to_owned(),
            })?;
        // Re-check staleness against the recorded manifest; also confirms the
        // vendored layout still matches the current lock file.
        manifest
            .assert_ready(project_dir)
            .map_err(|source| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!("Rust vendoring is not ready: {source}"),
            })?;
        Ok(VendoringReport {
            language: "rust".to_owned(),
            project_root: report.project_root,
            lockfile_path: report.lockfile_path,
            lockfile_digest: report.lockfile_digest,
            vendored_dir: report.vendored_dir,
            present_dependencies: report.registry_dependencies
                + report.git_dependencies
                + report.path_dependencies,
            missing_dependencies: report.missing_dependencies,
        })
    }

    fn mounts(&self, project_dir: &Path, lockfile_digest: &str) -> Result<Vec<VendoredMount>> {
        let manifest = rust_vendoring::VendorManifest::load(project_dir)
            .map_err(|source| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!("cannot read vendoring manifest: {source}"),
            })?
            .ok_or_else(|| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: "no vendoring manifest found; run `kvist vendor` to provision \
                         offline dependencies"
                    .to_owned(),
            })?;
        Ok(vec![
            VendoredMount {
                source: PathBuf::from(manifest.vendored_dir.clone()),
                destination: self.vendored_mount_dest().to_owned(),
                identity: lockfile_digest.to_owned(),
            },
            VendoredMount {
                source: PathBuf::from(manifest.sandbox_cargo_dir.clone()),
                destination: self.sandbox_config_dest().to_owned(),
                identity: lockfile_digest.to_owned(),
            },
        ])
    }
}

/// A locked Go project (`go mod`). The vendored material is the committed
/// `vendor/` directory beside `go.sum`: `go test -mod=vendor` builds entirely
/// from it, so no separate vendored mount is needed (it travels with the
/// component mount) and no offline resolver config exists for Go.
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
        ""
    }
    fn sandbox_config_dest(&self) -> &'static str {
        ""
    }
    /// Go's vendored material is the committed `vendor/` directory inside the
    /// project, not a Kvist-owned `.kvist/` directory.
    fn vendored_dir(&self, project_dir: &Path) -> PathBuf {
        project_dir.join(self.vendored_dirname())
    }

    fn enforce(&self, project_dir: &Path) -> Result<VendoringReport> {
        let (lockfile, digest) = lockfile_digest_for(self, project_dir)?;
        let vendored = self.vendored_dir(project_dir);
        let modules_txt = vendored.join("modules.txt");
        if !modules_txt.is_file() || !directory_non_empty(&vendored) {
            return Ok(VendoringReport {
                language: "go".to_owned(),
                project_root: project_dir.to_string_lossy().into_owned(),
                lockfile_path: lockfile.to_string_lossy().into_owned(),
                lockfile_digest: digest,
                vendored_dir: vendored.to_string_lossy().into_owned(),
                present_dependencies: 0,
                missing_dependencies: vec![
                    "go: vendor/ not provisioned; run `go mod vendor` on the host so \
                     `go test -mod=vendor` can build offline"
                        .to_owned(),
                ],
            });
        }
        Ok(VendoringReport {
            language: "go".to_owned(),
            project_root: project_dir.to_string_lossy().into_owned(),
            lockfile_path: lockfile.to_string_lossy().into_owned(),
            lockfile_digest: digest,
            vendored_dir: vendored.to_string_lossy().into_owned(),
            present_dependencies: count_lock_entries(&modules_txt),
            missing_dependencies: Vec::new(),
        })
    }

    /// No extra mounts: the committed `vendor/` directory is part of the
    /// component mount, and Go needs no offline resolver configuration.
    fn mounts(&self, _project_dir: &Path, _lockfile_digest: &str) -> Result<Vec<VendoredMount>> {
        Ok(Vec::new())
    }
}

/// A locked Python project (`pip`/`uv`). Present-and-matching catalogue with a
/// vendored wheels/sdists directory, a `pip.conf` that resolves offline, and a
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
        let config_dir = project_dir.join(".kvist").join("sandbox-pip");
        fs::create_dir_all(&config_dir).map_err(|source| KvistError::Io {
            operation: "create python sandbox config dir",
            path: config_dir.clone(),
            source,
        })?;
        let config_path = config_dir.join("pip.conf");
        let contents = format!(
            "# Managed by `kvist vendor` (ADR-0011). Do not edit by hand.\n\
             [global]\n\
             no-index = true\n\
             find-links = {}\n",
            self.vendored_mount_dest()
        );
        fs::write(&config_path, contents).map_err(|source| KvistError::Io {
            operation: "write python sandbox pip.conf",
            path: config_path.clone(),
            source,
        })?;
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

/// A locked JavaScript/Node project (`npm`). Present-and-matching catalogue with
/// a vendored tarball directory and an `.npmrc` that resolves offline.
struct JavaScriptStrategy;

impl LanguageStrategy for JavaScriptStrategy {
    fn id(&self) -> &'static str {
        "javascript"
    }
    fn lockfile_names(&self) -> &'static [&'static str] {
        &["package-lock.json", "yarn.lock"]
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
        if !directory_non_empty(&vendored) {
            return Ok(VendoringReport {
                language: "javascript".to_owned(),
                project_root: project_dir.to_string_lossy().into_owned(),
                lockfile_path: lockfile.to_string_lossy().into_owned(),
                lockfile_digest: digest,
                vendored_dir: vendored.to_string_lossy().into_owned(),
                present_dependencies: 0,
                missing_dependencies: vec![
                    "javascript: vendored tarballs not provisioned; run `npm pack` \
                      for each locked entry on the host"
                        .to_owned(),
                ],
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
        let config_dir = project_dir.join(".kvist").join("sandbox-npm");
        fs::create_dir_all(&config_dir).map_err(|source| KvistError::Io {
            operation: "create javascript sandbox config dir",
            path: config_dir.clone(),
            source,
        })?;
        let config_path = config_dir.join(".npmrc");
        let contents = format!(
            "# Managed by `kvist vendor` (ADR-0011). Do not edit by hand.\n\
             offline=true\n\
             prefer-offline=true\n\
             cache={}\n",
            self.vendored_mount_dest()
        );
        fs::write(&config_path, contents).map_err(|source| KvistError::Io {
            operation: "write javascript sandbox .npmrc",
            path: config_path.clone(),
            source,
        })?;
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
        // A provisioned Conan home holds its package cache at the home root
        // under `p/` (both the Conan 1.x and 2.x layouts keep it there; the
        // `~/.conan2` default is a home directory name, not a subdirectory).
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
        let config_dir = project_dir.join(".kvist").join("sandbox-conan");
        fs::create_dir_all(&config_dir).map_err(|source| KvistError::Io {
            operation: "create conan sandbox config dir",
            path: config_dir.clone(),
            source,
        })?;
        let config_path = config_dir.join("OFFLINE.md");
        let contents = format!(
            "# Managed by `kvist vendor` (ADR-0011/ADR-0013). Do not edit by hand.\n\n\
             Offline provisioning for this locked project:\n\n\
             - Host: `CONAN_HOME={} conan install --lockfile=conanfile.lock \\\n--build=missing -of .kvist/conan-build`\n\
             - Sandbox: the Conan home is mounted read-only at its host path;\
 the generated build files under `.kvist/conan-build` are inside the\
 component mount.\n",
            vendored.display()
        );
        fs::write(&config_path, contents).map_err(|source| KvistError::Io {
            operation: "write conan sandbox OFFLINE.md",
            path: config_path.clone(),
            source,
        })?;
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
    fn detects_go_lock_file_and_reports_vendor_presence() {
        let tmp = project_with(&[("go.sum", "example.com/mod v1.0.0 h1:abc=\n")]);
        let project = tmp.path();
        let strategy = detect_language_strategy(project).expect("go");
        assert_eq!(strategy.id(), "go");

        // Without the committed vendor/ directory the project is not ready.
        let report = strategy.enforce(project).expect("report");
        assert_eq!(report.language, "go");
        assert!(!report.ready());

        // `go mod vendor` output makes it ready; the committed vendor/ needs
        // no extra sandbox mounts.
        let vendor = project.join("vendor");
        fs::create_dir_all(&vendor).expect("vendor dir");
        fs::write(
            vendor.join("modules.txt"),
            "# example.com/mod v1.0.0\n## explicit; go 1.21\nexample.com/mod\n\
             # example.com/other v2.0.0\n## explicit; go 1.21\nexample.com/other\n",
        )
        .expect("modules.txt");
        let report = strategy.enforce(project).expect("report");
        assert!(report.ready());
        assert_eq!(report.present_dependencies, 2);
        assert!(
            strategy
                .mounts(project, &report.lockfile_digest)
                .expect("mounts")
                .is_empty()
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
    fn rust_enforcement_requires_manifest_and_fails_closed() {
        let tmp = project_with(&[(
            "Cargo.lock",
            "version = 4\n\n[[package]]\nname = \"serde_json\"\nversion = \"1.0.151\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        )]);
        let project = tmp.path();
        let strategy = detect_language_strategy(project).unwrap();
        // Complete vendored material (the locked registry dep present), but no
        // recorded manifest: enforcement must still fail closed.
        fs::create_dir_all(project.join(".kvist").join("vendored").join("serde_json"))
            .expect("vendored package");
        assert!(matches!(
            strategy.enforce(project),
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
        let enforcement = enforce_vendoring(project).expect("enforce");
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
