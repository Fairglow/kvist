//! Live native Cargo cache reuse. Directory vendoring is a separate explicit export.
//!
//! Catalogue observations are not content or provenance attestations. Receipts
//! bind only the lock, and never choose cache roots or freeze host cache contents.

use super::{ProvisionOptions, host_cargo};
use crate::{
    Error, Result,
    fs::{Dir, Identity, O_NOFOLLOW, absolute, io_error},
    integrity::{LockedGraph, Origin, Package, control, digest_shape},
    unavailable,
    vendoring::VendoringReport,
};
use sav::CancellationToken;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

mod admission;
mod failure;
mod native;
pub use admission::{
    SourceAdmission, SourceView, admit, admit_with, normalize_metadata_paths, sources,
};
pub use failure::classify_metadata_failure;

const RECEIPT: &str = "cargo-cache.json";
const OPERATION: &str = "cargo-cache-operation.lock";
const CRATES_IO: &str = "https://github.com/rust-lang/crates.io-index";

/// Selected native data roots and a cheap locked-package catalogue observation.
#[derive(Debug, Clone)]
pub struct CacheView {
    /// Explicit operator-selected Cargo administrative home, not a mount grant.
    pub cargo_home: PathBuf,
    /// Available native registry data root only.
    pub registry: Option<PathBuf>,
    /// Available native Git data root only.
    pub git: Option<PathBuf>,
    /// Package presence; not source-file integrity or an immutable cache promise.
    pub report: VendoringReport,
}

/// Choose operator authority without consulting project configuration or receipts.
pub fn home() -> Result<PathBuf> {
    let path = if let Some(home) = std::env::var_os("CARGO_HOME") {
        PathBuf::from(home)
    } else {
        PathBuf::from(std::env::var_os("HOME").ok_or_else(|| {
            unavailable(
                Path::new("HOME"),
                "set absolute CARGO_HOME or HOME for native Cargo cache",
            )
        })?)
        .join(".cargo")
    };
    selected(&path)
}

fn selected(home: &Path) -> Result<PathBuf> {
    if !home.is_absolute() || home == Path::new("/") {
        return Err(unavailable(
            home,
            "Cargo home must be an explicit absolute non-root path",
        ));
    }
    absolute(home)
}

fn optional(directory: &Dir, name: &str) -> Result<Option<Dir>> {
    if directory.exists(name)? {
        directory.child(name).map(Some)
    } else {
        Ok(None)
    }
}

fn ensure_home(home: &Path) -> Result<Dir> {
    let home = selected(home)?;
    let parent = home
        .parent()
        .ok_or_else(|| unavailable(&home, "Cargo home has no parent"))?;
    let name = home
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| unavailable(&home, "unsafe Cargo home"))?;
    Dir::open(parent)?.ensure_child(name)
}

fn roots(graph: &LockedGraph, home: &Dir) -> Result<CacheView> {
    let registry = optional(home, "registry")?.map(|d| d.display);
    let git = optional(home, "git")?.map(|d| d.display);
    Ok(CacheView {
        cargo_home: home.display.clone(),
        registry,
        git,
        report: graph.report(&home.display),
    })
}

fn unsupported(path: &Path, reason: &'static str) -> Error {
    Error::UnsupportedCacheSource {
        path: path.to_owned(),
        reason,
    }
}

fn canonical(package: &Package) -> bool {
    matches!(&package.origin, Origin::Registry { url }
        if matches!(url.as_str(), CRATES_IO | "sparse+https://index.crates.io/" | "https://index.crates.io/"))
}

