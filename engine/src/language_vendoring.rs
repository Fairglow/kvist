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
        // The manifest must exist (provisioning ran); its recorded absolute
        // paths are ignored for resolution (see below).
        let _manifest = rust_vendoring::VendorManifest::load(project_dir)
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
        // The registry and sandbox-config locations are fixed relative to the
        // project root, so resolve them from `project_dir` (the location cargo
        // and the sandbox actually see) rather than the absolute paths the
        // manifest recorded on the host that produced it. Using the recorded
        // paths would break any checkout at a different root with
        // `No such file or directory`.
        let vendored = self.vendored_dir(project_dir);
        let sandbox_cargo = project_dir
            .join(".kvist")
            .join(rust_vendoring::SANDBOX_CARGO_CONFIG_DIRNAME);
        Ok(vec![
            VendoredMount {
                source: vendored,
                destination: self.vendored_mount_dest().to_owned(),
                identity: lockfile_digest.to_owned(),
            },
            VendoredMount {
                source: sandbox_cargo,
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

/// A locked JavaScript/Node project (`npm`/`yarn`/`pnpm`). Present-and-matching
/// catalogue (tarball cache or pnpm store) and an offline `.npmrc`.
struct JavaScriptStrategy;

impl LanguageStrategy for JavaScriptStrategy {
    fn id(&self) -> &'static str {
        "javascript"
    }
    fn lockfile_names(&self) -> &'static [&'static str] {
        &["package-lock.json", "yarn.lock", "pnpm-lock.yaml"]
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
        // The vendored catalogue shape differs per manager (npm/yarn tarball
        // cache, pnpm content-addressable store) but its readiness contract is
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
        let config_dir = project_dir.join(".kvist").join("sandbox-npm");
        fs::create_dir_all(&config_dir).map_err(|source| KvistError::Io {
            operation: "create javascript sandbox config dir",
            path: config_dir.clone(),
            source,
        })?;
        let config_path = config_dir.join(".npmrc");
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
    fn rust_mounts_resolve_from_current_root_not_recorded_host_path() {
        let tmp = project_with(&[(
            "Cargo.lock",
            "version = 4\n\n[[package]]\nname = \"serde_json\"\nversion = \"1.0.151\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        )]);
        let project = tmp.path();
        let strategy = detect_language_strategy(project).unwrap();

        // Provision the material under the CURRENT root.
        fs::create_dir_all(project.join(".kvist").join("vendored").join("serde_json"))
            .expect("vendored package");

        // Record a manifest whose absolute paths name a DIFFERENT host root
        // (the path the manifest recorded on the machine that produced it).
        // This is the real-world layout: `kvist vendor` writes the producing
        // host's absolute paths, and the project is later checked out at a
        // different root.
        let lockfile_digest = rust_vendoring::lockfile_digest(
            &fs::read(project.join("Cargo.lock")).expect("lockfile"),
        );
        let manifest = rust_vendoring::VendorManifest {
            schema_version: rust_vendoring::VENDOR_SCHEMA_VERSION,
            generated_at_unix_secs: 1,
            project_root: "/opt/proj/kvist".to_owned(),
            lockfile_path: "Cargo.lock".to_owned(),
            lockfile_digest: lockfile_digest.clone(),
            vendored_dir: "/opt/proj/kvist/.kvist/vendored".to_owned(),
            sandbox_cargo_dir: "/opt/proj/kvist/.kvist/sandbox-cargo".to_owned(),
            sandbox_vendored_mount: rust_vendoring::VENDOR_SANDBOX_MOUNT.to_owned(),
            verified: true,
        };
        manifest.save(project).expect("save manifest");

        // Enforcement must resolve the registry from the CURRENT root (where
        // the material is), not the recorded host path (which does not exist
        // here). A stale absolute path must not produce "No such file or
        // directory".
        let report = strategy
            .enforce(project)
            .expect("enforce with foreign-root manifest");
        assert!(report.ready());

        // The mounts the offline build will make must point at the current
        // root's directories, not the recorded `/opt/proj/kvist` paths.
        let mounts = strategy.mounts(project, &lockfile_digest).expect("mounts");
        let registry = mounts
            .iter()
            .find(|mount| mount.destination == rust_vendoring::VENDOR_SANDBOX_MOUNT)
            .expect("registry mount");
        assert_eq!(registry.source, project.join(".kvist").join("vendored"));
        assert!(
            !registry
                .source
                .to_string_lossy()
                .starts_with("/opt/proj/kvist")
        );
        let cargo_config = mounts
            .iter()
            .find(|mount| mount.destination == rust_vendoring::SANDBOX_CARGO_CONFIG_MOUNT)
            .expect("cargo-config mount");
        assert_eq!(
            cargo_config.source,
            project
                .join(".kvist")
                .join(rust_vendoring::SANDBOX_CARGO_CONFIG_DIRNAME)
        );
        assert!(
            !cargo_config
                .source
                .to_string_lossy()
                .starts_with("/opt/proj/kvist")
        );
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
