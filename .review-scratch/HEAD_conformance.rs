//! Conformance tests for the redefined version-one sandbox protocol.
//!
//! These exercise the runner's own parser and validator directly (accepted
//! strict shape, canonical serialization, malformed/oversized/unknown values,
//! and legacy rejection) plus the fail-closed executable boundary. They do not
//! require Bubblewrap; production enforcement evidence belongs to the later
//! integration task and intentionally remains outside this file.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use kvist_sandbox_runner::protocol::SandboxRequest;
use kvist_sandbox_runner::validation::{self, ProtocolError};

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
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

fn encode(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("encode request")
}

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
    // Environment is a sorted map, so its serialized order is stable.
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
    // The new destination is a child of the writable `tests` grant and must be
    // rejected as an overlapping mount.
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
    request["network"]["allowed_sources"] = json!([{
        "kind": "cargo-registry",
        "name": "crates-io",
        "index_origin": "https://index.crates.io/",
        "download_origin": "https://static.crates.io/",
        "identity": digest(b"crates-io")
    }]);
    let error =
        validation::parse_and_validate(&encode(&request)).expect_err("deny+sources rejected");
    assert!(error.to_string().contains("allowed_sources"));
}

fn cargo_acquisition_request() -> Value {
    json!({
        "protocol": "kvist-sandbox-request-v1",
        "protocol_version": 1,
        "phase": "dependency-acquisition",
        "argv": ["/workspace/toolchain/cargo", "fetch", "--locked"],
        "working_directory": "/workspace/component",
        "environment": {
            "HOME": "/workspace/home",
            "PATH": "/usr/bin",
            "CARGO_HOME": "/workspace/dependencies/cargo-home"
        },
        "network": {
            "mode": "package-sources",
            "allowed_sources": [{
                "kind": "cargo-registry",
                "name": "crates-io",
                "index_origin": "https://index.crates.io/",
                "download_origin": "https://static.crates.io/",
                "identity": digest(b"crates-io")
            }]
        },
        "resources": {
            "wall_time_ms": 5000,
            "max_output_bytes": 65536,
            "max_processes": 16,
            "max_files": 256,
            "max_file_bytes": 1048576,
            "max_scratch_bytes": 1048576,
            "max_cache_bytes": 1048576
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
                "source": "/host/toolchain/cargo",
                "destination": "/workspace/toolchain/cargo",
                "access": "read-only",
                "purpose": "toolchain",
                "identity": digest(b"cargo")
            },
            {
                "source": "/host/cargo-home",
                "destination": "/workspace/dependencies/cargo-home",
                "access": "read-write",
                "purpose": "dependency-cache",
                "identity": digest(b"cargo-home")
            },
            {
                "source": "/host/attempt-cache",
                "destination": "/workspace/dependencies/attempt-cache",
                "access": "read-write",
                "purpose": "dependency-cache",
                "identity": digest(b"attempt-cache")
            },
            {
                "source": "/host/project-cache",
                "destination": "/workspace/dependencies/project-cache",
                "access": "read-only",
                "purpose": "dependency-cache",
                "identity": digest(b"project-cache")
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
            "kind": "cargo",
            "identity": digest(b"cargo"),
            "cargo": "/workspace/toolchain/cargo"
        },
        "cache": {
            "cargo_home": "/workspace/dependencies/cargo-home",
            "registry": "/workspace/dependencies/cargo-home/registry",
            "git": "/workspace/dependencies/cargo-home/git",
            "attempt": {
                "destination": "/workspace/dependencies/attempt-cache",
                "identity": digest(b"attempt-cache")
            },
            "promotion": {
                "enabled": false,
                "source": "/workspace/dependencies/attempt-cache",
                "destination": "/workspace/dependencies/project-cache",
                "expected_destination_identity": digest(b"project-cache"),
                "manifest": [{
                    "path": "crate",
                    "size": 4,
                    "checksum": digest(b"crate")
                }]
            }
        },
        "scratch": {
            "destination": "/workspace/scratch",
            "identity": digest(b"scratch")
        }
    })
}

#[test]
fn accepts_the_cargo_acquisition_shape() {
    let request = cargo_acquisition_request();
    validation::parse_and_validate(&encode(&request))
        .expect("cargo acquisition request must parse and validate");
}

