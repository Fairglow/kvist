use std::{
    fs,
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::support::{
    base_request, output_text, probe_runner, repository_root, run_runner_request, sandbox_fixture,
    sha256_bytes, temporary_directory,
};

fn protected_bytes(fixture: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    [
        "REQUIREMENTS.md",
        "TODOS.yaml",
        "IMPL.md",
        ".git/index",
        ".kvist/evidence.json",
        "child/src/lib.rs",
    ]
    .into_iter()
    .map(|relative| {
        let path = fixture.join(relative);
        let bytes = fs::read(&path).expect("read protected fixture bytes");
        (path, bytes)
    })
    .collect()
}

fn assert_protected_bytes_unchanged(before: &[(std::path::PathBuf, Vec<u8>)]) {
    for (path, expected) in before {
        assert_eq!(
            fs::read(path).expect("read protected fixture after request"),
            expected.as_slice(),
            "protected bytes changed at {}",
            path.display()
        );
    }
}

#[test]
fn real_runner_probe_reports_version_one_bubblewrap_identity_and_capabilities() {
    let probe = probe_runner();

    assert_eq!(
        probe.get("protocol").and_then(Value::as_str),
        Some("kvist-sandbox-probe-v1")
    );
    assert_eq!(
        probe.get("protocol_version").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        probe.pointer("/backend/kind").and_then(Value::as_str),
        Some("bubblewrap")
    );
    for pointer in [
        "/runner/path",
        "/runner/digest",
        "/backend/path",
        "/backend/digest",
    ] {
        let value = probe
            .pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("probe must report {pointer}"));
        assert!(!value.is_empty(), "probe field {pointer} must be nonempty");
    }
    let runner_path = std::path::Path::new(
        probe
            .pointer("/runner/path")
            .and_then(Value::as_str)
            .expect("runner path"),
    )
    .canonicalize()
    .expect("canonical installed runner");
    assert!(
        !runner_path.starts_with(
            repository_root()
                .canonicalize()
                .expect("canonical worktree")
        ),
        "the approved runner must be installed outside the selected worktree"
    );
    for capability in ["mount", "network", "pid", "ipc", "uts", "user"] {
        assert_eq!(
            probe
                .pointer(&format!("/capabilities/namespaces/{capability}"))
                .and_then(Value::as_bool),
            Some(true),
            "probe must verify the {capability} namespace"
        );
    }
    assert_eq!(
        probe
            .pointer("/capabilities/new_session")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        probe
            .pointer("/capabilities/parent_death_signal")
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn strict_authoring_request_uses_typed_phase_argv_working_directory_and_grants() {
    let fixture = sandbox_fixture();
    let request = base_request(fixture.path(), "authoring", &["/usr/bin/true"]);
    let output = run_runner_request(&request);

    assert!(
        output.status.success(),
        "strict version-one authoring request failed: {}",
        output_text(&output)
    );
}

#[test]
fn runner_rejects_the_retired_component_mount_request_shape() {
    let fixture = sandbox_fixture();
    let legacy = json!({
        "protocol_version": 1,
        "program": "/usr/bin/true",
        "arguments": [],
        "working_directory": "/workspace/component",
        "network": "deny",
        "mounts": [{
            "source": fixture.path(),
            "destination": "/workspace/component",
            "access": "read-write"
        }],
        "environment": {},
        "context_files": ["/workspace/component/REQUIREMENTS.md"]
    });

    let output = run_runner_request(&legacy);

    assert!(!output.status.success(), "legacy request must be rejected");
    assert!(
        output_text(&output).contains("legacy")
            || output_text(&output).contains("unknown field")
            || output_text(&output).contains("missing field"),
        "legacy rejection must be actionable: {}",
        output_text(&output)
    );
}

#[test]
fn runner_rejects_untyped_or_ambiguous_request_fields() {
    let fixture = sandbox_fixture();
    let valid = base_request(fixture.path(), "authoring", &["/usr/bin/true"]);
    let mut cases = Vec::new();

    let mut unknown_phase = valid.clone();
    unknown_phase["phase"] = json!("implementation");
    cases.push(("unknown phase", unknown_phase));

    let mut scalar_command = valid.clone();
    scalar_command
        .as_object_mut()
        .expect("request object")
        .remove("argv");
    scalar_command["program"] = json!("/usr/bin/true");
    cases.push(("scalar program instead of argv", scalar_command));

    let mut relative_working_directory = valid.clone();
    relative_working_directory["working_directory"] = json!("tests");
    cases.push(("relative working directory", relative_working_directory));

    let mut unknown_purpose = valid.clone();
    unknown_purpose["grants"][0]["purpose"] = json!("component");
    cases.push(("unknown grant purpose", unknown_purpose));

    let mut unknown_access = valid.clone();
    unknown_access["grants"][0]["access"] = json!("write");
    cases.push(("unknown grant access", unknown_access));

    let mut unknown_field = valid;
    unknown_field["fallback_to_host"] = json!(true);
    cases.push(("unknown field", unknown_field));

    for (name, request) in cases {
        let output = run_runner_request(&request);
        assert!(
            !output.status.success(),
            "{name} must be rejected by the closed protocol"
        );
        assert!(
            !output_text(&output).trim().is_empty(),
            "{name} must produce an actionable diagnostic"
        );
    }
}

#[test]
fn authoring_can_write_only_the_task_scope_and_cannot_observe_protected_state() {
    let fixture = sandbox_fixture();
    let script = r#"set -eu
printf 'allowed\n' > tests/generated.rs
! printf 'changed\n' > REQUIREMENTS.md
for path in TODOS.yaml IMPL.md .git .kvist child; do
  test ! -e "$path"
done
test ! -e /workspace/home/.cargo
test ! -e /workspace/home/.gitconfig
"#;
    let request = base_request(
        fixture.path(),
        "authoring",
        &["/usr/bin/bash", "-c", script],
    );

    let output = run_runner_request(&request);

    assert!(
        output.status.success(),
        "authoring scope enforcement failed: {}",
        output_text(&output)
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("tests/generated.rs"))
            .expect("read allowed authoring output"),
        "allowed\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("REQUIREMENTS.md")).expect("read protected intent"),
        "protected intent\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("TODOS.yaml")).expect("read protected queue"),
        "protected queue\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join(".git/index")).expect("read protected Git state"),
        "protected git index\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("child/src/lib.rs"))
            .expect("read protected child source"),
        "protected child\n"
    );
}

#[test]
fn runner_rejects_aliases_links_overlaps_and_paths_outside_approved_roots() {
    use std::os::unix::fs::symlink;

    let fixture = sandbox_fixture();
    symlink(
        fixture.path().join("tests"),
        fixture.path().join("tests-link"),
    )
    .expect("create link-like grant source");
    let valid = base_request(fixture.path(), "authoring", &["/usr/bin/true"]);
    let mut cases = Vec::new();

    let mut destination_alias = valid.clone();
    destination_alias["grants"][0]["destination"] =
        json!("/workspace/component/tests/../REQUIREMENTS.md");
    cases.push(("destination alias", destination_alias));

    let mut source_link = valid.clone();
    source_link["grants"][1]["source"] = json!(fixture.path().join("tests-link"));
    cases.push(("link-like source", source_link));

    let mut overlapping_destination = valid.clone();
    overlapping_destination["grants"]
        .as_array_mut()
        .expect("grants")
        .push(json!({
            "source": fixture.path().join("tests").canonicalize().expect("tests"),
            "destination": "/workspace/component/tests/nested",
            "access": "read-only",
            "purpose": "context",
            "identity": sha256_bytes(b"overlap")
        }));
    cases.push(("overlapping destination", overlapping_destination));

    for (name, request) in cases {
        let output = run_runner_request(&request);
        assert!(
            !output.status.success(),
            "{name} must fail before Bubblewrap launch"
        );
        assert!(
            !output_text(&output).trim().is_empty(),
            "{name} must report why the grant is invalid"
        );
    }
}

#[test]
fn authoring_rejects_path_escape_writable_symlink_and_unapproved_root_grants() {
    use std::os::unix::fs::symlink;

    let fixture = sandbox_fixture();
    let before = protected_bytes(fixture.path());

    let escape_script = r#"set -eu
printf started > tests/path-escape-started
! printf escaped > tests/../REQUIREMENTS.md
"#;
    let escape_request = base_request(
        fixture.path(),
        "authoring",
        &["/usr/bin/bash", "-c", escape_script],
    );
    let escape = run_runner_request(&escape_request);
    assert!(
        escape.status.success(),
        "the controlled escape probe must run while the protected write is denied: {}",
        output_text(&escape)
    );
    assert!(
        fixture.path().join("tests/path-escape-started").is_file(),
        "the authoring escape probe did not start"
    );
    assert_protected_bytes_unchanged(&before);

    symlink(
        "../REQUIREMENTS.md",
        fixture.path().join("tests/intent-link"),
    )
    .expect("create writable-scope symlink");
    let symlink_request = base_request(
        fixture.path(),
        "authoring",
        &["/usr/bin/bash", "-c", "printf changed > tests/intent-link"],
    );
    let symlink_output = run_runner_request(&symlink_request);
    assert!(
        !symlink_output.status.success(),
        "a writable grant containing a link to protected intent must be rejected"
    );
    assert!(
        output_text(&symlink_output).contains("link")
            || output_text(&symlink_output).contains("symlink")
            || output_text(&symlink_output).contains("grant"),
        "writable-scope symlink rejection must be explicit: {}",
        output_text(&symlink_output)
    );
    assert_protected_bytes_unchanged(&before);

    let unapproved = temporary_directory("unapproved-authoring-root-");
    fs::write(unapproved.path().join("outside"), "outside\n").expect("write unapproved root");
    let mut unapproved_request = base_request(fixture.path(), "authoring", &["/usr/bin/true"]);
    unapproved_request["grants"]
        .as_array_mut()
        .expect("grants")
        .push(json!({
            "source": unapproved.path().canonicalize().expect("unapproved root"),
            "destination": "/workspace/component/unapproved",
            "access": "read-write",
            "purpose": "authoring",
            "identity": sha256_bytes(b"not-approved-by-project-policy")
        }));
    let unapproved_output = run_runner_request(&unapproved_request);
    assert!(
        !unapproved_output.status.success(),
        "a repository request must not grant an unapproved host root"
    );
    assert!(
        output_text(&unapproved_output).contains("approved root")
            || output_text(&unapproved_output).contains("grant")
            || output_text(&unapproved_output).contains("source"),
        "unapproved-root refusal must be explicit: {}",
        output_text(&unapproved_output)
    );
    assert_protected_bytes_unchanged(&before);
}

#[test]
fn backend_identity_mismatch_fails_without_host_fallback() {
    let fixture = sandbox_fixture();
    let marker = fixture.path().join("tests/host-fallback-marker");
    let script = "printf fallback > tests/host-fallback-marker";
    let mut request = base_request(
        fixture.path(),
        "authoring",
        &["/usr/bin/bash", "-c", script],
    );
    request["identities"]["backend"]["digest"] = json!(sha256_bytes(b"substituted-backend"));

    let output = run_runner_request(&request);

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("backend")
            && (output_text(&output).contains("identity")
                || output_text(&output).contains("digest")),
        "backend mismatch must be explicit: {}",
        output_text(&output)
    );
    assert!(
        !marker.exists(),
        "a rejected backend must never execute the command directly on the host"
    );
}

