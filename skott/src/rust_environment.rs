//! Host-resolved, read-only Rust resources for the workspace authoring sandbox.
//!
//! This is not the maerg provisioning or Cargo verification authority. It
//! never installs anything, reads Cargo credentials, or trusts manifest paths.
//! The existing System/Authoring protocol grants the compiler, fixed Cargo shim
//! and dependency snapshot as read-only Toolchain build resources at disjoint
//! `/rust/*` destinations. No Cargo-phase topology or cache authority is added.
//! A snapshot is essential: a read-only alias of `.kvist/vendored` would still
//! be mutable through the workspace's writable bind.
//!
//! Selection ignores ambient Rust/Cargo overrides and uses the standard
//! `$HOME/.rustup/toolchains` installation layout. Channels, requested
//! components/targets and executable/layout identities are checked without
//! installing anything. Missing material must be provisioned separately on the
//! host. The snapshot is bounded to 1 GiB, 100,000 entries and 30 seconds;
//! scratch target/cache state is private to each sandbox invocation.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
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

#[path = "rust_environment/inventory.rs"]
mod inventory;

const TOOLCHAIN_DEST: &str = "/rust/toolchain";
const RUNTIME_DEST: &str = "/rust/runtime";
const VENDOR_DEST: &str = "/rust/vendor";
const RUSTUP_HOME_DEST: &str = "/rust/rustup-home";
const MAX_FILE_BYTES: u64 = 256 << 20;
const MAX_VENDOR_BYTES: u64 = 1 << 30;
const MAX_ENTRIES: usize = 100_000;
const PREPARATION_BUDGET: Duration = Duration::from_secs(30);

/// A complete installed inventory, initial selection and private sandbox resources.
///
/// Clones share the staging owner; the final registry/executor drop removes
/// staged wrappers and vendor bytes. No installed toolchain is modified.
#[derive(Debug, Clone)]
pub struct RustEnvironment {
    resources: Arc<Resources>,
}

/// An installed executable bound at resolve time to its exact bytes.
///
/// `identity` is the authoritative SHA-256 of the file contents captured once,
/// and it rides on the sandbox grant identity for every call. Per-call
/// validation compares the cheap `fingerprint` (dev/ino/size/mtime/ctime) only;
/// any modification, truncation or replacement updates at least one of those
/// fields, so re-reading and re-hashing hundreds of megabytes per tool call is
/// unnecessary to keep drift detection strict.
#[derive(Debug)]
struct TrackedExecutable {
    path: PathBuf,
    /// Read only through the environment identity's `Debug` projection, which
    /// binds every executable's exact bytes into the grant identity.
    #[allow(dead_code)]
    identity: String,
    fingerprint: Fingerprint,
}

#[derive(Debug)]
struct Resources {
    workspace: PathBuf,
    root: PathBuf,
    channel: String,
    identity: String,
    executables: Vec<TrackedExecutable>,
    directories: Vec<(PathBuf, (u64, u64))>,
    staging: tempfile::TempDir,
    vendor_identity: String,
    has_vendor: bool,
    pin_identity: String,
    pinned: bool,
    user_cargo_bin: Option<PathBuf>,
    rustup_home: PathBuf,
    inventory: inventory::Inventory,
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

/// One copied vendor file's SHA-256 digest bytes and copied byte count.
type FileDigest = (Vec<u8>, u64);
/// The copy-and-digest outcome of one scheduled vendor file.
type CopyOutcome = Result<FileDigest>;

struct Preparation<'a> {
    started: Instant,
    cancellation: &'a CancellationToken,
}

impl Preparation<'_> {
    fn check(&self) -> Result<()> {
        crate::executor::check_cancelled(self.cancellation)?;
        if self.started.elapsed() >= PREPARATION_BUDGET {
            return Err(failure(
                "Rust preparation exceeded 30 seconds; narrow the workspace/vendor tree",
            ));
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
    let metadata = inspect_path(path)?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(failure(format!(
            "Rust resource `{}` must be a bounded regular file (maximum {maximum} bytes)",
            path.display()
        )));
    }
    let opened = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| io_error("open Rust resource", Some(&path.to_string_lossy()), e))?;
    let actual = opened
        .metadata()
        .map_err(|e| io_error("inspect opened Rust resource", None, e))?;
    if !actual.is_file() || (metadata.dev(), metadata.ino()) != (actual.dev(), actual.ino()) {
        return Err(failure("Rust resource changed while opening"));
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

fn hash_file(path: &Path, preparation: &Preparation<'_>) -> Result<(String, Fingerprint)> {
    let mut opened = file(path, MAX_FILE_BYTES)?;
    let before = opened
        .metadata()
        .map_err(|e| io_error("inspect Rust identity", None, e))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut total = 0_u64;
    loop {
        preparation.check()?;
        let count = opened
            .read(&mut buffer)
            .map_err(|e| io_error("hash Rust executable", None, e))?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_FILE_BYTES {
            return Err(failure(
                "Rust executable grew beyond the 256 MiB identity bound",
            ));
        }
        hash.update(&buffer[..count]);
    }
    let after = inspect_path(path)?;
    if fingerprint(&before) != fingerprint(&after) {
        return Err(failure("Rust executable changed while hashing"));
    }
    Ok((
        format!("sha256:{}", hex::encode(hash.finalize())),
        fingerprint(&before),
    ))
}

fn track_executable(path: PathBuf, preparation: &Preparation<'_>) -> Result<TrackedExecutable> {
    if inspect_path(&path)?.permissions().mode() & 0o111 == 0 {
        return Err(failure(format!(
            "installed Rust executable `{}` is not executable",
            path.display()
        )));
    }
    let (identity, fingerprint) = hash_file(&path, preparation)?;
    tracing::trace!(path = %path.display(), %identity, "Rust executable validated");
    Ok(TrackedExecutable {
        path,
        identity,
        fingerprint,
    })
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
    let base = value.split('-').next().unwrap_or_default();
    let release = base.split('.').collect::<Vec<_>>();
    if !token(value)
        || !(matches!(base, "stable" | "beta" | "nightly")
            || ((2..=3).contains(&release.len())
                && release
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))))
    {
        return Err(failure(
            "Rust channel must be a rustup release channel/version, never a path, option or custom linked toolchain",
        ));
    }
    Ok(())
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

