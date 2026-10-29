use std::fs;

use serde_json::json;

use crate::support::{
    ATTEMPT_ID, TASK_ID, add_independent_ready_implementation_task, assert_success,
    create_target_project, git, output_text, run_kvist, sha256_file, task_block,
    write_ambiguous_attempt, write_finalizable_attempt,
};

fn attempt_journal(project: &std::path::Path) -> std::path::PathBuf {
    project.join(format!("maerg/.kvist-attempts/{TASK_ID}.jsonl"))
}

fn assert_attempt_is_fenced_and_unfinalized(project: &std::path::Path) {
    let queue = fs::read_to_string(project.join("maerg/TODOS.yaml"))
        .expect("read refused-finalization queue");
    let task = task_block(&queue, TASK_ID);
    assert!(task.contains("status: in-progress"));
    assert!(!task.contains("status: completed"));
    assert!(
        queue.contains("state: fenced") || queue.contains("evidence"),
        "evidence tampering must leave the attempt durably fenced"
    );
    let journal =
        fs::read_to_string(attempt_journal(project)).expect("read refused-finalization journal");
    assert!(!journal.contains("\"phase\":\"human-finalized\""));
    assert!(!journal.contains("\"disposition\":\"accepted\""));
}

#[test]
fn supervised_cli_never_auto_executes_a_suggested_task_and_exposes_recovery_and_finalization() {
    let project = create_target_project("pending");

    // A blank-task run must not auto-execute in a non-interactive context:
    // it offers the next ready task, which requires an interactive terminal to
    // confirm, so it fails and requests an explicit TASK_ID instead.
    let missing_task = run_kvist(project.path(), &["task", "run", "."]);
    assert!(
        !missing_task.status.success(),
        "supervised execution must not automatically select a task"
    );
    assert!(
        output_text(&missing_task).contains("TASK_ID")
            || output_text(&missing_task).contains("task id"),
        "missing-task diagnostic must request an explicit task ID: {}\ndoctor: {}",
        output_text(&missing_task),
        output_text(&run_kvist(project.path(), &["doctor"]))
    );

    let task_help = run_kvist(project.path(), &["task", "--help"]);
    assert!(task_help.status.success());
    let help = output_text(&task_help);
    assert!(help.contains("recover"), "task help must list recovery");
    assert!(
        help.contains("finalize"),
        "task help must list finalization"
    );

    let run_help = run_kvist(project.path(), &["task", "run", "--help"]);
    assert!(run_help.status.success());
    let help = output_text(&run_help);
    assert!(
        help.contains("[TASK_ID]"),
        "TASK_ID must be optional (a suggestion is offered when it is omitted): {}",
        help
    );
    assert!(
        !help.contains("--max-retries") && !help.contains("--retry"),
        "the supervised tier must not advertise automatic retry"
    );
}

#[test]
fn successful_process_and_verification_remain_pending_for_human_disposition() {
    let project = create_target_project("pending");
    let before_head = git(project.path(), &["rev-parse", "HEAD"]).stdout;

    let approval = run_kvist(project.path(), &["task", "approve-policy"]);
    let execution = run_kvist(project.path(), &["task", "run", ".", TASK_ID]);

    assert!(
        approval.status.success(),
        "controlled supervised policy approval failed: {}",
        output_text(&approval)
    );
    assert!(
        execution.status.success(),
        "controlled supervised run failed: {}",
        output_text(&execution)
    );
    assert!(
        project
            .path()
            .join("maerg/tests/supervised-run-started")
            .is_file()
            || project
                .path()
                .join("maerg/tests/supervised-agent-started")
                .is_file(),
        "the controlled runner or authoring command must prove task execution started"
    );
    let queue = fs::read_to_string(project.path().join("maerg/TODOS.yaml"))
        .expect("read pending-disposition queue");
    let task = task_block(&queue, TASK_ID);
    assert!(
        task.contains("status: in-progress"),
        "successful execution evidence alone must not complete the task"
    );
    assert!(!task.contains("status: completed"));
    let journal_path = project
        .path()
        .join(format!("maerg/.kvist-attempts/{TASK_ID}.jsonl"));
    let pending_journal =
        fs::read_to_string(&journal_path).expect("read pending-disposition journal");
    assert!(pending_journal.contains("\"phase\":\"execution-finished\""));
    assert!(pending_journal.contains("\"phase\":\"verification-finished\""));
    assert!(pending_journal.contains("\"result\":\"success\""));
    assert!(
        pending_journal.contains("\"phase\":\"pending-human-disposition\""),
        "a successful supervised run must durably await human disposition"
    );
    assert_eq!(
        git(project.path(), &["rev-parse", "HEAD"]).stdout,
        before_head,
        "recording a successful attempt must not create a commit"
    );

    let output = run_kvist(
        project.path(),
        &["task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept"],
    );

    assert!(
        output.status.success(),
        "human acceptance failed: {}",
        output_text(&output)
    );
    let queue =
        fs::read_to_string(project.path().join("maerg/TODOS.yaml")).expect("read final queue");
    assert!(task_block(&queue, TASK_ID).contains("status: completed"));
    let journal = fs::read_to_string(journal_path).expect("read final attempt journal");
    assert!(journal.contains("\"phase\":\"human-finalized\""));
    assert!(journal.contains("\"disposition\":\"accepted\""));
    assert_eq!(
        git(project.path(), &["rev-parse", "HEAD"]).stdout,
        before_head,
        "finalization without --commit must not create a VCS commit"
    );
}

