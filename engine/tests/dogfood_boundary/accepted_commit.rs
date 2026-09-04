use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

use crate::support::{
    ACCEPTANCE_ID, ATTEMPT_ID, TASK_ID, assert_success, create_target_project, git, output_text,
    run, run_kvist, run_kvist_with_path, sha256_file, task_block, temporary_directory,
    write_finalizable_attempt,
};

fn parse_json_stdout(output: &std::process::Output, operation: &str) -> Value {
    let bytes = if !output.stdout.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    serde_json::from_slice(bytes).unwrap_or_else(|error| {
        panic!(
            "{operation} must emit one JSON object: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn string_set(value: &Value, pointer: &str) -> BTreeSet<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("JSON output must contain {pointer}"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("{pointer} entries must be strings"))
                .to_owned()
        })
        .collect()
}

fn changed_paths(project: &Path, before: &str, after: &str) -> BTreeSet<String> {
    let output = git(
        project,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            before,
            after,
        ],
    );
    assert_success(&output, "list committed paths");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn git_path_set(project: &Path, arguments: &[&str], operation: &str) -> BTreeSet<String> {
    let output = git(project, arguments);
    assert_success(&output, operation);
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn head(project: &Path) -> String {
    let output = git(project, &["rev-parse", "HEAD"]);
    assert_success(&output, "read HEAD");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn append_commit_policy(project: &Path, signing: &str) {
    let path = project.join("kvist.toml");
    let mut config = fs::read_to_string(&path).expect("read config");
    config.push_str(&format!(
        "\n[vcs.commit]\nhooks = \"disabled\"\nsigning = \"{signing}\"\n"
    ));
    fs::write(path, config).expect("write commit policy");
}

fn commit_commit_policy(project: &Path, signing: &str) {
    append_commit_policy(project, signing);
    assert_success(&git(project, &["add", "kvist.toml"]), "stage commit policy");
    assert_success(
        &git(
            project,
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "configure commit policy",
            ],
        ),
        "commit commit policy",
    );
}

fn find_acceptance_journal(project: &Path, acceptance_id: &str) -> PathBuf {
    let mut pending = vec![project.join("engine")];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        {
            let entry = entry.expect("read directory entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if fs::read_to_string(&path)
                .map(|contents| contents.contains(acceptance_id))
                .unwrap_or(false)
            {
                return path;
            }
        }
    }
    panic!("no durable acceptance journal contains {acceptance_id}");
}

#[test]
fn component_accept_commit_preserves_unrelated_state_and_commits_only_reported_paths() {
    let project = create_target_project("pending");
    append_commit_policy(project.path(), "off");
    assert_success(
        &git(project.path(), &["add", "kvist.toml"]),
        "stage accepted policy",
    );
    assert_success(
        &git(
            project.path(),
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--quiet",
                "-m",
                "configure commit policy",
            ],
        ),
        "commit policy",
    );
    let old_head = head(project.path());

    let remote = temporary_directory("remote-");
    assert_success(
        &run("git", &["init", "--bare", "--quiet"], remote.path()),
        "initialize bare remote",
    );
    assert_success(
        &git(
            project.path(),
            &[
                "remote",
                "add",
                "origin",
                remote.path().to_str().expect("remote path"),
            ],
        ),
        "add remote",
    );
    assert_success(
        &git(project.path(), &["push", "--quiet", "origin", "HEAD:main"]),
        "push fixture base",
    );

    fs::write(
        project.path().join(".git/hooks/pre-commit"),
        "#!/bin/sh\nprintf ran > .git/hook-ran\nexit 1\n",
    )
    .expect("write rejecting hook");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        project.path().join(".git/hooks/pre-commit"),
        fs::Permissions::from_mode(0o755),
    )
    .expect("make hook executable");

    let requirements = project.path().join("engine/REQUIREMENTS.md");
    let mut accepted = fs::read_to_string(&requirements).expect("read requirements");
    accepted.push_str("\nAccepted fixture clarification.\n");
    fs::write(&requirements, accepted).expect("write accepted document");
    fs::write(
        project.path().join("engine/src/lib.rs"),
        "pub fn unrelated_unstaged() {}\n",
    )
    .expect("write unrelated unstaged change");
    fs::write(
        project.path().join("docs/staged-note.md"),
        "unrelated staged\n",
    )
    .expect("write unrelated staged change");
    assert_success(
        &git(project.path(), &["add", "docs/staged-note.md"]),
        "stage unrelated path",
    );
    fs::write(
        project.path().join("untracked-note.txt"),
        "unrelated untracked\n",
    )
    .expect("write unrelated untracked path");
    let index_before = fs::read(project.path().join(".git/index")).expect("read user index");

    let output = run_kvist(
        project.path(),
        &["--json", "component", "accept", ".", "--commit"],
    );

    assert!(
        output.status.success(),
        "component accept --commit failed: {}",
        output_text(&output)
    );
    let result = parse_json_stdout(&output, "component accept --commit");
    let new_head = result
        .get("commit_oid")
        .and_then(Value::as_str)
        .expect("commit_oid");
    let expected_accepted_paths = BTreeSet::from([
        "engine/REQUIREMENTS.md".to_owned(),
        "engine/TODOS.yaml".to_owned(),
    ]);
    let accepted_paths = string_set(&result, "/accepted_paths");
    assert_eq!(
        accepted_paths, expected_accepted_paths,
        "reported accepted paths must equal the independently declared document-acceptance set"
    );
    assert_eq!(
        changed_paths(project.path(), &old_head, new_head),
        expected_accepted_paths,
        "the commit tree delta must equal the independently declared acceptance set"
    );
    let committed_paths = changed_paths(project.path(), &old_head, new_head);
    for unrelated in [
        "engine/src/lib.rs",
        "docs/staged-note.md",
        "untracked-note.txt",
    ] {
        assert!(
            !committed_paths.contains(unrelated),
            "the acceptance commit must explicitly exclude unrelated path {unrelated}"
        );
    }
    assert_eq!(
        fs::read(project.path().join(".git/index")).expect("read preserved user index"),
        index_before,
        "isolated commit creation must preserve the real Git index byte-for-byte"
    );
    assert_eq!(
        fs::read_to_string(project.path().join("engine/src/lib.rs"))
            .expect("read unrelated unstaged path"),
        "pub fn unrelated_unstaged() {}\n"
    );
    assert!(project.path().join("docs/staged-note.md").exists());
    assert!(project.path().join("untracked-note.txt").exists());
    assert_eq!(
        git_path_set(
            project.path(),
            &["diff", "--name-only"],
            "list unstaged paths"
        ),
        BTreeSet::from([
            "engine/REQUIREMENTS.md".to_owned(),
            "engine/TODOS.yaml".to_owned(),
            "engine/src/lib.rs".to_owned()
        ]),
        "the unrelated unstaged change must remain unstaged against the new HEAD"
    );
    assert_eq!(
        git_path_set(
            project.path(),
            &["diff", "--cached", "--name-only", "HEAD"],
            "list staged paths against HEAD"
        ),
        BTreeSet::from([
            "docs/staged-note.md".to_owned(),
            "engine/REQUIREMENTS.md".to_owned(),
            "engine/TODOS.yaml".to_owned()
        ]),
        "the unrelated staged change must remain staged against the new HEAD"
    );
    assert_eq!(
        git_path_set(
            project.path(),
            &["ls-files", "--others", "--exclude-standard"],
            "list untracked paths"
        ),
        BTreeSet::from(["untracked-note.txt".to_owned()]),
        "the unrelated untracked path must remain untracked"
    );
    assert!(!project.path().join(".git/hook-ran").exists());
    assert!(!project.path().join("hook-ran").exists());
    assert_eq!(
        String::from_utf8_lossy(&git(remote.path(), &["rev-parse", "refs/heads/main"]).stdout)
            .trim(),
        old_head,
        "acceptance commit creation must never push"
    );
    let parent = git(project.path(), &["rev-parse", &format!("{new_head}^")]);
    assert_success(&parent, "read acceptance parent");
    assert_eq!(
        String::from_utf8_lossy(&parent.stdout).trim(),
        old_head,
        "acceptance must create a new commit, never amend the current commit"
    );
}

