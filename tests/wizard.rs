#[cfg(unix)]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    io::{Cursor, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

use agent_runtime::{ModelProfile, upsert_profile};
use kvist::config;
use kvist::wizard::{run_wizard, run_wizard_with_force, run_wizard_with_profile_config};

#[test]
fn test_wizard_ollama_local_config() {
    let project = TempDir::new().expect("create temp dir");
    let project_dir = project.path();

    // Mock inputs:
    // 1. Choose Ollama (Option 3)
    // 2. Enter Ollama base URL (Default)
    // 3. Enter Ollama model name (llama3.1:8b)
    // 4. Choose All Roles (Option 4)
    // 5. Choose Project-local configuration (Option 1)
    let mock_input = "1\n3\n\nllama3.1:8b\n\n4\n1\n";
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    let result = run_wizard_with_force(&mut reader, &mut writer, project_dir, true);
    assert!(result.is_ok());

    let output_str = String::from_utf8(writer).expect("valid utf-8 output");
    assert!(output_str.contains("Kvist Agent Setup Wizard"));
    assert!(output_str.contains("Successfully configured model"));

    // Verify written file
    let toml_path = project_dir.join("kvist.toml");
    assert!(toml_path.is_file());
    let toml_content = fs::read_to_string(&toml_path).expect("read toml");

    assert!(toml_content.contains("[agent.profiles.developer]"));
    assert!(toml_content.contains("[agent.profiles.architect]"));
    assert!(toml_content.contains("[agent.profiles.security-reviewer]"));
    assert!(toml_content.contains("OLLAMA_HOST=http://localhost:11434"));
    assert!(toml_content.contains("ollama run"));
    assert!(toml_content.contains("llama3.1:8b"));
}

#[test]
fn test_wizard_custom_script_local_config() {
    let project = TempDir::new().expect("create temp dir");
    let project_dir = project.path();

    // Create a dummy wrapper script
    let script_path = project_dir.join("my-llama-cli.sh");
    fs::write(&script_path, "#!/bin/sh\necho 'mock'").expect("write script");
    #[cfg(unix)]
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make script executable");

    // Mock inputs:
    // 1. Choose Custom Wrapper Script (Option 6)
    // 2. Enter script path
    // 3. Name the model
    // 4. Keep the default command template
    // 5. Choose Developer Role (Option 1 / default)
    // 6. Choose Project-local configuration (Option 1)
    let mock_input = format!("1\n6\n{}\nlocal-wrapper\n\n1\n1\n", script_path.display());
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    let result = run_wizard(&mut reader, &mut writer, project_dir);
    assert!(result.is_ok());

    let toml_path = project_dir.join("kvist.toml");
    assert!(toml_path.is_file());
    let toml_content = fs::read_to_string(&toml_path).expect("read toml");

    assert!(toml_content.contains("[agent.profiles.developer]"));
    assert!(toml_content.contains("local-wrapper"));
    assert!(toml_content.contains("my-llama-cli.sh"));
}

#[test]
fn wizard_merges_a_model_into_existing_configuration() {
    let project = TempDir::new().expect("create temp dir");
    let project_dir = project.path();
    fs::write(
        project_dir.join("kvist.toml"),
        r#"# Keep this project configuration comment.
schema_version = 1
component_root = "components"
custom_setting = "preserved"

[agent]
profiles = { developer = { model = "existing", default_model = "existing", models = [{ name = "existing", command = "existing-agent '{prompt}'" }] } }
"#,
    )
    .expect("write existing configuration");
    let script_path = project_dir.join("provider.sh");
    fs::write(&script_path, "#!/bin/sh\nexit 0\n").expect("write provider");
    #[cfg(unix)]
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mock_input = format!("1\n6\n{}\nnew-model\n\n1\n1\n", script_path.display());
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    run_wizard(&mut reader, &mut writer, project_dir).expect("update configuration");

    let path = project_dir.join("kvist.toml");
    let contents = fs::read_to_string(&path).expect("read updated configuration");
    assert!(contents.contains("# Keep this project configuration comment."));
    assert!(contents.contains("custom_setting = \"preserved\""));
    assert!(contents.contains("name = \"existing\""));
    assert!(contents.contains("name = \"new-model\""));

    let parsed = config::load(project_dir).expect("load updated configuration");
    assert_eq!(
        parsed.component_root,
        std::path::PathBuf::from("components")
    );
    assert_eq!(parsed.agent.developer.model.as_deref(), Some("new-model"));
    assert_eq!(parsed.agent.developer.models.len(), 2);
}

