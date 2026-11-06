//! Extracted maerg catalogue/report/manifest API.
//!
//! Name/version observations deliberately retain their cheap legacy semantics.
//! Use [`crate::rust`] for full locked source and content integrity.

use crate::{
    Error, Result,
    fs::{Dir, LOCK_LIMIT, METADATA_LIMIT, segment},
    unavailable,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Vendoring manifest schema version.
pub const VENDOR_SCHEMA_VERSION: u32 = 1;
/// Lockfile name enforced by Rust vendoring.
pub const CARGO_LOCK_FILENAME: &str = "Cargo.lock";
/// Manifest name under `.kvist`.
pub const VENDOR_MANIFEST_FILENAME: &str = "vendoring-v1.json";
/// Default registry directory under `.kvist`.
pub const DEFAULT_VENDORED_DIRNAME: &str = "vendored";
/// Directory containing generated sandbox Cargo configuration.
pub const SANDBOX_CARGO_CONFIG_DIRNAME: &str = "sandbox-cargo";
/// Fixed maerg sandbox registry destination.
pub const VENDOR_SANDBOX_MOUNT: &str = "/workspace/vendored";
/// Fixed maerg sandbox Cargo configuration destination.
pub const SANDBOX_CARGO_CONFIG_MOUNT: &str = "/workspace/.cargo";

/// The origin of a locked package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockSource {
    /// Registry dependency.
    Registry,
    /// Immutable Git dependency.
    Git,
    /// Project path or workspace dependency.
    Path,
}
impl LockSource {
    /// Whether this source requires durable external material.
    pub fn needs_durable_source(&self) -> bool {
        matches!(self, Self::Registry | Self::Git)
    }
    fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some(value) if value.starts_with("git+") => Self::Git,
            Some(_) => Self::Registry,
            None => Self::Path,
        }
    }
}

/// A name/version catalogue entry from Cargo.lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockPackage {
    /// Package name.
    pub name: String,
    /// Locked version.
    pub version: String,
    /// Package origin class, not exact provenance.
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

/// Parse the cheap catalogue; unknown keys and absent packages retain legacy semantics.
/// Parse diagnostics never include untrusted TOML snippets.
pub fn parse_cargo_lock(contents: &str) -> std::result::Result<Vec<LockPackage>, String> {
    if contents.len() as u64 > LOCK_LIMIT {
        return Err("Cargo.lock exceeds size limit".into());
    }
    let lock: CargoLockFile = toml::from_str(contents).map_err(|_| "cannot parse Cargo.lock")?;
    if lock.package.len() > 100_000 {
        return Err("Cargo.lock exceeds package limit".into());
    }
    lock.package
        .into_iter()
        .map(|raw| {
            if !segment(&raw.name) || !segment(&raw.version) {
                return Err("unsafe package identity in Cargo.lock".into());
            }
            Ok(LockPackage {
                name: raw.name,
                version: raw.version,
                source: LockSource::parse(raw.source.as_deref()),
            })
        })
        .collect()
}

/// SHA-256 identity of exact lock bytes, with a readable prefix.
pub fn lockfile_digest(contents: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(contents)))
}

fn lock(project: &Path) -> Result<Vec<u8>> {
    Dir::open(project)?
        .read(CARGO_LOCK_FILENAME, LOCK_LIMIT)
        .map(|(bytes, _)| bytes)
}

fn packages(project: &Path, bytes: &[u8]) -> Result<Vec<LockPackage>> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| unavailable(project, "Cargo.lock is not UTF-8"))?;
    parse_cargo_lock(text).map_err(|reason| Error::VendoringUnavailable {
        path: project.to_string_lossy().into_owned(),
        reason,
    })
}

fn package_field(directory: &Dir, key: &str) -> Result<Option<String>> {
    let name = if directory.exists("Cargo.toml")? {
        "Cargo.toml"
    } else if directory.exists("Cargo.toml.orig")? {
        "Cargo.toml.orig"
    } else {
        return Ok(None);
    };
    let (bytes, _) = directory.read(name, METADATA_LIMIT)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| unavailable(&directory.display, "package manifest is not UTF-8"))?;
    let prefix = format!("{key} = \"");
    Ok(text.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix(&prefix)
            .map(|value| value.trim_end_matches('"').to_owned())
    }))
}

fn durable_source_present(vendor: Option<&Dir>, pkg: &LockPackage) -> Result<bool> {
    let Some(vendor) = vendor else {
        return Ok(false);
    };
    if vendor.exists(&pkg.name)? {
        let flat = vendor.child(&pkg.name)?;
        if package_field(&flat, "version")?.as_deref() == Some(pkg.version.as_str()) {
            return Ok(true);
        }
    }
    let numbered = format!("{}-{}", pkg.name, pkg.version);
    if vendor.exists(&numbered)? {
        vendor.child(&numbered)?;
        return Ok(true);
    }
    Ok(false)
}