#[test]
fn human_can_block_a_successful_attempt_without_automatic_retry() {
    let project = create_target_project("pending");
    write_finalizable_attempt(project.path(), true);

    let output = run_kvist(
        project.path(),
        &[
            "task",
            "finalize",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "block",
            "--reason",
            "human review rejected the scoped change",
        ],
    );

    assert!(
        output.status.success(),
        "human block finalization failed: {}",
        output_text(&output)
    );
    let queue =
        fs::read_to_string(project.path().join("maerg/TODOS.yaml")).expect("read blocked queue");
    assert!(queue.contains("status: blocked"));
    assert!(queue.contains("human review rejected the scoped change"));
    let journal = fs::read_to_string(
        project
            .path()
            .join(format!("maerg/.kvist-attempts/{TASK_ID}.jsonl")),
    )
    .expect("read final attempt journal");
    assert!(journal.contains("\"disposition\":\"blocked\""));
    assert_eq!(
        journal.matches("\"attempt_id\":\"attempt-0001\"").count(),
        5,
        "finalization must append one event to the existing attempt, not retry it"
    );
}

fn stale_requirements(project: &std::path::Path) {
    let path = project.join("maerg/REQUIREMENTS.md");
    let mut contents = fs::read_to_string(&path).expect("read intent");
    contents.push_str("\nHuman-requested intent clarification.\n");
    fs::write(path, contents).expect("stale intent");
}

fn retain_prepared_attempt(project: &std::path::Path) -> Vec<u8> {
    let path = attempt_journal(project);
    let contents = fs::read_to_string(&path).expect("read attempt");
    let first = format!(
        "{}\n",
        contents
            .lines()
            .take_while(|line| {
                let event: serde_json::Value = serde_json::from_str(line).unwrap();
                event.get("schema_version").is_none() || event["phase"] == "prepared"
            })
            .collect::<Vec<_>>()
            .join("\n")
    );
    fs::write(path, &first).expect("retain interrupted attempt");
    first.into_bytes()
}

fn execute_reviewable_attempt(project: &std::path::Path) {
    assert_success(
        &run_kvist(project, &["task", "approve-policy"]),
        "approve fixture policy",
    );
    assert_success(
        &run_kvist(project, &["task", "run", ".", TASK_ID]),
        "record host-authenticated fixture attempt",
    );
}

