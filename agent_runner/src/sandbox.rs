//! Sandbox request construction and execution.
//!
//! Every agent tool call becomes one closed version-one `SandboxRequest` in the
//! `Authoring` phase, reusing the shared `kvist_sandbox_runner` protocol types so
//! the wire shape is identical to the runner's own contract. Execution spawns the
//! installed runner, supervises output, and fails closed whenever the sandbox is
//! unavailable rather than executing on the host.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use agent_runtime::CancellationToken;
use kvist_sandbox_runner::protocol::{
    Access, BackendIdentity, BackendKind, Grant, Identities, Network, NetworkMode, Phase, Purpose,
    Resources, SandboxRequest, Toolchain,
};
use kvist_sandbox_runner::validation;
use sha2::{Digest, Sha256};

use crate::config::SandboxPaths;
use crate::error::{Error, Result, io_error};

/// The CLI argument that selects the version-one request mode on the runner.
const REQUEST_ARGUMENT: &str = "--kvist-sandbox-request-v1";
/// The accepted request protocol identifier.
const PROTOCOL_ID: &str = "kvist-sandbox-request-v1";
/// The single supported protocol version.
const PROTOCOL_VERSION: u32 = 1;
/// The maximum encoded request size before parsing.
const MAX_REQUEST_BYTES: usize = 1 << 20;
const MAX_IDENTITY_BYTES: u64 = 256 << 20;
const MAX_SCOPE_ENTRIES: usize = 1_000_000;
const MAX_SCOPE_DEPTH: usize = 128;
const MAX_SCOPE_PATH_BYTES: usize = 32 << 20;
const MAX_PREFLIGHT_TIME: Duration = Duration::from_secs(30);

/// System `PATH` made available inside the sandbox so tools resolve by name.
const SANDBOX_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Reasonable, well-bounded execution limits for one tool call.
pub const fn default_resources() -> Resources {
    Resources {
        wall_time_ms: 120_000,
        max_output_bytes: 1 << 20,
        max_processes: 256,
        max_files: 1 << 12,
        max_file_bytes: 64 << 20,
        max_scratch_bytes: 1 << 30,
        max_cache_bytes: None,
    }
}

/// Inputs needed to build one sandbox request.
#[derive(Debug, Clone)]
pub struct BuildRequest<'a> {
    /// The argv to execute inside the sandbox; `argv[0]` must be absolute.
    pub argv: &'a [String],
    /// The host working directory, granted read-write at the write root.
    pub working_directory: &'a Path,
    /// Regular non-link host context files to expose read-only at `/context/N`.
    /// Directories, missing paths, overlaps and unsupported entries fail closed.
    pub read_roots: &'a [PathBuf],
    /// Environment variables to forward, filtered for portable, safe names.
    pub environment: BTreeMap<String, String>,
    /// The tool authority policy, bound as the policy identity.
    pub policy: &'a crate::config::ToolPolicy,
    /// Execution resource limits.
    pub resources: Resources,
}

impl Default for BuildRequest<'_> {
    fn default() -> Self {
        BuildRequest {
            argv: &[],
            working_directory: Path::new("/"),
            read_roots: &[],
            environment: BTreeMap::new(),
            policy: default_policy(),
            resources: default_resources(),
        }
    }
}

/// A process-wide default policy, giving [`BuildRequest::default`] a stable
/// reference instead of a dangling temporary.
static DEFAULT_POLICY: OnceLock<crate::config::ToolPolicy> = OnceLock::new();

fn default_policy() -> &'static crate::config::ToolPolicy {
    DEFAULT_POLICY.get_or_init(crate::config::ToolPolicy::default)
}

/// The outcome of one sandbox execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutcome {
    /// Whether the process produced an exit status.
    pub exited: bool,
    /// The exit code, when the process exited.
    pub status: Option<i32>,
    /// Captured stdout bytes.
    pub stdout: Vec<u8>,
    /// Captured stderr bytes.
    pub stderr: Vec<u8>,
    /// True when the wall-clock deadline elapsed.
    pub timed_out: bool,
    /// True when the output limit was exceeded.
    pub output_limit_exceeded: bool,
    /// True when the caller requested cancellation.
    pub cancelled: bool,
}