fn inspect(graph: &LockedGraph, home: &Dir, token: &CancellationToken) -> Result<CacheView> {
    control(token)?;
    let mut view = roots(graph, home)?;
    if !graph.external() {
        home.still_at_path()?;
        return Ok(view);
    }
    if graph
        .packages
        .iter()
        .any(|p| !matches!(p.origin, Origin::Local) && !canonical(p))
    {
        return Err(unsupported(
            &home.display,
            "alternate registry/Git identities require native Cargo offline validation; name/version alone is insufficient",
        ));
    }
    let registry = optional(home, "registry")?;
    let mut indexes = Vec::new();
    if let Some(registry) = &registry
        && let Some(index) = optional(registry, "index")?
    {
        for name in index.entries(graph.limits.max_sources)? {
            control(token)?;
            // Native namespace suffixes are opaque. Never compute or guess hashes.
            if name.strip_prefix("index.crates.io-").is_some_and(|suffix| {
                suffix.len() == 16 && suffix.bytes().all(|c| c.is_ascii_hexdigit())
            }) {
                let directory = index.child(&name)?;
                if !directory.exists("config.json")? {
                    continue;
                }
                let (bytes, _) = directory.read("config.json", graph.limits.max_metadata_bytes)?;
                #[derive(Deserialize)]
                struct IndexConfig {
                    dl: String,
                    api: Option<String>,
                }
                let config: IndexConfig = serde_json::from_slice(&bytes).map_err(|_| {
                    unavailable(&directory.display, "malformed native registry config")
                })?;
                if config.dl != "https://static.crates.io/crates"
                    || config
                        .api
                        .as_deref()
                        .is_some_and(|api| api != "https://crates.io")
                {
                    return Err(unsupported(
                        &directory.display,
                        "registry namespace/config does not identify canonical crates.io",
                    ));
                }
                indexes.push((name, directory));
            }
        }
        if indexes.is_empty()
            && !index.entries(graph.limits.max_sources)?.is_empty()
            && graph.external()
        {
            return Err(unsupported(
                &index.display,
                "noncanonical or Git index requires native Cargo offline validation",
            ));
        }
    }
    for package in &graph.packages {
        control(token)?;
        if matches!(package.origin, Origin::Local) {
            continue;
        }
        let mut present = false;
        if let Some(registry) = &registry {
            for (name, index) in &indexes {
                if catalogue_package(graph, registry, index, name, package)? {
                    present = true;
                    break;
                }
            }
        }
        if !present {
            view.report
                .missing_dependencies
                .push(format!("{}-{}", package.name, package.version));
        }
    }
    home.still_at_path()?;
    Ok(view)
}

fn index_path(name: &str) -> PathBuf {
    let name = name.to_ascii_lowercase();
    match name.len() {
        1 => PathBuf::from("1").join(name),
        2 => PathBuf::from("2").join(name),
        3 => PathBuf::from("3").join(&name[..1]).join(name),
        _ => PathBuf::from(&name[..2]).join(&name[2..4]).join(name),
    }
}