#[test]
fn stale_prepared_attempt_can_be_abandoned_before_acceptance_and_then_returned_to_pending() {
    let project = create_target_project("pending");
    execute_reviewable_attempt(project.path());
    let journal_before = retain_prepared_attempt(project.path());
    stale_requirements(project.path());
    let queue_path = project.path().join("maerg/TODOS.yaml");
    let queue_before = kvist::task_queue::parse(&fs::read_to_string(&queue_path).unwrap()).unwrap();

    for args in [
        vec!["component", "accept", "."],
        vec!["task", "transition", ".", TASK_ID, "pending"],
        vec!["task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept"],
        vec!["task", "run", ".", TASK_ID],
        vec!["task", "next", "."],
    ] {
        let output = run_kvist(project.path(), &args);
        assert!(
            !output.status.success(),
            "initial guard {args:?}: {}",
            output_text(&output)
        );
        let expected = if args[0] == "component" {
            "unresolved"
        } else {
            "stale"
        };
        assert!(
            output_text(&output).contains(expected),
            "{args:?} must identify {expected}: {}",
            output_text(&output)
        );
    }

    let logs_dir = project.path().join("maerg/.kvist/logs");
    let log_path = fs::read_dir(&logs_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(log_path, "retained failure evidence\n").unwrap();
    let log = run_kvist(project.path(), &["task", "log", ".", TASK_ID]);
    assert_success(&log, "inspect stale attempt log");
    assert!(output_text(&log).contains("retained failure evidence"));

    let reject = [
        "task",
        "finalize",
        ".",
        TASK_ID,
        ATTEMPT_ID,
        "block",
        "--reason",
        "Abandon failed attempt; completion is not certified.",
    ];
    let output = run_kvist(project.path(), &reject);
    assert_success(&output, "abandon stale prepared attempt");
    let message = output_text(&output);
    assert!(message.contains("completion is not certified"), "{message}");
    assert!(message.contains("kvist component accept ."), "{message}");
    assert!(
        message.contains(&format!("kvist task transition . {TASK_ID} pending")),
        "{message}"
    );
    let queue = kvist::task_queue::parse(&fs::read_to_string(&queue_path).unwrap()).unwrap();
    assert_eq!(
        queue.component, queue_before.component,
        "rejection cannot accept intent"
    );
    assert_eq!(
        queue.tasks[1].status,
        kvist::task_queue::TaskStatus::Blocked
    );
    assert!(queue.tasks[1].timestamps.completed_at.is_none());
    assert!(queue.tasks[1].acceptance_id.is_none());
    let journal = fs::read(attempt_journal(project.path())).unwrap();
    assert!(
        journal.starts_with(&journal_before),
        "never rewrite prior evidence"
    );
    let journal_text = String::from_utf8_lossy(&journal);
    assert!(journal_text.contains("\"disposition\":\"blocked\""));
    assert!(!journal_text.contains("execution-did-not-start"));
    assert!(!journal_text.contains("\"disposition\":\"accepted\""));
    let repeat = run_kvist(project.path(), &reject);
    assert_success(
        &repeat,
        "repeat identical block without duplicating evidence",
    );
    assert_eq!(fs::read(attempt_journal(project.path())).unwrap(), journal);
    let pending = run_kvist(
        project.path(),
        &["task", "transition", ".", TASK_ID, "pending"],
    );
    assert!(
        !pending.status.success(),
        "stale intent still fences runnable transitions"
    );
    let accept = run_kvist(project.path(), &["component", "accept", "."]);
    assert_success(&accept, "accept intent after abandonment");
    let message = output_text(&accept);
    assert!(message.contains("remains blocked"), "{message}");
    assert!(!message.contains("component is now current"), "{message}");
    assert!(
        message.contains(&format!("kvist task transition . {TASK_ID} pending")),
        "{message}"
    );
    assert_success(
        &run_kvist(
            project.path(),
            &["task", "transition", ".", TASK_ID, "pending"],
        ),
        "deliberately return to pending",
    );
    assert_success(
        &run_kvist(project.path(), &["task", "next", "."]),
        "selection is unfenced",
    );
}

#[test]
fn human_rejection_does_not_require_unchanged_result_digests() {
    let project = create_target_project("pending");
    write_finalizable_attempt(project.path(), true);
    let journal_before = fs::read(attempt_journal(project.path())).unwrap();
    stale_requirements(project.path());
    let changed = project.path().join("maerg/tests/generated.rs");
    fs::write(&changed, "// retained changed result\n").unwrap();
    let output = run_kvist(
        project.path(),
        &[
            "task",
            "finalize",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "block",
            "--reason",
            "Reject changed result without certifying it.",
        ],
    );
    assert_success(&output, "reject drifted result");
    assert_eq!(
        fs::read_to_string(changed).unwrap(),
        "// retained changed result\n"
    );
    assert!(
        fs::read(attempt_journal(project.path()))
            .unwrap()
            .starts_with(&journal_before)
    );
}

#[test]
fn another_blocked_task_does_not_prevent_explicit_attempt_rejection() {
    let project = create_target_project("pending");
    write_finalizable_attempt(project.path(), true);
    add_independent_ready_implementation_task(project.path());
    let path = project.path().join("maerg/TODOS.yaml");
    let mut queue = kvist::task_queue::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    queue.tasks[2].status = kvist::task_queue::TaskStatus::Blocked;
    queue.tasks[2].blocked_reason = Some("Unrelated blocker remains unresolved.".into());
    fs::write(&path, kvist::task_queue::serialize(&queue).unwrap()).unwrap();
    let other = queue.tasks[2].clone();
    let output = run_kvist(
        project.path(),
        &[
            "--json",
            "task",
            "finalize",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "block",
            "--reason",
            "Explicit human rejection.",
        ],
    );
    assert_success(&output, "reject attempt while another task is blocked");
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["disposition"], "block");
    let after = kvist::task_queue::parse(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(after.tasks[2], other);
}

#[test]
fn rejection_can_finish_its_interrupted_recording_without_accepting_stale_intent() {
    let project = create_target_project("pending");
    execute_reviewable_attempt(project.path());
    let prefix = retain_prepared_attempt(project.path());
    stale_requirements(project.path());
    let path = project.path().join("maerg/TODOS.yaml");
    let mut queue = kvist::task_queue::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    queue.tasks[1].status = kvist::task_queue::TaskStatus::Blocked;
    queue.tasks[1].blocked_reason = Some("Abandon interrupted attempt.".into());
    queue.tasks[1].disposition = Some(kvist::task_queue::FinalizeDispositionArgument::Block);
    fs::write(&path, kvist::task_queue::serialize(&queue).unwrap()).unwrap();
    let before = fs::read(&path).unwrap();
    let wrong = run_kvist(
        project.path(),
        &[
            "task",
            "finalize",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "block",
            "--reason",
            "different decision",
        ],
    );
    assert!(!wrong.status.success());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(attempt_journal(project.path())).unwrap(), prefix);
    let repeat = run_kvist(
        project.path(),
        &[
            "task",
            "finalize",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "block",
            "--reason",
            "Abandon interrupted attempt.",
        ],
    );
    assert_success(&repeat, "finish interrupted rejection");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(
        fs::read(attempt_journal(project.path()))
            .unwrap()
            .starts_with(&prefix)
    );
    assert_success(
        &run_kvist(project.path(), &["component", "accept", "."]),
        "accept after completed rejection record",
    );
}