#[test]
fn accepted_path_overlap_refuses_without_changing_head_or_index() {
    let project = create_target_project("pending");
    append_commit_policy(project.path(), "off");
    fs::write(
        project.path().join("engine/REQUIREMENTS.md"),
        "overlapping accepted change\n",
    )
    .expect("write overlap");
    assert_success(
        &git(project.path(), &["add", "engine/REQUIREMENTS.md"]),
        "stage overlapping path",
    );
    let old_head = head(project.path());
    let index_before = fs::read(project.path().join(".git/index")).expect("read index");

    let output = run_kvist(project.path(), &["component", "accept", ".", "--commit"]);

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("overlap")
            || output_text(&output).contains("staged")
            || output_text(&output).contains("index"),
        "accepted-path overlap must be explicit: {}",
        output_text(&output)
    );
    assert_eq!(head(project.path()), old_head);
    assert_eq!(
        fs::read(project.path().join(".git/index")).expect("read unchanged index"),
        index_before
    );
}

#[test]
fn detached_head_refuses_acceptance_commit_without_mutation() {
    let project = create_target_project("pending");
    commit_commit_policy(project.path(), "off");
    let detached_oid = head(project.path());
    assert_success(
        &git(project.path(), &["checkout", "--detach", "--quiet"]),
        "detach HEAD",
    );
    let requirements = project.path().join("engine/REQUIREMENTS.md");
    let mut contents = fs::read_to_string(&requirements).expect("read requirements");
    contents.push_str("\nDetached-head refusal fixture.\n");
    fs::write(requirements, contents).expect("write accepted candidate");
    let queue_before =
        fs::read(project.path().join("engine/TODOS.yaml")).expect("read queue before refusal");
    let index_before = fs::read(project.path().join(".git/index")).expect("read index");
    let status_before = git(project.path(), &["status", "--porcelain=v1", "-z"]).stdout;

    let output = run_kvist(project.path(), &["component", "accept", ".", "--commit"]);

    assert!(
        !output.status.success(),
        "acceptance commit must refuse detached HEAD"
    );
    assert!(
        !output_text(&output).contains("unexpected argument")
            && (output_text(&output).contains("detached")
                || output_text(&output).contains("branch")
                || output_text(&output).contains("symbolic")),
        "detached-HEAD refusal must be a typed VCS boundary failure: {}",
        output_text(&output)
    );
    assert_eq!(head(project.path()), detached_oid);
    assert!(
        !git(project.path(), &["symbolic-ref", "-q", "HEAD"])
            .status
            .success(),
        "refusal must not attach HEAD to a branch"
    );
    assert_eq!(
        fs::read(project.path().join("engine/TODOS.yaml")).expect("read queue after refusal"),
        queue_before
    );
    assert_eq!(
        fs::read(project.path().join(".git/index")).expect("read index after refusal"),
        index_before
    );
    assert_eq!(
        git(project.path(), &["status", "--porcelain=v1", "-z"]).stdout,
        status_before,
        "detached-HEAD refusal must not create acceptance evidence or other mutations"
    );
}

