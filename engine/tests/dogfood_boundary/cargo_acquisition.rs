//! Dogfood boundary evidence for mediated Cargo acquisition and verification.
//!
//! The first group exercises the *real* sandbox runner end to end (argv, source
//! policy, source-aware network guard, safe cache promotion, and offline locked
//! verification) with real filesystem assertions. Because the runner is still
//! fail-closed until OS Bubblewrap integration lands, the cases that require
//! actual execution, network transport, or descriptor-relative cache promotion
//! remain intentionally red; they must not be weakened into passing planning
//! checks. The rejection cases that the strict parser enforces (build-script
//! subcommands and source-policy violations) pass today.
//!
//! The second group is host-independent planning coverage that runs directly
//! against the engine's acquisition plan builder and the runner's independent
//! parser. These are additive and never substitute for the real-runner cases
//! above.

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::support::{
    base_request, output_text, run_runner_request, sandbox_fixture, sha256_bytes,
};

fn canonical_crates_io() -> Value {
    json!({
        "kind": "cargo-registry",
        "name": "crates-io",
        "index_origin": "https://index.crates.io/",
        "download_origin": "https://static.crates.io/",
        "identity": kvist_sandbox_runner::validation::registry_identity(
            "crates-io",
            "https://index.crates.io/",
            "https://static.crates.io/",
        )
    })
}

/// Builds a v1 `<cargo> fetch` acquisition request over the real fixture roots:
/// a Cargo toolchain rooted at an immutable directory containing `cargo`, a
/// writable Cargo home, target scratch, an isolated writable lockfile workspace,
/// and pre-execution promotion intent (no post-fetch state).
fn acquisition_request(fixture: &std::path::Path, argv: &[&str]) -> Value {
    let mut request = base_request(fixture, "dependency-acquisition", argv);
    let toolchain_identity = sha256_bytes(b"cargo-toolchain-root");
    let cargo_home_identity = sha256_bytes(b"attempt-cargo-home");
    let scratch_identity = sha256_bytes(b"acquisition-scratch");
    let lockfile_before = sha256_bytes(b"lockfile-before");

    request["working_directory"] = json!("/workspace/lockfile");
    request["environment"] = json!({
        "HOME": "/workspace/scratch/home",
        "PATH": "/usr/bin",
        "CARGO_HOME": "/workspace/cargo-home",
        "CARGO_TARGET_DIR": "/workspace/scratch/target",
        "CARGO_NET_GIT_FETCH_WITH_CLI": "false"
    });
    request["network"] = json!({
        "mode": "package-sources",
        "allowed_sources": [canonical_crates_io()]
    });
    request["resources"]["max_cache_bytes"] = json!(1_048_576);
    request["identities"]["toolchain"] = json!(toolchain_identity);
    request["grants"] = json!([
        {
            "source": fixture.join("toolchain").canonicalize().expect("toolchain root"),
            "destination": "/workspace/toolchain",
            "access": "read-only",
            "purpose": "toolchain",
            "identity": toolchain_identity
        },
        {
            "source": fixture.join("cargo-home").canonicalize().expect("cargo home"),
            "destination": "/workspace/cargo-home",
            "access": "read-write",
            "purpose": "dependency-cache",
            "identity": cargo_home_identity
        },
        {
            "source": fixture.join("scratch").canonicalize().expect("scratch"),
            "destination": "/workspace/scratch",
            "access": "read-write",
            "purpose": "scratch",
            "identity": scratch_identity
        },
        {
            "source": fixture.join("lockfile").canonicalize().expect("lockfile"),
            "destination": "/workspace/lockfile",
            "access": "read-write",
            "purpose": "lockfile",
            "identity": lockfile_before
        }
    ]);
    request["toolchain"] = json!({
        "kind": "cargo",
        "identity": toolchain_identity,
        "root": "/workspace/toolchain",
        "cargo": "/workspace/toolchain/cargo"
    });
    request["cache"] = json!({
        "cargo_home": "/workspace/cargo-home",
        "registry": "/workspace/cargo-home/registry",
        "git": "/workspace/cargo-home/git",
        "writable": {
            "destination": "/workspace/cargo-home",
            "identity": cargo_home_identity
        },
        "lockfile": {
            "destination": "/workspace/lockfile",
            "before_identity": lockfile_before
        },
        "promotion": {
            "enabled": false,
            "source": "/workspace/cargo-home"
        }
    });
    request["scratch"] = json!({
        "destination": "/workspace/scratch",
        "identity": scratch_identity
    });
    request
}

