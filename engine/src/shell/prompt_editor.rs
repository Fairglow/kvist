use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitStatus, Stdio};

use agent_runtime::{MAX_PROMPT_BYTES, split_raw_command};

use super::state::DynamicState;
use crate::{KvistError, Result, cli, task_queue};

/// Parses one line and either runs the `prompt <task_id>` editor flow or
/// dispatches the line as a normal command.
pub fn prompt_editor_command(project_dir: &Path, task_id: &str) -> Result<Option<cli::Command>> {
    let state = DynamicState::load(project_dir);
    let Some((component, task)) = state.find_task(task_id) else {
        return Ok(None);
    };

    println!("Authoring a prompt for task `{task_id}` in component `{component}`:");
    let prompt = edit_prompt_with_seed(&build_prompt_seed(task))?;
    print_prompt_block(&prompt);

    Ok(Some(cli::Command::Prompt {
        prompt: Some(prompt),
        file: None,
        editor: false,
        role: "developer".to_owned(),
        model: None,
        reasoning_effort: None,
        idle_timeout: 900,
        detect_loops: false,
        max_restarts: 3,
        allow_host_execution: false,
    }))
}

/// Opens `$VISUAL`/`$EDITOR` (defaulting to `vi`) on a temporary file seeded
/// with `seed`, and returns the validated contents after the editor exits.
pub fn edit_prompt_with_seed(seed: &str) -> Result<String> {
    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| "vi".into())
        .into_string()
        .map_err(|_| KvistError::InvalidPromptInput {
            reason: "VISUAL or EDITOR must be valid UTF-8".to_owned(),
        })?;
    edit_prompt_with_editor(seed, &editor)
}

/// Opens an explicit editor command (a file path or a `program args...`
/// line) on a temporary file seeded with `seed`, and returns the validated
/// contents after the editor exits. The prompt must be non-empty and within
/// [`MAX_PROMPT_BYTES`]; a non-zero editor exit is a cancellation, not a
/// crash.
pub fn edit_prompt_with_editor(seed: &str, editor: &str) -> Result<String> {
    let directory = tempfile::tempdir().map_err(|source| KvistError::Io {
        operation: "create prompt editor directory",
        path: std::env::temp_dir(),
        source,
    })?;
    let prompt_path = directory.path().join("prompt.md");
    fs::write(&prompt_path, seed).map_err(|source| KvistError::Io {
        operation: "create prompt editor file",
        path: prompt_path.clone(),
        source,
    })?;

    let (program, mut arguments) = if Path::new(editor).is_file() {
        (editor.to_owned(), Vec::new())
    } else {
        split_raw_command(editor).map_err(KvistError::AgentRuntime)?
    };
    arguments.push(
        prompt_path
            .to_str()
            .ok_or_else(|| KvistError::InvalidPromptInput {
                reason: "temporary prompt path must be valid UTF-8".to_owned(),
            })?
            .to_owned(),
    );

    let status = spawn_editor(&program, &arguments)?;

    if !status.success() {
        return Err(KvistError::InvalidPromptInput {
            reason: format!(
                "editor `{program}` exited with status {status}; prompt authoring cancelled"
            ),
        });
    }

    let contents = fs::read_to_string(&prompt_path).map_err(|source| KvistError::Io {
        operation: "read edited prompt",
        path: prompt_path.clone(),
        source,
    })?;
    if contents.len() as u64 > MAX_PROMPT_BYTES {
        return Err(KvistError::InvalidPromptInput {
            reason: format!("edited prompt exceeds the {MAX_PROMPT_BYTES}-byte limit"),
        });
    }
    if contents.trim().is_empty() {
        return Err(KvistError::InvalidPromptInput {
            reason: "edited prompt must not be empty".to_owned(),
        });
    }
    Ok(contents)
}

