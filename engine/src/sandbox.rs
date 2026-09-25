//! Versioned, shell-free protocol for a project-selected sandbox runner.
//!
//! Execution policy:
//!   - On Linux: uses `execve` for direct binary execution with no shell,
//!     environment pollution, and no path traversal. The runner binary is
//!     copied to a secure, user-owned state directory before execution.
//!   - On non-Linux: uses `Command::spawn` with `env_clear()` as a fallback
//!     that still avoids shell interpretation. The same environment allowlist
//!     is enforced in both cases.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    os::fd::AsFd,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use nix::{
    errno::Errno,
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    KvistError, Result,
    config::{
        MAX_SANDBOX_ENVIRONMENT_ENTRIES, MAX_SANDBOX_ENVIRONMENT_NAME_BYTES, SandboxConfig,
        VcsSelection,
    },
    vcs,
};

pub const PROTOCOL_VERSION: u32 = 1;
const REQUEST_PROTOCOL: &str = "kvist-sandbox-request-v1";
const PROBE_PROTOCOL: &str = "kvist-sandbox-probe-v1";
const PROBE_ARGUMENT: &str = "--kvist-sandbox-probe-v1";
const EXECUTE_ARGUMENT: &str = "--kvist-sandbox-request-v1";
const MAX_PROBE_BYTES: usize = 64 * 1024;
/// These producer bounds mirror `sandbox_runner::protocol`: both sides accept
/// at most 1024 argv entries and 4096 bytes per argv/environment value.
const MAX_ARGV_ENTRIES: usize = 1024;
const MAX_REQUEST_VALUE_BYTES: usize = 4096;
const SUPERVISION_POLL_INTERVAL: Duration = Duration::from_millis(10);
const STREAM_DRAIN_BUDGET: usize = 64 * 1024;

/// Default resource bounds for a request field the caller does not specify.
const DEFAULT_MAX_PROCESSES: u64 = 64;
const DEFAULT_MAX_FILES: u64 = 4096;
const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_MAX_SCRATCH_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_WALL_TIME_MS: u64 = 15 * 60 * 1000;
const DEFAULT_MAX_OUTPUT_BYTES: u64 = 1024 * 1024;

/// Explicit safe maxima for each bounded resource limit. These MUST match the
/// runner's authoritative `protocol::MAX_*` constants: the runner rejects any
/// request whose limits exceed them, so the engine never emits a request that
/// would exceed them and never saturates a converted value to `u64::MAX`.
const MAX_WALL_TIME_MS: u64 = 24 * 60 * 60 * 1000;
const MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PROCESSES: u64 = 4096;
const MAX_FILES: u64 = 1 << 20;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_SCRATCH_BYTES: u64 = 64 * 1024 * 1024 * 1024;

/// A fixed, short deadline applied to the capability probe so a hostile or
/// hung runner cannot block task startup indefinitely.
const PROBE_DEADLINE: Duration = Duration::from_secs(30);

const _: () = {
    assert!(DEFAULT_WALL_TIME_MS <= MAX_WALL_TIME_MS);
    assert!(DEFAULT_MAX_OUTPUT_BYTES <= MAX_OUTPUT_BYTES);
    assert!(DEFAULT_MAX_PROCESSES <= MAX_PROCESSES);
    assert!(DEFAULT_MAX_FILES <= MAX_FILES);
    assert!(DEFAULT_MAX_FILE_BYTES <= MAX_FILE_BYTES);
    assert!(DEFAULT_MAX_SCRATCH_BYTES <= MAX_SCRATCH_BYTES);
};

/// Component intent and record documents that must never be exposed inside a
/// writable ancestor. In an authoring phase each existing document is mounted
/// read-only at its own disjoint destination instead.
const COMPONENT_INTENT_DOCUMENTS: [&str; 5] = [
    "REQUIREMENTS.md",
    "CONTRACT.md",
    "DESIGN.md",
    "TODOS.yaml",
    "IMPL.md",
];

/// Conservative implementation and test roots an authoring agent may write.
/// Only these directories are granted read-write, and only when they exist as
/// real directories, so no writable ancestor exposes protected component state,
/// `.kvist`, `.git`, or child/peer implementation.
const AUTHORING_WRITABLE_ROOTS: [&str; 2] = ["src", "tests"];

/// The disjoint filesystem and network authority a request is issued under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionPhase {
    /// An authoring agent that may write only its explicit implementation roots.
    Authoring,
    /// A build/test verification run with network denied and read-only inputs.
    Verification,
}

impl ExecutionPhase {
    fn wire(self) -> &'static str {
        match self {
            ExecutionPhase::Authoring => "authoring",
            ExecutionPhase::Verification => "verification",
        }
    }
}

/// A relay invoked with each runner stdout chunk as it is drained.
pub type LiveStdoutSink = Box<dyn FnMut(&[u8])>;

/// Host-side resource controls for the sandbox runner process.
#[derive(Default)]
pub struct ExecutionOptions {
    pub timeout: Option<Duration>,
    pub output_limit: Option<usize>,
    /// Optional relay invoked with each runner stdout chunk as it is drained;
    /// the bounded capture continues unchanged, so evidence and live output
    /// cannot diverge.
    pub live_stdout: Option<LiveStdoutSink>,
}

/// Bounded result returned by a sandbox runner request.
pub struct ExecutionResult {
    pub output: std::process::Output,
    pub timed_out: bool,
    pub output_limit_exceeded: bool,
    /// True when the run was cancelled by SIGINT/SIGTERM.
    pub cancelled: bool,
}

/// Canonical, content-addressed identity of the trusted runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerIdentity {
    pub canonical_path: String,
    pub digest: String,
}

/// Validates and identifies a runner without starting its capability probe.
pub fn runner_identity(
    config: &SandboxConfig,
    project_root: &Path,
    vcs_selection: VcsSelection,
) -> Result<RunnerIdentity> {
    validate_runner(config, project_root, vcs_selection)?;
    let canonical_path = Path::new(&config.runner)
        .canonicalize()
        .map_err(|source| sandbox_error(config, "canonicalize trusted sandbox runner", source))?;
    // The runner can be a directory (containing probe/execute handlers) or a file.
    // If it is a directory, we compute the digest over the concatenation of its
    // handler scripts in a deterministic order. If it is a file, we read that file.
    let bytes = if canonical_path.is_dir() {
        // Directory runner: collect handler scripts in sorted order.
        let mut bytes = Vec::new();
        let entries = fs::read_dir(&canonical_path).map_err(|source| {
            sandbox_error(config, "read trusted sandbox runner directory", source)
        })?;
        let mut sorted_entries: Vec<_> = entries
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    let path = e.path();
                    if path.is_file() { Some(path) } else { None }
                })
            })
            .collect();
        sorted_entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        for path in sorted_entries {
            if let Ok(contents) = fs::read(&path) {
                bytes.extend_from_slice(&contents);
            }
        }
        bytes
    } else {
        fs::read(&canonical_path)
            .map_err(|source| sandbox_error(config, "read trusted sandbox runner", source))?
    };
    Ok(RunnerIdentity {
        canonical_path: canonical_path.to_string_lossy().into_owned(),
        digest: format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
    })
}

/// Validates and identifies the configured enforcement backend executable.
///
/// The backend (Bubblewrap) is an approval-bound identity: this reads its bytes
/// from a regular, non-symlink file installed outside both the project root and
/// the selected VCS worktree, and returns its canonical path and content
/// digest. It never trusts a syntax-only digest claim; the identity is derived
/// from the on-disk bytes so approval binds the exact executable.
pub fn backend_identity(
    config: &SandboxConfig,
    project_root: &Path,
    vcs_selection: VcsSelection,
) -> Result<BackendIdentity> {
    let backend = Path::new(&config.backend);
    if !backend.is_absolute() {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the enforcement backend path must be absolute".to_owned(),
        });
    }
    let metadata = fs::symlink_metadata(backend)
        .map_err(|source| sandbox_error(config, "inspect approved enforcement backend", source))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the enforcement backend must be a regular non-symlink file".to_owned(),
        });
    }
    let canonical_project = project_root
        .canonicalize()
        .map_err(|source| sandbox_error(config, "canonicalize project root", source))?;
    let inspection = vcs::inspect(project_root, vcs_selection, Vec::new());
    let worktree_root =
        inspection
            .repository_root
            .ok_or_else(|| KvistError::SandboxUnavailable {
                runner: config.runner.clone(),
                reason: format!(
                    "cannot resolve the selected VCS worktree root: {}",
                    inspection.diagnostic.unwrap_or(inspection.summary)
                ),
            })?;
    let canonical_worktree = worktree_root.canonicalize().map_err(|source| {
        sandbox_error(config, "canonicalize selected VCS worktree root", source)
    })?;
    let canonical_backend = backend
        .canonicalize()
        .map_err(|source| sandbox_error(config, "canonicalize enforcement backend", source))?;
    if canonical_backend.starts_with(&canonical_project) {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the enforcement backend must be installed outside the project root".to_owned(),
        });
    }
    if canonical_backend.starts_with(&canonical_worktree) {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the enforcement backend must be installed outside the selected VCS worktree"
                .to_owned(),
        });
    }
    let bytes = fs::read(&canonical_backend)
        .map_err(|source| sandbox_error(config, "read approved enforcement backend", source))?;
    Ok(BackendIdentity {
        kind: "bubblewrap".to_owned(),
        path: canonical_backend.to_string_lossy().into_owned(),
        digest: digest_label(&bytes),
    })
}

/// Values that describe one program invocation inside the component sandbox.
pub struct ExecutionRequest<'a> {
    pub project_root: &'a Path,
    pub vcs_selection: VcsSelection,
    pub component_dir: &'a Path,
    /// The disjoint authority this invocation is issued under.
    pub phase: ExecutionPhase,
    pub program: &'a str,
    pub arguments: &'a [String],
    pub environment: BTreeMap<String, String>,
    pub read_only_mounts: &'a [ReadOnlyMount],
    /// The enforcement backend identity confirmed by the availability probe.
    pub backend: &'a BackendIdentity,
    /// The authenticated execution-approval digest bound as the request policy
    /// identity. This is the approved policy identity, not a locally computed
    /// unapproved hash.
    pub policy_identity: &'a str,
}

/// One host path (file or directory) exposed to the sandbox at a fixed
/// read-only path.
///
/// The identity is the content address bound to the approved mount plan. For a
/// single file it is derived from the file bytes; for a directory it is an
/// explicit identity supplied by the caller (for example the lock-file digest
/// of a vendored registry), so large directory trees are never hashed file by
/// file. `None` means the caller wants the identity derived from the source
/// bytes, as for a single file.
#[derive(Clone)]
pub struct ReadOnlyMount {
    pub source: PathBuf,
    pub destination: String,
    /// Explicit mount identity. `None` derives the identity from the source.
    pub identity: Option<String>,
}

impl ReadOnlyMount {
    /// One read-only mount whose identity is derived from the source bytes.
    pub fn file(source: impl Into<PathBuf>, destination: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            identity: None,
        }
    }

    /// One read-only mount carrying an explicit identity (used for directory
    /// sources, whose content is catalogued by an external identity rather than
    /// hashed file by file).
    pub fn directory(
        source: impl Into<PathBuf>,
        destination: impl Into<String>,
        identity: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            identity: Some(identity.into()),
        }
    }
}

/// The identity of the enforcement backend confirmed by the version-one probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendIdentity {
    pub kind: String,
    pub path: String,
    pub digest: String,
}

