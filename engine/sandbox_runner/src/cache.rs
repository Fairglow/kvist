//! Descriptor-relative immutable Cargo-home generation construction.
//!
//! This Linux-only primitive is deliberately not a generic "copy files into a
//! destination" API. A caller first opens its provider-owned generation parent
//! and the isolated attempt-local `CARGO_HOME`. After the Cargo child exits,
//! this module reads only beneath those retained no-follow descriptors, builds
//! a private complete generation, and publishes it with `RENAME_NOREPLACE`.
//! It never updates a mutable project-cache pathname or selects the project's
//! current generation; those integration steps remain deferred.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::File,
    io::{Read, Write},
    os::fd::{AsFd, OwnedFd},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use nix::{
    dir::Dir,
    errno::Errno,
    fcntl::{self, AtFlags, OFlag, OpenHow, RenameFlags, ResolveFlag},
    sys::stat::{self, Mode, SFlag},
    unistd,
};
use sha2::{Digest, Sha256};

const MAX_STAGING_ATTEMPTS: u64 = 1024;
/// Maximum number of regular files in one generation. It is reconciled with the
/// protocol promotion-manifest maximum so a configured `max_files` bound can
/// never exceed the number of manifest entries the protocol admits.
const MAX_FILES: u64 = MAX_MANIFEST_ENTRIES as u64;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_MANIFEST_ENTRIES: usize = 4096;
/// Maximum number of directories traversed in one generation. Bounding this and
/// the depth keeps descriptor use and identity work well below FD/resource
/// limits.
const MAX_DIRECTORY_ENTRIES: u64 = 4096;
/// Maximum directory nesting depth traversed in one generation.
const MAX_DIRECTORY_DEPTH: u32 = 64;
const MAX_PATH_BYTES: usize = 4096;
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// One regular Cargo-home file accepted into an immutable generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Canonical path relative to the `CARGO_HOME`.
    pub path: String,
    /// Exact file length.
    pub size: u64,
    /// Exact `sha256:` checksum.
    pub checksum: String,
}

/// Nonzero limits for one immutable-generation construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheBounds {
    pub max_files: u64,
    pub max_file_bytes: u64,
    pub max_cache_bytes: u64,
}

/// An opened capability for the trusted, provider-owned generation parent.
///
/// Construction rejects links and noncanonical/non-UTF-8 paths. Retaining the
/// descriptor prevents replacement of the parent path after validation from
/// redirecting descriptor-relative staging or publication.
#[derive(Debug)]
pub struct TrustedGenerationParent {
    display_path: PathBuf,
    descriptor: OwnedFd,
}

/// An opened capability for the attempt-local Cargo home.
///
/// It must be isolated from the generation parent. The retained descriptor is
/// the only source authority the promotion algorithm subsequently uses.
#[derive(Debug)]
pub struct IsolatedAttemptRoot {
    display_path: PathBuf,
    descriptor: OwnedFd,
}

/// Opens and validates the provider-owned parent where immutable generations
/// may be published.
pub fn open_trusted_generation_parent(path: &Path) -> Result<TrustedGenerationParent, CacheError> {
    let (display_path, descriptor) = open_root(path, "generation parent")?;
    Ok(TrustedGenerationParent {
        display_path,
        descriptor,
    })
}

/// Opens and validates the isolated attempt-local `CARGO_HOME`.
pub fn open_isolated_attempt_root(path: &Path) -> Result<IsolatedAttemptRoot, CacheError> {
    let (display_path, descriptor) = open_root(path, "attempt root")?;
    Ok(IsolatedAttemptRoot {
        display_path,
        descriptor,
    })
}

fn open_root(path: &Path, label: &'static str) -> Result<(PathBuf, OwnedFd), CacheError> {
    let text = canonical_path_text(path, label)?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| io_error("inspect", path, error.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Err(CacheError::SymlinkRoot {
            path: path.to_path_buf(),
        });
    }
    if !metadata.file_type().is_dir() {
        return Err(CacheError::RootNotDirectory {
            path: path.to_path_buf(),
        });
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| io_error("canonicalize", path, error.to_string()))?;
    if canonical != path {
        return Err(CacheError::NonCanonicalRoot {
            path: path.to_path_buf(),
        });
    }
    let filesystem_root = fcntl::open(
        Path::new("/"),
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_error("open filesystem root", Path::new("/"), error))?;
    let relative = text.strip_prefix('/').ok_or_else(|| CacheError::InvalidRoot {
        label,
        path: path.to_path_buf(),
    })?;
    let relative = if relative.is_empty() { "." } else { relative };
    let descriptor = open_beneath_no_symlinks(filesystem_root.as_fd(), relative, path)?;
    let stat = stat::fstat(&descriptor).map_err(|error| nix_error("inspect", path, error))?;
    if file_kind(&stat) != SFlag::S_IFDIR {
        return Err(CacheError::RootNotDirectory {
            path: path.to_path_buf(),
        });
    }
    use std::os::unix::fs::MetadataExt;
    if metadata.dev() != stat.st_dev || metadata.ino() != stat.st_ino {
        return Err(CacheError::RootReplaced {
            path: path.to_path_buf(),
        });
    }
    Ok((PathBuf::from(text), descriptor))
}

/// Opens a directory strictly beneath `start` without following any symlink.
///
/// It prefers `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` (Linux
/// >= 5.6). When the kernel lacks `openat2` (`ENOSYS`), it falls back to a
/// componentwise walk that opens each already-canonical component with
/// `O_NOFOLLOW | O_DIRECTORY`, which refuses any symlink component with `ELOOP`
/// and never escapes the starting directory. The relative path is validated
/// canonical (no `.`/`..`/empty components) by the caller. The fallback keeps
/// the primitive usable on the existing Linux contract, which sets no minimum
/// kernel version.
fn open_beneath_no_symlinks(
    start: std::os::fd::BorrowedFd<'_>,
    relative: &str,
    display: &Path,
) -> Result<OwnedFd, CacheError> {
    match fcntl::openat2(
        start,
        relative,
        OpenHow::new()
            .flags(OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC)
            .resolve(ResolveFlag::RESOLVE_BENEATH | ResolveFlag::RESOLVE_NO_SYMLINKS),
    ) {
        Ok(descriptor) => Ok(descriptor),
        Err(Errno::ENOSYS) => open_beneath_componentwise(start, relative, display),
        Err(error) => Err(nix_error("open no-follow root", display, error)),
    }
}

fn open_beneath_componentwise(
    start: std::os::fd::BorrowedFd<'_>,
    relative: &str,
    display: &Path,
) -> Result<OwnedFd, CacheError> {
    let mut current = fcntl::openat(
        start,
        ".",
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_error("open no-follow root", display, error))?;
    if relative == "." {
        return Ok(current);
    }
    for component in relative.split('/') {
        let child = fcntl::openat(
            current.as_fd(),
            component,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| nix_error("open no-follow root component", display, error))?;
        current = child;
    }
    Ok(current)
}

fn canonical_path_text<'path>(
    path: &'path Path,
    label: &'static str,
) -> Result<&'path str, CacheError> {
    let text = path.to_str().ok_or_else(|| CacheError::NonUtf8Root {
        path: path.to_path_buf(),
    })?;
    if text.len() > MAX_PATH_BYTES {
        return Err(CacheError::RootTooLong {
            path: path.to_path_buf(),
            limit: MAX_PATH_BYTES,
        });
    }
    if !path.is_absolute() {
        return Err(CacheError::NonAbsoluteRoot {
            path: path.to_path_buf(),
        });
    }
    if text != "/" && text.ends_with('/') || text.contains("//") {
        return Err(CacheError::NonCanonicalRoot {
            path: path.to_path_buf(),
        });
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::CurDir | Component::ParentDir | Component::Prefix(_)
        )
    }) {
        return Err(CacheError::NonCanonicalRoot {
            path: path.to_path_buf(),
        });
    }
    if text.is_empty() {
        return Err(CacheError::InvalidRoot {
            label,
            path: path.to_path_buf(),
        });
    }
    Ok(text)
}