#[test]
fn stale_rejection_preserves_identity_artifact_and_explicit_recovery_guards() {
    for condition in [
        "missing-reason",
        "commit",
        "wrong-task",
        "wrong-attempt",
        "older-attempt",
        "invalid-document",
        "recovery-fence",
        "wrong-event-task",
        "malformed-journal",
        "sibling-fence",
        "untracked",
        "symlink-journal",
    ] {
        let project = create_target_project("pending");
        write_finalizable_attempt(project.path(), true);
        retain_prepared_attempt(project.path());
        stale_requirements(project.path());
        let mut args = vec![
            "task",
            "finalize",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "block",
            "--reason",
            "Abandon inspected attempt.",
        ];
        match condition {
            "missing-reason" => args.truncate(6),
            "commit" => args.push("--commit"),
            "wrong-task" => args[3] = "unknown-task",
            "wrong-attempt" => args[4] = "attempt-9999",
            "older-attempt" => {
                let path = attempt_journal(project.path());
                let original = fs::read_to_string(&path).unwrap();
                fs::write(
                    path,
                    format!("{original}{}", original.replace(ATTEMPT_ID, "attempt-0002")),
                )
                .unwrap();
            }
            "invalid-document" => {
                fs::write(project.path().join("maerg/DESIGN.md"), "malformed\n").unwrap()
            }
            "recovery-fence" => {
                let path = project.path().join("maerg/TODOS.yaml");
                let mut queue =
                    kvist::task_queue::parse(&fs::read_to_string(&path).unwrap()).unwrap();
                queue.tasks[1].recovery_state = Some(kvist::task_queue::RecoveryState {
                    state: kvist::task_queue::RecoveryStateKind::Fenced,
                    attempt_id: ATTEMPT_ID.into(),
                    reason: "evidence-tampered".into(),
                });
                fs::write(path, kvist::task_queue::serialize(&queue).unwrap()).unwrap();
            }
            "wrong-event-task" => {
                let path = attempt_journal(project.path());
                let original = fs::read_to_string(&path).unwrap();
                fs::write(path, original.replace(TASK_ID, "other-task")).unwrap();
            }
            "malformed-journal" => fs::write(attempt_journal(project.path()), "{bad\n").unwrap(),
            "sibling-fence" => {
                add_independent_ready_implementation_task(project.path());
                let path = project.path().join("maerg/TODOS.yaml");
                let mut queue =
                    kvist::task_queue::parse(&fs::read_to_string(&path).unwrap()).unwrap();
                queue.tasks[2].status = kvist::task_queue::TaskStatus::InProgress;
                queue.tasks[2].recovery_state = Some(kvist::task_queue::RecoveryState {
                    state: kvist::task_queue::RecoveryStateKind::Fenced,
                    attempt_id: ATTEMPT_ID.into(),
                    reason: "unrelated-unresolved-attempt".into(),
                });
                fs::write(path, kvist::task_queue::serialize(&queue).unwrap()).unwrap();
            }
            "untracked" => assert_success(
                &git(project.path(), &["rm", "--cached", "maerg/DESIGN.md"]),
                "untrack intent",
            ),
            "symlink-journal" => {
                let path = attempt_journal(project.path());
                let retained = path.with_extension("retained");
                fs::rename(&path, &retained).unwrap();
                std::os::unix::fs::symlink(retained, path).unwrap();
            }
            _ => unreachable!(),
        }
        let queue_before = fs::read(project.path().join("maerg/TODOS.yaml")).unwrap();
        let journal_before = fs::read(attempt_journal(project.path())).unwrap();
        let output = run_kvist(project.path(), &args);
        assert!(
            !output.status.success(),
            "{condition} must refuse: {}",
            output_text(&output)
        );
        assert_eq!(
            fs::read(project.path().join("maerg/TODOS.yaml")).unwrap(),
            queue_before,
            "{condition}"
        );
        assert_eq!(
            fs::read(attempt_journal(project.path())).unwrap(),
            journal_before,
            "{condition}"
        );
    }
}