/// The parsed, capability-confirmed version-one probe response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxProbe {
    pub runner_path: String,
    pub runner_digest: String,
    pub backend: BackendIdentity,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeWire {
    protocol: String,
    protocol_version: u32,
    runner: ProbeExecutable,
    backend: ProbeBackend,
    capabilities: ProbeCapabilities,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeExecutable {
    path: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeBackend {
    kind: String,
    path: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeCapabilities {
    namespaces: ProbeNamespaces,
    new_session: bool,
    parent_death_signal: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeNamespaces {
    mount: bool,
    network: bool,
    pid: bool,
    ipc: bool,
    uts: bool,
    user: bool,
}

/// The closed, deterministically serialized version-one execution request.
#[derive(Debug, Serialize)]
struct SandboxRequest<'a> {
    protocol: &'static str,
    protocol_version: u32,
    phase: &'static str,
    argv: &'a [String],
    working_directory: &'static str,
    environment: &'a BTreeMap<String, String>,
    network: SandboxNetwork,
    resources: SandboxResources,
    identities: SandboxIdentities,
    grants: &'a [SandboxGrant],
    toolchain: SandboxToolchain,
    cache: Option<SandboxCache>,
    scratch: Option<SandboxScratch>,
}

#[derive(Debug, Serialize)]
struct SandboxNetwork {
    mode: &'static str,
    allowed_sources: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SandboxResources {
    wall_time_ms: u64,
    max_output_bytes: u64,
    max_processes: u64,
    max_files: u64,
    max_file_bytes: u64,
    max_scratch_bytes: u64,
    /// Bound for a read-only approved Cargo home during Cargo phases. The
    /// generic (system-toolchain) path leaves it unset, preserving that wire
    /// shape; the offline Cargo verification path requires a nonzero bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    max_cache_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
struct SandboxIdentities {
    runner: String,
    backend: SandboxBackend,
    policy: String,
    toolchain: String,
    command: String,
    mount_plan: String,
}

#[derive(Debug, Serialize)]
struct SandboxBackend {
    kind: String,
    path: String,
    digest: String,
}

#[derive(Debug, Clone, Serialize)]
struct SandboxGrant {
    source: String,
    destination: String,
    access: &'static str,
    purpose: &'static str,
    identity: String,
}

/// The approved toolchain for an execution, serialized to match
/// `kvist_sandbox_runner::protocol::Toolchain` (`system` or `cargo`).
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum SandboxToolchain {
    /// A generic system toolchain rooted at an approval-bound immutable root.
    System { identity: String, root: String },
    /// A usable Cargo toolchain: the immutable root (cargo, rustc, rustlib,
    /// linker) plus the exact cargo executable path that must live beneath it.
    Cargo {
        identity: String,
        root: String,
        cargo: String,
    },
}

/// A read-only approved Cargo-home endpoint used by offline Cargo verification.
#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct SandboxCacheEndpoint {
    destination: String,
    identity: String,
}

/// The read-only approved Cargo home for an offline build. Shape mirrors
/// `kvist_sandbox_runner::protocol::Cache`: the exact registry and git children
/// of `cargo_home`, plus the approved read-only endpoint. The writable,
/// lockfile, and promotion acquisition fields are omitted, since verification
/// only mounts an approved, immutable Cargo home.
#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct SandboxCache {
    cargo_home: String,
    registry: String,
    git: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    approved: Option<SandboxCacheEndpoint>,
}

#[derive(Debug, Serialize)]
struct SandboxScratch {
    destination: &'static str,
    identity: String,
}

fn digest_label(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// The canonical, content-addressed identity of a resolved argv[0] executable.
struct ResolvedProgram {
    /// The canonical absolute normalized UTF-8 path of the executable.
    canonical_path: String,
    /// The `sha256:` digest of the executable's exact on-disk bytes.
    digest: String,
}

/// Resolves argv[0] to an exact executable before the request is serialized.
///
/// A bare program name (no `/`) is resolved only against a `PATH` that is
/// explicitly present in the request environment; there is no ambient host
/// `PATH` fallback. An absolute program path is used directly. In both cases
/// the resolved target must be a regular, non-symlink, executable file, is
/// canonicalized to an absolute normalized UTF-8 path, and its exact bytes are
/// hashed. A relative program that contains a `/` is rejected as ambiguous.
///
/// The returned canonical path replaces argv[0] and its digest is the exact
/// content identity used for the conservative toolchain grant, so the runner's
/// parser (which requires a canonical absolute argv[0]) and the engine producer
/// agree on the exact executable that will run.
fn resolve_program(
    config: &SandboxConfig,
    program: &str,
    environment: &BTreeMap<String, String>,
) -> Result<ResolvedProgram> {
    let unavailable = |reason: String| KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason,
    };

    let candidate = if program.contains('/') {
        let path = Path::new(program);
        if !path.is_absolute() {
            return Err(unavailable(format!(
                "argv[0] `{program}` must be an absolute path or a bare program name resolved via the request PATH"
            )));
        }
        path.to_path_buf()
    } else {
        if program.is_empty() {
            return Err(unavailable("argv[0] must not be empty".to_owned()));
        }
        let path_value = environment.get("PATH").ok_or_else(|| {
            unavailable(format!(
                "argv[0] `{program}` is a bare program name but the request environment declares no PATH to resolve it"
            ))
        })?;
        resolve_in_path(config, program, path_value)?
    };

    // Require a regular, non-symlink executable file at the exact submitted
    // location before resolving parent-directory links.
    let metadata = fs::symlink_metadata(&candidate).map_err(|source| {
        unavailable(format!(
            "cannot inspect argv[0] executable `{}`: {source}",
            candidate.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(unavailable(format!(
            "argv[0] executable `{}` must be a regular non-symlink file",
            candidate.display()
        )));
    }
    if !is_executable(&metadata) {
        return Err(unavailable(format!(
            "argv[0] executable `{}` is not executable",
            candidate.display()
        )));
    }
    let canonical = candidate.canonicalize().map_err(|source| {
        unavailable(format!(
            "cannot canonicalize argv[0] executable `{}`: {source}",
            candidate.display()
        ))
    })?;
    // The canonicalized target must still be a regular non-symlink executable.
    let canonical_metadata = fs::symlink_metadata(&canonical).map_err(|source| {
        unavailable(format!(
            "cannot inspect canonical argv[0] executable `{}`: {source}",
            canonical.display()
        ))
    })?;
    if canonical_metadata.file_type().is_symlink() || !canonical_metadata.file_type().is_file() {
        return Err(unavailable(format!(
            "canonical argv[0] executable `{}` must be a regular non-symlink file",
            canonical.display()
        )));
    }
    if !is_executable(&canonical_metadata) {
        return Err(unavailable(format!(
            "canonical argv[0] executable `{}` is not executable",
            canonical.display()
        )));
    }
    let canonical_path = canonical
        .to_str()
        .ok_or_else(|| {
            unavailable(format!(
                "argv[0] executable `{}` is not valid UTF-8 for the sandbox manifest",
                canonical.display()
            ))
        })?
        .to_owned();
    let bytes = fs::read(&canonical).map_err(|source| {
        unavailable(format!(
            "cannot read argv[0] executable `{}` for its content identity: {source}",
            canonical.display()
        ))
    })?;
    Ok(ResolvedProgram {
        canonical_path,
        digest: digest_label(&bytes),
    })
}

/// Resolves a bare program name against an explicit `PATH` value, returning the
/// first entry that is a regular, non-symlink, executable file.
fn resolve_in_path(config: &SandboxConfig, program: &str, path_value: &str) -> Result<PathBuf> {
    for directory in path_value.split(':') {
        if directory.is_empty() {
            continue;
        }
        let candidate = Path::new(directory).join(program);
        match fs::symlink_metadata(&candidate) {
            Ok(metadata)
                if metadata.file_type().is_file()
                    && !metadata.file_type().is_symlink()
                    && is_executable(&metadata) =>
            {
                return Ok(candidate);
            }
            _ => {}
        }
    }
    Err(KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: format!(
            "argv[0] `{program}` could not be resolved to a regular executable via the request PATH"
        ),
    })
}

/// Returns true when a file's mode grants any execute permission.
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

/// Canonicalizes a grant source path to an absolute, normalized UTF-8 string so
/// the producer never emits a non-canonical (`.`/`..`/duplicate-separator)
/// source that the runner's parser would reject.
fn canonical_source_str(
    config: &SandboxConfig,
    source: &Path,
    description: &'static str,
) -> Result<String> {
    let canonical = source
        .canonicalize()
        .map_err(|source_error| sandbox_error(config, description, source_error))?;
    canonical
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!("{description}: path is not valid UTF-8 for the sandbox manifest"),
        })
}

/// Converts a wall-clock deadline to bounded milliseconds, failing closed on
/// overflow or on a value that exceeds the safe maximum rather than saturating.
fn bounded_millis(config: &SandboxConfig, limit: Duration) -> Result<u64> {
    let millis = u64::try_from(limit.as_millis()).map_err(|_| KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: "requested wall-time limit overflows the protocol's u64 milliseconds field"
            .to_owned(),
    })?;
    if millis > MAX_WALL_TIME_MS {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!(
                "requested wall-time limit of {millis} ms exceeds the safe maximum of {MAX_WALL_TIME_MS} ms"
            ),
        });
    }
    Ok(millis)
}

/// Converts an output byte limit to a bounded u64, failing closed on overflow or
/// on a value that exceeds the safe maximum rather than saturating.
fn bounded_output_bytes(config: &SandboxConfig, limit: usize) -> Result<u64> {
    let bytes = u64::try_from(limit).map_err(|_| KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: "requested output limit overflows the protocol's u64 bytes field".to_owned(),
    })?;
    if bytes > MAX_OUTPUT_BYTES {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!(
                "requested output limit of {bytes} bytes exceeds the safe maximum of {MAX_OUTPUT_BYTES} bytes"
            ),
        });
    }
    Ok(bytes)
}

/// Builds the conservative authoring grant set: every existing component intent
/// document is mounted read-only at its own disjoint destination, and only the
/// recognized implementation and test roots are granted read-write. The whole
/// component directory is never mounted read-write, so no writable ancestor
/// exposes `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`,
/// `IMPL.md`, `.kvist`, `.git`, or child/peer implementation. When no safe
/// writable root can be represented, this fails closed rather than granting the
/// component root.
fn append_authoring_grants(
    config: &SandboxConfig,
    component_dir: &Path,
    grants: &mut Vec<SandboxGrant>,
) -> Result<()> {
    for document in COMPONENT_INTENT_DOCUMENTS {
        let source = component_dir.join(document);
        match fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.file_type().is_file() => {
                let source_str = canonical_source_str(
                    config,
                    &source,
                    "canonicalize component intent document for sandbox context",
                )?;
                let identity = match fs::read(&source) {
                    Ok(bytes) => digest_label(&bytes),
                    Err(source_error) => {
                        return Err(sandbox_error(
                            config,
                            "read component intent document for sandbox context",
                            source_error,
                        ));
                    }
                };
                grants.push(SandboxGrant {
                    source: source_str,
                    destination: format!("/workspace/component/{document}"),
                    access: "read-only",
                    purpose: "context",
                    identity,
                });
            }
            Ok(_) => {
                return Err(KvistError::SandboxUnavailable {
                    runner: config.runner.clone(),
                    reason: format!(
                        "component intent document `{document}` must be a regular non-link file"
                    ),
                });
            }
            Err(source_error) => {
                return Err(sandbox_error(
                    config,
                    "inspect component intent document for sandbox context",
                    source_error,
                ));
            }
        }
    }

    let mut writable_roots = 0_usize;
    for root in AUTHORING_WRITABLE_ROOTS {
        let source = component_dir.join(root);
        match fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                let source_str = canonical_source_str(
                    config,
                    &source,
                    "canonicalize authoring root for sandbox context",
                )?;
                grants.push(SandboxGrant {
                    source: source_str.clone(),
                    destination: format!("/workspace/component/{root}"),
                    access: "read-write",
                    purpose: "authoring",
                    identity: digest_label(format!("kvist-authoring-root:{source_str}").as_bytes()),
                });
                writable_roots += 1;
            }
            Ok(_) => {
                return Err(KvistError::SandboxUnavailable {
                    runner: config.runner.clone(),
                    reason: format!(
                        "authoring root `{root}` must be a real directory when present"
                    ),
                });
            }
            Err(source_error) if source_error.kind() == io::ErrorKind::NotFound => {}
            Err(source_error) => {
                return Err(sandbox_error(
                    config,
                    "inspect authoring root for sandbox context",
                    source_error,
                ));
            }
        }
    }

    if writable_roots == 0 {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "no safe authoring root (an implementation or test directory) exists; refusing to grant the component root read-write".to_owned(),
        });
    }
    Ok(())
}

