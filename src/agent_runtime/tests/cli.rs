#![cfg(target_os = "linux")]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use agent_runtime::load_profile;
use nix::{
    errno::Errno,
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use tempfile::TempDir;

#[test]
fn host_execution_requires_explicit_acknowledgement() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args(["run", "--command", "/bin/echo '{prompt}'", "hello"])
        .output()
        .expect("run standalone CLI");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("--allow-host-execution")
    );
}

#[test]
fn invalid_xdg_config_home_falls_back_to_absolute_home() {
    let workspace = TempDir::new().expect("workspace");
    let expected = workspace.path().join(".config/agent-runtime/config.toml");

    for xdg in ["", "relative-config"] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
            .current_dir(workspace.path())
            .env("XDG_CONFIG_HOME", xdg)
            .env("HOME", workspace.path())
            .args([
                "run",
                "--allow-host-execution",
                "--profile",
                "missing",
                "hello",
            ])
            .output()
            .expect("resolve default profile path");

        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .expect("UTF-8 stderr")
                .contains(&expected.to_string_lossy().into_owned())
        );
    }
}

#[test]
fn default_profile_path_reads_legacy_store_when_canonical_store_is_absent() {
    let workspace = TempDir::new().expect("workspace");
    let legacy_directory = workspace.path().join("supervised-agent");
    fs::create_dir(&legacy_directory).expect("create legacy profile directory");
    fs::write(
        legacy_directory.join("config.toml"),
        "schema_version = 1\n\
         [[profiles]]\n\
         name = \"legacy\"\n\
         provider = \"custom-script\"\n\
         command = \"/bin/echo '{prompt}'\"\n",
    )
    .expect("write legacy profiles");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .env("XDG_CONFIG_HOME", workspace.path())
        .env_remove("HOME")
        .args([
            "run",
            "--allow-host-execution",
            "--profile",
            "legacy",
            "legacy profile",
        ])
        .output()
        .expect("run legacy profile");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 output")
            .contains("legacy profile")
    );
}

#[test]
fn canonical_profile_store_takes_precedence_over_legacy_store() {
    let workspace = TempDir::new().expect("workspace");
    let legacy_directory = workspace.path().join("supervised-agent");
    fs::create_dir(&legacy_directory).expect("create legacy profile directory");
    fs::write(
        legacy_directory.join("config.toml"),
        "schema_version = 1\n\
         [[profiles]]\n\
         name = \"legacy\"\n\
         provider = \"custom-script\"\n\
         command = \"/bin/echo '{prompt}'\"\n",
    )
    .expect("write legacy profiles");
    let canonical_directory = workspace.path().join("agent-runtime");
    fs::create_dir(&canonical_directory).expect("create canonical profile directory");
    fs::write(
        canonical_directory.join("config.toml"),
        "not valid profile configuration",
    )
    .expect("write canonical profiles");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .env("XDG_CONFIG_HOME", workspace.path())
        .env_remove("HOME")
        .args([
            "run",
            "--allow-host-execution",
            "--profile",
            "legacy",
            "must not run",
        ])
        .output()
        .expect("select canonical profile store");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 error")
            .contains(&canonical_directory.to_string_lossy().into_owned())
    );
}

#[test]
fn standalone_cli_runs_a_prompt_from_a_file() {
    let workspace = TempDir::new().expect("workspace");
    let prompt = workspace.path().join("prompt.md");
    fs::write(&prompt, "standalone prompt").expect("write prompt");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .args([
            "run",
            "--allow-host-execution",
            "--command",
            "/bin/echo '{prompt}'",
            "--file",
        ])
        .arg(prompt)
        .output()
        .expect("run standalone CLI");

    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 stdout")
            .contains("standalone prompt")
    );
}

#[test]
fn supervised_provider_cannot_consume_caller_stdin() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "run",
            "--allow-host-execution",
            "--idle-timeout",
            "1",
            "--command",
            "sh -c 'if read value; then exit 9; fi'",
            "hello",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("run standalone CLI");
    let held_stdin = child.stdin.take().expect("provider stdin pipe");

    let status = child.wait().expect("wait for standalone CLI");
    drop(held_stdin);

    assert!(status.success());
}

