//! Vendoring enforcement: make Rust builds/tests runnable with the sandbox
//! network denied (ADR-0011).
//!
//! The Kvist sandbox denies network for every execution phase, and verification
//! mounts only the component and the toolchain. A `cargo` invocation therefore
//! cannot fetch dependencies: it must resolve them from material already present
//! on disk. This module provisions, records, and enforces that material so an
//! offline `cargo test --locked` succeeds deterministically.
//!
//! Kvist owns vendoring. `kvist vendor` populates a vendored registry (once, with
//! the host cargo) and writes a versioned manifest under the project. Verification
//! re-reads the manifest, re-checks that every locked registry dependency is
//! present, and fails closed when vendoring is incomplete or the lockfile has
//! drifted. The vendored registry and a sandbox cargo config are intended to be
//! mounted read-only during sandbox verification, with the cargo config mapped at
//! `/workspace/.cargo` (distinct from the component mount, which the sandbox runner
//! forbids overlapping). That verification wiring is the planned integration.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{KvistError, Result};

/// Vendoring manifest schema version.
pub const VENDOR_SCHEMA_VERSION: u32 = 1;
/// The lockfile name this capability enforces.
pub const CARGO_LOCK_FILENAME: &str = "Cargo.lock";
/// The manifest name recorded under `<project>/.kvist/`.
pub const VENDOR_MANIFEST_FILENAME: &str = "vendoring-v1.json";
/// Default vendored-registry directory name under `<project>/.kvist/`.
pub const DEFAULT_VENDORED_DIRNAME: &str = "vendored";
/// Host directory holding the sandbox cargo config mounted at `/workspace/.cargo`.
pub const SANDBOX_CARGO_CONFIG_DIRNAME: &str = "sandbox-cargo";
/// Fixed sandbox destination for the vendored registry.
pub const VENDOR_SANDBOX_MOUNT: &str = "/workspace/vendored";
/// Fixed sandbox destination for a directory that contains a `config.toml` mapping
/// the crates.io source at the vendored registry.
pub const SANDBOX_CARGO_CONFIG_MOUNT: &str = "/workspace/.cargo";
const GIT_SOURCE_PREFIX: &str = "git+";

/// The origin of a locked package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockSource {
    /// A registry (crates.io or a configured registry).
    Registry,
    /// An immutable Git dependency.
    Git,
    /// A path or workspace dependency resolved from the project.
    Path,
}

impl LockSource {
    /// Whether this package must resolve from vendored/on-disk material.
    ///
    /// Path and workspace dependencies live inside the project and need nothing
    /// vendored; registry and Git dependencies must resolve from on-disk material
    /// because the sandbox denies network.
    pub fn needs_durable_source(&self) -> bool {
        matches!(self, LockSource::Registry | LockSource::Git)
    }

    fn parse(raw: Option<&str>) -> LockSource {
        match raw {
            Some(value) if value.starts_with(GIT_SOURCE_PREFIX) => LockSource::Git,
            Some(_) => LockSource::Registry,
            None => LockSource::Path,
        }
    }
}

/// One entry of `Cargo.lock`: its name, version, and origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockPackage {
    /// The package name.
    pub name: String,
    /// The locked version.
    pub version: String,
    /// The package origin.
    pub source: LockSource,
}

#[derive(Deserialize)]
struct CargoLockFile {
    #[serde(default)]
    package: Vec<RawLockPackage>,
}

#[derive(Deserialize)]
struct RawLockPackage {
    name: String,
    version: String,
    source: Option<String>,
}

/// Parse `Cargo.lock` contents into locked packages. The lockfile is trusted
/// only as a bounded input here; unknown keys are ignored and a missing
/// `package` array yields an empty list.
pub fn parse_cargo_lock(contents: &str) -> std::result::Result<Vec<LockPackage>, String> {
    let lock: CargoLockFile =
        toml::from_str(contents).map_err(|source| format!("cannot parse Cargo.lock: {source}"))?;
    Ok(lock
        .package
        .into_iter()
        .map(|raw| LockPackage {
            name: raw.name,
            version: raw.version,
            source: LockSource::parse(raw.source.as_deref()),
        })
        .collect())
}

/// The canonical SHA-256 digest of lockfile bytes, prefixed for readability.
pub fn lockfile_digest(contents: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(contents)))
}