#[test]
fn ambiguous_effects_remain_fenced_and_cannot_be_force_unlocked() {
    let project = create_target_project("pending");
    write_ambiguous_attempt(project.path());
    let queue_path = project.path().join("maerg/TODOS.yaml");
    let before = fs::read(&queue_path).expect("read fenced queue");

    let recover = run_kvist(
        project.path(),
        &[
            "task",
            "recover",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "--disposition",
            "execution-did-not-start",
        ],
    );
    assert!(!recover.status.success());
    assert!(
        output_text(&recover).contains("fenced")
            || output_text(&recover).contains("ambiguous")
            || output_text(&recover).contains("source effect"),
        "ambiguous recovery refusal must be explicit: {}",
        output_text(&recover)
    );
    assert_eq!(
        fs::read(&queue_path).expect("read queue after refused recovery"),
        before
    );

    let unlock = run_kvist(project.path(), &["task", "unlock", ".", "--force"]);
    assert!(
        !unlock.status.success(),
        "force unlock must not bypass an ambiguous prepared attempt"
    );
    assert!(
        output_text(&unlock).contains("recover")
            || output_text(&unlock).contains("fenced")
            || output_text(&unlock).contains("ambiguous"),
        "unlock refusal must direct the user to recovery: {}",
        output_text(&unlock)
    );
}

#[test]
fn a_prepared_record_and_user_disposition_do_not_prove_execution_never_started() {
    let project = create_target_project("pending");
    let queue_path = project.path().join("maerg/TODOS.yaml");
    let attempts = project.path().join("maerg/.kvist-attempts");
    fs::create_dir_all(&attempts).expect("create attempts");
    let event = json!({
        "schema_version": 1,
        "attempt_id": ATTEMPT_ID,
        "task_id": TASK_ID,
        "phase": "prepared",
        "pre_queue_digest": sha256_file(&queue_path),
        "intended_post_queue_digest": sha256_file(&queue_path),
        "policy_identity": crate::support::sha256_bytes(b"policy"),
        "runner_identity": crate::support::sha256_bytes(b"runner"),
        "approved_write_scope": [{
            "path": "maerg/tests",
            "pre_digest": crate::support::sha256_bytes(b"empty")
        }]
    });
    fs::write(
        attempts.join(format!("{TASK_ID}.jsonl")),
        format!("{event}\n"),
    )
    .expect("write prepared event");

    let output = run_kvist(
        project.path(),
        &[
            "task",
            "recover",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "--disposition",
            "execution-did-not-start",
        ],
    );

    assert!(
        !output.status.success(),
        "a user assertion plus unchanged queue digests must remain fenced"
    );
    assert!(
        output_text(&output).contains("fenced")
            || output_text(&output).contains("evidence")
            || output_text(&output).contains("spawn"),
        "missing independent pre-spawn evidence must be explicit: {}",
        output_text(&output)
    );
    let queue = fs::read_to_string(queue_path).expect("read fenced queue");
    assert!(!task_block(&queue, TASK_ID).contains("status: completed"));
    let journal =
        fs::read_to_string(attempts.join(format!("{TASK_ID}.jsonl"))).expect("read journal");
    assert!(!journal.contains("\"phase\":\"recovered\""));
}

#[test]
fn recovery_can_use_independent_durable_pre_spawn_failure_evidence() {
    let project = create_target_project("pending");
    let approval = run_kvist(project.path(), &["task", "approve-policy"]);
    assert!(
        approval.status.success(),
        "controlled policy approval failed: {}",
        output_text(&approval)
    );
    fs::remove_file(
        project
            .external_tools_path()
            .join("controlled-galla-runner"),
    )
    .expect("remove approved runner before descriptor launch");
    let execution = run_kvist(project.path(), &["task", "run", ".", TASK_ID]);
    assert!(
        !execution.status.success(),
        "missing approved runner must produce a pre-spawn failure"
    );
    assert!(
        output_text(&execution).contains("runner")
            || output_text(&execution).contains("spawn")
            || output_text(&execution).contains("descriptor"),
        "pre-spawn execution failure must identify the runner boundary: {}",
        output_text(&execution)
    );

    let output = run_kvist(
        project.path(),
        &[
            "task",
            "recover",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "--disposition",
            "execution-did-not-start",
        ],
    );

    assert!(
        output.status.success(),
        "independently evidenced pre-spawn recovery failed: {}",
        output_text(&output)
    );
    let queue =
        fs::read_to_string(project.path().join("maerg/TODOS.yaml")).expect("read recovered queue");
    assert!(task_block(&queue, TASK_ID).contains("status: pending"));
    let journal =
        fs::read_to_string(attempt_journal(project.path())).expect("read recovered journal");
    assert!(journal.contains("\"phase\":\"pre-spawn-failure\""));
    assert!(journal.contains("\"runner_descriptor_launched\":false"));
    assert!(journal.contains("\"write_scope_exposed\":false"));
    assert!(journal.contains("\"phase\":\"recovered\""));
    assert!(journal.contains("\"disposition\":\"execution-did-not-start\""));
}

