//! Host-resolved, read-only Rust resources for the workspace authoring sandbox.
//!
//! This is not the maerg provisioning or Cargo verification authority. It
//! never installs anything, reads Cargo credentials, or trusts manifest paths.
//! The existing System/Authoring protocol grants the compiler, fixed Cargo shim
//! and shared vendor area as read-only Toolchain build resources at disjoint
//! `/rust/*` destinations. No Cargo-phase topology or cache authority is added.
//! The runner masks the writable workspace vendor alias. Per-tool directory
//! live views reflect host publication without copying or reader leases.
//!
//! Selection ignores ambient Rust/Cargo overrides and uses the standard
//! `$HOME/.rustup/toolchains` installation layout. Channels, requested
//! components/targets and executable/layout identities are checked without
//! installing anything. Missing material must be provisioned separately on the
//! host. Installed-resource preparation is bounded to 180 seconds;
//! cache/selection state is private to each sandbox invocation; build targets
//! persist in the writable workspace.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use galla::protocol::{Access, Grant, Purpose};
use sav::CancellationToken;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result, io_error};

#[path = "rust_environment/host.rs"]
mod host;
#[path = "rust_environment/inventory.rs"]
mod inventory;
#[path = "rust_environment/live.rs"]
mod live;
pub(crate) use live::Cache as RustEnvironmentCache;

const TOOLCHAIN_DEST: &str = "/rust/toolchain";
const RUNTIME_DEST: &str = "/rust/runtime";
const RUSTUP_HOME_DEST: &str = "/rust/rustup-home";
const PREPARATION_BUDGET: Duration = Duration::from_secs(180);

/// A complete installed inventory, initial selection and private sandbox resources.
///
/// Clones share the staging owner; the final registry/executor drop removes
/// staged wrappers and an optional empty vendor view. No packages are copied.
#[derive(Debug, Clone)]
pub struct RustEnvironment {
    resources: Arc<Resources>,
}

#[derive(Debug)]
struct Resources {
    workspace: PathBuf,
    root: PathBuf,
    rustup: PathBuf,
    channel: String,
    identity: String,
    staging: tempfile::TempDir,
    vendor_identity: String,
    has_vendor: bool,
    pin_identity: String,
    pinned: bool,
    user_cargo_bin: Option<PathBuf>,
    rustup_home: PathBuf,
    inventory: inventory::Inventory,
    topology: live::Topology,
    issues: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    toolchain: PinToolchain,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PinToolchain {
    channel: String,
    #[serde(default)]
    components: Vec<String>,
    #[serde(default)]
    targets: Vec<String>,
    profile: Option<String>,
}

/// The strict per-inode identity fields that detect any modification, truncation
/// or replacement of a file.
type Fingerprint = (u64, u64, u64, i64, i64, i64, i64);

struct Preparation<'a> {
    started: Instant,
    cancellation: &'a CancellationToken,
}

impl Preparation<'_> {
    fn check(&self) -> Result<()> {
        crate::executor::check_cancelled(self.cancellation)?;
        if self.started.elapsed() >= PREPARATION_BUDGET {
            return Err(failure(format!(
                "Rust preparation exceeded {} seconds; narrow the workspace/vendor tree",
                PREPARATION_BUDGET.as_secs()
            )));
        }
        Ok(())
    }
}

fn failure(reason: impl Into<String>) -> Error {
    Error::SandboxBuild {
        reason: reason.into(),
    }
}