#[cfg(unix)]
#[test]
fn wizard_tests_a_model_before_persisting_it() {
    let project = TempDir::new().expect("create temp dir");
    let script_path = project.path().join("provider.sh");
    let recorded_prompt = project.path().join("qualification-prompt.txt");
    fs::write(
        &script_path,
        format!(
            "#!/bin/sh\n[ \"$1\" = --prompt ] || exit 8\n\
             printf '%s' \"$2\" > '{}'\nprintf 'verified model: %s\\n' \"$*\"\n",
            recorded_prompt.display()
        ),
    )
    .expect("write provider");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mock_input = format!("1\n6\n{}\nverified\n\n1\n1\n", script_path.display());
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    run_wizard(&mut reader, &mut writer, project.path()).expect("test and save model");

    let output = String::from_utf8(writer).expect("UTF-8 wizard output");
    assert!(output.contains("Model test succeeded"));
    assert_eq!(
        fs::read_to_string(recorded_prompt).expect("read qualification prompt"),
        "Reply with exactly: OK"
    );
    assert!(project.path().join("kvist.toml").is_file());
}

#[cfg(unix)]
#[test]
fn wizard_does_not_persist_a_failed_model_test_without_confirmation() {
    let project = TempDir::new().expect("create temp dir");
    let script_path = project.path().join("provider.sh");
    fs::write(&script_path, "#!/bin/sh\nexit 7\n").expect("write provider");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mock_input = format!("1\n6\n{}\nfailing\n\n", script_path.display());
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    let error = run_wizard(&mut reader, &mut writer, project.path())
        .expect_err("failed model test must stop setup");
    assert!(error.to_string().contains("model verification failed"));
    assert!(!project.path().join("kvist.toml").exists());
}

#[cfg(unix)]
#[test]
fn agent_setup_force_persists_after_failed_qualification() {
    let project = TempDir::new().expect("create temp dir");
    let script_path = project.path().join("provider.sh");
    fs::write(&script_path, "#!/bin/sh\nexit 7\n").expect("write provider");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mut child = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["agent", "setup", "--force"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start forced setup");
    write!(
        child.stdin.take().expect("setup stdin"),
        "1\n6\n{}\nforced\n\n1\n1\n",
        script_path.display()
    )
    .expect("write setup answers");
    let output = child.wait_with_output().expect("wait for forced setup");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 setup output");
    assert!(stdout.contains("Warning: --force"));
    assert!(stdout.contains("Successfully configured model `forced`"));
    let configuration =
        fs::read_to_string(project.path().join("kvist.toml")).expect("read configuration");
    assert!(configuration.contains("model = \"forced\""));
}

#[cfg(unix)]
#[test]
fn json_agent_setup_keeps_stdout_machine_readable() {
    let project = TempDir::new().expect("create temp dir");
    let script_path = project.path().join("provider.sh");
    fs::write(
        &script_path,
        "#!/bin/sh\nprintf 'qualification stdout'\nprintf 'qualification stderr' >&2\n",
    )
    .expect("write provider");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mut child = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["--json", "agent", "setup"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start JSON setup");
    write!(
        child.stdin.take().expect("setup stdin"),
        "1\n6\n{}\njson-model\n\n1\n1\n",
        script_path.display()
    )
    .expect("write setup answers");
    let output = child.wait_with_output().expect("wait for JSON setup");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        "{\"status\":\"success\",\"command\":\"agent-setup\",\"message\":\"agent setup wizard complete\"}\n"
    );
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 stderr");
    assert!(stderr.contains("Kvist Agent Setup Wizard"));
    assert!(!stderr.contains("qualification stdout"));
    assert!(!stderr.contains("qualification stderr"));
    assert!(project.path().join("kvist.toml").is_file());
}

#[cfg(unix)]
#[test]
fn agent_setup_force_does_not_persist_after_cancellation() {
    let project = TempDir::new().expect("create temp dir");
    let marker = project.path().join("qualification-started");
    let script_path = project.path().join("provider.sh");
    fs::write(
        &script_path,
        format!("#!/bin/sh\ntouch '{}'\nexec sleep 30\n", marker.display()),
    )
    .expect("write provider");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mut child = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["agent", "setup", "--force"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start forced setup");
    write!(
        child.stdin.take().expect("setup stdin"),
        "1\n6\n{}\ncancelled\n\n",
        script_path.display()
    )
    .expect("write setup answers");

    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "qualification command did not start");
    kill(
        Pid::from_raw(i32::try_from(child.id()).expect("child PID")),
        Signal::SIGINT,
    )
    .expect("interrupt setup");
    let output = child.wait_with_output().expect("wait for cancelled setup");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("cancelled")
    );
    assert!(!project.path().join("kvist.toml").exists());
}