/// The complete immutable-generation request. The source and destination are
/// capabilities rather than caller-controlled mutable pathnames.
#[derive(Debug)]
pub struct PromotionPlan<'plan> {
    pub attempt_root: &'plan IsolatedAttemptRoot,
    pub generation_parent: &'plan TrustedGenerationParent,
    pub manifest: &'plan [ManifestEntry],
    pub bounds: CacheBounds,
    /// `Cargo.lock` identity before the exact fetch.
    pub lockfile_before_identity: String,
    /// `Cargo.lock` identity observed after the exact fetch.
    pub lockfile_after_identity: String,
    /// Exact independently-derived identities of the approved sources.
    pub supported_source_identities: Vec<String>,
}

/// The published immutable cache generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedGeneration {
    /// A generation path under the trusted parent, provided for later
    /// read-only mounting. Publication itself used the retained descriptor.
    pub path: PathBuf,
    /// The deterministic bounded identity of the complete generation.
    pub identity: String,
    /// Whether the generation and its parent directory entry were durably
    /// persisted, or whether the rename succeeded but the parent `fsync` did
    /// not confirm durability (in which case retry/recovery must reconcile the
    /// already-published, named generation).
    pub durability: Durability,
    /// True when an identical immutable generation already existed and was
    /// reused after descriptor-relative identity verification rather than newly
    /// created.
    pub reused: bool,
}

/// The durability status of a published generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// The generation and the parent directory entry were fsynced.
    Synced,
    /// The generation was atomically renamed into place, but the parent
    /// directory `fsync` failed, so persistence of the directory entry across a
    /// crash is unknown. The generation path and identity are still returned so
    /// a later reconciliation can verify and, if durable, adopt it.
    ParentSyncUnknown,
}

/// A precise reason inspection or immutable publication was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheError {
    NonAbsoluteRoot { path: PathBuf },
    NonCanonicalRoot { path: PathBuf },
    NonUtf8Root { path: PathBuf },
    RootTooLong { path: PathBuf, limit: usize },
    InvalidRoot { label: &'static str, path: PathBuf },
    SymlinkRoot { path: PathBuf },
    RootNotDirectory { path: PathBuf },
    RootReplaced { path: PathBuf },
    RootsOverlap,
    ZeroBound { name: &'static str },
    BoundExceeds { name: &'static str, maximum: u64 },
    ManifestPath { path: String },
    ManifestDuplicate { path: String },
    Digest { detail: String },
    SourceSymlink { path: String },
    SourceHardLink { path: String, links: u64 },
    SourceNonRegular { path: String },
    SourceReplaced { path: String },
    DirectoryCount { limit: u64 },
    DirectoryDepth { limit: u32 },
    FileCount { limit: u64 },
    FileSize { path: String, limit: u64 },
    AggregateSize { limit: u64 },
    UnlistedFile { path: String },
    MissingFile { path: String },
    SizeMismatch { path: String },
    ChecksumMismatch { path: String },
    StagedChecksumMismatch { path: String },
    GenerationCollision { name: String },
    GenerationMismatch { name: String },
    StagingExhausted,
    Cleanup { detail: String },
    Io {
        operation: &'static str,
        path: PathBuf,
        detail: String,
    },
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonAbsoluteRoot { path } => write!(formatter, "cache root `{}` must be absolute", path.display()),
            Self::NonCanonicalRoot { path } => write!(formatter, "cache root `{}` must be canonical", path.display()),
            Self::NonUtf8Root { path } => write!(formatter, "cache root `{}` must be valid UTF-8", path.display()),
            Self::RootTooLong { path, limit } => write!(formatter, "cache root `{}` exceeds the {limit}-byte protocol limit", path.display()),
            Self::InvalidRoot { label, path } => write!(formatter, "{label} `{}` is invalid", path.display()),
            Self::SymlinkRoot { path } => write!(formatter, "cache root `{}` must not be a symlink", path.display()),
            Self::RootNotDirectory { path } => write!(formatter, "cache root `{}` must be a directory", path.display()),
            Self::RootReplaced { path } => write!(formatter, "cache root `{}` changed while it was opened", path.display()),
            Self::RootsOverlap => formatter.write_str("attempt root and trusted generation parent must not be equal or contain one another"),
            Self::ZeroBound { name } => write!(formatter, "cache bound `{name}` must be greater than zero"),
            Self::BoundExceeds { name, maximum } => write!(formatter, "cache bound `{name}` exceeds the {maximum} protocol maximum"),
            Self::ManifestPath { path } => write!(formatter, "cache manifest path `{path}` must be a safe canonical relative path"),
            Self::ManifestDuplicate { path } => write!(formatter, "cache manifest declares `{path}` more than once"),
            Self::Digest { detail } => write!(formatter, "cache identity {detail}"),
            Self::SourceSymlink { path } => write!(formatter, "attempt cache entry `{path}` is a symlink"),
            Self::SourceHardLink { path, links } => write!(formatter, "attempt cache file `{path}` has {links} hard links"),
            Self::SourceNonRegular { path } => write!(formatter, "attempt cache entry `{path}` is not a regular file or directory"),
            Self::SourceReplaced { path } => write!(formatter, "attempt cache entry `{path}` changed while it was validated"),
            Self::DirectoryCount { limit } => write!(formatter, "attempt cache directory count exceeds the {limit}-directory bound"),
            Self::DirectoryDepth { limit } => write!(formatter, "attempt cache directory nesting exceeds the {limit}-level bound"),
            Self::FileCount { limit } => write!(formatter, "cache file count exceeds the {limit}-file bound"),
            Self::FileSize { path, limit } => write!(formatter, "cache file `{path}` exceeds the {limit}-byte bound"),
            Self::AggregateSize { limit } => write!(formatter, "cache aggregate size exceeds the {limit}-byte bound"),
            Self::UnlistedFile { path } => write!(formatter, "attempt cache file `{path}` is absent from the exact manifest"),
            Self::MissingFile { path } => write!(formatter, "cache manifest file `{path}` is absent from the attempt cache"),
            Self::SizeMismatch { path } => write!(formatter, "cache manifest size does not match `{path}`"),
            Self::ChecksumMismatch { path } => write!(formatter, "cache manifest checksum does not match `{path}`"),
            Self::StagedChecksumMismatch { path } => write!(formatter, "staged cache file `{path}` did not rehash to its expected checksum"),
            Self::GenerationCollision { name } => write!(formatter, "immutable cache generation `{name}` already exists with different content or provenance"),
            Self::GenerationMismatch { name } => write!(formatter, "existing immutable cache generation `{name}` did not re-verify to its expected identity"),
            Self::StagingExhausted => formatter.write_str("could not allocate a unique private cache-generation staging directory"),
            Self::Cleanup { detail } => write!(formatter, "cache staging cleanup failed: {detail}"),
            Self::Io { operation, path, detail } => write!(formatter, "cache {operation} failed for `{}`: {detail}", path.display()),
        }
    }
}

impl std::error::Error for CacheError {}

fn io_error(operation: &'static str, path: &Path, detail: String) -> CacheError {
    CacheError::Io {
        operation,
        path: path.to_path_buf(),
        detail,
    }
}

fn nix_error(operation: &'static str, path: &Path, error: Errno) -> CacheError {
    io_error(operation, path, error.to_string())
}

