use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use kvist::{init::initialize, task_queue};
use serde_json::{Value, json};
use tempfile::TempDir;

fn fixture() -> TempDir {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    project
}

fn run(project: &Path, args: &[&str]) -> (bool, Value) {
    let state = TempDir::new().expect("private user state");
    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .env("HOME", state.path())
        .env("XDG_CONFIG_HOME", state.path())
        .env("XDG_STATE_HOME", state.path())
        .args(["--json", "repair"])
        .arg(project)
        .args(args)
        .output()
        .expect("repair");
    let bytes = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let report = serde_json::from_slice(bytes)
        .unwrap_or_else(|_| json!({"raw": String::from_utf8_lossy(bytes)}));
    (output.status.success(), report)
}

fn broken_queue(project: &Path) -> String {
    let path = project.join("src/TODOS.yaml");
    let contents = fs::read_to_string(&path).expect("queue");
    let header = contents.split("tasks:").next().expect("header");
    let text = format!(
        r#"{header}tasks:
  - id: retain
    title: Preserve useful work
    description: Keep authored data.
    context: ""
    purpose: Recover without fabrication.
    expected_outcome: An explicitly blocked draft.
    kind: test
    status: completed
    depends_on: []
    requirements: [REQUIREMENTS.md#Purpose]
    timestamps:
      created_at: 2026-10-29T10:00:00Z
      updated_at: 2026-10-29T10:00:00Z
      completed_at: 2026-10-29T10:00:00Z
    blocked_reason: null
    recovery_state: null
"#
    );
    fs::write(path, &text).expect("broken queue");
    text
}

fn server(project: &Path, text: String) -> thread::JoinHandle<Value> {
    let listener = TcpListener::bind("127.0.0.1:0").expect("local model");
    listener.set_nonblocking(true).expect("bounded listener");
    let address = listener.local_addr().expect("address");
    fs::create_dir_all(project.join(".kvist")).expect("registry directory");
    fs::write(
        project.join(".kvist/providers.toml"),
        format!(
            "schema_version = 1\n[profiles.repair.backend]\nkind = \"native\"\nprovider = \"llama-server\"\nbase_url = \"http://{address}\"\nmodel = \"fixture-model\"\ncontext_limit = 32768\n"
        ),
    )
    .expect("native profile");
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut socket, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "repair never called the model");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept model request: {error}"),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let mut bytes = Vec::new();
        let (header_end, length) = loop {
            let mut chunk = [0; 4096];
            let count = socket.read(&mut chunk).expect("request bytes");
            assert_ne!(count, 0, "request ended before headers");
            bytes.extend_from_slice(&chunk[..count]);
            assert!(bytes.len() <= 512 * 1024);
            if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..position]);
                assert!(
                    headers.starts_with("POST /v1/chat/completions "),
                    "{headers}"
                );
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().expect("length"))
                    })
                    .expect("content length");
                break (position + 4, length);
            }
        };
        while bytes.len() < header_end + length {
            let mut chunk = [0; 4096];
            let count = socket.read(&mut chunk).expect("request body");
            assert_ne!(count, 0);
            bytes.extend_from_slice(&chunk[..count]);
            assert!(bytes.len() <= 512 * 1024);
        }
        let request =
            serde_json::from_slice(&bytes[header_end..header_end + length]).expect("JSON request");
        let response = json!({
            "id": "repair-fixture", "object": "chat.completion", "model": "fixture-model",
            "choices": [{"index": 0, "message": {"role":"assistant", "content":text}, "finish_reason":"stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20}
        }).to_string();
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
            response.len()
        ).expect("model response");
        request
    })
}

fn stage(project: &Path, directory: &Path) -> (PathBuf, String) {
    let original = broken_queue(project);
    let candidate = original
        .replace("context: \"\"", "context: Proposed recovered context.")
        .replace("recovery_state: null", "recovery_state: null\n    acceptance_id: fabricated-acceptance\n    disposition: accept");
    let response = json!({"contents": candidate, "explanation": "Restore the missing context as a reviewable draft."}).to_string();
    let model = server(project, response);
    let proposal = directory.join("proposal.json");
    let (ok, report) = run(
        project,
        &[
            "--ai",
            "repair",
            "--proposal",
            proposal.to_str().expect("path"),
        ],
    );
    let request = model.join().expect("model");
    assert!(!ok, "staging must not report applied recovery: {report}");
    assert!(proposal.is_file(), "{report}");
    assert!(
        request
            .get("tools")
            .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty)),
        "{request}"
    );
    assert!(request.to_string().contains("Preserve useful work"));
    assert!(
        !request
            .to_string()
            .contains("PEER_IMPLEMENTATION_MUST_NOT_ENTER_CONTEXT")
    );
    assert_eq!(
        fs::read_to_string(project.join("src/TODOS.yaml")).expect("original"),
        original
    );
    assert!(!project.join(".kvist-recovery").exists());
    (proposal, original)
}