#[test]
fn setup_persists_a_profile_and_run_can_select_it() {
    let workspace = TempDir::new().expect("workspace");
    let configuration = workspace.path().join("profiles.toml");
    let provider = workspace.path().join("provider.sh");
    fs::write(&provider, "#!/bin/sh\nprintf 'profile: %s\\n' \"$*\"\n").expect("write provider");
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mut setup = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .args(["setup", "--config", "profiles.toml"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start setup");
    use std::io::Write;
    write!(
        setup.stdin.take().expect("setup stdin"),
        "6\nprovider.sh\nstandalone\n\nn\n"
    )
    .expect("write setup answers");
    let setup_output = setup.wait_with_output().expect("wait for setup");
    assert!(
        setup_output.status.success(),
        "{}",
        String::from_utf8_lossy(&setup_output.stderr)
    );

    let execution_directory = TempDir::new().expect("execution directory");
    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(execution_directory.path())
        .args([
            "run",
            "--allow-host-execution",
            "--profile",
            "standalone",
            "--config",
        ])
        .arg(&configuration)
        .arg("hello profile")
        .output()
        .expect("run stored profile");

    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 output")
            .contains("hello profile")
    );
}

#[test]
fn llama_cli_fallback_preserves_provider_and_uses_supported_flags() {
    let workspace = TempDir::new().expect("workspace");
    let configuration = workspace.path().join("profiles.toml");
    let wrapper = workspace.path().join("llama-wrapper");
    let model = workspace.path().join("model.gguf");
    fs::write(
        &wrapper,
        "#!/bin/sh\n[ \"$1\" = --version ] && { echo 'llama 1.0'; exit 0; }\nexit 1\n",
    )
    .expect("write wrapper");
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))
        .expect("make wrapper executable");
    fs::write(&model, "test model").expect("write model");
    let empty_path = workspace.path().join("empty-path");
    fs::create_dir(&empty_path).expect("create empty PATH");

    let mut setup = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .env("PATH", &empty_path)
        .args(["setup", "--config"])
        .arg(&configuration)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start setup");
    use std::io::Write;
    write!(
        setup.stdin.take().expect("setup stdin"),
        "1\nllama-wrapper\nmodel.gguf\nlocal-llama\n\nn\n"
    )
    .expect("write setup answers");
    let output = setup.wait_with_output().expect("wait for setup");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let profile = load_profile(&configuration, "local-llama").expect("load profile");
    assert_eq!(profile.provider, "llama-cli");
    assert!(profile.command.starts_with(&format!(
        "\"{}\" --model \"{}\" --prompt",
        wrapper.display(),
        model.display()
    )));
    assert!(profile.command.contains("--single-turn"));
    assert!(profile.command.contains("--simple-io"));
    assert!(profile.command.contains("--no-display-prompt"));
    assert!(profile.command.contains("--predict 4096"));
    assert!(!profile.command.contains("--context"));
    assert!(!profile.command.contains("--format"));
}

#[test]
fn installed_gemini_and_copilot_use_noninteractive_templates() {
    let workspace = TempDir::new().expect("workspace");
    let bin = workspace.path().join("bin");
    fs::create_dir(&bin).expect("create bin");
    for executable in ["gemini", "copilot"] {
        let path = bin.join(executable);
        fs::write(
            &path,
            "#!/bin/sh\n[ \"$1\" = --version ] && { echo '1.0'; exit 0; }\nexit 1\n",
        )
        .expect("write executable");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("make executable");
    }
    let configuration = workspace.path().join("profiles.toml");

    for (answers, profile_name) in [
        ("5\ngemini-2.5-pro\ngemini-pro\n\nn\n", "gemini-pro"),
        ("4\ngpt-5.4\ncopilot-pro\n\nn\n", "copilot-pro"),
    ] {
        let mut setup = Command::new(env!("CARGO_BIN_EXE_agent-run"))
            .current_dir(workspace.path())
            .env("PATH", &bin)
            .args(["setup", "--config"])
            .arg(&configuration)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start setup");
        use std::io::Write;
        setup
            .stdin
            .take()
            .expect("setup stdin")
            .write_all(answers.as_bytes())
            .expect("write setup answers");
        let output = setup.wait_with_output().expect("wait for setup");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let profile = load_profile(&configuration, profile_name).expect("load profile");
        if profile.provider == "gemini-cli" {
            assert!(profile.command.starts_with("\"gemini\" --prompt"));
            assert!(profile.command.contains("--output-format text"));
            assert!(profile.command.contains("--approval-mode yolo"));
            assert!(profile.command.contains("--model"));
            assert!(profile.command.contains("gemini-2.5-pro"));
            assert!(!profile.command.contains("--files"));
        } else {
            assert_eq!(profile.provider, "copilot");
            assert!(profile.command.starts_with("\"copilot\" --prompt"));
            assert!(profile.command.contains("--silent"));
            assert!(profile.command.contains("--allow-all-tools"));
            assert!(profile.command.contains("--no-ask-user"));
            assert!(profile.command.contains("--model"));
            assert!(profile.command.contains("gpt-5.4"));
            assert!(!profile.command.contains("copilot chat"));
        }
    }
}

