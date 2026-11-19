//! Independent security audit of the session-mode confinement.
//!
//! This audit observes the session-mode launch-metadata bounds, reserved stdio,
//! IPC capability grant/denial, process-group reap, cancellation, and the
//! absence of any host loopback or general egress path. It constructs its own
//! launch-metadata fixtures and verifies the confinement holds, independently
//! of the implementer's self-certification.
//!
//! The observations are scoped for arbitration: each test names the exact
//! security property it verifies and the exact evidence it produces.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use galla::session::{self, SessionError};

/// Computes the SHA-256 digest of a byte slice, formatted as `sha256:<hex>`.
fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// Constructs a minimal, well-formed session launch-metadata object from
/// scratch. This is the audit's own fixture, independent of the implementer's
/// test fixtures.
fn audit_session_metadata() -> Value {
    json!({
        "protocol": "kvist-sandbox-session-v1",
        "protocol_version": 1,
        "phase": "authoring",
        "argv": ["/usr/bin/true"],
        "working_directory": "/workspace/audit",
        "environment": {
            "HOME": "/workspace/audit-home",
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
            "runner": digest(b"audit-runner"),
            "backend": {
                "kind": "bubblewrap",
                "path": "/usr/bin/bwrap",
                "digest": digest(b"audit-backend")
            },
            "policy": digest(b"audit-policy"),
            "toolchain": digest(b"audit-toolchain"),
            "command": digest(b"audit-command"),
            "mount_plan": digest(b"audit-mount-plan")
        },
        "grants": [
            {
                "source": "/host/audit-component",
                "destination": "/workspace/audit-component",
                "access": "read-write",
                "purpose": "authoring",
                "identity": digest(b"audit-component")
            },
            {
                "source": "/usr",
                "destination": "/usr",
                "access": "read-only",
                "purpose": "toolchain",
                "identity": digest(b"audit-toolchain-usr")
            },
            {
                "source": "/host/audit-scratch",
                "destination": "/workspace/audit-scratch",
                "access": "read-write",
                "purpose": "scratch",
                "identity": digest(b"audit-scratch")
            }
        ],
        "toolchain": {
            "kind": "system",
            "identity": digest(b"audit-toolchain"),
            "root": "/usr"
        },
        "cache": null,
        "scratch": {
            "destination": "/workspace/audit-scratch",
            "identity": digest(b"audit-scratch")
        },
        "session": {
            "stdio": "reserved-for-acp",
            "ipc": null,
            "cancellation": "run-scoped"
        }
    })
}

fn encode(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("encode audit session metadata")
}

// -- Launch-metadata bounds ---------------------------------------------------

#[test]
fn audit_launch_metadata_accepts_the_strict_shape() {
    let metadata = audit_session_metadata();
    session::parse_and_validate(&encode(&metadata))
        .expect("a well-formed audit session launch-metadata must parse and validate");
}

#[test]
fn audit_launch_metadata_rejects_oversized_input() {
    let mut metadata = audit_session_metadata();
    // Pad the environment to exceed the 1 MiB limit.
    metadata["environment"]["AUDIT_PADDING"] = json!("x".repeat(2 * 1024 * 1024));
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("oversized audit session metadata must be rejected");
    assert!(
        matches!(error, SessionError::TooLarge { .. }),
        "oversized audit session metadata must be rejected as TooLarge, got: {error}"
    );
}

#[test]
fn audit_launch_metadata_rejects_malformed_json() {
    let not_json = b"this is not json at all";
    assert!(
        session::parse_and_validate(not_json).is_err(),
        "non-JSON audit session metadata must be rejected"
    );

    let bare_array = b"[1, 2, 3]";
    assert!(
        session::parse_and_validate(bare_array).is_err(),
        "a bare-array audit session metadata must be rejected"
    );
}

