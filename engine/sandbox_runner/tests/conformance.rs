//! Conformance tests for the redefined version-one sandbox protocol.
//!
//! These exercise the runner's own parser and validator directly (accepted
//! strict shape, canonical serialization, malformed/oversized/unknown values,
//! legacy rejection, and the mediated-Cargo acquisition/verification policy)
//! plus the fail-closed executable boundary. They do not require Bubblewrap;
//! production enforcement evidence belongs to the later integration task and
//! intentionally remains outside this file.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use kvist_sandbox_runner::protocol::SandboxRequest;
use kvist_sandbox_runner::validation::{self, ProtocolError};

fn digest(bytes: &[u8]) -> String {
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

/// A generic authoring request with a system toolchain. This is the baseline
/// non-Cargo shape.
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

fn cargo_identity() -> String {
    digest(b"exact-cargo")
}

fn cargo_identities() -> Value {
    json!({
        "runner": digest(b"runner"),
        "backend": {
            "kind": "bubblewrap",
            "path": "/usr/bin/bwrap",
            "digest": digest(b"backend")
        },
        "policy": digest(b"policy"),
        "toolchain": cargo_identity(),
        "command": digest(b"command"),
        "mount_plan": digest(b"mount-plan")
    })
}

/// A network-enabled `<cargo> fetch` acquisition request in the exact v1 shape:
/// a Cargo toolchain rooted at an immutable directory that contains `cargo`, a
/// writable Cargo home, an isolated writable lockfile workspace, target
/// scratch, and pre-execution promotion intent.
fn cargo_acquisition_request() -> Value {
    json!({
        "protocol": "kvist-sandbox-request-v1",
        "protocol_version": 1,
        "phase": "dependency-acquisition",
        "argv": ["/workspace/toolchain/cargo", "fetch"],
        "working_directory": "/workspace/lockfile",
        "environment": {
            "HOME": "/workspace/scratch/home",
            "PATH": "/usr/bin",
            "CARGO_HOME": "/workspace/cargo-home",
            "CARGO_TARGET_DIR": "/workspace/scratch/target",
            "CARGO_NET_GIT_FETCH_WITH_CLI": "false"
        },
        "network": { "mode": "package-sources", "allowed_sources": [crates_io()] },
        "resources": {
            "wall_time_ms": 5000,
            "max_output_bytes": 65536,
            "max_processes": 16,
            "max_files": 256,
            "max_file_bytes": 1048576,
            "max_scratch_bytes": 1048576,
            "max_cache_bytes": 1048576
        },
        "identities": cargo_identities(),
        "grants": [
            {
                "source": "/host/toolchain",
                "destination": "/workspace/toolchain",
                "access": "read-only",
                "purpose": "toolchain",
                "identity": cargo_identity()
            },
            {
                "source": "/host/attempt/cargo-home",
                "destination": "/workspace/cargo-home",
                "access": "read-write",
                "purpose": "dependency-cache",
                "identity": digest(b"attempt-cargo-home")
            },
            {
                "source": "/host/attempt/scratch",
                "destination": "/workspace/scratch",
                "access": "read-write",
                "purpose": "scratch",
                "identity": digest(b"acquisition-scratch")
            },
            {
                "source": "/host/attempt/lockfile",
                "destination": "/workspace/lockfile",
                "access": "read-write",
                "purpose": "lockfile",
                "identity": digest(b"lockfile-before")
            }
        ],
        "toolchain": {
            "kind": "cargo",
            "identity": cargo_identity(),
            "root": "/workspace/toolchain",
            "cargo": "/workspace/toolchain/cargo"
        },
        "cache": {
            "cargo_home": "/workspace/cargo-home",
            "registry": "/workspace/cargo-home/registry",
            "git": "/workspace/cargo-home/git",
            "writable": {
                "destination": "/workspace/cargo-home",
                "identity": digest(b"attempt-cargo-home")
            },
            "lockfile": {
                "destination": "/workspace/lockfile",
                "before_identity": digest(b"lockfile-before")
            },
            "promotion": {
                "enabled": true,
                "source": "/workspace/cargo-home"
            }
        },
        "scratch": {
            "destination": "/workspace/scratch",
            "identity": digest(b"acquisition-scratch")
        }
    })
}

/// An offline `<cargo> test --locked` verification request in the exact v1
/// shape: a Cargo toolchain, a read-only approved Cargo-home generation, target
/// scratch, and a read-only verification workspace.
fn cargo_verification_request() -> Value {
    json!({
        "protocol": "kvist-sandbox-request-v1",
        "protocol_version": 1,
        "phase": "verification",
        "argv": ["/workspace/toolchain/cargo", "test", "--locked"],
        "working_directory": "/workspace/component",
        "environment": {
            "HOME": "/workspace/scratch/home",
            "PATH": "/usr/bin",
            "CARGO_HOME": "/workspace/cargo-home",
            "CARGO_TARGET_DIR": "/workspace/scratch/target",
            "CARGO_NET_OFFLINE": "true"
        },
        "network": { "mode": "deny", "allowed_sources": [] },
        "resources": {
            "wall_time_ms": 5000,
            "max_output_bytes": 65536,
            "max_processes": 16,
            "max_files": 256,
            "max_file_bytes": 1048576,
            "max_scratch_bytes": 1048576,
            "max_cache_bytes": 1048576
        },
        "identities": cargo_identities(),
        "grants": [
            {
                "source": "/host/toolchain",
                "destination": "/workspace/toolchain",
                "access": "read-only",
                "purpose": "toolchain",
                "identity": cargo_identity()
            },
            {
                "source": "/host/project-cache/generation",
                "destination": "/workspace/cargo-home",
                "access": "read-only",
                "purpose": "dependency-cache",
                "identity": digest(b"approved-cargo-home")
            },
            {
                "source": "/host/verify/scratch",
                "destination": "/workspace/scratch",
                "access": "read-write",
                "purpose": "scratch",
                "identity": digest(b"verification-scratch")
            },
            {
                "source": "/host/component",
                "destination": "/workspace/component",
                "access": "read-only",
                "purpose": "verification",
                "identity": digest(b"verification-workspace")
            }
        ],
        "toolchain": {
            "kind": "cargo",
            "identity": cargo_identity(),
            "root": "/workspace/toolchain",
            "cargo": "/workspace/toolchain/cargo"
        },
        "cache": {
            "cargo_home": "/workspace/cargo-home",
            "registry": "/workspace/cargo-home/registry",
            "git": "/workspace/cargo-home/git",
            "approved": {
                "destination": "/workspace/cargo-home",
                "identity": digest(b"approved-cargo-home")
            }
        },
        "scratch": {
            "destination": "/workspace/scratch",
            "identity": digest(b"verification-scratch")
        }
    })
}

fn encode(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("encode request")
}

// -- Baseline strict-shape, serialization, and legacy rejection ---------------

#[test]
fn accepts_the_strict_version_one_shape() {
    let request = valid_request();
    validation::parse_and_validate(&encode(&request))
        .expect("valid request must parse and validate");
}

#[test]
fn serialization_round_trips_deterministically() {
    let bytes = encode(&valid_request());
    let parsed: SandboxRequest = validation::parse_request(&bytes).expect("parse");
    let reserialized = serde_json::to_vec(&parsed).expect("reserialize");
    let reparsed: SandboxRequest =
        validation::parse_request(&reserialized).expect("reparse canonical bytes");
    assert_eq!(parsed, reparsed, "canonical serialization must round-trip");
    let twice = serde_json::to_vec(&reparsed).expect("serialize again");
    assert_eq!(reserialized, twice, "serialization must be deterministic");
}

#[test]
fn rejects_the_retired_component_mount_request_shape() {
    let legacy = json!({
        "protocol_version": 1,
        "program": "/usr/bin/true",
        "arguments": [],
        "working_directory": "/workspace/component",
        "network": "deny",
        "mounts": [{
            "source": "/host/component",
            "destination": "/workspace/component",
            "access": "read-write"
        }],
        "environment": {},
        "context_files": ["/workspace/component/REQUIREMENTS.md"]
    });
    let error =
        validation::parse_and_validate(&encode(&legacy)).expect_err("legacy must be rejected");
    let text = error.to_string();
    assert!(
        matches!(error, ProtocolError::Malformed { .. }),
        "legacy shape must fail parsing: {text}"
    );
    assert!(
        text.contains("unknown field") || text.contains("missing field"),
        "legacy rejection must be actionable: {text}"
    );
}

#[test]
fn rejects_unknown_top_level_field() {
    let mut request = valid_request();
    request["fallback_to_host"] = json!(true);
    let error =
        validation::parse_and_validate(&encode(&request)).expect_err("unknown field must reject");
    assert!(matches!(error, ProtocolError::Malformed { .. }));
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn rejects_untyped_or_ambiguous_fields() {
    let base = valid_request();

    let mut unknown_phase = base.clone();
    unknown_phase["phase"] = json!("implementation");

    let mut scalar_program = base.clone();
    scalar_program
        .as_object_mut()
        .expect("object")
        .remove("argv");
    scalar_program["program"] = json!("/usr/bin/true");

    let mut unknown_purpose = base.clone();
    unknown_purpose["grants"][0]["purpose"] = json!("component");

    let mut unknown_access = base.clone();
    unknown_access["grants"][0]["access"] = json!("write");

    let mut string_network = base.clone();
    string_network["network"] = json!("deny");

    for (name, request) in [
        ("unknown phase", unknown_phase),
        ("scalar program", scalar_program),
        ("unknown purpose", unknown_purpose),
        ("unknown access", unknown_access),
        ("string network", string_network),
    ] {
        let result = validation::parse_and_validate(&encode(&request));
        assert!(result.is_err(), "{name} must be rejected");
        assert!(
            !result.unwrap_err().to_string().trim().is_empty(),
            "{name} must produce a diagnostic"
        );
    }
}

#[test]
fn rejects_relative_working_directory() {
    let mut request = valid_request();
    request["working_directory"] = json!("tests");
    let error = validation::parse_and_validate(&encode(&request)).expect_err("relative rejected");
    assert!(matches!(error, ProtocolError::Invalid { .. }));
    assert!(error.to_string().contains("working directory"));
}

#[test]
fn rejects_traversal_and_overlapping_destinations() {
    let mut alias = valid_request();
    alias["grants"][1]["destination"] = json!("/workspace/component/tests/../REQUIREMENTS.md");
    assert!(validation::parse_and_validate(&encode(&alias)).is_err());

    let mut overlap = valid_request();
    overlap["grants"]
        .as_array_mut()
        .expect("grants")
        .push(json!({
            "source": "/host/component/tests/nested",
            "destination": "/workspace/component/tests/nested",
            "access": "read-write",
            "purpose": "authoring",
            "identity": digest(b"nested")
        }));
    let error = validation::parse_and_validate(&encode(&overlap))
        .expect_err("overlapping destination rejected");
    assert!(error.to_string().contains("overlap"));
}

#[test]
fn rejects_malformed_digests_and_bad_bounds() {
    let mut bad_digest = valid_request();
    bad_digest["identities"]["policy"] = json!("sha1:deadbeef");
    assert!(validation::parse_and_validate(&encode(&bad_digest)).is_err());

    let mut zero_limit = valid_request();
    zero_limit["resources"]["wall_time_ms"] = json!(0);
    let error =
        validation::parse_and_validate(&encode(&zero_limit)).expect_err("zero limit rejected");
    assert!(error.to_string().contains("wall_time_ms"));
}

#[test]
fn rejects_oversized_request() {
    let mut request = valid_request();
    request["environment"]["PADDING"] = json!("x".repeat(2 * 1024 * 1024));
    let error =
        validation::parse_and_validate(&encode(&request)).expect_err("oversized must be rejected");
    assert!(matches!(error, ProtocolError::TooLarge { .. }));
}

#[test]
fn rejects_deny_network_with_sources() {
    let mut request = valid_request();
    request["network"]["allowed_sources"] = json!([crates_io()]);
    let error =
        validation::parse_and_validate(&encode(&request)).expect_err("deny+sources rejected");
    assert!(error.to_string().contains("allowed_sources"));
}

// -- Mediated-Cargo acquisition and verification ------------------------------

#[test]
fn accepts_the_cargo_acquisition_shape() {
    let request = cargo_acquisition_request();
    validation::parse_and_validate(&encode(&request))
        .expect("cargo acquisition request must parse and validate");
}

#[test]
fn accepts_the_cargo_verification_shape() {
    let request = cargo_verification_request();
    validation::parse_and_validate(&encode(&request))
        .expect("cargo verification request must parse and validate");
}

#[test]
fn accepts_the_immutable_cargo_git_source() {
    let mut request = cargo_acquisition_request();
    request["network"]["allowed_sources"] = json!([{
        "kind": "cargo-git",
        "repository": "https://git.example.invalid/dependency.git",
        "revision": "0123456789abcdef0123456789abcdef01234567",
        "identity": validation::git_identity(
            "https://git.example.invalid/dependency.git",
            "0123456789abcdef0123456789abcdef01234567"
        )
    }]);
    validation::parse_and_validate(&encode(&request))
        .expect("immutable cargo-git source must parse and validate");
}

#[test]
fn rejects_unknown_package_source_kind() {
    let mut request = cargo_acquisition_request();
    request["network"]["allowed_sources"] = json!([{
        "kind": "model-api",
        "origin": "https://model.example.invalid/",
        "identity": digest(b"model")
    }]);
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("unknown source kind must reject");
    assert!(matches!(error, ProtocolError::Malformed { .. }));
    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn rejects_package_sources_outside_acquisition_phase() {
    let mut request = valid_request();
    request["network"] = json!({
        "mode": "package-sources",
        "allowed_sources": [crates_io()]
    });
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("package-sources outside acquisition must reject");
    assert!(error.to_string().contains("package-sources"));
}

#[test]
fn rejects_read_write_dependency_cache_outside_acquisition() {
    let mut request = valid_request();
    request["phase"] = json!("verification");
    request["grants"][1] = json!({
        "source": "/host/cache",
        "destination": "/workspace/dependencies/project-cache",
        "access": "read-write",
        "purpose": "dependency-cache",
        "identity": digest(b"cache")
    });
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("writable verification cache must reject");
    assert!(error.to_string().contains("read-only purpose"));
}

#[test]
fn rejects_zero_max_cache_bytes() {
    let mut request = cargo_acquisition_request();
    request["resources"]["max_cache_bytes"] = json!(0);
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("zero max_cache_bytes must reject");
    assert!(error.to_string().contains("max_cache_bytes"));
}

#[test]
fn rejects_resource_limits_exceeding_safe_maxima() {
    let cases = [
        ("wall_time_ms", json!(86_400_000_u64 + 1)),
        ("max_output_bytes", json!(268_435_456_u64 + 1)),
        ("max_processes", json!(4096_u64 + 1)),
        ("max_files", json!(1_048_576_u64 + 1)),
        ("max_file_bytes", json!(8_589_934_592_u64 + 1)),
        ("max_scratch_bytes", json!(68_719_476_736_u64 + 1)),
    ];
    for (field, value) in cases {
        let mut request = valid_request();
        request["resources"][field] = value;
        let error = validation::parse_and_validate(&encode(&request))
            .expect_err(&format!("{field} over maximum must reject"));
        assert!(
            error.to_string().contains("safe maximum") && error.to_string().contains(field),
            "{field} rejection must be actionable: {error}"
        );
    }

    let mut cache_over = cargo_acquisition_request();
    cache_over["resources"]["max_cache_bytes"] = json!(68_719_476_736_u64 + 1);
    let error = validation::parse_and_validate(&encode(&cache_over))
        .expect_err("max_cache_bytes over maximum must reject");
    assert!(error.to_string().contains("max_cache_bytes"));
    assert!(error.to_string().contains("safe maximum"));
}

#[test]
fn accepts_resource_limits_at_the_safe_maxima() {
    let mut request = valid_request();
    request["resources"]["wall_time_ms"] = json!(86_400_000_u64);
    request["resources"]["max_output_bytes"] = json!(268_435_456_u64);
    request["resources"]["max_processes"] = json!(4096_u64);
    request["resources"]["max_files"] = json!(1_048_576_u64);
    request["resources"]["max_file_bytes"] = json!(8_589_934_592_u64);
    request["resources"]["max_scratch_bytes"] = json!(68_719_476_736_u64);
    validation::parse_and_validate(&encode(&request))
        .expect("resource limits exactly at the safe maxima must be accepted");
}

// -- Scratch/cache endpoint <-> grant correspondence --------------------------

#[test]
fn rejects_scratch_without_a_corresponding_grant() {
    let mut request = valid_request();
    request["grants"].as_array_mut().expect("grants").remove(3);
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("scratch without a matching grant must reject");
    assert!(
        error.to_string().contains("scratch") && error.to_string().contains("does not correspond"),
        "diagnostic must be actionable: {error}"
    );
}

#[test]
fn rejects_scratch_identity_mismatch_with_its_grant() {
    let mut request = valid_request();
    request["scratch"]["identity"] = json!(digest(b"different-scratch-identity"));
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("scratch identity mismatch must reject");
    assert!(error.to_string().contains("identity does not match"));
}

#[test]
fn rejects_scratch_grant_with_wrong_access() {
    let mut request = valid_request();
    request["grants"][3]["access"] = json!("read-only");
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("read-only scratch grant must reject");
    assert!(error.to_string().contains("scratch"));
}

#[test]
fn rejects_cache_attempt_without_a_corresponding_grant() {
    // v1: the writable Cargo-home endpoint must correspond to a declared grant.
    let mut request = cargo_acquisition_request();
    request["cache"]["writable"]["destination"] = json!("/workspace/unbacked");
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("unbacked writable Cargo-home endpoint must reject");
    assert!(
        error.to_string().contains("does not correspond"),
        "diagnostic must be actionable: {error}"
    );
}

#[test]
fn rejects_cache_attempt_identity_mismatch() {
    // v1: the writable Cargo-home endpoint identity must match its grant.
    let mut request = cargo_acquisition_request();
    request["cache"]["writable"]["identity"] = json!(digest(b"mismatched-attempt"));
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("writable Cargo-home identity mismatch must reject");
    assert!(error.to_string().contains("identity does not match"));
}

#[test]
fn rejects_cache_promotion_destination_identity_mismatch() {
    // v1: the isolated lockfile workspace identity must match its grant.
    let mut request = cargo_acquisition_request();
    request["cache"]["lockfile"]["before_identity"] = json!(digest(b"unexpected-lockfile"));
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("lockfile identity mismatch must reject");
    assert!(error.to_string().contains("identity does not match"));
}

#[test]
fn rejects_cache_promotion_source_not_the_attempt_endpoint() {
    // v1: the promotion source must be the real writable Cargo home, not the
    // lockfile workspace or any other declared grant.
    let mut request = cargo_acquisition_request();
    request["cache"]["promotion"]["source"] = json!("/workspace/lockfile");
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("promotion source that is not the Cargo home must reject");
    assert!(
        error.to_string().contains("promotion source"),
        "diagnostic must be actionable: {error}"
    );
}

#[test]
fn rejects_noncanonical_paths() {
    let mut duplicate_separator = valid_request();
    duplicate_separator["grants"][1]["destination"] = json!("/workspace/component//tests");
    let error = validation::parse_and_validate(&encode(&duplicate_separator))
        .expect_err("duplicate separator must reject");
    assert!(error.to_string().contains("duplicate"));

    let mut trailing_slash = valid_request();
    trailing_slash["grants"][1]["destination"] = json!("/workspace/component/tests/");
    let error = validation::parse_and_validate(&encode(&trailing_slash))
        .expect_err("trailing slash must reject");
    assert!(error.to_string().contains("trailing"));

    let mut argv_duplicate = valid_request();
    argv_duplicate["argv"] = json!(["/usr//bin/true"]);
    assert!(
        validation::parse_and_validate(&encode(&argv_duplicate)).is_err(),
        "a non-canonical argv program must be rejected"
    );

    let mut source_dup = valid_request();
    source_dup["grants"][1]["source"] = json!("/host//component/tests");
    assert!(
        validation::parse_and_validate(&encode(&source_dup)).is_err(),
        "a non-canonical grant source must be rejected"
    );
}

#[test]
fn rejects_oversized_environment_name() {
    let mut request = valid_request();
    let name = format!("A{}", "B".repeat(8192));
    request["environment"][name] = json!("value");
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("oversized environment name must reject");
    assert!(
        matches!(error, ProtocolError::TooLarge { .. })
            || error.to_string().contains("environment name")
    );
}

// -- Additional v1 mediated-Cargo policy cases --------------------------------

#[test]
fn acquisition_rejects_toolchain_and_grant_substitution() {
    let base = cargo_acquisition_request();
    let mut wrong_kind = base.clone();
    wrong_kind["toolchain"]["kind"] = json!("system");
    wrong_kind["toolchain"]
        .as_object_mut()
        .expect("obj")
        .remove("cargo");
    let mut argv_tool = base.clone();
    argv_tool["argv"][0] = json!("/workspace/toolchain/other-cargo");
    let mut identity = base.clone();
    identity["identities"]["toolchain"] = json!(digest(b"replacement"));
    let mut grant = base.clone();
    grant["grants"][0]["identity"] = json!(digest(b"replacement"));
    for request in [wrong_kind, argv_tool, identity, grant] {
        assert!(validation::parse_and_validate(&encode(&request)).is_err());
    }
}

#[test]
fn cargo_toolchain_grant_must_be_the_root_not_the_executable() {
    // The single read-only toolchain grant is the immutable root, not the cargo
    // executable file alone.
    let mut request = cargo_acquisition_request();
    request["grants"][0]["destination"] = json!("/workspace/toolchain/cargo");
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("a bare cargo-executable grant must reject");
    assert!(!error.to_string().trim().is_empty());
    // The cargo executable must live strictly beneath the toolchain root.
    let mut flat = cargo_acquisition_request();
    flat["toolchain"]["root"] = json!("/workspace/toolchain/cargo");
    assert!(validation::parse_and_validate(&encode(&flat)).is_err());
}

#[test]
fn cargo_phase_environment_is_an_exact_allowlist() {
    for (mut request, name, value) in [
        (cargo_acquisition_request(), "LD_PRELOAD", "x"),
        (cargo_acquisition_request(), "HTTPS_PROXY", "http://proxy/"),
        (
            cargo_acquisition_request(),
            "GIT_CONFIG_GLOBAL",
            "/host/config",
        ),
        (
            cargo_acquisition_request(),
            "CARGO_SOURCE_CRATES_IO_REPLACE_WITH",
            "evil",
        ),
        (
            cargo_verification_request(),
            "CARGO_REGISTRIES_PRIVATE_TOKEN",
            "secret",
        ),
    ] {
        request["environment"][name] = json!(value);
        assert!(
            validation::parse_and_validate(&encode(&request)).is_err(),
            "{name} must reject"
        );
    }
    let mut missing = cargo_acquisition_request();
    missing["environment"]
        .as_object_mut()
        .expect("environment")
        .remove("CARGO_TARGET_DIR");
    assert!(validation::parse_and_validate(&encode(&missing)).is_err());
    let mut incorrect = cargo_verification_request();
    incorrect["environment"]["CARGO_NET_OFFLINE"] = json!("false");
    assert!(validation::parse_and_validate(&encode(&incorrect)).is_err());
}

#[test]
fn cargo_phase_topology_is_exact() {
    let base = cargo_acquisition_request();
    let mut no_lockfile = base.clone();
    no_lockfile["cache"]
        .as_object_mut()
        .expect("cache")
        .remove("lockfile");
    let mut wrong_working_directory = base.clone();
    wrong_working_directory["working_directory"] = json!("/workspace/component");
    let mut extra_grant = base.clone();
    extra_grant["grants"]
        .as_array_mut()
        .expect("grants")
        .push(json!({
            "source": "/host/extra",
            "destination": "/workspace/extra",
            "access": "read-only",
            "purpose": "context",
            "identity": digest(b"extra")
        }));
    let mut verification_cache = cargo_verification_request();
    verification_cache["cache"]["writable"] = json!({
        "destination": "/workspace/cargo-home",
        "identity": digest(b"approved-cargo-home")
    });
    let mut locked_fetch = cargo_acquisition_request();
    locked_fetch["argv"] = json!(["/workspace/toolchain/cargo", "fetch", "--locked"]);
    for request in [
        no_lockfile,
        wrong_working_directory,
        extra_grant,
        verification_cache,
        locked_fetch,
    ] {
        assert!(validation::parse_and_validate(&encode(&request)).is_err());
    }
}

#[test]
fn source_identities_use_golden_vectors_and_reject_substitutions() {
    assert_eq!(
        registry_identity(
            "private",
            "https://registry.example.invalid/index/",
            "https://registry.example.invalid/crates/"
        ),
        "sha256:4d3fc763437bea9217f14b2bc9b5201dbf78ea13ac0ef390497cc54896ab9486"
    );
    assert_eq!(
        validation::git_identity(
            "https://git.example.invalid/dependency.git",
            "0123456789abcdef0123456789abcdef01234567"
        ),
        "sha256:3405bc5e24e0f91ff11d2271a69f9c9b7a0547adb6acace6d10638fee39d2fe6"
    );
    let mut identity = cargo_acquisition_request();
    identity["network"]["allowed_sources"][0]["identity"] = json!(digest(b"forged"));
    assert!(validation::parse_and_validate(&encode(&identity)).is_err());
    let mut field = cargo_acquisition_request();
    field["network"]["allowed_sources"][0]["download_origin"] = json!("https://mirror.invalid/");
    assert!(validation::parse_and_validate(&encode(&field)).is_err());
}

#[test]
fn production_sources_reject_loopback_aliases_duplicates_and_overlap() {
    let mut loopback = cargo_acquisition_request();
    loopback["network"]["allowed_sources"] = json!([{
        "kind": "cargo-registry",
        "name": "local",
        "index_origin": "http://127.0.0.1:8080/index/",
        "download_origin": "http://127.0.0.1:8080/crates/",
        "identity": registry_identity("local", "http://127.0.0.1:8080/index/", "http://127.0.0.1:8080/crates/")
    }]);
    assert!(validation::parse_and_validate(&encode(&loopback)).is_err());

    let mut alias = cargo_acquisition_request();
    alias["network"]["allowed_sources"] = json!([{
        "kind": "cargo-registry",
        "name": "numeric",
        "index_origin": "https://2130706433/index/",
        "download_origin": "https://registry.example.invalid/crates/",
        "identity": registry_identity("numeric", "https://2130706433/index/", "https://registry.example.invalid/crates/")
    }]);
    assert!(validation::parse_and_validate(&encode(&alias)).is_err());

    let mut overlapping = cargo_acquisition_request();
    overlapping["network"]["allowed_sources"] = json!([
        {
            "kind": "cargo-registry",
            "name": "first",
            "index_origin": "https://registry.example.invalid/index/",
            "download_origin": "https://registry.example.invalid/crates/",
            "identity": registry_identity("first", "https://registry.example.invalid/index/", "https://registry.example.invalid/crates/")
        },
        {
            "kind": "cargo-git",
            "repository": "https://registry.example.invalid/index/repository.git",
            "revision": "0123456789abcdef0123456789abcdef01234567",
            "identity": validation::git_identity("https://registry.example.invalid/index/repository.git", "0123456789abcdef0123456789abcdef01234567")
        }
    ]);
    assert!(validation::parse_and_validate(&encode(&overlapping)).is_err());
}

// -- Fail-closed executable boundary ------------------------------------------

#[test]
fn executable_rejects_legacy_request_with_actionable_diagnostic() {
    let legacy = json!({
        "protocol_version": 1,
        "program": "/usr/bin/true",
        "mounts": [],
        "context_files": []
    });
    let output = run_runner(&encode(&legacy));
    assert!(!output.status.success(), "legacy request must fail closed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown field") || stderr.contains("missing field"),
        "diagnostic must be actionable: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "a rejected request must emit no stdout"
    );
}

#[test]
fn executable_executes_successfully_on_valid_request() {
    let temp = tempfile::tempdir().unwrap();
    let req_file = temp.path().join("REQUIREMENTS.md");
    std::fs::write(&req_file, "intent").unwrap();
    let tests_dir = temp.path().join("tests");
    std::fs::create_dir(&tests_dir).unwrap();
    let scratch_dir = temp.path().join("scratch");
    std::fs::create_dir(&scratch_dir).unwrap();

    let mut request = valid_request();
    request["grants"][0]["source"] = serde_json::json!(req_file.to_string_lossy());
    request["grants"][1]["source"] = serde_json::json!(tests_dir.to_string_lossy());
    request["grants"][3]["source"] = serde_json::json!(scratch_dir.to_string_lossy());

    let output = run_runner(&encode(&request));
    assert!(
        output.status.success(),
        "runner must succeed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn executable_probe_succeeds_with_enforcement() {
    let output = Command::new(env!("CARGO_BIN_EXE_kvist-sandbox-runner"))
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .expect("run probe");
    assert!(
        output.status.success(),
        "probe must succeed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.stdout.is_empty(), "confirmed probe must emit JSON");
    let probe: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(probe["protocol"], "kvist-sandbox-probe-v1");
    assert_eq!(probe["protocol_version"], 1);
}

#[test]
fn schema_mirrors_parser_enumerants_and_required_fields() {
    let schema: Value = serde_json::from_str(include_str!(
        "../schema/kvist-sandbox-request-v1.schema.json"
    ))
    .expect("request schema must be valid JSON");

    let strings = |value: Option<&Value>| -> Vec<String> {
        value
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };

    let mut phases = strings(schema.pointer("/properties/phase/enum"));
    phases.sort();
    assert_eq!(
        phases,
        ["authoring", "dependency-acquisition", "verification"]
    );

    let mut modes = strings(schema.pointer("/$defs/network/properties/mode/enum"));
    modes.sort();
    assert_eq!(modes, ["deny", "package-sources"]);

    let mut purposes = strings(schema.pointer("/$defs/grant/properties/purpose/enum"));
    purposes.sort();
    assert_eq!(
        purposes,
        [
            "authoring",
            "context",
            "dependency-cache",
            "lockfile",
            "scratch",
            "toolchain",
            "verification"
        ]
    );

    let toolchain_kinds: Vec<String> = schema
        .pointer("/$defs/toolchain/oneOf")
        .and_then(Value::as_array)
        .expect("toolchain oneOf")
        .iter()
        .filter_map(|variant| {
            variant
                .pointer("/properties/kind/const")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert!(toolchain_kinds.contains(&"system".to_owned()));
    assert!(toolchain_kinds.contains(&"cargo".to_owned()));

    // The Cargo toolchain variant carries an immutable root and a cargo path.
    let cargo_required: Vec<String> = schema
        .pointer("/$defs/toolchain/oneOf")
        .and_then(Value::as_array)
        .expect("toolchain oneOf")
        .iter()
        .find(|variant| {
            variant
                .pointer("/properties/kind/const")
                .and_then(Value::as_str)
                == Some("cargo")
        })
        .map(|variant| strings(variant.pointer("/required")))
        .unwrap_or_default();
    assert!(cargo_required.contains(&"root".to_owned()));
    assert!(cargo_required.contains(&"cargo".to_owned()));

    let source_kinds: Vec<String> = schema
        .pointer("/$defs/allowedSource/oneOf")
        .and_then(Value::as_array)
        .expect("allowedSource oneOf")
        .iter()
        .filter_map(|variant| {
            variant
                .pointer("/properties/kind/const")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert!(source_kinds.contains(&"cargo-registry".to_owned()));
    assert!(source_kinds.contains(&"cargo-git".to_owned()));

    assert!(
        schema
            .pointer("/$defs/resources/properties/max_cache_bytes")
            .is_some()
    );

    // The pre-execution promotion intent must not carry post-fetch state.
    let promotion_required = strings(schema.pointer("/$defs/cachePromotion/required"));
    assert_eq!(promotion_required, ["enabled", "source"]);
    for absent in [
        "lockfile_after_identity",
        "supported_source_identities",
        "manifest",
    ] {
        assert!(
            schema
                .pointer(&format!("/$defs/cachePromotion/properties/{absent}"))
                .is_none(),
            "promotion must not declare post-fetch field `{absent}`"
        );
    }
    assert!(
        schema
            .pointer("/$defs/lockfileWorkspace/properties/after_identity")
            .is_none(),
        "the lockfile workspace must not declare a post-fetch identity"
    );

    let mut cache_required = strings(schema.pointer("/$defs/cache/required"));
    cache_required.sort();
    assert_eq!(cache_required, ["cargo_home", "git", "registry"]);

    let required: Vec<String> = strings(schema.pointer("/required"));
    let properties = schema
        .pointer("/properties")
        .and_then(Value::as_object)
        .expect("schema properties");
    for fixture in [
        valid_request(),
        cargo_acquisition_request(),
        cargo_verification_request(),
    ] {
        let object = fixture.as_object().expect("fixture object");
        for field in &required {
            assert!(
                object.contains_key(field),
                "accepted fixture is missing required schema field `{field}`"
            );
        }
        for key in object.keys() {
            assert!(
                properties.contains_key(key),
                "accepted fixture field `{key}` is not a declared schema property"
            );
        }
    }
}

fn run_runner(request: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kvist-sandbox-runner"))
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