#[test]
fn ai_freezes_validated_proposals_and_applies_without_inference_or_fabricated_completion() {
    let project = fixture();
    let output = TempDir::new().expect("proposal storage");
    let (proposal, original) = stage(project.path(), output.path());
    let (_, preview) = run(
        project.path(),
        &["--proposal", proposal.to_str().expect("path")],
    );
    let token = preview["approval"].as_str().expect("token");
    let (ok, report) = run(
        project.path(),
        &[
            "--proposal",
            proposal.to_str().expect("path"),
            "--approve",
            token,
        ],
    );
    assert!(ok, "{report}");
    let recovered = task_queue::parse(
        &fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue"),
    )
    .expect("native validity");
    assert_eq!(recovered.tasks.len(), 1);
    assert_eq!(recovered.tasks[0].id, "retain");
    assert_eq!(recovered.tasks[0].status, task_queue::TaskStatus::Blocked);
    assert!(recovered.tasks[0].timestamps.completed_at.is_none());
    assert!(recovered.tasks[0].acceptance_id.is_none());
    assert!(recovered.tasks[0].disposition.is_none());
    assert_eq!(
        recovered.component.revalidation.state,
        task_queue::RevalidationState::Stale
    );
    let backup = Path::new(report["backup"].as_str().expect("backup"));
    assert_eq!(
        fs::read_to_string(backup.join("originals/src/TODOS.yaml")).expect("backup"),
        original
    );
}

#[test]
fn changed_proposal_context_and_source_invalidate_application_without_backups() {
    for change in ["proposal", "context", "source"] {
        let project = fixture();
        let output = TempDir::new().expect("proposal storage");
        let (proposal, original) = stage(project.path(), output.path());
        let (_, preview) = run(
            project.path(),
            &["--proposal", proposal.to_str().expect("path")],
        );
        let token = preview["approval"].as_str().expect("token");
        match change {
            "proposal" => {
                let contents = fs::read_to_string(&proposal).expect("proposal");
                fs::write(&proposal, format!("{contents}\n")).expect("changed proposal bytes");
            }
            "context" => {
                let path = project.path().join("ROOT_CONTRACT.md");
                let contents = fs::read_to_string(&path).expect("context");
                fs::write(path, format!("{contents}\nContext changed.\n"))
                    .expect("changed context");
            }
            _ => fs::write(
                project.path().join("src/TODOS.yaml"),
                format!("{original}\n"),
            )
            .expect("changed source"),
        }
        let (ok, report) = run(
            project.path(),
            &[
                "--proposal",
                proposal.to_str().expect("path"),
                "--approve",
                token,
            ],
        );
        assert!(!ok, "{change}: {report}");
        assert!(!project.path().join(".kvist-recovery").exists());
    }
}

#[test]
fn malformed_ai_output_is_not_staged_and_dry_run_never_invokes_or_writes() {
    let project = fixture();
    let original = broken_queue(project.path());
    let output = TempDir::new().expect("proposal storage");
    let proposal = output.path().join("bad.json");
    let model = server(project.path(), "not a JSON proposal".to_owned());
    let (ok, report) = run(
        project.path(),
        &[
            "--ai",
            "repair",
            "--proposal",
            proposal.to_str().expect("path"),
        ],
    );
    model.join().expect("model");
    assert!(!ok, "{report}");
    assert!(!proposal.exists());
    let (ok, report) = run(
        project.path(),
        &[
            "--ai",
            "repair",
            "--proposal",
            proposal.to_str().expect("path"),
            "--dry-run",
        ],
    );
    assert!(!ok, "{report}");
    assert!(!proposal.exists());
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("original"),
        original
    );
    assert!(!project.path().join(".kvist-recovery").exists());
}

#[test]
fn external_profiles_and_existing_or_managed_proposal_paths_are_refused() {
    let project = fixture();
    let original = broken_queue(project.path());
    fs::create_dir_all(project.path().join(".kvist")).expect("registry");
    fs::write(
        project.path().join(".kvist/providers.toml"),
        "schema_version = 1\n[profiles.external.backend]\nkind = \"external-agent\"\nprovider = \"copilot\"\nexecutable = \"/must-not-run\"\n",
    ).expect("external profile");
    let output = TempDir::new().expect("output");
    let existing = output.path().join("existing.json");
    fs::write(&existing, "retained").expect("existing");
    for path in [
        output.path().join("new.json"),
        existing.clone(),
        project.path().join("src/TODOS.yaml"),
    ] {
        let (ok, report) = run(
            project.path(),
            &[
                "--ai",
                "external",
                "--proposal",
                path.to_str().expect("path"),
            ],
        );
        assert!(!ok, "{report}");
    }
    assert_eq!(fs::read_to_string(existing).expect("retained"), "retained");
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("source"),
        original
    );
}