fn verification_request(fixture: &std::path::Path, argv: &[&str]) -> Value {
    let mut request = base_request(fixture, "verification", argv);
    let toolchain_identity = sha256_bytes(b"cargo-toolchain-root");
    let approved_identity = sha256_bytes(b"approved-cargo-home");
    let scratch_identity = sha256_bytes(b"verification-scratch");
    let workspace_identity = sha256_bytes(b"verification-workspace");

    request["working_directory"] = json!("/workspace/component");
    request["environment"] = json!({
        "HOME": "/workspace/scratch/home",
        "PATH": "/usr/bin",
        "CARGO_HOME": "/workspace/cargo-home",
        "CARGO_TARGET_DIR": "/workspace/scratch/target",
        "CARGO_NET_OFFLINE": "true"
    });
    request["resources"]["max_cache_bytes"] = json!(1_048_576);
    request["identities"]["toolchain"] = json!(toolchain_identity);
    request["grants"] = json!([
        {
            "source": fixture.join("toolchain").canonicalize().expect("toolchain root"),
            "destination": "/workspace/toolchain",
            "access": "read-only",
            "purpose": "toolchain",
            "identity": toolchain_identity
        },
        {
            "source": fixture.join("project-cache").canonicalize().expect("project cache"),
            "destination": "/workspace/cargo-home",
            "access": "read-only",
            "purpose": "dependency-cache",
            "identity": approved_identity
        },
        {
            "source": fixture.join("scratch").canonicalize().expect("scratch"),
            "destination": "/workspace/scratch",
            "access": "read-write",
            "purpose": "scratch",
            "identity": scratch_identity
        },
        {
            "source": fixture.join("component").canonicalize().expect("component"),
            "destination": "/workspace/component",
            "access": "read-only",
            "purpose": "verification",
            "identity": workspace_identity
        }
    ]);
    request["toolchain"] = json!({
        "kind": "cargo",
        "identity": toolchain_identity,
        "root": "/workspace/toolchain",
        "cargo": "/workspace/toolchain/cargo"
    });
    request["cache"] = json!({
        "cargo_home": "/workspace/cargo-home",
        "registry": "/workspace/cargo-home/registry",
        "git": "/workspace/cargo-home/git",
        "approved": {
            "destination": "/workspace/cargo-home",
            "identity": approved_identity
        }
    });
    request["scratch"] = json!({
        "destination": "/workspace/scratch",
        "identity": scratch_identity
    });
    request
}

fn cargo_fixture() -> tempfile::TempDir {
    let fixture = sandbox_fixture();
    let root = fixture.path();
    fs::create_dir(root.join("toolchain")).expect("create toolchain root");
    fs::create_dir(root.join("cargo-home")).expect("create Cargo home");
    fs::create_dir(root.join("lockfile")).expect("create lockfile workspace");
    fs::create_dir(root.join("project-cache")).expect("create project cache");
    fs::create_dir_all(root.join("component")).expect("create verification workspace");
    fs::write(root.join("lockfile/Cargo.lock"), "# lockfile\n").expect("write lockfile");
    write_fake_cargo(
        root,
        r#"#!/usr/bin/bash
set -eu
printf '%s\n' "$@" > /workspace/scratch/cargo-argv
printf '%s\n' "$CARGO_HOME" > /workspace/scratch/cargo-home
printf '%s\n' "${HOME-unset}" > /workspace/scratch/home
"#,
    );
    fixture
}