/// The directory names cargo uses for a vendored package. Newer cargo uses a
/// flat `<name>` directory; older cargo used `<name>-<version>`. Both are
/// accepted so enforcement is robust across cargo versions.
fn vendored_entry_candidates(pkg: &LockPackage) -> (PathBuf, PathBuf) {
    let flat = PathBuf::from(&pkg.name);
    let numbered = PathBuf::from(format!("{}-{}", pkg.name, pkg.version));
    (flat, numbered)
}

/// Whether a registry or Git dependency is present in the vendored directory
/// with the exact version locked in Cargo.lock. The version is checked by
/// parsing the vendored package's Cargo.toml to ensure the vendored material
/// actually matches the locked version, not just that the crate name exists.
fn durable_source_present(vendored: &Path, pkg: &LockPackage) -> bool {
    let (flat, numbered) = vendored_entry_candidates(pkg);

    // Check flat layout first (newer cargo)
    let flat_dir = vendored.join(flat);
    if flat_dir.is_dir() && version_matches(&flat_dir, &pkg.version) {
        return true;
    }

    // Check numbered layout (older cargo)
    let numbered_dir = vendored.join(numbered);
    if numbered_dir.is_dir() {
        // For numbered layout, the version is in the directory name, which
        // we already know matches because we constructed the path from pkg.version.
        return true;
    }

    false
}

/// Check if the version in a vendored package's Cargo.toml matches the expected version.
fn version_matches(vendored_pkg_dir: &Path, expected_version: &str) -> bool {
    // Try Cargo.toml first, fall back to Cargo.toml.orig
    let cargo_toml = vendored_pkg_dir.join("Cargo.toml");
    let cargo_toml_orig = vendored_pkg_dir.join("Cargo.toml.orig");

    let content = if cargo_toml.is_file() {
        std::fs::read_to_string(&cargo_toml).ok()
    } else if cargo_toml_orig.is_file() {
        std::fs::read_to_string(&cargo_toml_orig).ok()
    } else {
        return false;
    };

    match content {
        Some(text) => {
            // Parse version from Cargo.toml
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("version = \"") {
                    let version = trimmed
                        .trim_start_matches("version = \"")
                        .trim_end_matches("\"")
                        .to_string();
                    return version == expected_version;
                }
            }
            false
        }
        None => false,
    }
}

/// The outcome of checking that a locked project can build offline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendoringReport {
    /// Canonical project directory the report is about.
    pub project_root: String,
    /// Path of the lockfile that was checked.
    pub lockfile_path: String,
    /// Digest of the lockfile that was checked.
    pub lockfile_digest: String,
    /// Vendored-registry directory that was inspected.
    pub vendored_dir: String,
    /// Registry dependencies that must resolve from the vendored registry.
    pub registry_dependencies: usize,
    /// Git dependencies that must resolve from on-disk material.
    pub git_dependencies: usize,
    /// Path/workspace dependencies resolved from the project itself.
    pub path_dependencies: usize,
    /// Locked dependencies not found in the vendored layout, as `name-version`.
    pub missing_dependencies: Vec<String>,
}

impl std::fmt::Display for VendoringReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            formatter,
            "offline vendoring ready for {}",
            self.project_root
        )?;
        writeln!(
            formatter,
            "  registry dependencies: {}",
            self.registry_dependencies
        )?;
        writeln!(formatter, "  git dependencies: {}", self.git_dependencies)?;
        writeln!(formatter, "  path dependencies: {}", self.path_dependencies)?;
        writeln!(formatter, "  vendored registry: {}", self.vendored_dir)
    }
}

impl VendoringReport {
    /// Whether every locked dependency that needs durable material is present.
    pub fn ready(&self) -> bool {
        self.missing_dependencies.is_empty()
    }

    /// A human-readable, non-secret explanation of why vendoring is incomplete.
    pub fn missing_human_readable(&self) -> String {
        self.missing_dependencies.to_vec().join(", ")
    }

    /// Returns `Err` when the project cannot build offline.
    pub fn enforce(&self) -> Result<()> {
        if self.ready() {
            return Ok(());
        }
        Err(KvistError::VendoringIncomplete {
            path: self.project_root.clone(),
            count: self.missing_dependencies.len(),
            missing: self.missing_human_readable(),
        })
    }
}

