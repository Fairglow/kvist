use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::support::{
    base_request, output_text, run_runner_request, sandbox_fixture, sha256_bytes, sha256_file,
};

fn canonical_crates_io() -> Value {
    json!({
        "kind": "cargo-registry",
        "name": "crates-io",
        "index_origin": "https://index.crates.io/",
        "download_origin": "https://static.crates.io/",
        "identity": sha256_bytes(b"canonical-crates-io")
    })
}

fn acquisition_request(fixture: &std::path::Path, argv: &[&str]) -> Value {
    let mut request = base_request(fixture, "dependency-acquisition", argv);
    request["resources"]["max_cache_bytes"] = json!(1_048_576);
    request["network"] = json!({
        "mode": "package-sources",
        "allowed_sources": [canonical_crates_io()]
    });
    request["environment"] = json!({
        "HOME": "/workspace/home",
        "PATH": "/usr/bin",
        "CARGO_HOME": "/workspace/dependencies/cargo-home",
        "CARGO_TARGET_DIR": "/workspace/scratch/target",
        "CARGO_NET_GIT_FETCH_WITH_CLI": "false"
    });
    request["grants"] = json!([
        request["grants"][0].clone(),
        request["grants"][2].clone(),
        {
            "source": fixture.join("toolchain/cargo").canonicalize().expect("fake cargo"),
            "destination": "/workspace/toolchain/cargo",
            "access": "read-only",
            "purpose": "toolchain",
            "identity": sha256_file(&fixture.join("toolchain/cargo"))
        },
        {
            "source": fixture.join("cargo-home").canonicalize().expect("cargo home"),
            "destination": "/workspace/dependencies/cargo-home",
            "access": "read-write",
            "purpose": "dependency-cache",
            "identity": sha256_bytes(b"attempt-cargo-home")
        },
        {
            "source": fixture.join("attempt-cache").canonicalize().expect("attempt cache"),
            "destination": "/workspace/dependencies/attempt-cache",
            "access": "read-write",
            "purpose": "dependency-cache",
            "identity": sha256_bytes(b"attempt-local-dependency-cache")
        },
        {
            "source": fixture.join("project-cache").canonicalize().expect("project cache"),
            "destination": "/workspace/dependencies/project-cache",
            "access": "read-only",
            "purpose": "dependency-cache",
            "identity": directory_identity(&fixture.join("project-cache"))
        },
        request["grants"][3].clone()
    ]);
    request["cache"] = json!({
        "cargo_home": "/workspace/dependencies/cargo-home",
        "registry": "/workspace/dependencies/cargo-home/registry",
        "git": "/workspace/dependencies/cargo-home/git",
        "attempt": {
            "destination": "/workspace/dependencies/attempt-cache",
            "identity": sha256_bytes(b"attempt-local-dependency-cache")
        },
        "promotion": {
            "enabled": false,
            "source": "/workspace/dependencies/attempt-cache",
            "destination": "/workspace/dependencies/project-cache",
            "expected_destination_identity": directory_identity(&fixture.join("project-cache")),
            "manifest": []
        }
    });
    request["toolchain"] = json!({
        "kind": "cargo",
        "identity": sha256_file(&fixture.join("toolchain/cargo")),
        "cargo": "/workspace/toolchain/cargo"
    });
    request
}

fn cargo_fixture() -> tempfile::TempDir {
    let fixture = sandbox_fixture();
    fs::create_dir(fixture.path().join("toolchain")).expect("create toolchain");
    fs::create_dir(fixture.path().join("cargo-home")).expect("create Cargo home");
    fs::create_dir(fixture.path().join("attempt-cache")).expect("create attempt cache");
    fs::create_dir(fixture.path().join("project-cache")).expect("create project cache");
    fs::write(
        fixture.path().join("toolchain/cargo"),
        r#"#!/usr/bin/bash
set -eu
printf '%s\n' "$@" > /workspace/scratch/cargo-argv
printf '%s\n' "$CARGO_HOME" > /workspace/scratch/cargo-home
printf '%s\n' "${HOME-unset}" > /workspace/scratch/home
"#,
    )
    .expect("write fake Cargo");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        fixture.path().join("toolchain/cargo"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make fake Cargo executable");
    fixture
}

fn directory_identity(root: &Path) -> String {
    fn collect(root: &Path, directory: &Path, entries: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut children = fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("read cache identity directory: {error}"))
            .map(|entry| entry.expect("read cache identity entry").path())
            .collect::<Vec<_>>();
        children.sort();
        for path in children {
            let metadata = fs::symlink_metadata(&path).expect("inspect cache identity entry");
            if metadata.file_type().is_dir() {
                collect(root, &path, entries);
            } else if metadata.file_type().is_file() {
                entries.push((
                    path.strip_prefix(root)
                        .expect("cache identity path below root")
                        .to_path_buf(),
                    fs::read(&path).expect("read cache identity file"),
                ));
            } else {
                entries.push((
                    path.strip_prefix(root)
                        .expect("cache identity path below root")
                        .to_path_buf(),
                    format!("mode:{:x}", std::os::unix::fs::MetadataExt::mode(&metadata))
                        .into_bytes(),
                ));
            }
        }
    }

    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    let mut canonical = Vec::new();
    for (path, bytes) in entries {
        canonical.extend_from_slice(path.as_os_str().as_encoded_bytes());
        canonical.push(0);
        canonical.extend_from_slice(bytes.len().to_string().as_bytes());
        canonical.push(0);
        canonical.extend_from_slice(&bytes);
        canonical.push(0xff);
    }
    sha256_bytes(&canonical)
}