#[test]
fn audit_launch_metadata_rejects_unknown_top_level_field() {
    let mut metadata = audit_session_metadata();
    metadata["audit_unknown_field"] = json!(true);
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("an unknown top-level field must be rejected");
    assert!(
        matches!(error, SessionError::Malformed { .. }),
        "an unknown top-level field must be rejected as Malformed, got: {error}"
    );
    assert!(
        error.to_string().contains("unknown field"),
        "the diagnostic must name the unknown field: {error}"
    );
}

#[test]
fn audit_launch_metadata_rejects_unknown_session_block_field() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["audit_host_socket_directory"] = json!("/var/run/audit");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("an unknown session-block field must be rejected");
    assert!(
        matches!(error, SessionError::Malformed { .. }),
        "an unknown session-block field must be rejected as Malformed, got: {error}"
    );
    assert!(
        error.to_string().contains("unknown field"),
        "the diagnostic must name the unknown field: {error}"
    );
}

#[test]
fn audit_launch_metadata_rejects_the_wrong_protocol_identifier() {
    let mut metadata = audit_session_metadata();
    metadata["protocol"] = json!("kvist-sandbox-request-v1");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("the one-shot request protocol identifier must be rejected");
    assert!(
        matches!(error, SessionError::Invalid { .. }),
        "the one-shot request protocol identifier must be rejected as Invalid, got: {error}"
    );
    assert!(
        error.to_string().contains("protocol must be"),
        "the diagnostic must name the protocol mismatch: {error}"
    );
}

#[test]
fn audit_launch_metadata_rejects_the_wrong_protocol_version() {
    let mut metadata = audit_session_metadata();
    metadata["protocol_version"] = json!(2);
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("the wrong protocol version must be rejected");
    assert!(
        matches!(error, SessionError::Invalid { .. }),
        "the wrong protocol version must be rejected as Invalid, got: {error}"
    );
    assert!(
        error.to_string().contains("protocol_version must be"),
        "the diagnostic must name the version mismatch: {error}"
    );
}

// -- Reserved stdio for ACP ---------------------------------------------------

#[test]
fn audit_session_stdio_is_reserved_for_acp() {
    let metadata = audit_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid audit metadata");
    assert_eq!(
        parsed.session.stdio,
        session::StdioRole::ReservedForAcp,
        "the session mode must reserve the payload's stdio for ACP"
    );
}

#[test]
fn audit_rejects_a_session_that_captures_stdio() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["stdio"] = json!("captured");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("a session that captures stdio must be rejected");
    assert!(
        matches!(error, SessionError::Invalid { .. }),
        "a session that captures stdio must be rejected as Invalid, got: {error}"
    );
    assert!(
        error.to_string().contains("stdio"),
        "the diagnostic must name the stdio violation: {error}"
    );
}

#[test]
fn audit_rejects_a_session_with_an_unknown_stdio_role() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["stdio"] = json!("inherited");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("a session with an unknown stdio role must be rejected");
    assert!(
        matches!(error, SessionError::Invalid { .. }),
        "a session with an unknown stdio role must be rejected as Invalid, got: {error}"
    );
    assert!(
        error.to_string().contains("stdio"),
        "the diagnostic must name the stdio violation: {error}"
    );
}

// -- IPC capability grant and denial ------------------------------------------

#[test]
fn audit_accepts_an_explicitly_granted_bounded_ipc_capability() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "inference-relay",
        "socket": "sandbox-loopback",
        "routes": ["POST /v1/chat/completions"]
    });
    session::parse_and_validate(&encode(&metadata))
        .expect("an explicitly granted bounded IPC capability must parse and validate");
}

#[test]
fn audit_rejects_an_ipc_capability_that_shares_host_loopback() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "inference-relay",
        "socket": "host-loopback",
        "routes": ["POST /v1/chat/completions"]
    });
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("an IPC capability that shares host loopback must be rejected");
    assert!(
        matches!(error, SessionError::Invalid { .. }),
        "an IPC capability that shares host loopback must be rejected as Invalid, got: {error}"
    );
    assert!(
        error.to_string().contains("host loopback"),
        "the diagnostic must name the host loopback violation: {error}"
    );
}