/// Validates that the configured runner is reachable and confirms its
/// version-one deny-network Bubblewrap-backed isolation probe before task
/// state is changed. Returns the parsed backend identity for request binding.
pub fn ensure_available(
    config: &SandboxConfig,
    project_root: &Path,
    vcs_selection: VcsSelection,
    expected_runner: &RunnerIdentity,
    expected_backend: &BackendIdentity,
) -> Result<SandboxProbe> {
    tracing::debug!(runner = %config.runner, "probing sandbox runner availability");
    let launch = checked_runner_launch(config, project_root, vcs_selection, expected_runner)?;
    let result = run_launched_bounded(
        &launch,
        config,
        PROBE_ARGUMENT,
        None,
        Some(PROBE_DEADLINE),
        Some(MAX_PROBE_BYTES),
        None,
    )?;
    let ExecutionResult {
        output,
        timed_out,
        output_limit_exceeded,
        cancelled,
    } = result;
    if cancelled {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the sandbox availability probe was interrupted before it could attest"
                .to_owned(),
        });
    }
    if timed_out {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the sandbox availability probe exceeded its deadline".to_owned(),
        });
    }
    if output_limit_exceeded {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner probe response exceeded the accepted size bound".to_owned(),
        });
    }
    if !output.status.success() {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner did not confirm the version-one isolation probe".to_owned(),
        });
    }
    if !output.stderr.is_empty() {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason:
                "the runner probe emitted diagnostics on standard error despite reporting success"
                    .to_owned(),
        });
    }
    if output.stdout.len() > MAX_PROBE_BYTES {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner probe response exceeded the accepted size bound".to_owned(),
        });
    }
    let probe: ProbeWire =
        serde_json::from_slice(&output.stdout).map_err(|error| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!("the runner probe response is not a valid version-one object: {error}"),
        })?;
    if probe.protocol != PROBE_PROTOCOL || probe.protocol_version != PROTOCOL_VERSION {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner probe declared an unsupported protocol or version".to_owned(),
        });
    }
    if probe.backend.kind != "bubblewrap" {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner probe did not confirm a Bubblewrap enforcement backend".to_owned(),
        });
    }
    let namespaces = &probe.capabilities.namespaces;
    let confirmed = namespaces.mount
        && namespaces.network
        && namespaces.pid
        && namespaces.ipc
        && namespaces.uts
        && namespaces.user
        && probe.capabilities.new_session
        && probe.capabilities.parent_death_signal;
    if !confirmed {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason:
                "the runner probe did not confirm the required namespace and session capabilities"
                    .to_owned(),
        });
    }
    validate_probe_digest(config, &probe.runner.digest, "runner")?;
    validate_probe_digest(config, &probe.backend.digest, "backend")?;
    // The runner's canonical path and digest are already bound by
    // `checked_runner_launch`; here the probe additionally confirms the running
    // process reports the approved runner digest and the approval-bound backend
    // kind, path, and digest. The runner path reported by the probe is the
    // descriptor-bound launch path, so only its content digest is compared.
    if probe.runner.digest != expected_runner.digest {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner probe reported a runner digest that does not match the approved runner identity".to_owned(),
        });
    }
    if probe.backend.kind != expected_backend.kind
        || probe.backend.path != expected_backend.path
        || probe.backend.digest != expected_backend.digest
    {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner probe reported an enforcement backend kind, path, or digest that does not match the approved backend identity".to_owned(),
        });
    }
    let sandbox_probe = SandboxProbe {
        runner_path: probe.runner.path,
        runner_digest: probe.runner.digest,
        backend: BackendIdentity {
            kind: probe.backend.kind,
            path: probe.backend.path,
            digest: probe.backend.digest,
        },
    };
    tracing::debug!(
        runner = %config.runner,
        backend = %sandbox_probe.backend.path,
        digest = %sandbox_probe.backend.digest,
        "sandbox probe confirmed availability"
    );
    Ok(sandbox_probe)
}

fn validate_probe_digest(config: &SandboxConfig, value: &str, label: &str) -> Result<()> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    });
    if !valid {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!("the runner probe {label} digest is not a canonical sha256 digest"),
        });
    }
    Ok(())
}

/// Runs one program through the configured runner. The runner receives a JSON
/// request on standard input and must proxy the contained program's exit code
/// and output without host fallback.
pub fn execute(
    config: &SandboxConfig,
    request: ExecutionRequest<'_>,
    expected_runner: &RunnerIdentity,
) -> Result<std::process::Output> {
    execute_with_timeout(
        config,
        request,
        ExecutionOptions::default(),
        expected_runner,
    )
    .map(|result| result.output)
}

/// Executes a request with an optional runner deadline. A timeout terminates
/// the runner rather than retrying or invoking the requested program on host.
pub fn execute_with_timeout(
    config: &SandboxConfig,
    request: ExecutionRequest<'_>,
    options: ExecutionOptions,
    expected_runner: &RunnerIdentity,
) -> Result<ExecutionResult> {
    let project_root = request.project_root;
    let vcs_selection = request.vcs_selection;

    // Validate the complete producer-controlled request before examining the
    // runner or serializing/spawning its request. The runner repeats these
    // checks for untrusted wire input.
    validate_request_inputs(
        config,
        request.program,
        request.arguments,
        &request.environment,
    )?;
    let resolved_program = resolve_program(config, request.program, &request.environment)?;
    let argv_capacity = request
        .arguments
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid_request_input(config, "argv entry count overflows"))?;
    let mut argv = Vec::with_capacity(argv_capacity);
    argv.push(resolved_program.canonical_path.clone());
    argv.extend_from_slice(request.arguments);
    // Canonicalization can lengthen argv[0], so validate its final wire value
    // before creating the runner launch.
    validate_request_inputs(config, &argv[0], &argv[1..], &request.environment)?;

    // Rehash and revalidate the approval-bound enforcement backend immediately
    // before execution. A backend whose bytes, path, or kind no longer match the
    // identity confirmed by the probe fails closed rather than executing.
    let current_backend = backend_identity(config, project_root, vcs_selection)?;
    if &current_backend != request.backend {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the enforcement backend identity changed after the availability probe"
                .to_owned(),
        });
    }

    // Verify the trusted runner identity and bind its bytes before constructing
    // the request, so a runner that changed after approval fails closed before
    // any further work.
    let launch = checked_runner_launch(config, project_root, vcs_selection, expected_runner)?;

    let mut grants = Vec::with_capacity(request.read_only_mounts.len() + 8);
    match request.phase {
        ExecutionPhase::Authoring => {
            append_authoring_grants(config, request.component_dir, &mut grants)?;
        }
        ExecutionPhase::Verification => {
            let component_source = canonical_source_str(
                config,
                request.component_dir,
                "canonicalize component directory for sandbox context",
            )?;
            grants.push(SandboxGrant {
                source: component_source.clone(),
                destination: "/workspace/component".to_owned(),
                access: "read-only",
                purpose: "verification",
                identity: digest_label(component_source.as_bytes()),
            });
        }
    }
    for mount in request.read_only_mounts {
        let source = canonical_source_str(
            config,
            &mount.source,
            "canonicalize read-only context mount for sandbox manifest",
        )?;
        // Directory sources carry an explicit identity (for example the
        // lock-file digest of a vendored registry); single files derive theirs
        // from their own bytes. Large directory trees are never hashed file by
        // file.
        let identity = match &mount.identity {
            Some(identity) => identity.clone(),
            None => {
                let bytes = fs::read(&mount.source).map_err(|source_error| {
                    sandbox_error(
                        config,
                        "read approved context mount for sandbox identity",
                        source_error,
                    )
                })?;
                digest_label(&bytes)
            }
        };
        grants.push(SandboxGrant {
            source,
            destination: mount.destination.clone(),
            access: "read-only",
            purpose: "context",
            identity,
        });
    }
    // The toolchain identity is derived from the exact resolved argv[0] bytes,
    // and the conservative toolchain grant exposes exactly that executable at
    // its canonical path. This is a narrow, internally consistent toolchain
    // approval for the exact command executable; a full immutable
    // toolchain-set approval is deferred to the later runner integration.
    let toolchain_identity = resolved_program.digest.clone();
    grants.push(SandboxGrant {
        source: resolved_program.canonical_path.clone(),
        destination: resolved_program.canonical_path.clone(),
        access: "read-only",
        purpose: "toolchain",
        identity: toolchain_identity.clone(),
    });

    let resources = SandboxResources {
        wall_time_ms: match options.timeout {
            Some(limit) => bounded_millis(config, limit)?,
            None => DEFAULT_WALL_TIME_MS,
        },
        max_output_bytes: match options.output_limit {
            Some(limit) => bounded_output_bytes(config, limit)?,
            None => DEFAULT_MAX_OUTPUT_BYTES,
        },
        max_processes: DEFAULT_MAX_PROCESSES,
        max_files: DEFAULT_MAX_FILES,
        max_file_bytes: DEFAULT_MAX_FILE_BYTES,
        max_scratch_bytes: DEFAULT_MAX_SCRATCH_BYTES,
        // The generic system-toolchain path never mounts a Cargo home, so no
        // cache bound is declared; this preserves the established wire shape.
        max_cache_bytes: None,
    };

    let network = SandboxNetwork {
        mode: "deny",
        allowed_sources: Vec::new(),
    };

    let command_identity = digest_label(
        serde_json::to_string(&argv)
            .map_err(|error| KvistError::SandboxUnavailable {
                runner: config.runner.clone(),
                reason: format!("cannot canonicalize sandbox command identity: {error}"),
            })?
            .as_bytes(),
    );
    let mount_plan_identity = digest_label(
        serde_json::to_string(&grants)
            .map_err(|error| KvistError::SandboxUnavailable {
                runner: config.runner.clone(),
                reason: format!("cannot canonicalize sandbox mount plan identity: {error}"),
            })?
            .as_bytes(),
    );

    let sandbox_request = SandboxRequest {
        protocol: REQUEST_PROTOCOL,
        protocol_version: PROTOCOL_VERSION,
        phase: request.phase.wire(),
        argv: &argv,
        working_directory: "/workspace/component",
        environment: &request.environment,
        network,
        resources,
        identities: SandboxIdentities {
            runner: expected_runner.digest.clone(),
            backend: SandboxBackend {
                kind: request.backend.kind.clone(),
                path: request.backend.path.clone(),
                digest: request.backend.digest.clone(),
            },
            // The policy identity is the authenticated execution-approval
            // digest, not a locally computed unapproved hash.
            policy: request.policy_identity.to_owned(),
            toolchain: toolchain_identity.clone(),
            command: command_identity,
            mount_plan: mount_plan_identity,
        },
        grants: &grants,
        toolchain: SandboxToolchain::System {
            identity: toolchain_identity,
            root: resolved_program.canonical_path.clone(),
        },
        cache: None,
        scratch: None,
    };
    tracing::info!(
        program = %request.program,
        phase = ?request.phase,
        component = %request.component_dir.display(),
        "executing command in sandbox"
    );
    tracing::debug!(
        program = %request.program,
        grants_count = grants.len(),
        timeout = ?options.timeout,
        "dispatched sandbox execution request"
    );

    let encoded =
        serde_json::to_vec(&sandbox_request).map_err(|error| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!("cannot encode sandbox request: {error}"),
        })?;

    run_launched_bounded(
        &launch,
        config,
        EXECUTE_ARGUMENT,
        Some(&encoded),
        options.timeout,
        options.output_limit,
        options.live_stdout,
    )
}

/// Sandbox destination of the writable scratch for an offline Cargo
/// verification. Kept distinct from the verification workspace so the shared
/// runner never observes overlapping read-only and writable destinations.
pub const CARGO_SCRATCH_DEST: &str = "/workspace/scratch";
/// Sandbox working directory mounted read-only as the offline verification
/// workspace. Distinct from `CARGO_SCRATCH_DEST` and the component mount.
const CARGO_WORKING_DIRECTORY: &str = "/workspace/component";
/// The exact argument list an offline Cargo verification runs, appended after
/// the resolved `cargo` path: `cargo test --locked`.
const CARGO_VERIFY_ARGS: [&str; 2] = ["test", "--locked"];
/// The read-only bound applied to a vendored, approved Cargo home during
/// offline verification. Well under the shared runner's 64 GiB maximum.
const DEFAULT_MAX_CARGO_CACHE_BYTES: u64 = 32 * 1024 * 1024 * 1024;
/// Sandbox destination of the immutable Rust toolchain, mounted read-only.
const CARGO_TOOLCHAIN_DEST: &str = "/workspace/toolchain";
/// Sandbox destination of the approved, read-only Cargo-home cache. The shared
/// runner's closed Cargo verification topology requires an approved Cargo home;
/// the vendored registry (mounted at `VENDOR_SANDBOX_MOUNT`) is what `cargo`
/// actually resolves dependencies from, so the approved home need only be a
/// real, immutable directory the runner's topology can reference.
const CARGO_CARGO_HOME_DEST: &str = "/workspace/cargo-home";

