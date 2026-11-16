//! Tests verifying JSON schema completeness and runtime identity verification.
//!
//! The security audit found two gaps:
//! 1. The schema must enumerate every purpose value the parser accepts.
//! 2. Identities are validated as digest shape but not verified at runtime.
//!
//! These tests assert both properties. The identity verification tests will
//! fail until the runtime actually computes and compares identities.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use galla::validation;

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn registry_identity(name: &str, index: &str, download: &str) -> String {
    validation::registry_identity(name, index, download)
}

fn crates_io() -> Value {
    let index = "https://index.crates.io/";
    let download = "https://static.crates.io/";
    json!({
        "kind": "cargo-registry",
        "name": "crates-io",
        "index_origin": index,
        "download_origin": download,
        "identity": registry_identity("crates-io", index, download)
    })
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
            "runner": digest_bytes(b"runner"),
            "backend": {
                "kind": "bubblewrap",
                "path": "/usr/bin/bwrap",
                "digest": digest_bytes(b"backend")
            },
            "policy": digest_bytes(b"policy"),
            "toolchain": digest_bytes(b"toolchain"),
            "command": digest_bytes(b"command"),
            "mount_plan": digest_bytes(b"mount-plan")
        },
        "grants": [
            {
                "source": "/host/component/REQUIREMENTS.md",
                "destination": "/workspace/component/REQUIREMENTS.md",
                "access": "read-only",
                "purpose": "context",
                "identity": digest_bytes(b"requirements")
            },
            {
                "source": "/host/component/tests",
                "destination": "/workspace/component/tests",
                "access": "read-write",
                "purpose": "authoring",
                "identity": digest_bytes(b"tests")
            },
            {
                "source": "/usr",
                "destination": "/usr",
                "access": "read-only",
                "purpose": "toolchain",
                "identity": digest_bytes(b"toolchain-usr")
            },
            {
                "source": "/host/scratch",
                "destination": "/workspace/scratch",
                "access": "read-write",
                "purpose": "scratch",
                "identity": digest_bytes(b"scratch")
            }
        ],
        "toolchain": {
            "kind": "system",
            "identity": digest_bytes(b"toolchain"),
            "root": "/usr"
        },
        "cache": null,
        "scratch": {
            "destination": "/workspace/scratch",
            "identity": digest_bytes(b"scratch")
        }
    })
}

fn encode(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("encode request")
}

fn run_runner(request: &[u8]) -> std::process::Output {
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
        .write_all(request)
        .expect("write request");
    child.wait_with_output().expect("wait for runner")
}

// -- Schema completeness tests ------------------------------------------------

#[test]
fn schema_enumerates_all_accepted_purpose_values() {
    let schema: Value = serde_json::from_str(include_str!(
        "../schema/kvist-sandbox-request-v1.schema.json"
    ))
    .expect("request schema must be valid JSON");

    let schema_purposes = schema
        .pointer("/$defs/grant/properties/purpose/enum")
        .and_then(Value::as_array)
        .expect("schema must declare purpose enum")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    // The parser accepts these purpose values (from src/protocol.rs Purpose enum).
    // The schema must enumerate every one of them.
    let parser_purposes = [
        "context",
        "authoring",
        "verification",
        "toolchain",
        "scratch",
        "dependency-cache",
        "lockfile",
        "registry",
        "cargo-config",
        "runtime",
    ];

    for purpose in parser_purposes {
        assert!(
            schema_purposes.contains(&purpose),
            "schema omits accepted purpose value `{purpose}`"
        );
    }
}

#[test]
fn schema_enumerates_all_accepted_phase_values() {
    let schema: Value = serde_json::from_str(include_str!(
        "../schema/kvist-sandbox-request-v1.schema.json"
    ))
    .expect("request schema must be valid JSON");

    let schema_phases = schema
        .pointer("/properties/phase/enum")
        .and_then(Value::as_array)
        .expect("schema must declare phase enum")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    let parser_phases = ["authoring", "verification", "dependency-acquisition"];

    for phase in parser_phases {
        assert!(
            schema_phases.contains(&phase),
            "schema omits accepted phase value `{phase}`"
        );
    }
}

#[test]
fn schema_enumerates_all_accepted_network_modes() {
    let schema: Value = serde_json::from_str(include_str!(
        "../schema/kvist-sandbox-request-v1.schema.json"
    ))
    .expect("request schema must be valid JSON");

    let schema_modes = schema
        .pointer("/$defs/network/properties/mode/enum")
        .and_then(Value::as_array)
        .expect("schema must declare network mode enum")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    let parser_modes = ["deny", "package-sources"];

    for mode in parser_modes {
        assert!(
            schema_modes.contains(&mode),
            "schema omits accepted network mode `{mode}`"
        );
    }
}