#[cfg(unix)]
#[test]
fn wizard_qualification_implies_host_acknowledgement() {
    let project = TempDir::new().expect("create temp dir");
    let marker = project.path().join("executed");
    let script_path = project.path().join("provider.sh");
    fs::write(
        &script_path,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .expect("write provider");
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mock_input = format!("1\n6\n{}\nimplicit\n\n1\n1\n", script_path.display());
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    run_wizard(&mut reader, &mut writer, project.path())
        .expect("setup qualification implies host acknowledgement");

    assert!(marker.exists());
    assert!(project.path().join("kvist.toml").exists());
}

#[test]
fn wizard_rejects_invalid_existing_configuration_before_editing() {
    let project = TempDir::new().expect("create temp dir");
    let config_path = project.path().join("kvist.toml");
    let original = r#"schema_version = 1
component_root = "src"

[agent.profiles.developer]
models = []
"#;
    fs::write(&config_path, original).expect("write invalid configuration");
    let script_path = project.path().join("provider.sh");
    fs::write(&script_path, "#!/bin/sh\nexit 0\n").expect("write provider");
    #[cfg(unix)]
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mock_input = format!("1\n6\n{}\nnew-model\n\n1\n1\n", script_path.display());
    let mut reader = Cursor::new(mock_input);
    let mut writer = Vec::new();

    run_wizard(&mut reader, &mut writer, project.path())
        .expect_err("invalid configuration must not be repaired");
    assert_eq!(
        fs::read_to_string(config_path).expect("read unchanged configuration"),
        original
    );
}

#[test]
fn wizard_materializes_a_standalone_profile_into_kvist_roles() {
    let project = TempDir::new().expect("create project");
    let profile_config = project.path().join("profiles.toml");
    upsert_profile(
        &profile_config,
        &ModelProfile {
            name: "shared".to_owned(),
            provider: "custom-script".to_owned(),
            command: "/bin/echo '{prompt}'".to_owned(),
        },
    )
    .expect("write reusable profile");
    let mut reader = Cursor::new("2\nshared\n4\n1\n");
    let mut writer = Vec::new();

    run_wizard_with_profile_config(&mut reader, &mut writer, project.path(), &profile_config)
        .expect("bind standalone profile");

    let contents =
        fs::read_to_string(project.path().join("kvist.toml")).expect("read Kvist config");
    assert_eq!(contents.matches("name = \"shared\"").count(), 3);
    assert_eq!(
        contents
            .matches("command = \"/bin/echo '{prompt}'\"")
            .count(),
        3
    );
}