fn enable_cache_promotion(request: &mut Value, fixture: &Path, paths: &[&str]) {
    request["cache"]["promotion"]["enabled"] = json!(true);
    request["cache"]["promotion"]["expected_destination_identity"] =
        json!(directory_identity(&fixture.join("project-cache")));
    request["cache"]["promotion"]["manifest"] = Value::Array(
        paths
            .iter()
            .map(|relative| {
                let path = fixture.join("attempt-cache").join(relative);
                json!({
                    "path": relative,
                    "size": fs::metadata(&path).expect("promotion file metadata").len(),
                    "checksum": sha256_file(&path)
                })
            })
            .collect(),
    );
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
        "an invalid cache must not enter the actual project-cache destination"
    );
}

#[test]
fn acquisition_request_separates_attempt_cache_from_project_promotion_destination() {
    let fixture = cargo_fixture();
    let request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    let grants = request["grants"].as_array().expect("cache grants");
    let attempt = grants
        .iter()
        .find(|grant| grant["destination"] == "/workspace/dependencies/attempt-cache")
        .expect("attempt-local cache grant");
    let project = grants
        .iter()
        .find(|grant| grant["destination"] == "/workspace/dependencies/project-cache")
        .expect("project-cache grant");

    assert_eq!(attempt["access"], "read-write");
    assert_eq!(project["access"], "read-only");
    assert_ne!(attempt["source"], project["source"]);
    assert_eq!(
        request
            .pointer("/cache/promotion/source")
            .and_then(Value::as_str),
        Some("/workspace/dependencies/attempt-cache")
    );
    assert_eq!(
        request
            .pointer("/cache/promotion/destination")
            .and_then(Value::as_str),
        Some("/workspace/dependencies/project-cache")
    );
}

#[test]
fn acquisition_allows_only_fetch_metadata_with_attempt_local_cargo_directories() {
    let fixture = cargo_fixture();
    let request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );

    let output = run_runner_request(&request);

    assert!(
        output.status.success(),
        "approved Cargo acquisition failed: {}",
        output_text(&output)
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/cargo-argv")).expect("read Cargo argv"),
        "fetch\n--locked\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/cargo-home")).expect("read CARGO_HOME"),
        "/workspace/dependencies/cargo-home\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/home")).expect("read HOME"),
        "/workspace/home\n"
    );
}