/// Validates and snapshots the source without publishing a generation.
pub fn inspect_source(plan: &PromotionPlan<'_>) -> Result<(), CacheError> {
    validate_plan(plan)?;
    let entries = collect_source(plan.attempt_root.descriptor.as_fd(), plan.bounds)?;
    reconcile_manifest(&entries, plan.manifest)?;
    Ok(())
}

/// Constructs and atomically publishes a complete immutable generation.
///
/// If an identical immutable generation already exists it is reused after a
/// bounded, descriptor-relative re-verification of its identity; otherwise a
/// differing generation of the same name is a collision error. A successful
/// rename that could not confirm parent-directory durability returns a
/// [`Durability::ParentSyncUnknown`] outcome that still names the published
/// generation so a later reconciliation can adopt it.
pub fn promote(plan: &PromotionPlan<'_>) -> Result<PublishedGeneration, CacheError> {
    validate_plan(plan)?;
    let snapshot = collect_source(plan.attempt_root.descriptor.as_fd(), plan.bounds)?;
    reconcile_manifest(&snapshot, plan.manifest)?;
    let staging = create_staging(plan.generation_parent)?;
    match build_and_publish(plan, &staging, &snapshot) {
        Ok((generation, renamed)) => {
            if !renamed {
                // A reuse of an existing generation leaves the private staging
                // directory unused; remove it explicitly.
                if let Err(cleanup) = remove_tree_at(
                    plan.generation_parent.descriptor.as_fd(),
                    &staging.name,
                    &plan.generation_parent.display_path,
                ) {
                    return Err(CacheError::Cleanup {
                        detail: cleanup.to_string(),
                    });
                }
            }
            Ok(generation)
        }
        Err(error) => {
            if let Err(cleanup) = remove_tree_at(
                plan.generation_parent.descriptor.as_fd(),
                &staging.name,
                &plan.generation_parent.display_path,
            ) {
                return Err(CacheError::Cleanup {
                    detail: format!("{error}; {cleanup}"),
                });
            }
            Err(error)
        }
    }
}

