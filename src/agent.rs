//! External agent execution and response capture.

use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{
    KvistError, Result,
    config::{AgentProfile, SandboxConfig, VcsSelection},
    sandbox::{self, RunnerIdentity},
    task_queue::Timestamp,
};

/// Structured execution result of an external agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunResult {
    /// True if the agent succeeded and exited with status 0.
    pub success: bool,
    /// Number of prompt/input tokens used, if reported.
    pub tokens_input: Option<usize>,
    /// Number of output/completion tokens used, if reported.
    pub tokens_output: Option<usize>,
    /// Path to the raw execution log file.
    pub log_path: PathBuf,
}

/// Inputs for one sandboxed agent task.
pub struct AgentExecutionRequest<'a> {
    pub project_root: &'a Path,
    pub vcs_selection: VcsSelection,
    pub prompt: &'a str,
    pub context_paths: &'a [PathBuf],
    pub target_dir: &'a Path,
    pub task_id: &'a str,
    pub stream_output: bool,
}

/// Run-record schema parsed from the agent's run metadata file.
#[derive(Debug, Deserialize)]
struct RunRecord {
    #[allow(dead_code)]
    status: String,
    tokens_input: Option<usize>,
    tokens_output: Option<usize>,
}

/// Splits and interpolates command arguments safely without spawning a shell.
pub fn split_command(
    template: &str,
    prompt: &str,
    context_paths: &[PathBuf],
    target_dir: &Path,
) -> Result<(String, Vec<String>)> {
    let raw_args: Vec<&str> = template.split_whitespace().collect();
    if raw_args.is_empty() {
        return Err(KvistError::Io {
            operation: "split agent command template",
            path: PathBuf::from("."),
            source: io::Error::other("empty agent command template"),
        });
    }

    let program = raw_arg_trim(raw_args[0]).to_owned();
    let mut args = Vec::new();

    for raw_arg in &raw_args[1..] {
        if raw_arg.contains("{context_files}") {
            // Replace {context_files} with separate path arguments
            for path in context_paths {
                let path_str = path.to_str().ok_or_else(|| KvistError::Io {
                    operation: "serialize context path",
                    path: path.clone(),
                    source: io::Error::other("non-UTF-8 context path"),
                })?;
                let replaced = raw_arg.replace("{context_files}", path_str);
                args.push(raw_arg_trim(&replaced));
            }
        } else {
            let mut replaced = raw_arg.replace("{prompt}", prompt);
            if replaced.contains("{target_directory}") {
                let dir_str = target_dir.to_str().ok_or_else(|| KvistError::Io {
                    operation: "serialize target directory path",
                    path: target_dir.to_path_buf(),
                    source: io::Error::other("non-UTF-8 target directory path"),
                })?;
                replaced = replaced.replace("{target_directory}", dir_str);
            }
            args.push(raw_arg_trim(&replaced));
        }
    }

    Ok((program, args))
}

fn raw_arg_trim(s: &str) -> String {
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_owned()
    } else {
        s.to_owned()
    }
}

/// Spawns the subprocess, redirects output to log file, and optionally streams to console.
pub fn execute_agent(
    profile: &AgentProfile,
    sandbox_config: &SandboxConfig,
    expected_runner: &RunnerIdentity,
    request: AgentExecutionRequest<'_>,
) -> Result<AgentRunResult> {
    let (program, args) = split_command(
        &profile.command_template,
        request.prompt,
        request.context_paths,
        request.target_dir,
    )?;

    // Ensure logs directory exists
    let logs_dir = request.target_dir.join(".kvist").join("logs");
    fs::create_dir_all(&logs_dir).map_err(|source| KvistError::Io {
        operation: "create agent logs directory",
        path: logs_dir.clone(),
        source,
    })?;

    let timestamp = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let log_file_name = format!(
        "{}_{}.log",
        request.task_id,
        timestamp.to_string().replace(':', "-")
    );
    let log_path = logs_dir.join(log_file_name);
    let mut log_file = File::create(&log_path).map_err(|source| KvistError::Io {
        operation: "create agent log file",
        path: log_path.clone(),
        source,
    })?;

    let context_files = request
        .context_paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let output = sandbox::execute(
        sandbox_config,
        sandbox::ExecutionRequest {
            project_root: request.project_root,
            vcs_selection: request.vcs_selection,
            component_dir: request.target_dir,
            program: &program,
            arguments: &args,
            environment: sandbox::allowed_environment(sandbox_config, None),
            context_files: &context_files,
        },
        expected_runner,
    )?;
    log_file
        .write_all(&output.stdout)
        .and_then(|()| log_file.write_all(&output.stderr))
        .map_err(|source| KvistError::Io {
            operation: "write agent log",
            path: log_path.clone(),
            source,
        })?;
    if request.stream_output {
        io::stdout().write_all(&output.stdout).ok();
        io::stderr().write_all(&output.stderr).ok();
    }
    let success = output.status.success();

    // 4. Try parsing the JSON Run Record for token feedback
    // The run record should be written by the agent at .kvist/runs/<task_id>_<timestamp>.json
    let runs_dir = request.target_dir.join(".kvist").join("runs");
    let record_name = format!(
        "{}_{}.json",
        request.task_id,
        timestamp.to_string().replace(':', "-")
    );
    let record_path = runs_dir.join(record_name);

    let mut tokens_input = None;
    let mut tokens_output = None;

    if record_path.exists() {
        if let Ok(contents) = fs::read_to_string(&record_path) {
            if let Ok(record) = serde_json::from_str::<RunRecord>(&contents) {
                tokens_input = record.tokens_input;
                tokens_output = record.tokens_output;
            }
        }
    }

    Ok(AgentRunResult {
        success,
        tokens_input,
        tokens_output,
        log_path,
    })
}
