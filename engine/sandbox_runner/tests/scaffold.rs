use std::process::Command;

#[test]
fn scaffold_does_not_claim_runner_support() {
    assert!(kvist_sandbox_runner::IMPLEMENTATION_STATUS.contains("not implemented"));

    let output = Command::new(env!("CARGO_BIN_EXE_kvist-sandbox-runner"))
        .arg("--kvist-sandbox-probe-v1")
        .output()
        .expect("run scaffold");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not implemented"));
}
