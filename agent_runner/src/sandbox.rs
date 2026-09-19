//! Sandbox request construction and execution.
//!
//! Every agent tool call becomes one closed version-one `SandboxRequest` in the
//! `Authoring` phase, reusing the shared `kvist_sandbox_runner` protocol types so
//! the wire shape is identical to the runner's own contract. Execution spawns the
//! installed runner, supervises output, and fails closed whenever the sandbox is
//! unavailable rather than executing on the host.

use std::collections::BTreeMap;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread;
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
/// The maximum size of a single stdout/stderr drain chunk.
const DRAIN_CHUNK_BYTES: usize = 8192;
/// How long the main loop waits on a drain channel before re-checking limits.
const DRAIN_POLL: Duration = Duration::from_millis(50);

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
    /// Host directories to expose read-only, in addition to the system layout.
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
        ToolOutcome {
            exited: false,
            status: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
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

    /// Whether the process exited with a non-zero status.
    pub fn failed(&self) -> bool {
        self.exited && self.status != Some(0)
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
    let runner_identity = read_file_identity("sandbox runner", &sandbox.runner)?;
    let backend_identity = backend_identity("sandbox backend", &sandbox.backend)?;

    let workdir = request.working_directory.canonicalize().map_err(|source| {
        io_error(
            "canonicalize working directory for the sandbox",
            Some(&request.working_directory.to_string_lossy()),
            source,
        )
    })?;

    // The writable authoring grant maps the working directory to the write root.
    let write_root = request.policy.write_root.clone();
    if write_root.is_empty() || !write_root.starts_with('/') {
        return Err(Error::SandboxBuild {
            reason: "the write root must be a non-empty absolute path".to_owned(),
        });
    }
    check_writable_scope(&workdir)?;
    let authoring = grant(
        &workdir,
        &write_root,
        Access::ReadWrite,
        Purpose::Authoring,
        &digest_input(b"authoring", workdir.to_string_lossy().as_bytes()),
    )?;

    // Read-only context grants for any declared read roots. A read root that
    // overlaps the write root would mount the same host path both read-write (as
    // the authoring scope) and read-only (as a context grant), so such an overlap
    // is rejected up front per the sandbox request contract. Destinations stay
    // disjoint, so the runner's overlap check also passes.
    let mut wires = vec![authoring];
    for (index, root) in request.read_roots.iter().enumerate() {
        match root.canonicalize() {
            Ok(canonical_root) => {
                if roots_overlap(&canonical_root, &workdir) {
                    return Err(Error::SandboxBuild {
                        reason: format!(
                            "read root `{}` overlaps the write root `{}`;
                             declare a read root outside the working directory scope",
                            root.display(),
                            workdir.display()
                        ),
                    });
                }
                match build_context_grant(&canonical_root, index) {
                    Some(grant) => wires.push(grant),
                    None => {
                        tracing::debug!(root = ?root, "skipping unreadable read-only grant")
                    }
                }
            }
            Err(source) => {
                tracing::debug!(root = ?root, error = %source, "skipping unreadable read-only grant")
            }
        }
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

    Ok(request)
}

fn read_file_identity(label: &str, path: &Path) -> Result<String> {
    let bytes = read_trusted_file(label, path)?;
    Ok(digest(&bytes))
}

fn backend_identity(label: &str, path: &Path) -> Result<BackendIdentity> {
    let bytes = read_trusted_file(label, path)?;
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
        digest: digest(&bytes),
    })
}