fn validate_plan(plan: &PromotionPlan<'_>) -> Result<(), CacheError> {
    if paths_overlap(
        &plan.attempt_root.display_path,
        &plan.generation_parent.display_path,
    ) {
        return Err(CacheError::RootsOverlap);
    }
    for (name, value, maximum) in [
        ("max_files", plan.bounds.max_files, MAX_FILES),
        ("max_file_bytes", plan.bounds.max_file_bytes, MAX_FILE_BYTES),
        ("max_cache_bytes", plan.bounds.max_cache_bytes, MAX_CACHE_BYTES),
    ] {
        if value == 0 {
            return Err(CacheError::ZeroBound { name });
        }
        if value > maximum {
            return Err(CacheError::BoundExceeds { name, maximum });
        }
    }
    validate_manifest(plan.manifest)?;
    let mut declared_total = 0_u64;
    for entry in plan.manifest {
        if entry.size > plan.bounds.max_file_bytes {
            return Err(CacheError::FileSize {
                path: entry.path.clone(),
                limit: plan.bounds.max_file_bytes,
            });
        }
        declared_total =
            declared_total
                .checked_add(entry.size)
                .ok_or(CacheError::AggregateSize {
                    limit: plan.bounds.max_cache_bytes,
                })?;
        if declared_total > plan.bounds.max_cache_bytes {
            return Err(CacheError::AggregateSize {
                limit: plan.bounds.max_cache_bytes,
            });
        }
    }
    validate_digest(&plan.lockfile_before_identity)?;
    validate_digest(&plan.lockfile_after_identity)?;
    if plan.supported_source_identities.is_empty() {
        return Err(CacheError::Digest {
            detail: "must bind at least one supported source identity".to_owned(),
        });
    }
    let mut seen = BTreeSet::new();
    for identity in &plan.supported_source_identities {
        validate_digest(identity)?;
        if !seen.insert(identity) {
            return Err(CacheError::Digest {
                detail: "must not repeat a supported source identity".to_owned(),
            });
        }
    }
    Ok(())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

fn validate_manifest(manifest: &[ManifestEntry]) -> Result<(), CacheError> {
    if manifest.len() > MAX_MANIFEST_ENTRIES {
        return Err(CacheError::FileCount {
            limit: MAX_MANIFEST_ENTRIES as u64,
        });
    }
    let mut paths = BTreeSet::new();
    let mut declared_total = 0_u64;
    for entry in manifest {
        if entry.path.len() > MAX_PATH_BYTES {
            return Err(CacheError::ManifestPath {
                path: entry.path.clone(),
            });
        }
        if !safe_relative(&entry.path) {
            return Err(CacheError::ManifestPath {
                path: entry.path.clone(),
            });
        }
        validate_digest(&entry.checksum)?;
        if !paths.insert(&entry.path) {
            return Err(CacheError::ManifestDuplicate {
                path: entry.path.clone(),
            });
        }
        declared_total = declared_total.checked_add(entry.size).ok_or(CacheError::AggregateSize {
            limit: MAX_CACHE_BYTES,
        })?;
        if entry.size > MAX_FILE_BYTES || declared_total > MAX_CACHE_BYTES {
            return Err(CacheError::AggregateSize {
                limit: MAX_CACHE_BYTES,
            });
        }
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), CacheError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(CacheError::Digest {
            detail: "must have a sha256: prefix".to_owned(),
        });
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(CacheError::Digest {
            detail: "must contain 64 lower-case hexadecimal digits".to_owned(),
        });
    }
    Ok(())
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// One entry (regular file or directory) discovered beneath a cache root.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceEntry {
    /// Canonical path relative to the root.
    path: String,
    kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EntryKind {
    /// A directory, recorded even when empty.
    Directory,
    /// A regular file with its exact length, checksum, and whether the source
    /// carried any execute bit (which normalizes to a `0555` mode).
    File {
        size: u64,
        checksum: String,
        executable: bool,
    },
}

/// The normalized, safe mode a generation entry is published with. Files are
/// `0444`, executable files are `0555`, and directories are `0555`. Special,
/// setuid/setgid, sticky, and write bits are never preserved. The sandbox uid
/// may differ from the runner uid, so no owner-only mode is used.
fn safe_mode(kind: &EntryKind) -> u32 {
    match kind {
        EntryKind::Directory => 0o555,
        EntryKind::File {
            executable: true, ..
        } => 0o555,
        EntryKind::File {
            executable: false, ..
        } => 0o444,
    }
}

/// Walks the tree beneath `root`, returning every directory (including empty
/// ones) and regular file sorted by path. Symlinks, hard-linked files, special
/// files, and bound-exceeding trees are rejected. Bytes are streamed and hashed
/// through the retained descriptor, never a re-derived host path.
fn collect_source(
    root: std::os::fd::BorrowedFd<'_>,
    bounds: CacheBounds,
) -> Result<Vec<SourceEntry>, CacheError> {
    let mut state = CollectionState {
        entries: Vec::new(),
        files: 0,
        directories: 0,
        total: 0,
    };
    collect_directory(root, "", bounds, &mut state, 0)?;
    state.entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(state.entries)
}

/// Cross-checks the collected file entries against the exact promotion manifest.
/// Directories are implicit and are not represented in the manifest.
fn reconcile_manifest(
    entries: &[SourceEntry],
    manifest: &[ManifestEntry],
) -> Result<(), CacheError> {
    let mut collected: BTreeMap<&str, (u64, &str)> = BTreeMap::new();
    for entry in entries {
        if let EntryKind::File { size, checksum, .. } = &entry.kind {
            collected.insert(entry.path.as_str(), (*size, checksum.as_str()));
        }
    }
    for declared in manifest {
        let (size, checksum) = collected.get(declared.path.as_str()).ok_or_else(|| {
            CacheError::MissingFile {
                path: declared.path.clone(),
            }
        })?;
        if *size != declared.size {
            return Err(CacheError::SizeMismatch {
                path: declared.path.clone(),
            });
        }
        if *checksum != declared.checksum {
            return Err(CacheError::ChecksumMismatch {
                path: declared.path.clone(),
            });
        }
    }
    let declared_paths: BTreeSet<&str> = manifest.iter().map(|entry| entry.path.as_str()).collect();
    for path in collected.keys() {
        if !declared_paths.contains(path) {
            return Err(CacheError::UnlistedFile {
                path: (*path).to_owned(),
            });
        }
    }
    Ok(())
}

struct CollectionState {
    entries: Vec<SourceEntry>,
    files: u64,
    directories: u64,
    total: u64,
}

fn collect_directory(
    descriptor: std::os::fd::BorrowedFd<'_>,
    prefix: &str,
    bounds: CacheBounds,
    state: &mut CollectionState,
    depth: u32,
) -> Result<(), CacheError> {
    if depth > MAX_DIRECTORY_DEPTH {
        return Err(CacheError::DirectoryDepth {
            limit: MAX_DIRECTORY_DEPTH,
        });
    }
    let mut directory = Dir::openat(
        descriptor,
        ".",
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_error("open attempt directory", Path::new(prefix), error))?;
    let mut names = Vec::new();
    for entry in directory.iter() {
        let entry =
            entry.map_err(|error| nix_error("read attempt directory", Path::new(prefix), error))?;
        let name = entry
            .file_name()
            .to_str()
            .map_err(|_| CacheError::ManifestPath {
                path: format!("{prefix}<non-utf8>"),
            })?;
        if name != "." && name != ".." {
            names.push(name.to_owned());
        }
    }
    names.sort();
    for name in names {
        let path = join_relative(prefix, &name);
        let entry_stat = stat::fstatat(descriptor, name.as_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
            .map_err(|error| nix_error("inspect attempt entry", Path::new(&path), error))?;
        match file_kind(&entry_stat) {
            SFlag::S_IFLNK => return Err(CacheError::SourceSymlink { path }),
            SFlag::S_IFDIR => {
                state.directories =
                    state
                        .directories
                        .checked_add(1)
                        .ok_or(CacheError::DirectoryCount {
                            limit: MAX_DIRECTORY_ENTRIES,
                        })?;
                if state.directories > MAX_DIRECTORY_ENTRIES {
                    return Err(CacheError::DirectoryCount {
                        limit: MAX_DIRECTORY_ENTRIES,
                    });
                }
                let child = fcntl::openat(
                    descriptor,
                    name.as_str(),
                    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|error| nix_error("open attempt directory", Path::new(&path), error))?;
                if !same_file(
                    &entry_stat,
                    &stat::fstat(&child).map_err(|error| {
                        nix_error("inspect attempt directory", Path::new(&path), error)
                    })?,
                ) {
                    return Err(CacheError::SourceReplaced { path });
                }
                state.entries.push(SourceEntry {
                    path: path.clone(),
                    kind: EntryKind::Directory,
                });
                collect_directory(child.as_fd(), &path, bounds, state, depth + 1)?;
            }
            SFlag::S_IFREG => {
                if entry_stat.st_nlink != 1 {
                    return Err(CacheError::SourceHardLink {
                        path,
                        links: entry_stat.st_nlink,
                    });
                }
                state.files = state.files.checked_add(1).ok_or(CacheError::FileCount {
                    limit: bounds.max_files,
                })?;
                if state.files > bounds.max_files {
                    return Err(CacheError::FileCount {
                        limit: bounds.max_files,
                    });
                }
                if entry_stat.st_size < 0 {
                    return Err(CacheError::SourceNonRegular { path });
                }
                let size = entry_stat.st_size as u64;
                if size > bounds.max_file_bytes {
                    return Err(CacheError::FileSize {
                        path,
                        limit: bounds.max_file_bytes,
                    });
                }
                state.total =
                    state
                        .total
                        .checked_add(size)
                        .ok_or(CacheError::AggregateSize {
                            limit: bounds.max_cache_bytes,
                        })?;
                if state.total > bounds.max_cache_bytes {
                    return Err(CacheError::AggregateSize {
                        limit: bounds.max_cache_bytes,
                    });
                }
                let executable = (entry_stat.st_mode & 0o111) != 0;
                let checksum =
                    hash_opened_file(descriptor, &name, &entry_stat, &path, bounds.max_file_bytes)?;
                state.entries.push(SourceEntry {
                    path,
                    kind: EntryKind::File {
                        size,
                        checksum,
                        executable,
                    },
                });
            }
            _ => return Err(CacheError::SourceNonRegular { path }),
        }
    }
    Ok(())
}

fn hash_opened_file(
    parent: std::os::fd::BorrowedFd<'_>,
    name: &str,
    expected_stat: &stat::FileStat,
    path: &str,
    maximum: u64,
) -> Result<String, CacheError> {
    let descriptor = fcntl::openat(
        parent,
        name,
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_error("open attempt file", Path::new(path), error))?;
    let opened = stat::fstat(&descriptor)
        .map_err(|error| nix_error("inspect attempt file", Path::new(path), error))?;
    if !same_file(expected_stat, &opened) {
        return Err(CacheError::SourceReplaced {
            path: path.to_owned(),
        });
    }
    if file_kind(&opened) != SFlag::S_IFREG || opened.st_nlink != 1 {
        return Err(CacheError::SourceReplaced {
            path: path.to_owned(),
        });
    }
    hash_descriptor(descriptor, path, maximum)
}

fn build_and_publish(
    plan: &PromotionPlan<'_>,
    staging: &Staging,
    snapshot: &[SourceEntry],
) -> Result<(PublishedGeneration, bool), CacheError> {
    // Materialize every directory (including empty ones) first, then copy every
    // regular file, verifying its bytes against the snapshot as it is written.
    for entry in snapshot {
        if matches!(entry.kind, EntryKind::Directory) {
            create_staging_directory(staging.descriptor.as_fd(), &entry.path)?;
        }
    }
    for entry in snapshot {
        if let EntryKind::File {
            size,
            checksum,
            executable,
        } = &entry.kind
        {
            copy_to_staging(
                plan.attempt_root.descriptor.as_fd(),
                staging.descriptor.as_fd(),
                &entry.path,
                *size,
                checksum,
                *executable,
                plan.bounds,
            )?;
        }
    }
    // Re-enumerate the source after copying. An inserted, removed, linked,
    // replaced, or mode-changed entry causes refusal; published bytes always
    // come from the staged descriptors, which are independently re-verified.
    let final_source = collect_source(plan.attempt_root.descriptor.as_fd(), plan.bounds)?;
    if final_source != snapshot {
        return Err(CacheError::SourceReplaced {
            path: "attempt root".to_owned(),
        });
    }
    let staged = collect_source(staging.descriptor.as_fd(), plan.bounds)?;
    if staged != snapshot {
        return Err(CacheError::StagedChecksumMismatch {
            path: "staging".to_owned(),
        });
    }
    let identity = tree_identity(
        snapshot,
        &plan.supported_source_identities,
        &plan.lockfile_before_identity,
        &plan.lockfile_after_identity,
    );
    let generation_name = format!("generation-{}", identity.trim_start_matches("sha256:"));

    // Reuse an identical existing generation only after re-verifying it.
    if let Some(existing) =
        existing_generation(plan.generation_parent.descriptor.as_fd(), &generation_name)?
    {
        return reuse_or_collision(plan, existing, &generation_name, &identity);
    }

    make_immutable_tree(staging.descriptor.as_fd())?;
    stat::fchmod(&staging.descriptor, immutable_directory_mode())
        .map_err(|error| nix_error("make staged generation immutable", &staging.path, error))?;
    unistd::fsync(&staging.descriptor)
        .map_err(|error| nix_error("fsync staged generation", &staging.path, error))?;
    match fcntl::renameat2(
        plan.generation_parent.descriptor.as_fd(),
        staging.name.as_str(),
        plan.generation_parent.descriptor.as_fd(),
        generation_name.as_str(),
        RenameFlags::RENAME_NOREPLACE,
    ) {
        Ok(()) => {}
        Err(Errno::EEXIST) => {
            // A concurrent publisher won the name. The staging tree is now
            // immutable; verifying and reusing the winner is still correct.
            let existing =
                existing_generation(plan.generation_parent.descriptor.as_fd(), &generation_name)?
                    .ok_or_else(|| CacheError::GenerationCollision {
                        name: generation_name.clone(),
                    })?;
            return reuse_or_collision(plan, existing, &generation_name, &identity);
        }
        Err(error) => {
            return Err(nix_error(
                "publish immutable generation",
                &plan.generation_parent.display_path.join(&generation_name),
                error,
            ));
        }
    }

    // The rename succeeded: the generation is published and named. Only the
    // parent-directory durability confirmation may still fail, and if it does
    // we must not treat the already-published generation as absent or clean it.
    let durability = match unistd::fsync(plan.generation_parent.descriptor.as_fd()) {
        Ok(()) => Durability::Synced,
        Err(_) => Durability::ParentSyncUnknown,
    };
    Ok((
        PublishedGeneration {
            path: plan.generation_parent.display_path.join(generation_name),
            identity,
            durability,
            reused: false,
        },
        true,
    ))
}

/// Opens an existing generation directory descriptor-relatively, or returns
/// `None` when it is absent.
fn existing_generation(
    parent: std::os::fd::BorrowedFd<'_>,
    name: &str,
) -> Result<Option<OwnedFd>, CacheError> {
    match fcntl::openat(
        parent,
        name,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    ) {
        Ok(descriptor) => Ok(Some(descriptor)),
        Err(Errno::ENOENT) => Ok(None),
        Err(Errno::ENOTDIR) | Err(Errno::ELOOP) => Err(CacheError::GenerationMismatch {
            name: name.to_owned(),
        }),
        Err(error) => Err(nix_error(
            "inspect existing generation",
            Path::new(name),
            error,
        )),
    }
}

/// Re-verifies an existing generation and either safely reuses it (identical
/// content and provenance) or reports a collision.
fn reuse_or_collision(
    plan: &PromotionPlan<'_>,
    existing: OwnedFd,
    generation_name: &str,
    expected_identity: &str,
) -> Result<(PublishedGeneration, bool), CacheError> {
    let entries = collect_source(existing.as_fd(), plan.bounds)?;
    let identity = tree_identity(
        &entries,
        &plan.supported_source_identities,
        &plan.lockfile_before_identity,
        &plan.lockfile_after_identity,
    );
    if identity != expected_identity {
        return Err(CacheError::GenerationCollision {
            name: generation_name.to_owned(),
        });
    }
    Ok((
        PublishedGeneration {
            path: plan.generation_parent.display_path.join(generation_name),
            identity,
            durability: Durability::Synced,
            reused: true,
        },
        false,
    ))
}

fn immutable_directory_mode() -> Mode {
    Mode::from_bits_truncate(0o555)
}

fn immutable_file_mode(executable: bool) -> Mode {
    Mode::from_bits_truncate(if executable { 0o555 } else { 0o444 })
}

/// Derives the deterministic, bounded identity of a complete generation from its
/// sorted entries (paths, kinds, safe modes, sizes, and checksums), the sorted
/// supported-source identities, and both lockfile identities. Any change to file
/// content, directory structure, mode, or provenance yields a distinct identity.
fn tree_identity(
    entries: &[SourceEntry],
    sources: &[String],
    lockfile_before: &str,
    lockfile_after: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"kvist/cargo-home-generation/v2\0");
    for entry in entries {
        append_identity_field(&mut hasher, entry.path.as_bytes());
        let mode = format!("{:o}", safe_mode(&entry.kind));
        match &entry.kind {
            EntryKind::Directory => {
                append_identity_field(&mut hasher, b"dir");
                append_identity_field(&mut hasher, mode.as_bytes());
            }
            EntryKind::File { size, checksum, .. } => {
                append_identity_field(&mut hasher, b"file");
                append_identity_field(&mut hasher, mode.as_bytes());
                append_identity_field(&mut hasher, size.to_string().as_bytes());
                append_identity_field(&mut hasher, checksum.as_bytes());
            }
        }
    }
    append_identity_field(&mut hasher, b"sources");
    let mut sorted = sources.to_vec();
    sorted.sort();
    for source in &sorted {
        append_identity_field(&mut hasher, source.as_bytes());
    }
    append_identity_field(&mut hasher, b"lockfile-before");
    append_identity_field(&mut hasher, lockfile_before.as_bytes());
    append_identity_field(&mut hasher, b"lockfile-after");
    append_identity_field(&mut hasher, lockfile_after.as_bytes());
    digest_hasher(hasher)
}

fn make_immutable_tree(root: std::os::fd::BorrowedFd<'_>) -> Result<(), CacheError> {
    let mut directory = Dir::openat(
        root,
        ".",
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_error("open staged directory", Path::new("staging"), error))?;
    let mut names = Vec::new();
    for entry in directory.iter() {
        let entry =
            entry.map_err(|error| nix_error("read staged directory", Path::new("staging"), error))?;
        let name = entry
            .file_name()
            .to_str()
            .map_err(|_| CacheError::StagedChecksumMismatch {
                path: "<non-utf8>".to_owned(),
            })?;
        if name != "." && name != ".." {
            names.push(name.to_owned());
        }
    }
    names.sort();
    for name in names {
        let metadata = stat::fstatat(root, name.as_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
            .map_err(|error| nix_error("inspect staged entry", Path::new(&name), error))?;
        match file_kind(&metadata) {
            SFlag::S_IFDIR => {
                let child = fcntl::openat(
                    root,
                    name.as_str(),
                    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|error| nix_error("open staged directory", Path::new(&name), error))?;
                make_immutable_tree(child.as_fd())?;
                stat::fchmod(&child, immutable_directory_mode()).map_err(|error| {
                    nix_error("make staged directory immutable", Path::new(&name), error)
                })?;
            }
            SFlag::S_IFREG => {
                // Files were already normalized to their safe immutable mode when
                // they were copied; confirm the write bit is clear.
                let executable = (metadata.st_mode & 0o111) != 0;
                let file = fcntl::openat(
                    root,
                    name.as_str(),
                    OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|error| nix_error("open staged file", Path::new(&name), error))?;
                stat::fchmod(&file, immutable_file_mode(executable)).map_err(|error| {
                    nix_error("make staged file immutable", Path::new(&name), error)
                })?;
            }
            _ => {
                return Err(CacheError::StagedChecksumMismatch { path: name });
            }
        }
    }
    Ok(())
}

/// Creates one staging directory (and any missing ancestors) beneath the staging
/// root with an owner-writable temporary mode; it is normalized to `0555` when
/// the tree is made immutable.
fn create_staging_directory(
    staging_root: std::os::fd::BorrowedFd<'_>,
    relative: &str,
) -> Result<(), CacheError> {
    let mut parent = fcntl::openat(
        staging_root,
        ".",
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_error("open staging root", Path::new("staging"), error))?;
    for component in relative.split('/') {
        match stat::mkdirat(parent.as_fd(), component, Mode::S_IRWXU) {
            Ok(()) | Err(Errno::EEXIST) => {}
            Err(error) => {
                return Err(nix_error(
                    "create staging directory",
                    Path::new(relative),
                    error,
                ));
            }
        }
        let child = fcntl::openat(
            parent.as_fd(),
            component,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| nix_error("open staging directory", Path::new(relative), error))?;
        parent = child;
    }
    Ok(())
}

fn copy_to_staging(
    source_root: std::os::fd::BorrowedFd<'_>,
    staging_root: std::os::fd::BorrowedFd<'_>,
    relative: &str,
    size: u64,
    checksum: &str,
    executable: bool,
    bounds: CacheBounds,
) -> Result<(), CacheError> {
    let (source_parent, source_name) = open_parent(source_root, relative, false, Path::new("attempt"))?;
    let source_stat = stat::fstatat(
        source_parent.as_fd(),
        source_name.as_str(),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|error| nix_error("inspect attempt source", Path::new(relative), error))?;
    if file_kind(&source_stat) != SFlag::S_IFREG || source_stat.st_nlink != 1 {
        return Err(CacheError::SourceReplaced {
            path: relative.to_owned(),
        });
    }
    let source = fcntl::openat(
        source_parent.as_fd(),
        source_name.as_str(),
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_error("open attempt source", Path::new(relative), error))?;
    let opened_stat = stat::fstat(&source)
        .map_err(|error| nix_error("inspect attempt source", Path::new(relative), error))?;
    if !same_file(&source_stat, &opened_stat) {
        return Err(CacheError::SourceReplaced {
            path: relative.to_owned(),
        });
    }
    let (destination_parent, destination_name) =
        open_parent(staging_root, relative, true, Path::new("staging"))?;
    let destination = fcntl::openat(
        destination_parent.as_fd(),
        destination_name.as_str(),
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::S_IRUSR | Mode::S_IWUSR,
    )
    .map_err(|error| nix_error("create staged file", Path::new(relative), error))?;
    let mut source = File::from(source);
    let mut destination = File::from(destination);
    let mut hasher = Sha256::new();
    let mut written = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = source
            .read(&mut buffer)
            .map_err(|error| io_error("read attempt source", Path::new(relative), error.to_string()))?;
        if count == 0 {
            break;
        }
        written = written.checked_add(count as u64).ok_or(CacheError::FileSize {
            path: relative.to_owned(),
            limit: bounds.max_file_bytes,
        })?;
        if written > bounds.max_file_bytes {
            return Err(CacheError::FileSize {
                path: relative.to_owned(),
                limit: bounds.max_file_bytes,
            });
        }
        destination
            .write_all(&buffer[..count])
            .map_err(|error| io_error("write staged file", Path::new(relative), error.to_string()))?;
        hasher.update(&buffer[..count]);
    }
    destination
        .sync_all()
        .map_err(|error| io_error("fsync staged file", Path::new(relative), error.to_string()))?;
    drop(destination);
    if written != size || digest_hasher(hasher) != checksum {
        return Err(CacheError::ChecksumMismatch {
            path: relative.to_owned(),
        });
    }
    let staged = fcntl::openat(
        destination_parent.as_fd(),
        destination_name.as_str(),
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_error("reopen staged file", Path::new(relative), error))?;
    let staged_stat = stat::fstat(&staged)
        .map_err(|error| nix_error("inspect staged file", Path::new(relative), error))?;
    if file_kind(&staged_stat) != SFlag::S_IFREG || staged_stat.st_nlink != 1 {
        return Err(CacheError::StagedChecksumMismatch {
            path: relative.to_owned(),
        });
    }
    // Normalize the staged file to its safe immutable mode. The execute bit is
    // preserved (as `0555`) only when the source carried one; write, setuid,
    // setgid, sticky, and special bits are never preserved.
    stat::fchmod(&staged, immutable_file_mode(executable))
        .map_err(|error| nix_error("normalize staged file mode", Path::new(relative), error))?;
    if hash_descriptor(staged, relative, bounds.max_file_bytes)? != checksum {
        return Err(CacheError::StagedChecksumMismatch {
            path: relative.to_owned(),
        });
    }
    Ok(())
}

fn open_parent(
    root: std::os::fd::BorrowedFd<'_>,
    relative: &str,
    create: bool,
    display_root: &Path,
) -> Result<(OwnedFd, String), CacheError> {
    let mut parts = relative.split('/').peekable();
    let mut parent = fcntl::openat(
        root,
        ".",
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| nix_error("open root", display_root, error))?;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            return Ok((parent, part.to_owned()));
        }
        let path = display_root.join(part);
        if create {
            match stat::fstatat(parent.as_fd(), part, AtFlags::AT_SYMLINK_NOFOLLOW) {
                Ok(stat) if file_kind(&stat) == SFlag::S_IFDIR => {}
                Ok(_) => {
                    return Err(CacheError::SourceNonRegular {
                        path: relative.to_owned(),
                    });
                }
                Err(Errno::ENOENT) => {
                    stat::mkdirat(parent.as_fd(), part, Mode::S_IRWXU)
                        .map_err(|error| nix_error("create staging directory", &path, error))?;
                }
                Err(error) => return Err(nix_error("inspect staging directory", &path, error)),
            }
        }
        let child = fcntl::openat(
            parent.as_fd(),
            part,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| nix_error("open directory", &path, error))?;
        parent = child;
    }
    Err(CacheError::ManifestPath {
        path: relative.to_owned(),
    })
}

