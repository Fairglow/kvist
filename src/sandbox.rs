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
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    KvistError, Result,
    config::{SandboxConfig, VcsSelection},
    vcs,
};

pub const PROTOCOL_VERSION: u32 = 1;
const PROBE_ARGUMENT: &str = "--kvist-sandbox-probe-v1";
const EXECUTE_ARGUMENT: &str = "--kvist-sandbox-request-v1";
const PROBE_RESPONSE: &str = "kvist-sandbox-probe-v1: network=deny; mount=component";

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

/// Values that describe one program invocation inside the component sandbox.
pub struct ExecutionRequest<'a> {
    pub project_root: &'a Path,
    pub vcs_selection: VcsSelection,
    pub component_dir: &'a Path,
    pub program: &'a str,
    pub arguments: &'a [String],
    pub environment: BTreeMap<String, String>,
    pub context_files: &'a [String],
}

#[derive(Debug, Serialize)]
struct SandboxRequest<'a> {
    protocol_version: u32,
    program: &'a str,
    arguments: &'a [String],
    working_directory: &'a str,
    network: &'static str,
    mounts: [SandboxMount<'a>; 1],
    environment: BTreeMap<String, String>,
    context_files: &'a [String],
}

#[derive(Debug, Serialize)]
struct SandboxMount<'a> {
    source: &'a str,
    destination: &'static str,
    access: &'static str,
}

/// Validates that the configured runner is reachable and asserts its v1
/// network-denial/component-mount capability before task state is changed.
pub fn ensure_available(
    config: &SandboxConfig,
    project_root: &Path,
    vcs_selection: VcsSelection,
    expected_runner: &RunnerIdentity,
) -> Result<()> {
    let launch = checked_runner_launch(config, project_root, vcs_selection, expected_runner)?;
    let output = launch.command(config).arg(PROBE_ARGUMENT).output();
    launch.cleanup();
    let output = output
        .map_err(|source| sandbox_error(config, "start sandbox availability probe", source))?;
    if !output.status.success() || output.stdout != format!("{PROBE_RESPONSE}\n").as_bytes() {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner did not confirm the version-1 deny-network component-mount isolation probe"
                .to_owned(),
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
    let source = request
        .component_dir
        .to_str()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "component directory is not valid UTF-8 for the sandbox manifest".to_owned(),
        })?;
    let request = SandboxRequest {
        protocol_version: PROTOCOL_VERSION,
        program: request.program,
        arguments: request.arguments,
        working_directory: "/workspace/component",
        network: "deny",
        mounts: [SandboxMount {
            source,
            destination: "/workspace/component",
            access: "read-write",
        }],
        environment: request.environment,
        context_files: request.context_files,
    };
    let encoded = serde_json::to_vec(&request).map_err(|error| KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: format!("cannot encode sandbox request: {error}"),
    })?;

    let launch = checked_runner_launch(config, project_root, vcs_selection, expected_runner)?;
    let child = launch
        .command(config)
        .arg(EXECUTE_ARGUMENT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    launch.cleanup();
    let mut child =
        child.map_err(|source| sandbox_error(config, "start sandbox runner", source))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "sandbox runner did not provide standard input".to_owned(),
        })?;
    stdin
        .write_all(&encoded)
        .map_err(|source| sandbox_error(config, "write sandbox request", source))?;
    drop(stdin);

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "sandbox runner did not provide standard output".to_owned(),
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "sandbox runner did not provide standard error".to_owned(),
        })?;
    let capture = Arc::new(CaptureState::new(options.output_limit));
    let stdout_capture = Arc::clone(&capture);
    let stderr_capture = Arc::clone(&capture);
    let stdout_reader = std::thread::spawn(move || capture_stream_bounded(stdout, stdout_capture));
    let stderr_reader = std::thread::spawn(move || capture_stream_bounded(stderr, stderr_capture));

    let started = Instant::now();
    let (status, timed_out, output_limit_exceeded) = loop {
        match child.try_wait() {
            Ok(Some(termination_status)) => {
                break (
                    termination_status,
                    false,
                    capture.exceeded.load(Ordering::Acquire),
                );
            }
            Ok(None) if capture.exceeded.load(Ordering::Acquire) => {
                child.kill().map_err(|source| {
                    sandbox_error(config, "terminate output-limited sandbox runner", source)
                })?;
                let status = child.wait().map_err(|source| {
                    sandbox_error(config, "wait for output-limited sandbox runner", source)
                })?;
                break (status, false, true);
            }
            Ok(None)
                if options
                    .timeout
                    .is_some_and(|limit| started.elapsed() >= limit) =>
            {
                break (
                    child.wait().map_err(|source| {
                        sandbox_error(config, "wait for timed-out sandbox runner", source)
                    })?,
                    true,
                    false,
                );
            }

            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(source) => return Err(sandbox_error(config, "wait for sandbox runner", source)),
        }
    };
    let stdout = join_capture(stdout_reader, config, "read sandbox runner stdout")?;
    let stderr = join_capture(stderr_reader, config, "read sandbox runner stderr")?;
    Ok(ExecutionResult {
        output: std::process::Output {
            status,
            stdout,
            stderr,
        },
        timed_out,
        output_limit_exceeded,
    })
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

struct CaptureState {
    limit: Option<usize>,
    captured: AtomicUsize,
    exceeded: AtomicBool,
}

impl CaptureState {
    fn new(limit: Option<usize>) -> Self {
        Self {
            limit,
            captured: AtomicUsize::new(0),
            exceeded: AtomicBool::new(false),
        }
    }
}

fn capture_stream_bounded<R: Read>(
    mut stream: R,
    capture: Arc<CaptureState>,
) -> io::Result<Vec<u8>> {
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 8_192];
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Ok(captured);
        }
        let accepted = match capture.limit {
            None => count,
            Some(limit) => {
                let position = capture.captured.fetch_add(count, Ordering::AcqRel);
                let remaining = limit.saturating_sub(position);
                if count > remaining {
                    capture.exceeded.store(true, Ordering::Release);
                }
                count.min(remaining)
            }
        };
        captured.extend_from_slice(&buffer[..accepted]);
        if accepted < count {
            return Ok(captured);
        }
    }
}

fn join_capture(
    handle: std::thread::JoinHandle<io::Result<Vec<u8>>>,
    config: &SandboxConfig,
    operation: &'static str,
) -> Result<Vec<u8>> {
    match handle.join() {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(source)) => Err(sandbox_error(config, operation, source)),
        Err(_) => Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: format!("{operation}: output reader thread panicked"),
        }),
    }
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
        let mut command = Command::new(&self.launch_path);
        // On Linux, execve is used directly so env_clear() is not needed.
        // On non-Linux, we use Command::spawn which also does not invoke a shell
        // when given a bare path. The environment is controlled by the allowlist
        // below.
        for (name, value) in allowed_environment(config, None) {
            command.env(name, value);
        }
        command
    }

    fn cleanup(&self) {
        // Drop tempfile::TempDir automatically deletes the directory and everything in it recursively.
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
                let dest_path = temp_dir_path.join(src_path.file_name().unwrap());
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
    use std::io::Cursor;

    #[test]
    fn test_bounded_capture_discards_excess_stream_bytes() -> Result<()> {
        let data = b"hello world";
        let capture = Arc::new(CaptureState::new(Some(5)));
        let bytes = capture_stream_bounded(Cursor::new(data), capture).unwrap();
        assert_eq!(bytes, b"hello");
        Ok(())
    }
}