#[test]
fn schema_enumerates_all_accepted_access_modes() {
    let schema: Value = serde_json::from_str(include_str!(
        "../schema/kvist-sandbox-request-v1.schema.json"
    ))
    .expect("request schema must be valid JSON");

    let schema_access = schema
        .pointer("/$defs/grant/properties/access/enum")
        .and_then(Value::as_array)
        .expect("schema must declare access enum")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    let parser_access = ["read-only", "read-write"];

    for access in parser_access {
        assert!(
            schema_access.contains(&access),
            "schema omits accepted access mode `{access}`"
        );
    }
}

#[test]
fn schema_enumerates_all_accepted_toolchain_kinds() {
    let schema: Value = serde_json::from_str(include_str!(
        "../schema/kvist-sandbox-request-v1.schema.json"
    ))
    .expect("request schema must be valid JSON");

    let variants = schema
        .pointer("/$defs/toolchain/oneOf")
        .and_then(Value::as_array)
        .expect("schema must declare toolchain oneOf");

    let mut found_system = false;
    let mut found_cargo = false;
    for variant in variants {
        let kind = variant
            .pointer("/properties/kind/const")
            .and_then(Value::as_str);
        if kind == Some("system") {
            found_system = true;
        }
        if kind == Some("cargo") {
            found_cargo = true;
        }
    }

    assert!(found_system, "schema omits `system` toolchain kind");
    assert!(found_cargo, "schema omits `cargo` toolchain kind");
}

#[test]
fn schema_enumerates_all_accepted_source_kinds() {
    let schema: Value = serde_json::from_str(include_str!(
        "../schema/kvist-sandbox-request-v1.schema.json"
    ))
    .expect("request schema must be valid JSON");

    let variants = schema
        .pointer("/$defs/allowedSource/oneOf")
        .and_then(Value::as_array)
        .expect("schema must declare allowedSource oneOf");

    let mut found_registry = false;
    let mut found_git = false;
    for variant in variants {
        let kind = variant
            .pointer("/properties/kind/const")
            .and_then(Value::as_str);
        if kind == Some("cargo-registry") {
            found_registry = true;
        }
        if kind == Some("cargo-git") {
            found_git = true;
        }
    }

    assert!(found_registry, "schema omits `cargo-registry` source kind");
    assert!(found_git, "schema omits `cargo-git` source kind");
}

#[test]
fn schema_declares_all_required_top_level_fields() {
    let schema: Value = serde_json::from_str(include_str!(
        "../schema/kvist-sandbox-request-v1.schema.json"
    ))
    .expect("request schema must be valid JSON");

    let required = schema
        .pointer("/required")
        .and_then(Value::as_array)
        .expect("schema must declare required fields")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    // Every field the parser requires must be in the schema's required list.
    let parser_required = [
        "protocol",
        "protocol_version",
        "phase",
        "argv",
        "working_directory",
        "environment",
        "network",
        "resources",
        "identities",
        "grants",
        "toolchain",
        "cache",
        "scratch",
    ];

    for field in parser_required {
        assert!(
            required.contains(&field),
            "schema omits required field `{field}`"
        );
    }
}

#[test]
fn schema_declares_all_identity_fields() {
    let schema: Value = serde_json::from_str(include_str!(
        "../schema/kvist-sandbox-request-v1.schema.json"
    ))
    .expect("request schema must be valid JSON");

    let identities_required = schema
        .pointer("/$defs/identities/required")
        .and_then(Value::as_array)
        .expect("schema must declare identities required fields")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    // The parser requires all six identity fields.
    let parser_identities = [
        "runner",
        "backend",
        "policy",
        "toolchain",
        "command",
        "mount_plan",
    ];

    for field in parser_identities {
        assert!(
            identities_required.contains(&field),
            "schema omits required identity field `{field}`"
        );
    }
}

// -- Runtime identity verification tests --------------------------------------

/// Compute the actual SHA256 digest of the running galla binary.
fn runner_digest() -> String {
    let path = std::env!("CARGO_BIN_EXE_galla");
    let bytes = std::fs::read(path).expect("read galla binary");
    digest_bytes(&bytes)
}