fn write_fake_cargo(root: &std::path::Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let cargo = root.join("toolchain/cargo");
    fs::write(&cargo, body).expect("write fake Cargo");
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).expect("make fake Cargo exec");
}

/// Enables pre-execution promotion intent. The post-fetch manifest, checksums,
/// and lockfile-after identity are derived by the runner integration and are not
/// part of the request.
fn enable_cache_promotion(request: &mut Value) {
    request["cache"]["promotion"]["enabled"] = json!(true);
}

fn serve_one_http_request(listener: TcpListener) -> thread::JoinHandle<Option<String>> {
    thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("make controlled source listener nonblocking");
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = [0_u8; 1024];
                    let count = stream.read(&mut request).expect("read source request");
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
                        .expect("write source response");
                    return Some(String::from_utf8_lossy(&request[..count]).into_owned());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept controlled source request: {error}"),
            }
        }
        None
    })
}

fn create_promotion_sentinel(fixture: &std::path::Path) -> Vec<u8> {
    let bytes = b"approved-cache-before-attempt\n".to_vec();
    fs::write(fixture.join("project-cache/sentinel"), &bytes).expect("write promotion sentinel");
    bytes
}

fn assert_no_cache_promotion(fixture: &std::path::Path, sentinel: &[u8]) {
    assert_eq!(
        fs::read(fixture.join("project-cache/sentinel")).expect("read promotion sentinel"),
        sentinel
    );
    assert!(
        !fixture.join("project-cache/crate").exists(),
        "an invalid cache must not enter the project cache destination"
    );
}

// -- Real-runner mediated acquisition cases -----------------------------------

#[test]
fn acquisition_request_separates_attempt_cache_from_project_promotion_destination() {
    let fixture = cargo_fixture();
    let request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    let grants = request["grants"].as_array().expect("cache grants");
    let writable = grants
        .iter()
        .find(|grant| grant["destination"] == "/workspace/cargo-home")
        .expect("writable Cargo-home grant");
    let toolchain = grants
        .iter()
        .find(|grant| grant["destination"] == "/workspace/toolchain")
        .expect("toolchain root grant");

    assert_eq!(writable["access"], "read-write");
    assert_eq!(writable["purpose"], "dependency-cache");
    assert_eq!(toolchain["access"], "read-only");
    // The immutable generation parent is provider-owned and never named in the
    // request; the promotion binds only the real writable Cargo home as source.
    assert_eq!(
        request
            .pointer("/cache/promotion/source")
            .and_then(Value::as_str),
        Some("/workspace/cargo-home")
    );
    assert!(request.pointer("/cache/promotion/destination").is_none());
    assert!(request.pointer("/cache/approved").is_none());
}

#[test]
fn acquisition_allows_only_fetch_metadata_with_attempt_local_cargo_directories() {
    let fixture = cargo_fixture();
    let request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);

    let output = run_runner_request(&request);

    assert!(
        output.status.success(),
        "approved Cargo acquisition failed: {}",
        output_text(&output)
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/cargo-argv")).expect("read Cargo argv"),
        "fetch\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/cargo-home")).expect("read CARGO_HOME"),
        "/workspace/cargo-home\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/home")).expect("read HOME"),
        "/workspace/scratch/home\n"
    );
}

#[test]
fn acquisition_rejects_commands_that_can_execute_build_scripts() {
    let fixture = cargo_fixture();
    for subcommand in ["build", "check", "test", "run", "rustc"] {
        let request =
            acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", subcommand]);
        let output = run_runner_request(&request);
        assert!(
            !output.status.success(),
            "network-enabled acquisition must reject `cargo {subcommand}`"
        );
        assert!(
            output_text(&output).contains("acquisition")
                || output_text(&output).contains("Cargo")
                || output_text(&output).contains("argv"),
            "rejection for `cargo {subcommand}` must be actionable: {}",
            output_text(&output)
        );
    }
}