#[test]
fn network_denial_uses_an_isolated_namespace_not_a_host_allowance() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind host-only listener");
    listener
        .set_nonblocking(true)
        .expect("make listener nonblocking");
    let port = listener.local_addr().expect("listener address").port();
    let fixture = sandbox_fixture();
    let script = format!(
        r#"printf started > /workspace/scratch/network-started
if exec 3<>/dev/tcp/127.0.0.1/{port}; then
  printf connected > /workspace/scratch/network-connected
  exit 70
fi
printf network-denied > /workspace/scratch/network-failure
printf 'network denied by isolated namespace\n' >&2
exit 73
"#
    );
    let request = base_request(
        fixture.path(),
        "verification",
        &["/usr/bin/bash", "-c", &script],
    );

    let output = run_runner_request(&request);

    assert!(
        !output.status.success(),
        "network-denied verification reached a host TCP listener"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/network-started"))
            .expect("read network started marker"),
        "started"
    );
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/network-failure"))
            .expect("read network failure marker"),
        "network-denied"
    );
    assert!(
        output_text(&output).contains("network denied"),
        "network failure must retain specific denial evidence: {}",
        output_text(&output)
    );
    assert!(!fixture.path().join("scratch/network-connected").exists());
    thread::sleep(Duration::from_millis(50));
    assert!(
        listener.accept().is_err(),
        "the sandbox must not share the host loopback namespace"
    );
}

