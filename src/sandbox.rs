//! Versioned, shell-free protocol for a project-selected sandbox runner.

use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use serde::Serialize;

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
) -> Result<()> {
    validate_runner(config, project_root, vcs_selection)?;
    let output = runner_command(config)
        .arg(PROBE_ARGUMENT)
        .output()
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
) -> Result<std::process::Output> {
    execute_with_timeout(config, request, ExecutionOptions::default()).map(|result| result.0)
}

/// Executes a request with an optional runner deadline. A timeout terminates
/// the runner rather than retrying or invoking the requested program on host.
pub fn execute_with_timeout(
    config: &SandboxConfig,
    request: ExecutionRequest<'_>,
    options: ExecutionOptions,
) -> Result<(std::process::Output, bool)> {
    validate_runner(config, request.project_root, request.vcs_selection)?;
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

    let mut child = runner_command(config)
        .arg(EXECUTE_ARGUMENT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| sandbox_error(config, "start sandbox runner", source))?;
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
    let stdout_reader = std::thread::spawn(move || capture_stream(stdout, options.output_limit));
    let stderr_reader = std::thread::spawn(move || capture_stream(stderr, options.output_limit));

    let started = Instant::now();
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status, false),
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
                break (status, true);
            }

            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(source) => return Err(sandbox_error(config, "wait for sandbox runner", source)),
        }
    };
    let stdout = join_capture(stdout_reader, config, "read sandbox runner stdout")?;
    let stderr = join_capture(stderr_reader, config, "read sandbox runner stderr")?;
    Ok((
        std::process::Output {
            status,
            stdout,
            stderr,
        },
        timed_out,
    ))
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

fn capture_stream<R: Read>(mut stream: R, limit: Option<usize>) -> io::Result<Vec<u8>> {
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 8_192];
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Ok(captured);
        }
        let remaining = limit.map_or(usize::MAX, |limit| limit.saturating_sub(captured.len()));
        captured.extend_from_slice(&buffer[..count.min(remaining)]);
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

fn runner_command(config: &SandboxConfig) -> Command {
    let mut command = Command::new(&config.runner);
    command.env_clear();
    for (name, value) in allowed_environment(config, None) {
        command.env(name, value);
    }
    command
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
}