#[test]
fn source_policy_accepts_canonical_crates_io_and_exact_additional_sources_only() {
    let fixture = cargo_fixture();
    let valid = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    let mut cases = Vec::new();

    let mut substituted_crates_io = valid.clone();
    substituted_crates_io["network"]["allowed_sources"][0]["download_origin"] =
        json!("https://mirror.invalid/crates/");
    cases.push((
        "substituted crates.io download origin",
        substituted_crates_io,
    ));

    let mut incomplete_registry = valid.clone();
    incomplete_registry["network"]["allowed_sources"] = json!([{
        "kind": "cargo-registry",
        "name": "private",
        "index_origin": "https://registry.example.invalid/index/",
        "identity": sha256_bytes(b"private")
    }]);
    cases.push((
        "registry without exact download origin",
        incomplete_registry,
    ));

    let mut moving_git_reference = valid.clone();
    moving_git_reference["network"]["allowed_sources"] = json!([{
        "kind": "cargo-git",
        "repository": "https://git.example.invalid/dependency.git",
        "branch": "main",
        "identity": sha256_bytes(b"moving-git")
    }]);
    cases.push(("mutable Git branch", moving_git_reference));

    let mut missing_git_repository = valid.clone();
    missing_git_repository["network"]["allowed_sources"] = json!([{
        "kind": "cargo-git",
        "revision": "0123456789abcdef0123456789abcdef01234567",
        "identity": sha256_bytes(b"git-without-url")
    }]);
    cases.push((
        "Git revision without exact repository URL",
        missing_git_repository,
    ));

    let mut model_network = valid;
    model_network["network"]["allowed_sources"] = json!([{
        "kind": "model-api",
        "origin": "https://model.example.invalid/",
        "identity": sha256_bytes(b"model-network")
    }]);
    cases.push((
        "model transport disguised as package acquisition",
        model_network,
    ));

    for (name, request) in cases {
        let output = run_runner_request(&request);
        assert!(!output.status.success(), "{name} must be rejected");
        assert!(
            !output_text(&output).trim().is_empty(),
            "{name} must produce an actionable source-policy diagnostic"
        );
    }
}

#[test]
fn immutable_git_source_requires_exact_url_and_revision() {
    let fixture = cargo_fixture();
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    request["network"]["allowed_sources"] = json!([{
        "kind": "cargo-git",
        "repository": "https://git.example.invalid/dependency.git",
        "revision": "0123456789abcdef0123456789abcdef01234567",
        "identity": kvist_sandbox_runner::validation::git_identity(
            "https://git.example.invalid/dependency.git",
            "0123456789abcdef0123456789abcdef01234567",
        )
    }]);

    let output = run_runner_request(&request);

    assert!(
        output.status.success(),
        "exact immutable Git source policy should validate and execute: {}",
        output_text(&output)
    );
}