fn catalogue_package(
    graph: &LockedGraph,
    registry: &Dir,
    index: &Dir,
    namespace: &str,
    package: &Package,
) -> Result<bool> {
    let Some(mut cache) = optional(index, ".cache")? else {
        return Ok(false);
    };
    let path = index_path(&package.name);
    let Some(parent) = path.parent() else {
        return Ok(false);
    };
    for name in parent.components() {
        let Some(child) = optional(&cache, &name.as_os_str().to_string_lossy())? else {
            return Ok(false);
        };
        cache = child;
    }
    let name = package.name.to_ascii_lowercase();
    if !cache.exists(&name)? {
        return Ok(false);
    }
    let (bytes, _) = cache.read(&name, graph.limits.max_metadata_bytes)?;
    if bytes.get(..5) != Some(b"\x03\x02\x00\x00\x00") {
        return Err(unsupported(
            &cache.display,
            "unsupported native sparse index cache version",
        ));
    }
    let mut fields = bytes[5..].split(|b| *b == 0);
    let _etag = fields.next();
    let mut matched = false;
    while let Some(version) = fields.next() {
        if version.is_empty() {
            break;
        }
        let record = fields
            .next()
            .ok_or_else(|| unavailable(&cache.display, "truncated native sparse index record"))?;
        if version != package.version.as_bytes() {
            continue;
        }
        #[derive(Deserialize)]
        struct Record {
            name: String,
            vers: String,
            cksum: String,
        }
        let record: Record = serde_json::from_slice(record).map_err(|_| {
            unavailable(
                &cache.display,
                "malformed native sparse index package record",
            )
        })?;
        if record.name != package.name
            || record.vers != package.version
            || Some(record.cksum.as_str()) != package.checksum.as_deref()
        {
            return Err(unavailable(
                &cache.display,
                "native registry index identity/checksum differs from Cargo.lock",
            ));
        }
        matched = true;
        break;
    }
    if !matched {
        return Ok(false);
    }
    let Some(archives) = optional(registry, "cache")? else {
        return Ok(false);
    };
    let Some(archives) = optional(&archives, namespace)? else {
        return Ok(false);
    };
    let archive = format!("{}-{}.crate", package.name, package.version);
    if !archives.exists(&archive)? {
        return Ok(false);
    }
    archives.regular(&archive, u64::MAX)?;
    let Some(source) = optional(registry, "src")? else {
        return Ok(false);
    };
    let Some(source) = optional(&source, namespace)? else {
        return Ok(false);
    };
    let Some(source) = optional(&source, &format!("{}-{}", package.name, package.version))? else {
        return Ok(false);
    };
    if !source.exists(".cargo-ok")? || !source.exists("Cargo.toml")? {
        return Ok(false);
    }
    let (marker, _) = source.read(".cargo-ok", 1024)?;
    #[derive(Deserialize)]
    struct Marker {
        v: u32,
    }
    if serde_json::from_slice::<Marker>(&marker)
        .ok()
        .is_none_or(|m| m.v != 1)
    {
        return Err(unsupported(
            &source.display,
            "unsupported native Cargo extraction completion marker",
        ));
    }
    let (manifest, _) = source.read("Cargo.toml", graph.limits.max_metadata_bytes)?;
    let manifest: toml::Value = toml::from_slice(&manifest)
        .map_err(|_| unavailable(&source.display, "malformed native package Cargo.toml"))?;
    if manifest
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
        != Some(package.name.as_str())
        || manifest
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(toml::Value::as_str)
            != Some(package.version.as_str())
    {
        return Err(unavailable(
            &source.display,
            "native package manifest identity differs from Cargo.lock",
        ));
    }
    Ok(true)
}

/// Process-, write- and network-free catalogue inspection, not a cache pin.
pub fn view(project: &Path, home: &Path, cancellation: &CancellationToken) -> Result<CacheView> {
    control(cancellation)?;
    let project = Dir::open(project)?;
    let graph = LockedGraph::read_native(&project, cancellation)?;
    let home = selected(home)?;
    let parent = Dir::open(
        home.parent()
            .ok_or_else(|| unavailable(&home, "Cargo home has no parent"))?,
    )?;
    let name = home
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| unavailable(&home, "unsafe Cargo home"))?;
    let view = if parent.exists(name)? {
        inspect(&graph, &parent.child(name)?, cancellation)?
    } else {
        let mut report = graph.report(&home);
        report.missing_dependencies = graph
            .packages
            .iter()
            .filter(|p| !matches!(p.origin, Origin::Local))
            .map(|p| format!("{}-{}", p.name, p.version))
            .collect();
        CacheView {
            cargo_home: home,
            registry: None,
            git: None,
            report,
        }
    };
    parent.still_at_path()?;
    graph.unchanged(&project, cancellation)?;
    Ok(view)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    lockfile_digest: String,
}

fn receipt_matches(state: &Dir, graph: &LockedGraph) -> Result<bool> {
    if !state.exists(RECEIPT)? {
        return Ok(false);
    }
    let (bytes, _) = state.read(RECEIPT, graph.limits.max_metadata_bytes)?;
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|_| unavailable(&state.display, "malformed native Cargo cache receipt"))?;
    if receipt.schema_version != 1
        || !receipt
            .lockfile_digest
            .strip_prefix("sha256:")
            .is_some_and(digest_shape)
    {
        return Err(unavailable(
            &state.display,
            "invalid native Cargo cache receipt",
        ));
    }
    Ok(receipt.lockfile_digest == graph.digest)
}

#[derive(Debug)]
struct HeldLock {
    directory: Dir,
    file: File,
    name: &'static str,
}

