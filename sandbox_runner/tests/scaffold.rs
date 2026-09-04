//! Structural evidence: the crate honestly reports its enforcement status
//! and succeeds for probe invocation.

use std::process::Command;

#[test]
fn status_is_honest_about_enforcement() {
    let status = kvist_sandbox_runner::IMPLEMENTATION_STATUS;
    assert!(status.contains("fully implemented"));
    assert!(status.contains("Bubblewrap"));
}

#[test]
fn probe_invocation_succeeds() {
    let output = Command::new(env!("CARGO_BIN_EXE_kvist-sandbox-runner"))
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .expect("run scaffold probe");

    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
}