/// Ensure a locked project can build offline: every registry and Git
/// dependency must be present under the vendored directory.
///
/// Only returns an error for setup failures (a missing or unreadable lockfile).
/// An incomplete vendored directory is reported, not fatal, so callers can
/// decide how to present it.
pub fn enforce_offline_readiness(
    project_root: &Path,
    vendored_dir: &Path,
) -> Result<VendoringReport> {
    let lockfile_path = project_root.join(CARGO_LOCK_FILENAME);
    let contents = std::fs::read(&lockfile_path).map_err(|source| KvistError::Io {
        operation: "read Cargo.lock for vendoring readiness",
        path: lockfile_path.clone(),
        source,
    })?;
    let digest = lockfile_digest(&contents);
    let lock_text = String::from_utf8_lossy(&contents);
    let packages =
        parse_cargo_lock(&lock_text).map_err(|reason| KvistError::VendoringUnavailable {
            path: project_root.to_string_lossy().into_owned(),
            reason,
        })?;
    let mut registry = 0usize;
    let mut git = 0usize;
    let mut path = 0usize;
    let mut missing = Vec::new();
    for package in &packages {
        match package.source {
            LockSource::Registry => registry += 1,
            LockSource::Git => git += 1,
            LockSource::Path => path += 1,
        }
        if package.source.needs_durable_source() && !durable_source_present(vendored_dir, package) {
            tracing::debug!(
                name = %package.name,
                version = %package.version,
                "package not found in vendored directory"
            );
            missing.push(format!("{}-{}", package.name, package.version));
        }
    }
    Ok(VendoringReport {
        project_root: project_root.to_string_lossy().into_owned(),
        lockfile_path: lockfile_path.to_string_lossy().into_owned(),
        lockfile_digest: digest,
        vendored_dir: vendored_dir.to_string_lossy().into_owned(),
        registry_dependencies: registry,
        git_dependencies: git,
        path_dependencies: path,
        missing_dependencies: missing,
    })
}

/// Identify packages present in the vendored directory but not listed in
/// Cargo.lock. These are stale and should be cleaned up so the vendored
/// directory exactly matches the locked dependencies.
pub fn find_stale_vendored_packages(
    project_root: &Path,
    vendored_dir: &Path,
) -> Result<Vec<String>> {
    let lockfile_path = project_root.join(CARGO_LOCK_FILENAME);
    let contents = std::fs::read(&lockfile_path).map_err(|source| KvistError::Io {
        operation: "read Cargo.lock for stale package detection",
        path: lockfile_path.clone(),
        source,
    })?;
    let lock_text = String::from_utf8_lossy(&contents);
    let packages =
        parse_cargo_lock(&lock_text).map_err(|reason| KvistError::VendoringUnavailable {
            path: project_root.to_string_lossy().into_owned(),
            reason,
        })?;

    // Build a set of (name, version) pairs for all packages that should be
    // vendored (registry and git sources).
    let mut expected = std::collections::HashSet::new();
    for package in &packages {
        if package.source.needs_durable_source() {
            expected.insert((package.name.clone(), package.version.clone()));
        }
    }

    // Scan the vendored directory and find packages that are not expected.
    let mut stale = Vec::new();
    let entries = std::fs::read_dir(vendored_dir).map_err(|source| KvistError::Io {
        operation: "scan vendored directory for stale packages",
        path: vendored_dir.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| KvistError::Io {
            operation: "read vendored directory entry",
            path: vendored_dir.to_path_buf(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().to_string();

        // Only process directories
        if !entry.path().is_dir() {
            continue;
        }

        // Try to extract version from Cargo.toml (works for both flat and numbered layouts)
        let version = extract_version_from_vendored_entry(&entry.path());

        if let Some(version) = version {
            // Determine package name: use Cargo.toml name if available, otherwise
            // parse from directory name
            let package_name =
                extract_name_from_vendored_entry(&entry.path()).unwrap_or_else(|| {
                    // Fall back to parsing directory name
                    if let Some(pos) = name.rfind('-') {
                        let possible_version = &name[pos + 1..];
                        if possible_version
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_digit())
                        {
                            name[..pos].to_string()
                        } else {
                            name.clone()
                        }
                    } else {
                        name.clone()
                    }
                });

            if !expected.contains(&(package_name, version)) {
                stale.push(name);
            }
        } else {
            // Can't determine version, fall back to parsing directory name
            if let Some(pos) = name.rfind('-') {
                let possible_version = &name[pos + 1..];
                if possible_version
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
                {
                    let package_name = &name[..pos];
                    if !expected.contains(&(package_name.to_string(), possible_version.to_string()))
                    {
                        stale.push(name);
                    }
                }
            }
        }
    }

    Ok(stale)
}

/// Extract the package name from a vendored package's Cargo.toml.
fn extract_name_from_vendored_entry(vendored_pkg_dir: &Path) -> Option<String> {
    let cargo_toml = vendored_pkg_dir.join("Cargo.toml");
    let cargo_toml_orig = vendored_pkg_dir.join("Cargo.toml.orig");

    let content = if cargo_toml.is_file() {
        std::fs::read_to_string(&cargo_toml).ok()
    } else if cargo_toml_orig.is_file() {
        std::fs::read_to_string(&cargo_toml_orig).ok()
    } else {
        return None;
    };

    match content {
        Some(text) => {
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("name = \"") {
                    let name = trimmed
                        .trim_start_matches("name = \"")
                        .trim_end_matches("\"")
                        .to_string();
                    return Some(name);
                }
            }
            None
        }
        None => None,
    }
}

/// Extract the version string from a vendored package's Cargo.toml.
fn extract_version_from_vendored_entry(vendored_pkg_dir: &Path) -> Option<String> {
    // Try Cargo.toml first, fall back to Cargo.toml.orig
    let cargo_toml = vendored_pkg_dir.join("Cargo.toml");
    let cargo_toml_orig = vendored_pkg_dir.join("Cargo.toml.orig");

    let content = if cargo_toml.is_file() {
        std::fs::read_to_string(&cargo_toml).ok()
    } else if cargo_toml_orig.is_file() {
        std::fs::read_to_string(&cargo_toml_orig).ok()
    } else {
        return None;
    };

    match content {
        Some(text) => {
            // Parse version from Cargo.toml
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("version = \"") {
                    let version = trimmed
                        .trim_start_matches("version = \"")
                        .trim_end_matches("\"")
                        .to_string();
                    return Some(version);
                }
            }
            None
        }
        None => None,
    }
}

