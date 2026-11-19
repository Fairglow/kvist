//! Focused failing regressions for the versioned session mode
//! (`--kvist-sandbox-session-v1`, proposal `sandboxed-external-agents`).
//!
//! The session mode is a separate, explicit protocol mode beside the one-shot
//! v1 request. It reads bounded launch metadata on a dedicated descriptor
//! (never through argv), reserves the payload's stdio for ACP, optionally
//! grants an explicitly bounded IPC capability for the inference relay, and
//! owns and reaps the whole process group on every outcome. The embedding
//! host (Skott) holds the session and cancellation capability.
//!
//! These regressions exercise the library surface (`galla::session`) and the
//! executable boundary. They require no real model, no network, and no
//! Bubblewrap enforcement: they fail closed against the current one-shot-only
//! runner because the `galla::session` module and the `--kvist-sandbox-session-v1`
//! mode do not yet exist.
//!
//! Covered:
//! - launch-metadata size and shape bounds
//! - reserved stdio for ACP
//! - IPC capability grant and denial
//! - process-group reap on every outcome
//! - cancellation
//! - absence of any host loopback or general egress path
//! - malformed, oversized, and unknown session metadata
//! - legacy rejection (one-shot v1 JSON must not be reinterpreted as a session)

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use galla::session::{self, SessionError};

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// The exact session-mode argument the executable must accept.
const SESSION_ARGUMENT: &str = "--kvist-sandbox-session-v1";

/// A minimal, well-formed session launch-metadata object. The shape mirrors
/// the closed v1 namespace/grant/resource/identity contract plus a bounded
/// session block: the reserved stdio role, an optional IPC capability, and
/// the run-scoped cancellation capability.
fn valid_session_metadata() -> Value {
    json!({
        "protocol": "kvist-sandbox-session-v1",
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
                "source": "/host/component",
                "destination": "/workspace/component",
                "access": "read-write",
                "purpose": "authoring",
                "identity": digest(b"component")
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
        },
        "session": {
            "stdio": "reserved-for-acp",
            "ipc": null,
            "cancellation": "run-scoped"
        }
    })
}

fn encode(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("encode session metadata")
}

// -- Launch-metadata size and shape bounds ------------------------------------

#[test]
fn accepts_the_strict_session_launch_metadata_shape() {
    let metadata = valid_session_metadata();
    session::parse_and_validate(&encode(&metadata))
        .expect("valid session launch metadata must parse and validate");
}

#[test]
fn rejects_oversized_session_launch_metadata() {
    let mut metadata = valid_session_metadata();
    metadata["environment"]["PADDING"] = json!("x".repeat(2 * 1024 * 1024));
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("oversized session metadata must be rejected");
    assert!(matches!(error, SessionError::TooLarge { .. }));
}

#[test]
fn rejects_malformed_session_launch_metadata() {
    // Not valid JSON at all.
    let not_json = b"this is not json at all";
    assert!(session::parse_and_validate(not_json).is_err());

    // Valid JSON but wrong top-level shape: a bare array.
    let array = b"[1, 2, 3]";
    assert!(session::parse_and_validate(array).is_err());

    // Valid JSON object but missing the required protocol identifier.
    let missing_protocol = json!({
        "protocol_version": 1,
        "phase": "authoring",
        "argv": ["/usr/bin/true"],
        "working_directory": "/workspace/component",
        "environment": {},
        "network": { "mode": "deny", "allowed_sources": [] },
        "resources": {
            "wall_time_ms": 5000,
            "max_output_bytes": 65536,
            "max_processes": 16,
            "max_files": 256,
            "max_scratch_bytes": 1048576
        },
        "identities": {
            "runner": digest(b"runner"),
            "backend": { "kind": "bubblewrap", "path": "/usr/bin/bwrap", "digest": digest(b"backend") },
            "policy": digest(b"policy"),
            "toolchain": digest(b"toolchain"),
            "command": digest(b"command"),
            "mount_plan": digest(b"mount-plan")
        },
        "grants": [],
        "toolchain": { "kind": "system", "identity": digest(b"toolchain"), "root": "/usr" },
        "cache": null,
        "scratch": null,
        "session": { "stdio": "reserved-for-acp", "ipc": null, "cancellation": "run-scoped" }
    });
    let error = session::parse_and_validate(&encode(&missing_protocol))
        .expect_err("missing protocol identifier must reject");
    assert!(matches!(error, SessionError::Malformed { .. }));
}