impl ToolOutcome {
    /// A zeroed, failed outcome used to record a tool that was rejected by
    /// policy or failed before producing real output (so the record stays
    /// faithful without inventing sandbox results).
    pub fn rejected() -> Self {
        Self::rejected_with("")
    }

    /// A rejected outcome whose reason is carried in `stderr`, so a tool the
    /// loop refused (policy denial, or a failure before it produced real output)
    /// still reports *why* to the model on the next turn instead of an empty
    /// result. The process never ran, so it is not reported as exited.
    pub fn rejected_with(reason: impl Into<String>) -> Self {
        ToolOutcome {
            exited: false,
            status: None,
            stdout: Vec::new(),
            stderr: reason.into().into_bytes(),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }
    }

    /// The captured stdout as text, truncated at `limit` bytes and lossily
    /// decoded so non-UTF-8 output never panics.
    pub fn output_text(&self, limit: usize) -> String {
        truncate_text(&self.stdout, limit)
    }

    /// The captured stderr as text, truncated at `limit` bytes.
    pub fn error_text(&self, limit: usize) -> String {
        truncate_text(&self.stderr, limit)
    }

    /// Whether execution failed to produce an observed, uninterrupted zero exit.
    pub fn failed(&self) -> bool {
        !self.exited
            || self.status != Some(0)
            || self.timed_out
            || self.output_limit_exceeded
            || self.cancelled
    }
}

fn truncate_text(bytes: &[u8], limit: usize) -> String {
    let bounded = if bytes.len() > limit {
        &bytes[..limit]
    } else {
        bytes
    };
    String::from_utf8_lossy(bounded).into_owned()
}

/// The minimal grant representation required to build a request.
#[derive(Debug, Clone)]
struct GrantWire {
    source: String,
    destination: String,
    access: Access,
    purpose: Purpose,
    identity: String,
}

/// Builds a closed version-one `Authoring` phase request.
pub fn build_request(sandbox: &SandboxPaths, request: &BuildRequest) -> Result<SandboxRequest> {
    build_request_cancellable(sandbox, request, &CancellationToken::new())
}