/// Remove stale packages from the vendored directory.
pub fn remove_stale_vendored_packages(
    vendored_dir: &Path,
    stale_packages: &[String],
) -> Result<usize> {
    let mut removed = 0;
    for package in stale_packages {
        let path = vendored_dir.join(package);
        if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|source| KvistError::Io {
                operation: "remove stale vendored package",
                path: path.clone(),
                source,
            })?;
            removed += 1;
            tracing::debug!(package = %package, "removed stale vendored package");
        }
    }
    if removed > 0 {
        tracing::info!(
            vendored_dir = %vendored_dir.to_string_lossy(),
            removed = removed,
            "removed stale vendored packages"
        );
    }
    Ok(removed)
}

/// A versioned record of a project's vendored dependency material, owned and
/// re-enforced by Kvist. It is written under `<project>/.kvist/` and re-validated
/// on every verification run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendorManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Unix seconds when the manifest was last written.
    pub generated_at_unix_secs: u64,
    /// Canonical project directory.
    pub project_root: String,
    /// Lockfile path relative to the project directory.
    pub lockfile_path: String,
    /// Digest of the lockfile the vendored material was produced against.
    pub lockfile_digest: String,
    /// Absolute host path of the vendored registry.
    pub vendored_dir: String,
    /// Absolute host path of the directory mounted at `/workspace/.cargo`.
    pub sandbox_cargo_dir: String,
    /// Sandbox destination of the vendored registry (informational).
    pub sandbox_vendored_mount: String,
    /// Whether the last check found the project ready to build offline.
    pub verified: bool,
}

impl VendorManifest {
    /// The path the manifest is recorded at for a project directory.
    pub fn path_in(project_dir: &Path) -> PathBuf {
        project_dir.join(".kvist").join(VENDOR_MANIFEST_FILENAME)
    }