/// Inputs for an offline, network-denied `cargo test --locked` verification
/// against a vendored Rust project.
///
/// Every host path must be a real, non-symlink directory: `toolchain_root`
/// contains cargo/rustc/rustlib/linker, `cargo_path` is the exact cargo
/// executable beneath it, `cargo_home` is the approved read-only Cargo home,
/// `vendored_registry` is the `cargo vendor` registry directory, `cargo_config`
/// holds the offline `.cargo/config.toml`, and `scratch_host_dir` is a writable
/// directory that becomes the target/HOME scratch inside the sandbox.
pub struct OfflineCargoVerification<'a> {
    pub project_root: &'a Path,
    pub vcs_selection: VcsSelection,
    pub toolchain_root: &'a Path,
    pub cargo_path: &'a Path,
    pub cargo_home: &'a Path,
    pub vendored_registry: &'a Path,
    pub cargo_config: &'a Path,
    pub scratch_host_dir: &'a Path,
    pub component_dir: &'a Path,
    /// Lock-file digest identifying every read-only vendored mount. This is the
    /// authoritative catalogue of the locked content, so it is both the manifest
    /// identity and the identity of every vendored-directory mount.
    pub lockfile_digest: String,
    pub policy_identity: &'a str,
    pub backend: &'a BackendIdentity,
    pub config: &'a SandboxConfig,
    pub expected_runner: &'a RunnerIdentity,
}

/// Build the closed version-one offline-Cargo verification request bytes.
///
/// Emits exactly the four-grant Cargo topology the shared runner enforces: a
/// read-only toolchain root, a read-only approved vendored Cargo home, a writable
/// scratch, and a read-only verification workspace, with network denied and the
/// exact Cargo environment allowlist. The `identities_*` arguments are the
/// approval-bound digests; the command and mount-plan identities are derived
/// deterministically so identical inputs serialize identically. This is a pure
/// function over on-disk paths so it can be validated against the shared runner's
/// validator without a live execution.
#[allow(clippy::too_many_arguments)]
fn build_offline_cargo_verification_request(
    config: &SandboxConfig,
    runner_identity: &str,
    backend: &BackendIdentity,
    policy_identity: &str,
    toolchain_identity: &str,
    lockfile_digest: &str,
    toolchain_root: &Path,
    cargo_path: &Path,
    cargo_home: &Path,
    vendored_registry: &Path,
    cargo_config: &Path,
    scratch_host_dir: &Path,
    component_dir: &Path,
) -> Result<Vec<u8>> {
    // The closed Cargo topology is six mutually non-overlapping grants. Their
    // sources are canonicalized once; destinations are the fixed sandbox paths.
    let toolchain_root = canonical_source_str(
        config,
        toolchain_root,
        "canonicalize cargo toolchain root for offline verification",
    )?;
    let cargo_home = canonical_source_str(
        config,
        cargo_home,
        "canonicalize approved cargo home for offline verification",
    )?;
    let vendored_registry = canonical_source_str(
        config,
        vendored_registry,
        "canonicalize vendored registry for offline verification",
    )?;
    let cargo_config = canonical_source_str(
        config,
        cargo_config,
        "canonicalize cargo config for offline verification",
    )?;
    let scratch_host_dir = canonical_source_str(
        config,
        scratch_host_dir,
        "canonicalize scratch directory for offline verification",
    )?;
    let component_dir = canonical_source_str(
        config,
        component_dir,
        "canonicalize verification workspace for offline verification",
    )?;
    let cargo_path = canonical_source_str(
        config,
        cargo_path,
        "canonicalize cargo executable for offline verification",
    )?;

    // The cargo executable lives beneath the toolchain root; its sandbox path is
    // the toolchain destination plus cargo's path relative to that root, so the
    // single toolchain grant covers the whole immutable toolchain.
    // `str::strip_prefix` is a lexical prefix match that keeps a leading slash
    // when cargo sits directly under the root, so use the component-aware `Path`
    // variant to obtain a clean relative remainder.
    let rel = Path::new(&cargo_path)
        .strip_prefix(&toolchain_root)
        .map(|relative| relative.to_string_lossy().into_owned())
        .map_err(|source| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!("cargo executable is not beneath the toolchain root: {source}"),
        })?;
    let cargo_sandbox_path = format!("{CARGO_TOOLCHAIN_DEST}/{rel}");

    // The approved Cargo-home endpoint identity binds the dependency-cache
    // grant; the toolchain and scratch/workspace identities bind their grants.
    // Each is a build-time, path-derived content claim over immutable,
    // approval-bound material (a directory is not hashed byte by byte).
    let cache_identity = digest_label(format!("kvist-cargo-home:{cargo_home}").as_bytes());
    let scratch_identity = digest_label(
        serde_json::to_string(&scratch_host_dir)
            .map_err(|error| KvistError::SandboxUnavailable {
                runner: config.runner.clone(),
                reason: format!("cannot canonicalize scratch identity: {error}"),
            })?
            .as_bytes(),
    );
    let verification_identity = digest_label(
        serde_json::to_string(&component_dir)
            .map_err(|error| KvistError::SandboxUnavailable {
                runner: config.runner.clone(),
                reason: format!("cannot canonicalize workspace identity: {error}"),
            })?
            .as_bytes(),
    );

    // `PATH` is the immutable toolchain's `bin` directory so `cargo` resolves
    // `rustc`, `rustdoc`, and the linker without a host search path; the runner
    // validates it as a single canonical absolute directory.
    let cargo_parent = Path::new(&cargo_sandbox_path)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_else(|| CARGO_TOOLCHAIN_DEST.to_owned());

    let mut environment: BTreeMap<String, String> = BTreeMap::new();
    environment.insert("HOME".to_owned(), format!("{CARGO_SCRATCH_DEST}/home"));
    environment.insert("PATH".to_owned(), cargo_parent);
    environment.insert("CARGO_HOME".to_owned(), CARGO_CARGO_HOME_DEST.to_owned());
    environment.insert(
        "CARGO_TARGET_DIR".to_owned(),
        format!("{CARGO_SCRATCH_DEST}/target"),
    );
    environment.insert("CARGO_NET_OFFLINE".to_owned(), "true".to_owned());

    let argv: Vec<String> = vec![
        cargo_sandbox_path.clone(),
        CARGO_VERIFY_ARGS[0].to_owned(),
        CARGO_VERIFY_ARGS[1].to_owned(),
    ];

    let grants: Vec<SandboxGrant> = vec![
        SandboxGrant {
            source: toolchain_root.clone(),
            destination: CARGO_TOOLCHAIN_DEST.to_owned(),
            access: "read-only",
            purpose: "toolchain",
            identity: toolchain_identity.to_owned(),
        },
        SandboxGrant {
            source: cargo_home.clone(),
            destination: CARGO_CARGO_HOME_DEST.to_owned(),
            access: "read-only",
            purpose: "dependency-cache",
            identity: cache_identity.clone(),
        },
        SandboxGrant {
            source: scratch_host_dir.clone(),
            destination: CARGO_SCRATCH_DEST.to_owned(),
            access: "read-write",
            purpose: "scratch",
            identity: scratch_identity.clone(),
        },
        SandboxGrant {
            source: component_dir.clone(),
            destination: CARGO_WORKING_DIRECTORY.to_owned(),
            access: "read-only",
            purpose: "verification",
            identity: verification_identity,
        },
        // The read-only vendored registry is what `cargo` actually resolves
        // dependencies from offline; the cargo config directs the resolver at it.
        // Each is pinned to its fixed destination and identified by the lock-file
        // digest, the authoritative catalogue of the locked content.
        SandboxGrant {
            source: vendored_registry.clone(),
            destination: crate::vendoring::VENDOR_SANDBOX_MOUNT.to_owned(),
            access: "read-only",
            purpose: "registry",
            identity: lockfile_digest.to_owned(),
        },
        SandboxGrant {
            source: cargo_config.clone(),
            destination: crate::vendoring::SANDBOX_CARGO_CONFIG_MOUNT.to_owned(),
            access: "read-only",
            purpose: "cargo-config",
            identity: lockfile_digest.to_owned(),
        },
    ];

    let resources = SandboxResources {
        wall_time_ms: DEFAULT_WALL_TIME_MS,
        max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        max_processes: DEFAULT_MAX_PROCESSES,
        max_files: DEFAULT_MAX_FILES,
        max_file_bytes: DEFAULT_MAX_FILE_BYTES,
        max_scratch_bytes: DEFAULT_MAX_SCRATCH_BYTES,
        max_cache_bytes: Some(DEFAULT_MAX_CARGO_CACHE_BYTES),
    };

    let command_identity = digest_label(
        serde_json::to_string(&argv)
            .map_err(|error| KvistError::SandboxUnavailable {
                runner: config.runner.clone(),
                reason: format!("cannot canonicalize command identity: {error}"),
            })?
            .as_bytes(),
    );
    let mount_plan_identity = digest_label(
        serde_json::to_string(&grants)
            .map_err(|error| KvistError::SandboxUnavailable {
                runner: config.runner.clone(),
                reason: format!("cannot canonicalize mount plan identity: {error}"),
            })?
            .as_bytes(),
    );

    let sandbox_request = SandboxRequest {
        protocol: REQUEST_PROTOCOL,
        protocol_version: PROTOCOL_VERSION,
        phase: ExecutionPhase::Verification.wire(),
        argv: &argv,
        working_directory: CARGO_WORKING_DIRECTORY,
        environment: &environment,
        network: SandboxNetwork {
            mode: "deny",
            allowed_sources: Vec::new(),
        },
        resources,
        identities: SandboxIdentities {
            runner: runner_identity.to_owned(),
            backend: SandboxBackend {
                kind: backend.kind.clone(),
                path: backend.path.clone(),
                digest: backend.digest.clone(),
            },
            policy: policy_identity.to_owned(),
            toolchain: toolchain_identity.to_owned(),
            command: command_identity,
            mount_plan: mount_plan_identity,
        },
        grants: &grants,
        toolchain: SandboxToolchain::Cargo {
            identity: toolchain_identity.to_owned(),
            // The toolchain root is the sandbox destination of the immutable
            // toolchain grant; `cargo` lives strictly beneath it in the same
            // sandbox namespace, so the runner binds the exact binary it runs.
            root: CARGO_TOOLCHAIN_DEST.to_owned(),
            // `cargo` is the sandbox destination of the executable, which must
            // equal `argv[0]` so the runner binds the exact binary it runs.
            cargo: cargo_sandbox_path.clone(),
        },
        cache: Some(SandboxCache {
            // The approved Cargo home is the read-only dependency-cache endpoint
            // mounted in the sandbox; the resolver's `CARGO_HOME` points at it.
            cargo_home: CARGO_CARGO_HOME_DEST.to_owned(),
            registry: format!("{CARGO_CARGO_HOME_DEST}/registry"),
            git: format!("{CARGO_CARGO_HOME_DEST}/git"),
            approved: Some(SandboxCacheEndpoint {
                destination: CARGO_CARGO_HOME_DEST.to_owned(),
                identity: cache_identity,
            }),
        }),
        scratch: Some(SandboxScratch {
            destination: CARGO_SCRATCH_DEST,
            identity: scratch_identity.clone(),
        }),
    };

    serde_json::to_vec(&sandbox_request).map_err(|error| KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: format!("cannot encode offline Cargo verification request: {error}"),
    })
}

