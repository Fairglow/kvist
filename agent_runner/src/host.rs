//! Host-execution tool executor: bypasses the Bubblewrap sandbox.
//!
//! [`HostExecutor`] renders tool intents the same way the sandbox executor does,
//! then runs the resulting commands directly on the host, with no namespace
//! isolation. It is the privilege escape hatch: it is only ever selected when a
//! caller explicitly requests `--allow-host-execution`, and an interactive host
//! session is single-turn by default. Everything the agent can still do is
//! bounded in time and output. Native file primitives confine their own writes
//! to the mapped working directory; arbitrary host shell commands do not.
//! The shell denylist is only an advisory filter, not a confinement boundary.
//! Each tool call is cancellable, time-boxed, and output-bounded.
//!
//! The agent works in the sandbox path namespace (`/workspace/...` for writes,
//! `/usr`, `/bin`, … for the read-only system layout). Those absolute paths do
//! not resolve on the host, so every argv element is translated: a path prefixed
//! with the write root is remapped onto the working directory, and paths outside
//! the write root (the real system directories) pass through unchanged. The shell
//! also runs with the working directory as its cwd, so relative paths resolve
//! against it exactly as they do inside the sandbox.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use agent_runtime::{CancellationToken, ToolIntent};

use crate::error::{Error, Result, io_error};
use crate::executor::{check_cancelled, resolve_file_helper, stage_file_request};
use crate::sandbox::ToolOutcome;
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
    /// may modify with native file primitives. Shell execution retains the
    /// caller's host authority and is not confined by this write root.
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
        check_cancelled(cancellation)?;
        let context =
            crate::tools::ExecContext::new(self.working_directory.clone(), intent.id.clone());
        let rendered = self.registry.render(intent, &context)?;

        if let Some(request) = &rendered.file_request {
            let helper =
                resolve_file_helper(rendered.file_helper.as_deref(), &self.working_directory)?;
            let mapped = request.host_mapped(&self.working_directory)?;
            check_cancelled(cancellation)?;
            let payload = stage_file_request(&mapped, &self.working_directory, cancellation)?;
            let argv = vec![
                helper.to_string_lossy().into_owned(),
                payload.path().to_string_lossy().into_owned(),
            ];
            check_cancelled(cancellation)?;
            return run_host_command(
                &argv,
                &self.working_directory,
                cancellation,
                crate::file_tools::MAX_OUTPUT_BYTES + 1,
            );
        }

        // The explicitly UNSANDBOXED shell inherits host authority. Translation
        // is a namespace convenience, not a security mechanism.
        let write_root = self.write_root();
        let argv: Vec<String> = rendered
            .argv
            .iter()
            .map(|element| translate_host_element(element, &write_root, &self.working_directory))
            .collect();
        run_host_command(
            &argv,
            &self.working_directory,
            cancellation,
            HOST_TOOL_OUTPUT_LIMIT,
        )
    }
}

impl HostExecutor {
    /// The sandbox write root, taken from the registry policy.
    fn write_root(&self) -> String {
        self.registry.policy().write_root.clone()
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
fn run_host_command(
    argv: &[String],
    workdir: &Path,
    cancellation: &CancellationToken,
    output_byte_limit: usize,
) -> Result<ToolOutcome> {
    check_cancelled(cancellation)?;
    if argv.is_empty() {
        return Err(Error::ToolRender {
            tool: "shell".to_owned(),
            reason: "empty command".to_owned(),
        });
    }

    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).current_dir(workdir);
    crate::process::run(
        &mut command,
        None,
        HOST_TOOL_WALL_TIME,
        output_byte_limit,
        cancellation,
        |source| io_error("spawn host command", Some(&argv[0]), source),
    )
}