pub(crate) fn build_request_cancellable(
    sandbox: &SandboxPaths,
    request: &BuildRequest,
    cancellation: &CancellationToken,
) -> Result<SandboxRequest> {
    let preflight = Preflight {
        cancellation,
        started: Instant::now(),
    };
    preflight.check()?;
    let runner_identity = read_file_identity("sandbox runner", &sandbox.runner, &preflight)?;
    let backend_identity = backend_identity("sandbox backend", &sandbox.backend, &preflight)?;

    let workdir = request.working_directory.canonicalize().map_err(|source| {
        io_error(
            "canonicalize working directory for the sandbox",
            Some(&request.working_directory.to_string_lossy()),
            source,
        )
    })?;
    let runner = sandbox
        .runner
        .canonicalize()
        .map_err(|source| io_error("resolve installed sandbox runner", None, source))?;
    if runner.starts_with(&workdir) {
        return Err(Error::SandboxBuild {
            reason:
                "the independently installed sandbox runner must be outside the writable workspace"
                    .to_owned(),
        });
    }

    // The writable authoring grant maps the working directory to the write root.
    let write_root = request.policy.write_root.clone();
    if write_root.is_empty() || !write_root.starts_with('/') {
        return Err(Error::SandboxBuild {
            reason: "the write root must be a non-empty absolute path".to_owned(),
        });
    }
    check_writable_scope(&workdir, &preflight)?;
    let authoring = grant(
        &workdir,
        &write_root,
        Access::ReadWrite,
        Purpose::Authoring,
        &digest_input(b"authoring", workdir.to_string_lossy().as_bytes()),
        &preflight,
    )?;

    // Read-only context grants for any declared read roots. A read root that
    // overlaps the write root would mount the same host path both read-write (as
    // the authoring scope) and read-only (as a context grant), so such an overlap
    // is rejected up front per the sandbox request contract. Destinations stay
    // disjoint, so the runner's overlap check also passes.
    let mut wires = vec![authoring];
    for (index, root) in request.read_roots.iter().enumerate() {
        preflight.check()?;
        let metadata = std::fs::symlink_metadata(root).map_err(|e| {
            io_error(
                "inspect declared read-only context file",
                Some(&root.to_string_lossy()),
                e,
            )
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(Error::SandboxBuild {
                reason: "read_roots supports regular non-link context files only, not directories"
                    .to_owned(),
            });
        }
        let canonical_root = root.canonicalize().map_err(|e| {
            io_error(
                "resolve declared read-only context file",
                Some(&root.to_string_lossy()),
                e,
            )
        })?;
        if roots_overlap(&canonical_root, &workdir) {
            return Err(Error::SandboxBuild {
                reason: "read-only context file overlaps the writable workspace".to_owned(),
            });
        }
        wires.push(build_context_grant(&canonical_root, index, &preflight)?);
    }
    if wires.is_empty() {
        return Err(Error::SandboxBuild {
            reason: "at least the authoring grant is required".to_owned(),
        });
    }
    let grants: Vec<Grant> = wires
        .iter()
        .map(|wire| Grant {
            source: wire.source.clone(),
            destination: wire.destination.clone(),
            access: wire.access,
            purpose: wire.purpose,
            identity: wire.identity.clone(),
        })
        .collect();

    let command_identity = digest(&digest_input(b"command", &join_argv(request.argv)));
    let toolchain_identity = digest(&digest_input(b"toolchain", b"/usr"));
    let mount_plan_identity = digest(&grant_plan_identity(&wires));

    let environment = filter_environment(&request.environment);

    let request = SandboxRequest {
        protocol: PROTOCOL_ID.to_owned(),
        protocol_version: PROTOCOL_VERSION,
        phase: Phase::Authoring,
        argv: request.argv.to_vec(),
        working_directory: write_root.clone(),
        environment,
        network: Network {
            mode: NetworkMode::Deny,
            allowed_sources: Vec::new(),
        },
        resources: request.resources,
        identities: Identities {
            runner: runner_identity,
            backend: backend_identity,
            policy: request.policy.identity(),
            toolchain: toolchain_identity.clone(),
            command: command_identity,
            mount_plan: mount_plan_identity,
        },
        toolchain: Toolchain::System {
            identity: toolchain_identity,
            root: "/usr".to_owned(),
        },
        grants: grants.clone(),
        cache: None,
        scratch: None,
    };

    // Validate locally before spawning so a malformed request fails fast and is
    // testable without the installed runner. The runner repeats these checks.
    validation::validate(&request).map_err(|error| Error::SandboxBuild {
        reason: error.to_string(),
    })?;

    preflight.check()?;
    Ok(request)
}

struct Preflight<'a> {
    cancellation: &'a CancellationToken,
    started: Instant,
}

impl Preflight<'_> {
    fn check(&self) -> Result<()> {
        crate::executor::check_cancelled(self.cancellation)?;
        if self.started.elapsed() >= MAX_PREFLIGHT_TIME {
            return Err(Error::SandboxBuild {
                reason: "sandbox preflight exceeded 30 seconds; narrow the working directory or reduce context files".into(),
            });
        }
        Ok(())
    }
}

fn backend_identity(
    label: &str,
    path: &Path,
    preflight: &Preflight<'_>,
) -> Result<BackendIdentity> {
    let identity = read_file_identity(label, path, preflight)?;
    let canonical = path.canonicalize().map_err(|source| {
        io_error(
            &format!("canonicalize {label}"),
            Some(&path.to_string_lossy()),
            source,
        )
    })?;
    Ok(BackendIdentity {
        kind: BackendKind::Bubblewrap,
        path: canonical.to_string_lossy().into_owned(),
        digest: identity,
    })
}

fn read_file_identity(label: &str, path: &Path, preflight: &Preflight<'_>) -> Result<String> {
    preflight.check()?;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|source| {
            io_error(
                &format!("open {label}"),
                Some(&path.to_string_lossy()),
                source,
            )
        })?;
    let metadata = file.metadata().map_err(|source| {
        io_error(
            &format!("inspect {label}"),
            Some(&path.to_string_lossy()),
            source,
        )
    })?;
    if !metadata.file_type().is_file() {
        return Err(Error::SandboxUnavailable {
            runner: Some(path.to_string_lossy().into_owned()),
            reason: format!("{label} must be a regular non-link file"),
        });
    }
    if metadata.len() > MAX_IDENTITY_BYTES {
        return Err(Error::SandboxBuild {
            reason: format!("{label} exceeds the 256 MiB identity bound"),
        });
    }
    let mut hash = Sha256::new();
    let mut bytes = [0_u8; 65536];
    let mut total = 0_u64;
    loop {
        preflight.check()?;
        let count = file.read(&mut bytes).map_err(|source| {
            io_error(
                &format!("read {label}"),
                Some(&path.to_string_lossy()),
                source,
            )
        })?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_IDENTITY_BYTES {
            return Err(Error::SandboxBuild {
                reason: format!("{label} grew beyond the 256 MiB identity bound"),
            });
        }
        hash.update(&bytes[..count]);
    }
    preflight.check()?;
    Ok(format!("sha256:{}", hex::encode(hash.finalize())))
}