impl HeldLock {
    fn acquire(directory: &Dir, name: &'static str) -> Result<Self> {
        let path = directory.display.join(name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(directory.path().join(name))
            .map_err(|e| io_error("open native advisory lock", &path, e))?;
        let metadata = file
            .metadata()
            .map_err(|e| io_error("inspect native advisory lock", &path, e))?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > 4096 {
            return Err(unavailable(
                &path,
                "native advisory lock must be a bounded regular non-link file",
            ));
        }

        file.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => Error::OperationLocked { path: path.clone() },
            std::fs::TryLockError::Error(e) => io_error("acquire native advisory lock", &path, e),
        })?;
        let held = Self {
            directory: Dir::open(&directory.display)?,
            file,
            name,
        };
        held.intact()?;
        Ok(held)
    }
    fn intact(&self) -> Result<()> {
        self.directory.still_at_path()?;
        let current = self.directory.regular(self.name, 4096)?;
        let path = self.directory.display.join(self.name);
        if Identity::of(&current, &path)? != Identity::of(&self.file, &path)? {
            return Err(unavailable(&path, "native advisory lock identity changed"));
        }
        Ok(())
    }
}

impl Drop for HeldLock {
    fn drop(&mut self) {
        // Explicit unlock also releases fork-inherited descriptors before exec.
        let _ = self.file.unlock();
    }
}

/// Cargo native mutation/download locks for an isolated administrative home.
///
/// Do not hold this guard while spawning Cargo with the same home: it would
/// wait for these locks. This coordinates cooperating Cargo, not arbitrary edits.
#[derive(Debug)]
pub struct AcquisitionLease {
    download: HeldLock,
    mutate: HeldLock,
}

impl AcquisitionLease {
    /// Selected operator home whose native locks are held.
    pub fn cargo_home(&self) -> &Path {
        &self.mutate.directory.display
    }

    /// Recheck the anchored home and retained native lock-file identities.
    pub fn check(&self, cancellation: &CancellationToken) -> Result<()> {
        control(cancellation)?;
        self.mutate.intact()?;
        self.download.intact()
    }
}

/// Acquire native exclusive locks in Cargo's mutation-before-download order.
/// Busy/unsupported locking fails; no best-effort frozen-cache claim is made.
pub fn acquisition_lease(
    home: &Path,
    cancellation: &CancellationToken,
) -> Result<AcquisitionLease> {
    control(cancellation)?;
    let home = Dir::open(&selected(home)?)?;
    let mutate = HeldLock::acquire(&home, ".package-cache-mutate")?;
    control(cancellation)?;
    let download = HeldLock::acquire(&home, ".package-cache")?;
    Ok(AcquisitionLease { mutate, download })
}

fn complete(
    project: &Dir,
    graph: &LockedGraph,
    home: &Dir,
    token: &CancellationToken,
    validate: &mut impl FnMut(&Dir, &LockedGraph, &Dir) -> Result<CacheView>,
) -> Result<CacheView> {
    if let Some(missing) = native::directory_missing(graph, home)? {
        return Ok(missing);
    }
    match inspect(graph, home, token) {
        Err(Error::UnsupportedCacheSource { .. }) => validate(project, graph, home),
        result => result,
    }
}

fn provision_using(
    project: &Path,
    home: &Path,
    cancellation: &CancellationToken,
    force: bool,
    acquire: impl FnOnce(&Path) -> Result<()>,
    mut validate: impl FnMut(&Dir, &LockedGraph, &Dir) -> Result<CacheView>,
) -> Result<CacheView> {
    control(cancellation)?;
    let project = Dir::open(project)?;
    let graph = LockedGraph::read_native(&project, cancellation)?;
    let home = ensure_home(home)?;
    // These roots are validated even when not currently required by the lock.
    roots(&graph, &home)?;
    let state = project.ensure_child(".kvist")?;
    let operation = HeldLock::acquire(&state, OPERATION)?;
    let matching = receipt_matches(&state, &graph)?;
    let mut view = complete(&project, &graph, &home, cancellation, &mut validate)?;
    if !view.report.ready() {
        home.ensure_child("registry")?;
        home.ensure_child("git")?;
        operation.intact()?;
        graph.unchanged(&project, cancellation)?;
        acquire(&home.display)?;
        control(cancellation)?;
        operation.intact()?;
        home.still_at_path()?;
        graph.unchanged(&project, cancellation)?;
        view = complete(&project, &graph, &home, cancellation, &mut validate)?;
        if !view.report.ready() {
            return Err(Error::VendoringIncomplete {
                path: project.display.to_string_lossy().into_owned(),
                count: view.report.missing_dependencies.len(),
                missing: view.report.missing_human_readable(),
            });
        }
    }
    graph.unchanged(&project, cancellation)?;
    operation.intact()?;
    if !matching || force {
        let bytes = serde_json::to_vec(&Receipt {
            schema_version: 1,
            lockfile_digest: graph.digest.clone(),
        })
        .map_err(|_| unavailable(&state.display, "cannot serialize native cache receipt"))?;
        state.atomic(RECEIPT, &bytes)?;
    }
    graph.unchanged(&project, cancellation)?;
    operation.intact()?;
    control(cancellation)?;
    Ok(view)
}