#[test]
fn invalid_or_oversized_commit_messages_refuse_before_acceptance_mutation() {
    let cases = [
        ("oversized", "x".repeat(65_537)),
        ("control character", "\u{001b}[31mforged".to_owned()),
    ];

    for (name, message) in cases {
        let project = create_target_project("pending");
        commit_commit_policy(project.path(), "off");
        let requirements = project.path().join("engine/REQUIREMENTS.md");
        let mut contents = fs::read_to_string(&requirements).expect("read requirements");
        contents.push_str("\nCommit-message refusal fixture.\n");
        fs::write(requirements, contents).expect("write accepted candidate");
        let head_before = head(project.path());
        let queue_before =
            fs::read(project.path().join("engine/TODOS.yaml")).expect("read queue before refusal");
        let index_before = fs::read(project.path().join(".git/index")).expect("read index");
        let status_before = git(project.path(), &["status", "--porcelain=v1", "-z"]).stdout;

        let output = run_kvist(
            project.path(),
            &[
                "component",
                "accept",
                ".",
                "--commit",
                "--message",
                message.as_str(),
            ],
        );

        assert!(!output.status.success(), "{name} message must be rejected");
        let text = output_text(&output);
        assert!(
            !text.contains("unexpected argument") && !text.contains("unrecognized"),
            "{name} must exercise the bounded commit-message interface: {text}"
        );
        assert!(
            text.contains("message")
                && (text.contains("limit")
                    || text.contains("size")
                    || text.contains("invalid")
                    || text.contains("control")),
            "{name} rejection must identify the commit-message rule: {text}"
        );
        assert!(
            output.stdout.len() + output.stderr.len() <= 16 * 1024,
            "{name} rejection must not echo an unbounded message"
        );
        assert_eq!(head(project.path()), head_before);
        assert_eq!(
            fs::read(project.path().join("engine/TODOS.yaml"))
                .expect("read queue after message refusal"),
            queue_before
        );
        assert_eq!(
            fs::read(project.path().join(".git/index")).expect("read index after message refusal"),
            index_before
        );
        assert_eq!(
            git(project.path(), &["status", "--porcelain=v1", "-z"]).stdout,
            status_before,
            "{name} message must be refused before acceptance state is written"
        );
    }
}