#[test]
fn accepts_the_immutable_cargo_git_source() {
    let mut request = cargo_acquisition_request();
    request["network"]["allowed_sources"] = json!([{
        "kind": "cargo-git",
        "repository": "https://git.example.invalid/dependency.git",
        "revision": "0123456789abcdef0123456789abcdef01234567",
        "identity": digest(b"git")
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
        "allowed_sources": [{
            "kind": "cargo-registry",
            "name": "crates-io",
            "index_origin": "https://index.crates.io/",
            "identity": digest(b"crates-io")
        }]
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
    // Each u64 resource limit has an explicit safe maximum; a value one past the
    // maximum must be rejected rather than accepted or saturated.
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

#[test]
fn rejects_scratch_without_a_corresponding_grant() {
    let mut request = valid_request();
    // Remove the scratch grant (index 3), leaving the scratch block dangling.
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
    // A scratch grant must be read-write; forcing read-only breaks the required
    // scratch access invariant before the topology check even runs.
    request["grants"][3]["access"] = json!("read-only");
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("read-only scratch grant must reject");
    assert!(error.to_string().contains("scratch"));
}

#[test]
fn rejects_cache_attempt_without_a_corresponding_grant() {
    let mut request = cargo_acquisition_request();
    request["cache"]["attempt"]["destination"] = json!("/workspace/dependencies/unbacked");
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("unbacked cache attempt endpoint must reject");
    assert!(
        error.to_string().contains("cache attempt")
            && error.to_string().contains("does not correspond"),
        "diagnostic must be actionable: {error}"
    );
}

#[test]
fn rejects_cache_attempt_identity_mismatch() {
    let mut request = cargo_acquisition_request();
    request["cache"]["attempt"]["identity"] = json!(digest(b"mismatched-attempt"));
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("cache attempt identity mismatch must reject");
    assert!(error.to_string().contains("identity does not match"));
}

#[test]
fn rejects_cache_promotion_destination_identity_mismatch() {
    let mut request = cargo_acquisition_request();
    request["cache"]["promotion"]["expected_destination_identity"] =
        json!(digest(b"unexpected-destination"));
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("promotion destination identity mismatch must reject");
    assert!(
        error
            .to_string()
            .contains("expected_destination_identity does not match")
    );
}

#[test]
fn rejects_cache_promotion_source_not_the_attempt_endpoint() {
    let mut request = cargo_acquisition_request();
    // Point the promotion source at the read-only project cache instead of the
    // writable attempt endpoint.
    request["cache"]["promotion"]["source"] = json!("/workspace/dependencies/project-cache");
    let error = validation::parse_and_validate(&encode(&request))
        .expect_err("promotion source that is not the attempt endpoint must reject");
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
    // Either the size bound before parsing or the per-name bound must reject it.
    assert!(
        matches!(error, ProtocolError::TooLarge { .. })
            || error.to_string().contains("environment name")
    );
}

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
fn executable_fails_closed_on_a_valid_request() {
    let output = run_runner(&encode(&valid_request()));
    assert!(
        !output.status.success(),
        "a valid request must fail closed until enforcement is integrated"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Bubblewrap enforcement is not yet integrated"),
        "fail-closed diagnostic must be explicit: {stderr}"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn executable_probe_fails_closed_without_enforcement() {
    let output = Command::new(env!("CARGO_BIN_EXE_kvist-sandbox-runner"))
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .expect("run probe");
    assert!(!output.status.success(), "probe must fail closed");
    assert!(
        output.stdout.is_empty(),
        "unconfirmed probe must emit no JSON"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot be confirmed"),
        "probe diagnostic must be explicit"
    );
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

    // Phase enumerants mirror the parser's `Phase`.
    let mut phases = strings(schema.pointer("/properties/phase/enum"));
    phases.sort();
    assert_eq!(
        phases,
        ["authoring", "dependency-acquisition", "verification"]
    );

    // Network modes mirror the parser's `NetworkMode`.
    let mut modes = strings(schema.pointer("/$defs/network/properties/mode/enum"));
    modes.sort();
    assert_eq!(modes, ["deny", "package-sources"]);

    // Grant purposes mirror the parser's `Purpose`.
    let mut purposes = strings(schema.pointer("/$defs/grant/properties/purpose/enum"));
    purposes.sort();
    assert_eq!(
        purposes,
        [
            "authoring",
            "context",
            "dependency-cache",
            "scratch",
            "toolchain",
            "verification"
        ]
    );

    // Toolchain kinds mirror the parser's `Toolchain` variants.
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

    // Allowed source kinds mirror the parser's `AllowedSource` variants.
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

    // The optional cache-size resource bound is present.
    assert!(
        schema
            .pointer("/$defs/resources/properties/max_cache_bytes")
            .is_some()
    );

    // The rich cache required fields mirror the parser's `Cache`.
    let mut cache_required = strings(schema.pointer("/$defs/cache/required"));
    cache_required.sort();
    assert_eq!(cache_required, ["cargo_home", "git", "registry"]);

    // Every top-level required schema field is present in each accepted fixture,
    // and every fixture field is a declared schema property. This prevents the
    // schema and the accepted parser shapes from drifting apart.
    let required: Vec<String> = strings(schema.pointer("/required"));
    let properties = schema
        .pointer("/properties")
        .and_then(Value::as_object)
        .expect("schema properties");
    for fixture in [valid_request(), cargo_acquisition_request()] {
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