/// Run an offline, network-denied `cargo test --locked` verification against an
/// approved, vendored Cargo home through the closed four-grant Cargo topology.
///
/// Unlike the generic system-toolchain path, this resolves the immutable Cargo
/// toolchain, mounts the read-only approved Cargo home plus a writable scratch
/// and verification workspace, and pins the exact `cargo test --locked` command.
/// A sandbox infrastructure failure fails closed and never builds on the host.
pub fn execute_offline_cargo_verification(
    config: &SandboxConfig,
    request: &OfflineCargoVerification<'_>,
    options: ExecutionOptions,
) -> Result<ExecutionResult> {
    let environment: BTreeMap<String, String> = BTreeMap::new();
    let program = request.cargo_path.to_string_lossy().into_owned();
    let verify_args: Vec<String> = CARGO_VERIFY_ARGS
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    validate_request_inputs(config, &program, &verify_args, &environment)?;

    // Rehash and revalidate the approval-bound enforcement backend immediately
    // before execution, as in the generic path.
    let current_backend = backend_identity(config, request.project_root, request.vcs_selection)?;
    if &current_backend != request.backend {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the enforcement backend identity changed after the availability probe"
                .to_owned(),
        });
    }
    let launch = checked_runner_launch(
        config,
        request.project_root,
        request.vcs_selection,
        request.expected_runner,
    )?;

    // Resolve the exact cargo executable so argv[0] and the toolchain block
    // agree, and derive the toolchain identity from its bytes like the system
    // path derives its identity from argv[0].
    let resolved = resolve_program(config, &program, &environment)?;
    let cargo_bytes = fs::read(&resolved.canonical_path).map_err(|source| KvistError::Io {
        operation: "read cargo executable for offline verification toolchain identity",
        path: PathBuf::from(&resolved.canonical_path),
        source,
    })?;
    let toolchain_identity = digest_label(&cargo_bytes);

    let encoded = build_offline_cargo_verification_request(
        config,
        &request.expected_runner.digest,
        request.backend,
        request.policy_identity,
        &toolchain_identity,
        &request.lockfile_digest,
        request.toolchain_root,
        Path::new(&resolved.canonical_path),
        request.cargo_home,
        request.vendored_registry,
        request.cargo_config,
        request.scratch_host_dir,
        request.component_dir,
    )?;

    tracing::info!(
        component = %request.component_dir.display(),
        cargo_home = %request.cargo_home.display(),
        "executing offline vendored cargo verification"
    );

    run_launched_bounded(
        &launch,
        config,
        EXECUTE_ARGUMENT,
        Some(&encoded),
        options.timeout,
        options.output_limit,
        options.live_stdout,
    )
}

/// The immutable Rust toolchain resolved for offline cargo verification.
#[derive(Debug)]
pub struct ResolvedCargoToolchain {
    /// The toolchain root containing cargo, rustc, rustlib, and the linker.
    pub root: PathBuf,
    /// The exact cargo executable beneath `root`.
    pub cargo: PathBuf,
}

/// Derive and validate an immutable toolchain layout from a resolved `cargo`
/// path, returning the toolchain root and the cargo executable beneath it.
///
/// The path is expected to be `<root>/bin/cargo`; `root` is that path's parent's
/// parent, and it must be a complete toolchain (it contains `rustlib`). cargo is
/// required to live strictly beneath `root`, so the single read-only toolchain
/// grant covers the whole immutable toolchain set. This is a pure validation over
/// a resolved path so it can be unit-tested without invoking `rustup`.
fn cargo_toolchain_from_path(cargo_str: &str, runner: &str) -> Result<ResolvedCargoToolchain> {
    let cargo = PathBuf::from(cargo_str);
    let bin_dir = cargo
        .parent()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: runner.to_owned(),
            reason: format!("resolved cargo path `{cargo_str}` has no parent directory"),
        })?;
    let root = bin_dir
        .parent()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: runner.to_owned(),
            reason: format!("resolved cargo path `{cargo_str}` is not inside a toolchain root"),
        })?;
    let cargo = cargo
        .canonicalize()
        .map_err(|source| KvistError::SandboxUnavailable {
            runner: runner.to_owned(),
            reason: format!("cannot canonicalize resolved cargo path `{cargo_str}`: {source}"),
        })?;
    let root = root
        .canonicalize()
        .map_err(|source| KvistError::SandboxUnavailable {
            runner: runner.to_owned(),
            reason: format!(
                "cannot canonicalize Rust toolchain root `{}`: {source}",
                root.display()
            ),
        })?;
    if !root.join("rustlib").is_dir() {
        return Err(KvistError::SandboxUnavailable {
            runner: runner.to_owned(),
            reason: format!(
                "Rust toolchain root `{}` is not a complete toolchain (missing rustlib)",
                root.display()
            ),
        });
    }
    if !cargo.starts_with(&root) {
        return Err(KvistError::SandboxUnavailable {
            runner: runner.to_owned(),
            reason: format!(
                "resolved cargo `{}` is not beneath the toolchain root `{}`",
                cargo.display(),
                root.display()
            ),
        });
    }
    Ok(ResolvedCargoToolchain { root, cargo })
}

/// Locate the immutable Rust toolchain (cargo, rustc, rustlib, linker) so an
/// offline `cargo` build can mount the whole toolchain read-only.
///
/// `rustup` is the canonical source of a complete toolchain set; resolution fails
/// closed with a non-secret, actionable message when `rustup` or the toolchain is
/// absent. This never falls back to building on the host.
pub fn resolve_cargo_toolchain(runner: &str) -> Result<ResolvedCargoToolchain> {
    let output = std::process::Command::new("rustup")
        .args(["which", "--cargo"])
        .output()
        .map_err(|source| KvistError::SandboxUnavailable {
            runner: runner.to_owned(),
            reason: format!("cannot invoke rustup to locate the Rust toolchain: {source}"),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(KvistError::SandboxUnavailable {
            runner: runner.to_owned(),
            reason: format!("`rustup which --cargo` failed: {}", stderr.trim()),
        });
    }
    let cargo_str = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if cargo_str.is_empty() {
        return Err(KvistError::SandboxUnavailable {
            runner: runner.to_owned(),
            reason: "`rustup which --cargo` returned no toolchain path".to_owned(),
        });
    }
    cargo_toolchain_from_path(&cargo_str, runner)
}

/// Provision a fresh, empty, read-only approved Cargo home for offline
/// verification.
///
/// Crate sources resolve from the vendored registry through the sandbox cargo
/// configuration, so the approved home need only be a real, immutable directory
/// the closed topology references. It holds only the standard, empty registry and
/// git layout cargo expects, and is regenerated on every run under the
/// Kvist-owned `.kvist/` directory.
fn provision_cargo_home(project_root: &Path) -> Result<PathBuf> {
    let cargo_home = project_root.join(".kvist").join("cargo-home");
    for child in ["registry/cache", "git/db"] {
        std::fs::create_dir_all(cargo_home.join(child)).map_err(|source| KvistError::Io {
            operation: "provision read-only cargo home for offline cargo verification",
            path: cargo_home.join(child),
            source,
        })?;
    }
    Ok(cargo_home)
}

/// Run offline, network-denied `cargo test --locked` verification for a vendored
/// Rust project through the closed Cargo topology.
///
/// Enforces vendoring readiness (failing closed when the project is not vendored,
/// the lockfile drifted, or material is missing), resolves the immutable
/// toolchain, provisions a fresh read-only approved Cargo home and a writable,
/// disjoint target/HOME scratch, and delegates to
/// [`execute_offline_cargo_verification`]. A sandbox infrastructure failure fails
/// closed and never builds on the host.
pub fn run_offline_cargo_verification(
    config: &SandboxConfig,
    project_root: &Path,
    vcs_selection: VcsSelection,
    probe: &SandboxProbe,
    policy_identity: &str,
    component_dir: &Path,
    options: ExecutionOptions,
) -> Result<ExecutionResult> {
    // Enforce vendoring readiness against the manifest and lock file. This fails
    // closed with a clear message when the project is not vendored, the lockfile
    // has drifted since vendoring, or a registry or Git dependency is absent.
    let enforcement = crate::language_vendoring::enforce_vendoring(project_root)?;
    if enforcement.language != "rust" {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "verification routed to offline cargo verification but the project is not Rust"
                .to_owned(),
        });
    }

    // The read-only vendored registry and sandbox cargo configuration are the
    // mounts an offline build needs; their host sources come from the enforced
    // manifest. Each is pinned to its fixed sandbox destination.
    let mut vendored_registry = None;
    let mut cargo_config = None;
    for mount in &enforcement.mounts {
        match mount.destination.as_str() {
            crate::vendoring::VENDOR_SANDBOX_MOUNT => {
                vendored_registry = Some(mount.source.clone())
            }
            crate::vendoring::SANDBOX_CARGO_CONFIG_MOUNT => {
                cargo_config = Some(mount.source.clone())
            }
            _ => {}
        }
    }
    let vendored_registry = vendored_registry.ok_or_else(|| KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: "the enforced Rust vendoring manifest declares no vendored-registry mount"
            .to_owned(),
    })?;
    let cargo_config = cargo_config.ok_or_else(|| KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: "the enforced Rust vendoring manifest declares no sandbox cargo-config mount"
            .to_owned(),
    })?;

    let toolchain = resolve_cargo_toolchain(&config.runner)?;
    let cargo_home = provision_cargo_home(project_root)?;
    let scratch = tempfile::tempdir().map_err(|source| KvistError::Io {
        operation: "create writable scratch for offline cargo verification",
        path: PathBuf::from("."),
        source,
    })?;

    tracing::info!(
        component = %component_dir.display(),
        vendored = %vendored_registry.display(),
        "executing offline vendored cargo verification"
    );

    let request = OfflineCargoVerification {
        project_root,
        vcs_selection,
        toolchain_root: &toolchain.root,
        cargo_path: &toolchain.cargo,
        cargo_home: &cargo_home,
        vendored_registry: &vendored_registry,
        cargo_config: &cargo_config,
        scratch_host_dir: scratch.path(),
        component_dir,
        lockfile_digest: enforcement.lockfile_digest,
        policy_identity,
        backend: &probe.backend,
        config,
        expected_runner: &RunnerIdentity {
            canonical_path: probe.runner_path.clone(),
            digest: probe.runner_digest.clone(),
        },
    };

    execute_offline_cargo_verification(config, &request, options)
}

/// Spawns the verified runner launch with one protocol argument, optionally
/// writes a request to its standard input, and captures its output under a hard
/// deadline and combined-output cap using the bounded capture and process-group
/// termination machinery. It never calls the unbounded `Command::output()`.
/// Clears the registered process group when the supervision scope ends.
struct ProcessGroupGuard {
    /// Registered process group ID.
    pgid: i32,
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        // Clear only when this guard's registration is still the active one,
        // so a newer nested registration is never clobbered by an older scope.
        let _ = agent_runtime::clear_active_process_group_if(self.pgid);
    }
}