#[test]
fn source_aware_network_guard_allows_only_the_declared_origin() {
    let allowed = TcpListener::bind("127.0.0.1:0").expect("bind allowed source");
    let allowed_port = allowed.local_addr().expect("allowed source address").port();
    let allowed_request = serve_one_http_request(allowed);
    let disallowed = TcpListener::bind("127.0.0.1:0").expect("bind disallowed source");
    disallowed
        .set_nonblocking(true)
        .expect("make disallowed source nonblocking");
    let disallowed_port = disallowed
        .local_addr()
        .expect("disallowed source address")
        .port();
    let fixture = cargo_fixture();
    write_fake_cargo(
        fixture.path(),
        &format!(
            r#"#!/usr/bin/bash
set -eu
printf started > /workspace/scratch/source-guard-started
/usr/bin/curl --fail --silent --show-error http://127.0.0.1:{allowed_port}/index/allowed > /workspace/scratch/allowed-response
printf allowed > /workspace/scratch/allowed-origin-reached
if /usr/bin/curl --fail --silent --show-error http://127.0.0.1:{disallowed_port}/undeclared; then
  printf connected > /workspace/scratch/disallowed-origin-reached
  exit 70
fi
printf source-denied > /workspace/scratch/disallowed-origin-denied
printf 'undeclared package source denied\n' >&2
exit 73
"#
        ),
    );
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    request["network"]["allowed_sources"] = json!([{
        "kind": "cargo-registry",
        "name": "controlled-local-registry",
        "index_origin": format!("http://127.0.0.1:{allowed_port}/index/"),
        "download_origin": format!("http://127.0.0.1:{allowed_port}/crates/"),
        "identity": sha256_bytes(b"controlled-local-registry")
    }]);

    let output = run_runner_request(&request);

    assert!(
        !output.status.success(),
        "the undeclared origin attempt must fail the acquisition"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/source-guard-started"))
            .expect("read source guard started marker"),
        "started"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/allowed-origin-reached"))
            .expect("read allowed-origin marker"),
        "allowed"
    );
    let allowed_http = allowed_request
        .join()
        .expect("join allowed source")
        .expect("allowed source was not reached");
    assert!(
        allowed_http.starts_with("GET /index/allowed "),
        "unexpected allowed-source request: {allowed_http:?}"
    );
    thread::sleep(Duration::from_millis(50));
    assert!(
        disallowed.accept().is_err(),
        "the source-aware guard connected to an undeclared origin"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/disallowed-origin-denied"))
            .expect("read denied-origin marker"),
        "source-denied"
    );
    assert!(
        !fixture
            .path()
            .join("scratch/disallowed-origin-reached")
            .exists()
    );
    assert!(
        output_text(&output).contains("package source denied"),
        "source-aware rejection must identify the denied origin class: {}",
        output_text(&output)
    );
}

#[test]
fn cache_symlink_is_rejected_without_promotion() {
    use std::os::unix::fs::symlink;

    let fixture = cargo_fixture();
    let sentinel = create_promotion_sentinel(fixture.path());
    fs::write(fixture.path().join("cargo-home/one"), "one").expect("write cache file");
    symlink(
        fixture.path().join("lockfile/Cargo.lock"),
        fixture.path().join("cargo-home/link"),
    )
    .expect("create cache link");
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    enable_cache_promotion(&mut request);

    let output = run_runner_request(&request);

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("link")
            || output_text(&output).contains("file")
            || output_text(&output).contains("bound"),
        "cache rejection must identify the unsafe or oversized input: {}",
        output_text(&output)
    );
    assert_no_cache_promotion(fixture.path(), &sentinel);
}

#[test]
fn cache_hard_link_is_rejected_without_promotion() {
    let fixture = cargo_fixture();
    let sentinel = create_promotion_sentinel(fixture.path());
    fs::write(fixture.path().join("cargo-home/one"), "one").expect("write cache file");
    fs::hard_link(
        fixture.path().join("cargo-home/one"),
        fixture.path().join("cargo-home/hard-link"),
    )
    .expect("create cache hard link");
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    enable_cache_promotion(&mut request);

    let output = run_runner_request(&request);

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("link")
            || output_text(&output).contains("multiple")
            || output_text(&output).contains("file"),
        "cache hard-link rejection must be explicit: {}",
        output_text(&output)
    );
    assert_no_cache_promotion(fixture.path(), &sentinel);
}

#[test]
fn cache_non_regular_file_is_rejected_without_promotion() {
    use std::os::unix::net::UnixListener;

    let fixture = cargo_fixture();
    let sentinel = create_promotion_sentinel(fixture.path());
    fs::write(fixture.path().join("cargo-home/one"), "one").expect("write cache file");
    let _socket =
        UnixListener::bind(fixture.path().join("cargo-home/socket")).expect("create cache socket");
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    enable_cache_promotion(&mut request);

    let output = run_runner_request(&request);

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("regular")
            || output_text(&output).contains("special")
            || output_text(&output).contains("file"),
        "cache special-file rejection must be explicit: {}",
        output_text(&output)
    );
    assert_no_cache_promotion(fixture.path(), &sentinel);
}