fn build_context_grant(root: &Path, index: usize, preflight: &Preflight<'_>) -> Result<GrantWire> {
    let destination = format!("/context/{index}");
    let identity = read_file_identity("read-only context file", root, preflight)?;
    Ok(GrantWire {
        source: root.to_string_lossy().into_owned(),
        destination,
        access: Access::ReadOnly,
        purpose: Purpose::Context,
        identity,
    })
}

fn check_writable_scope(workdir: &Path, preflight: &Preflight<'_>) -> Result<()> {
    // `workdir` is canonical in the caller, so it is a real directory: the scope
    // root. The sandbox bind-mounts it read-write, so every symlink inside it is
    // live to the sandboxed process. The escape risk is a symlink whose resolved
    // target lies OUTSIDE the scope -- following it for writing leaves the write
    // root. Symlinks that stay inside the scope are common (Node's `.bin` links,
    // in-project aliases) and safe, so they are allowed; only scope-escaping links
    // fail the build, named with the target they point at.
    let mut stack: Vec<(PathBuf, usize)> = Vec::new();
    let mut visited: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
    let mut path_bytes = 0;
    queue_scope_directory(
        workdir.to_owned(),
        0,
        &mut stack,
        &mut visited,
        &mut path_bytes,
    )?;
    let mut count = 0;
    while let Some((dir, depth)) = stack.pop() {
        preflight.check()?;
        let entries = std::fs::read_dir(&dir).map_err(|source| {
            io_error(
                "inspect working directory scope for the sandbox",
                Some(&dir.display().to_string()),
                source,
            )
        })?;
        for entry in entries {
            preflight.check()?;
            admit_scope_entry(&mut count)?;
            let entry = entry.map_err(|source| {
                io_error(
                    "enumerate working directory scope",
                    Some(&dir.to_string_lossy()),
                    source,
                )
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|source| {
                io_error(
                    "inspect working directory scope for the sandbox",
                    Some(&path.display().to_string()),
                    source,
                )
            })?;
            if file_type.is_dir() {
                queue_scope_directory(path, depth + 1, &mut stack, &mut visited, &mut path_bytes)?;
                continue;
            }
            if !file_type.is_symlink() {
                continue;
            }
            let target = std::fs::read_link(&path).map_err(|source| {
                io_error(
                    "read working directory symbolic link",
                    Some(&path.to_string_lossy()),
                    source,
                )
            })?;
            let resolved = resolve_link_target(&target, &path);
            if target_escapes_scope(&resolved, workdir) {
                return Err(Error::SandboxBuild {
                    reason: format!(
                        "working directory `{}` contains a symbolic link `{}` pointing at
                         `{}` which escapes the writable scope `{}`; a write through such a
                         link could leave the scope, so remove the link, move its target
                         inside the working directory, or narrow the working directory",
                        workdir.display(),
                        path.display(),
                        target.to_string_lossy(),
                        workdir.display(),
                    ),
                });
            }
            // In-scope link is allowed, but if it points at a directory keep
            // walking its canonical target so links reachable through it are still
            // checked -- without ever following the link at sandbox runtime.
            let canonical = match std::fs::canonicalize(&path) {
                Ok(canonical) => canonical,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(io_error(
                        "resolve working directory symbolic link",
                        Some(&path.to_string_lossy()),
                        source,
                    ));
                }
            };
            if !canonical.starts_with(workdir) {
                return Err(Error::SandboxBuild {
                    reason: "symbolic link left the working directory during inspection".into(),
                });
            }
            let metadata = std::fs::metadata(&canonical).map_err(|source| {
                io_error(
                    "inspect symbolic link target",
                    Some(&path.to_string_lossy()),
                    source,
                )
            })?;
            if metadata.is_dir() {
                let target_depth = canonical
                    .strip_prefix(workdir)
                    .map_err(|_| Error::SandboxBuild {
                        reason: "symbolic link left the working directory during inspection".into(),
                    })?
                    .components()
                    .count();
                queue_scope_directory(
                    canonical,
                    target_depth,
                    &mut stack,
                    &mut visited,
                    &mut path_bytes,
                )?;
            }
        }
    }
    Ok(())
}