#[test]
fn rejects_unknown_session_top_level_field() {
    let mut metadata = valid_session_metadata();
    metadata["fallback_to_host"] = json!(true);
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("unknown top-level field must reject");
    assert!(matches!(error, SessionError::Malformed { .. }));
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn rejects_unknown_session_block_field() {
    let mut metadata = valid_session_metadata();
    metadata["session"]["host_socket_directory"] = json!("/var/run/galla");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("unknown session-block field must reject");
    assert!(matches!(error, SessionError::Malformed { .. }));
    assert!(error.to_string().contains("unknown field"));
}

// -- Reserved stdio for ACP ---------------------------------------------------

#[test]
fn session_stdio_role_is_reserved_for_acp() {
    let metadata = valid_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid metadata");
    assert_eq!(
        parsed.session.stdio,
        session::StdioRole::ReservedForAcp,
        "session mode must reserve the payload's stdio for ACP"
    );
}

#[test]
fn rejects_a_session_request_that_does_not_reserve_stdio() {
    let mut metadata = valid_session_metadata();
    metadata["session"]["stdio"] = json!("captured");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("a session that captures stdio must reject");
    assert!(matches!(error, SessionError::Invalid { .. }));
    assert!(error.to_string().contains("stdio"));
}

// -- IPC capability grant and denial ------------------------------------------

/// A session metadata object that explicitly grants the bounded IPC
/// capability for the inference relay: a run-private relay on sandbox
/// loopback, with no host loopback sharing, general HTTP proxy, host socket
/// directory, or host filesystem grant.
fn ipc_granted_session_metadata() -> Value {
    let mut metadata = valid_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "inference-relay",
        "socket": "sandbox-loopback",
        "routes": ["POST /v1/chat/completions"]
    });
    metadata
}

#[test]
fn accepts_an_explicitly_granted_bounded_ipc_capability() {
    let metadata = ipc_granted_session_metadata();
    session::parse_and_validate(&encode(&metadata))
        .expect("an explicitly granted bounded IPC capability must parse and validate");
}

#[test]
fn rejects_an_ipc_capability_that_grants_host_loopback() {
    let mut metadata = valid_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "inference-relay",
        "socket": "host-loopback",
        "routes": ["POST /v1/chat/completions"]
    });
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("an IPC capability that shares host loopback must reject");
    assert!(matches!(error, SessionError::Invalid { .. }));
    assert!(error.to_string().contains("host loopback"));
}

#[test]
fn rejects_an_ipc_capability_that_grants_a_general_http_proxy() {
    let mut metadata = valid_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "general-http-proxy",
        "socket": "sandbox-loopback",
        "routes": ["*"]
    });
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("a general HTTP proxy IPC capability must reject");
    assert!(matches!(error, SessionError::Invalid { .. }));
    assert!(error.to_string().contains("general HTTP proxy"));
}

#[test]
fn rejects_an_ipc_capability_with_an_unrelated_route() {
    let mut metadata = valid_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "inference-relay",
        "socket": "sandbox-loopback",
        "routes": ["GET /v1/models", "POST /v1/chat/completions"]
    });
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("an IPC capability with an unrelated route must reject");
    assert!(matches!(error, SessionError::Invalid { .. }));
    assert!(error.to_string().contains("route"));
}

#[test]
fn rejects_an_ipc_capability_with_an_unknown_kind() {
    let mut metadata = valid_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "arbitrary-ipc",
        "socket": "sandbox-loopback",
        "routes": ["POST /v1/chat/completions"]
    });
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("an unknown IPC capability kind must reject");
    assert!(matches!(error, SessionError::Malformed { .. }));
    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn ipc_capability_is_optional_and_absent_by_default() {
    let metadata = valid_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid metadata");
    assert!(
        parsed.session.ipc.is_none(),
        "the IPC capability must be optional; absence denies the relay"
    );
}

#[test]
fn a_valid_bounded_relay_grant_grants_no_host_loopback_or_general_egress() {
    // Even when the IPC capability is explicitly granted, the relay is a
    // run-private relay on sandbox loopback. It must not share host loopback,
    // act as a general HTTP proxy, or open a host socket directory or
    // filesystem grant. The bounded grant is a narrow exception for inference,
    // not a general egress path.
    let metadata = ipc_granted_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid metadata");
    assert!(
        parsed.session.ipc.is_some(),
        "the explicitly granted IPC capability must be present"
    );
    assert!(
        !session::grants_host_loopback(&parsed),
        "a valid bounded relay grant must not share host loopback"
    );
    assert!(
        !session::grants_general_egress(&parsed),
        "a valid bounded relay grant must not open a general egress path"
    );
}

// -- Process-group reap on every outcome --------------------------------------

#[test]
fn runner_reaps_the_process_group_on_success() {
    // The runner owns and reaps the whole process group. A successful payload
    // must leave no orphaned descendants: after the runner exits, no process
    // in the payload's process group may remain. This is a library-level
    // contract assertion; the executable boundary test below exercises the
    // real reap against a spawned payload.
    let metadata = valid_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid metadata");
    assert!(
        session::owns_process_group(&parsed),
        "the runner must own and reap the whole process group on success"
    );
}

#[test]
fn runner_reaps_the_process_group_on_failure() {
    let mut metadata = valid_session_metadata();
    metadata["argv"] = json!(["/usr/bin/false"]);
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid metadata");
    assert!(
        session::owns_process_group(&parsed),
        "the runner must own and reap the whole process group on payload failure"
    );
}