fn label(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn inspect_path(path: &Path) -> Result<fs::Metadata> {
    crate::file_tools::canonical_path(
        path.to_str()
            .ok_or_else(|| failure("Rust resource path must be UTF-8"))?,
    )?;
    let mut prefix = PathBuf::from("/");
    for component in path.components().filter_map(|part| match part {
        std::path::Component::Normal(part) => Some(part),
        _ => None,
    }) {
        prefix.push(component);
        let metadata = fs::symlink_metadata(&prefix).map_err(|e| {
            io_error(
                "inspect non-link Rust resource",
                Some(&prefix.to_string_lossy()),
                e,
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(failure(format!(
                "Rust resource `{}` and its ancestors must not be symbolic links",
                path.display()
            )));
        }
    }
    fs::symlink_metadata(path)
        .map_err(|e| io_error("inspect Rust resource", Some(&path.to_string_lossy()), e))
}

fn file(path: &Path, maximum: u64) -> Result<File> {
    let origin = File::from(galla::enforcement::source::pin(path).map_err(failure)?);
    let metadata = origin
        .metadata()
        .map_err(|error| io_error("inspect pinned Rust resource", None, error))?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(failure(format!(
            "Rust resource `{}` must be a bounded regular file (maximum {maximum} bytes)",
            path.display()
        )));
    }
    // Reopen the pinned regular inode, never a second mutable host pathname.
    let opened = File::open(format!("/proc/self/fd/{}", origin.as_raw_fd()))
        .map_err(|e| io_error("open Rust resource", Some(&path.to_string_lossy()), e))?;
    let actual = opened
        .metadata()
        .map_err(|e| io_error("inspect opened Rust resource", None, e))?;
    if !actual.is_file() {
        return Err(failure("opened Rust resource must be a regular file"));
    }
    Ok(opened)
}

fn bounded_bytes(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file(path, maximum)?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_error("read bounded Rust resource", None, e))?;
    if bytes.len() as u64 > maximum {
        return Err(failure("Rust resource grew beyond its byte bound"));
    }
    Ok(bytes)
}

fn fingerprint(metadata: &fs::Metadata) -> Fingerprint {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

/// A `sha256:` grant identity derived from a file's cheap fingerprint instead
/// of its contents. Used for the large native standard-library rlibs, whose
/// contents are never re-hashed by the runner (only the bwrap backend is) and
/// whose drift is already detected per call through the fingerprint. This keeps
/// the identity a valid digest bound to the file's inode, size and timestamps
/// without reading hundreds of megabytes at startup.
fn fingerprint_identity(fingerprint: Fingerprint) -> String {
    let (dev, ino, len, mtime, mtime_nsec, ctime, ctime_nsec) = fingerprint;
    let mut bytes = Vec::with_capacity(7 * 8);
    bytes.extend_from_slice(&dev.to_le_bytes());
    bytes.extend_from_slice(&ino.to_le_bytes());
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(&mtime.to_le_bytes());
    bytes.extend_from_slice(&mtime_nsec.to_le_bytes());
    bytes.extend_from_slice(&ctime.to_le_bytes());
    bytes.extend_from_slice(&ctime_nsec.to_le_bytes());
    label(&bytes)
}

fn hash_file(path: &Path, preparation: &Preparation<'_>) -> Result<(String, Fingerprint)> {
    let opened = file(path, u64::MAX)?;
    let before = opened
        .metadata()
        .map_err(|e| io_error("inspect Rust identity", None, e))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut opened = opened.take(before.len());
    loop {
        preparation.check()?;
        let count = opened
            .read(&mut buffer)
            .map_err(|e| io_error("hash Rust executable", None, e))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok((
        format!("sha256:{}", hex::encode(hash.finalize())),
        fingerprint(&before),
    ))
}

fn track_executable(path: PathBuf, _preparation: &Preparation<'_>) -> Result<String> {
    let metadata = inspect_path(&path)?;
    if !metadata.is_file() {
        return Err(failure(format!(
            "installed Rust executable `{}` must be a regular non-link file",
            path.display()
        )));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(failure(format!(
            "installed Rust executable `{}` is not executable",
            path.display()
        )));
    }
    let fingerprint = fingerprint(&metadata);
    let identity = fingerprint_identity(fingerprint);
    tracing::trace!(path = %path.display(), %identity, "Rust executable validated");
    Ok(identity)
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_'))
        && !value.starts_with('-')
        && !value.contains("..")
}

fn channel(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with('-')
        || matches!(value, "." | "..")
        || value.chars().any(|character| {
            character.is_control() || character.is_whitespace() || matches!(character, '/' | '\\')
        })
    {
        return Err(failure(
            "Rust selection must be one bounded registration name, not a path or option",
        ));
    }
    Ok(())
}

fn registered_root(home: &Path, workspace: &Path, name: &str) -> Result<PathBuf> {
    channel(name)?;
    if !host::operator_selector(&home.join(".rustup/toolchains"), workspace)? {
        return Err(failure(
            "host Rust registration selector slots must be outside the write scope before resolving their targets",
        ));
    }
    let parent = home
        .join(".rustup/toolchains")
        .canonicalize()
        .map_err(|error| {
            io_error(
                "resolve trusted host Rust registration directory",
                None,
                error,
            )
        })?;
    if parent.starts_with(workspace) {
        return Err(failure(
            "host Rust registration pointers must be outside the writable workspace",
        ));
    }
    let root = parent
        .join(name)
        .canonicalize()
        .map_err(|error| io_error("resolve host-registered Rust root", None, error))?;
    if root.starts_with(workspace)
        || workspace.starts_with(&root)
        || home.starts_with(&root)
        || home.join(".cargo").starts_with(&root)
        || !inspect_path(&root)?.is_dir()
    {
        return Err(failure(
            "registered Rust root must be outside the writable workspace and must not expose the host home or Cargo credential store",
        ));
    }
    Ok(root)
}

fn project_pin(workspace: &Path) -> Result<Option<PinToolchain>> {
    let toml = workspace.join("rust-toolchain.toml");
    let plain = workspace.join("rust-toolchain");
    let present = |path: &Path| -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(io_error("inspect project Rust pin", None, e)),
        }
    };
    let (has_toml, has_plain) = (present(&toml)?, present(&plain)?);
    if has_toml && has_plain {
        return Err(failure(
            "both rust-toolchain.toml and rust-toolchain exist; retain one authoritative project pin",
        ));
    }
    let pin = if has_toml {
        let bytes = bounded_bytes(&toml, 65536)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| failure("Rust pin must be UTF-8"))?;
        toml::from_str::<Pin>(text).map_err(|_| failure("invalid rust-toolchain.toml; only channel, components, targets and profile are supported"))?.toolchain
    } else if has_plain {
        let bytes = bounded_bytes(&plain, 65536)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| failure("Rust pin must be UTF-8"))?;
        if text.trim().contains(['\n', '\r']) {
            return Err(failure("rust-toolchain must contain exactly one channel"));
        }
        PinToolchain {
            channel: text.trim().to_owned(),
            components: Vec::new(),
            targets: Vec::new(),
            profile: None,
        }
    } else {
        return Ok(None);
    };
    channel(&pin.channel)?;
    if pin.components.len() > 32
        || pin.targets.len() > 32
        || pin.components.iter().chain(&pin.targets).any(|s| !token(s))
        || pin
            .profile
            .as_deref()
            .is_some_and(|p| !matches!(p, "minimal" | "default" | "complete"))
    {
        return Err(failure(
            "Rust pin has invalid/bounded components, targets or profile",
        ));
    }
    Ok(Some(pin))
}

fn pin_identity(workspace: &Path) -> Result<String> {
    let mut bytes = Vec::new();
    for name in ["rust-toolchain.toml", "rust-toolchain"] {
        let path = workspace.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                bytes.extend_from_slice(name.as_bytes());
                bytes.push(0);
                bytes.extend_from_slice(&bounded_bytes(&path, 65536)?);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error("recheck Rust project pin", None, e)),
        }
    }
    Ok(label(&bytes))
}