fn run_launched_bounded(
    launch: &VerifiedRunnerLaunch,
    config: &SandboxConfig,
    argument: &str,
    stdin_bytes: Option<&[u8]>,
    timeout: Option<Duration>,
    output_limit: Option<usize>,
    mut live_stdout: Option<LiveStdoutSink>,
) -> Result<ExecutionResult> {
    let mut attempts = 0;
    let child = loop {
        let res = launch
            .command(config)
            .arg(argument)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        match res {
            Err(ref e) if e.raw_os_error() == Some(26) && attempts < 5 => {
                attempts += 1;
                tracing::warn!(
                    runner = %config.runner,
                    attempt = attempts,
                    "transient Text file busy (os error 26) spawning runner copy; retrying in 10ms"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            other => break other,
        }
    };
    let mut child =
        child.map_err(|source| sandbox_error(config, "start sandbox runner", source))?;
    // The runner runs in its own process group (PGID == runner PID). Register
    // it so SIGINT/SIGTERM reaches the runner and its descendants.
    let pgid = i32::try_from(child.id()).map_err(|_| KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: "sandbox runner process ID exceeds the supported range".to_owned(),
    })?;
    agent_runtime::set_active_process_group(pgid);
    let _group_guard = ProcessGroupGuard { pgid };
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "sandbox runner did not provide standard input".to_owned(),
        })?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "sandbox runner did not provide standard output".to_owned(),
        })?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "sandbox runner did not provide standard error".to_owned(),
        })?;
    set_nonblocking(&stdin, config, "configure sandbox runner standard input")?;
    set_nonblocking(&stdout, config, "configure sandbox runner standard output")?;
    set_nonblocking(&stderr, config, "configure sandbox runner standard error")?;

    let started = Instant::now();
    let stdin_bytes = stdin_bytes.unwrap_or_default();
    let mut stdin_offset = 0;
    let mut stdin = Some(stdin);
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut status = None;
    let mut captured_stdout = Vec::new();
    let mut captured_stderr = Vec::new();
    let mut captured_total = 0_usize;

    loop {
        if let Some(stream) = stdin.as_mut() {
            if stdin_offset < stdin_bytes.len() {
                match stream.write(&stdin_bytes[stdin_offset..]) {
                    Ok(0) => stdin = None,
                    Ok(count) => stdin_offset += count,
                    Err(source) if source.kind() == io::ErrorKind::WouldBlock => {}
                    Err(source) if source.kind() == io::ErrorKind::BrokenPipe => stdin = None,
                    Err(source) => {
                        return Err(sandbox_error(config, "write sandbox request", source));
                    }
                }
            } else {
                stdin = None;
            }
        }

        let mut output_limit_exceeded = false;
        if stdout_open {
            let before = captured_stdout.len();
            output_limit_exceeded |= drain_stream_bounded(
                &mut stdout,
                &mut captured_stdout,
                &mut stdout_open,
                &mut captured_total,
                output_limit,
                config,
                "read sandbox runner stdout",
            )?;
            if let Some(sink) = live_stdout.as_mut()
                && captured_stdout.len() > before
            {
                sink(&captured_stdout[before..]);
            }
        }
        if stderr_open {
            output_limit_exceeded |= drain_stream_bounded(
                &mut stderr,
                &mut captured_stderr,
                &mut stderr_open,
                &mut captured_total,
                output_limit,
                config,
                "read sandbox runner stderr",
            )?;
        }

        if status.is_none() {
            status = child
                .try_wait()
                .map_err(|source| sandbox_error(config, "wait for sandbox runner", source))?;
        }

        if agent_runtime::take_interrupted() {
            tracing::info!(runner = %config.runner, "sandbox execution interrupted by signal");
            // The shared handler already signalled the group; give it a short
            // grace period to exit before escalating.
            for _ in 0..20 {
                let exited = child
                    .try_wait()
                    .map_err(|source| {
                        sandbox_error(config, "wait for interrupted sandbox runner", source)
                    })?
                    .is_some();
                if exited {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            terminate_process_group(&child, config, "interrupted")?;
            let status = match status {
                Some(status) => status,
                None => child.wait().map_err(|source| {
                    sandbox_error(config, "wait for interrupted sandbox runner", source)
                })?,
            };
            return Ok(ExecutionResult {
                output: std::process::Output {
                    status,
                    stdout: captured_stdout,
                    stderr: captured_stderr,
                },
                timed_out: false,
                output_limit_exceeded: false,
                cancelled: true,
            });
        }

        let timed_out = timeout.is_some_and(|limit| started.elapsed() >= limit);
        if output_limit_exceeded || timed_out {
            terminate_process_group(
                &child,
                config,
                if output_limit_exceeded {
                    "output-limited"
                } else {
                    "timed-out"
                },
            )?;
            let status = match status {
                Some(status) => status,
                None => child.wait().map_err(|source| {
                    sandbox_error(config, "wait for terminated sandbox runner", source)
                })?,
            };
            if timed_out {
                tracing::warn!(timeout = ?timeout, "sandbox execution timed out");
            }
            if output_limit_exceeded {
                tracing::warn!(limit = ?output_limit, "sandbox execution exceeded output limit");
            }
            // A direct runner can exit while an escaped descendant keeps either
            // pipe open. Do not wait for that holder after terminating its group.
            return Ok(ExecutionResult {
                output: std::process::Output {
                    status,
                    stdout: captured_stdout,
                    stderr: captured_stderr,
                },
                timed_out,
                output_limit_exceeded,
                cancelled: false,
            });
        }

        if let Some(status) = status
            && !stdout_open
            && !stderr_open
        {
            tracing::debug!(status = ?status, "sandbox runner exited normally");
            return Ok(ExecutionResult {
                output: std::process::Output {
                    status,
                    stdout: captured_stdout,
                    stderr: captured_stderr,
                },
                timed_out: false,
                output_limit_exceeded: false,
                cancelled: false,
            });
        }

        std::thread::sleep(SUPERVISION_POLL_INTERVAL);
    }
}

fn terminate_process_group(
    child: &std::process::Child,
    config: &SandboxConfig,
    reason: &'static str,
) -> Result<()> {
    let pid = i32::try_from(child.id()).map_err(|_| KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: "sandbox runner process ID exceeds the supported range".to_owned(),
    })?;
    match killpg(Pid::from_raw(pid), Signal::SIGKILL) {
        Ok(()) | Err(Errno::ESRCH) => {}
        Err(source) => {
            return Err(KvistError::SandboxUnavailable {
                runner: config.runner.clone(),
                reason: format!("cannot terminate {reason} sandbox runner process group: {source}"),
            });
        }
    }
    Ok(())
}

fn validate_request_inputs(
    config: &SandboxConfig,
    program: &str,
    arguments: &[String],
    environment: &BTreeMap<String, String>,
) -> Result<()> {
    let argc = arguments
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid_request_input(config, "argv entry count overflows"))?;
    if argc > MAX_ARGV_ENTRIES {
        return Err(invalid_request_input(
            config,
            &format!(
                "argv has {argc} entries, exceeding the {MAX_ARGV_ENTRIES}-entry runner limit"
            ),
        ));
    }
    validate_request_value(config, program, "argv entry 0")?;
    for (index, argument) in arguments.iter().enumerate() {
        validate_request_value(config, argument, &format!("argv entry {}", index + 1))?;
    }

    if environment.len() > MAX_SANDBOX_ENVIRONMENT_ENTRIES {
        return Err(invalid_request_input(
            config,
            &format!(
                "environment has {} entries, exceeding the {MAX_SANDBOX_ENVIRONMENT_ENTRIES}-entry runner limit",
                environment.len()
            ),
        ));
    }
    for (name, value) in environment {
        if name.len() > MAX_SANDBOX_ENVIRONMENT_NAME_BYTES {
            return Err(invalid_request_input(
                config,
                &format!(
                    "environment name exceeds the {MAX_SANDBOX_ENVIRONMENT_NAME_BYTES}-byte runner limit"
                ),
            ));
        }
        if !is_portable_environment_name(name) {
            return Err(invalid_request_input(
                config,
                "environment names must be portable identifiers",
            ));
        }
        validate_request_value(config, value, "environment value")?;
    }
    Ok(())
}

fn validate_request_value(config: &SandboxConfig, value: &str, label: &str) -> Result<()> {
    if value.len() > MAX_REQUEST_VALUE_BYTES {
        return Err(invalid_request_input(
            config,
            &format!("{label} exceeds the {MAX_REQUEST_VALUE_BYTES}-byte runner limit"),
        ));
    }
    if value.contains('\0') {
        return Err(invalid_request_input(
            config,
            &format!("{label} contains an interior NUL byte"),
        ));
    }
    Ok(())
}

fn is_portable_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(byte) if byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn invalid_request_input(config: &SandboxConfig, reason: &str) -> KvistError {
    KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: format!("invalid sandbox request before runner execution: {reason}"),
    }
}

fn set_nonblocking<T: AsFd>(
    stream: &T,
    config: &SandboxConfig,
    operation: &'static str,
) -> Result<()> {
    let flags = fcntl(stream, FcntlArg::F_GETFL)
        .map_err(|source| sandbox_error(config, operation, source))?;
    let flags = OFlag::from_bits_truncate(flags);
    fcntl(stream, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))
        .map_err(|source| sandbox_error(config, operation, source))?;
    Ok(())
}

fn drain_stream_bounded<R: Read>(
    stream: &mut R,
    captured: &mut Vec<u8>,
    open: &mut bool,
    captured_total: &mut usize,
    output_limit: Option<usize>,
    config: &SandboxConfig,
    operation: &'static str,
) -> Result<bool> {
    let mut buffer = [0_u8; 8_192];
    let mut drained = 0;
    while drained < STREAM_DRAIN_BUDGET {
        let read_len = buffer.len().min(STREAM_DRAIN_BUDGET - drained);
        match stream.read(&mut buffer[..read_len]) {
            Ok(0) => {
                *open = false;
                return Ok(false);
            }
            Ok(count) => {
                drained += count;
                let remaining =
                    output_limit.map_or(usize::MAX, |limit| limit.saturating_sub(*captured_total));
                let accepted = count.min(remaining);
                captured.extend_from_slice(&buffer[..accepted]);
                *captured_total += accepted;
                if accepted < count {
                    return Ok(true);
                }
            }
            Err(source) if source.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(source) => return Err(sandbox_error(config, operation, source)),
        }
    }
    Ok(false)
}

fn validate_runner(
    config: &SandboxConfig,
    project_root: &Path,
    vcs_selection: VcsSelection,
) -> Result<()> {
    let runner = Path::new(&config.runner);
    if !runner.is_absolute() {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner path must be absolute".to_owned(),
        });
    }
    let metadata = fs::symlink_metadata(runner)
        .map_err(|source| sandbox_error(config, "inspect trusted sandbox runner", source))?;
    // The runner can be a regular non-symlink file or a directory containing
    // handler scripts. Symlinks are rejected for security reasons.
    if metadata.file_type().is_symlink() {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner must be a regular non-symlink file or directory".to_owned(),
        });
    }
    if !metadata.file_type().is_file() && !metadata.file_type().is_dir() {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner must be a regular file or directory".to_owned(),
        });
    }
    let canonical_project = project_root
        .canonicalize()
        .map_err(|source| sandbox_error(config, "canonicalize project root", source))?;
    let inspection = vcs::inspect(project_root, vcs_selection, Vec::new());
    let worktree_root =
        inspection
            .repository_root
            .ok_or_else(|| KvistError::SandboxUnavailable {
                runner: config.runner.clone(),
                reason: format!(
                    "cannot resolve the selected VCS worktree root: {}",
                    inspection.diagnostic.unwrap_or(inspection.summary)
                ),
            })?;
    let canonical_worktree = worktree_root.canonicalize().map_err(|source| {
        sandbox_error(config, "canonicalize selected VCS worktree root", source)
    })?;
    let canonical_runner = runner
        .canonicalize()
        .map_err(|source| sandbox_error(config, "canonicalize trusted sandbox runner", source))?;
    if canonical_runner.starts_with(&canonical_project) {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner must be installed outside the project root".to_owned(),
        });
    }
    if canonical_runner.starts_with(&canonical_worktree) {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner must be installed outside the selected VCS worktree".to_owned(),
        });
    }
    Ok(())
}

/// Builds the environment visible to the sandbox runner and declared for the
/// sandboxed child. Test execution can further restrict it with `additional`.
pub fn allowed_environment(
    config: &SandboxConfig,
    additional: Option<&[String]>,
) -> BTreeMap<String, String> {
    config
        .environment_allowlist
        .iter()
        .filter_map(|name| {
            if let Some(allowed) = additional.as_ref()
                && !allowed.contains(name)
            {
                return None;
            }
            std::env::var(name).ok().map(|value| (name.clone(), value))
        })
        .collect()
}

fn checked_runner_launch(
    config: &SandboxConfig,
    project_root: &Path,
    vcs_selection: VcsSelection,
    expected_runner: &RunnerIdentity,
) -> Result<VerifiedRunnerLaunch> {
    let current = runner_identity(config, project_root, vcs_selection)?;
    if &current != expected_runner {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "trusted sandbox runner identity or content changed after approval".to_owned(),
        });
    }
    VerifiedRunnerLaunch::create(expected_runner)
}

/// A portable, security-hardened runner launch handle.
///
/// On Linux, it uses `execve` for direct binary execution with no shell,
/// environment pollution, and no path traversal. The runner binary is copied
/// to a secure, user-owned state directory before execution.
///
/// On non-Linux platforms, it falls back to `Command::spawn` with `env_clear()`
/// as a portable alternative that still avoids shell interpretation. The same
/// environment allowlist is enforced in both cases.
///
/// Invariant: the runner binary is copied to a secure, user-owned state
/// directory with restricted permissions (`0o500` file, `0o700` directory) before
/// execution.
struct VerifiedRunnerLaunch {
    #[cfg(target_os = "linux")]
    _file: File,
    _copy_path: PathBuf,
    launch_path: PathBuf,
    _temp_dir: tempfile::TempDir,
}