    /// Load the manifest for a project, or `Ok(None)` when it is absent.
    pub fn load(project_dir: &Path) -> Result<Option<VendorManifest>> {
        let path = Self::path_in(project_dir);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "read vendoring manifest",
                    path: path.clone(),
                    source,
                });
            }
        };
        let manifest: VendorManifest =
            serde_json::from_slice(&bytes).map_err(|source| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!("cannot parse vendoring manifest: {source}"),
            })?;
        Ok(Some(manifest))
    }

    /// Persist the manifest atomically under `<project>/.kvist/`.
    pub fn save(&self, project_dir: &Path) -> Result<()> {
        let path = Self::path_in(project_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| KvistError::Io {
                operation: "create vendoring manifest directory",
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let bytes =
            serde_json::to_vec_pretty(self).map_err(|source| KvistError::VendoringUnavailable {
                path: project_dir.to_string_lossy().into_owned(),
                reason: format!("cannot serialize vendoring manifest: {source}"),
            })?;
        crate::file_io::replace_file_atomically(&path, &String::from_utf8_lossy(&bytes))
    }

    /// The current lockfile digest for a project directory.
    pub fn current_digest(project_dir: &Path) -> Result<String> {
        let lockfile_path = project_dir.join(CARGO_LOCK_FILENAME);
        let contents = std::fs::read(&lockfile_path).map_err(|source| KvistError::Io {
            operation: "read Cargo.lock for vendoring staleness check",
            path: lockfile_path.clone(),
            source,
        })?;
        Ok(lockfile_digest(&contents))
    }

    /// The on-disk vendored registry for this project, resolved from the
    /// current project root. The manifest's recorded `vendored_dir` is the
    /// absolute path of the host that produced it (informational); resolution
    /// must always derive from the project root so a checkout at any location
    /// — including the sandbox's — resolves to the registry that is actually
    /// there instead of a `No such file or directory` on the recorded host.
    pub fn vendored_registry(&self, project_dir: &Path) -> PathBuf {
        project_dir.join(".kvist").join(DEFAULT_VENDORED_DIRNAME)
    }

    /// The sandbox cargo-configuration directory for this project, resolved
    /// from the current project root; see [`Self::vendored_registry`].
    pub fn sandbox_cargo_dir(&self, project_dir: &Path) -> PathBuf {
        project_dir
            .join(".kvist")
            .join(SANDBOX_CARGO_CONFIG_DIRNAME)
    }

    /// Recompute a readiness report against the current project state.
    pub fn report(&self, project_dir: &Path) -> Result<VendoringReport> {
        enforce_offline_readiness(project_dir, &self.vendored_registry(project_dir))
    }

    /// Enforce that the manifest still matches a buildable project. Returns an
    /// error when the lockfile has drifted since vendoring or a dependency is
    /// missing from the vendored registry.
    pub fn assert_ready(&self, project_dir: &Path) -> Result<VendoringReport> {
        let current = Self::current_digest(project_dir)?;
        if current != self.lockfile_digest {
            return Err(KvistError::VendoringStale {
                path: project_dir.to_string_lossy().into_owned(),
                current,
                expected: self.lockfile_digest.clone(),
            });
        }
        let report = self.report(project_dir)?;
        report.enforce()?;
        Ok(report)
    }
}

/// Render a cargo configuration that maps the crates.io source to a vendored
/// directory at `directory`, so a locked build resolves everything offline.
///
/// `directory` is the fixed sandbox mount path (`VENDOR_SANDBOX_MOUNT`). This
/// configuration is the authoritative offline resolver config for sandbox
/// verification: it is mounted at `/workspace/.cargo`, a parent of the sandbox
/// working directory. Kvist deliberately does not write source-replacement keys
/// into the project's own `.cargo/config.toml`, because that file is carried
/// into the sandbox through the component mount and would shadow this one.
pub fn offline_cargo_config(directory: &str) -> String {
    format!(
        "# Managed by `kvist vendor` (ADR-0011). Do not edit by hand.\n\
         [source.crates-io]\n\
         replace-with = \"vendored-sources\"\n\
         \n\
         [source.vendored-sources]\n\
         directory = \"{directory}\"\n"
    )
}

/// The current Unix time in seconds, used to stamp a freshly written manifest.
pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK_WITH_MISSING: &str = "version = 4\n\n\
[[package]]\nname = \"vendortest\"\nversion = \"0.1.0\"\n\n\
[[package]]\nname = \"serde_json\"\nversion = \"1.0.151\"\n\
source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
checksum = \"c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14\"\n\n\
[[package]]\nname = \"itoa\"\nversion = \"1.0.11\"\n\
source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
checksum = \"41ed3c71d68f1f04ad7790f37911905f10320b73714c9c6f7e6f6b92\"\n\n\
[[package]]\nname = \"local_dep\"\nversion = \"0.1.0\"\n";

    fn write_lock(dir: &Path, contents: &str) {
        std::fs::create_dir_all(dir).expect("project dir");
        std::fs::write(dir.join(CARGO_LOCK_FILENAME), contents).expect("write lock");
    }

    fn report_for(lock: &str, present: &[&str]) -> VendoringReport {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project = tmp.path();
        write_lock(project, lock);
        let vendored = project.join("vendored");
        for entry in present {
            std::fs::create_dir_all(vendored.join(entry)).expect("vendored dir");
        }
        enforce_offline_readiness(project, &vendored).expect("readiness check")
    }

    fn digest_of(contents: &str) -> String {
        lockfile_digest(contents.as_bytes())
    }

    #[test]
    fn classifies_sources_by_prefix() {
        let parsed = parse_cargo_lock(LOCK_WITH_MISSING).expect("parse");
        let by_name: std::collections::BTreeMap<&str, LockSource> = parsed
            .iter()
            .map(|package| (package.name.as_str(), package.source))
            .collect();
        assert_eq!(by_name["serde_json"], LockSource::Registry);
        assert_eq!(by_name["itoa"], LockSource::Registry);
        assert_eq!(by_name["local_dep"], LockSource::Path);
    }

    #[test]
    fn ready_only_when_all_durable_sources_are_present() {
        // `vendortest` (the workspace root) and `local_dep` are both source-less,
        // so they classify as path dependencies: the project counts as one and the
        // local path dependency as the other.
        let report = report_for(LOCK_WITH_MISSING, &["serde_json", "itoa"]);
        assert!(report.ready());
        assert_eq!(report.registry_dependencies, 2);
        assert_eq!(report.path_dependencies, 2);
        report.enforce().expect("ready project enforces");

        let incomplete = report_for(LOCK_WITH_MISSING, &["serde_json"]);
        assert!(!incomplete.ready());
        assert_eq!(
            incomplete.missing_dependencies,
            vec!["itoa-1.0.11".to_owned()]
        );
        assert!(matches!(
            incomplete.enforce(),
            Err(KvistError::VendoringIncomplete { count: 1, .. })
        ));
    }

    #[test]
    fn accepts_classic_name_version_layout() {
        let report = report_for(LOCK_WITH_MISSING, &["serde_json-1.0.151", "itoa-1.0.11"]);
        assert!(report.ready(), "classic layout should be accepted");
    }

    #[test]
    fn digest_is_deterministic_and_prefixed() {
        assert!(digest_of(LOCK_WITH_MISSING).starts_with("sha256:"));
        assert_eq!(digest_of(LOCK_WITH_MISSING), digest_of(LOCK_WITH_MISSING));
        let other = "version = 4\n\n[[package]]\nname = \"z\"\nversion = \"1\"\n";
        assert_ne!(digest_of(LOCK_WITH_MISSING), digest_of(other));
    }

    #[test]
    fn offline_cargo_config_maps_crates_io_to_vendored() {
        let config = offline_cargo_config(VENDOR_SANDBOX_MOUNT);
        assert!(config.contains("replace-with = \"vendored-sources\""));
        assert!(config.contains(&format!("directory = \"{VENDOR_SANDBOX_MOUNT}\"")));
    }

    #[test]
    fn manifest_round_trips_and_detects_staleness() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let project = tmp.path();
        write_lock(project, LOCK_WITH_MISSING);
        // The registry lives at `.kvist/vendored`; resolution derives from the
        // project root (not the manifest's recorded absolute path).
        let vendored = project.join(".kvist").join("vendored");
        std::fs::create_dir_all(vendored.join("serde_json")).expect("vendored dir");
        std::fs::create_dir_all(vendored.join("itoa")).expect("vendored dir");
        let digest = digest_of(LOCK_WITH_MISSING);
        let manifest = VendorManifest {
            schema_version: VENDOR_SCHEMA_VERSION,
            generated_at_unix_secs: 1,
            project_root: project.to_string_lossy().into_owned(),
            lockfile_path: CARGO_LOCK_FILENAME.to_owned(),
            lockfile_digest: digest.clone(),
            vendored_dir: vendored.to_string_lossy().into_owned(),
            sandbox_cargo_dir: project
                .join(".kvist")
                .join(SANDBOX_CARGO_CONFIG_DIRNAME)
                .to_string_lossy()
                .into_owned(),
            sandbox_vendored_mount: VENDOR_SANDBOX_MOUNT.to_owned(),
            verified: true,
        };
        manifest.save(project).expect("save");
        assert_eq!(
            VendorManifest::load(project).expect("load"),
            Some(manifest.clone())
        );
        assert!(
            manifest.assert_ready(project).is_ok(),
            "matching lock enforces"
        );

        let mut drifted = String::from(LOCK_WITH_MISSING);
        drifted.push_str("\n\n[[package]]\nname = \"new_dep\"\nversion = \"0.1.0\"\n");
        std::fs::write(project.join(CARGO_LOCK_FILENAME), &drifted).expect("write drifted");
        let fresh = VendorManifest::load(project)
            .expect("load")
            .expect("present");
        assert!(matches!(
            fresh.assert_ready(project),
            Err(KvistError::VendoringStale { .. })
        ));
    }
}