fn query_rustup(
    rustup: &Path,
    home: &Path,
    args: &[&str],
    preparation: &Preparation<'_>,
) -> Result<String> {
    preparation.check()?;
    let started = Instant::now();
    tracing::debug!(operation = args[0], "querying installed Rust selection");
    let mut command = Command::new(rustup);
    command
        .args(args)
        .current_dir("/")
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("RUSTUP_AUTO_INSTALL", "0");
    let result = crate::process::run(
        &mut command,
        None,
        PREPARATION_BUDGET.saturating_sub(preparation.started.elapsed()),
        16384,
        preparation.cancellation,
        |e| io_error("query installed Rust toolchain (never installs)", None, e),
    )?;
    tracing::debug!(
        operation = args[0],
        elapsed_ms = started.elapsed().as_millis() as u64,
        status = ?result.status,
        timed_out = result.timed_out,
        output_limit_exceeded = result.output_limit_exceeded,
        cancelled = result.cancelled,
        "installed Rust selection query completed"
    );
    if result.failed() {
        return Err(failure(
            "installed Rust selection is unavailable; provision or correct the project pin on the host, then retry the affected Rust command without restart (no installation or automatic replay is performed)",
        ));
    }
    String::from_utf8(result.stdout).map_err(|_| failure("rustup selection output must be UTF-8"))
}

/// Resolve installed Cargo for explicit operator provisioning without copying
/// dependencies, loading application configuration, or installing a toolchain.
pub(crate) fn host_cargo(workspace: &Path, cancellation: &CancellationToken) -> Result<PathBuf> {
    let workspace = workspace
        .canonicalize()
        .map_err(|error| io_error("resolve Rust workspace", None, error))?;
    let preparation = Preparation {
        started: Instant::now(),
        cancellation,
    };
    let pin = project_pin(&workspace)?;
    Ok(host::resolve(&workspace, pin.as_ref(), &preparation)?.cargo)
}

fn source_configuration(workspace: &Path, issues: &mut Vec<String>) -> Result<Option<Vec<u8>>> {
    let parent = workspace.join(".kvist/sandbox-cargo");
    let path = parent.join("config.toml");
    let input = match fs::symlink_metadata(&parent) {
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(error) => Err(io_error(
            "inspect native Cargo configuration parent",
            None,
            error,
        )),
        Ok(_) => match inspect_path(&parent) {
            Ok(metadata) if !metadata.is_dir() => return Ok(None),
            Ok(_) => match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => Err(io_error("inspect native Cargo configuration", None, error)),
                Ok(_) => bounded_bytes(&path, 1 << 20),
            },
            Err(error) => Err(error),
        },
    };
    let bytes = match input {
        Ok(bytes) => bytes,
        Err(error) => {
            issues.push(format!("native Cargo configuration `{}` could not be prepared: {error}; Cargo receives an explicit configuration error, while the compiler remains available. Repair the local file/permissions or use direct Cargo with an explicit sandbox configuration; the next invocation refreshes without restart", path.display()));
            return Ok(Some(b"[SKOTT_SOURCE_CONFIGURATION_UNAVAILABLE\n".to_vec()));
        }
    };
    let mut table = match std::str::from_utf8(&bytes)
        .ok()
        .and_then(|text| toml::from_str::<toml::Table>(text).ok())
    {
        Some(table) => table,
        None => {
            issues.push(format!("native Cargo configuration `{}` is malformed/non-UTF-8; its original bytes are preserved for Cargo's own diagnostic, not used to disable rustc. Correct that file and retry the affected Cargo operation without restart", path.display()));
            return Ok(Some(bytes));
        }
    };
    if let Some(sources) = table.get_mut("source").and_then(toml::Value::as_table_mut) {
        for source in sources
            .iter_mut()
            .filter_map(|(_, value)| value.as_table_mut())
        {
            if source.get("directory").is_some_and(toml::Value::is_str) {
                source.insert(
                    "directory".into(),
                    toml::Value::String("/rust/vendor".into()),
                );
            }
        }
    }
    toml::to_string(&table)
        .map(|text| Some(text.into_bytes()))
        .map_err(|error| {
            failure(format!(
                "cannot serialize native Cargo configuration: {error}"
            ))
        })
}

impl RustEnvironment {
    /// Resolves installed registration topology and a live read-only vendor view.
    /// Ambient Cargo/Rust configuration and rustup overrides are not
    /// forwarded. Absence is an error for callers that explicitly enable Rust.
    pub fn resolve(workspace: &Path) -> Result<Self> {
        Self::resolve_cancellable(workspace, &CancellationToken::new())
    }