fn read_trusted_file(label: &str, path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path).map_err(|source| {
        io_error(
            &format!("inspect {label}"),
            Some(&path.to_string_lossy()),
            source,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(Error::SandboxUnavailable {
            runner: Some(path.to_string_lossy().into_owned()),
            reason: format!("{label} must be a regular non-link file"),
        });
    }
    std::fs::read(path).map_err(|source| {
        io_error(
            &format!("read {label}"),
            Some(&path.to_string_lossy()),
            source,
        )
    })
}

fn build_context_grant(root: &Path, index: usize) -> Option<GrantWire> {
    let source = root.canonicalize().ok()?;
    let metadata = std::fs::symlink_metadata(&source).ok()?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return None;
    }
    let destination = format!("/ro/{index}");
    let bytes = std::fs::read(&source).ok()?;
    Some(GrantWire {
        source: source.to_string_lossy().into_owned(),
        destination,
        access: Access::ReadOnly,
        purpose: Purpose::Context,
        identity: digest(&bytes),
    })
}

fn check_writable_scope(workdir: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(workdir).map_err(|source| {
        io_error(
            "inspect working directory scope for the sandbox",
            Some(&workdir.to_string_lossy()),
            source,
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(Error::SandboxBuild {
            reason: format!(
                "working directory `{}` is a symbolic link",
                workdir.display()
            ),
        });
    }
    if let Ok(entries) = std::fs::read_dir(workdir) {
        for entry in entries.flatten() {
            if entry
                .file_type()
                .map(|type_| type_.is_symlink())
                .unwrap_or(false)
            {
                return Err(Error::SandboxBuild {
                    reason: format!(
                        "working directory `{}` contains a symbolic link in its writable scope; \
                         run in a symlink-free directory or narrow the working directory",
                        workdir.display()
                    ),
                });
            }
        }
    }
    Ok(())
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
    let json = serde_json::to_vec(request).map_err(|source| Error::SandboxBuild {
        reason: format!("cannot serialize the sandbox request: {source}"),
    })?;
    if json.len() > MAX_REQUEST_BYTES {
        return Err(Error::SandboxBuild {
            reason: format!("the sandbox request exceeds the {MAX_REQUEST_BYTES}-byte limit"),
        });
    }

    let mut child = Command::new(&sandbox.runner)
        .arg(REQUEST_ARGUMENT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| Error::SandboxUnavailable {
            runner: Some(sandbox.runner.to_string_lossy().into_owned()),
            reason: format!("spawn sandbox runner: {source}"),
        })?;

    let started = Instant::now();
    let wall_time = Duration::from_millis(request.resources.wall_time_ms);
    let output_limit = request.resources.max_output_bytes as usize;
    let limit_exceeded = Arc::new(AtomicBool::new(false));

    let stdout_done = Arc::new(AtomicBool::new(false));
    let stderr_done = Arc::new(AtomicBool::new(false));
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = mpsc::channel::<Vec<u8>>();

    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| sandbox_unavailable(sandbox, "standard output"))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| sandbox_unavailable(sandbox, "standard error"))?;
    let limit_for_stdout = limit_exceeded.clone();
    let stdout_done_for_stdout = stdout_done.clone();
    thread::spawn(move || {
        drain_pipe(
            stdout_pipe,
            limit_for_stdout,
            stdout_done_for_stdout,
            out_tx,
        )
    });
    let limit_for_stderr = limit_exceeded.clone();
    let stderr_done_for_stderr = stderr_done.clone();
    thread::spawn(move || {
        drain_pipe(
            stderr_pipe,
            limit_for_stderr,
            stderr_done_for_stderr,
            err_tx,
        )
    });

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    // Write the bounded request once, then close stdin.
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| sandbox_unavailable(sandbox, "standard input"))?;
        if let Err(source) = write_all(&mut stdin, &json) {
            let _ = kill_group(&mut child);
            return Err(io_error("write sandbox request", None, source));
        }
    }

    let mut timed_out = false;
    let mut cancelled = false;
    let mut status = None;

    loop {
        if cancellation.is_cancelled() {
            cancelled = true;
            let _ = kill_group(&mut child);
            break;
        }
        if started.elapsed() >= wall_time {
            timed_out = true;
            let _ = kill_group(&mut child);
            break;
        }
        if !stdout_done.load(Ordering::SeqCst)
            && let Ok(chunk) = out_rx.recv_timeout(DRAIN_POLL)
        {
            if stdout.len().saturating_add(chunk.len()) > output_limit {
                limit_exceeded.store(true, Ordering::SeqCst);
            } else if !limit_exceeded.load(Ordering::SeqCst) {
                stdout.extend_from_slice(&chunk);
            }
        }
        if !stderr_done.load(Ordering::SeqCst)
            && let Ok(chunk) = err_rx.recv_timeout(DRAIN_POLL)
        {
            if stderr.len().saturating_add(chunk.len()) > output_limit {
                limit_exceeded.store(true, Ordering::SeqCst);
            } else if !limit_exceeded.load(Ordering::SeqCst) {
                stderr.extend_from_slice(&chunk);
            }
        }
        if let Ok(exited) = child.try_wait() {
            if let Some(exit) = exited {
                status = Some(exit);
            }
            if exited.is_some()
                && stdout_done.load(Ordering::SeqCst)
                && stderr_done.load(Ordering::SeqCst)
            {
                break;
            }
        }
    }

    // Flush whatever the drain threads already buffered before we stopped.
    flush_blocking(&out_rx, &mut stdout);
    flush_blocking(&err_rx, &mut stderr);

    let _ = child.wait();

    Ok(ToolOutcome {
        exited: status.is_some(),
        status: status.and_then(|s| s.code()),
        stdout,
        stderr,
        timed_out,
        output_limit_exceeded: limit_exceeded.load(Ordering::SeqCst),
        cancelled,
    })
}