impl VerifiedRunnerLaunch {
    fn create(expected_runner: &RunnerIdentity) -> Result<Self> {
        let canonical_path = Path::new(&expected_runner.canonical_path);
        let is_directory_runner = canonical_path.is_dir();

        // Collect the bytes from the runner (file or directory).
        let bytes = if is_directory_runner {
            // Directory runner: collect handler scripts in sorted order.
            let mut bytes = Vec::new();
            let entries =
                fs::read_dir(canonical_path).map_err(|source| KvistError::SandboxUnavailable {
                    runner: expected_runner.canonical_path.clone(),
                    reason: format!("read trusted sandbox runner directory: {source}"),
                })?;
            let mut sorted_entries: Vec<_> = entries
                .filter_map(|entry| {
                    entry.ok().and_then(|e| {
                        let path = e.path();
                        if path.is_file() { Some(path) } else { None }
                    })
                })
                .collect();
            sorted_entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
            for path in sorted_entries {
                if let Ok(contents) = fs::read(&path) {
                    bytes.extend_from_slice(&contents);
                }
            }
            bytes
        } else {
            fs::read(canonical_path).map_err(|source| KvistError::SandboxUnavailable {
                runner: expected_runner.canonical_path.clone(),
                reason: format!("read verified sandbox runner: {source}"),
            })?
        };

        let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
        if digest != expected_runner.digest {
            return Err(KvistError::SandboxUnavailable {
                runner: expected_runner.canonical_path.clone(),
                reason: "trusted sandbox runner content changed before launch".to_owned(),
            });
        }

        let temp_dir = tempfile::tempdir_in(
            std::env::var("KVIST_TEMPORARY_DIR")
                .ok()
                .unwrap_or_else(|| {
                    std::env::var("TMPDIR").unwrap_or_else(|_| {
                        std::env::var("TMP").unwrap_or_else(|_| {
                            std::env::var("TEMP").unwrap_or_else(|_| ".".into())
                        })
                    })
                }),
        )
        .map_err(|source| KvistError::SandboxUnavailable {
            runner: expected_runner.canonical_path.clone(),
            reason: format!("create secure runner state directory: {source}"),
        })?;

        #[cfg(target_os = "linux")]
        {
            let (file, copy_path, launch_path) = create_descriptor_bound_copy(
                canonical_path,
                &temp_dir,
                &bytes,
                is_directory_runner,
            )?;
            Ok(Self {
                _file: file,
                _copy_path: copy_path,
                launch_path,
                _temp_dir: temp_dir,
            })
        }

        #[cfg(not(target_os = "linux"))]
        {
            let (_file, copy_path, launch_path) = create_descriptor_bound_copy(
                canonical_path,
                &temp_dir,
                &bytes,
                is_directory_runner,
            )?;
            Ok(Self {
                _copy_path: copy_path,
                launch_path,
                _temp_dir: temp_dir,
            })
        }
    }

    fn command(&self, config: &SandboxConfig) -> Command {
        use std::os::unix::process::CommandExt;

        let mut command = Command::new(&self.launch_path);
        command.env_clear().process_group(0);
        for (name, value) in allowed_environment(config, None) {
            command.env(name, value);
        }
        command
    }
}

#[cfg(target_os = "linux")]
fn create_descriptor_bound_copy(
    runner_path: &Path,
    temp_dir: &tempfile::TempDir,
    bytes: &[u8],
    is_directory_runner: bool,
) -> Result<(File, PathBuf, PathBuf)> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::fs::PermissionsExt;

    let temp_dir_path = temp_dir.path();
    if is_directory_runner {
        secure_copy_directory_or_file(runner_path, temp_dir_path, is_directory_runner)?;
    }

    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|_error| KvistError::SandboxUnavailable {
        runner: runner_path.to_string_lossy().into_owned(),
        reason: "generate descriptor-bound runner copy name: random number generation failed"
            .to_owned(),
    })?;
    let copy_path = temp_dir_path.join(format!(
        "runner-{}",
        nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ));
    let mut options = OpenOptions::new();
    options.write(true).read(true).create_new(true).mode(0o700);
    let mut writable_file = options.open(&copy_path).map_err(|source| {
        KvistError::SandboxUnavailable {
            runner: runner_path.to_string_lossy().into_owned(),
            reason: format!(
                "create descriptor-bound runner copy: permission denied or file exists: {source}"
            ),
        }
    })?;
    if let Err(source) = writable_file
        .write_all(bytes)
        .and_then(|()| writable_file.sync_all())
    {
        drop(writable_file);
        let _ = fs::remove_file(&copy_path);
        return Err(KvistError::SandboxUnavailable {
            runner: runner_path.to_string_lossy().into_owned(),
            reason: format!("write descriptor-bound runner copy: write error: {source}"),
        });
    }
    drop(writable_file);
    fs::set_permissions(&copy_path, fs::Permissions::from_mode(0o500)).map_err(|source| {
        KvistError::SandboxUnavailable {
            runner: runner_path.to_string_lossy().into_owned(),
            reason: format!("protect descriptor-bound runner copy: permission denied: {source}"),
        }
    })?;
    let file = File::open(&copy_path).map_err(|source| KvistError::SandboxUnavailable {
        runner: runner_path.to_string_lossy().into_owned(),
        reason: format!("reopen descriptor-bound runner copy: permission denied: {source}"),
    })?;
    nix::fcntl::fcntl(
        &file,
        nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
    )
    .map_err(|source| KvistError::SandboxUnavailable {
        runner: runner_path.to_string_lossy().into_owned(),
        reason: format!("retain descriptor-bound runner across execve: syscall failed: {source}"),
    })?;
    let launch_path = copy_path.clone();
    Ok((file, copy_path, launch_path))
}

#[cfg(not(target_os = "linux"))]
fn create_descriptor_bound_copy(
    _runner_path: &Path,
    _temp_dir: &tempfile::TempDir,
    _bytes: &[u8],
    _is_directory_runner: bool,
) -> Result<(File, PathBuf, PathBuf)> {
    Err(KvistError::SandboxUnavailable {
        runner: _runner_path.to_string_lossy().into_owned(),
        reason: "descriptor-bound verified execution is only supported on Linux".to_owned(),
    })
}

#[cfg(target_os = "linux")]
fn secure_copy_directory_or_file(
    runner_path: &Path,
    temp_dir_path: &Path,
    is_directory_runner: bool,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    // Copy the runner to the temporary directory.
    if is_directory_runner {
        // Directory runner: copy all handler scripts.
        let entries =
            fs::read_dir(runner_path).map_err(|source| KvistError::SandboxUnavailable {
                runner: runner_path.to_string_lossy().into_owned(),
                reason: format!("copy trusted sandbox runner directory: {source}"),
            })?;
        for entry in entries {
            let entry = entry.map_err(|source| KvistError::SandboxUnavailable {
                runner: runner_path.to_string_lossy().into_owned(),
                reason: format!("copy trusted sandbox runner directory: {source}"),
            })?;
            let src_path = entry.path();
            if src_path.is_file() {
                let file_name =
                    src_path
                        .file_name()
                        .ok_or_else(|| KvistError::SandboxUnavailable {
                            runner: runner_path.to_string_lossy().into_owned(),
                            reason: "trusted sandbox runner directory entry has no file name"
                                .to_owned(),
                        })?;
                let dest_path = temp_dir_path.join(file_name);
                fs::copy(&src_path, &dest_path).map_err(|source| {
                    KvistError::SandboxUnavailable {
                        runner: runner_path.to_string_lossy().into_owned(),
                        reason: format!("copy trusted sandbox runner file: {source}"),
                    }
                })?;
            }
        }
    } else {
        // File runner: copy the runner file.
        let dest_path = temp_dir_path.join(format!("runner-{}", sha256_hash(runner_path)));
        fs::copy(runner_path, &dest_path).map_err(|source| KvistError::SandboxUnavailable {
            runner: runner_path.to_string_lossy().into_owned(),
            reason: format!("copy trusted sandbox runner: {source}"),
        })?;
    }

    // Set restrictive permissions on the copy directory.
    let permissions = fs::Permissions::from_mode(0o700);
    fs::set_permissions(temp_dir_path, permissions).map_err(|source| {
        KvistError::SandboxUnavailable {
            runner: runner_path.to_string_lossy().into_owned(),
            reason: format!("set restrictive permissions on runner copy directory: {source}"),
        }
    })?;

    Ok(())
}