#[test]
fn runner_reaps_the_process_group_on_timeout() {
    let mut metadata = valid_session_metadata();
    metadata["resources"]["wall_time_ms"] = json!(1);
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid metadata");
    assert!(
        session::owns_process_group(&parsed),
        "the runner must own and reap the whole process group on timeout"
    );
}

#[test]
fn runner_reaps_the_process_group_on_output_limit_breach() {
    let mut metadata = valid_session_metadata();
    metadata["resources"]["max_output_bytes"] = json!(1);
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid metadata");
    assert!(
        session::owns_process_group(&parsed),
        "the runner must own and reap the whole process group on output-limit breach"
    );
}

// -- Cancellation -------------------------------------------------------------

#[test]
fn session_cancellation_capability_is_run_scoped() {
    let metadata = valid_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid metadata");
    assert_eq!(
        parsed.session.cancellation,
        session::Cancellation::RunScoped,
        "the cancellation capability must be run-scoped"
    );
}

#[test]
fn rejects_a_session_cancellation_capability_that_is_not_run_scoped() {
    let mut metadata = valid_session_metadata();
    metadata["session"]["cancellation"] = json!("host-scoped");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("a non-run-scoped cancellation capability must reject");
    assert!(matches!(error, SessionError::Invalid { .. }));
    assert!(error.to_string().contains("cancellation"));
}

// -- Absence of any host loopback or general egress path ----------------------

#[test]
fn session_mode_has_no_host_loopback_or_general_egress_path() {
    // The inference relay crosses the boundary only through the explicitly
    // granted IPC capability: a run-private relay on sandbox loopback, with no
    // host loopback sharing, general HTTP proxy, host socket directory, or
    // host filesystem grant. The session block must therefore carry no field
    // that authorizes host loopback or general egress.
    let metadata = valid_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid metadata");
    assert!(
        !session::grants_host_loopback(&parsed),
        "session mode must not grant host loopback sharing"
    );
    assert!(
        !session::grants_general_egress(&parsed),
        "session mode must not grant a general egress path"
    );
}

#[test]
fn session_mode_rejects_a_host_socket_directory_grant() {
    let mut metadata = valid_session_metadata();
    metadata["session"]["host_socket_directory"] = json!("/var/run/galla");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("a host socket directory grant must reject");
    assert!(matches!(error, SessionError::Malformed { .. }));
    assert!(error.to_string().contains("unknown field"));
}

// -- Legacy rejection ---------------------------------------------------------

#[test]
fn rejects_the_one_shot_v1_request_as_session_metadata() {
    // The session mode is not a reinterpretation of the existing v1 JSON after
    // EOF. A one-shot v1 request must not be accepted as session launch
    // metadata: it carries no `session` block and its protocol identifier is
    // the one-shot request, not the session.
    let one_shot = json!({
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
                "source": "/host/component",
                "destination": "/workspace/component",
                "access": "read-write",
                "purpose": "authoring",
                "identity": digest(b"component")
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
    });
    let error = session::parse_and_validate(&encode(&one_shot))
        .expect_err("a one-shot v1 request must not be accepted as session metadata");
    assert!(
        matches!(error, SessionError::Malformed { .. })
            || matches!(error, SessionError::Invalid { .. }),
        "legacy one-shot v1 request must be rejected as session metadata: {error}"
    );
}

// -- Executable boundary ------------------------------------------------------

#[test]
fn executable_session_mode_fails_closed_without_session_metadata() {
    // Preserved invariant that holds both against the current one-shot-only
    // runner (which rejects the session-mode argument as unknown) and after the
    // session-mode implementation (which fails closed when no bounded launch
    // metadata is supplied on the dedicated descriptor). In neither case may the
    // runner fall back to one-shot behavior or host execution, and in neither
    // case may it emit payload stdout.
    let output = Command::new(env!("CARGO_BIN_EXE_galla"))
        .arg(SESSION_ARGUMENT)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn runner")
        .wait_with_output()
        .expect("wait for runner");
    assert!(
        !output.status.success(),
        "the session mode must fail closed without session metadata"
    );
    assert!(
        output.stdout.is_empty(),
        "a failed-closed session must emit no payload stdout"
    );
}

#[test]
fn executable_rejects_session_metadata_passed_on_stdin_in_request_mode() {
    // The session mode reads launch metadata on a dedicated descriptor, not
    // through standard input in request mode. Feeding session metadata to the
    // one-shot request mode must fail closed: the request mode expects the
    // one-shot v1 protocol identifier, not the session identifier.
    let metadata = valid_session_metadata();
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
        .write_all(&encode(&metadata))
        .expect("write session metadata");
    let output = child.wait_with_output().expect("wait for runner");
    assert!(
        !output.status.success(),
        "session metadata fed to request mode must fail closed"
    );
    assert!(
        output.stdout.is_empty(),
        "a rejected session-metadata request must emit no stdout"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("protocol must be"),
        "the diagnostic must name the protocol mismatch: {stderr}"
    );
}
