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
fn models_http_text_and_json_outputs_are_deterministic() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind catalog server");
    let endpoint = format!("http://{}", listener.local_addr().expect("catalog address"));
    thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept models request");
            let mut request = [0_u8; 1024];
            let count = stream.read(&mut request).expect("read models request");
            assert!(count > 0, "models request must not be empty");
            let body = r#"{"data":[{"id":"b"},{"id":"a"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write models response");
        }
    });

    let text = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "models",
            "--provider",
            "llama-server",
            "--endpoint",
            &endpoint,
        ])
        .output()
        .expect("list text models");
    assert!(
        text.status.success(),
        "{}",
        String::from_utf8_lossy(&text.stderr)
    );
    assert_eq!(
        String::from_utf8(text.stdout).expect("UTF-8 model IDs"),
        "b\na\n"
    );

    let json = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "models",
            "--provider",
            "llama-server",
            "--endpoint",
            &endpoint,
            "--json",
        ])
        .output()
        .expect("list JSON models");
    assert!(
        json.status.success(),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&json.stdout).expect("one catalog object"),
        serde_json::json!({
            "format_version": 1,
            "provider": "llama-server",
            "current_model_id": null,
            "models": [{"id":"b","name":"b"},{"id":"a","name":"a"}]
        })
    );
}

#[test]
fn models_reports_manual_providers_as_unsupported_without_spawning() {
    let workspace = TempDir::new().expect("workspace");
    let marker = workspace.path().join("spawned");
    let executable = workspace.path().join("provider");
    fs::write(
        &executable,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .expect("write provider");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    for provider in ["llama-cli", "custom-script"] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
            .args(["models", "--provider", provider, "--executable"])
            .arg(&executable)
            .output()
            .expect("request unsupported catalog");

        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .expect("UTF-8 unsupported-catalog error")
                .contains("provider model catalog")
        );
        assert!(!marker.exists());
    }
}

#[test]
fn models_acp_requires_host_discovery_acknowledgement_before_spawn() {
    let workspace = TempDir::new().expect("workspace");
    let marker = workspace.path().join("started");
    let provider = workspace.path().join("provider");
    fs::write(
        &provider,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .expect("write provider");
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args(["models", "--provider", "copilot", "--executable"])
        .arg(provider)
        .output()
        .expect("run refused ACP discovery");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 error")
            .contains("--allow-host-discovery")
    );
    assert!(!marker.exists());
}

#[test]
fn models_acp_lists_correlated_provider_ids() {
    let workspace = TempDir::new().expect("workspace");
    let provider = workspace.path().join("provider");
    fs::write(
        &provider,
        r#"#!/bin/sh
test "$1" = --acp || exit 20
IFS= read -r initialize || exit 21
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'
IFS= read -r session || exit 22
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"models":{"availableModels":[{"modelId":"account-current","name":"Current"},{"modelId":"account-other","name":"Other"}],"currentModelId":"account-current"}}}'
"#,
    )
    .expect("write ACP provider");
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))
        .expect("make ACP provider executable");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .args([
            "models",
            "--provider",
            "gemini",
            "--allow-host-discovery",
            "--executable",
        ])
        .arg(provider)
        .args(["--json"])
        .output()
        .expect("list ACP models");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let catalog =
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("catalog JSON");
    assert_eq!(catalog["current_model_id"], "account-current");
    assert_eq!(catalog["models"][0]["id"], "account-current");
    assert_eq!(catalog["models"][1]["id"], "account-other");
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
fn standalone_json_run_emits_only_captured_content() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "run",
            "--json",
            "--allow-host-execution",
            "--command",
            "sh -c 'printf answer; printf progress >&2'",
            "hello",
        ])
        .output()
        .expect("run standalone JSON prompt");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("single JSON output"),
        serde_json::json!({"content": "answer"})
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn standalone_json_run_replaces_invalid_utf8_content() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "run",
            "--json",
            "--allow-host-execution",
            "--command",
            "sh -c 'printf \"\\377\"'",
            "hello",
        ])
        .output()
        .expect("run standalone JSON prompt with invalid UTF-8");

    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("single JSON output"),
        serde_json::json!({"content": "\u{fffd}"})
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn standalone_run_selects_reasoning_effort_per_prompt() {
    let workspace = TempDir::new().expect("workspace");
    let configuration = workspace.path().join("profiles.toml");
    fs::write(
        &configuration,
        "schema_version = 1\n\
         [[profiles]]\n\
         name = \"copilot-high\"\n\
         provider = \"copilot\"\n\
         command = \"/bin/echo '{reasoning_effort}'\"\n",
    )
    .expect("write profile");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "run",
            "--allow-host-execution",
            "--profile",
            "copilot-high",
            "--config",
        ])
        .arg(configuration)
        .args(["--reasoning-effort", "high", "hello"])
        .output()
        .expect("run selected effort");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        "high\n"
    );
}