#[test]
fn wall_time_termination_cleans_up_the_complete_sandbox_process_tree() {
    let fixture = sandbox_fixture();
    let marker = fixture.path().join("tests/late-child-write");
    let script = "printf started > /workspace/scratch/process-started; (sleep 1; printf escaped > tests/late-child-write) & sleep 30";
    let mut request = base_request(
        fixture.path(),
        "authoring",
        &["/usr/bin/bash", "-c", script],
    );
    request["resources"]["wall_time_ms"] = json!(200);
    let started = Instant::now();

    let output = run_runner_request(&request);

    assert!(!output.status.success(), "timed out request must fail");
    assert_eq!(
        fs::read_to_string(fixture.path().join("scratch/process-started"))
            .expect("read process started marker"),
        "started"
    );
    assert!(
        output_text(&output).contains("timeout")
            || output_text(&output).contains("timed out")
            || output_text(&output).contains("wall time"),
        "wall-time failure must retain specific timeout evidence: {}",
        output_text(&output)
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "runner did not enforce its wall-time bound"
    );
    thread::sleep(Duration::from_millis(1200));
    assert!(
        !marker.exists(),
        "a timed-out descendant survived the sandbox process-tree cleanup"
    );
}

fn assert_bounded_resource_failure(
    output: &std::process::Output,
    resource_names: &[&str],
    max_diagnostic_bytes: usize,
) {
    let text = output_text(output);
    assert!(!output.status.success(), "resource breach must fail");
    assert!(
        resource_names.iter().any(|name| text.contains(name))
            && (text.contains("limit") || text.contains("bound") || text.contains("exceed")),
        "resource failure must identify its specific bound: {text}"
    );
    assert!(
        output.stdout.len() + output.stderr.len() <= max_diagnostic_bytes,
        "resource failure output was not bounded: stdout={} stderr={}",
        output.stdout.len(),
        output.stderr.len()
    );
}

