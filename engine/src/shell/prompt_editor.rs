use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};

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

/// Opens `$VISUAL`/`$EDITOR` (defaulting to `nano`, `vim`, or `vi`) on a temporary file
/// seeded with `seed`, and returns the validated contents after the editor exits.
pub fn edit_prompt_with_seed(seed: &str) -> Result<String> {
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

    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| "vi".into())
        .into_string()
        .map_err(|_| KvistError::InvalidPromptInput {
            reason: "VISUAL or EDITOR must be valid UTF-8".to_owned(),
        })?;
    let (program, mut arguments) = if Path::new(&editor).is_file() {
        (editor, Vec::new())
    } else {
        split_raw_command(&editor).map_err(KvistError::AgentRuntime)?
    };
    arguments.push(
        prompt_path
            .to_str()
            .ok_or_else(|| KvistError::InvalidPromptInput {
                reason: "temporary prompt path must be valid UTF-8".to_owned(),
            })?
            .to_owned(),
    );

    let status = ProcessCommand::new(&program)
        .args(&arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|source| KvistError::Io {
            operation: "open prompt editor",
            path: PathBuf::from(&program),
            source,
        })?;

    if !status.success() {
        return Err(KvistError::InvalidPromptInput {
            reason: format!("editor `{program}` exited with status {status}"),
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
}