fn admit_scope_entry(count: &mut usize) -> Result<()> {
    *count = count.saturating_add(1);
    if *count > MAX_SCOPE_ENTRIES {
        return Err(Error::SandboxBuild {
            reason: "working directory scan exceeds 1000000 entries; narrow the working directory"
                .into(),
        });
    }
    Ok(())
}

fn queue_scope_directory(
    path: PathBuf,
    depth: usize,
    stack: &mut Vec<(PathBuf, usize)>,
    visited: &mut std::collections::BTreeSet<PathBuf>,
    path_bytes: &mut usize,
) -> Result<()> {
    if depth > MAX_SCOPE_DEPTH {
        return Err(Error::SandboxBuild {
            reason: "working directory scan exceeds depth 128; narrow the working directory".into(),
        });
    }
    if visited.contains(&path) {
        return Ok(());
    }
    let charge = path.as_os_str().len().saturating_mul(2);
    let total = path_bytes.saturating_add(charge);
    if total > MAX_SCOPE_PATH_BYTES {
        return Err(Error::SandboxBuild { reason: "working directory scan exceeds the 32 MiB path-storage bound; narrow the working directory".into() });
    }
    *path_bytes = total;
    visited.insert(path.clone());
    stack.push((path, depth));
    Ok(())
}

/// Join a link target onto the directory that holds it: absolute targets are
/// returned unchanged, relative targets resolve against the link's own
/// directory (matching how the kernel resolves the link).
fn resolve_link_target(target: &Path, link_path: &Path) -> PathBuf {
    if target.is_absolute() {
        target.to_path_buf()
    } else {
        link_path
            .parent()
            .map(|parent| parent.join(target))
            .unwrap_or_else(|| target.to_path_buf())
    }
}

/// Whether `candidate` resolves to a location outside `scope_root`. Symlinks in
/// the path are resolved when they exist; a dangling candidate is normalized
/// lexically, and one that cannot be proven inside the scope is treated as an
/// escape (fail safe).
fn target_escapes_scope(candidate: &Path, scope_root: &Path) -> bool {
    match std::fs::canonicalize(candidate) {
        Ok(resolved) => !resolved.starts_with(scope_root),
        Err(_) => lexical_escapes(candidate, scope_root),
    }
}

fn lexical_escapes(candidate: &Path, scope_root: &Path) -> bool {
    !lexical_normalizes_inside(candidate, scope_root)
}

/// Lexically resolve `candidate` against `scope_root` without touching the
/// filesystem, for symlink targets that cannot be canonicalized (dangling or
/// looping). Returns whether the result stays inside the scope.
fn lexical_normalizes_inside(candidate: &Path, scope_root: &Path) -> bool {
    let mut resolved: PathBuf = PathBuf::from(scope_root);
    for component in candidate.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !resolved.pop() {
                    return false;
                }
            }
            std::path::Component::RootDir => {
                resolved.clear();
                resolved.push(std::path::Component::RootDir);
            }
            std::path::Component::Prefix(_) => return false,
            std::path::Component::Normal(part) => resolved.push(part),
        }
    }
    resolved.starts_with(scope_root)
}

fn filter_environment(provided: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut environment = BTreeMap::new();
    environment.insert("PATH".to_owned(), SANDBOX_PATH.to_owned());
    // The runner mounts a private `/tmp` tmpfs in every sandbox, so it is the
    // only scratch area that is guaranteed to exist; tool homes and caches land
    // there and vanish with the short-lived sandbox.
    environment.insert("HOME".to_owned(), "/tmp".to_owned());
    for (name, value) in provided {
        if is_portable_name(name) && !is_dangerous_name(name) {
            environment.insert(name.clone(), value.clone());
        }
    }
    environment
}

fn is_portable_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(byte) if byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn is_dangerous_name(name: &str) -> bool {
    name.starts_with("LD_")
        || name.ends_with("_PROXY")
        || name == "NO_PROXY"
        || name.starts_with("GIT_")
        || name.starts_with("CARGO_SOURCE_")
        || name.starts_with("CARGO_REGISTRIES_")
        || name.starts_with("CARGO_HTTP_")
        || name == "CARGO_REGISTRY_TOKEN"
}