/// Reuse complete live material, or invoke a locked native acquisition once.
/// Failed callbacks leave acquired material intact; legacy export state is untouched.
pub fn provision_with(
    project: &Path,
    home: &Path,
    cancellation: &CancellationToken,
    force: bool,
    acquire: impl FnOnce(&Path) -> Result<()>,
) -> Result<CacheView> {
    let options = ProvisionOptions {
        cargo: PathBuf::from("/usr/bin/cargo"),
        offline: true,
        vendored_dir: None,
        timeout: Duration::from_secs(crate::VendorLimits::default().timeout_secs),
        force,
    };
    provision_using(
        project,
        home,
        cancellation,
        force,
        acquire,
        |project, graph, home| native::validate(project, graph, home, &options, cancellation),
    )
}

/// Native preparation with caller-authorized private-admin metadata validation.
///
/// The validator runs bounded literal offline/locked Cargo metadata and returns
/// stdout with fixed native sandbox endpoint paths. Only an explicitly classified
/// missing-source error permits acquisition; no host process/config fallback runs.
pub fn provision_with_validation(
    project: &Path,
    home: &Path,
    cancellation: &CancellationToken,
    force: bool,
    acquire: impl FnOnce(&Path) -> Result<()>,
    mut validate: impl FnMut(&Path) -> Result<Vec<u8>>,
) -> Result<CacheView> {
    provision_using(
        project,
        home,
        cancellation,
        force,
        acquire,
        |project, graph, home| {
            home.ensure_child("registry")?;
            home.ensure_child("git")?;
            graph.unchanged(project, cancellation)?;
            match validate(&home.display) {
                Ok(bytes) => {
                    native::validate_bytes(project, graph, home, &bytes, true, cancellation)
                }
                Err(Error::VendoringIncomplete { .. }) => native::missing(graph, home),
                Err(error) => Err(error),
            }
        },
    )
}

/// One bounded literal `cargo fetch --locked`, offline only when selected.
/// Cargo itself coordinates its native writer/GC locks; no private lock pins it.
pub fn populate(
    project: &Path,
    home: &Path,
    options: &ProvisionOptions,
    cancellation: &CancellationToken,
) -> Result<()> {
    control(cancellation)?;
    let project = Dir::open(project)?;
    let graph = LockedGraph::read_native(&project, cancellation)?;
    let home = ensure_home(home)?;
    home.ensure_child("registry")?;
    home.ensure_child("git")?;
    let mut argv = vec!["fetch".into(), "--locked".into()];
    if options.offline {
        argv.push("--offline".into());
    }
    native::run(
        &project,
        &graph,
        &home,
        options,
        argv,
        "acquisition",
        cancellation,
    )?;
    home.still_at_path()?;
    graph.unchanged(&project, cancellation)
}

/// Host-authorized native preparation. Explicit export destinations use
/// [`super::provision`] instead; this function never runs Cargo vendor.
pub fn provision(
    project: &Path,
    options: ProvisionOptions,
    cancellation: &CancellationToken,
) -> Result<CacheView> {
    if options.vendored_dir.is_some() {
        return Err(unavailable(
            project,
            "native cache preparation has no export destination; use rust::provision for explicit directory export",
        ));
    }
    let project = absolute(project)?;
    host_cargo(&project, &options)?;
    let home = home()?;
    provision_using(
        &project,
        &home,
        cancellation,
        options.force,
        |home| populate(&project, home, &options, cancellation),
        |project, graph, home| native::validate(project, graph, home, &options, cancellation),
    )
}