#[test]
fn tampered_attempt_evidence_cannot_be_finalized_and_committed() {
    let project = create_target_project("pending");
    commit_commit_policy(project.path(), "off");
    write_finalizable_attempt(project.path(), true);
    fs::write(
        project.path().join("engine/tests/generated.rs"),
        "#[test]\nfn tampered_before_commit() {}\n",
    )
    .expect("tamper with scoped change before commit finalization");
    let head_before = head(project.path());
    let journal_path = project
        .path()
        .join(format!("engine/.kvist-attempts/{TASK_ID}.jsonl"));
    let index_before = fs::read(project.path().join(".git/index")).expect("read index");

    let output = run_kvist(
        project.path(),
        &[
            "task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept", "--commit",
        ],
    );

    assert!(
        !output.status.success(),
        "tampered evidence must prevent acceptance commit"
    );
    assert!(
        output_text(&output).contains("digest")
            || output_text(&output).contains("changed")
            || output_text(&output).contains("evidence"),
        "commit refusal must identify changed attempt evidence: {}",
        output_text(&output)
    );
    assert_eq!(head(project.path()), head_before);
    let queue = fs::read_to_string(project.path().join("engine/TODOS.yaml"))
        .expect("read queue after refusal");
    let task = task_block(&queue, TASK_ID);
    assert!(task.contains("status: in-progress"));
    assert!(!task.contains("status: completed"));
    assert!(
        queue.contains("state: fenced") || queue.contains("evidence"),
        "tampered commit attempt must remain durably fenced"
    );
    let journal = fs::read_to_string(&journal_path).expect("read journal after refusal");
    assert!(!journal.contains("\"phase\":\"human-finalized\""));
    assert!(!journal.contains("\"disposition\":\"accepted\""));
    assert_eq!(
        fs::read(project.path().join(".git/index")).expect("read index after refusal"),
        index_before
    );
}

