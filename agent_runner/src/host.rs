//! Host-execution tool executor: bypasses the Bubblewrap sandbox.
//!
//! [`HostExecutor`] renders tool intents the same way the sandbox executor does,
//! then runs the resulting commands directly on the host, with no namespace
//! isolation. It is the privilege escape hatch: it is only ever selected when a
//! caller explicitly requests `--allow-host-execution`, and an interactive host
//! session is single-turn by default. Everything the agent can still do is
//! bounded: writes stay inside the working directory (the sandbox write root is
//! mapped back onto it), the shell keeps its denylist, and each tool call is
//! cancellable, time-boxed, and output-bounded.
//!
//! The agent works in the sandbox path namespace (`/workspace/...` for writes,
//! `/usr`, `/bin`, … for the read-only system layout). Those absolute paths do
//! not resolve on the host, so every argv element is translated: a path prefixed
//! with the write root is remapped onto the working directory, and paths outside
//! the write root (the real system directories) pass through unchanged. The shell
//! also runs with the working directory as its cwd, so relative paths resolve
//! against it exactly as they do inside the sandbox.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use agent_runtime::{CancellationToken, ToolIntent};

use crate::error::{Error, Result, io_error};
use crate::sandbox::{ToolOutcome, drain_pipe, flush_blocking, kill_group};
use crate::session::{MAX_TOOL_RESULT_BYTES, ToolExecutor};
use crate::tools::ToolRegistry;

/// The wall-clock cap for a single host tool call. Host commands run with real
/// privileges, so a call that exceeds this is terminated like the sandbox would.
const HOST_TOOL_WALL_TIME: Duration = Duration::from_secs(120);

/// The maximum bytes of a single host tool result folded back to the model.
const HOST_TOOL_OUTPUT_LIMIT: usize = MAX_TOOL_RESULT_BYTES;

/// Executes tool intents directly on the host (no sandbox).
pub struct HostExecutor {
    registry: ToolRegistry,
    working_directory: PathBuf,
}

impl HostExecutor {
    /// Builds a host executor from a registry and the working directory the agent
    /// may write to. The write root is taken from the registry policy so host and
    /// sandbox executions confine writes identically.
    pub fn new(registry: ToolRegistry, working_directory: PathBuf) -> Self {
        HostExecutor {
            registry,
            working_directory,
        }
    }

    /// The working directory this executor writes to.
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    /// The enabled tool profiles.
    pub fn profiles(&self) -> Vec<&'static str> {
        self.registry.profiles()
    }
}

impl ToolExecutor for HostExecutor {
    fn execute(
        &self,
        intent: &ToolIntent,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutcome> {
        let context =
            crate::tools::ExecContext::new(self.working_directory.clone(), intent.id.clone());
        let rendered = self.registry.render(intent, &context)?;

        // `write_file` is staged: the content is written straight to the mapped
        // host path (inside the working directory) rather than moved into place
        // by the sandbox. Confinement is preserved because the registry already
        // rejected any target outside the write root before rendering.
        if let Some(staged) = rendered.staged_write {
            return self.write_hosted(intent, &staged);
        }

        // `shell`, `read_file`, and `list_dir` run the rendered argv directly on
        // the host. Every element is path-translated so `/workspace/...` resolves
        // against the working directory and real system paths pass through.
        let write_root = self.write_root();
        let argv: Vec<String> = rendered
            .argv
            .iter()
            .map(|element| translate_host_element(element, &write_root, &self.working_directory))
            .collect();
        run_host_command(&argv, cancellation)
    }
}

impl HostExecutor {
    /// The sandbox write root, taken from the registry policy.
    fn write_root(&self) -> String {
        self.registry.policy().write_root.clone()
    }

    /// Writes one `write_file` intent straight to the host, inside the working
    /// directory, and reports a successful outcome.
    fn write_hosted(
        &self,
        intent: &ToolIntent,
        staged: &crate::tools::StagedWrite,
    ) -> Result<ToolOutcome> {
        let host_target = self.map_to_host(&staged.target);
        let content = intent
            .arguments
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if let Some(parent) = host_target.parent() {
            std::fs::create_dir_all(parent).map_err(|source| {
                io_error(
                    "create staging directory for the write",
                    Some(&parent.to_string_lossy()),
                    source,
                )
            })?;
        }
        let display = host_target.to_string_lossy();
        std::fs::write(&host_target, content.as_bytes())
            .map_err(|source| io_error("write content on the host", Some(&display), source))?;
        tracing::debug!(path = ?host_target, "wrote content on the host");
        Ok(ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: format!("wrote {}\n", host_target.display()).into_bytes(),
            stderr: Vec::new(),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        })
    }