#[test]
fn runtime_verifies_runner_identity() {
    // Skip if inside a sandbox (can't spawn nested runner).
    if std::path::Path::new("/.galla-sandbox").exists() {
        return;
    }

    let probe_output = Command::new(env!("CARGO_BIN_EXE_galla"))
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .expect("run probe");
    assert!(probe_output.status.success());
    let probe: Value = serde_json::from_slice(&probe_output.stdout).expect("valid probe JSON");

    // First, verify with the correct runner identity.
    let mut correct = valid_request();
    correct["identities"]["backend"]["path"] = probe["backend"]["path"].clone();
    correct["identities"]["backend"]["digest"] = probe["backend"]["digest"].clone();
    correct["identities"]["runner"] = json!(runner_digest());

    let output = run_runner(&encode(&correct));
    assert!(
        output.status.success(),
        "request with correct runner identity must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Now verify with an incorrect runner identity.
    let mut wrong = valid_request();
    wrong["identities"]["backend"]["path"] = probe["backend"]["path"].clone();
    wrong["identities"]["backend"]["digest"] = probe["backend"]["digest"].clone();
    wrong["identities"]["runner"] = json!(digest_bytes(b"wrong-runner"));

    let output = run_runner(&encode(&wrong));
    assert!(
        !output.status.success(),
        "request with incorrect runner identity must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("runner") || stderr.contains("identity") || stderr.contains("digest"),
        "diagnostic must mention runner/identity/digest: {stderr}"
    );
}

#[test]
fn runtime_verifies_policy_identity() {
    // Skip if inside a sandbox.
    if std::path::Path::new("/.galla-sandbox").exists() {
        return;
    }

    let probe_output = Command::new(env!("CARGO_BIN_EXE_galla"))
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .expect("run probe");
    assert!(probe_output.status.success());
    let probe: Value = serde_json::from_slice(&probe_output.stdout).expect("valid probe JSON");

    // Use the correct runner identity but wrong policy identity.
    let mut request = valid_request();
    request["identities"]["backend"]["path"] = probe["backend"]["path"].clone();
    request["identities"]["backend"]["digest"] = probe["backend"]["digest"].clone();
    request["identities"]["runner"] = json!(runner_digest());
    request["identities"]["policy"] = json!(digest_bytes(b"wrong-policy"));

    let output = run_runner(&encode(&request));
    // This test will fail until the runtime actually verifies the policy identity.
    assert!(
        !output.status.success(),
        "request with incorrect policy identity must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("policy") || stderr.contains("identity") || stderr.contains("digest"),
        "diagnostic must mention policy/identity/digest: {stderr}"
    );
}

#[test]
fn runtime_verifies_command_identity() {
    // Skip if inside a sandbox.
    if std::path::Path::new("/.galla-sandbox").exists() {
        return;
    }

    let probe_output = Command::new(env!("CARGO_BIN_EXE_galla"))
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .expect("run probe");
    assert!(probe_output.status.success());
    let probe: Value = serde_json::from_slice(&probe_output.stdout).expect("valid probe JSON");

    let mut request = valid_request();
    request["identities"]["backend"]["path"] = probe["backend"]["path"].clone();
    request["identities"]["backend"]["digest"] = probe["backend"]["digest"].clone();
    request["identities"]["runner"] = json!(runner_digest());
    request["identities"]["command"] = json!(digest_bytes(b"wrong-command"));

    let output = run_runner(&encode(&request));
    assert!(
        !output.status.success(),
        "request with incorrect command identity must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("command") || stderr.contains("identity") || stderr.contains("digest"),
        "diagnostic must mention command/identity/digest: {stderr}"
    );
}

#[test]
fn runtime_verifies_mount_plan_identity() {
    // Skip if inside a sandbox.
    if std::path::Path::new("/.galla-sandbox").exists() {
        return;
    }

    let probe_output = Command::new(env!("CARGO_BIN_EXE_galla"))
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .expect("run probe");
    assert!(probe_output.status.success());
    let probe: Value = serde_json::from_slice(&probe_output.stdout).expect("valid probe JSON");

    let mut request = valid_request();
    request["identities"]["backend"]["path"] = probe["backend"]["path"].clone();
    request["identities"]["backend"]["digest"] = probe["backend"]["digest"].clone();
    request["identities"]["runner"] = json!(runner_digest());
    request["identities"]["mount_plan"] = json!(digest_bytes(b"wrong-mount-plan"));

    let output = run_runner(&encode(&request));
    assert!(
        !output.status.success(),
        "request with incorrect mount plan identity must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("mount") || stderr.contains("identity") || stderr.contains("digest"),
        "diagnostic must mention mount/identity/digest: {stderr}"
    );
}