#[test]
fn task_finalization_commit_handles_creations_deletions_and_renames_exactly() {
    let project = create_target_project("pending");
    append_commit_policy(project.path(), "off");
    assert_success(&git(project.path(), &["add", "kvist.toml"]), "stage policy");
    assert_success(
        &git(
            project.path(),
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--quiet",
                "-m",
                "configure policy",
            ],
        ),
        "commit policy",
    );
    let old_head = head(project.path());
    let old_source = project.path().join("engine/src/lib.rs");
    let old_digest = sha256_file(&old_source);
    fs::rename(&old_source, project.path().join("engine/src/renamed.rs"))
        .expect("rename accepted source");
    write_finalizable_attempt(project.path(), true);
    fs::write(
        project.path().join("engine/src/created.rs"),
        "pub fn created() {}\n",
    )
    .expect("write accepted creation");
    let queue_path = project.path().join("engine/TODOS.yaml");
    let journal_path = project
        .path()
        .join(format!("engine/.kvist-attempts/{TASK_ID}.jsonl"));
    let changes = json!([
        {
            "path": "engine/src/lib.rs",
            "operation": "delete",
            "pre_digest": old_digest,
            "post_digest": Value::Null
        },
        {
            "path": "engine/src/renamed.rs",
            "operation": "create",
            "pre_digest": Value::Null,
            "post_digest": sha256_file(&project.path().join("engine/src/renamed.rs"))
        },
        {
            "path": "engine/src/created.rs",
            "operation": "create",
            "pre_digest": Value::Null,
            "post_digest": sha256_file(&project.path().join("engine/src/created.rs"))
        },
        {
            "path": "engine/tests/generated.rs",
            "operation": "create",
            "pre_digest": Value::Null,
            "post_digest": sha256_file(&project.path().join("engine/tests/generated.rs"))
        }
    ]);
    let events = [
        json!({"schema_version":1,"attempt_id":ATTEMPT_ID,"task_id":TASK_ID,"phase":"prepared"}),
        json!({"schema_version":1,"attempt_id":ATTEMPT_ID,"task_id":TASK_ID,"phase":"execution-finished","result":"success","changes":changes}),
        json!({"schema_version":1,"attempt_id":ATTEMPT_ID,"task_id":TASK_ID,"phase":"verification-finished","result":"success"}),
        json!({"schema_version":1,"attempt_id":ATTEMPT_ID,"task_id":TASK_ID,"phase":"pending-human-disposition"}),
    ];
    fs::write(
        &journal_path,
        events
            .iter()
            .map(|event| format!("{event}\n"))
            .collect::<String>(),
    )
    .expect("write exact attempt journal");
    let index_before = fs::read(project.path().join(".git/index")).expect("read index");

    let output = run_kvist(
        project.path(),
        &[
            "--json", "task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept", "--commit",
        ],
    );

    assert!(
        output.status.success(),
        "task finalization --commit failed: {}",
        output_text(&output)
    );
    let result = parse_json_stdout(&output, "task finalize --commit");
    let new_head = result
        .get("commit_oid")
        .and_then(Value::as_str)
        .expect("commit_oid");
    let expected_accepted_paths = BTreeSet::from([
        "engine/.kvist-attempts/implement-code.jsonl".to_owned(),
        "engine/TODOS.yaml".to_owned(),
        "engine/src/created.rs".to_owned(),
        "engine/src/lib.rs".to_owned(),
        "engine/src/renamed.rs".to_owned(),
        "engine/tests/generated.rs".to_owned(),
    ]);
    let accepted_paths = string_set(&result, "/accepted_paths");
    assert_eq!(accepted_paths, expected_accepted_paths);
    assert_eq!(
        changed_paths(project.path(), &old_head, new_head),
        expected_accepted_paths
    );
    assert_eq!(
        fs::read(project.path().join(".git/index")).expect("read index after commit"),
        index_before
    );
    let committed_queue = git(
        project.path(),
        &["show", &format!("{new_head}:engine/TODOS.yaml")],
    );
    assert_success(&committed_queue, "read committed queue");
    assert!(
        task_block(&String::from_utf8_lossy(&committed_queue.stdout), TASK_ID)
            .contains("status: completed")
    );
    assert!(queue_path.exists());
}