#[test]
fn model_json_rejects_separate_reasoning_presentation() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "model",
            "--provider",
            "ollama",
            "--endpoint",
            "http://127.0.0.1:1",
            "--model",
            "test-model",
            "--json",
            "--show-reasoning",
            "hello",
        ])
        .output()
        .expect("parse conflicting model presentation flags");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("cannot be used with")
    );
}

#[test]
fn setup_uses_fixed_prompt_and_refuses_failed_qualification() {
    let workspace = TempDir::new().expect("workspace");
    let configuration = workspace.path().join("profiles.toml");
    let recorded_prompt = workspace.path().join("prompt.txt");
    let provider = workspace.path().join("provider.sh");
    fs::write(
        &provider,
        format!(
            "#!/bin/sh\n[ \"$1\" = --prompt ] || exit 8\nprintf '%s' \"$2\" > '{}'\nexit 7\n",
            recorded_prompt.display()
        ),
    )
    .expect("write provider");
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mut setup = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .args(["setup", "--config"])
        .arg(&configuration)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start setup");
    use std::io::Write;
    write!(
        setup.stdin.take().expect("setup stdin"),
        "6\nprovider.sh\nfailed\n\n"
    )
    .expect("write setup answers");
    let output = setup.wait_with_output().expect("wait for setup");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 setup output");

    assert!(!output.status.success());
    assert!(!configuration.exists());
    assert_eq!(
        fs::read_to_string(recorded_prompt).expect("recorded prompt"),
        "Reply with exactly: OK"
    );
    assert!(!stdout.contains("Test prompt"));
    assert!(!stdout.contains("full host permissions?"));
    assert!(!stdout.contains("Save profile anyway?"));
}

#[test]
fn setup_force_persists_profile_after_failed_qualification() {
    let workspace = TempDir::new().expect("workspace");
    let configuration = workspace.path().join("profiles.toml");
    let provider = workspace.path().join("provider.sh");
    fs::write(&provider, "#!/bin/sh\nexit 7\n").expect("write provider");
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mut setup = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .args(["setup", "--force", "--config"])
        .arg(&configuration)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start forced setup");
    use std::io::Write;
    write!(
        setup.stdin.take().expect("setup stdin"),
        "6\nprovider.sh\nforced\n\n"
    )
    .expect("write setup answers");
    let output = setup.wait_with_output().expect("wait for setup");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        load_profile(&configuration, "forced")
            .expect("forced profile")
            .name,
        "forced"
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 setup output");
    assert!(stdout.contains("--force"));
    assert!(!stdout.contains("Save profile anyway?"));
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
        .args(["setup", "--force", "--config"])
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
        ("5\n1\ngemini-auto\n\n", "gemini-auto"),
        ("4\n2\ngpt-5.4\ncopilot-pro\n\n", "copilot-pro"),
    ] {
        let mut setup = Command::new(env!("CARGO_BIN_EXE_agent-run"))
            .current_dir(workspace.path())
            .env("PATH", &bin)
            .args(["setup", "--force", "--config"])
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
            assert!(profile.command.contains("auto"));
            assert!(!profile.command.contains("--files"));
        } else {
            assert_eq!(profile.provider, "copilot");
            assert!(profile.command.starts_with("\"copilot\" --prompt"));
            assert!(profile.command.contains("--silent"));
            assert!(profile.command.contains("--allow-all-tools"));
            assert!(profile.command.contains("--no-ask-user"));
            assert!(profile.command.contains("--reasoning-effort"));
            assert!(profile.command.contains("{reasoning_effort}"));
            assert!(profile.command.contains("--model"));
            assert!(profile.command.contains("gpt-5.4"));
            assert!(!profile.command.contains("copilot chat"));
        }
    }
}

