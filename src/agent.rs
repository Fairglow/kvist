//! External agent execution and response capture.

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

use serde::Deserialize;

use crate::{KvistError, Result, config::AgentProfile, task_queue::Timestamp};

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
    prompt: &str,
    context_paths: &[PathBuf],
    target_dir: &Path,
    task_id: &str,
    stream_output: bool,
) -> Result<AgentRunResult> {
    let (program, args) =
        split_command(&profile.command_template, prompt, context_paths, target_dir)?;

    // Ensure logs directory exists
    let logs_dir = target_dir.join(".kvist").join("logs");
    fs::create_dir_all(&logs_dir).map_err(|source| KvistError::Io {
        operation: "create agent logs directory",
        path: logs_dir.clone(),
        source,
    })?;

    let timestamp = Timestamp::now().map_err(|source| KvistError::TaskClock { source })?;
    let log_file_name = format!("{task_id}_{}.log", timestamp.to_string().replace(':', "-"));
    let log_path = logs_dir.join(log_file_name);
    let log_file = File::create(&log_path).map_err(|source| KvistError::Io {
        operation: "create agent log file",
        path: log_path.clone(),
        source,
    })?;

    // Prepare command
    let mut child = Command::new(&program)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| KvistError::Io {
            operation: "spawn external agent",
            path: PathBuf::from(&program),
            source,
        })?;

    let mut stdout_pipe = child.stdout.take().ok_or_else(|| KvistError::Io {
        operation: "take agent stdout",
        path: PathBuf::from(&program),
        source: io::Error::other("cannot take agent stdout pipe"),
    })?;

    let mut stderr_pipe = child.stderr.take().ok_or_else(|| KvistError::Io {
        operation: "take agent stderr",
        path: PathBuf::from(&program),
        source: io::Error::other("cannot take agent stderr pipe"),
    })?;

    // Create log file clones for stdout/stderr reader threads
    let mut stdout_log = log_file.try_clone().map_err(|source| KvistError::Io {
        operation: "clone agent log file for stdout",
        path: log_path.clone(),
        source,
    })?;

    let mut stderr_log = log_file.try_clone().map_err(|source| KvistError::Io {
        operation: "clone agent log file for stderr",
        path: log_path.clone(),
        source,
    })?;

    // Thread 1: Read stdout, write to log file, optionally write to console
    let stdout_handle = thread::spawn(move || -> io::Result<()> {
        let mut buffer = [0u8; 1024];
        loop {
            let bytes_read = stdout_pipe.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            stdout_log.write_all(&buffer[..bytes_read])?;
            if stream_output {
                io::stdout().write_all(&buffer[..bytes_read])?;
                let _ = io::stdout().flush();
            }
        }
        Ok(())
    });

    // Thread 2: Read stderr, write to log file, optionally write to console
    let stderr_handle = thread::spawn(move || -> io::Result<()> {
        let mut buffer = [0u8; 1024];
        loop {
            let bytes_read = stderr_pipe.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            stderr_log.write_all(&buffer[..bytes_read])?;
            if stream_output {
                io::stderr().write_all(&buffer[..bytes_read])?;
                let _ = io::stderr().flush();
            }
        }
        Ok(())
    });

    // Wait for agent to exit
    let exit_status = child.wait().map_err(|source| KvistError::Io {
        operation: "wait for external agent",
        path: PathBuf::from(&program),
        source,
    })?;

    // Ensure reading threads are complete
    let _ = stdout_handle.join().map_err(|_| KvistError::Io {
        operation: "join stdout reader thread",
        path: log_path.clone(),
        source: io::Error::other("stdout reader thread panicked"),
    })?;

    let _ = stderr_handle.join().map_err(|_| KvistError::Io {
        operation: "join stderr reader thread",
        path: log_path.clone(),
        source: io::Error::other("stderr reader thread panicked"),
    })?;

    let success = exit_status.success();

    // 4. Try parsing the JSON Run Record for token feedback
    // The run record should be written by the agent at .kvist/runs/<task_id>_<timestamp>.json
    let runs_dir = target_dir.join(".kvist").join("runs");
    let record_name = format!("{task_id}_{}.json", timestamp.to_string().replace(':', "-"));
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