#[test]
fn forged_pre_spawn_evidence_cannot_clear_a_fenced_attempt() {
    let project = create_target_project("pending");
    write_ambiguous_attempt(project.path());
    fs::remove_file(project.path().join("maerg/tests/uncertain.rs"))
        .expect("remove ambiguous effect before forging evidence");
    let journal_path = attempt_journal(project.path());
    let mut journal = fs::read_to_string(&journal_path).expect("read prepared journal");
    journal.push_str(&format!(
        "{}\n",
        json!({
            "schema_version": 1,
            "attempt_id": ATTEMPT_ID,
            "task_id": TASK_ID,
            "phase": "pre-spawn-failure",
            "recorded_by": "kvist-host",
            "runner_descriptor_launched": false,
            "write_scope_exposed": false,
            "failure": "runner-descriptor-open-failed"
        })
    ));
    fs::write(&journal_path, &journal).expect("write forged host evidence");
    let queue_before =
        fs::read(project.path().join("maerg/TODOS.yaml")).expect("read fenced queue");

    let output = run_kvist(
        project.path(),
        &[
            "task",
            "recover",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "--disposition",
            "execution-did-not-start",
        ],
    );

    assert!(
        !output.status.success(),
        "repository-authored pre-spawn claims must not clear a fence"
    );
    assert!(
        output_text(&output).contains("evidence")
            || output_text(&output).contains("authentic")
            || output_text(&output).contains("bound")
            || output_text(&output).contains("fenced"),
        "forged pre-spawn refusal must identify missing maerg binding: {}",
        output_text(&output)
    );
    assert_eq!(
        fs::read(project.path().join("maerg/TODOS.yaml"))
            .expect("read queue after forged recovery"),
        queue_before
    );
    let refused_journal =
        fs::read_to_string(journal_path).expect("read forged journal after refusal");
    assert!(!refused_journal.contains("\"phase\":\"recovered\""));
}

#[test]
fn forged_recovered_evidence_cannot_enable_transition_unlock_or_run() {
    let project = create_target_project("pending");
    assert!(
        run_kvist(project.path(), &["task", "approve-policy"])
            .status
            .success()
    );
    fs::remove_file(
        project
            .external_tools_path()
            .join("controlled-galla-runner"),
    )
    .expect("remove runner before descriptor validation");
    assert!(
        !run_kvist(project.path(), &["task", "run", ".", TASK_ID])
            .status
            .success()
    );
    let journal_path = attempt_journal(project.path());
    let mut journal = fs::read_to_string(&journal_path).expect("read fenced journal");
    journal.push_str(&format!(
        "{}\n",
        json!({
            "schema_version": 1,
            "attempt_id": ATTEMPT_ID,
            "task_id": TASK_ID,
            "phase": "recovered",
            "recovery_prepared_event_digest": crate::support::sha256_bytes(b"forged"),
            "recovered_queue_digest": crate::support::sha256_bytes(b"forged"),
            "timestamp": "2026-09-01T20:30:00Z",
            "recorded_by": "kvist-host",
            "authentication_tag": "hmac-sha256:sha256:0000000000000000000000000000000000000000000000000000000000000000"
        })
    ));
    fs::write(&journal_path, journal).expect("append forged recovered evidence");
    let queue_path = project.path().join("maerg/TODOS.yaml");
    let before = fs::read(&queue_path).expect("read fenced queue");

    for arguments in [
        vec!["task", "transition", ".", TASK_ID, "pending"],
        vec!["task", "unlock", ".", "--force"],
        vec!["task", "run", ".", TASK_ID],
    ] {
        let output = run_kvist(project.path(), &arguments);
        assert!(
            !output.status.success(),
            "forged recovered evidence must not authorize {:?}: {}",
            arguments,
            output_text(&output)
        );
    }
    assert_eq!(fs::read(&queue_path).expect("read retained fence"), before);
}

