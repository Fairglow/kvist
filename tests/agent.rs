use std::{
    fs,
    path::{Path, PathBuf},
};

use kvist::{
    agent::{execute_agent, split_command},
    config::AgentProfile,
};
use tempfile::TempDir;

#[test]
fn split_command_interpolates_placeholders_and_trims_quotes_correctly() {
    let template =
        "my-agent --message '{prompt}' --files {context_files} --dir '{target_directory}'";
    let prompt = "Implement user authentication";
    let context_paths = vec![
        PathBuf::from("/workspace/src/SPEC.md"),
        PathBuf::from("/workspace/src/TODOS.yaml"),
    ];
    let target_dir = Path::new("/workspace/src");

    let (program, args) = split_command(template, prompt, &context_paths, target_dir)
        .expect("successful split and interpolation");

    assert_eq!(program, "my-agent");
    assert_eq!(
        args,
        vec![
            "--message".to_owned(),
            "Implement user authentication".to_owned(),
            "--files".to_owned(),
            "/workspace/src/SPEC.md".to_owned(),
            "/workspace/src/TODOS.yaml".to_owned(),
            "--dir".to_owned(),
            "/workspace/src".to_owned(),
        ]
    );
}

#[test]
fn execute_agent_captures_stdout_and_stderr_in_log_file() {
    let workspace = TempDir::new().expect("workspace");

    // We use a basic command available on standard platforms like 'echo'
    let profile = AgentProfile {
        command_template: "echo '{prompt}'".to_owned(),
        token_limit: None,
    };

    let prompt = "hello external agent";
    let context_paths: Vec<PathBuf> = vec![];
    let target_dir = workspace.path();
    let task_id = "test-task";

    let result = execute_agent(&profile, prompt, &context_paths, target_dir, task_id, false)
        .expect("agent execution success");

    assert!(result.success);
    assert_eq!(result.tokens_input, None);
    assert_eq!(result.tokens_output, None);

    // Verify log file exists and contains the echoed prompt
    assert!(result.log_path.exists());
    let log_contents = fs::read_to_string(&result.log_path).expect("read log contents");
    assert!(log_contents.contains("hello external agent"));
}

#[test]
#[cfg(unix)]
fn execute_agent_parses_json_run_record_for_token_feedback() {
    let workspace = TempDir::new().expect("workspace");

    // We use a command that sleeps for 1 second so we can write the run record concurrently
    let profile = AgentProfile {
        command_template: "sleep 1".to_owned(),
        token_limit: None,
    };

    let prompt = "sleep-task";
    let context_paths: Vec<PathBuf> = vec![];
    let target_dir = workspace.path();
    let task_id = "sleep-task";

    // Spawn execution in a separate thread
    let target_dir_clone = target_dir.to_path_buf();
    let profile_clone = profile.clone();
    let context_paths_clone = context_paths.clone();
    let handle = std::thread::spawn(move || {
        execute_agent(
            &profile_clone,
            prompt,
            &context_paths_clone,
            &target_dir_clone,
            task_id,
            false,
        )
    });

    // Wait for the log file to be created, and capture its timestamp
    let logs_dir = target_dir.join(".kvist").join("logs");
    let mut log_file_path = None;
    for _ in 0..100 {
        if let Ok(entries) = fs::read_dir(&logs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("log") {
                    log_file_path = Some(path);
                    break;
                }
            }
        }
        if log_file_path.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let log_path = log_path_val(log_file_path);
    let file_name_str = log_path.file_name().unwrap().to_str().unwrap();
    let json_file_name = file_name_str.replace(".log", ".json");

    // Write the run record
    let runs_dir = target_dir.join(".kvist").join("runs");
    fs::create_dir_all(&runs_dir).expect("create runs dir");
    let record_path = runs_dir.join(json_file_name);
    fs::write(
        &record_path,
        r#"{"status":"success","tokens_input":1250,"tokens_output":450}"#,
    )
    .expect("write record");

    // Join the execution thread and verify token counts
    let result = handle.join().unwrap().expect("execution success");
    assert!(result.success);
    assert_eq!(result.tokens_input, Some(1250));
    assert_eq!(result.tokens_output, Some(450));
}

#[cfg(unix)]
fn log_path_val(opt: Option<PathBuf>) -> PathBuf {
    opt.unwrap()
}