#[test]
fn audit_rejects_an_ipc_capability_that_grants_a_general_http_proxy() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "general-http-proxy",
        "socket": "sandbox-loopback",
        "routes": ["*"]
    });
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("a general HTTP proxy IPC capability must be rejected");
    assert!(
        matches!(error, SessionError::Invalid { .. }),
        "a general HTTP proxy IPC capability must be rejected as Invalid, got: {error}"
    );
    assert!(
        error.to_string().contains("general HTTP proxy"),
        "the diagnostic must name the general HTTP proxy violation: {error}"
    );
}

#[test]
fn audit_rejects_an_ipc_capability_with_an_unrelated_route() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "inference-relay",
        "socket": "sandbox-loopback",
        "routes": ["GET /v1/models", "POST /v1/chat/completions"]
    });
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("an IPC capability with an unrelated route must be rejected");
    assert!(
        matches!(error, SessionError::Invalid { .. }),
        "an IPC capability with an unrelated route must be rejected as Invalid, got: {error}"
    );
    assert!(
        error.to_string().contains("route"),
        "the diagnostic must name the route violation: {error}"
    );
}

#[test]
fn audit_rejects_an_ipc_capability_with_an_unknown_kind() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "arbitrary-ipc",
        "socket": "sandbox-loopback",
        "routes": ["POST /v1/chat/completions"]
    });
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("an unknown IPC capability kind must be rejected");
    assert!(
        matches!(error, SessionError::Malformed { .. }),
        "an unknown IPC capability kind must be rejected as Malformed, got: {error}"
    );
    assert!(
        error.to_string().contains("unknown variant"),
        "the diagnostic must name the unknown variant: {error}"
    );
}

#[test]
fn audit_ipc_capability_is_optional_and_absent_by_default() {
    let metadata = audit_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid audit metadata");
    assert!(
        parsed.session.ipc.is_none(),
        "the IPC capability must be optional; absence denies the relay"
    );
}

// -- Process-group reap -------------------------------------------------------

#[test]
fn audit_runner_owns_the_process_group_on_success() {
    let metadata = audit_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid audit metadata");
    assert!(
        session::owns_process_group(&parsed),
        "the runner must own and reap the whole process group on success"
    );
}

#[test]
fn audit_runner_owns_the_process_group_on_failure() {
    let mut metadata = audit_session_metadata();
    metadata["argv"] = json!(["/usr/bin/false"]);
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid audit metadata");
    assert!(
        session::owns_process_group(&parsed),
        "the runner must own and reap the whole process group on payload failure"
    );
}

#[test]
fn audit_runner_owns_the_process_group_on_timeout() {
    let mut metadata = audit_session_metadata();
    metadata["resources"]["wall_time_ms"] = json!(1);
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid audit metadata");
    assert!(
        session::owns_process_group(&parsed),
        "the runner must own and reap the whole process group on timeout"
    );
}

#[test]
fn audit_runner_owns_the_process_group_on_output_limit_breach() {
    let mut metadata = audit_session_metadata();
    metadata["resources"]["max_output_bytes"] = json!(1);
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid audit metadata");
    assert!(
        session::owns_process_group(&parsed),
        "the runner must own and reap the whole process group on output-limit breach"
    );
}

// -- Cancellation -------------------------------------------------------------

#[test]
fn audit_cancellation_capability_is_run_scoped() {
    let metadata = audit_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid audit metadata");
    assert_eq!(
        parsed.session.cancellation,
        session::Cancellation::RunScoped,
        "the cancellation capability must be run-scoped"
    );
}

#[test]
fn audit_rejects_a_cancellation_capability_that_is_not_run_scoped() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["cancellation"] = json!("host-scoped");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("a non-run-scoped cancellation capability must be rejected");
    assert!(
        matches!(error, SessionError::Invalid { .. }),
        "a non-run-scoped cancellation capability must be rejected as Invalid, got: {error}"
    );
    assert!(
        error.to_string().contains("cancellation"),
        "the diagnostic must name the cancellation violation: {error}"
    );
}