    /// Maps a sandbox path onto the host. Write-root-prefixed paths resolve under
    /// the working directory; everything else is already a host-native path.
    fn map_to_host(&self, sandbox_path: &str) -> PathBuf {
        let root = self.write_root();
        let root = root.trim_end_matches('/');
        if sandbox_path == root {
            return self.working_directory.clone();
        }
        match sandbox_path.strip_prefix(&format!("{root}/")) {
            Some(relative) => self.working_directory.join(relative),
            None => PathBuf::from(sandbox_path),
        }
    }
}

/// Rewrites one argv element so a write-root-prefixed absolute path resolves
/// under the working directory on the host. Non-path elements (the program,
/// `-c`, the argv0 placeholder) are unchanged because they are not under the
/// write root.
fn translate_host_element(element: &str, write_root: &str, working_directory: &Path) -> String {
    let root = write_root.trim_end_matches('/');
    if element == root {
        return working_directory.to_string_lossy().into_owned();
    }
    element.replace(
        &format!("{root}/"),
        &format!("{}/", working_directory.display()),
    )
}

/// Runs one rendered host command, streaming its output and honoring
/// cancellation, the wall-clock deadline, and the output limit.
fn run_host_command(argv: &[String], cancellation: &CancellationToken) -> Result<ToolOutcome> {
    if argv.is_empty() {
        return Err(Error::ToolRender {
            tool: "shell".to_owned(),
            reason: "empty command".to_owned(),
        });
    }

    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| io_error("spawn host command", Some(&argv[0]), source))?;

    let stdout_pipe = child.stdout.take().ok_or_else(|| Error::ToolRender {
        tool: argv[0].clone(),
        reason: "host command did not provide standard output".to_owned(),
    })?;
    let stderr_pipe = child.stderr.take().ok_or_else(|| Error::ToolRender {
        tool: argv[0].clone(),
        reason: "host command did not provide standard error".to_owned(),
    })?;

    let output_limit = Arc::new(AtomicBool::new(false));
    let stdout_done = Arc::new(AtomicBool::new(false));
    let stderr_done = Arc::new(AtomicBool::new(false));
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = mpsc::channel::<Vec<u8>>();

    // Clone shared flags for the drain threads (each thread gets its own) and for
    // the loop below, which observes completion and the output limit.
    let stdout_drain = (output_limit.clone(), stdout_done.clone());
    let stderr_drain = (output_limit.clone(), stderr_done.clone());
    let output_limit_main = output_limit.clone();
    let stdout_done_main = stdout_done.clone();
    let stderr_done_main = stderr_done.clone();

    thread::spawn(move || drain_pipe(stdout_pipe, stdout_drain.0, stdout_drain.1, out_tx));
    thread::spawn(move || drain_pipe(stderr_pipe, stderr_drain.0, stderr_drain.1, err_tx));

    let started = Instant::now();
    let wall_time = HOST_TOOL_WALL_TIME;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
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
        if !stdout_done_main.load(Ordering::SeqCst)
            && let Ok(chunk) = out_rx.recv_timeout(Duration::from_millis(20))
        {
            if stdout.len().saturating_add(chunk.len()) > HOST_TOOL_OUTPUT_LIMIT {
                output_limit_main.store(true, Ordering::SeqCst);
            } else if !output_limit_main.load(Ordering::SeqCst) {
                stdout.extend_from_slice(&chunk);
            }
        }
        if !stderr_done_main.load(Ordering::SeqCst)
            && let Ok(chunk) = err_rx.recv_timeout(Duration::from_millis(20))
        {
            if stderr.len().saturating_add(chunk.len()) > HOST_TOOL_OUTPUT_LIMIT {
                output_limit_main.store(true, Ordering::SeqCst);
            } else if !output_limit_main.load(Ordering::SeqCst) {
                stderr.extend_from_slice(&chunk);
            }
        }
        if let Ok(exited) = child.try_wait() {
            if let Some(exit) = exited {
                status = Some(exit);
            }
            if exited.is_some()
                && stdout_done_main.load(Ordering::SeqCst)
                && stderr_done_main.load(Ordering::SeqCst)
            {
                break;
            }
        }
    }

    flush_blocking(&out_rx, &mut stdout);
    flush_blocking(&err_rx, &mut stderr);
    let _ = child.wait();

    Ok(ToolOutcome {
        exited: status.is_some(),
        status: status.and_then(|s| s.code()),
        stdout,
        stderr,
        timed_out,
        output_limit_exceeded: output_limit.load(Ordering::SeqCst),
        cancelled,
    })
}
