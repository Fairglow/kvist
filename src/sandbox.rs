//! Versioned, shell-free protocol for a project-selected sandbox runner.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
use std::{
    fs::{File, OpenOptions},
    path::PathBuf,
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
    let bytes = fs::read(&canonical_path)
        .map_err(|source| sandbox_error(config, "read trusted sandbox runner", source))?;
    Ok(RunnerIdentity {
        canonical_path: canonical_path.to_string_lossy().into_owned(),
        digest: format!("sha256:{:x}", Sha256::digest(bytes)),
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
            Ok(Some(status)) => break (status, false, capture.exceeded.load(Ordering::Acquire)),
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
                child.kill().map_err(|source| {
                    sandbox_error(config, "terminate timed-out sandbox runner", source)
                })?;
                let status = child.wait().map_err(|source| {
                    sandbox_error(config, "wait for timed-out sandbox runner", source)
                })?;
                break (status, true, false);
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
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(KvistError::SandboxUnavailable {
            runner: config.runner.clone(),
            reason: "the runner must be a regular non-symlink file".to_owned(),
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

#[cfg(test)]
fn capture_stream<R: Read>(stream: R, limit: Option<usize>) -> io::Result<Vec<u8>> {
    capture_stream_bounded(stream, Arc::new(CaptureState::new(limit)))
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
        .filter(|name| additional.is_none_or(|allowed| allowed.contains(*name)))
        .filter_map(|name| std::env::var(name).ok().map(|value| (name.clone(), value)))
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
    VerifiedRunnerLaunch::create(project_root, expected_runner)
}

/// A Linux-only descriptor-bound copy of verified runner bytes.
///
/// The private copy prevents path replacement or in-place source modification
/// after hashing from changing the bytes passed to `exec`.
#[cfg(target_os = "linux")]
struct VerifiedRunnerLaunch {
    _file: File,
    copy_path: PathBuf,
    launch_path: PathBuf,
}

#[cfg(target_os = "linux")]
impl VerifiedRunnerLaunch {
    fn create(project_root: &Path, expected_runner: &RunnerIdentity) -> Result<Self> {
        use std::os::{
            fd::AsRawFd,
            unix::fs::{OpenOptionsExt, PermissionsExt},
        };

        let bytes = fs::read(&expected_runner.canonical_path).map_err(|source| {
            KvistError::SandboxUnavailable {
                runner: expected_runner.canonical_path.clone(),
                reason: format!(
                    "read verified sandbox runner for descriptor-bound launch: {source}"
                ),
            }
        })?;
        let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        if digest != expected_runner.digest {
            return Err(KvistError::SandboxUnavailable {
                runner: expected_runner.canonical_path.clone(),
                reason: "trusted sandbox runner content changed before descriptor-bound launch"
                    .to_owned(),
            });
        }

        let directory = secure_copy_directory(project_root)?;
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce).map_err(|error| KvistError::SandboxUnavailable {
            runner: expected_runner.canonical_path.clone(),
            reason: format!("generate descriptor-bound runner copy name: {error}"),
        })?;
        let copy_path = directory.join(format!(
            "runner-{}",
            nonce
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ));
        let mut options = OpenOptions::new();
        options.write(true).read(true).create_new(true).mode(0o700);
        let mut writable_file =
            options
                .open(&copy_path)
                .map_err(|source| KvistError::SandboxUnavailable {
                    runner: expected_runner.canonical_path.clone(),
                    reason: format!("create descriptor-bound runner copy: {source}"),
                })?;
        if let Err(source) = writable_file
            .write_all(&bytes)
            .and_then(|()| writable_file.sync_all())
        {
            let _ = fs::remove_file(&copy_path);
            return Err(KvistError::SandboxUnavailable {
                runner: expected_runner.canonical_path.clone(),
                reason: format!("write descriptor-bound runner copy: {source}"),
            });
        }
        fs::set_permissions(&copy_path, fs::Permissions::from_mode(0o500)).map_err(|source| {
            KvistError::SandboxUnavailable {
                runner: expected_runner.canonical_path.clone(),
                reason: format!("protect descriptor-bound runner copy: {source}"),
            }
        })?;
        drop(writable_file);
        let file = File::open(&copy_path).map_err(|source| KvistError::SandboxUnavailable {
            runner: expected_runner.canonical_path.clone(),
            reason: format!("reopen descriptor-bound runner copy: {source}"),
        })?;
        nix::fcntl::fcntl(
            file.as_raw_fd(),
            nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
        )
        .map_err(|source| KvistError::SandboxUnavailable {
            runner: expected_runner.canonical_path.clone(),
            reason: format!("retain descriptor-bound runner across exec: {source}"),
        })?;
        let launch_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
        Ok(Self {
            _file: file,
            copy_path,
            launch_path,
        })
    }

    fn command(&self, config: &SandboxConfig) -> Command {
        let mut command = Command::new(&self.launch_path);
        command.env_clear();
        for (name, value) in allowed_environment(config, None) {
            command.env(name, value);
        }
        command
    }

    fn cleanup(&self) {
        let _ = fs::remove_file(&self.copy_path);
    }
}

#[cfg(not(target_os = "linux"))]
struct VerifiedRunnerLaunch;

#[cfg(not(target_os = "linux"))]
impl VerifiedRunnerLaunch {
    fn create(_project_root: &Path, expected_runner: &RunnerIdentity) -> Result<Self> {
        Err(KvistError::SandboxUnavailable {
            runner: expected_runner.canonical_path.clone(),
            reason: "descriptor-bound sandbox runner execution is unavailable on this platform"
                .to_owned(),
        })
    }

    fn command(&self, _config: &SandboxConfig) -> Command {
        unreachable!("unsupported runner launch cannot produce a command")
    }

    fn cleanup(&self) {}
}

#[cfg(target_os = "linux")]
fn secure_copy_directory(project_root: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;

    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .ok_or_else(|| KvistError::SandboxUnavailable {
            runner: "<unconfigured>".to_owned(),
            reason: "cannot determine user-owned descriptor-bound runner state directory"
                .to_owned(),
        })?;
    let directory = base.join("kvist").join("runner-copies-v1");
    if directory.starts_with(project_root.canonicalize().map_err(|source| {
        KvistError::SandboxUnavailable {
            runner: "<unconfigured>".to_owned(),
            reason: format!("canonicalize project for descriptor-bound runner state: {source}"),
        }
    })?) {
        return Err(KvistError::SandboxUnavailable {
            runner: "<unconfigured>".to_owned(),
            reason: "descriptor-bound runner state must not be inside the project".to_owned(),
        });
    }
    fs::create_dir_all(&directory).map_err(|source| KvistError::SandboxUnavailable {
        runner: "<unconfigured>".to_owned(),
        reason: format!("create descriptor-bound runner state directory: {source}"),
    })?;
    let metadata =
        fs::symlink_metadata(&directory).map_err(|source| KvistError::SandboxUnavailable {
            runner: "<unconfigured>".to_owned(),
            reason: format!("inspect descriptor-bound runner state directory: {source}"),
        })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(KvistError::SandboxUnavailable {
            runner: "<unconfigured>".to_owned(),
            reason: "descriptor-bound runner state must be a real directory".to_owned(),
        });
    }
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).map_err(|source| {
        KvistError::SandboxUnavailable {
            runner: "<unconfigured>".to_owned(),
            reason: format!("protect descriptor-bound runner state directory: {source}"),
        }
    })?;
    Ok(directory)
}

fn sandbox_error(config: &SandboxConfig, operation: &'static str, source: io::Error) -> KvistError {
    KvistError::SandboxUnavailable {
        runner: config.runner.clone(),
        reason: format!("{operation}: {source}"),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::capture_stream;

    #[test]
    fn bounded_capture_discards_excess_stream_bytes() {
        let output =
            capture_stream(Cursor::new(vec![b'x'; 1_000_000]), Some(16)).expect("capture stream");

        assert_eq!(output, vec![b'x'; 16]);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn descriptor_bound_launch_uses_verified_bytes_after_source_replacement() {
        use std::{fs, os::unix::fs::PermissionsExt, process::Command};

        use tempfile::TempDir;

        use crate::config::{SandboxConfig, VcsSelection};

        let project = TempDir::new().expect("project");
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(project.path())
            .status()
            .expect("initialize Git");
        assert!(status.success());
        let runner_dir = TempDir::new().expect("runner directory");
        let runner_path = runner_dir.path().join("runner");
        fs::write(&runner_path, "#!/bin/sh\nprintf approved\n").expect("write runner");
        fs::set_permissions(&runner_path, fs::Permissions::from_mode(0o755))
            .expect("make runner executable");
        let config = SandboxConfig {
            runner: runner_path.to_string_lossy().into_owned(),
            environment_allowlist: Vec::new(),
        };
        let identity =
            super::runner_identity(&config, project.path(), VcsSelection::Git).expect("identity");
        let launch =
            super::checked_runner_launch(&config, project.path(), VcsSelection::Git, &identity)
                .expect("verified launch");

        fs::write(&runner_path, "#!/bin/sh\nprintf replaced\n").expect("replace source runner");
        let output = launch.command(&config).output().expect("launch copy");
        launch.cleanup();

        assert!(output.status.success());
        assert_eq!(output.stdout, b"approved");
    }
}