fn drain_pipe<R>(pipe: R, limit: Arc<AtomicBool>, done: Arc<AtomicBool>, tx: mpsc::Sender<Vec<u8>>)
where
    R: Read,
{
    let mut reader = BufReader::new(pipe);
    let mut chunk = vec![0u8; DRAIN_CHUNK_BYTES];
    loop {
        if limit.load(Ordering::SeqCst) {
            break;
        }
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if tx.send(chunk[..n].to_vec()).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    done.store(true, Ordering::SeqCst);
}

fn flush_blocking(rx: &mpsc::Receiver<Vec<u8>>, buffer: &mut Vec<u8>) {
    while let Ok(chunk) = rx.recv() {
        buffer.extend_from_slice(&chunk);
    }
}

fn write_all<W: Write>(writer: &mut W, mut bytes: &[u8]) -> std::io::Result<()> {
    while !bytes.is_empty() {
        match writer.write(bytes) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "short write",
                ));
            }
            Ok(n) => bytes = &bytes[n..],
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn sandbox_unavailable(sandbox: &SandboxPaths, stream: &str) -> Error {
    Error::SandboxUnavailable {
        runner: Some(sandbox.runner.to_string_lossy().into_owned()),
        reason: format!("sandbox runner did not provide {stream}"),
    }
}

fn kill_group(child: &mut std::process::Child) -> Result<()> {
    use nix::sys::signal;
    use nix::unistd::Pid;
    let pid = child.id();
    let negative = i32::try_from(pid).map(|p| -p).unwrap_or(0);
    let result = signal::kill(Pid::from_raw(negative), signal::Signal::SIGTERM);
    match result {
        Ok(()) => Ok(()),
        Err(_) => {
            let _ = child.kill();
            Ok(())
        }
    }
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
) -> Result<GrantWire> {
    let metadata = std::fs::symlink_metadata(source)
        .map_err(|source| io_error("inspect grant source", Some(&source.to_string()), source))?;
    if metadata.file_type().is_symlink() {
        return Err(Error::SandboxBuild {
            reason: format!("grant source `{}` is a symbolic link", source.display()),
        });
    }
    let identity = if metadata.file_type().is_file() {
        let bytes = std::fs::read(source).map_err(|source| {
            io_error(
                "read grant source for identity",
                Some(&source.to_string()),
                source,
            )
        })?;
        digest(&bytes)
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