#[test]
fn cache_file_count_bound_is_rejected_without_promotion() {
    let fixture = cargo_fixture();
    let sentinel = create_promotion_sentinel(fixture.path());
    for name in ["one", "two", "three"] {
        fs::write(fixture.path().join("cargo-home").join(name), name)
            .expect("write bounded cache file");
    }
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    enable_cache_promotion(&mut request);
    request["resources"]["max_files"] = json!(2);

    let output = run_runner_request(&request);

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("file")
            && (output_text(&output).contains("count")
                || output_text(&output).contains("bound")
                || output_text(&output).contains("limit")),
        "cache file-count rejection must identify the exceeded bound: {}",
        output_text(&output)
    );
    assert_no_cache_promotion(fixture.path(), &sentinel);
}

#[test]
fn cache_file_size_bound_is_rejected_without_promotion() {
    let fixture = cargo_fixture();
    let sentinel = create_promotion_sentinel(fixture.path());
    fs::write(fixture.path().join("cargo-home/crate"), b"oversized")
        .expect("write oversized cache file");
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    enable_cache_promotion(&mut request);
    request["resources"]["max_file_bytes"] = json!(4);

    let output = run_runner_request(&request);

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("file")
            && (output_text(&output).contains("size")
                || output_text(&output).contains("bound")
                || output_text(&output).contains("limit")),
        "cache file-size rejection must identify the exceeded bound: {}",
        output_text(&output)
    );
    assert_no_cache_promotion(fixture.path(), &sentinel);
}

#[test]
fn aggregate_cache_size_bound_is_rejected_without_promotion() {
    let fixture = cargo_fixture();
    let sentinel = create_promotion_sentinel(fixture.path());
    fs::write(fixture.path().join("cargo-home/one"), b"1234").expect("write first cache file");
    fs::write(fixture.path().join("cargo-home/two"), b"5678").expect("write second cache file");
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    enable_cache_promotion(&mut request);
    request["resources"]["max_cache_bytes"] = json!(6);

    let output = run_runner_request(&request);

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("cache")
            && (output_text(&output).contains("size")
                || output_text(&output).contains("bound")
                || output_text(&output).contains("limit")),
        "aggregate cache-size rejection must identify the exceeded bound: {}",
        output_text(&output)
    );
    assert_no_cache_promotion(fixture.path(), &sentinel);
}

#[test]
fn cache_checksum_mismatch_is_rejected_without_promotion() {
    let fixture = cargo_fixture();
    let sentinel = create_promotion_sentinel(fixture.path());
    fs::write(fixture.path().join("cargo-home/crate"), b"downloaded crate")
        .expect("write cache file");
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    enable_cache_promotion(&mut request);
    // The runner derives and verifies the promotion manifest checksums after the
    // fetch; a mismatch between the derived digest and the staged bytes must
    // prevent promotion. This path requires OS integration.
    request["cache"]["writable"]["identity"] =
        serde_json::Value::String(sha256_bytes(b"attempt-cargo-home"));

    let output = run_runner_request(&request);

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("checksum")
            || output_text(&output).contains("digest")
            || output_text(&output).contains("identity"),
        "cache checksum rejection must be explicit: {}",
        output_text(&output)
    );
    assert_no_cache_promotion(fixture.path(), &sentinel);
}