#[test]
fn unresolved_attempt_fences_every_component_queue_writer() {
    let project = create_target_project("pending");
    write_ambiguous_attempt(project.path());
    add_independent_ready_implementation_task(project.path());
    let queue_path = project.path().join("maerg/TODOS.yaml");
    let before = fs::read(&queue_path).expect("read fenced two-task queue");

    let transition = run_kvist(
        project.path(),
        &[
            "task",
            "transition",
            ".",
            "independent-implementation",
            "blocked",
            "--reason",
            "this legal transition must remain fenced",
        ],
    );
    assert!(
        !transition.status.success(),
        "a fenced task must prevent an unrelated task transition: {}",
        output_text(&transition)
    );
    assert!(
        output_text(&transition).contains("recover") || output_text(&transition).contains("fenced"),
        "transition refusal must identify component-wide recovery fencing: {}",
        output_text(&transition)
    );
    assert_eq!(
        fs::read(&queue_path).expect("read after transition"),
        before
    );

    let accept = run_kvist(project.path(), &["component", "accept", "."]);
    assert!(
        !accept.status.success(),
        "a fenced task must prevent component acceptance from rewriting the queue: {}",
        output_text(&accept)
    );
    assert!(
        output_text(&accept).contains("recover") || output_text(&accept).contains("fenced"),
        "acceptance refusal must identify component-wide recovery fencing: {}",
        output_text(&accept)
    );
    assert_eq!(
        fs::read(&queue_path).expect("read after acceptance"),
        before
    );

    assert_success(
        &run_kvist(project.path(), &["task", "approve-policy"]),
        "approve task policy",
    );
    fs::remove_file(
        project
            .external_tools_path()
            .join("controlled-galla-runner"),
    )
    .expect("remove runner before independent automatic selection");
    let run = run_kvist(
        project.path(),
        &["task", "run", ".", "independent-implementation"],
    );
    assert!(
        !run.status.success(),
        "a fenced task must prevent task-run from fencing a selected sibling: {}",
        output_text(&run)
    );
    assert!(
        output_text(&run).contains("recover") || output_text(&run).contains("fenced"),
        "task-run refusal must identify component-wide recovery fencing: {}",
        output_text(&run)
    );
    assert_eq!(fs::read(&queue_path).expect("read after task run"), before);
}

#[test]
fn recovery_resumes_durable_partial_states_and_restores_the_pre_attempt_status() {
    let project = create_target_project("in-progress");
    assert!(
        run_kvist(project.path(), &["task", "approve-policy"])
            .status
            .success()
    );
    fs::remove_file(
        project
            .external_tools_path()
            .join("controlled-galla-runner"),
    )
    .expect("remove runner before descriptor validation");
    assert!(
        !run_kvist(project.path(), &["task", "run", ".", TASK_ID])
            .status
            .success()
    );
    let queue_path = project.path().join("maerg/TODOS.yaml");
    let fenced_queue = fs::read(&queue_path).expect("save fenced queue");
    let journal_path = attempt_journal(project.path());

    let first = run_kvist(
        project.path(),
        &[
            "task",
            "recover",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "--disposition",
            "execution-did-not-start",
        ],
    );
    assert!(
        first.status.success(),
        "initial recovery failed: {}",
        output_text(&first)
    );
    let recovered_queue = fs::read(&queue_path).expect("save recovered queue");
    assert!(
        task_block(&String::from_utf8_lossy(&recovered_queue), TASK_ID,)
            .contains("status: in-progress"),
        "recovery must restore an explicitly resumed task to in-progress"
    );
    let recovered_journal = fs::read_to_string(&journal_path).expect("read recovered journal");
    let without_recovered = recovered_journal
        .lines()
        .filter(|line| !line.contains("\"phase\":\"recovered\""))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    // These direct durable fault states model a crash after recovery-prepared,
    // first before queue replacement and then after it but before the commit record.
    fs::write(&queue_path, &fenced_queue).expect("restore fenced partial state");
    fs::write(&journal_path, &without_recovered).expect("remove recovered record");
    let before_replace = run_kvist(
        project.path(),
        &[
            "task",
            "recover",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "--disposition",
            "execution-did-not-start",
        ],
    );
    assert!(
        before_replace.status.success(),
        "fenced recovery-prepared state did not resume: {}",
        output_text(&before_replace)
    );
    assert_eq!(
        fs::read(&queue_path).expect("read resumed recovered queue"),
        recovered_queue
    );

    fs::write(&journal_path, &without_recovered).expect("model missing committed recovery");
    let before_commit = run_kvist(
        project.path(),
        &[
            "task",
            "recover",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "--disposition",
            "execution-did-not-start",
        ],
    );
    assert!(
        before_commit.status.success(),
        "recovered queue missing its record did not resume: {}",
        output_text(&before_commit)
    );
    assert_eq!(
        fs::read(&queue_path).expect("read idempotent recovered queue"),
        recovered_queue
    );
    assert_eq!(
        fs::read_to_string(&journal_path)
            .expect("read idempotent journal")
            .matches("\"phase\":\"recovered\"")
            .count(),
        1
    );
    let transition = run_kvist(
        project.path(),
        &[
            "task",
            "transition",
            ".",
            TASK_ID,
            "blocked",
            "--reason",
            "human review resumes after authenticated recovery",
        ],
    );
    assert!(
        transition.status.success(),
        "a complete authenticated chain must permit ordinary lifecycle work: {}",
        output_text(&transition)
    );
    let reset = run_kvist(
        project.path(),
        &["task", "transition", ".", TASK_ID, "pending"],
    );
    assert!(
        reset.status.success(),
        "a completed recovery chain must remain terminal after one legal queue update: {}",
        output_text(&reset)
    );
    let queue = fs::read_to_string(queue_path).expect("read queue after second lifecycle update");
    assert!(task_block(&queue, TASK_ID).contains("status: pending"));
}