/// Launches the editor, retrying the transient `ETXTBSY` ("text file
/// busy") failure.
///
/// On Linux the kernel briefly reports a file as busy while another thread
/// in the same process is writing files, and an editor binary can be busy
/// while an updater rewrites it; both clear on their own, so a short bounded
/// retry is safe and keeps a launch failure from being misreported as a
/// missing editor.
fn spawn_editor(program: &str, arguments: &[String]) -> Result<ExitStatus> {
    let mut retries = 0u32;
    loop {
        match ProcessCommand::new(program)
            .args(arguments)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
        {
            Ok(status) => return Ok(status),
            Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy && retries < 4 => {
                retries += 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => {
                return Err(KvistError::Io {
                    operation: "open prompt editor",
                    path: PathBuf::from(program),
                    source: error,
                });
            }
        }
    }
}

/// Seeds the editor buffer with the task's recorded context.
pub fn build_prompt_seed(task: &task_queue::Task) -> String {
    let mut seed = String::new();
    seed.push_str(&format!(
        "# Prompt for task `{}`
",
        task.id
    ));
    seed.push_str(&format!(
        "Title: {}
",
        task.title
    ));
    if !task.description.is_empty() {
        seed.push_str(&format!(
            "
Description:
{}
",
            task.description
        ));
    }
    if !task.context.is_empty() {
        seed.push_str(&format!(
            "
Context:
{}
",
            task.context
        ));
    }
    if !task.purpose.is_empty() {
        seed.push_str(&format!(
            "
Purpose:
{}
",
            task.purpose
        ));
    }
    if !task.expected_outcome.is_empty() {
        seed.push_str(&format!(
            "
Expected outcome:
{}
",
            task.expected_outcome
        ));
    }
    seed.push_str(
        "
---
Edit the prompt below and save to submit:

",
    );
    seed
}

/// Displays the authored prompt as a formatted block before submission.
pub fn print_prompt_block(prompt: &str) {
    println!("--------------------------------------------------");
    println!(" prompt");
    for line in prompt.lines() {
        println!("   {line}");
    }
    println!("--------------------------------------------------");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_queue::{Task, TaskKind, TaskStatus, TaskTimestamps};

    fn sample_task(id: &str, description: &str) -> Task {
        Task {
            id: id.to_owned(),
            title: id.to_owned(),
            description: description.to_owned(),
            context: "Context info".to_owned(),
            purpose: "Purpose info".to_owned(),
            expected_outcome: "Outcome info".to_owned(),
            kind: TaskKind::Test,
            status: TaskStatus::Pending,
            depends_on: Vec::new(),
            requirements: Vec::new(),
            timestamps: TaskTimestamps {
                created_at: task_queue::Timestamp::default(),
                updated_at: task_queue::Timestamp::default(),
                completed_at: None,
            },
            blocked_reason: None,
            recovery_state: None,
            acceptance_id: None,
            disposition: None,
        }
    }

    #[test]
    fn prompt_editor_command_is_none_for_an_unknown_task() {
        let dir = tempfile::tempdir().unwrap();
        assert!(prompt_editor_command(dir.path(), "nope").unwrap().is_none());
    }

    #[test]
    fn seed_includes_the_task_context() {
        let task = sample_task("write-tests", "Define the tests.");
        let seed = build_prompt_seed(&task);
        assert!(seed.contains("# Prompt for task `write-tests`"));
        assert!(seed.contains("Title: write-tests"));
        assert!(seed.contains("Define the tests."));
        assert!(seed.contains("Context info"));
        assert!(seed.contains("Purpose info"));
        assert!(seed.contains("Outcome info"));
        assert!(seed.ends_with(
            "Edit the prompt below and save to submit:

"
        ));
    }

    #[test]
    fn seed_omits_empty_fields() {
        let mut task = sample_task("write-tests", "");
        task.context = String::new();
        task.purpose = String::new();
        task.expected_outcome = String::new();
        let seed = build_prompt_seed(&task);
        assert!(!seed.contains("Description:"));
        assert!(!seed.contains("Context:"));
        assert!(!seed.contains("Purpose:"));
        assert!(!seed.contains("Expected outcome:"));
    }

    #[test]
    fn print_prompt_block_runs_without_panicking() {
        print_prompt_block(
            "hello world
line 2",
        );
    }

    /// Writes an executable POSIX shell script acting as the editor: it
    /// receives the prompt file path as its first argument.
    #[cfg(unix)]
    fn editor_script(dir: &Path, body: &str) -> String {
        let script = dir.join("editor.sh");
        fs::write(&script, format!("#!/bin/sh\n{body}")).expect("write editor script");
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
        script.to_str().expect("utf-8 path").to_owned()
    }

    #[test]
    #[cfg(unix)]
    fn editor_cancellation_is_a_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let editor = editor_script(dir.path(), "exit 3");
        let error = edit_prompt_with_editor("seed", &editor).unwrap_err();
        match error {
            KvistError::InvalidPromptInput { reason } => {
                assert!(reason.contains("exited with status"), "reason: {reason}");
                assert!(reason.contains("cancelled"), "reason: {reason}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn editor_empty_prompt_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let editor = editor_script(dir.path(), ": > \"$1\"");
        let error = edit_prompt_with_editor("seed", &editor).unwrap_err();
        match error {
            KvistError::InvalidPromptInput { reason } => {
                assert!(reason.contains("must not be empty"), "reason: {reason}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn editor_oversized_prompt_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        // The script emits MAX_PROMPT_BYTES + 1 bytes into the prompt file.
        let editor = editor_script(
            dir.path(),
            &format!(
                "head -c {} /dev/zero | tr '\\0' 'a' > \"$1\"",
                MAX_PROMPT_BYTES + 1
            ),
        );
        let error = edit_prompt_with_editor("seed", &editor).unwrap_err();
        match error {
            KvistError::InvalidPromptInput { reason } => {
                assert!(reason.contains("exceeds the"), "reason: {reason}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn editor_valid_edit_is_returned() {
        let dir = tempfile::tempdir().unwrap();
        let editor = editor_script(dir.path(), "printf 'do the thing' > \"$1\"");
        let prompt = edit_prompt_with_editor("seed", &editor).expect("valid edit");
        assert_eq!(prompt, "do the thing");
    }

    #[test]
    #[cfg(unix)]
    fn editor_must_exist_or_the_error_names_the_program() {
        let error = edit_prompt_with_editor("seed", "/definitely/not/an/editor").unwrap_err();
        match error {
            KvistError::Io { operation, .. } => {
                assert!(operation == "open prompt editor");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