#[test]
fn concurrent_project_cache_change_prevents_promotion_without_overwrite() {
    let fixture = cargo_fixture();
    let original = create_promotion_sentinel(fixture.path());
    write_fake_cargo(
        fixture.path(),
        r#"#!/usr/bin/bash
set -eu
printf started > /workspace/scratch/acquisition-started
while test ! -e /workspace/scratch/continue-acquisition; do sleep 0.01; done
printf 'downloaded crate' > /workspace/cargo-home/crate
"#,
    );
    fs::write(fixture.path().join("cargo-home/crate"), b"downloaded crate")
        .expect("write expected promotion input");
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    enable_cache_promotion(&mut request);
    let request_thread = thread::spawn(move || run_runner_request(&request));
    let started = fixture.path().join("scratch/acquisition-started");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !started.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(started.exists(), "controlled acquisition did not start");
    let concurrent = b"concurrent project-cache update\n";
    fs::write(fixture.path().join("project-cache/sentinel"), concurrent)
        .expect("write concurrent project-cache change");
    fs::write(fixture.path().join("scratch/continue-acquisition"), b"go")
        .expect("release controlled acquisition");

    let output = request_thread.join().expect("join acquisition request");

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("concurrent")
            || output_text(&output).contains("precondition")
            || output_text(&output).contains("identity"),
        "concurrent project-cache refusal must be explicit: {}",
        output_text(&output)
    );
    assert_ne!(original, concurrent);
    assert_eq!(
        fs::read(fixture.path().join("project-cache/sentinel"))
            .expect("read concurrent project-cache sentinel"),
        concurrent
    );
    assert!(
        !fixture.path().join("project-cache/crate").exists(),
        "promotion must not partially update a concurrently changed destination"
    );
}

#[test]
fn valid_cache_promotion_preserves_existing_project_entries_and_adds_manifest_content() {
    let fixture = cargo_fixture();
    let sentinel = create_promotion_sentinel(fixture.path());
    fs::write(fixture.path().join("cargo-home/crate"), b"validated crate")
        .expect("write valid cache file");
    let mut request = acquisition_request(fixture.path(), &["/workspace/toolchain/cargo", "fetch"]);
    enable_cache_promotion(&mut request);

    let output = run_runner_request(&request);

    assert!(
        output.status.success(),
        "valid cache promotion failed: {}",
        output_text(&output)
    );
    assert_eq!(
        fs::read(fixture.path().join("project-cache/sentinel"))
            .expect("read preserved project-cache sentinel"),
        sentinel
    );
    assert_eq!(
        fs::read(fixture.path().join("project-cache/crate")).expect("read promoted cache content"),
        b"validated crate"
    );
}

#[test]
fn verification_is_network_denied_locked_and_uses_a_read_only_cache() {
    let fixture = cargo_fixture();
    fs::write(fixture.path().join("project-cache/crate"), "approved")
        .expect("write approved cache entry");
    write_fake_cargo(
        fixture.path(),
        r#"#!/usr/bin/bash
set -eu
test "${CARGO_NET_OFFLINE-unset}" = "true"
test "$1" = "test"
test "$2" = "--locked"
! printf changed > /workspace/cargo-home/crate
printf verified > /workspace/scratch/verification
"#,
    );
    let request = verification_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "test", "--locked"],
    );

    let output = run_runner_request(&request);

    assert!(
        output.status.success(),
        "offline locked verification failed: {}",
        output_text(&output)
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("project-cache/crate"))
            .expect("read cache after verification"),
        "approved"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/verification"))
            .expect("read verification marker"),
        "verified"
    );
}

// -- Host-independent planning coverage (additive; not a substitute) ----------

mod planning {
    use kvist::{
        acquisition::{
            ContentIdentity, HostPath, build_acquisition_plan, build_verification_plan,
            git_identity, registry_identity,
        },
        config::{AcquisitionCacheBounds, AcquisitionConfig, PackageSource},
    };
    use kvist_sandbox_runner::validation;

    fn source_config() -> AcquisitionConfig {
        AcquisitionConfig {
            cache_bounds: AcquisitionCacheBounds {
                max_files: 256,
                max_file_bytes: 1_048_576,
                max_cache_bytes: 16 * 1_048_576,
            },
            additional_sources: vec![PackageSource::CargoRegistry {
                name: "private".to_owned(),
                index_origin: "https://registry.example.invalid/index/".to_owned(),
                download_origin: "https://registry.example.invalid/crates/".to_owned(),
            }],
        }
    }