/// Returns the SHA-256 hash of the given path as a hexadecimal string.
#[cfg(target_os = "linux")]
fn sha256_hash(path: &Path) -> String {
    let mut hasher = Sha256::new();
    if let Ok(contents) = fs::read(path) {
        hasher.update(&contents);
    } else {
        hasher.update(path.to_string_lossy().as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Returns a sandbox error with the given context.
fn sandbox_error(
    config: &SandboxConfig,
    context: &str,
    source: impl std::error::Error,
) -> KvistError {
    KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: format!("sandbox error in {context}: {source}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;

    fn sample_backend() -> BackendIdentity {
        BackendIdentity {
            kind: "bubblewrap".to_owned(),
            path: "/usr/bin/bwrap".to_owned(),
            digest: format!("sha256:{}", "a".repeat(64)),
        }
    }

    fn build_request(phase: ExecutionPhase, argv: &[String], grants: Vec<SandboxGrant>) -> Vec<u8> {
        let network = SandboxNetwork {
            mode: "deny",
            allowed_sources: Vec::new(),
        };
        let resources = SandboxResources {
            wall_time_ms: 5000,
            max_output_bytes: 65536,
            max_processes: DEFAULT_MAX_PROCESSES,
            max_files: DEFAULT_MAX_FILES,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_scratch_bytes: DEFAULT_MAX_SCRATCH_BYTES,
            max_cache_bytes: None,
        };
        let toolchain_identity = digest_label(b"kvist-system-toolchain:/usr");
        let backend = sample_backend();
        let request = SandboxRequest {
            protocol: REQUEST_PROTOCOL,
            protocol_version: PROTOCOL_VERSION,
            phase: phase.wire(),
            argv,
            working_directory: "/workspace/component",
            environment: &BTreeMap::new(),
            network,
            resources,
            identities: SandboxIdentities {
                runner: format!("sha256:{}", "b".repeat(64)),
                backend: SandboxBackend {
                    kind: backend.kind.clone(),
                    path: backend.path.clone(),
                    digest: backend.digest.clone(),
                },
                policy: digest_label(b"policy"),
                toolchain: toolchain_identity.clone(),
                command: digest_label(b"command"),
                mount_plan: digest_label(b"mount"),
            },
            grants: &grants,
            toolchain: SandboxToolchain::System {
                identity: toolchain_identity,
                root: "/usr".to_owned(),
            },
            cache: None,
            scratch: None,
        };
        serde_json::to_vec(&request).expect("serialize request")
    }

    #[test]
    fn emits_the_closed_version_one_authoring_shape() {
        let argv = vec!["/usr/bin/true".to_owned()];
        let grants = vec![SandboxGrant {
            source: "/host/component".to_owned(),
            destination: "/workspace/component".to_owned(),
            access: "read-write",
            purpose: "authoring",
            identity: digest_label(b"component"),
        }];
        let encoded = build_request(ExecutionPhase::Authoring, &argv, grants);
        let text = String::from_utf8(encoded).expect("utf8");
        assert!(text.contains("\"protocol\":\"kvist-sandbox-request-v1\""));
        assert!(text.contains("\"protocol_version\":1"));
        assert!(text.contains("\"phase\":\"authoring\""));
        assert!(text.contains("\"argv\":[\"/usr/bin/true\"]"));
        assert!(text.contains("\"network\":{\"mode\":\"deny\",\"allowed_sources\":[]}"));
        // No retired fields.
        assert!(!text.contains("\"program\""));
        assert!(!text.contains("\"arguments\""));
        assert!(!text.contains("\"mounts\""));
        assert!(!text.contains("\"context_files\""));
    }

    #[test]
    fn serialization_is_deterministic() {
        let argv = vec!["/usr/bin/true".to_owned()];
        let grants = vec![SandboxGrant {
            source: "/host/component".to_owned(),
            destination: "/workspace/component".to_owned(),
            access: "read-only",
            purpose: "verification",
            identity: digest_label(b"component"),
        }];
        let first = build_request(ExecutionPhase::Verification, &argv, grants.clone());
        let second = build_request(ExecutionPhase::Verification, &argv, grants);
        assert_eq!(
            first, second,
            "identical requests must serialize identically"
        );
    }

    #[test]
    fn probe_digest_validation_rejects_noncanonical_values() {
        let config = SandboxConfig {
            runner: "/runner".to_owned(),
            backend: "/usr/bin/true".to_owned(),
            environment_allowlist: Vec::new(),
            acquisition: crate::config::AcquisitionConfig::default(),
        };
        assert!(
            validate_probe_digest(&config, &format!("sha256:{}", "a".repeat(64)), "runner").is_ok()
        );
        assert!(validate_probe_digest(&config, "sha1:deadbeef", "runner").is_err());
        assert!(
            validate_probe_digest(&config, &format!("sha256:{}", "A".repeat(64)), "runner")
                .is_err()
        );
    }

    #[test]
    fn producer_rejects_runner_invalid_argv_and_environment_before_launch() {
        let config = SandboxConfig {
            runner: "/runner-not-started".to_owned(),
            backend: "/backend-not-started".to_owned(),
            environment_allowlist: Vec::new(),
            acquisition: crate::config::AcquisitionConfig::default(),
        };
        assert!(
            validate_request_inputs(
                &config,
                "/usr/bin/echo",
                &["p".repeat(MAX_REQUEST_VALUE_BYTES + 1)],
                &BTreeMap::new(),
            )
            .is_err(),
            "an oversized rendered prompt argument must fail before runner execution"
        );
        assert!(
            validate_request_inputs(
                &config,
                "/usr/bin/echo",
                &vec!["arg".to_owned(); MAX_ARGV_ENTRIES],
                &BTreeMap::new(),
            )
            .is_err(),
            "argv including its program must not exceed the runner limit"
        );

        let excessive_environment = (0..=MAX_SANDBOX_ENVIRONMENT_ENTRIES)
            .map(|index| (format!("ENV_{index}"), "value".to_owned()))
            .collect();
        assert!(
            validate_request_inputs(&config, "/usr/bin/echo", &[], &excessive_environment,)
                .is_err(),
            "environment entry count must not exceed the runner limit"
        );
        for (name, value) in [
            ("9INVALID".to_owned(), "value".to_owned()),
            (
                "A".repeat(MAX_SANDBOX_ENVIRONMENT_NAME_BYTES + 1),
                "value".to_owned(),
            ),
            ("VALID".to_owned(), "x".repeat(MAX_REQUEST_VALUE_BYTES + 1)),
        ] {
            let environment = BTreeMap::from([(name, value)]);
            assert!(
                validate_request_inputs(&config, "/usr/bin/echo", &[], &environment).is_err(),
                "runner-invalid environment input must fail before runner execution"
            );
        }
    }

    fn launched_test_runner(script: &str) -> (TempDir, SandboxConfig, VerifiedRunnerLaunch) {
        let directory = tempfile::tempdir().expect("create runner fixture");
        let runner = directory.path().join("runner");
        fs::write(&runner, script).expect("write runner fixture");
        fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
            .expect("make runner fixture executable");
        let canonical_path = runner
            .canonicalize()
            .expect("canonicalize runner fixture")
            .to_string_lossy()
            .into_owned();
        let identity = RunnerIdentity {
            canonical_path,
            digest: digest_label(&fs::read(&runner).expect("read runner fixture")),
        };
        let config = SandboxConfig {
            runner: runner.to_string_lossy().into_owned(),
            backend: "/usr/bin/true".to_owned(),
            environment_allowlist: Vec::new(),
            acquisition: crate::config::AcquisitionConfig::default(),
        };
        let launch = VerifiedRunnerLaunch::create(&identity).expect("create verified launch");
        (directory, config, launch)
    }

    #[test]
    fn supervision_times_out_when_exited_runner_leaves_pipe_holder() {
        let (_directory, config, launch) = launched_test_runner("#!/bin/sh\n(sleep 5) &\nexit 0\n");
        let started = Instant::now();
        let result = run_launched_bounded(
            &launch,
            &config,
            "--test",
            None,
            Some(Duration::from_millis(75)),
            Some(1024),
            None,
        )
        .expect("supervision must return after deadline");
        assert!(result.timed_out);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a descendant retaining an output descriptor must not delay return"
        );
    }

    #[test]
    fn supervision_kills_pipe_holder_when_exited_runner_overflows_output() {
        let (_directory, config, launch) =
            launched_test_runner("#!/bin/sh\n(sleep 5) &\nprintf 'overflow'\nexit 0\n");
        let started = Instant::now();
        let result = run_launched_bounded(
            &launch,
            &config,
            "--test",
            None,
            Some(Duration::from_secs(1)),
            Some(4),
            None,
        )
        .expect("supervision must return after output overflow");
        assert!(result.output_limit_exceeded);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "output overflow must terminate descendants even after parent exit"
        );
    }

    /// A valid offline Cargo verification request must satisfy the shared runner's
    /// validator, which enforces the closed four-grant Cargo topology (`cargo
    /// test --locked`, denied network, the exact Cargo environment allowlist).
    /// This is a producer-side contract test: it proves the builder emits a
    /// well-formed request without requiring a live runner.
    #[test]
    fn offline_cargo_request_satisfies_the_shared_runner_validator() {
        use kvist_sandbox_runner::protocol::{Access, Purpose, Toolchain};
        use kvist_sandbox_runner::validation;
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("tmp root");
        let project = root.path().join("project");
        fs::create_dir_all(&project).expect("project dir");

        // Immutable toolchain root holding an exact cargo executable beneath it.
        let toolchain = tempfile::tempdir().expect("toolchain");
        let bin = toolchain.path().join("bin");
        fs::create_dir_all(&bin).expect("toolchain bin");
        let cargo = bin.join("cargo");
        fs::write(&cargo, "#!/bin/sh\n").expect("cargo file");
        fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755))
            .expect("cargo executable");
        fs::write(bin.join("rustc"), "").expect("rustc file");

        // Vendored, approved Cargo home with its registry and git children.
        let cargo_home = tempfile::tempdir().expect("cargo home");
        fs::create_dir_all(cargo_home.path().join("registry/cache/c")).expect("registry subdir");
        fs::create_dir_all(cargo_home.path().join("git/db")).expect("git subdir");

        // Writable scratch directory and the verification workspace (the project).
        let scratch = tempfile::tempdir().expect("scratch");

        // Read-only vendored registry and its offline resolver configuration.
        let vendored_registry = tempfile::tempdir().expect("vendored registry");
        let cargo_config = tempfile::tempdir().expect("cargo config");
        let lockfile_digest = digest_label(b"kvist-cargo-lockfile");

        let config = SandboxConfig {
            runner: "/usr/bin/bwrap".to_owned(),
            backend: "/usr/bin/true".to_owned(),
            environment_allowlist: Vec::new(),
            acquisition: crate::config::AcquisitionConfig::default(),
        };
        let backend = sample_backend();
        let runner_digest = format!("sha256:{}", "c".repeat(64));
        let toolchain_identity = digest_label(b"kvist-cargo-toolchain");

        let encoded = build_offline_cargo_verification_request(
            &config,
            &runner_digest,
            &backend,
            &digest_label(b"policy"),
            &toolchain_identity,
            &lockfile_digest,
            toolchain.path(),
            &cargo,
            cargo_home.path(),
            vendored_registry.path(),
            cargo_config.path(),
            scratch.path(),
            &project,
        )
        .expect("build offline cargo verification request");
        let request = validation::parse_and_validate(&encoded)
            .expect("offline cargo request must satisfy the closed Cargo topology validator");

        // The closed topology is exactly the four base grants plus the two
        // read-only vendored extensions: toolchain, dependency-cache, scratch,
        // verification, registry, and cargo-config.
        assert_eq!(
            request.grants.len(),
            6,
            "closed Cargo topology is six grants"
        );
        let purposes: Vec<_> = request.grants.iter().map(|grant| grant.purpose).collect();
        assert!(purposes.contains(&Purpose::Registry));
        assert!(purposes.contains(&Purpose::CargoConfig));

        // The vendored registry mount is pinned, read-only, at its fixed
        // destination, and identified by the lock-file digest.
        let registry = request
            .grants
            .iter()
            .find(|grant| grant.purpose == Purpose::Registry)
            .expect("registry grant");
        assert_eq!(registry.destination, crate::vendoring::VENDOR_SANDBOX_MOUNT);
        assert_eq!(registry.access, Access::ReadOnly);
        assert_eq!(registry.identity, lockfile_digest);

        let cargo_config_mount = request
            .grants
            .iter()
            .find(|grant| grant.purpose == Purpose::CargoConfig)
            .expect("cargo-config grant");
        assert_eq!(
            cargo_config_mount.destination,
            crate::vendoring::SANDBOX_CARGO_CONFIG_MOUNT
        );
        assert_eq!(cargo_config_mount.access, Access::ReadOnly);
        assert_eq!(cargo_config_mount.identity, lockfile_digest);

        // The Cargo toolchain `cargo` must equal argv[0] (the sandbox path of the
        // executable), so the runner binds the exact binary it runs.
        let Toolchain::Cargo { cargo, .. } = &request.toolchain else {
            panic!("offline verification uses a Cargo toolchain");
        };
        assert_eq!(*cargo, request.argv[0]);
        assert!(
            request.argv[0].starts_with(&format!("{CARGO_TOOLCHAIN_DEST}/")),
            "cargo must be the sandbox path under the toolchain root"
        );
    }

    #[test]
    fn cargo_toolchain_from_path_accepts_a_complete_toolchain_layout() {
        let root = tempfile::tempdir().expect("toolchain root");
        fs::create_dir_all(root.path().join("rustlib")).expect("rustlib");
        fs::create_dir_all(root.path().join("bin")).expect("bin");
        fs::write(root.path().join("bin").join("cargo"), b"cargo").expect("cargo");

        let cargo = root.path().join("bin").join("cargo");
        let resolved = cargo_toolchain_from_path(&cargo.to_string_lossy(), "/runner")
            .expect("complete toolchain validates");

        // The root is the canonical toolchain directory, and cargo is a real file
        // strictly beneath it, so the single read-only toolchain grant covers it.
        assert_eq!(
            resolved.root,
            root.path().canonicalize().expect("canonical root")
        );
        assert!(resolved.cargo.is_file());
        assert!(resolved.cargo.starts_with(&resolved.root));
    }

    #[test]
    fn cargo_toolchain_from_path_rejects_a_toolchain_missing_rustlib() {
        let root = tempfile::tempdir().expect("toolchain root");
        fs::create_dir_all(root.path().join("bin")).expect("bin");
        fs::write(root.path().join("bin").join("cargo"), b"cargo").expect("cargo");

        let cargo = root.path().join("bin").join("cargo");
        let error = cargo_toolchain_from_path(&cargo.to_string_lossy(), "/runner")
            .expect_err("a toolchain without rustlib is not complete");
        assert!(
            error.to_string().contains("rustlib"),
            "diagnostic should name the missing rustlib marker: {error}"
        );
    }

    #[test]
    fn cargo_toolchain_from_path_rejects_a_path_outside_a_toolchain_root() {
        let error = cargo_toolchain_from_path("cargo", "/runner")
            .expect_err("a bare program name is not inside a toolchain root");
        assert!(
            error.to_string().contains("toolchain root"),
            "diagnostic should name the missing toolchain root: {error}"
        );
    }

    #[test]
    fn provision_cargo_home_creates_a_real_empty_home() {
        let project = tempfile::tempdir().expect("project root");
        let cargo_home = provision_cargo_home(project.path()).expect("provision cargo home");
        assert!(cargo_home.is_dir());
        assert!(cargo_home.join("registry/cache").is_dir());
        assert!(cargo_home.join("git/db").is_dir());
    }

    #[test]
    fn run_offline_cargo_verification_fails_closed_without_vendoring() {
        let project = tempfile::tempdir().expect("project root");
        let config = SandboxConfig {
            runner: "/usr/bin/bwrap".to_owned(),
            backend: "/usr/bin/true".to_owned(),
            environment_allowlist: Vec::new(),
            acquisition: crate::config::AcquisitionConfig::default(),
        };
        let probe = SandboxProbe {
            runner_path: "/runner".to_owned(),
            runner_digest: String::new(),
            backend: sample_backend(),
        };
        let result = run_offline_cargo_verification(
            &config,
            project.path(),
            VcsSelection::Git,
            &probe,
            "policy",
            project.path(),
            ExecutionOptions {
                timeout: None,
                output_limit: None,
                live_stdout: None,
            },
        );
        assert!(
            matches!(result, Err(KvistError::VendoringUnavailable { .. })),
            "verification must fail closed when the project is not vendored"
        );
    }
}