    pub(crate) fn resolve_cancellable(
        workspace: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Self> {
        Self::resolve_observed(workspace, cancellation, |_| {})
    }

    fn resolve_observed(
        workspace: &Path,
        cancellation: &CancellationToken,
        mut selected_root: impl FnMut(&Path),
    ) -> Result<Self> {
        let preparation = Preparation {
            started: Instant::now(),
            cancellation,
        };
        let workspace = workspace
            .canonicalize()
            .map_err(|e| io_error("resolve Rust workspace", None, e))?;
        if !inspect_path(&workspace)?.is_dir() {
            return Err(failure("Rust workspace must be a regular directory"));
        }
        tracing::debug!(workspace = %workspace.display(), "preparing offline Rust resources");
        let mut topology = live::Topology::capture(&workspace)?;
        let pin = project_pin(&workspace)?;
        let pinned = pin.is_some();
        let pin_identity = pin_identity(&workspace)?;
        let host::Selection {
            rustup,
            home,
            selected,
            cargo: _,
            root,
            installed_name,
        } = host::resolve(&workspace, pin.as_ref(), &preparation)?;
        selected_root(&root);
        topology.observe_root(&root)?;
        tracing::debug!(
            requested_channel = %selected,
            installed_channel = %installed_name,
            root = %root.display(),
            pinned,
            "resolved concrete installed Rust toolchain"
        );
        let mut executables = Vec::new();
        for name in ["cargo", "rustc"] {
            let path = root.join("bin").join(name);
            executables.push(track_executable(path, &preparation)?);
        }
        executables.push(track_executable(rustup.clone(), &preparation)?);
        let inventory =
            inventory::resolve(&home, &workspace, &installed_name, &rustup, &preparation)?;
        let mut issues = inventory.diagnostics.clone();
        let user_cargo_bin = home.join(".cargo").join("bin");
        let user_cargo_bin = match host::operator_selector(&user_cargo_bin, &workspace) {
            Err(error) => {
                issues.push(format!("optional Cargo extension selector cannot be authorized: {error}; no outside origin is mounted, compiler tools remain usable"));
                None
            }
            Ok(false) => {
                issues.push("optional Cargo extension selector is writable in this workspace; no outside target is followed or mounted. Compiler tools remain usable; keep host installation selectors outside the work scope to expose extensions".into());
                None
            }
            Ok(true) => match user_cargo_bin.canonicalize() {
                Ok(path)
                    if !path.starts_with(&workspace)
                        && !workspace.starts_with(&path)
                        && !home.starts_with(&path)
                        && !home.join(".cargo").starts_with(&path) =>
                {
                    match inspect_path(&path) {
                        Ok(metadata) if metadata.is_dir() => Some(path),
                        Ok(_) => {
                            issues.push("optional Cargo extension origin is no longer a directory; compiler tools remain available. Repair ~/.cargo/bin and retry the extension".into());
                            None
                        }
                        Err(error) => {
                            issues.push(format!("optional Cargo extensions cannot be inspected: {error}; compiler tools remain available. Repair ~/.cargo/bin and retry the extension"));
                            None
                        }
                    }
                }
                Ok(_) => {
                    issues.push("optional user Cargo extension directory is not usable outside the write scope; compiler tools remain available. Correct ~/.cargo/bin to restore extensions".into());
                    None
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    issues.push(format!("optional Cargo extensions unavailable: {error}; selected compiler remains usable. Repair ~/.cargo/bin to restore extensions"));
                    None
                }
            },
        };
        let has_user_cargo_bin = user_cargo_bin.is_some();
        let user_cargo_bin_path = user_cargo_bin
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "none".into());
        let staging = tempfile::Builder::new()
            .prefix(".skott-rust-")
            .tempdir_in(
                workspace
                    .parent()
                    .ok_or_else(|| failure("Rust staging requires a writable workspace parent"))?,
            )
            .map_err(|e| io_error("create private Rust resources outside workspace", None, e))?;
        fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o700))
            .map_err(|e| io_error("protect Rust resource staging", None, e))?;
        let vendor = workspace.join(".kvist/vendored");
        let state_usable = match fs::symlink_metadata(workspace.join(".kvist")) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => true,
            Ok(_) => {
                issues.push("workspace .kvist is not an ordinary directory; no host origin is followed through that pointer. Installed compiler/local operations remain available; provide ordinary local vendor/configuration nodes for dependency-using operations".into());
                false
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                issues.push(format!("workspace vendor/configuration state could not be inspected: {error}; compiler operations remain available; repair local state permissions and retry affected builds"));
                false
            }
        };
        let vendor_metadata = if state_usable {
            match fs::symlink_metadata(&vendor) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    Some(metadata)
                }
                Ok(_) => {
                    issues.push("workspace vendor is not an ordinary directory; no extra origin is mounted. Compiler/local operations remain available; provide an ordinary vendor directory for dependency-using builds".into());
                    None
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    issues.push(format!("workspace vendor could not be inspected: {error}; compiler/local operations remain available; repair the vendor path before dependency-using builds"));
                    None
                }
            }
        } else {
            None
        };
        let has_vendor = vendor_metadata.is_some();
        let vendor_identity = if has_vendor {
            let metadata = vendor_metadata
                .as_ref()
                .ok_or_else(|| failure("vendor metadata is unavailable"))?;
            label(format!("{}:{}", metadata.dev(), metadata.ino()).as_bytes())
        } else {
            fs::create_dir(staging.path().join("vendor"))
                .map_err(|e| io_error("create empty offline vendor resource", None, e))?;
            label(b"empty offline authoring vendor")
        };
        tracing::debug!(
            has_vendor,
            vendor_identity = %vendor_identity,
            elapsed_ms = preparation.started.elapsed().as_millis() as u64,
            "offline shared vendor view prepared"
        );
        fs::create_dir(staging.path().join("runtime"))
            .map_err(|e| io_error("create private Rust runtime", None, e))?;
        fs::create_dir(staging.path().join("runtime/bin"))
            .map_err(|e| io_error("create private Rust wrappers", None, e))?;
        if let Some(contents) = if state_usable {
            source_configuration(&workspace, &mut issues)?
        } else {
            None
        } {
            let staged = staging.path().join("runtime/vendor-config.toml");
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o400)
                .open(&staged)
                .map_err(|error| io_error("create private offline source config", None, error))?;
            file.write_all(&contents)
                .and_then(|_| file.sync_all())
                .map_err(|error| io_error("write private offline source config", None, error))?;
            executables.push(hash_file(&staged, &preparation)?.0);
        }
        let initialization = staging.path().join("runtime/initialize");
        fs::write(&initialization, format!("{}mkdir -p /tmp/cargo-home\nif test -f /rust/runtime/vendor-config.toml; then cp /rust/runtime/vendor-config.toml /tmp/cargo-home/config.toml; fi\n", sav::offline_rust::INITIALIZE))
            .map_err(|e| io_error("write sandbox-only Rust initialization", None, e))?;
        executables.push(hash_file(&initialization, &preparation)?.0);
        for executable in sav::offline_rust::TOOLS {
            let wrapper = staging.path().join("runtime/bin").join(executable.name());
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o500)
                .open(&wrapper)
                .map_err(|e| io_error("create trusted Rust runtime wrapper", None, e))?;
            let script = sav::offline_rust::workspace_wrapper(*executable);
            file.write_all(script.as_bytes())
                .map_err(|e| io_error("write fixed Rust runtime wrapper", None, e))?;
            file.sync_all()
                .map_err(|e| io_error("synchronize Rust runtime wrapper", None, e))?;
            executables.push(hash_file(&wrapper, &preparation)?.0);
        }
        std::os::unix::fs::symlink("/rust/rustup", staging.path().join("runtime/bin/rustup"))
            .map_err(|error| io_error("create private Rustup PATH facade", None, error))?;

        let rustup_home = staging.path().join("rustup-home");
        let toolchains = rustup_home.join("toolchains");
        fs::create_dir_all(&toolchains)
            .map_err(|e| io_error("create private rustup registration", None, e))?;
        // The target deliberately names the sandbox mount, never the host root.
        for installation in &inventory.installations {
            std::os::unix::fs::symlink(
                &installation.destination,
                toolchains.join(&installation.name),
            )
            .map_err(|e| io_error("register installed sandbox toolchain", None, e))?;
        }
        let settings = rustup_home.join("settings.toml");
        fs::write(
            &settings,
            format!(
                "version = \"12\"\ndefault_toolchain = {}\nprofile = \"minimal\"\n",
                toml::Value::String(installed_name.clone())
            ),
        )
        .map_err(|e| io_error("write private rustup selection", None, e))?;
        executables.push(hash_file(&settings, &preparation)?.0);
        let identity = label(
            format!(
                "kvist/authoring-rust/v1\0{installed_name}\0{}\0{}\0{executables:?}\0{inventory:?}",
                root.display(),
                user_cargo_bin_path
            )
            .as_bytes(),
        );
        tracing::debug!(
            channel = %installed_name,
            identity = %identity,
            tracked_files = executables.len(),
            installed_toolchains = inventory.installations.len(),
            user_cargo_bin = has_user_cargo_bin,
            elapsed_ms = preparation.started.elapsed().as_millis() as u64,
            "offline Rust resources prepared"
        );
        Ok(Self {
            resources: Arc::new(Resources {
                workspace,
                root,
                rustup,
                channel: installed_name,
                identity,
                staging,
                vendor_identity,
                has_vendor,
                pin_identity,
                pinned,
                user_cargo_bin,
                rustup_home,
                inventory,
                topology,
                issues,
            }),
        })
    }

    /// Short, human-readable startup diagnostic suitable for the initial
    /// transcript. Omits the long SHA-based identity details.
    pub fn short_diagnostic(&self) -> String {
        if self.resources.pinned {
            format!(
                "Rust: {} (project pin; {})",
                self.resources.channel,
                if self.resources.has_vendor {
                    "live vendor available"
                } else {
                    "no vendor; local dependencies remain usable"
                }
            )
        } else if self.resources.has_vendor {
            format!(
                "Rust: {} (offline; live vendor available)",
                self.resources.channel
            )
        } else {
            format!(
                "Rust: {} (offline; no vendor; local dependencies remain usable)",
                self.resources.channel
            )
        }
    }

    /// An inspectable exact selection, identity and offline-material diagnostic.
    pub fn diagnostic(&self) -> String {
        let mut diagnostic = format!(
            "Rust authoring: {} selects installed `{}` at `{}`; preparation identity {}; pin digest {}; network denied, workspace Cargo offline without forced locked resolution, private HOME/cache/target; {}; vendor view {}. Rustup exposes {} installed registrations through live read-only mounts; choose with cargo +<name>, rustup run or invocation-local rustup default (a project pin takes precedence). Host changes are legitimate inputs; topology/configuration refresh on subsequent tools without restart, receipt or reader lease. Missing packages/components affect only the commands needing them. No host home/Cargo credentials mounted; ambient Cargo/Rust overrides ignored.",
            if self.resources.pinned {
                "project pin"
            } else {
                "live host default (no project pin)"
            },
            self.resources.channel,
            self.resources.root.display(),
            self.resources.identity,
            self.resources.pin_identity,
            if self.resources.has_vendor {
                "shared read-only workspace .kvist/vendored"
            } else {
                "no vendored registry: only dependency-free/local-path builds can resolve; provision dependencies on the host"
            },
            self.resources.vendor_identity,
            self.resources.inventory.installations.len()
        );
        for issue in &self.resources.issues {
            diagnostic.push('\n');
            diagnostic.push_str(issue);
        }
        diagnostic
    }

    pub(crate) fn validate(
        &self,
        workspace: &Path,
        cancellation: &CancellationToken,
    ) -> Result<()> {
        crate::executor::check_cancelled(cancellation)?;
        if workspace != self.resources.workspace {
            return Err(failure(
                "resolved Rust resources belong to a different writable workspace",
            ));
        }
        Ok(())
    }

    pub(crate) fn grants(&self) -> Vec<Grant> {
        let mut grants = vec![
            Grant {
                source: self.resources.root.to_string_lossy().into_owned(),
                destination: TOOLCHAIN_DEST.into(),
                access: Access::ReadOnly,
                purpose: Purpose::Toolchain,
                identity: self.resources.identity.clone(),
            },
            Grant {
                source: self
                    .resources
                    .staging
                    .path()
                    .join("runtime")
                    .to_string_lossy()
                    .into_owned(),
                destination: RUNTIME_DEST.into(),
                access: Access::ReadOnly,
                purpose: Purpose::Toolchain,
                identity: self.resources.identity.clone(),
            },
            Grant {
                source: self.resources.rustup_home.to_string_lossy().into_owned(),
                destination: RUSTUP_HOME_DEST.into(),
                access: Access::ReadOnly,
                purpose: Purpose::Toolchain,
                identity: self.resources.identity.clone(),
            },
            Grant {
                source: self.vendor_path().to_string_lossy().into_owned(),
                destination: "/rust/vendor".into(),
                access: Access::ReadOnly,
                purpose: Purpose::Toolchain,
                identity: self.resources.vendor_identity.clone(),
            },
        ];
        if let Some(user_cargo_bin) = &self.resources.user_cargo_bin {
            grants.push(Grant {
                source: user_cargo_bin.to_string_lossy().into_owned(),
                destination: "/rust/user-cargo-bin".into(),
                access: Access::ReadOnly,
                purpose: Purpose::Toolchain,
                identity: self.resources.identity.clone(),
            });
        }
        grants.push(Grant {
            source: self.resources.rustup.to_string_lossy().into_owned(),
            destination: "/rust/rustup".into(),
            access: Access::ReadOnly,
            purpose: Purpose::Toolchain,
            identity: self.resources.identity.clone(),
        });
        for installation in &self.resources.inventory.installations {
            if installation.destination != TOOLCHAIN_DEST {
                grants.push(Grant {
                    source: installation.root.to_string_lossy().into_owned(),
                    destination: installation.destination.clone(),
                    access: Access::ReadOnly,
                    purpose: Purpose::Toolchain,
                    identity: self.resources.identity.clone(),
                });
            }
        }
        grants
    }

    fn vendor_path(&self) -> PathBuf {
        if self.resources.has_vendor {
            self.resources.workspace.join(".kvist/vendored")
        } else {
            self.resources.staging.path().join("vendor")
        }
    }

    pub(crate) fn identity(&self) -> &str {
        &self.resources.identity
    }

    pub(crate) fn environment(&self) -> BTreeMap<String, String> {
        let mut path = vec!["/rust/runtime/bin", "/rust/toolchain/bin"];
        if self.resources.user_cargo_bin.is_some() {
            path.push("/rust/user-cargo-bin");
        }
        path.extend(["/usr/bin", "/bin"]);
        for directory in crate::toolchain::SANDBOX_SYSTEM_PATH.split(':') {
            if !path.contains(&directory) {
                path.push(directory);
            }
        }
        let mut environment: BTreeMap<String, String> = [
            ("HOME", "/tmp"),
            ("RUSTUP_HOME", "/tmp/rustup-home"),
            ("RUSTUP_AUTO_INSTALL", "0"),
            ("CARGO_HOME", "/tmp/cargo-home"),
            ("CARGO_TARGET_DIR", "/workspace/target"),
            ("CARGO_NET_OFFLINE", "true"),
            ("RUSTC", "/rust/runtime/bin/rustc"),
            ("RUSTDOC", "/rust/runtime/bin/rustdoc"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        environment.insert("PATH".into(), path.join(":"));
        if self.resources.pinned {
            environment.insert("RUSTUP_TOOLCHAIN".into(), self.resources.channel.clone());
        }
        environment
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn preparation_timeout_reports_the_actual_180_second_budget() {
        let cancellation = CancellationToken::new();
        let preparation = Preparation {
            started: Instant::now() - PREPARATION_BUDGET,
            cancellation: &cancellation,
        };
        let error = preparation.check().unwrap_err();
        assert!(error.to_string().contains("180 seconds"), "{error}");
    }

    #[test]
    fn a_host_home_launcher_does_not_require_system_installation_or_home_mounts() {
        let directory = fixture();
        let workspace = directory.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let launcher = directory.path().join("host-rustup");
        fs::write(&launcher, "fixture launcher").unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!(
            host::select_rustup(std::slice::from_ref(&launcher), &workspace).unwrap(),
            launcher.canonicalize().unwrap()
        );
        assert!(host::select_rustup(std::slice::from_ref(&launcher), directory.path()).is_err());
    }

    #[test]
    fn trusted_linked_registration_is_usable_but_cannot_point_into_write_scope() {
        let directory = fixture();
        let home = directory.path().join("home");
        let workspace = directory.path().join("workspace");
        let installed = directory.path().join("installed");
        fs::create_dir_all(home.join(".rustup/toolchains")).unwrap();
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&installed).unwrap();
        let link = home.join(".rustup/toolchains/stable");
        symlink(&installed, &link).unwrap();
        assert_eq!(
            registered_root(&home, &workspace, "stable").unwrap(),
            installed.canonicalize().unwrap()
        );
        fs::remove_file(&link).unwrap();
        symlink(&workspace, &link).unwrap();
        assert!(registered_root(&home, &workspace, "stable").is_err());
    }

    #[test]
    fn writable_host_selector_slots_cannot_authorize_outside_executables_or_roots() {
        let directory = fixture();
        let home = directory.path().join("home");
        let outside = directory.path().join("outside");
        fs::create_dir_all(home.join(".cargo")).unwrap();
        fs::create_dir_all(home.join(".rustup")).unwrap();
        fs::create_dir_all(outside.join("stable")).unwrap();
        fs::write(outside.join("rustup"), "fixture").unwrap();
        fs::set_permissions(outside.join("rustup"), fs::Permissions::from_mode(0o500)).unwrap();
        symlink(&outside, home.join(".cargo/bin")).unwrap();
        symlink(&outside, home.join(".rustup/toolchains")).unwrap();
        assert!(
            host::select_rustup(&[home.join(".cargo/bin/rustup")], &home.join(".cargo")).is_err()
        );
        assert!(registered_root(&home, &home.join(".rustup"), "stable").is_err());
    }

    fn fixture() -> tempfile::TempDir {
        // Prefer the run-time `CARGO_MANIFEST_DIR` (set by cargo) so the
        // fixture directory stays correct when a previously built test binary
        // is reused after the repository has moved; the compile-time value is
        // the fallback.
        let manifest = std::env::var("CARGO_MANIFEST_DIR")
            .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_string());
        tempfile::Builder::new()
            .prefix(".rust-resource-test-")
            .tempdir_in(manifest)
            .unwrap()
    }

    fn synthetic_environment(directory: &Path) -> RustEnvironment {
        let directory = directory.canonicalize().unwrap();
        let workspace = directory.join("workspace");
        let root = directory.join("toolchain");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&root).unwrap();
        let executable = root.join("cargo");
        fs::write(&executable, b"initial executable").unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".rust-staging-")
            .tempdir_in(&directory)
            .unwrap();
        let rustup_home = staging.path().join("rustup-home");
        fs::create_dir_all(rustup_home.join("toolchains")).unwrap();
        symlink(TOOLCHAIN_DEST, rustup_home.join("toolchains/stable")).unwrap();
        let topology = live::Topology::capture(&workspace).unwrap();
        RustEnvironment {
            resources: Arc::new(Resources {
                pin_identity: pin_identity(&workspace).unwrap(),
                workspace,
                root: root.clone(),
                rustup: root.join("cargo"),
                channel: "stable".into(),
                identity: label(b"synthetic"),
                staging,
                vendor_identity: label(b"empty"),
                has_vendor: false,
                pinned: false,
                user_cargo_bin: None,
                rustup_home,
                inventory: inventory::Inventory {
                    diagnostics: Vec::new(),
                    installations: vec![inventory::Installation {
                        name: "stable".into(),
                        root: directory.join("toolchain"),
                        destination: TOOLCHAIN_DEST.into(),
                    }],
                },
                topology,
                issues: Vec::new(),
            }),
        }
    }

    #[test]
    fn shared_vendor_grant_uses_the_project_tree_not_a_package_copy() {
        let directory = fixture();
        let mut environment = synthetic_environment(directory.path());
        let vendor = environment.resources.workspace.join(".kvist/vendored");
        fs::create_dir_all(&vendor).unwrap();
        Arc::get_mut(&mut environment.resources).unwrap().has_vendor = true;
        let resource = environment
            .grants()
            .into_iter()
            .find(|resource| resource.destination == "/rust/vendor")
            .unwrap();
        assert_eq!(Path::new(&resource.source), vendor);
        assert_eq!(resource.access, Access::ReadOnly);
        assert!(!environment.resources.staging.path().join("vendor").exists());
    }

    #[test]
    fn shared_vendor_changes_do_not_require_read_ownership_or_restart() {
        let directory = fixture();
        let mut environment = synthetic_environment(directory.path());
        let vendor = environment.resources.workspace.join(".kvist/vendored");
        fs::create_dir_all(&vendor).unwrap();
        let resources = Arc::get_mut(&mut environment.resources).unwrap();
        resources.has_vendor = true;
        let writer = File::open(&vendor).unwrap();
        writer.try_lock().unwrap();
        drop(writer);
        let writer = File::open(&vendor).unwrap();
        writer.try_lock().unwrap();
        drop(writer);
        fs::rename(&vendor, vendor.with_file_name("retired-vendor")).unwrap();
        fs::create_dir(&vendor).unwrap();
        assert!(
            environment
                .validate(&environment.resources.workspace, &CancellationToken::new())
                .is_ok()
        );
    }

    #[test]
    fn live_lock_replacement_does_not_require_restart() {
        let directory = fixture();
        let environment = synthetic_environment(directory.path());
        let lock = environment.resources.workspace.join("Cargo.lock");
        fs::write(&lock, "version=4\n").unwrap();
        environment
            .validate(&environment.resources.workspace, &CancellationToken::new())
            .unwrap();
        let replacement = lock.with_file_name("replacement.lock");
        fs::write(&replacement, "version=4\n").unwrap();
        fs::rename(replacement, lock).unwrap();
        environment
            .validate(&environment.resources.workspace, &CancellationToken::new())
            .unwrap();
    }

    #[test]
    fn prepared_rust_path_preserves_system_discovery_and_existing_precedence() {
        for with_cargo_extensions in [false, true] {
            let directory = fixture();
            let mut environment = synthetic_environment(directory.path());
            let mut expected = vec!["/rust/runtime/bin", "/rust/toolchain/bin"];
            if with_cargo_extensions {
                Arc::get_mut(&mut environment.resources)
                    .unwrap()
                    .user_cargo_bin = Some(directory.path().join("cargo-bin"));
                expected.push("/rust/user-cargo-bin");
            }
            expected.extend([
                "/usr/bin",
                "/bin",
                "/usr/local/bin",
                "/usr/local/sbin",
                "/usr/sbin",
            ]);
            let variables = environment.environment();
            assert_eq!(
                variables["PATH"].split(':').collect::<Vec<_>>(),
                expected,
                "prepared Rust must preserve System discovery without changing existing precedence"
            );
        }
    }

    #[test]
    fn host_changes_are_not_a_mounted_resource_breach() {
        let directory = fixture();
        let environment = synthetic_environment(directory.path());
        let workspace = &environment.resources.workspace;
        let cancellation = CancellationToken::new();
        environment.validate(workspace, &cancellation).unwrap();
        fs::write(
            environment.resources.root.join("cargo"),
            b"substituted executable",
        )
        .unwrap();
        environment.validate(workspace, &cancellation).unwrap();
        fs::rename(
            &environment.resources.root,
            directory.path().join("old-toolchain"),
        )
        .unwrap();
        fs::create_dir(&environment.resources.root).unwrap();
        environment.validate(workspace, &cancellation).unwrap();
        fs::remove_dir(&environment.resources.root).unwrap();
        symlink(
            directory
                .path()
                .join("old-toolchain")
                .canonicalize()
                .unwrap(),
            &environment.resources.root,
        )
        .unwrap();
        environment.validate(workspace, &cancellation).unwrap();
    }

    #[test]
    fn resources_are_read_only_disjoint_and_cleaned_with_final_owner() {
        let directory = fixture();
        let environment = synthetic_environment(directory.path());
        let staging = environment.resources.staging.path().to_owned();
        let clone = environment.clone();
        for grant in environment.grants() {
            assert_eq!(grant.access, Access::ReadOnly);
            assert_eq!(grant.purpose, Purpose::Toolchain);
            assert!(!Path::new(&grant.source).starts_with(&environment.resources.workspace));
            assert!(!grant.destination.starts_with("/workspace"));
        }
        assert!(environment.grants().iter().any(|grant| {
            grant.destination == "/rust/vendor"
                && grant.source == staging.join("vendor").to_string_lossy()
                && grant.access == Access::ReadOnly
        }));
        drop(environment);
        assert!(staging.exists());
        drop(clone);
        assert!(!staging.exists());
    }

    #[test]
    fn registration_uses_private_home_without_a_host_edit_integrity_gate() {
        let directory = fixture();
        let environment = synthetic_environment(directory.path());
        let variables = environment.environment();
        assert_eq!(variables["HOME"], "/tmp");
        assert_eq!(variables["RUSTUP_HOME"], "/tmp/rustup-home");
        assert!(!variables.contains_key("RUSTUP_TOOLCHAIN"));
        assert_eq!(variables["RUSTUP_AUTO_INSTALL"], "0");
        let link = environment.resources.rustup_home.join("toolchains/stable");
        fs::remove_file(&link).unwrap();
        symlink("/home/hidden/toolchain", &link).unwrap();
        assert!(
            environment
                .validate(&environment.resources.workspace, &CancellationToken::new())
                .is_ok()
        );
    }

    #[test]
    fn host_target_and_registration_updates_do_not_trip_integrity_checks() {
        for change in [
            "library",
            "target-added",
            "installation-added",
            "registration-added",
            "registration-replaced",
        ] {
            let directory = fixture();
            let mut environment = synthetic_environment(directory.path());
            let parent = directory.path().canonicalize().unwrap();
            let alternate = parent.join("alternate");
            let libraries = alternate.join("lib");
            fs::create_dir_all(&libraries).unwrap();
            let library = libraries.join("libstd.rlib");
            fs::write(&library, b"installed target").unwrap();
            let resources = Arc::get_mut(&mut environment.resources).unwrap();
            resources
                .inventory
                .installations
                .push(inventory::Installation {
                    name: "nightly".into(),
                    root: alternate.clone(),
                    destination: "/rust/toolchains/1".into(),
                });
            let registrations = resources.rustup_home.join("toolchains");
            symlink("/rust/toolchains/1", registrations.join("nightly")).unwrap();
            environment
                .validate(&environment.resources.workspace, &CancellationToken::new())
                .unwrap();
            match change {
                "library" => fs::write(&library, b"changed target").unwrap(),
                "target-added" => fs::create_dir(libraries.join("new-target")).unwrap(),
                "installation-added" => fs::create_dir(parent.join("new-installation")).unwrap(),
                "registration-added" => {
                    symlink("/rust/toolchain", registrations.join("extra")).unwrap()
                }
                _ => {
                    fs::remove_file(registrations.join("nightly")).unwrap();
                    symlink("/host/not-mounted", registrations.join("nightly")).unwrap();
                }
            }
            assert!(
                environment
                    .validate(&environment.resources.workspace, &CancellationToken::new())
                    .is_ok(),
                "{change}"
            );
        }
    }

    #[test]
    fn companion_executables_reject_missing_nonexecutable_links_and_directories() {
        let directory = fixture();
        let cancellation = CancellationToken::new();
        let preparation = Preparation {
            started: Instant::now(),
            cancellation: &cancellation,
        };
        let path = directory
            .path()
            .canonicalize()
            .unwrap()
            .join("cargo-clippy");
        assert!(track_executable(path.clone(), &preparation).is_err());
        fs::write(&path, b"binary").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(track_executable(path.clone(), &preparation).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(track_executable(path.clone(), &preparation).is_ok());
        fs::remove_file(&path).unwrap();
        symlink("/usr/bin/true", &path).unwrap();
        assert!(track_executable(path.clone(), &preparation).is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(track_executable(path, &preparation).is_err());
    }

    #[test]
    fn live_executable_updates_do_not_trip_fingerprint_checks() {
        let directory = fixture();
        let environment = synthetic_environment(directory.path());
        let workspace = &environment.resources.workspace;
        let cancellation = CancellationToken::new();
        environment.validate(workspace, &cancellation).unwrap();
        // Same length, different bytes: a size-only check would pass, so the
        // per-call drift detection must rely on the full fingerprint.
        fs::write(
            environment.resources.root.join("cargo"),
            b"hijacked executable!",
        )
        .unwrap();
        environment.validate(workspace, &cancellation).unwrap();
    }

    #[test]
    fn resource_hashes_have_no_general_file_cap_and_remain_cancellable() {
        let directory = fixture();
        let path = directory.path().join("large");
        File::create(&path)
            .unwrap()
            .set_len((256 << 20) + 1)
            .unwrap();
        hash_file(
            &path,
            &Preparation {
                started: Instant::now(),
                cancellation: &CancellationToken::new(),
            },
        )
        .unwrap();
        let environment = synthetic_environment(directory.path());
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(
            environment
                .validate(&environment.resources.workspace, &cancellation)
                .is_err()
        );
    }

    #[test]
    fn native_registration_tokens_accept_custom_names_but_not_paths_or_options() {
        for value in [
            "stable",
            "beta",
            "nightly-2026-01-01",
            "1.95",
            "1.95.0",
            "stable-x86_64-unknown-linux-gnu",
            "custom-host",
            "custom..host",
            "stable;echo",
        ] {
            channel(value).unwrap();
        }
        for value in [
            "",
            "--help",
            "/host/root",
            "../../toolchain",
            "stable\n",
            "stable\\root",
        ] {
            assert!(channel(value).is_err(), "{value}");
        }
    }
}
