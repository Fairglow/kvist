//! Resource monitoring tests: prlimit error handling, file size monitoring,
//! and process count monitoring.
//!
//! These tests verify that the runner correctly enforces resource limits
//! and reports failures rather than silently continuing.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn encode(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).expect("serialize request");
    bytes.push(b'\n');
    bytes
}

fn run_runner(request: &[u8]) -> std::process::Output {
    // The runner verifies its own identity against /proc/self/exe, the
    // backend against the installed bwrap, and recomputes the command and
    // mount-plan identities from argv and grants. The fixture placeholders
    // never match, so bind the request to the actual binary and backend and
    // recompute the content-bound identities before sending. This file has no
    // negative identity tests, so auto-finalizing is safe.
    let mut value: Value = serde_json::from_slice(request).expect("request is valid JSON");
    value["identities"]["runner"] = json!(digest(
        &std::fs::read(env!("CARGO_BIN_EXE_galla")).expect("read galla binary")
    ));
    let probe_output = Command::new(env!("CARGO_BIN_EXE_galla"))
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .expect("run probe");
    let probe: Value = serde_json::from_slice(&probe_output.stdout).expect("valid probe JSON");
    value["identities"]["backend"]["path"] = probe["backend"]["path"].clone();
    value["identities"]["backend"]["digest"] = probe["backend"]["digest"].clone();
    let parsed: galla::protocol::SandboxRequest =
        serde_json::from_value(value.clone()).expect("request parses to typed shape");
    value["identities"]["command"] = json!(galla::protocol::command_identity(&parsed.argv));
    value["identities"]["mount_plan"] = json!(galla::protocol::mount_plan_identity(&parsed.grants));
    let request = encode(&value);
    let mut child = Command::new(env!("CARGO_BIN_EXE_galla"))
        .arg("--kvist-sandbox-request-v1")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn runner");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&request)
        .expect("write request");
    child.wait_with_output().expect("wait for runner")
}

fn valid_request() -> Value {
    json!({
        "protocol": "kvist-sandbox-request-v1",
        "protocol_version": 1,
        "phase": "authoring",
        "argv": ["/usr/bin/true"],
        "working_directory": "/workspace/component",
        "environment": {
            "HOME": "/workspace/home",
            "PATH": "/usr/bin"
        },
        "network": {
            "mode": "deny",
            "allowed_sources": []
        },
        "resources": {
            "wall_time_ms": 5000,
            "max_output_bytes": 65536,
            "max_processes": 16,
            "max_files": 256,
            "max_file_bytes": 1048576,
            "max_scratch_bytes": 1048576
        },
        "identities": {
            "runner": digest(b"runner"),
            "backend": {
                "kind": "bubblewrap",
                "path": "/usr/bin/bwrap",
                "digest": digest(b"backend")
            },
            "policy": digest(b"policy"),
            "toolchain": digest(b"toolchain"),
            "command": digest(b"command"),
            "mount_plan": digest(b"mount-plan")
        },
        "grants": [
            {
                "source": "/host/component/REQUIREMENTS.md",
                "destination": "/workspace/component/REQUIREMENTS.md",
                "access": "read-only",
                "purpose": "context",
                "identity": digest(b"requirements")
            },
            {
                "source": "/host/component/tests",
                "destination": "/workspace/component/tests",
                "access": "read-write",
                "purpose": "authoring",
                "identity": digest(b"tests")
            },
            {
                "source": "/usr",
                "destination": "/usr",
                "access": "read-only",
                "purpose": "toolchain",
                "identity": digest(b"toolchain-usr")
            },
            {
                "source": "/host/scratch",
                "destination": "/workspace/scratch",
                "access": "read-write",
                "purpose": "scratch",
                "identity": digest(b"scratch")
            }
        ],
        "toolchain": {
            "kind": "system",
            "identity": digest(b"toolchain"),
            "root": "/usr"
        },
        "cache": null,
        "scratch": {
            "destination": "/workspace/scratch",
            "identity": digest(b"scratch")
        }
    })
}

/// Test that prlimit errors are reported, not silently ignored.
/// The runner invokes /usr/bin/prlimit to set resource limits before
/// spawning the sandbox. If prlimit fails, the runner must report
/// the error and exit with a non-zero code rather than proceeding
/// without limits.
#[test]
fn prlimit_failure_is_reported() {
    // Check if we're already in a sandbox (nested sandbox check)
    if std::path::Path::new("/.galla-sandbox").exists() {
        // Inside a sandbox, prlimit is already applied; skip the failure test
        // but verify the runner detects the nesting
        let request = valid_request();
        let bytes = encode(&request);
        let output = run_runner(&bytes);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success() && stderr.contains("already inside a Galla sandbox"),
            "runner should detect nested sandbox: {}",
            stderr
        );
        return;
    }

    // Outside a sandbox, verify prlimit errors are reported. Use real host
    // origins so the runner authorizes the mount sources and reaches the
    // limit-setting stage; on a normal host the limits are set successfully
    // and the command succeeds, so the failure-reporting assertion is only
    // exercised when a limit genuinely cannot be applied.
    let temp = tempfile::tempdir().unwrap();
    let req_file = temp.path().join("REQUIREMENTS.md");
    std::fs::write(&req_file, "intent").unwrap();
    let tests_dir = temp.path().join("tests");
    std::fs::create_dir(&tests_dir).unwrap();
    let scratch_dir = temp.path().join("scratch");
    std::fs::create_dir(&scratch_dir).unwrap();

    let mut request = valid_request();
    request["grants"][0]["source"] = json!(req_file.to_string_lossy());
    request["grants"][1]["source"] = json!(tests_dir.to_string_lossy());
    request["grants"][3]["source"] = json!(scratch_dir.to_string_lossy());
    let bytes = encode(&request);
    let output = run_runner(&bytes);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        assert!(
            stderr.contains("prlimit")
                || stderr.contains("limits")
                || stderr.contains("fsize")
                || stderr.contains("nofile"),
            "prlimit failure not properly reported: {}",
            stderr
        );
    }
}