#[test]
fn setup_uses_the_acp_current_model_as_the_numbered_default() {
    let workspace = TempDir::new().expect("workspace");
    let bin = workspace.path().join("bin");
    fs::create_dir(&bin).expect("create bin");
    let gemini = bin.join("gemini");
    fs::write(
        &gemini,
        r#"#!/bin/sh
if [ "$1" = --version ]; then
  printf '1.0\n'
  exit 0
fi
if [ "$1" = --acp ]; then
  IFS= read -r initialize || exit 21
  printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1}}'
  IFS= read -r session || exit 22
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"models":{"availableModels":[{"modelId":"first","name":"First"},{"modelId":"current","name":"Current"}],"currentModelId":"current"}}}'
  exec /bin/sleep 30
fi
exit 0
"#,
    )
    .expect("write Gemini provider");
    fs::set_permissions(&gemini, fs::Permissions::from_mode(0o700))
        .expect("make Gemini executable");
    let configuration = workspace.path().join("profiles.toml");

    let mut setup = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .current_dir(workspace.path())
        .env("PATH", &bin)
        .args(["setup", "--config"])
        .arg(&configuration)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start setup");
    use std::io::Write;
    setup
        .stdin
        .take()
        .expect("setup stdin")
        .write_all(b"5\n\n\n\n")
        .expect("select current ACP model and defaults");
    let output = setup.wait_with_output().expect("wait for setup");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let profile = load_profile(&configuration, "gemini").expect("load Gemini profile");
    assert!(profile.command.contains("--model \"current\""));
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 setup output");
    assert!(stdout.contains("2) current"));
    assert!(stdout.contains("3) Other model ID..."));
    assert!(stdout.contains("no-prompt ACP model discovery"));
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
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec /bin/sleep 30\n",
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

#[test]
fn standalone_cli_replays_trajectory_journal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("session.jsonl");
    let content = r#"{"event":"session_start","session_id":"test-session-1","task_id":"my-task","timestamp":1700000000}
{"event":"turn_start","turn":1,"timestamp":1700000001}
{"event":"tool_dispatch","turn":1,"call_id":"c1","tool":"read_file","args":{"path":"src/main.rs"},"action_hash":"sha256:abc"}
{"event":"tool_result","turn":1,"call_id":"c1","tool":"read_file","stdout":"fn main() {}","stderr":"","exit_code":0,"bytes":12,"state_mutated":false}
{"event":"turn_finish","turn":1,"finish_reason":"stop"}
{"event":"session_finish","session_id":"test-session-1","task_id":"my-task","total_turns":1,"total_tokens":42,"success":true}
"#;
    fs::write(&file, content).expect("write session file");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args(["replay", file.to_str().unwrap()])
        .output()
        .expect("run agent-run replay");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Replaying session `test-session-1` for task `my-task`"));
    assert!(stdout.contains("[Turn 1 ToolDispatch] read_file"));
    assert!(stdout.contains("[SessionFinish] turns=1 tokens=42 success=true"));
}

#[test]
fn standalone_cli_replays_trajectory_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("session.jsonl");
    let content = r#"{"event":"session_start","session_id":"json-sess","task_id":"task-2","timestamp":1700000000}
{"event":"session_finish","session_id":"json-sess","task_id":"task-2","total_turns":0,"total_tokens":0,"success":true}
"#;
    fs::write(&file, content).expect("write session file");

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args(["replay", "--json", file.to_str().unwrap()])
        .output()
        .expect("run agent-run replay json");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json output");
    assert_eq!(parsed["session_id"], "json-sess");
    assert_eq!(parsed["final_success"], true);
}