/// Cheap catalogue report. `ready` does not imply full file integrity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VendoringReport {
    /// Project directory observed.
    pub project_root: String,
    /// Lock path observed.
    pub lockfile_path: String,
    /// Digest of exact lock bytes.
    pub lockfile_digest: String,
    /// Registry directory observed.
    pub vendored_dir: String,
    /// Registry dependency count.
    pub registry_dependencies: usize,
    /// Git dependency count.
    pub git_dependencies: usize,
    /// Local dependency count.
    pub path_dependencies: usize,
    /// Missing entries, formatted as name-version.
    pub missing_dependencies: Vec<String>,
}
impl std::fmt::Display for VendoringReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "offline vendoring ready for {}", self.project_root)?;
        writeln!(f, "  registry dependencies: {}", self.registry_dependencies)?;
        writeln!(f, "  git dependencies: {}", self.git_dependencies)?;
        writeln!(f, "  path dependencies: {}", self.path_dependencies)?;
        writeln!(f, "  vendored registry: {}", self.vendored_dir)
    }
}
impl VendoringReport {
    /// Whether every required catalogue entry was observed.
    pub fn ready(&self) -> bool {
        self.missing_dependencies.is_empty()
    }
    /// Non-secret missing-name summary.
    pub fn missing_human_readable(&self) -> String {
        self.missing_dependencies.join(", ")
    }
    /// Refuse an incomplete catalogue.
    pub fn enforce(&self) -> Result<()> {
        if self.ready() {
            return Ok(());
        }
        Err(Error::VendoringIncomplete {
            path: self.project_root.clone(),
            count: self.missing_dependencies.len(),
            missing: self.missing_human_readable(),
        })
    }
}

/// Observe exact name/version presence with the legacy flat/numbered layouts.
/// This helper is not full integrity validation.
pub fn enforce_offline_readiness(
    project_root: &Path,
    vendored_dir: &Path,
) -> Result<VendoringReport> {
    let contents = lock(project_root)?;
    let packages = packages(project_root, &contents)?;
    let parent = vendored_dir
        .parent()
        .ok_or_else(|| unavailable(vendored_dir, "registry has no parent"))?;
    let vendor = match std::fs::symlink_metadata(vendored_dir) {
        Ok(_) => Some(Dir::open(vendored_dir)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // No material is an incomplete catalogue, not a parse or I/O success.
            let _ = parent;
            None
        }
        Err(e) => return Err(crate::fs::io_error("inspect catalogue", vendored_dir, e)),
    };
    let mut report = VendoringReport {
        project_root: project_root.to_string_lossy().into_owned(),
        lockfile_path: project_root
            .join(CARGO_LOCK_FILENAME)
            .to_string_lossy()
            .into_owned(),
        lockfile_digest: lockfile_digest(&contents),
        vendored_dir: vendored_dir.to_string_lossy().into_owned(),
        registry_dependencies: 0,
        git_dependencies: 0,
        path_dependencies: 0,
        missing_dependencies: vec![],
    };
    for package in packages {
        match package.source {
            LockSource::Registry => report.registry_dependencies += 1,
            LockSource::Git => report.git_dependencies += 1,
            LockSource::Path => report.path_dependencies += 1,
        }
        if package.source.needs_durable_source()
            && !durable_source_present(vendor.as_ref(), &package)?
        {
            report
                .missing_dependencies
                .push(format!("{}-{}", package.name, package.version));
        }
    }
    Ok(report)
}

/// Find stale name/version catalogue directories, retaining legacy observation semantics.
pub fn find_stale_vendored_packages(
    project_root: &Path,
    vendored_dir: &Path,
) -> Result<Vec<String>> {
    let contents = lock(project_root)?;
    let expected: std::collections::HashSet<_> = packages(project_root, &contents)?
        .into_iter()
        .filter(|pkg| pkg.source.needs_durable_source())
        .map(|pkg| (pkg.name, pkg.version))
        .collect();
    let vendor = Dir::open(vendored_dir)?;
    let mut stale = Vec::new();
    for name in vendor.entries()? {
        let metadata = std::fs::symlink_metadata(vendor.path().join(&name)).map_err(|e| {
            crate::fs::io_error("inspect catalogue entry", &vendor.display.join(&name), e)
        })?;
        if metadata.file_type().is_symlink() {
            return Err(unavailable(vendored_dir, "catalogue contains link"));
        }
        if !metadata.is_dir() {
            continue;
        }
        let entry = vendor.child(&name)?;
        let fallback = name
            .rsplit_once('-')
            .filter(|(_, version)| version.chars().next().is_some_and(|c| c.is_ascii_digit()));
        if let Some(version) = package_field(&entry, "version")? {
            let pkg_name = package_field(&entry, "name")?
                .unwrap_or_else(|| fallback.map(|(pkg, _)| pkg).unwrap_or(&name).to_owned());
            if !expected.contains(&(pkg_name, version)) {
                stale.push(name);
            }
        } else if let Some((pkg_name, version)) = fallback
            && !expected.contains(&(pkg_name.to_owned(), version.to_owned()))
        {
            stale.push(name);
        }
    }
    Ok(stale)
}