#[test]
fn ai_can_stage_without_valid_project_configuration_and_never_receives_peer_internals() {
    let project = fixture();
    fs::create_dir(project.path().join("peer")).expect("peer");
    for filename in [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        let contents =
            fs::read_to_string(project.path().join("src").join(filename)).expect("fixture");
        fs::write(project.path().join("peer").join(filename), contents).expect("peer artifact");
    }
    let peer = project.path().join("peer/DESIGN.md");
    let contents = fs::read_to_string(&peer).expect("peer");
    fs::write(
        peer,
        format!("{contents}\nPEER_IMPLEMENTATION_MUST_NOT_ENTER_CONTEXT\n"),
    )
    .expect("private peer");
    fs::write(project.path().join("kvist.toml"), "invalid [").expect("broken configuration");
    let output = TempDir::new().expect("proposal storage");
    stage(project.path(), output.path());
}

#[test]
fn frozen_unknown_duplicate_traversing_and_authority_broadening_entries_are_refused() {
    let project = fixture();
    let output = TempDir::new().expect("proposal storage");
    let (proposal, original) = stage(project.path(), output.path());
    let frozen: Value =
        serde_json::from_slice(&fs::read(&proposal).expect("proposal")).expect("JSON");
    for defect in [
        "unknown",
        "duplicate",
        "traversal",
        "completed",
        "extra-field",
    ] {
        let mut changed = frozen.clone();
        match defect {
            "unknown" => changed["replacements"][0]["path"] = json!("source.rs"),
            "duplicate" => {
                let entry = changed["replacements"][0].clone();
                changed["replacements"]
                    .as_array_mut()
                    .expect("entries")
                    .push(entry);
            }

            "traversal" => changed["replacements"][0]["path"] = json!("../src/TODOS.yaml"),
            "completed" => {
                let text = changed["replacements"][0]["contents"]
                    .as_str()
                    .expect("contents")
                    .replace("status: blocked", "status: completed");
                changed["replacements"][0]["contents"] = json!(text);
            }
            _ => changed["unknown"] = json!(true),
        }
        let path = output.path().join(format!("{defect}.json"));
        fs::write(&path, changed.to_string()).expect("tampered proposal");
        let (ok, report) = run(
            project.path(),
            &["--proposal", path.to_str().expect("path")],
        );
        assert!(!ok, "{defect}: {report}");
        assert!(!project.path().join(".kvist-recovery").exists());
        assert_eq!(
            fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("source"),
            original
        );
    }
}

#[test]
fn ai_eligibility_detects_identity_loss_but_not_action_words_in_authored_text() {
    let project = fixture();
    let original =
        broken_queue(project.path()).replace("context: \"\"", "context: Original context.");
    let entry = original.split_once("tasks:\n").expect("task boundary").1;
    let source = format!(
        "{original}{}",
        entry.replace(
            "title: Preserve useful work",
            "title: Conflicting authored label"
        )
    );
    fs::write(project.path().join("src/TODOS.yaml"), &source).expect("conflicting identities");
    let output = TempDir::new().expect("proposal storage");
    let path = output.path().join("duplicates.json");
    let model = server(project.path(), json!({
        "contents": &original,
        "explanation": "Propose one identity for human arbitration; both originals remain backed up."
    }).to_string());
    let (_, report) = run(
        project.path(),
        &["--ai", "repair", "--proposal", path.to_str().expect("path")],
    );
    model
        .join()
        .expect("model called for data-discarding salvage");
    assert!(path.is_file(), "{report}");
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("unmodified"),
        source
    );
    let trim_only = original.replace(
        "title: Preserve useful work",
        "title: \"  remove task phrase is authored data  \"",
    );
    fs::write(project.path().join("src/TODOS.yaml"), trim_only)
        .expect("only harmless trimming is needed");
    let unused = output.path().join("not-needed.json");
    let (ok, report) = run(
        project.path(),
        &[
            "--ai",
            "repair",
            "--proposal",
            unused.to_str().expect("path"),
        ],
    );
    assert!(!ok && report.to_string().contains("found 0"), "{report}");
    assert!(
        !unused.exists(),
        "authored wording must not trigger inference"
    );
}
