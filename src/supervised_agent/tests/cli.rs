#![cfg(target_os = "linux")]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use nix::{
    errno::Errno,
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use tempfile::TempDir;

#[test]
fn host_execution_requires_explicit_acknowledgement() {
    let output = Command::new(env!("CARGO_BIN_EXE_supervised-agent"))
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
    let expected = workspace
        .path()
        .join(".config/supervised-agent/config.toml");

    for xdg in ["", "relative-config"] {
        let output = Command::new(env!("CARGO_BIN_EXE_supervised-agent"))
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
fn standalone_cli_runs_a_prompt_from_a_file() {
    let workspace = TempDir::new().expect("workspace");
    let prompt = workspace.path().join("prompt.md");
    fs::write(&prompt, "standalone prompt").expect("write prompt");

    let output = Command::new(env!("CARGO_BIN_EXE_supervised-agent"))
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
fn setup_persists_a_profile_and_run_can_select_it() {
    let workspace = TempDir::new().expect("workspace");
    let configuration = workspace.path().join("profiles.toml");
    let provider = workspace.path().join("provider.sh");
    fs::write(&provider, "#!/bin/sh\nprintf 'profile: %s\\n' \"$*\"\n").expect("write provider");
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mut setup = Command::new(env!("CARGO_BIN_EXE_supervised-agent"))
        .current_dir(workspace.path())
        .args(["setup", "--config", "profiles.toml"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start setup");
    use std::io::Write;
    write!(
        setup.stdin.take().expect("setup stdin"),
        "6\n{}\nstandalone\n\nn\n",
        provider.display()
    )
    .expect("write setup answers");
    let setup_output = setup.wait_with_output().expect("wait for setup");
    assert!(
        setup_output.status.success(),
        "{}",
        String::from_utf8_lossy(&setup_output.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_supervised-agent"))
        .current_dir(workspace.path())
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
fn interrupt_terminates_the_supervised_process_group() {
    let workspace = TempDir::new().expect("workspace");
    let marker = workspace.path().join("provider.pid");
    let provider = workspace.path().join("provider.sh");
    fs::write(
        &provider,
        format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec sleep 30\n",
            marker.display()
        ),
    )
    .expect("write provider");
    fs::set_permissions(&provider, fs::Permissions::from_mode(0o700))
        .expect("make provider executable");

    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_supervised-agent"))
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