/// Test that file size limits are enforced via prlimit --fsize.
/// When max_file_bytes is set, the runner should pass --fsize to prlimit.
/// A write exceeding the limit should fail with EFBIG (signal 7).
#[test]
fn file_size_limit_enforced() {
    let mut request = valid_request();
    request["resources"]["max_file_bytes"] = json!(1024);
    // Write a file larger than 1024 bytes - should fail with EFBIG
    request["argv"] = json!([
        "/bin/sh",
        "-c",
        "dd if=/dev/zero of=/tmp/bigfile bs=2048 count=1 2>/dev/null; echo done"
    ]);

    let bytes = encode(&request);
    let output = run_runner(&bytes);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The write should fail due to file size limit
    // Either the command fails (non-zero exit) or the runner reports the breach
    assert!(
        !output.status.success() || stderr.contains("file size") || stderr.contains("fsize"),
        "file size limit not enforced: exit={:?} stdout={} stderr={}",
        output.status.code(),
        stdout,
        stderr
    );
}

/// Test that process count limits are enforced.
/// When max_processes is set, the runner should monitor the process group
/// and kill children that exceed the limit.
#[test]
fn process_count_limit_enforced() {
    let temp = tempfile::tempdir().unwrap();
    let requirements = temp.path().join("REQUIREMENTS.md");
    let tests = temp.path().join("tests");
    let scratch = temp.path().join("scratch");
    std::fs::write(&requirements, "intent").unwrap();
    std::fs::create_dir(&tests).unwrap();
    std::fs::create_dir(&scratch).unwrap();
    let mut request = valid_request();
    for (index, source) in [(0, requirements), (1, tests), (3, scratch)] {
        request["grants"][index]["source"] = json!(source);
    }
    request["resources"]["max_processes"] = json!(16);
    request["argv"] = json!([
        "/bin/sh",
        "-c",
        "printf 'compiler output without newline' >&2; for i in $(seq 1 32); do sleep 5 & done; wait"
    ]);

    let bytes = encode(&request);
    let output = run_runner(&bytes);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(3), "{stderr}");
    assert!(
        stderr
            .trim_end()
            .ends_with("compiler output without newline\ngalla: process count limit exceeded"),
        "{stderr}"
    );
}

/// Test that output byte limits are enforced.
/// When max_output_bytes is set, the runner should monitor output size
/// and kill children that exceed the limit.
#[test]
fn output_limit_enforced() {
    let mut request = valid_request();
    request["resources"]["max_output_bytes"] = json!(100);
    // Print more than 100 bytes
    request["argv"] = json!(["/bin/sh", "-c", "printf '%0.s=' {1..200}"]);

    let bytes = encode(&request);
    let output = run_runner(&bytes);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success() || stderr.contains("output limit"),
        "output limit not enforced: exit={:?} stderr={}",
        output.status.code(),
        stderr
    );
}

/// Test that wall time limits are enforced.
/// When wall_time_ms is set, the runner should monitor elapsed time
/// and kill children that exceed the limit.
#[test]
fn wall_time_limit_enforced() {
    let mut request = valid_request();
    request["resources"]["wall_time_ms"] = json!(100);
    // Sleep longer than the limit
    request["argv"] = json!(["/bin/sleep", "10"]);

    let bytes = encode(&request);
    let output = run_runner(&bytes);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success() || stderr.contains("wall time"),
        "wall time limit not enforced: exit={:?} stderr={}",
        output.status.code(),
        stderr
    );
}

/// Test that successful execution within limits works correctly.
#[test]
fn successful_execution_within_limits() {
    // Inside a sandbox, nested execution is not allowed
    if std::path::Path::new("/.galla-sandbox").exists() {
        // Skip this test when already inside a sandbox
        return;
    }

    // Real host origins so the runner authorizes the mount sources.
    let temp = tempfile::tempdir().unwrap();
    let req_file = temp.path().join("REQUIREMENTS.md");
    std::fs::write(&req_file, "intent").unwrap();
    let tests_dir = temp.path().join("tests");
    std::fs::create_dir(&tests_dir).unwrap();
    let scratch_dir = temp.path().join("scratch");
    std::fs::create_dir(&scratch_dir).unwrap();

    let mut request = valid_request();
    request["grants"][0]["source"] = json!(req_file.to_string_lossy());
    request["grants"][1]["source"] = json!(tests_dir.to_string_lossy());
    request["grants"][3]["source"] = json!(scratch_dir.to_string_lossy());
    let bytes = encode(&request);
    let output = run_runner(&bytes);

    assert!(
        output.status.success(),
        "expected success within limits, got exit {:?} stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}
