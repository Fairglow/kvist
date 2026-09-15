//! Per-process-env isolation tests for global configuration resolution.
//!
//! `global_user_config_path()` resolves purely from the process environment.
//! The real binary echoes the resolved path in its `agent list` output, so a
//! child process with an explicit `KVIST_CONFIG_PATH` lets the test assert
//! both resolution and that the resolved file is actually loaded.
//!
//! Resolution is a pure function of the process environment. The only correct
//! way to exercise it from a test is to run the real binary with an explicit
//! environment: `std::env::set_var` is `unsafe` on edition 2024 and racy under
//! the parallel test runner, so isolation happens on a child process, never in
//! the shared test process.

use std::process::Command;

/// With `KVIST_CONFIG_PATH` pointed at an injected user config, `agent list`
/// resolves it end-to-end and reports both the resolved path and its profiles.
#[test]
fn resolves_global_user_config_via_kvist_config_path() {
    let sandbox = tempfile::tempdir().expect("tempdir");
    let project = sandbox.path().join("project");
    let config_file = sandbox.path().join("injected-user-config.toml");
    std::fs::create_dir_all(&project).expect("create project dir");
    std::fs::write(
        &config_file,
        "# Injected user configuration for the resolution test\n\
         [agent.profiles.integration-profile]\n\
         provider = \"custom\"\n\
         model = \"integration-model\"\n",
    )
    .expect("write injected user config");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(["agent", "list"])
        .current_dir(&project)
        .env("NO_COLOR", "1")
        // Point resolution exclusively at the injected config so neither the
        // real user config nor any XDG/HOME fallback can influence the result.
        .env("KVIST_CONFIG_PATH", &config_file)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("KVIST_ALLOW_USER_CONFIG")
        .env("HOME", sandbox.path())
        .output()
        .expect("run kvist agent list");

    assert!(
        output.status.success(),
        "agent list should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The resolved path is echoed, proving `global_user_config_path()` honored
    // `KVIST_CONFIG_PATH`.
    assert!(
        stdout.contains(config_file.to_str().expect("config path is valid unicode")),
        "expected the resolved path in output: {stdout}"
    );
    assert!(
        stdout.contains("integration-profile"),
        "expected the injected profile in output: {stdout}"
    );
}