    fn host(path: &str) -> HostPath {
        HostPath::new(path).expect("lexical host path")
    }

    #[test]
    fn engine_and_runner_independently_agree_on_source_identity_golden_vectors() {
        let engine_registry = registry_identity(
            "private",
            "https://registry.example.invalid/index/",
            "https://registry.example.invalid/crates/",
        );
        let runner_registry = validation::registry_identity(
            "private",
            "https://registry.example.invalid/index/",
            "https://registry.example.invalid/crates/",
        );
        assert_eq!(engine_registry.as_str(), runner_registry);
        assert_eq!(
            engine_registry.as_str(),
            "sha256:4d3fc763437bea9217f14b2bc9b5201dbf78ea13ac0ef390497cc54896ab9486"
        );

        let engine_git = git_identity(
            "https://git.example.invalid/dependency.git",
            "0123456789abcdef0123456789abcdef01234567",
        );
        let runner_git = validation::git_identity(
            "https://git.example.invalid/dependency.git",
            "0123456789abcdef0123456789abcdef01234567",
        );
        assert_eq!(engine_git.as_str(), runner_git);
        assert_eq!(
            engine_git.as_str(),
            "sha256:3405bc5e24e0f91ff11d2271a69f9c9b7a0547adb6acace6d10638fee39d2fe6"
        );
    }

    #[test]
    fn acquisition_uses_real_cargo_home_and_writable_lockfile_workspace() {
        let plan = build_acquisition_plan(
            host("/workspace/acquire-attempt"),
            host("/opt/toolchain/cargo"),
            ContentIdentity::from_bytes(b"cargo"),
            ContentIdentity::from_bytes(b"lock-before"),
            "/usr/bin".to_owned(),
            &source_config(),
        )
        .expect("valid mediated acquisition plan");

        assert_eq!(plan.argv(), ["/workspace/toolchain/cargo", "fetch"]);
        assert_eq!(
            plan.cargo_home_host().as_path().to_str(),
            Some("/workspace/acquire-attempt/cargo-home")
        );
        assert_eq!(plan.cargo_home_sandbox().as_str(), "/workspace/cargo-home");
        assert_eq!(
            plan.environment()
                .get("CARGO_TARGET_DIR")
                .map(String::as_str),
            Some("/workspace/scratch/target")
        );
        assert_eq!(
            plan.environment()
                .get("CARGO_NET_GIT_FETCH_WITH_CLI")
                .map(String::as_str),
            Some("false")
        );
        assert!(!plan.argv().contains(&"--locked".to_owned()));

        let result = plan
            .bind_result(ContentIdentity::from_bytes(b"lock-after"), Vec::new())
            .expect("lockfile update is an accepted acquisition result");
        assert_ne!(
            result.lockfile_before_identity(),
            result.lockfile_after_identity(),
            "new dependency resolution is permitted to update the isolated lockfile"
        );
        assert_eq!(result.cargo_home_source(), plan.cargo_home_host());
    }

    #[test]
    fn verification_uses_approved_cargo_home_read_only_and_locked_offline_test() {
        let plan = build_verification_plan(
            host("/workspace/verification-attempt"),
            host("/opt/toolchain/cargo"),
            ContentIdentity::from_bytes(b"cargo"),
            host("/var/project-cache/generation-immutable"),
            "/usr/bin".to_owned(),
        )
        .expect("valid offline verification plan");
        assert_eq!(
            plan.argv(),
            ["/workspace/toolchain/cargo", "test", "--locked"]
        );
        assert_eq!(
            plan.environment().get("CARGO_HOME").map(String::as_str),
            Some("/workspace/cargo-home")
        );
        assert_eq!(
            plan.environment()
                .get("CARGO_NET_OFFLINE")
                .map(String::as_str),
            Some("true")
        );
        assert_ne!(
            plan.approved_cargo_home_host(),
            plan.scratch_host(),
            "verification target scratch is distinct from its read-only Cargo home"
        );
    }
}