#[test]
fn required_signing_failure_keeps_acceptance_pending_for_exact_retry() {
    let project = create_target_project("pending");
    append_commit_policy(project.path(), "required");
    let signing_key = project.path().join(".kvist-test-signing-key");
    assert_success(
        &git(project.path(), &["config", "gpg.format", "ssh"]),
        "configure SSH signing",
    );
    assert_success(
        &git(
            project.path(),
            &[
                "config",
                "user.signingkey",
                signing_key.to_str().expect("signing key path"),
            ],
        ),
        "configure absent signing key",
    );
    assert_success(
        &git(project.path(), &["add", "kvist.toml"]),
        "stage signing policy",
    );
    assert_success(
        &git(
            project.path(),
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "configure required signing",
            ],
        ),
        "commit signing policy",
    );
    let old_head = head(project.path());
    let requirements = project.path().join("engine/REQUIREMENTS.md");
    let mut contents = fs::read_to_string(&requirements).expect("read requirements");
    contents.push_str("\nSigning recovery fixture.\n");
    fs::write(requirements, contents).expect("write accepted change");

    let failed = run_kvist(
        project.path(),
        &["--json", "component", "accept", ".", "--commit"],
    );

    assert!(!failed.status.success());
    assert!(
        output_text(&failed).contains("sign")
            && (output_text(&failed).contains("pending")
                || output_text(&failed).contains("acceptance")),
        "required signing failure must retain accepted state: {}",
        output_text(&failed)
    );
    assert_eq!(head(project.path()), old_head);
    let failure = parse_json_stdout(&failed, "failed signed acceptance");
    let acceptance_id = failure
        .get("acceptance_id")
        .and_then(Value::as_str)
        .expect("pending acceptance_id");
    let journal = find_acceptance_journal(project.path(), acceptance_id);
    let journal_contents = fs::read_to_string(&journal).expect("read pending journal");
    assert!(journal_contents.contains("commit-pending"));
    assert!(journal_contents.contains("required"));

    let keygen = run(
        "ssh-keygen",
        &[
            "-q",
            "-t",
            "ed25519",
            "-N",
            "",
            "-f",
            signing_key.to_str().expect("signing key path"),
        ],
        project.path(),
    );
    assert_success(&keygen, "generate signing key for retry");
    let retry = run_kvist(
        project.path(),
        &["--json", "vcs", "commit-accepted", acceptance_id],
    );
    assert!(
        retry.status.success(),
        "exact pending acceptance retry failed: {}",
        output_text(&retry)
    );
    let retry_result = parse_json_stdout(&retry, "vcs commit-accepted");
    assert_eq!(
        retry_result.get("acceptance_id").and_then(Value::as_str),
        Some(acceptance_id)
    );
    assert_ne!(head(project.path()), old_head);
}

#[test]
fn pending_commit_refuses_concurrent_head_movement() {
    let project = create_target_project("pending");
    append_commit_policy(project.path(), "required");
    let missing_key = project.path().join("missing-signing-key");
    assert_success(
        &git(project.path(), &["config", "gpg.format", "ssh"]),
        "configure SSH signing",
    );
    assert_success(
        &git(
            project.path(),
            &[
                "config",
                "user.signingkey",
                missing_key.to_str().expect("key path"),
            ],
        ),
        "configure missing key",
    );
    assert_success(&git(project.path(), &["add", "kvist.toml"]), "stage policy");
    assert_success(
        &git(
            project.path(),
            &[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--quiet",
                "-m",
                "configure policy",
            ],
        ),
        "commit policy",
    );
    let requirements = project.path().join("engine/REQUIREMENTS.md");
    let mut contents = fs::read_to_string(&requirements).expect("read requirements");
    contents.push_str("\nConcurrent head fixture.\n");
    fs::write(requirements, contents).expect("write accepted change");
    let failed = run_kvist(
        project.path(),
        &["--json", "component", "accept", ".", "--commit"],
    );
    assert!(!failed.status.success());
    let failure = parse_json_stdout(&failed, "pending acceptance");
    let acceptance_id = failure
        .get("acceptance_id")
        .and_then(Value::as_str)
        .expect("acceptance ID");

    fs::write(project.path().join("docs/concurrent.md"), "concurrent\n")
        .expect("write concurrent change");
    assert_success(
        &git(project.path(), &["add", "docs/concurrent.md"]),
        "stage concurrent commit",
    );
    assert_success(
        &git(
            project.path(),
            &[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--quiet",
                "-m",
                "concurrent head movement",
            ],
        ),
        "create concurrent commit",
    );
    let concurrent_head = head(project.path());

    let retry = run_kvist(project.path(), &["vcs", "commit-accepted", acceptance_id]);

    assert!(!retry.status.success());
    assert!(
        output_text(&retry).contains("head")
            || output_text(&retry).contains("concurrent")
            || output_text(&retry).contains("expected"),
        "concurrent-head refusal must be explicit: {}",
        output_text(&retry)
    );
    assert_eq!(head(project.path()), concurrent_head);
}