#[test]
fn acquisition_rejects_commands_that_can_execute_build_scripts() {
    let fixture = cargo_fixture();
    for subcommand in ["build", "check", "test", "run", "rustc"] {
        let request = acquisition_request(
            fixture.path(),
            &["/workspace/toolchain/cargo", subcommand, "--locked"],
        );
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
    let valid = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
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
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    request["network"]["allowed_sources"] = json!([{
        "kind": "cargo-git",
        "repository": "https://git.example.invalid/dependency.git",
        "revision": "0123456789abcdef0123456789abcdef01234567",
        "identity": sha256_bytes(b"exact-git-source")
    }]);

    let output = run_runner_request(&request);

    assert!(
        output.status.success(),
        "exact immutable Git source policy should validate: {}",
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
    fs::write(
        fixture.path().join("toolchain/cargo"),
        format!(
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
    )
    .expect("write source-aware fake Cargo");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        fixture.path().join("toolchain/cargo"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make source-aware fake Cargo executable");
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
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
    fs::write(fixture.path().join("attempt-cache/one"), "one").expect("write cache file");
    symlink(
        fixture.path().join("REQUIREMENTS.md"),
        fixture.path().join("attempt-cache/link"),
    )
    .expect("create cache link");
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    enable_cache_promotion(&mut request, fixture.path(), &["one"]);
    request["resources"]["max_files"] = json!(256);

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
    fs::write(fixture.path().join("attempt-cache/one"), "one").expect("write cache file");
    fs::hard_link(
        fixture.path().join("attempt-cache/one"),
        fixture.path().join("attempt-cache/hard-link"),
    )
    .expect("create cache hard link");
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    enable_cache_promotion(&mut request, fixture.path(), &["one"]);

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
    fs::write(fixture.path().join("attempt-cache/one"), "one").expect("write cache file");
    let _socket = UnixListener::bind(fixture.path().join("attempt-cache/socket"))
        .expect("create cache socket");
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    enable_cache_promotion(&mut request, fixture.path(), &["one"]);

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
        fs::write(fixture.path().join("attempt-cache").join(name), name)
            .expect("write bounded cache file");
    }
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    enable_cache_promotion(&mut request, fixture.path(), &["one", "two", "three"]);
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
    fs::write(fixture.path().join("attempt-cache/crate"), b"oversized")
        .expect("write oversized cache file");
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    enable_cache_promotion(&mut request, fixture.path(), &["crate"]);
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
    fs::write(fixture.path().join("attempt-cache/one"), b"1234").expect("write first cache file");
    fs::write(fixture.path().join("attempt-cache/two"), b"5678").expect("write second cache file");
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    enable_cache_promotion(&mut request, fixture.path(), &["one", "two"]);
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
    fs::write(
        fixture.path().join("attempt-cache/crate"),
        b"downloaded crate",
    )
    .expect("write cache file");
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    enable_cache_promotion(&mut request, fixture.path(), &["crate"]);
    request["cache"]["promotion"]["manifest"][0]["checksum"] =
        json!(sha256_bytes(b"forged checksum"));

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
    fs::write(
        fixture.path().join("toolchain/cargo"),
        r#"#!/usr/bin/bash
set -eu
printf started > /workspace/scratch/acquisition-started
while test ! -e /workspace/scratch/continue-acquisition; do sleep 0.01; done
printf 'downloaded crate' > /workspace/dependencies/attempt-cache/crate
"#,
    )
    .expect("write synchronized fake Cargo");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        fixture.path().join("toolchain/cargo"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make synchronized fake Cargo executable");
    fs::write(
        fixture.path().join("attempt-cache/crate"),
        b"downloaded crate",
    )
    .expect("write expected promotion input");
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    enable_cache_promotion(&mut request, fixture.path(), &["crate"]);
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
    fs::write(
        fixture.path().join("attempt-cache/crate"),
        b"validated crate",
    )
    .expect("write valid cache file");
    let mut request = acquisition_request(
        fixture.path(),
        &["/workspace/toolchain/cargo", "fetch", "--locked"],
    );
    enable_cache_promotion(&mut request, fixture.path(), &["crate"]);

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
    fs::write(
        fixture.path().join("toolchain/cargo"),
        r#"#!/usr/bin/bash
set -eu
test "${CARGO_NET_OFFLINE-unset}" = "true"
test "$1" = "test"
test "$2" = "--locked"
! printf changed > /workspace/dependencies/project-cache/crate
printf verified > /workspace/scratch/verification
"#,
    )
    .expect("write verification Cargo");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        fixture.path().join("toolchain/cargo"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make verification Cargo executable");
    let mut request = base_request(
        fixture.path(),
        "verification",
        &["/workspace/toolchain/cargo", "test", "--locked"],
    );
    request["environment"] = json!({
        "HOME": "/workspace/home",
        "PATH": "/usr/bin",
        "CARGO_HOME": "/workspace/dependencies/cargo-home",
        "CARGO_NET_OFFLINE": "true"
    });
    request["grants"] = json!([
        request["grants"][0].clone(),
        request["grants"][2].clone(),
        {
            "source": fixture.path().join("toolchain/cargo").canonicalize().expect("cargo"),
            "destination": "/workspace/toolchain/cargo",
            "access": "read-only",
            "purpose": "toolchain",
            "identity": sha256_file(&fixture.path().join("toolchain/cargo"))
        },
        {
            "source": fixture.path().join("cargo-home").canonicalize().expect("cargo home"),
            "destination": "/workspace/dependencies/cargo-home",
            "access": "read-only",
            "purpose": "dependency-cache",
            "identity": sha256_bytes(b"approved-cargo-home")
        },
        {
            "source": fixture.path().join("project-cache").canonicalize().expect("cache"),
            "destination": "/workspace/dependencies/project-cache",
            "access": "read-only",
            "purpose": "dependency-cache",
            "identity": directory_identity(&fixture.path().join("project-cache"))
        },
        request["grants"][3].clone()
    ]);
    request["cache"] = json!({
        "cargo_home": "/workspace/dependencies/cargo-home",
        "registry": "/workspace/dependencies/project-cache",
        "git": "/workspace/dependencies/project-cache",
        "approved": {
            "destination": "/workspace/dependencies/project-cache",
            "identity": directory_identity(&fixture.path().join("project-cache"))
        }
    });
    request["toolchain"] = json!({
        "kind": "cargo",
        "identity": sha256_file(&fixture.path().join("toolchain/cargo")),
        "cargo": "/workspace/toolchain/cargo"
    });

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
