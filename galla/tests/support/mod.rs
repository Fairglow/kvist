use std::process::Command;

pub fn probe() -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_galla"))
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .expect("run Galla probe");
    assert!(
        output.status.success(),
        "Galla probe failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("successful Galla probe must emit JSON")
}