#[test]
fn output_byte_limit_terminates_the_process_tree_without_late_effects() {
    let fixture = sandbox_fixture();
    let marker = fixture.path().join("tests/output-limit-late-write");
    let script = r#"printf started > tests/output-limit-started
(sleep 1; printf late > tests/output-limit-late-write) &
/usr/bin/yes output | /usr/bin/head -c 4096
sleep 30
"#;
    let mut request = base_request(
        fixture.path(),
        "authoring",
        &["/usr/bin/bash", "-c", script],
    );
    request["resources"]["max_output_bytes"] = json!(128);
    let started = Instant::now();

    let output = run_runner_request(&request);

    assert_bounded_resource_failure(&output, &["output", "byte"], 4096);
    assert!(
        fixture.path().join("tests/output-limit-started").is_file(),
        "controlled output-limit command did not start"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "runner did not terminate promptly after output overflow"
    );
    thread::sleep(Duration::from_millis(1200));
    assert!(
        !marker.exists(),
        "an output-limited descendant survived or produced a late effect"
    );
}

#[test]
fn process_count_limit_terminates_all_descendants_without_late_effects() {
    let fixture = sandbox_fixture();
    let marker = fixture.path().join("tests/process-limit-late-write");
    let script = r#"printf started > tests/process-limit-started
(sleep 1; printf late > tests/process-limit-late-write) &
for unused in 1 2 3 4 5 6 7 8 9 10 11 12; do
  sleep 30 &
done
wait
"#;
    let mut request = base_request(
        fixture.path(),
        "authoring",
        &["/usr/bin/bash", "-c", script],
    );
    request["resources"]["max_processes"] = json!(4);
    let started = Instant::now();

    let output = run_runner_request(&request);

    assert_bounded_resource_failure(&output, &["process", "pid"], 4096);
    assert!(
        fixture.path().join("tests/process-limit-started").is_file(),
        "controlled process-limit command did not start"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "runner did not terminate promptly after process-count overflow"
    );
    thread::sleep(Duration::from_millis(1200));
    assert!(
        !marker.exists(),
        "a process-limited descendant survived or produced a late effect"
    );
}