#[test]
fn audit_rejects_a_cancellation_capability_with_an_unknown_scope() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["cancellation"] = json!("ambient");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("an unknown cancellation scope must be rejected");
    assert!(
        matches!(error, SessionError::Invalid { .. }),
        "an unknown cancellation scope must be rejected as Invalid, got: {error}"
    );
    assert!(
        error.to_string().contains("cancellation"),
        "the diagnostic must name the cancellation violation: {error}"
    );
}

// -- Absence of any host loopback or general egress path ----------------------

#[test]
fn audit_session_mode_grants_no_host_loopback() {
    let metadata = audit_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid audit metadata");
    assert!(
        !session::grants_host_loopback(&parsed),
        "session mode must not grant host loopback sharing"
    );
}

#[test]
fn audit_session_mode_grants_no_general_egress() {
    let metadata = audit_session_metadata();
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid audit metadata");
    assert!(
        !session::grants_general_egress(&parsed),
        "session mode must not grant a general egress path"
    );
}

#[test]
fn audit_a_valid_bounded_relay_grant_grants_no_host_loopback_or_general_egress() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["ipc"] = json!({
        "kind": "inference-relay",
        "socket": "sandbox-loopback",
        "routes": ["POST /v1/chat/completions"]
    });
    let parsed = session::parse_and_validate(&encode(&metadata)).expect("valid audit metadata");
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

#[test]
fn audit_session_mode_rejects_a_host_socket_directory_grant() {
    let mut metadata = audit_session_metadata();
    metadata["session"]["host_socket_directory"] = json!("/var/run/audit");
    let error = session::parse_and_validate(&encode(&metadata))
        .expect_err("a host socket directory grant must be rejected");
    assert!(
        matches!(error, SessionError::Malformed { .. }),
        "a host socket directory grant must be rejected as Malformed, got: {error}"
    );
    assert!(
        error.to_string().contains("unknown field"),
        "the diagnostic must name the unknown field: {error}"
    );
}

// -- Legacy rejection ---------------------------------------------------------

#[test]
fn audit_rejects_the_one_shot_v1_request_as_session_metadata() {
    let one_shot = json!({
        "protocol": "kvist-sandbox-request-v1",
        "protocol_version": 1,
        "phase": "authoring",
        "argv": ["/usr/bin/true"],
        "working_directory": "/workspace/audit",
        "environment": {
            "HOME": "/workspace/audit-home",
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
            "runner": digest(b"audit-runner"),
            "backend": {
                "kind": "bubblewrap",
                "path": "/usr/bin/bwrap",
                "digest": digest(b"audit-backend")
            },
            "policy": digest(b"audit-policy"),
            "toolchain": digest(b"audit-toolchain"),
            "command": digest(b"audit-command"),
            "mount_plan": digest(b"audit-mount-plan")
        },
        "grants": [
            {
                "source": "/host/audit-component",
                "destination": "/workspace/audit-component",
                "access": "read-write",
                "purpose": "authoring",
                "identity": digest(b"audit-component")
            },
            {
                "source": "/usr",
                "destination": "/usr",
                "access": "read-only",
                "purpose": "toolchain",
                "identity": digest(b"audit-toolchain-usr")
            },
            {
                "source": "/host/audit-scratch",
                "destination": "/workspace/audit-scratch",
                "access": "read-write",
                "purpose": "scratch",
                "identity": digest(b"audit-scratch")
            }
        ],
        "toolchain": {
            "kind": "system",
            "identity": digest(b"audit-toolchain"),
            "root": "/usr"
        },
        "cache": null,
        "scratch": {
            "destination": "/workspace/audit-scratch",
            "identity": digest(b"audit-scratch")
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