#[test]
fn jujutsu_commit_automation_is_explicitly_unsupported() {
    let project = create_target_project("pending");
    let jj_state = project.path().join(".jj/repo");
    fs::create_dir_all(&jj_state).expect("create controlled Jujutsu metadata");
    fs::write(jj_state.join("fixture-state"), "unchanged\n")
        .expect("write controlled Jujutsu state");
    let proxy = project.external_tools_path().join("jj");
    let proxy_marker = project.external_tools_path().join("jj-invoked");
    fs::write(
        &proxy,
        format!(
            "#!/bin/sh\nprintf invoked > '{}'\nexit 99\n",
            proxy_marker.display()
        ),
    )
    .expect("write controlled Jujutsu proxy");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&proxy, fs::Permissions::from_mode(0o755))
        .expect("make Jujutsu proxy executable");
    let config_path = project.path().join("kvist.toml");
    let config = fs::read_to_string(&config_path)
        .expect("read config")
        .replace("kind = \"git\"", "kind = \"jj\"");
    fs::write(config_path, config).expect("select Jujutsu");
    let requirements = project.path().join("engine/REQUIREMENTS.md");
    let mut contents = fs::read_to_string(&requirements).expect("read requirements");
    contents.push_str("\nUnsupported Jujutsu commit fixture.\n");
    fs::write(requirements, contents).expect("write accepted change");
    let git_head_before = head(project.path());
    let index_before = fs::read(project.path().join(".git/index")).expect("read index");
    let queue_before =
        fs::read(project.path().join("engine/TODOS.yaml")).expect("read queue before rejection");
    let status_before = git(project.path(), &["status", "--porcelain=v1", "-z"]).stdout;
    let controlled_path = PathBuf::from(format!(
        "{}:/usr/bin:/bin",
        project.external_tools_path().display()
    ));

    let output = run_kvist_with_path(
        project.path(),
        &["component", "accept", ".", "--commit"],
        &controlled_path,
    );

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("Jujutsu") && output_text(&output).contains("unsupported"),
        "Jujutsu write backend must fail explicitly: {}",
        output_text(&output)
    );
    assert_eq!(head(project.path()), git_head_before);
    assert_eq!(
        fs::read(project.path().join(".git/index")).expect("read unchanged index"),
        index_before
    );
    assert_eq!(
        fs::read(project.path().join("engine/TODOS.yaml")).expect("read unchanged queue"),
        queue_before
    );
    assert_eq!(
        git(project.path(), &["status", "--porcelain=v1", "-z"]).stdout,
        status_before,
        "unsupported Jujutsu rejection must not mutate Git-visible state"
    );
    assert_eq!(
        fs::read_to_string(jj_state.join("fixture-state")).expect("read Jujutsu fixture state"),
        "unchanged\n"
    );
    assert!(
        !proxy_marker.exists(),
        "unsupported backend rejection must not invoke a Jujutsu write-capable command"
    );
}

#[test]
fn vcs_commit_accepted_cli_is_scoped_to_one_acceptance_id() {
    let project = create_target_project("pending");
    let help = run_kvist(project.path(), &["vcs", "commit-accepted", "--help"]);
    assert!(
        help.status.success(),
        "planned VCS recovery command missing: {}",
        output_text(&help)
    );
    let text = output_text(&help);
    assert!(text.contains("ACCEPTANCE_ID"));
    assert!(!text.contains("--all"));
    assert!(!text.contains("--push"));
    assert!(!text.contains("--amend"));

    let missing = run_kvist(project.path(), &["vcs", "commit-accepted", ACCEPTANCE_ID]);
    assert!(!missing.status.success());
    assert!(
        output_text(&missing).contains(ACCEPTANCE_ID)
            || output_text(&missing).contains("acceptance"),
        "unknown acceptance failure must identify the requested set: {}",
        output_text(&missing)
    );
}