fn native_library_layout(root: &Path) -> Result<Vec<PathBuf>> {
    let manifest = root.join("lib/rustlib/components");
    let bytes = bounded_bytes(&manifest, 65536)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| failure("installed Rust component manifest must be UTF-8"))?;
    let native = text
        .lines()
        .filter_map(|s| s.strip_prefix("rustc-"))
        .collect::<Vec<_>>();
    if native.len() != 1 || !token(native[0]) {
        return Err(failure(
            "installed Rust lacks one native compiler component; repair/provision it on the host",
        ));
    }
    let directory = root.join("lib/rustlib").join(native[0]).join("lib");
    if !inspect_path(&directory)?.is_dir() {
        return Err(failure(
            "installed Rust lacks its native standard library; provision it on the host",
        ));
    }
    let mut core = None;
    let mut std = None;
    let mut entries = 0;
    for entry in
        fs::read_dir(&directory).map_err(|e| io_error("inspect native Rust libraries", None, e))?
    {
        entries += 1;
        if entries > 1024 {
            return Err(failure(
                "native Rust library directory exceeds 1024 entries",
            ));
        }
        let entry = entry.map_err(|e| io_error("enumerate native Rust libraries", None, e))?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| failure("native Rust library names must be UTF-8"))?;
        if name.ends_with(".rlib") && (name.starts_with("libcore-") || name.starts_with("libstd-"))
        {
            if !inspect_path(&entry.path())?.is_file() {
                return Err(failure(
                    "native Rust libraries must be regular non-link files",
                ));
            }
            let slot = if name.starts_with("libcore-") {
                &mut core
            } else {
                &mut std
            };
            if slot.replace(entry.path()).is_some() {
                return Err(failure(
                    "installed Rust has ambiguous native standard libraries; repair it on the host",
                ));
            }
        }
    }
    Ok(vec![
        manifest,
        core.ok_or_else(|| {
            failure("installed Rust lacks native libcore; provision rust-std on the host")
        })?,
        std.ok_or_else(|| {
            failure("installed Rust lacks native libstd; provision rust-std on the host")
        })?,
    ])
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
            "installed Rust selection is unavailable; provision the project pin on the host before starting skott (no installation or fallback is performed)",
        ));
    }
    String::from_utf8(result.stdout).map_err(|_| failure("rustup selection output must be UTF-8"))
}

