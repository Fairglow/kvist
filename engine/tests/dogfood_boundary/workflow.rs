use std::fs;

use serde_json::json;

use crate::support::{
    ATTEMPT_ID, TASK_ID, add_independent_ready_implementation_task, assert_success,
    create_target_project, git, output_text, run_kvist, sha256_file, task_block,
    write_ambiguous_attempt, write_finalizable_attempt,
};

fn attempt_journal(project: &std::path::Path) -> std::path::PathBuf {
    project.join(format!("engine/.kvist-attempts/{TASK_ID}.jsonl"))
}

fn assert_attempt_is_fenced_and_unfinalized(project: &std::path::Path) {
    let queue = fs::read_to_string(project.join("engine/TODOS.yaml"))
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
fn supervised_cli_requires_explicit_task_and_exposes_recovery_and_finalization() {
    let project = create_target_project("pending");

    let missing_task = run_kvist(project.path(), &["task", "run", "."]);
    assert!(
        !missing_task.status.success(),
        "supervised execution must not automatically select a task"
    );
    assert!(
        output_text(&missing_task).contains("TASK_ID")
            || output_text(&missing_task).contains("task id"),
        "missing-task diagnostic must request an explicit task ID: {}",
        output_text(&missing_task)
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
    assert!(help.contains("TASK_ID"));
    assert!(
        !help.contains("[TASK_ID]"),
        "TASK_ID must be required, not optional"
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
            .join("engine/tests/supervised-run-started")
            .is_file()
            || project
                .path()
                .join("engine/tests/supervised-agent-started")
                .is_file(),
        "the controlled runner or authoring command must prove task execution started"
    );
    let queue = fs::read_to_string(project.path().join("engine/TODOS.yaml"))
        .expect("read pending-disposition queue");
    let task = task_block(&queue, TASK_ID);
    assert!(
        task.contains("status: in-progress"),
        "successful execution evidence alone must not complete the task"
    );
    assert!(!task.contains("status: completed"));
    let journal_path = project
        .path()
        .join(format!("engine/.kvist-attempts/{TASK_ID}.jsonl"));
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
        fs::read_to_string(project.path().join("engine/TODOS.yaml")).expect("read final queue");
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
        fs::read_to_string(project.path().join("engine/TODOS.yaml")).expect("read blocked queue");
    assert!(queue.contains("status: blocked"));
    assert!(queue.contains("human review rejected the scoped change"));
    let journal = fs::read_to_string(
        project
            .path()
            .join(format!("engine/.kvist-attempts/{TASK_ID}.jsonl")),
    )
    .expect("read final attempt journal");
    assert!(journal.contains("\"disposition\":\"blocked\""));
    assert_eq!(
        journal.matches("\"attempt_id\":\"attempt-0001\"").count(),
        5,
        "finalization must append one event to the existing attempt, not retry it"
    );
}

#[test]
fn ambiguous_effects_remain_fenced_and_cannot_be_force_unlocked() {
    let project = create_target_project("pending");
    write_ambiguous_attempt(project.path());
    let queue_path = project.path().join("engine/TODOS.yaml");
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
    let queue_path = project.path().join("engine/TODOS.yaml");
    let attempts = project.path().join("engine/.kvist-attempts");
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
            "path": "engine/tests",
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
            .join("controlled-sandbox-runner"),
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
        fs::read_to_string(project.path().join("engine/TODOS.yaml")).expect("read recovered queue");
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
    fs::remove_file(project.path().join("engine/tests/uncertain.rs"))
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
        fs::read(project.path().join("engine/TODOS.yaml")).expect("read fenced queue");

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
        "forged pre-spawn refusal must identify missing engine binding: {}",
        output_text(&output)
    );
    assert_eq!(
        fs::read(project.path().join("engine/TODOS.yaml"))
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
            .join("controlled-sandbox-runner"),
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
    let queue_path = project.path().join("engine/TODOS.yaml");
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
    let queue_path = project.path().join("engine/TODOS.yaml");
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
            .join("controlled-sandbox-runner"),
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
            .join("controlled-sandbox-runner"),
    )
    .expect("remove runner before descriptor validation");
    assert!(
        !run_kvist(project.path(), &["task", "run", ".", TASK_ID])
            .status
            .success()
    );
    let queue_path = project.path().join("engine/TODOS.yaml");
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
        project.path().join("engine/tests/generated.rs"),
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
        project.path().join("engine/tests/generated.rs"),
        project.path().join("engine/src/out-of-scope.rs"),
    )
    .expect("move reported change outside engine-approved scope");
    let journal_path = attempt_journal(project.path());
    let journal = fs::read_to_string(&journal_path)
        .expect("read attempt journal")
        .replace("engine/tests/generated.rs", "engine/src/out-of-scope.rs")
        .replace("\"path\":\"engine/tests\"", "\"path\":\"engine/src\"");
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
        "out-of-scope refusal must use engine-bound authority, not journal claims: {}",
        output_text(&output)
    );
    assert_attempt_is_fenced_and_unfinalized(project.path());
}

#[test]
fn mismatched_reported_post_digest_refuses_finalization() {
    let project = create_target_project("pending");
    write_finalizable_attempt(project.path(), true);
    let generated = project.path().join("engine/tests/generated.rs");
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
    let queue_path = project.path().join("engine/TODOS.yaml");
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