/// Explicitly remove named stale catalogue directories; traversal and links are refused.
pub fn remove_stale_vendored_packages(
    vendored_dir: &Path,
    stale_packages: &[String],
) -> Result<usize> {
    let vendor = Dir::open(vendored_dir)?;
    let mut removed = 0;
    for package in stale_packages {
        if !segment(package) {
            return Err(unavailable(vendored_dir, "unsafe stale package name"));
        }
        if vendor.exists(package)? {
            let entry = vendor.child(package)?;
            crate::integrity::validate_tree(&entry, &sav::CancellationToken::new())?;
            std::fs::remove_dir_all(vendor.path().join(package)).map_err(|e| {
                crate::fs::io_error(
                    "remove stale vendored package",
                    &vendor.display.join(package),
                    e,
                )
            })?;
            removed += 1;
        }
    }
    vendor.sync()?;
    Ok(removed)
}

/// Versioned legacy manifest. Recorded host paths are informational, never grants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VendorManifest {
    /// Independent schema version.
    pub schema_version: u32,
    /// Timestamp of publication.
    pub generated_at_unix_secs: u64,
    /// Informational producer path.
    pub project_root: String,
    /// Relative lock filename.
    pub lockfile_path: String,
    /// Exact locked graph digest.
    pub lockfile_digest: String,
    /// Informational producer registry path.
    pub vendored_dir: String,
    /// Informational producer configuration path.
    pub sandbox_cargo_dir: String,
    /// Informational sandbox registry destination.
    pub sandbox_vendored_mount: String,
    /// Whether publication was verified.
    pub verified: bool,
}
impl VendorManifest {
    /// Default project manifest path.
    pub fn path_in(project_dir: &Path) -> PathBuf {
        project_dir.join(".kvist").join(VENDOR_MANIFEST_FILENAME)
    }
    /// Load a bounded regular manifest, or return None when absent.
    pub fn load(project_dir: &Path) -> Result<Option<Self>> {
        let project = Dir::open(project_dir)?;
        if !project.exists(".kvist")? {
            return Ok(None);
        }
        let state = project.child(".kvist")?;
        if !state.exists(VENDOR_MANIFEST_FILENAME)? {
            return Ok(None);
        }
        let (bytes, _) = state.read(VENDOR_MANIFEST_FILENAME, METADATA_LIMIT)?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| unavailable(project_dir, "cannot parse vendoring manifest"))
    }
    /// Atomically persist this manifest with file/directory synchronization.
    pub fn save(&self, project_dir: &Path) -> Result<()> {
        let state = Dir::open(project_dir)?.ensure_child(".kvist")?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|_| unavailable(project_dir, "cannot serialize vendoring manifest"))?;
        state.atomic(VENDOR_MANIFEST_FILENAME, &bytes)
    }
    /// Digest of current bounded regular lock bytes.
    pub fn current_digest(project_dir: &Path) -> Result<String> {
        Ok(lockfile_digest(&lock(project_dir)?))
    }
    /// Default registry derived from this checkout, never the recorded producer path.
    pub fn vendored_registry(&self, project_dir: &Path) -> PathBuf {
        project_dir.join(".kvist").join(DEFAULT_VENDORED_DIRNAME)
    }
    /// Default resolver directory derived from this checkout.
    pub fn sandbox_cargo_dir(&self, project_dir: &Path) -> PathBuf {
        project_dir
            .join(".kvist")
            .join(SANDBOX_CARGO_CONFIG_DIRNAME)
    }
    /// Recompute the cheap catalogue report.
    pub fn report(&self, project_dir: &Path) -> Result<VendoringReport> {
        enforce_offline_readiness(project_dir, &self.vendored_registry(project_dir))
    }
    /// Enforce the expected lock binding and full current managed readiness.
    pub fn assert_ready(&self, project_dir: &Path) -> Result<VendoringReport> {
        let current = Self::current_digest(project_dir)?;
        if current != self.lockfile_digest {
            return Err(Error::VendoringStale {
                path: project_dir.to_string_lossy().into_owned(),
                current,
                expected: self.lockfile_digest.clone(),
            });
        }
        let report = crate::rust::check(project_dir, &sav::CancellationToken::new())?;
        report.enforce()?;
        Ok(report)
    }
}

/// Legacy crates.io-only resolver renderer. Full consumers must gate it structurally.
pub fn offline_cargo_config(directory: &str) -> String {
    format!(
        "# Managed by `kvist vendor` (ADR-0011). Do not edit by hand.\n\
        [source.crates-io]\nreplace-with = \"vendored-sources\"\n\n\
        [source.vendored-sources]\ndirectory = \"{directory}\"\n"
    )
}
/// Unix seconds used for a freshly published manifest.
pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}
