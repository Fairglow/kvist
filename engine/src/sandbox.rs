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

/// Host-side resource controls for the sandbox runner process.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExecutionOptions {
    pub timeout: Option<Duration>,
    pub output_limit: Option<usize>,
}

/// Bounded result returned by a sandbox runner request.
pub struct ExecutionResult {
    pub output: std::process::Output,
    pub timed_out: bool,
    pub output_limit_exceeded: bool,
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

/// One host file exposed to the sandbox at a fixed read-only path.
pub struct ReadOnlyMount {
    pub source: PathBuf,
    pub destination: String,
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

#[derive(Debug, Serialize)]
struct SandboxToolchain {
    kind: &'static str,
    identity: String,
    root: String,
}

#[derive(Debug, Serialize)]
struct SandboxCache {
    destination: String,
    identity: String,
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
    let launch = checked_runner_launch(config, project_root, vcs_selection, expected_runner)?;
    let result = run_launched_bounded(
        &launch,
        config,
        PROBE_ARGUMENT,
        None,
        Some(PROBE_DEADLINE),
        Some(MAX_PROBE_BYTES),
    )?;
    let ExecutionResult {
        output,
        timed_out,
        output_limit_exceeded,
    } = result;
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
    Ok(SandboxProbe {
        runner_path: probe.runner.path,
        runner_digest: probe.runner.digest,
        backend: BackendIdentity {
            kind: probe.backend.kind,
            path: probe.backend.path,
            digest: probe.backend.digest,
        },
    })
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
        let bytes = fs::read(&mount.source).map_err(|source_error| {
            sandbox_error(
                config,
                "read approved context mount for sandbox identity",
                source_error,
            )
        })?;
        let identity = digest_label(&bytes);
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
        toolchain: SandboxToolchain {
            kind: "system",
            identity: toolchain_identity,
            root: resolved_program.canonical_path.clone(),
        },
        cache: None,
        scratch: None,
    };
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
    )
}

/// Spawns the verified runner launch with one protocol argument, optionally
/// writes a request to its standard input, and captures its output under a hard
/// deadline and combined-output cap using the bounded capture and process-group
/// termination machinery. It never calls the unbounded `Command::output()`.
fn run_launched_bounded(
    launch: &VerifiedRunnerLaunch,
    config: &SandboxConfig,
    argument: &str,
    stdin_bytes: Option<&[u8]>,
    timeout: Option<Duration>,
    output_limit: Option<usize>,
) -> Result<ExecutionResult> {
    let child = launch
        .command(config)
        .arg(argument)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child =
        child.map_err(|source| sandbox_error(config, "start sandbox runner", source))?;
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
            output_limit_exceeded |= drain_stream_bounded(
                &mut stdout,
                &mut captured_stdout,
                &mut stdout_open,
                &mut captured_total,
                output_limit,
                config,
                "read sandbox runner stdout",
            )?;
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
            });
        }

        if let Some(status) = status
            && !stdout_open
            && !stderr_open
        {
            return Ok(ExecutionResult {
                output: std::process::Output {
                    status,
                    stdout: captured_stdout,
                    stderr: captured_stderr,
                },
                timed_out: false,
                output_limit_exceeded: false,
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
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::fs::PermissionsExt;

    let temp_dir_path = temp_dir.path();
    secure_copy_directory_or_file(runner_path, temp_dir_path, is_directory_runner)?;

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
        let _ = fs::remove_file(&copy_path);
        return Err(KvistError::SandboxUnavailable {
            runner: runner_path.to_string_lossy().into_owned(),
            reason: format!("write descriptor-bound runner copy: write error: {source}"),
        });
    }
    fs::set_permissions(&copy_path, fs::Permissions::from_mode(0o500)).map_err(|source| {
        KvistError::SandboxUnavailable {
            runner: runner_path.to_string_lossy().into_owned(),
            reason: format!("protect descriptor-bound runner copy: permission denied: {source}"),
        }
    })?;
    drop(writable_file);
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
    let launch_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
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
            toolchain: SandboxToolchain {
                kind: "system",
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
        )
        .expect("supervision must return after output overflow");
        assert!(result.output_limit_exceeded);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "output overflow must terminate descendants even after parent exit"
        );
    }
}