#[test]
fn changed_scoped_file_after_journal_recording_refuses_finalization() {
    let project = create_target_project("pending");
    write_finalizable_attempt(project.path(), true);
    fs::write(
        project.path().join("maerg/tests/generated.rs"),
        "#[test]\nfn changed_after_recording() {}\n",
    )
    .expect("change scoped file after journal recording");

    let output = run_kvist(
        project.path(),
        &["task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept"],
    );

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("digest")
            || output_text(&output).contains("changed")
            || output_text(&output).contains("evidence"),
        "changed scoped file refusal must identify evidence drift: {}",
        output_text(&output)
    );
    assert_attempt_is_fenced_and_unfinalized(project.path());
}

#[test]
fn journal_claimed_out_of_scope_path_cannot_expand_engine_bound_scope() {
    let project = create_target_project("pending");
    write_finalizable_attempt(project.path(), true);
    fs::rename(
        project.path().join("maerg/tests/generated.rs"),
        project.path().join("maerg/src/out-of-scope.rs"),
    )
    .expect("move reported change outside maerg-approved scope");
    let journal_path = attempt_journal(project.path());
    let journal = fs::read_to_string(&journal_path)
        .expect("read attempt journal")
        .replace("maerg/tests/generated.rs", "maerg/src/out-of-scope.rs")
        .replace("\"path\":\"maerg/tests\"", "\"path\":\"maerg/src\"");
    fs::write(&journal_path, &journal).expect("forge expanded journal scope");

    let output = run_kvist(
        project.path(),
        &["task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept"],
    );

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("scope")
            || output_text(&output).contains("policy")
            || output_text(&output).contains("approved"),
        "out-of-scope refusal must use maerg-bound authority, not journal claims: {}",
        output_text(&output)
    );
    assert_attempt_is_fenced_and_unfinalized(project.path());
}

#[test]
fn mismatched_reported_post_digest_refuses_finalization() {
    let project = create_target_project("pending");
    write_finalizable_attempt(project.path(), true);
    let generated = project.path().join("maerg/tests/generated.rs");
    let actual_digest = sha256_file(&generated);
    let journal_path = attempt_journal(project.path());
    let journal = fs::read_to_string(&journal_path)
        .expect("read attempt journal")
        .replace(
            &actual_digest,
            &crate::support::sha256_bytes(b"forged post digest"),
        );
    assert!(!journal.contains(&actual_digest));
    fs::write(&journal_path, &journal).expect("write mismatched post digest");

    let output = run_kvist(
        project.path(),
        &["task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept"],
    );

    assert!(!output.status.success());
    assert!(
        output_text(&output).contains("digest")
            || output_text(&output).contains("evidence")
            || output_text(&output).contains("mismatch"),
        "post-digest refusal must be explicit: {}",
        output_text(&output)
    );
    assert_attempt_is_fenced_and_unfinalized(project.path());
}

#[test]
fn finalization_is_bound_to_the_exact_task_and_attempt_identity() {
    let project = create_target_project("pending");
    write_finalizable_attempt(project.path(), true);
    let queue_path = project.path().join("maerg/TODOS.yaml");
    let before = fs::read(&queue_path).expect("read queue");

    for arguments in [
        [
            "task",
            "finalize",
            ".",
            "different-task",
            ATTEMPT_ID,
            "accept",
        ],
        [
            "task",
            "finalize",
            ".",
            TASK_ID,
            "different-attempt",
            "accept",
        ],
    ] {
        let output = run_kvist(project.path(), &arguments);
        assert!(!output.status.success());
        assert!(
            !output_text(&output).contains("unrecognized subcommand"),
            "finalize must be a real command before identity checks are credited"
        );
        assert!(
            output_text(&output).contains("task")
                || output_text(&output).contains("attempt")
                || output_text(&output).contains("identity"),
            "identity mismatch must be explicit: {}",
            output_text(&output)
        );
        assert_eq!(fs::read(&queue_path).expect("read unchanged queue"), before);
    }
}