impl RustEnvironment {
    /// Resolves the installed inventory and initial selection, and snapshots vendor
    /// material. Ambient Cargo/Rust configuration and rustup overrides are not
    /// forwarded. Absence is an error for callers that explicitly enable Rust.
    pub fn resolve(workspace: &Path) -> Result<Self> {
        let cancellation = CancellationToken::new();
        let preparation = Preparation {
            started: Instant::now(),
            cancellation: &cancellation,
        };
        let workspace = workspace
            .canonicalize()
            .map_err(|e| io_error("resolve Rust workspace", None, e))?;
        if !inspect_path(&workspace)?.is_dir() {
            return Err(failure("Rust workspace must be a regular directory"));
        }
        tracing::debug!(workspace = %workspace.display(), "preparing offline Rust resources");
        let pin = project_pin(&workspace)?;
        let pinned = pin.is_some();
        let pin_identity = pin_identity(&workspace)?;
        // Only the resolved system rustup is an executable authority. Neither
        // repository config nor PATH may select this host subprocess.
        let rustup = Path::new("/usr/bin/rustup")
            .canonicalize()
            .map_err(|e| io_error("resolve system rustup; provision Rust on the host", None, e))?;
        if !rustup.starts_with("/usr")
            || !inspect_path(&rustup)?.is_file()
            || rustup.starts_with(&workspace)
        {
            return Err(failure(
                "rustup must be a trusted system executable outside the writable workspace",
            ));
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            failure("HOME is required to resolve the installed host Rust selection")
        })?;
        if !inspect_path(&home)?.is_dir() || home.starts_with(&workspace) {
            return Err(failure(
                "host HOME must be a canonical non-link directory outside the writable workspace",
            ));
        }
        let selected = match &pin {
            Some(pin) => pin.channel.clone(),
            None => {
                let text =
                    query_rustup(&rustup, &home, &["show", "active-toolchain"], &preparation)?;
                text.split_whitespace()
                    .next()
                    .ok_or_else(|| failure("rustup returned no installed default selection"))?
                    .to_owned()
            }
        };
        channel(&selected)?;
        let cargo_output = query_rustup(
            &rustup,
            &home,
            &["which", "--toolchain", &selected, "cargo"],
            &preparation,
        )
        .map_err(|e| {
            failure(format!(
                "Rust `{selected}` is unavailable; provision it on the host before startup: {e}"
            ))
        })?;
        let cargo = PathBuf::from(cargo_output.trim());
        let root = cargo
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| failure("rustup returned an invalid concrete cargo path"))?
            .to_owned();
        let installed_name = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| failure("installed Rust selection name must be UTF-8"))?
            .to_owned();
        channel(&installed_name)?;
        let installations = home.join(".rustup/toolchains");
        if root.parent() != Some(installations.as_path())
            || cargo != root.join("bin/cargo")
            || root.starts_with(&workspace)
        {
            return Err(failure(
                "Rust root must be an exact installed toolchain outside the writable workspace, not a custom linked or substituted root",
            ));
        }
        tracing::debug!(
            requested_channel = %selected,
            installed_channel = %installed_name,
            root = %root.display(),
            pinned,
            "resolved concrete installed Rust toolchain"
        );
        let mut directories = Vec::new();
        for path in [
            &root,
            &root.join("bin"),
            &root.join("lib"),
            &root.join("lib/rustlib"),
        ] {
            let metadata = inspect_path(path)?;
            if !metadata.is_dir() {
                return Err(failure(
                    "installed Rust root lacks its bin/lib/rustlib layout",
                ));
            }
            directories.push((path.to_owned(), (metadata.dev(), metadata.ino())));
        }
        if let Some(pin) = &pin {
            for component in &pin.components {
                let executable = match component.as_str() {
                    "cargo" | "rustc" | "rustdoc" | "rustfmt" | "rust-analyzer" => {
                        component.as_str()
                    }
                    "clippy" => "cargo-clippy",
                    "rust-std" => continue,
                    _ => {
                        return Err(failure(format!(
                            "unsupported requested Rust component `{component}`; verify/provision it on the host"
                        )));
                    }
                };
                inspect_path(&root.join("bin").join(executable))
                    .map_err(|e| failure(format!("requested Rust component `{component}` is absent; provision it on the host: {e}")))?;
            }
            for target in &pin.targets {
                if !inspect_path(&root.join("lib/rustlib").join(target).join("lib"))?.is_dir() {
                    return Err(failure(
                        "requested Rust target is absent; provision it on the host",
                    ));
                }
            }
        }
        let mut executables = Vec::new();
        for name in ["cargo", "rustc", "rustdoc"] {
            let path = root.join("bin").join(name);
            executables.push(track_executable(path, &preparation)?);
        }
        for name in ["rustfmt", "cargo-fmt", "cargo-clippy", "clippy-driver"] {
            let path = root.join("bin").join(name);
            let required = pin.as_ref().is_some_and(|pin| {
                pin.components.iter().any(|component| match name {
                    "rustfmt" | "cargo-fmt" => component == "rustfmt",
                    _ => component == "clippy",
                })
            });
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => continue,
                Err(error) => {
                    return Err(failure(format!(
                        "Rust companion `{name}` is unavailable; provision it on the host: {error}"
                    )));
                }
                Ok(_) => {}
            }
            executables.push(track_executable(path, &preparation)?);
        }
        for path in native_library_layout(&root)? {
            let (file_identity, file_fingerprint) = hash_file(&path, &preparation)?;
            executables.push(TrackedExecutable {
                path: path.clone(),
                identity: file_identity,
                fingerprint: file_fingerprint,
            });
        }
        for name in ["cc", "ar", "as"] {
            let path = Path::new("/usr/bin")
                .join(name)
                .canonicalize()
                .map_err(|e| io_error("resolve system Rust linker/archiver", None, e))?;
            if !path.starts_with("/usr")
                || path.starts_with(&workspace)
                || inspect_path(&path)?.permissions().mode() & 0o111 == 0
            {
                return Err(failure(
                    "Rust requires executable system cc/ar/as outside the writable workspace",
                ));
            }
            let (file_identity, file_fingerprint) = hash_file(&path, &preparation)?;
            executables.push(TrackedExecutable {
                path: path.clone(),
                identity: file_identity,
                fingerprint: file_fingerprint,
            });
        }
        executables.push(track_executable(rustup.clone(), &preparation)?);
        let inventory = inventory::resolve(
            &home,
            &workspace,
            &installed_name,
            &rustup,
            &preparation,
            &mut executables,
        )?;
        let user_cargo_bin = home.join(".cargo").join("bin");
        let has_user_cargo_bin = match fs::symlink_metadata(&user_cargo_bin) {
            Ok(_) => {
                let metadata = inspect_path(&user_cargo_bin)?;
                if !metadata.is_dir() || user_cargo_bin.starts_with(&workspace) {
                    return Err(failure(
                        "user Cargo bin must be a non-link directory outside the workspace",
                    ));
                }
                directories.push((user_cargo_bin.clone(), (metadata.dev(), metadata.ino())));
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(io_error("inspect optional user Cargo bin", None, error)),
        };

        let user_cargo_bin_path = if has_user_cargo_bin {
            user_cargo_bin.to_string_lossy().into_owned()
        } else {
            "none".to_owned()
        };
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
        match fs::symlink_metadata(workspace.join(".kvist")) {
            Ok(_) => {
                if !inspect_path(&workspace.join(".kvist"))?.is_dir() {
                    return Err(failure(
                        "workspace .kvist must be a regular non-link directory",
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error("inspect local vendor parent", None, e)),
        }
        let has_vendor = match fs::symlink_metadata(&vendor) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(io_error("inspect local vendored registry", None, e)),
        };
        let vendor_identity = if has_vendor {
            snapshot_vendor(&vendor, &staging.path().join("vendor"), &preparation)?
        } else {
            fs::create_dir(staging.path().join("vendor"))
                .map_err(|e| io_error("create empty offline vendor resource", None, e))?;
            label(b"empty offline authoring vendor")
        };
        tracing::debug!(
            has_vendor,
            vendor_identity = %vendor_identity,
            elapsed_ms = preparation.started.elapsed().as_millis() as u64,
            "offline vendor snapshot prepared"
        );
        fs::create_dir(staging.path().join("runtime"))
            .map_err(|e| io_error("create private Rust runtime", None, e))?;
        fs::create_dir(staging.path().join("runtime/bin"))
            .map_err(|e| io_error("create private Rust wrappers", None, e))?;
        let initialization = staging.path().join("runtime/initialize");
        fs::write(&initialization, sav::offline_rust::INITIALIZE)
            .map_err(|e| io_error("write sandbox-only Rust initialization", None, e))?;
        let (identity, fingerprint) = hash_file(&initialization, &preparation)?;
        executables.push(TrackedExecutable {
            path: initialization,
            identity,
            fingerprint,
        });
        for executable in sav::offline_rust::TOOLS {
            let wrapper = staging.path().join("runtime/bin").join(executable.name());
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o500)
                .open(&wrapper)
                .map_err(|e| io_error("create trusted Rust runtime wrapper", None, e))?;
            let script = sav::offline_rust::wrapper(*executable);
            file.write_all(script.as_bytes())
                .map_err(|e| io_error("write fixed Rust runtime wrapper", None, e))?;
            file.sync_all()
                .map_err(|e| io_error("synchronize Rust runtime wrapper", None, e))?;
            let (file_identity, file_fingerprint) = hash_file(&wrapper, &preparation)?;
            executables.push(TrackedExecutable {
                path: wrapper.clone(),
                identity: file_identity,
                fingerprint: file_fingerprint,
            });
        }

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
            format!("version = \"12\"\ndefault_toolchain = \"{installed_name}\"\nprofile = \"minimal\"\n"),
        )
        .map_err(|e| io_error("write private rustup selection", None, e))?;
        let (identity, fingerprint) = hash_file(&settings, &preparation)?;
        executables.push(TrackedExecutable {
            path: settings,
            identity,
            fingerprint,
        });
        for path in [&rustup_home, &toolchains] {
            let metadata = inspect_path(path)?;
            directories.push((path.to_owned(), (metadata.dev(), metadata.ino())));
        }
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
                channel: installed_name,
                identity,
                executables,
                directories,
                staging,
                vendor_identity,
                has_vendor,
                pin_identity,
                pinned,
                user_cargo_bin: if has_user_cargo_bin {
                    Some(user_cargo_bin)
                } else {
                    None
                },
                rustup_home,
                inventory,
            }),
        })
    }

    /// An inspectable exact selection, identity and offline-material diagnostic.
    pub fn diagnostic(&self) -> String {
        format!(
            "Rust authoring: {} initially selects installed `{}` at `{}`; identity {}; pin digest {}; network denied, Cargo explicitly offline and locked, private HOME/cache/target; {}; vendor snapshot {}. Rustup exposes {} installed toolchains and their targets through read-only sandbox-native registrations; choose with cargo +<name>, rustup run or invocation-local rustup default (a project pin takes precedence). Automatic installation disabled. Provision a matching Cargo.lock separately. No host home/Cargo credentials mounted; ambient Cargo/Rust overrides ignored.",
            if self.resources.pinned {
                "project pin"
            } else {
                "host default (resolved once; no project pin)"
            },
            self.resources.channel,
            self.resources.root.display(),
            self.resources.identity,
            self.resources.pin_identity,
            if self.resources.has_vendor {
                "read-only snapshot of workspace .kvist/vendored"
            } else {
                "no vendored registry: only dependency-free/local-path builds can resolve; provision dependencies on the host"
            },
            self.resources.vendor_identity,
            self.resources.inventory.installations.len()
        )
    }

    pub(crate) fn validate(
        &self,
        workspace: &Path,
        cancellation: &CancellationToken,
    ) -> Result<()> {
        let preparation = Preparation {
            started: Instant::now(),
            cancellation,
        };
        if workspace != self.resources.workspace {
            return Err(failure(
                "resolved Rust resources belong to a different writable workspace",
            ));
        }
        if pin_identity(workspace)? != self.resources.pin_identity {
            return Err(failure(
                "project Rust pin changed since startup; restart to explicitly resolve the new installed selection",
            ));
        }
        inventory::validate(&self.resources.inventory, &preparation)?;
        for (path, expected) in &self.resources.directories {
            preparation.check()?;
            let actual = inspect_path(path)?;
            if !actual.is_dir() || (actual.dev(), actual.ino()) != *expected {
                return Err(failure(
                    "installed Rust root/layout changed since startup; restart after host provisioning",
                ));
            }
        }
        for tracked in &self.resources.executables {
            preparation.check()?;
            let metadata = inspect_path(&tracked.path)?;
            if fingerprint(&metadata) != tracked.fingerprint {
                return Err(failure(
                    "installed Rust executable or trusted shim drifted since startup; restart after host provisioning",
                ));
            }
        }
        let registrations = self.resources.rustup_home.join("toolchains");
        let count = fs::read_dir(&registrations)
            .map_err(|e| io_error("recheck sandbox Rust inventory", None, e))?
            .try_fold(0_usize, |count, entry| {
                entry.map_err(|e| io_error("recheck sandbox Rust registration entry", None, e))?;
                Ok::<_, Error>(count + 1)
            })?;
        if count != self.resources.inventory.installations.len() {
            return Err(failure(
                "sandbox Rust registration inventory drifted since startup; restart",
            ));
        }
        for installation in &self.resources.inventory.installations {
            let link = registrations.join(&installation.name);
            let target = fs::read_link(&link)
                .map_err(|e| io_error("recheck sandbox Rust registration", None, e))?;
            if target != Path::new(&installation.destination) {
                return Err(failure(
                    "sandbox Rust registration drifted since startup; restart",
                ));
            }
        }
        tracing::debug!(
            channel = %self.resources.channel,
            identity = %self.resources.identity,
            tracked_files = self.resources.executables.len(),
            elapsed_ms = preparation.started.elapsed().as_millis() as u64,
            "offline Rust resources validated"
        );
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
                source: self
                    .resources
                    .staging
                    .path()
                    .join("vendor")
                    .to_string_lossy()
                    .into_owned(),
                destination: VENDOR_DEST.into(),
                access: Access::ReadOnly,
                purpose: Purpose::Toolchain,
                identity: self.resources.vendor_identity.clone(),
            },
            Grant {
                source: self.resources.rustup_home.to_string_lossy().into_owned(),
                destination: RUSTUP_HOME_DEST.into(),
                access: Access::ReadOnly,
                purpose: Purpose::Toolchain,
                identity: self.resources.identity.clone(),
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

    pub(crate) fn identity(&self) -> &str {
        &self.resources.identity
    }

    pub(crate) fn environment(&self) -> BTreeMap<String, String> {
        let path = if self.resources.user_cargo_bin.is_some() {
            "/rust/runtime/bin:/rust/toolchain/bin:/rust/user-cargo-bin:/usr/bin:/bin:/usr/sbin:/sbin"
        } else {
            "/rust/runtime/bin:/rust/toolchain/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        };
        let mut environment: BTreeMap<String, String> = [
            ("PATH", path),
            ("HOME", "/tmp"),
            ("RUSTUP_HOME", "/tmp/rustup-home"),
            ("RUSTUP_AUTO_INSTALL", "0"),
            ("CARGO_HOME", "/tmp/cargo-home"),
            ("CARGO_TARGET_DIR", "/tmp/target"),
            ("CARGO_NET_OFFLINE", "true"),
            ("RUSTC", "/rust/runtime/bin/rustc"),
            ("RUSTDOC", "/rust/runtime/bin/rustdoc"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        if self.resources.pinned {
            environment.insert("RUSTUP_TOOLCHAIN".into(), self.resources.channel.clone());
        }
        environment
    }
}

/// One bounded vendor file scheduled for the parallel copy phase.
struct SnapshotFile {
    source: PathBuf,
    destination: PathBuf,
    /// The source-root-relative path as encoded bytes, used by the identity.
    relative: Vec<u8>,
}

/// The number of copy workers; bounded so a large registry neither oversubscribes
/// the host nor stays serial.
fn copy_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(8)
}

/// Copies and digests one vendor file, re-checking its own source fingerprint
/// immediately after the copy so a concurrent host change fails this file
/// without any worker trusting a stale global recheck.
fn copy_vendor_file(entry: &SnapshotFile, preparation: &Preparation<'_>) -> CopyOutcome {
    let mut input = file(&entry.source, MAX_FILE_BYTES)?;
    let before = input
        .metadata()
        .map_err(|e| io_error("inspect vendor snapshot input", None, e))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(&entry.destination)
        .map_err(|e| io_error("create private vendor snapshot file", None, e))?;
    let mut hash = Sha256::new();
    let mut bytes = [0; 65536];
    let mut file_bytes = 0_u64;
    loop {
        preparation.check()?;
        let n = input
            .read(&mut bytes)
            .map_err(|e| io_error("read local vendor snapshot", None, e))?;
        if n == 0 {
            break;
        }
        file_bytes += n as u64;
        if file_bytes > MAX_FILE_BYTES {
            return Err(failure(
                "vendor snapshot exceeds 256 MiB/file or 1 GiB aggregate",
            ));
        }
        output
            .write_all(&bytes[..n])
            .map_err(|e| io_error("write private vendor snapshot", None, e))?;
        hash.update(&bytes[..n]);
    }
    if fingerprint(&before) != fingerprint(&inspect_path(&entry.source)?) {
        return Err(failure(
            "vendored file changed while snapshotting; refresh on the host and restart",
        ));
    }
    Ok((hash.finalize().to_vec(), file_bytes))
}

/// Snapshots the local vendored registry into a private, read-only-bounded
/// destination and returns its content identity.
///
/// Two phases: a serial enumeration that validates every entry, bounds the
/// tree, creates the destination directories, and schedules files in a
/// deterministic order, followed by a bounded parallel copy where each worker
/// re-checks its own file fingerprint right after copying. The identity folds
/// the per-file SHA-256 digests in that deterministic order, so it is stable
/// across runs and binds every relative path and byte without re-reading the
/// tree serially.
fn snapshot_vendor(
    source: &Path,
    destination: &Path,
    preparation: &Preparation<'_>,
) -> Result<String> {
    let source_root = source.to_owned();
    let mut stack = vec![(source.to_owned(), destination.to_owned(), 0_usize)];
    let mut count = 0_usize;
    let mut total = 0_u64;
    let mut directories = Vec::new();
    let mut files: Vec<SnapshotFile> = Vec::new();
    let mut path_bytes = 0_usize;
    while let Some((source, destination, depth)) = stack.pop() {
        preparation.check()?;
        let directory = inspect_path(&source)?;
        if depth > 64 || !directory.is_dir() {
            return Err(failure(
                "vendor resource must be a non-link directory tree of depth at most 64",
            ));
        }
        directories.push((source.clone(), fingerprint(&directory)));
        fs::create_dir(&destination)
            .map_err(|e| io_error("create private vendor snapshot directory", None, e))?;
        let mut entries = Vec::new();
        for entry in
            fs::read_dir(&source).map_err(|e| io_error("read local vendored registry", None, e))?
        {
            preparation.check()?;
            count += 1;
            if count > MAX_ENTRIES {
                return Err(failure("vendored registry exceeds 100000 snapshot entries"));
            }
            let entry =
                entry.map_err(|e| io_error("enumerate local vendored registry", None, e))?;
            path_bytes =
                path_bytes.saturating_add(entry.path().as_os_str().len().saturating_mul(3));
            if path_bytes > 32 << 20 {
                return Err(failure(
                    "vendor snapshot exceeds its 32 MiB retained-path bound",
                ));
            }
            entries.push(entry);
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            preparation.check()?;
            let src = entry.path();
            let dst = destination.join(entry.file_name());
            let metadata = inspect_path(&src)?;
            if metadata.is_dir() {
                stack.push((src, dst, depth + 1));
                continue;
            }
            if !metadata.is_file() || metadata.nlink() != 1 {
                return Err(failure(
                    "vendored registry must contain only regular single-link files/directories, not links or special entries",
                ));
            }
            if metadata.len() > MAX_FILE_BYTES {
                return Err(failure(
                    "vendor snapshot exceeds 256 MiB/file or 1 GiB aggregate",
                ));
            }
            total += metadata.len();
            if total > MAX_VENDOR_BYTES {
                return Err(failure(
                    "vendor snapshot exceeds 256 MiB/file or 1 GiB aggregate",
                ));
            }
            let relative = src
                .strip_prefix(&source_root)
                .map_err(|_| failure("invalid vendor entry"))?
                .as_os_str()
                .as_encoded_bytes()
                .to_vec();
            files.push(SnapshotFile {
                source: src,
                destination: dst,
                relative,
            });
        }
    }
    tracing::debug!(
        entries = count,
        files = files.len(),
        directories = directories.len(),
        bytes = total,
        elapsed_ms = preparation.started.elapsed().as_millis() as u64,
        "vendor snapshot enumeration completed"
    );
    // Parallel copy phase: contiguous index ranges keep scheduling simple and
    // deterministic; a single worker handles small trees without thread setup.
    let workers = copy_worker_count().min(files.len());
    let mut results: Vec<Option<CopyOutcome>> = (0..files.len()).map(|_| None).collect();
    if files.len() > 1 && workers > 1 {
        let files = &files;
        std::thread::scope(|scope| -> Result<()> {
            let mut handles = Vec::new();
            let mut start = 0_usize;
            for worker in 0..workers {
                let base = files.len() / workers;
                let rem = files.len() % workers;
                let extra = if worker < rem { 1 } else { 0 };
                let range = start..start + base + extra;
                start = range.end;
                handles.push(scope.spawn(move || {
                    let mut copied: Vec<(usize, CopyOutcome)> = Vec::with_capacity(range.len());
                    for index in range {
                        copied.push((index, copy_vendor_file(&files[index], preparation)));
                    }
                    copied
                }));
            }
            for handle in handles {
                let copied = handle
                    .join()
                    .map_err(|_| failure("vendor snapshot worker terminated abnormally"))?;
                for (index, result) in copied {
                    results[index] = Some(result);
                }
            }
            Ok(())
        })?;
    } else {
        for index in 0..files.len() {
            results[index] = Some(copy_vendor_file(&files[index], preparation));
        }
    }
    tracing::debug!(
        workers,
        elapsed_ms = preparation.started.elapsed().as_millis() as u64,
        "vendor snapshot copy completed"
    );
    for (path, before) in directories {
        preparation.check()?;
        if fingerprint(&inspect_path(&path)?) != before {
            return Err(failure(
                "vendor directory changed while snapshotting; refresh on the host and restart",
            ));
        }
    }
    let mut identity = Sha256::new();
    for (index, entry) in files.iter().enumerate() {
        preparation.check()?;
        let result = results[index]
            .take()
            .ok_or_else(|| failure("vendor snapshot worker did not report a result"))?;
        let (digest, bytes) = result?;
        identity.update((entry.relative.len() as u64).to_be_bytes());
        identity.update(&entry.relative);
        identity.update(bytes.to_be_bytes());
        identity.update(digest);
    }
    Ok(format!("sha256:{}", hex::encode(identity.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn fixture() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(".rust-resource-test-")
            .tempdir_in(env!("CARGO_MANIFEST_DIR"))
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
        let cancellation = CancellationToken::new();
        let preparation = Preparation {
            started: Instant::now(),
            cancellation: &cancellation,
        };
        let metadata = inspect_path(&root).unwrap();
        let staging = tempfile::Builder::new()
            .prefix(".rust-staging-")
            .tempdir_in(&directory)
            .unwrap();
        let rustup_home = staging.path().join("rustup-home");
        fs::create_dir_all(rustup_home.join("toolchains")).unwrap();
        symlink(TOOLCHAIN_DEST, rustup_home.join("toolchains/stable")).unwrap();
        RustEnvironment {
            resources: Arc::new(Resources {
                pin_identity: pin_identity(&workspace).unwrap(),
                workspace,
                root: root.clone(),
                channel: "stable".into(),
                identity: label(b"synthetic"),
                executables: vec![{
                    let (identity, fingerprint) = hash_file(&executable, &preparation).unwrap();
                    TrackedExecutable {
                        path: executable.clone(),
                        identity,
                        fingerprint,
                    }
                }],
                directories: vec![(root, (metadata.dev(), metadata.ino()))],
                staging,
                vendor_identity: label(b"empty"),
                has_vendor: false,
                pinned: false,
                user_cargo_bin: None,
                rustup_home,
                inventory: inventory::Inventory {
                    installations: vec![inventory::Installation {
                        name: "stable".into(),
                        root: directory.join("toolchain"),
                        destination: TOOLCHAIN_DEST.into(),
                    }],
                    fingerprints: Vec::new(),
                },
            }),
        }
    }

    #[test]
    fn drift_and_substituted_roots_are_rejected_without_mutating_real_installs() {
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
        assert!(environment.validate(workspace, &cancellation).is_err());
        fs::rename(
            &environment.resources.root,
            directory.path().join("old-toolchain"),
        )
        .unwrap();
        fs::create_dir(&environment.resources.root).unwrap();
        assert!(environment.validate(workspace, &cancellation).is_err());
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
        assert!(environment.validate(workspace, &cancellation).is_err());
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
        drop(environment);
        assert!(staging.exists());
        drop(clone);
        assert!(!staging.exists());
    }

    #[test]
    fn registration_uses_exact_selection_and_private_home_and_rejects_drift() {
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
                .is_err()
        );
    }

    #[test]
    fn non_selected_targets_and_inventory_membership_drift_fail_closed() {
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
            resources.inventory.fingerprints = [&parent, &alternate, &libraries, &library]
                .into_iter()
                .map(|path| (path.clone(), fingerprint(&inspect_path(path).unwrap())))
                .collect();
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
                    .is_err(),
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
    fn snapshot_rejects_linked_files_special_files_and_linked_ancestors() {
        let directory = fixture();
        let source = directory.path().join("vendor");
        fs::create_dir(&source).unwrap();
        let cancellation = CancellationToken::new();
        let preparation = Preparation {
            started: Instant::now(),
            cancellation: &cancellation,
        };
        symlink("/etc/passwd", source.join("link")).unwrap();
        assert!(
            snapshot_vendor(
                &source.canonicalize().unwrap(),
                &directory.path().join("snapshot1"),
                &preparation
            )
            .is_err()
        );
        fs::remove_file(source.join("link")).unwrap();
        nix::unistd::mkfifo(&source.join("fifo"), nix::sys::stat::Mode::S_IRUSR).unwrap();
        assert!(
            snapshot_vendor(
                &source.canonicalize().unwrap(),
                &directory.path().join("snapshot2"),
                &preparation
            )
            .is_err()
        );
        fs::remove_file(source.join("fifo")).unwrap();
        fs::write(directory.path().join("outside"), b"not registry material").unwrap();
        fs::hard_link(directory.path().join("outside"), source.join("hardlink")).unwrap();
        assert!(
            snapshot_vendor(
                &source.canonicalize().unwrap(),
                &directory.path().join("snapshot-hardlink"),
                &preparation
            )
            .is_err()
        );
        fs::remove_file(source.join("hardlink")).unwrap();
        symlink(
            source.canonicalize().unwrap(),
            directory.path().join("alias"),
        )
        .unwrap();
        assert!(
            snapshot_vendor(
                &directory.path().join("alias"),
                &directory.path().join("snapshot3"),
                &preparation
            )
            .is_err()
        );
    }

    #[test]
    fn snapshot_identity_binds_paths_and_bytes_and_copy_is_independent() {
        let directory = fixture();
        let source = directory.path().join("vendor");
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("crate-a")).unwrap();
        fs::write(source.join("crate-a/file"), b"initial").unwrap();
        let source = source.canonicalize().unwrap();
        let cancellation = CancellationToken::new();
        let preparation = Preparation {
            started: Instant::now(),
            cancellation: &cancellation,
        };
        let first =
            snapshot_vendor(&source, &directory.path().join("copy1"), &preparation).unwrap();
        fs::rename(source.join("crate-a"), source.join("crate-b")).unwrap();
        let second =
            snapshot_vendor(&source, &directory.path().join("copy2"), &preparation).unwrap();
        assert_ne!(first, second);
        fs::write(source.join("crate-b/file"), b"replacement").unwrap();
        let third =
            snapshot_vendor(&source, &directory.path().join("copy3"), &preparation).unwrap();
        assert_ne!(second, third);
        assert_eq!(
            fs::read(directory.path().join("copy1/crate-a/file")).unwrap(),
            b"initial"
        );
    }

    #[test]
    fn same_size_executable_drift_is_rejected_by_fingerprint() {
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
        assert!(environment.validate(workspace, &cancellation).is_err());
    }

    #[test]
    fn parallel_snapshot_is_complete_and_identity_is_deterministic() {
        let directory = fixture();
        let source = directory.path().join("vendor");
        let crates = source.join("registry/src");
        fs::create_dir_all(&crates).unwrap();
        for index in 0..120 {
            let name = format!("crate-{index:03}");
            fs::create_dir(crates.join(&name)).unwrap();
            fs::write(
                crates.join(&name).join("lib.rs"),
                format!("// crate {index}\n{}", "x".repeat(7000)),
            )
            .unwrap();
        }
        fs::write(source.join("registry/CACHEDIR.TAG"), b"some\n").unwrap();
        let source = source.canonicalize().unwrap();
        let cancellation = CancellationToken::new();
        let preparation = Preparation {
            started: Instant::now(),
            cancellation: &cancellation,
        };
        let first =
            snapshot_vendor(&source, &directory.path().join("copy-a"), &preparation).unwrap();
        let second =
            snapshot_vendor(&source, &directory.path().join("copy-b"), &preparation).unwrap();
        assert_eq!(
            first, second,
            "the identity must be deterministic across runs"
        );
        for index in 0..120 {
            let name = format!("crate-{index:03}");
            let src = fs::read(source.join("registry/src").join(&name).join("lib.rs")).unwrap();
            let dst = fs::read(
                directory
                    .path()
                    .join("copy-a/registry/src")
                    .join(&name)
                    .join("lib.rs"),
            )
            .unwrap();
            assert_eq!(src, dst, "snapshot copy must be byte-identical");
        }
        assert_eq!(
            fs::read(source.join("registry/CACHEDIR.TAG")).unwrap(),
            fs::read(directory.path().join("copy-a/registry/CACHEDIR.TAG")).unwrap()
        );
        // A same-size byte change must move the identity.
        let target = source.join("registry/src/crate-000/lib.rs");
        let mut bytes = fs::read(&target).unwrap();
        bytes[3] = b'Y';
        fs::write(&target, &bytes).unwrap();
        let third =
            snapshot_vendor(&source, &directory.path().join("copy-c"), &preparation).unwrap();
        assert_ne!(first, third);
    }

    #[test]
    fn preparation_is_cancelled_and_file_bound_is_checked_before_allocating() {
        let directory = fixture();
        let path = directory.path().join("large");
        File::create(&path)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        assert!(file(&path.canonicalize().unwrap(), MAX_FILE_BYTES).is_err());
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
    fn native_standard_library_is_required_and_cannot_be_a_link() {
        let directory = fixture();
        let root = directory.path().canonicalize().unwrap();
        assert!(native_library_layout(&root).is_err());
        fs::create_dir_all(root.join("lib/rustlib/test-native/lib")).unwrap();
        fs::write(root.join("lib/rustlib/components"), "rustc-test-native\n").unwrap();
        assert!(native_library_layout(&root).is_err());
        fs::write(
            root.join("lib/rustlib/test-native/lib/libcore-test.rlib"),
            b"core",
        )
        .unwrap();
        fs::write(
            root.join("lib/rustlib/test-native/lib/libstd-test.rlib"),
            b"std",
        )
        .unwrap();
        assert_eq!(native_library_layout(&root).unwrap().len(), 3);
        fs::remove_file(root.join("lib/rustlib/test-native/lib/libstd-test.rlib")).unwrap();
        symlink(
            "/etc/passwd",
            root.join("lib/rustlib/test-native/lib/libstd-test.rlib"),
        )
        .unwrap();
        assert!(native_library_layout(&root).is_err());
    }

    #[test]
    fn native_release_channel_tokens_accept_pins_but_not_paths_or_custom_roots() {
        for value in [
            "stable",
            "beta",
            "nightly-2026-01-01",
            "1.95",
            "1.95.0",
            "stable-x86_64-unknown-linux-gnu",
        ] {
            channel(value).unwrap();
        }
        for value in [
            "",
            "custom",
            "--help",
            "/host/root",
            "../../toolchain",
            "stable\n",
            "stable;echo",
            "stable\\root",
        ] {
            assert!(channel(value).is_err(), "{value}");
        }
    }
}