#[test]
fn unusable_provider_fallback_is_rejected_before_persistence() {
    let workspace = TempDir::new().expect("workspace");
    let configuration = workspace.path().join("profiles.toml");
    let unusable = workspace.path().join("broken-gemini");
    fs::write(&unusable, "#!/bin/sh\nexit 7\n").expect("write executable");
    fs::set_permissions(&unusable, fs::Permissions::from_mode(0o700)).expect("make executable");
    let empty_path = workspace.path().join("empty-path");
    fs::create_dir(&empty_path).expect("create empty PATH");

    let mut setup = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .env("PATH", &empty_path)
        .args(["setup", "--config"])
        .arg(&configuration)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start setup");
    use std::io::Write;
    writeln!(
        setup.stdin.take().expect("setup stdin"),
        "5\n{}",
        unusable.display()
    )
    .expect("write setup answers");
    let output = setup.wait_with_output().expect("wait for setup");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("version probe")
    );
    assert!(!configuration.exists());
}

#[test]
fn cancelling_conventional_probe_does_not_request_a_fallback() {
    let workspace = TempDir::new().expect("workspace");
    let bin = workspace.path().join("bin");
    fs::create_dir(&bin).expect("create bin");
    let marker = workspace.path().join("probe.pid");
    let gemini = bin.join("gemini");
    fs::write(
        &gemini,
        format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec sleep 30\n",
            marker.display()
        ),
    )
    .expect("write executable");
    fs::set_permissions(&gemini, fs::Permissions::from_mode(0o700)).expect("make executable");

    let mut setup = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .env("PATH", &bin)
        .args(["setup", "--config", "profiles.toml"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start setup");
    use std::io::Write;
    let mut stdin = setup.stdin.take().expect("setup stdin");
    stdin.write_all(b"5\n").expect("select Gemini");

    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "version probe did not start");
    kill(
        Pid::from_raw(i32::try_from(setup.id()).expect("setup pid")),
        Signal::SIGINT,
    )
    .expect("interrupt setup");
    drop(stdin);
    let output = setup.wait_with_output().expect("wait for setup");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("cancelled")
    );
    assert!(
        !String::from_utf8(output.stdout)
            .expect("UTF-8 stdout")
            .contains("Supply a direct executable")
    );
}

#[test]
fn interrupt_terminates_the_supervised_process_group() {
    let workspace = TempDir::new().expect("workspace");
    let marker = workspace.path().join("provider.pid");
    let provider = workspace.path().join("provider.sh");
    fs::write(
        &provider,
        format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nsetsid sleep 2 &\nexec sleep 30\n",
            marker.display()
        ),
    )
    .expect("write provider");
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "run",
            "--allow-host-execution",
            "--idle-timeout",
            "30",
            "--command",
            &format!("{} '{{prompt}}'", provider.display()),
            "interrupt test",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start standalone CLI");

    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let provider_pid = fs::read_to_string(&marker)
        .expect("provider wrote pid")
        .parse::<i32>()
        .expect("valid provider pid");

    kill(
        Pid::from_raw(i32::try_from(supervisor.id()).expect("supervisor pid")),
        Signal::SIGINT,
    )
    .expect("interrupt supervisor");
    let status = supervisor.wait().expect("wait for supervisor");
    assert!(!status.success());

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match kill(Pid::from_raw(provider_pid), None) {
            Err(Errno::ESRCH) => break,
            _ if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            result => panic!("provider process remained after cancellation: {result:?}"),
        }
    }
}