/// Spawns the installed runner with the request and supervises execution.
pub fn execute(
    sandbox: &SandboxPaths,
    request: &SandboxRequest,
    cancellation: &CancellationToken,
) -> Result<ToolOutcome> {
    crate::executor::check_cancelled(cancellation)?;
    let json = serde_json::to_vec(request).map_err(|source| Error::SandboxBuild {
        reason: format!("cannot serialize the sandbox request: {source}"),
    })?;
    if json.len() > MAX_REQUEST_BYTES {
        return Err(Error::SandboxBuild {
            reason: format!("the sandbox request exceeds the {MAX_REQUEST_BYTES}-byte limit"),
        });
    }

    let output_limit =
        usize::try_from(request.resources.max_output_bytes).map_err(|_| Error::SandboxBuild {
            reason: "the output limit is not representable on this platform".to_owned(),
        })?;
    let mut command = Command::new(&sandbox.runner);
    command.arg(REQUEST_ARGUMENT);
    crate::process::run(
        &mut command,
        Some(&json),
        Duration::from_millis(request.resources.wall_time_ms),
        output_limit,
        cancellation,
        |source| Error::SandboxUnavailable {
            runner: Some(sandbox.runner.to_string_lossy().into_owned()),
            reason: format!("spawn sandbox runner: {source}"),
        },
    )
}

/// Resolves a bare program name to an absolute executable path by searching
/// `PATH`, then standard system directories.
pub fn resolve_executable(program: &str) -> Result<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        return Err(Error::SandboxUnavailable {
            runner: None,
            reason: format!("executable `{program}` does not exist"),
        });
    }
    if let Some(env_path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&env_path) {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    for directory in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        let candidate = Path::new(directory).join(program);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(Error::SandboxUnavailable {
        runner: None,
        reason: format!("could not resolve `{program}` on PATH"),
    })
}

// -- Request identity construction -------------------------------------------

fn grant(
    source: &Path,
    destination: &str,
    access: Access,
    purpose: Purpose,
    seed: &[u8],
    preflight: &Preflight<'_>,
) -> Result<GrantWire> {
    let metadata = std::fs::symlink_metadata(source)
        .map_err(|source| io_error("inspect grant source", Some(&source.to_string()), source))?;
    if metadata.file_type().is_symlink() {
        return Err(Error::SandboxBuild {
            reason: format!("grant source `{}` is a symbolic link", source.display()),
        });
    }

    let identity = if metadata.file_type().is_file() {
        read_file_identity("grant source", source, preflight)?
    } else {
        digest(&digest_input(b"dir", seed))
    };
    Ok(GrantWire {
        source: source.to_string_lossy().into_owned(),
        destination: destination.to_owned(),
        access,
        purpose,
        identity,
    })
}

fn grant_plan_identity(grants: &[GrantWire]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for grant in grants {
        bytes.extend_from_slice(grant.destination.as_bytes());
        bytes.push(b'\0');
        bytes.push(match grant.access {
            Access::ReadOnly => b'0',
            Access::ReadWrite => b'1',
        });
        bytes.push(match grant.purpose {
            Purpose::Context => b'0',
            Purpose::Authoring => b'1',
            Purpose::Scratch => b'2',
            Purpose::Toolchain => b'3',
            Purpose::Verification => b'4',
            Purpose::DependencyCache => b'5',
            Purpose::Lockfile => b'6',
            Purpose::Registry => b'7',
            Purpose::CargoConfig => b'8',
            Purpose::Runtime => b'9',
        });
        bytes.push(b'\0');
    }
    bytes
}

/// True when two paths are equal, or one contains the other, so a bind of the
/// same host directory under two different accesses would be contradictory.
fn roots_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.strip_prefix(right).is_ok() || right.strip_prefix(left).is_ok()
}

fn digest_input(domain: &[u8], data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(domain.len() + 1 + data.len());
    bytes.extend_from_slice(domain);
    bytes.push(b'\0');
    bytes.extend_from_slice(data);
    bytes
}

fn join_argv(argv: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (index, entry) in argv.iter().enumerate() {
        if index > 0 {
            bytes.push(b'\0');
        }
        bytes.extend_from_slice(entry.as_bytes());
    }
    bytes
}