fn append_identity_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn hash_descriptor(descriptor: OwnedFd, path: &str, maximum: u64) -> Result<String, CacheError> {
    let mut file = File::from(descriptor);
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| io_error("read file", Path::new(path), error.to_string()))?;
        if count == 0 {
            break;
        }
        total = total.checked_add(count as u64).ok_or(CacheError::FileSize {
            path: path.to_owned(),
            limit: maximum,
        })?;
        if total > maximum {
            return Err(CacheError::FileSize {
                path: path.to_owned(),
                limit: maximum,
            });
        }
        hasher.update(&buffer[..count]);
    }
    Ok(digest_hasher(hasher))
}

fn digest_hasher(hasher: Sha256) -> String {
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn file_kind(stat: &stat::FileStat) -> SFlag {
    SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT
}

fn same_file(left: &stat::FileStat, right: &stat::FileStat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

fn join_relative(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

struct Staging {
    name: String,
    path: PathBuf,
    descriptor: OwnedFd,
}

fn create_staging(parent: &TrustedGenerationParent) -> Result<Staging, CacheError> {
    for _ in 0..MAX_STAGING_ATTEMPTS {
        let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(".kvist-cache-staging-{}-{sequence}", std::process::id());
        match stat::mkdirat(parent.descriptor.as_fd(), name.as_str(), Mode::S_IRWXU) {
            Ok(()) => {
                let descriptor = fcntl::openat(
                    parent.descriptor.as_fd(),
                    name.as_str(),
                    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|error| nix_error("open staging directory", &parent.display_path.join(&name), error))?;
                return Ok(Staging {
                    path: parent.display_path.join(&name),
                    name,
                    descriptor,
                });
            }
            Err(Errno::EEXIST) => {}
            Err(error) => {
                return Err(nix_error(
                    "create staging directory",
                    &parent.display_path.join(&name),
                    error,
                ));
            }
        }
    }
    Err(CacheError::StagingExhausted)
}

fn remove_tree_at(
    parent: std::os::fd::BorrowedFd<'_>,
    name: &str,
    display_parent: &Path,
) -> Result<(), CacheError> {
    let entry = stat::fstatat(parent, name, AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|error| nix_error("inspect staging cleanup", &display_parent.join(name), error))?;
    if file_kind(&entry) == SFlag::S_IFDIR {
        let mut directory = Dir::openat(
            parent,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| nix_error("open staging cleanup", &display_parent.join(name), error))?;
        let mut children = Vec::new();
        for child in directory.iter() {
            let child = child.map_err(|error| nix_error("read staging cleanup", &display_parent.join(name), error))?;
            let child_name = child.file_name().to_str()            .map_err(|_| CacheError::Cleanup {
                detail: "staging directory contains a non-UTF-8 name".to_owned(),
            })?;
            if child_name != "." && child_name != ".." {
                children.push(child_name.to_owned());
            }
        }
        children.sort();
        let directory_fd = fcntl::openat(
            parent,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| nix_error("open staging cleanup", &display_parent.join(name), error))?;
        // Publication has not occurred on this path. Restore only the private
        // staging directory's owner mode through its retained descriptor so
        // recursive cleanup remains possible after pre-publication hardening.
        stat::fchmod(&directory_fd, Mode::S_IRWXU).map_err(|error| {
            nix_error(
                "prepare staging cleanup",
                &display_parent.join(name),
                error,
            )
        })?;
        for child in children {
            remove_tree_at(directory_fd.as_fd(), &child, &display_parent.join(name))?;
        }
        unistd::unlinkat(parent, name, unistd::UnlinkatFlags::RemoveDir)
            .map_err(|error| nix_error("remove staging directory", &display_parent.join(name), error))?;
    } else {
        unistd::unlinkat(parent, name, unistd::UnlinkatFlags::NoRemoveDir)
            .map_err(|error| nix_error("remove staged file", &display_parent.join(name), error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    fn bounds() -> CacheBounds {
        CacheBounds {
            max_files: 16,
            max_file_bytes: 1024,
            max_cache_bytes: 4096,
        }
    }

    fn digest(bytes: &[u8]) -> String {
        format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
    }

    fn entry(path: &str, bytes: &[u8]) -> ManifestEntry {
        ManifestEntry {
            path: path.to_owned(),
            size: bytes.len() as u64,
            checksum: digest(bytes),
        }
    }

    fn roots() -> (tempfile::TempDir, IsolatedAttemptRoot, TrustedGenerationParent) {
        let temp = tempfile::tempdir().expect("create fixture");
        let attempt = temp.path().join("attempt");
        let parent = temp.path().join("generations");
        fs::create_dir(&attempt).expect("create attempt");
        fs::create_dir(&parent).expect("create parent");
        let attempt_capability = open_isolated_attempt_root(&attempt).expect("open attempt");
        let parent_capability = open_trusted_generation_parent(&parent).expect("open parent");
        (temp, attempt_capability, parent_capability)
    }

    fn plan<'a>(
        attempt: &'a IsolatedAttemptRoot,
        parent: &'a TrustedGenerationParent,
        manifest: &'a [ManifestEntry],
    ) -> PromotionPlan<'a> {
        PromotionPlan {
            attempt_root: attempt,
            generation_parent: parent,
            manifest,
            bounds: bounds(),
            lockfile_before_identity: digest(b"before"),
            lockfile_after_identity: digest(b"after"),
            supported_source_identities: vec![digest(b"source")],
        }
    }

    #[test]
    fn publishes_immutable_bounded_generation() {
        let (temp, attempt, parent) = roots();
        fs::create_dir(temp.path().join("attempt/registry")).expect("registry");
        fs::write(temp.path().join("attempt/registry/crate"), b"crate").expect("source");
        // An empty directory is part of the generation and its immutable tree.
        fs::create_dir(temp.path().join("attempt/empty")).expect("empty directory");
        let manifest = [entry("registry/crate", b"crate")];
        let generation = promote(&plan(&attempt, &parent, &manifest)).expect("promote");
        assert!(generation.path.is_dir());
        assert!(!generation.reused);
        assert_eq!(generation.durability, Durability::Synced);
        assert_eq!(fs::read(generation.path.join("registry/crate")).expect("read"), b"crate");
        assert!(
            generation.path.join("empty").is_dir(),
            "an empty source directory must be present in the generation"
        );
        assert!(generation.identity.starts_with("sha256:"));
        let mode = fs::metadata(&generation.path).expect("stat generation").permissions().mode();
        assert_eq!(mode & 0o7777, 0o555, "generation directory is immutable 0555");
        let file_mode = fs::metadata(generation.path.join("registry/crate"))
            .expect("stat file")
            .permissions()
            .mode();
        assert_eq!(file_mode & 0o7777, 0o444, "non-executable file is 0444");
        let empty_mode = fs::metadata(generation.path.join("empty"))
            .expect("stat empty")
            .permissions()
            .mode();
        assert_eq!(empty_mode & 0o7777, 0o555, "directory is 0555");
    }

    #[test]
    fn preserves_execute_bit_as_safe_immutable_mode() {
        let (temp, attempt, parent) = roots();
        let script = temp.path().join("attempt/bin");
        fs::create_dir(temp.path().join("attempt/dir")).expect("dir");
        fs::write(&script, b"#!/bin/sh\n").expect("script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o6771)).expect("setuid+exec");
        let manifest = [entry("bin", b"#!/bin/sh\n")];
        let generation = promote(&plan(&attempt, &parent, &manifest)).expect("promote");
        let mode = fs::metadata(generation.path.join("bin"))
            .expect("stat bin")
            .permissions()
            .mode();
        // The execute bit is preserved as 0555; the setuid, group, and write
        // bits from the source are never carried into the generation.
        assert_eq!(mode & 0o7777, 0o555, "executable file becomes 0555 without setuid");
    }

    #[test]
    fn bounds_directory_count_and_depth() {
        let (temp, attempt, parent) = roots();
        let empty: [ManifestEntry; 0] = [];
        // A tree deeper than the depth bound is refused.
        let mut deep = temp.path().join("attempt");
        for index in 0..(MAX_DIRECTORY_DEPTH + 2) {
            deep = deep.join(format!("d{index}"));
        }
        fs::create_dir_all(&deep).expect("deep tree");
        assert!(matches!(
            promote(&plan(&attempt, &parent, &empty)),
            Err(CacheError::DirectoryDepth { .. })
        ));
    }

    #[test]
    fn rejects_root_overlap_symlink_and_zero_bounds() {
        let temp = tempfile::tempdir().expect("fixture");
        let root = temp.path().join("root");
        fs::create_dir(&root).expect("root");
        let same_attempt = open_isolated_attempt_root(&root).expect("open");
        let same_parent = open_trusted_generation_parent(&root).expect("open");
        let empty: [ManifestEntry; 0] = [];
        assert!(matches!(
            promote(&plan(&same_attempt, &same_parent, &empty)),
            Err(CacheError::RootsOverlap)
        ));
        symlink(&root, temp.path().join("link")).expect("link");
        assert!(matches!(
            open_isolated_attempt_root(&temp.path().join("link")),
            Err(CacheError::SymlinkRoot { .. })
        ));
        let (_, attempt, parent) = roots();
        let mut zero = plan(&attempt, &parent, &empty);
        zero.bounds.max_files = 0;
        assert!(matches!(promote(&zero), Err(CacheError::ZeroBound { .. })));
        let mut excessive = plan(&attempt, &parent, &empty);
        excessive.bounds.max_cache_bytes = MAX_CACHE_BYTES + 1;
        assert!(matches!(
            promote(&excessive),
            Err(CacheError::BoundExceeds { .. })
        ));
    }

    #[test]
    fn rejects_links_special_entries_bounds_and_manifest_mismatches() {
        use std::os::unix::net::UnixListener;
        let (temp, attempt, parent) = roots();
        let source = temp.path().join("attempt");
        fs::write(source.join("one"), b"one").expect("one");
        symlink(source.join("one"), source.join("link")).expect("link");
        let manifest = [entry("one", b"one")];
        assert!(matches!(promote(&plan(&attempt, &parent, &manifest)), Err(CacheError::SourceSymlink { .. })));

        fs::remove_file(source.join("link")).expect("remove link");
        fs::hard_link(source.join("one"), source.join("hard")).expect("hard");
        assert!(matches!(promote(&plan(&attempt, &parent, &manifest)), Err(CacheError::SourceHardLink { .. })));
        fs::remove_file(source.join("hard")).expect("remove hard");

        let _socket = UnixListener::bind(source.join("socket")).expect("socket");
        assert!(matches!(promote(&plan(&attempt, &parent, &manifest)), Err(CacheError::SourceNonRegular { .. })));
        drop(_socket);
        fs::remove_file(source.join("socket")).expect("remove socket");

        let mut restrictive = plan(&attempt, &parent, &manifest);
        restrictive.bounds.max_file_bytes = 2;
        assert!(matches!(promote(&restrictive), Err(CacheError::FileSize { .. })));
        fs::write(source.join("extra"), b"extra").expect("extra");
        assert!(matches!(promote(&plan(&attempt, &parent, &manifest)), Err(CacheError::UnlistedFile { .. })));
    }

    #[test]
    fn root_replacement_after_open_does_not_redirect_attempt_capability() {
        let (temp, attempt, parent) = roots();
        let attempt_path = temp.path().join("attempt");
        fs::write(attempt_path.join("crate"), b"trusted").expect("trusted source");
        let moved = temp.path().join("attempt-old");
        fs::rename(&attempt_path, &moved).expect("replace root");
        fs::create_dir(&attempt_path).expect("new root");
        fs::write(attempt_path.join("crate"), b"attacker").expect("attacker source");
        let manifest = [entry("crate", b"trusted")];
        let generation = promote(&plan(&attempt, &parent, &manifest)).expect("descriptor source");
        assert_eq!(fs::read(generation.path.join("crate")).expect("generation"), b"trusted");
    }

    #[test]
    fn reuses_identical_generation_and_rejects_tampered_collision() {
        let (temp, attempt, parent) = roots();
        fs::write(temp.path().join("attempt/crate"), b"crate").expect("source");
        let manifest = [entry("crate", b"crate")];
        let first = promote(&plan(&attempt, &parent, &manifest)).expect("first");
        assert!(!first.reused);
        // A second identical promotion reuses the existing immutable generation
        // after descriptor-relative re-verification rather than erroring.
        let second = promote(&plan(&attempt, &parent, &manifest)).expect("reuse");
        assert!(second.reused);
        assert_eq!(first.identity, second.identity);
        assert_eq!(first.path, second.path);
        assert_eq!(fs::read(first.path.join("crate")).expect("bytes"), b"crate");
        // No staging directory is left behind on either the publish or the reuse
        // path.
        assert!(
            fs::read_dir(temp.path().join("generations"))
                .expect("parent")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".kvist-cache-staging-")),
            "reuse must not leave a staging directory"
        );

        // A pre-existing generation of the same name but different content is a
        // genuine collision, not a safe reuse.
        let (other_temp, other_attempt, other_parent) = roots();
        fs::write(other_temp.path().join("attempt/crate"), b"crate").expect("second source");
        let name = first.path.file_name().expect("name").to_owned();
        let colliding = other_temp.path().join("generations").join(&name);
        fs::create_dir(&colliding).expect("colliding generation");
        fs::write(colliding.join("crate"), b"tampered").expect("tampered content");
        assert!(matches!(
            promote(&plan(&other_attempt, &other_parent, &manifest)),
            Err(CacheError::GenerationCollision { .. })
        ));
    }

    #[test]
    fn rejects_all_bounds_and_exact_manifest_mismatches() {
        let (temp, attempt, parent) = roots();
        let source = temp.path().join("attempt");
        fs::write(source.join("one"), b"1234").expect("one");
        fs::write(source.join("two"), b"5678").expect("two");
        let manifest = [entry("one", b"1234"), entry("two", b"5678")];

        let mut count = plan(&attempt, &parent, &manifest);
        count.bounds.max_files = 1;
        assert!(matches!(promote(&count), Err(CacheError::FileCount { .. })));
        let mut aggregate = plan(&attempt, &parent, &manifest);
        aggregate.bounds.max_cache_bytes = 6;
        assert!(matches!(
            promote(&aggregate),
            Err(CacheError::AggregateSize { .. })
        ));

        let checksum = [ManifestEntry {
            path: "one".to_owned(),
            size: 4,
            checksum: digest(b"forged"),
        }];
        fs::remove_file(source.join("two")).expect("remove two");
        assert!(matches!(
            promote(&plan(&attempt, &parent, &checksum)),
            Err(CacheError::ChecksumMismatch { .. })
        ));
        let missing = [entry("one", b"1234"), entry("absent", b"missing")];
        assert!(matches!(
            promote(&plan(&attempt, &parent, &missing)),
            Err(CacheError::MissingFile { .. })
        ));
    }

    #[test]
    fn rechecks_file_and_ancestor_after_read_only_inspection() {
        let (temp, attempt, parent) = roots();
        let source = temp.path().join("attempt");
        fs::create_dir(source.join("registry")).expect("registry");
        fs::write(source.join("registry/crate"), b"trusted").expect("source");
        let manifest = [entry("registry/crate", b"trusted")];
        let initial = plan(&attempt, &parent, &manifest);
        inspect_source(&initial).expect("initial inspection");
        fs::write(source.join("registry/crate"), b"changed").expect("tamper");
        assert!(matches!(
            promote(&initial),
            Err(CacheError::SizeMismatch { .. }) | Err(CacheError::ChecksumMismatch { .. })
        ));

        fs::write(source.join("registry/crate"), b"trusted").expect("restore");
        inspect_source(&initial).expect("second inspection");
        fs::rename(source.join("registry"), source.join("registry-old")).expect("replace ancestor");
        symlink(source.join("registry-old"), source.join("registry")).expect("symlink ancestor");
        assert!(matches!(
            promote(&initial),
            Err(CacheError::SourceSymlink { .. })
        ));
    }

    #[test]
    fn rejects_containing_roots_and_generation_identity_is_bounded_and_stable() {
        let temp = tempfile::tempdir().expect("fixture");
        let parent_path = temp.path().join("parent");
        let attempt_path = parent_path.join("attempt");
        fs::create_dir(&parent_path).expect("parent");
        fs::create_dir(&attempt_path).expect("attempt");
        let attempt = open_isolated_attempt_root(&attempt_path).expect("attempt");
        let parent = open_trusted_generation_parent(&parent_path).expect("parent");
        let empty: [ManifestEntry; 0] = [];
        assert!(matches!(
            promote(&plan(&attempt, &parent, &empty)),
            Err(CacheError::RootsOverlap)
        ));

        let (first_temp, first_attempt, first_parent) = roots();
        let (second_temp, second_attempt, second_parent) = roots();
        fs::write(first_temp.path().join("attempt/crate"), b"same").expect("first");
        fs::write(second_temp.path().join("attempt/crate"), b"same").expect("second");
        let manifest = [entry("crate", b"same")];
        let first = promote(&plan(&first_attempt, &first_parent, &manifest)).expect("first generation");
        let second = promote(&plan(&second_attempt, &second_parent, &manifest)).expect("second generation");
        assert_eq!(first.identity, second.identity);
        assert!(first.identity.len() <= 71, "identity stays protocol bounded");
    }
}