#[test]
fn writable_file_size_limit_stops_the_tree_without_late_effects() {
    let fixture = sandbox_fixture();
    let oversized = fixture.path().join("tests/oversized-output.bin");
    let marker = fixture.path().join("tests/file-limit-late-write");
    let script = r#"printf started > tests/file-limit-started
(sleep 1; printf late > tests/file-limit-late-write) &
/usr/bin/head -c 4096 /dev/zero > tests/oversized-output.bin
sleep 30
"#;
    let mut request = base_request(
        fixture.path(),
        "authoring",
        &["/usr/bin/bash", "-c", script],
    );
    request["resources"]["max_file_bytes"] = json!(256);
    let started = Instant::now();

    let output = run_runner_request(&request);

    assert_bounded_resource_failure(&output, &["file", "size"], 4096);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "runner did not terminate promptly after file-size overflow"
    );
    assert!(
        !oversized.exists()
            || fs::metadata(&oversized)
                .expect("inspect bounded oversized file")
                .len()
                <= 256,
        "runner left an over-limit writable file behind"
    );
    thread::sleep(Duration::from_millis(1200));
    assert!(
        !marker.exists(),
        "a file-size-limited descendant survived or produced a late effect"
    );
}

#[test]
fn scratch_size_limit_stops_the_tree_without_late_effects() {
    let fixture = sandbox_fixture();
    let oversized = fixture.path().join("scratch/oversized-scratch.bin");
    let marker = fixture.path().join("tests/scratch-limit-late-write");
    let script = r#"printf started > tests/scratch-limit-started
(sleep 1; printf late > tests/scratch-limit-late-write) &
/usr/bin/head -c 4096 /dev/zero > /workspace/scratch/oversized-scratch.bin
sleep 30
"#;
    let mut request = base_request(
        fixture.path(),
        "authoring",
        &["/usr/bin/bash", "-c", script],
    );
    request["resources"]["max_scratch_bytes"] = json!(256);
    let started = Instant::now();

    let output = run_runner_request(&request);

    assert_bounded_resource_failure(&output, &["scratch", "storage"], 4096);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "runner did not terminate promptly after scratch-size overflow"
    );
    assert!(
        !oversized.exists()
            || fs::metadata(&oversized)
                .expect("inspect bounded scratch file")
                .len()
                <= 256,
        "runner left over-limit scratch data behind"
    );
    thread::sleep(Duration::from_millis(1200));
    assert!(
        !marker.exists(),
        "a scratch-limited descendant survived or produced a late effect"
    );
}