fn digest(data: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(data)))
}

#[cfg(test)]
mod preflight_tests {
    use super::*;
    use crate::tools::ToolPolicy;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn fixture(root: &Path, size: u64) -> (SandboxPaths, PathBuf) {
        let runner = root.join("runner");
        std::fs::File::create(&runner)
            .unwrap()
            .set_len(size)
            .unwrap();
        let backend = root.join("backend");
        std::fs::write(&backend, "backend").unwrap();
        let work = root.join("work");
        std::fs::create_dir(&work).unwrap();
        (SandboxPaths { runner, backend }, work)
    }

    #[test]
    fn identity_read_rejects_more_than_256_mib_before_request_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let (sandbox, work) = fixture(root.path(), (256 << 20) + 1);
        let policy = ToolPolicy::minimum();
        let argv = vec!["/bin/bash".into(), "-c".into(), "true".into()];
        let request = BuildRequest {
            argv: &argv,
            working_directory: &work,
            read_roots: &[],
            environment: BTreeMap::new(),
            policy: &policy,
            resources: default_resources(),
        };
        let error = build_request(&sandbox, &request).unwrap_err();
        assert!(error.to_string().contains("256 MiB"), "{error}");
    }

    #[test]
    fn cancellation_during_identity_read_prevents_request_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let (sandbox, work) = fixture(root.path(), 128 << 20);
        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();
        let ready = Arc::new(Barrier::new(2));
        let worker_ready = ready.clone();
        let canceller = thread::spawn(move || {
            worker_ready.wait();
            thread::sleep(Duration::from_millis(1));
            trigger.cancel();
        });
        let policy = ToolPolicy::minimum();
        let argv = vec!["/bin/bash".into(), "-c".into(), "true".into()];
        let request = BuildRequest {
            argv: &argv,
            working_directory: &work,
            read_roots: &[],
            environment: BTreeMap::new(),
            policy: &policy,
            resources: default_resources(),
        };
        ready.wait();
        let result = build_request_cancellable(&sandbox, &request, &cancellation);
        canceller.join().unwrap();
        assert!(result.is_err(), "cancelled preflight returned a request");
    }

    #[test]
    fn workspace_scan_rejects_directory_depth_over_128() {
        let root = tempfile::tempdir().unwrap();
        let mut path = root.path().to_owned();
        for _ in 0..128 {
            path.push("d");
            std::fs::create_dir(&path).unwrap();
        }
        let token = CancellationToken::new();
        let preflight = Preflight {
            cancellation: &token,
            started: Instant::now(),
        };
        check_writable_scope(root.path(), &preflight).unwrap();
        path.push("d");
        std::fs::create_dir(&path).unwrap();
        let error = check_writable_scope(root.path(), &preflight).unwrap_err();
        assert!(error.to_string().contains("128"), "{error}");
    }

    #[test]
    fn scope_entry_quota_accepts_exactly_one_million() {
        let mut count = 0;
        for _ in 0..MAX_SCOPE_ENTRIES {
            admit_scope_entry(&mut count).unwrap();
        }
        assert_eq!(count, 1_000_000);
        assert!(admit_scope_entry(&mut count).is_err());
    }

    #[test]
    fn scope_path_quota_accepts_exact_bound_then_rejects_growth() {
        let mut stack = Vec::new();
        let mut visited = std::collections::BTreeSet::new();
        let mut bytes = MAX_SCOPE_PATH_BYTES - 2;
        queue_scope_directory(PathBuf::from("d"), 0, &mut stack, &mut visited, &mut bytes).unwrap();
        assert_eq!(bytes, 32 << 20);
        assert!(
            queue_scope_directory(PathBuf::from("e"), 0, &mut stack, &mut visited, &mut bytes)
                .is_err()
        );
    }

    #[test]
    fn expired_preflight_and_cancellation_stop_scope_inspection() {
        let root = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let expired = Preflight {
            cancellation: &token,
            started: Instant::now() - MAX_PREFLIGHT_TIME,
        };
        assert!(check_writable_scope(root.path(), &expired).is_err());
        let current = Preflight {
            cancellation: &token,
            started: Instant::now(),
        };
        token.cancel();
        assert!(check_writable_scope(root.path(), &current).is_err());
    }
}
