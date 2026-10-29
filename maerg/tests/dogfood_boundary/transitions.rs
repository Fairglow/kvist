use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use kvist::task_queue::{TaskQueue, TaskStatus, parse, serialize};
use sha2::{Digest, Sha256};

use super::support::*;

const STATES: [(&str, TaskStatus); 5] = [
    ("pending", TaskStatus::Pending),
    ("in-progress", TaskStatus::InProgress),
    ("blocked", TaskStatus::Blocked),
    ("awaiting-decision", TaskStatus::AwaitingDecision),
    ("completed", TaskStatus::Completed),
];

const EDGES: [[bool; 5]; 5] = [
    [false, true, true, false, false],
    [true, false, true, true, true],
    [true, true, false, false, false],
    [true, true, false, false, false],
    [false, false, false, false, false],
];

fn queue_path(project: &Path) -> PathBuf {
    project.join("maerg/TODOS.yaml")
}

fn journal_path(project: &Path) -> PathBuf {
    project.join(format!("maerg/.kvist-attempts/{TASK_ID}.jsonl"))
}

fn read_queue(project: &Path) -> TaskQueue {
    parse(&fs::read_to_string(queue_path(project)).unwrap()).unwrap()
}

fn set_status(project: &Path, status: TaskStatus, dependencies_complete: bool) -> TaskQueue {
    let mut queue = read_queue(project);
    queue.tasks[0].status = if dependencies_complete {
        TaskStatus::Completed
    } else {
        TaskStatus::Pending
    };
    queue.tasks[0].timestamps.completed_at =
        dependencies_complete.then(|| queue.tasks[0].timestamps.updated_at.clone());
    let task = &mut queue.tasks[1];
    task.status = status;
    task.timestamps.completed_at =
        (status == TaskStatus::Completed).then(|| task.timestamps.updated_at.clone());
    task.blocked_reason = matches!(status, TaskStatus::Blocked | TaskStatus::AwaitingDecision)
        .then(|| "human decision remains recorded".to_owned());
    fs::write(queue_path(project), serialize(&queue).unwrap()).unwrap();
    queue
}

fn stale_requirements(project: &Path) {
    let path = project.join("maerg/REQUIREMENTS.md");
    let contents = fs::read_to_string(&path).unwrap();
    fs::write(path, format!("{contents}\nHuman intent clarification.\n")).unwrap();
}

#[test]
fn transition_matrix_covers_every_pair_reason_and_dependency_gate_without_refusal_mutation() {
    let mut failures = Vec::new();
    let mut cases = 0;
    for (from_index, (from_name, from)) in STATES.iter().enumerate() {
        for (to_index, (to_name, to)) in STATES.iter().enumerate() {
            for dependencies_complete in [false, true] {
                let project = create_target_project("pending");
                let initial = set_status(project.path(), *from, dependencies_complete);
                let before = fs::read(queue_path(project.path())).unwrap();
                for reason in [None, Some(""), Some(" \t "), Some("human decision")] {
                    fs::write(queue_path(project.path()), &before).unwrap();
                    match fs::remove_file(journal_path(project.path())) {
                        Ok(()) => {}
                        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
                    }
                    let mut arguments = vec!["task", "transition", ".", TASK_ID, to_name];
                    if let Some(reason) = reason {
                        arguments.extend(["--reason", reason]);
                    }
                    let needs_reason =
                        matches!(to, TaskStatus::Blocked | TaskStatus::AwaitingDecision);
                    let expected = EDGES[from_index][to_index]
                        && (*to != TaskStatus::InProgress || dependencies_complete)
                        && if needs_reason {
                            reason.is_some_and(|value| !value.trim().is_empty())
                        } else {
                            reason.is_none()
                        };
                    let output = run_kvist(project.path(), &arguments);
                    cases += 1;
                    if output.status.success() != expected {
                        failures.push(format!(
                            "{from_name} -> {to_name}, dependencies={dependencies_complete}, reason={reason:?}: {}",
                            output_text(&output)
                        ));
                    }
                    if !output.status.success() {
                        assert_eq!(fs::read(queue_path(project.path())).unwrap(), before);
                        assert!(!journal_path(project.path()).exists());
                        continue;
                    }
                    let observed = read_queue(project.path());
                    let mut expected_queue = initial.clone();
                    let task = &mut expected_queue.tasks[1];
                    task.status = *to;
                    task.timestamps.updated_at = observed.tasks[1].timestamps.updated_at.clone();
                    task.timestamps.completed_at =
                        (*to == TaskStatus::Completed).then(|| task.timestamps.updated_at.clone());
                    task.blocked_reason = needs_reason.then(|| reason.unwrap().to_owned());
                    assert_eq!(observed, expected_queue);
                    let events: Vec<serde_json::Value> =
                        fs::read_to_string(journal_path(project.path()))
                            .unwrap()
                            .lines()
                            .map(|line| serde_json::from_str(line).unwrap())
                            .collect();
                    assert_eq!(events.len(), 2);
                    assert_eq!(events[0]["phase"], "prepared");
                    assert_eq!(events[1]["phase"], "committed");
                    for event in events {
                        assert_eq!(event["from_status"], *from_name);
                        assert_eq!(event["to_status"], *to_name);
                        assert_eq!(event["reason"], serde_json::json!(reason));
                        assert_eq!(
                            event["timestamp"],
                            serde_json::json!(observed.tasks[1].timestamps.updated_at)
                        );
                    }
                }
            }
        }
    }
    assert_eq!(cases, 200);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn stale_intent_refuses_every_ordinary_transition_without_mutation() {
    for (from_name, from) in STATES {
        let project = create_target_project("pending");
        set_status(project.path(), from, true);
        stale_requirements(project.path());
        let before = fs::read(queue_path(project.path())).unwrap();
        for (to_name, to) in STATES {
            let mut arguments = vec!["task", "transition", ".", TASK_ID, to_name];
            if matches!(to, TaskStatus::Blocked | TaskStatus::AwaitingDecision) {
                arguments.extend(["--reason", "human decision"]);
            }
            let output = run_kvist(project.path(), &arguments);
            assert!(!output.status.success(), "{from_name} -> {to_name}");
            assert!(
                output_text(&output).contains("stale"),
                "{}",
                output_text(&output)
            );
            assert_eq!(fs::read(queue_path(project.path())).unwrap(), before);
            assert!(!journal_path(project.path()).exists());
        }
    }
}

#[test]
fn malformed_missing_unsupported_and_untracked_artifacts_keep_all_lifecycle_gates_closed() {
    for defect in ["invalid", "missing", "unsupported", "untracked"] {
        let project = create_target_project("pending");
        let path = project.path().join("maerg/CONTRACT.md");
        match defect {
            "invalid" => fs::write(&path, "invalid document\n").unwrap(),
            "missing" => fs::remove_file(&path).unwrap(),
            "unsupported" => {
                let contents = fs::read_to_string(&path).unwrap();
                fs::write(
                    &path,
                    contents.replace("kvist-contract-version: 1", "kvist-contract-version: 2"),
                )
                .unwrap();
            }
            "untracked" => assert_success(
                &git(
                    project.path(),
                    &["rm", "--cached", "--", "maerg/CONTRACT.md"],
                ),
                "untrack fixture contract",
            ),
            _ => unreachable!(),
        }
        let before = fs::read(queue_path(project.path())).unwrap();
        let commands = [
            vec!["component", "accept", "."],
            vec!["task", "transition", ".", TASK_ID, "in-progress"],
            vec!["task", "next", "."],
            vec!["task", "run", ".", TASK_ID],
            vec!["task", "unlock", ".", "--force"],
            vec![
                "task",
                "recover",
                ".",
                TASK_ID,
                ATTEMPT_ID,
                "--disposition",
                "execution-did-not-start",
            ],
            vec![
                "task",
                "finalize",
                ".",
                TASK_ID,
                ATTEMPT_ID,
                "block",
                "--reason",
                "human rejection",
            ],
            vec!["task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept"],
            vec!["task", "log", ".", TASK_ID],
        ];
        for arguments in commands {
            let output = run_kvist(project.path(), &arguments);
            assert!(
                !output.status.success(),
                "{defect}: {arguments:?}: {}",
                output_text(&output)
            );
            assert_eq!(fs::read(queue_path(project.path())).unwrap(), before);
            assert!(!journal_path(project.path()).exists());
        }
    }
}

fn isolated_command(project: &Path, state: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .current_dir(project)
        .env("XDG_STATE_HOME", state)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn stale_lock_cleanup_is_available_in_current_blocked_and_stale_states_but_protects_live_owners() {
    for component_state in ["current", "blocked", "stale"] {
        let project = create_target_project("pending");
        if component_state == "blocked" {
            set_status(project.path(), TaskStatus::Blocked, true);
        } else if component_state == "stale" {
            stale_requirements(project.path());
        }
        let state = temporary_directory("kvist-transition-lock-state-");
        let directory = state.path().join("kvist/task-locks-v1");
        fs::create_dir_all(&directory).unwrap();
        let identity = format!(
            "{}\n{}",
            project.path().canonicalize().unwrap().display(),
            project
                .path()
                .join("maerg")
                .canonicalize()
                .unwrap()
                .display()
        );
        let lock = directory.join(format!(
            "{}.lock",
            hex::encode(Sha256::digest(identity.as_bytes()))
        ));
        let before = fs::read(queue_path(project.path())).unwrap();
        for owner in ["live", "malformed", "dead"] {
            let contents = if owner == "malformed" {
                "pid: 999999999\n".to_owned()
            } else {
                let pid = if owner == "live" {
                    std::process::id()
                } else {
                    999_999_999
                };
                format!(
                    "schema_version: 1\nstarted_at: 2026-09-01T20:30:00Z\ntask_id: \"{TASK_ID}\"\npid: {pid}\nnonce: 00000000000000000000000000000000\n"
                )
            };
            fs::write(&lock, &contents).unwrap();
            let output = isolated_command(
                project.path(),
                state.path(),
                &["task", "unlock", ".", "--force"],
            );
            assert_eq!(
                output.status.success(),
                owner == "dead",
                "{component_state}/{owner}: {}",
                output_text(&output)
            );
            if owner == "dead" {
                assert!(!lock.exists());
            } else {
                assert_eq!(fs::read_to_string(&lock).unwrap(), contents);
            }
            assert_eq!(fs::read(queue_path(project.path())).unwrap(), before);
            assert!(!journal_path(project.path()).exists());
        }
        assert_success(
            &isolated_command(
                project.path(),
                state.path(),
                &["task", "unlock", ".", "--force"],
            ),
            "repeat cleanup with no lock",
        );
        if component_state == "stale" {
            assert_success(
                &isolated_command(project.path(), state.path(), &["component", "accept", "."]),
                "accept intent after stale lock cleanup",
            );
            write_ambiguous_attempt(project.path());
            stale_requirements(project.path());
            let queue = fs::read(queue_path(project.path())).unwrap();
            let journal = fs::read(journal_path(project.path())).unwrap();
            fs::write(&lock, format!(
                "schema_version: 1\nstarted_at: 2026-09-01T20:30:00Z\ntask_id: \"{TASK_ID}\"\npid: 999999999\nnonce: 00000000000000000000000000000000\n"
            )).unwrap();
            let output = isolated_command(
                project.path(),
                state.path(),
                &["task", "unlock", ".", "--force"],
            );
            assert!(!output.status.success());
            assert!(output_text(&output).contains("stale operation lock was removed"));
            assert!(output_text(&output).contains("unresolved"));
            assert!(!lock.exists());
            assert_eq!(fs::read(queue_path(project.path())).unwrap(), queue);
            assert_eq!(fs::read(journal_path(project.path())).unwrap(), journal);
        }
    }
}

#[test]
fn authenticated_no_launch_recovery_precedes_acceptance_when_intent_is_stale() {
    for pre_status in ["pending", "in-progress"] {
        let project = create_target_project(pre_status);
        assert_success(
            &run_kvist(project.path(), &["task", "approve-policy"]),
            "approve no-launch fixture",
        );
        fs::remove_file(
            project
                .external_tools_path()
                .join("controlled-galla-runner"),
        )
        .unwrap();
        let failed = run_kvist(project.path(), &["task", "run", ".", TASK_ID]);
        assert!(
            !failed.status.success(),
            "missing runner must fail before launch"
        );
        let fenced = read_queue(project.path());
        assert!(fenced.tasks[1].recovery_state.is_some());
        let retained = fs::read(journal_path(project.path())).unwrap();
        stale_requirements(project.path());
        let accept = run_kvist(project.path(), &["component", "accept", "."]);
        assert!(
            !accept.status.success(),
            "unresolved attempt must fence acceptance"
        );
        let arguments = [
            "task",
            "recover",
            ".",
            TASK_ID,
            ATTEMPT_ID,
            "--disposition",
            "execution-did-not-start",
        ];
        let recovered = run_kvist(project.path(), &arguments);
        assert_success(
            &recovered,
            "recover authenticated no-launch proof before acceptance",
        );
        let queue = read_queue(project.path());
        assert_eq!(
            queue.component, fenced.component,
            "recovery must not accept intent"
        );
        assert_eq!(
            queue.tasks[1].status,
            if pre_status == "pending" {
                TaskStatus::Pending
            } else {
                TaskStatus::InProgress
            }
        );
        assert!(queue.tasks[1].recovery_state.is_none());
        let journal = fs::read(journal_path(project.path())).unwrap();
        assert!(journal.starts_with(&retained));
        assert_success(
            &run_kvist(project.path(), &arguments),
            "repeat exact recovered decision",
        );
        assert_eq!(fs::read(journal_path(project.path())).unwrap(), journal);
        let transition = run_kvist(
            project.path(),
            &[
                "task",
                "transition",
                ".",
                TASK_ID,
                "blocked",
                "--reason",
                "review intent",
            ],
        );
        assert!(!transition.status.success());
        assert!(output_text(&transition).contains("stale"));
        assert_success(
            &run_kvist(project.path(), &["component", "accept", "."]),
            "accept changed intent after authenticated recovery",
        );
        assert_success(
            &run_kvist(
                project.path(),
                &[
                    "task",
                    "transition",
                    ".",
                    TASK_ID,
                    "blocked",
                    "--reason",
                    "review completed",
                ],
            ),
            "resume ordinary transitions only after acceptance",
        );
    }
}

#[test]
fn explicitly_resumed_decision_attempt_retains_valid_evidence_for_later_disposition() {
    let project = create_target_project("pending");
    set_status(project.path(), TaskStatus::AwaitingDecision, true);
    assert_success(
        &run_kvist(project.path(), &["task", "approve-policy"]),
        "approve resumed decision fixture",
    );
    assert_success(
        &run_kvist(project.path(), &["task", "run", ".", TASK_ID]),
        "explicitly resume a decided task",
    );
    let events: Vec<serde_json::Value> = fs::read_to_string(journal_path(project.path()))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(events.iter().any(|event| event["phase"] == "prepared"
        && event["schema_version"] == 1
        && event["pre_status"] == "awaiting-decision"));
    assert_success(
        &run_kvist(
            project.path(),
            &[
                "task",
                "finalize",
                ".",
                TASK_ID,
                ATTEMPT_ID,
                "block",
                "--reason",
                "review retained work",
            ],
        ),
        "dispose resumed decision attempt",
    );
    assert_success(
        &run_kvist(project.path(), &["component", "accept", "."]),
        "authenticate resumed-attempt history during acceptance",
    );
    assert_success(
        &run_kvist(
            project.path(),
            &["task", "transition", ".", TASK_ID, "pending"],
        ),
        "reopen disposed decision attempt",
    );
}

#[test]
fn blocked_guidance_names_a_permitted_nonexecuting_transition() {
    let project = create_target_project("pending");
    set_status(project.path(), TaskStatus::Blocked, true);
    let status = run_kvist(project.path(), &["status"]);
    assert_success(&status, "inspect blocked guidance");
    assert!(
        output_text(&status).contains(&format!("kvist task transition . {TASK_ID} pending")),
        "{}",
        output_text(&status)
    );
    let next = run_kvist(project.path(), &["task", "next", "."]);
    assert!(!next.status.success());
    assert!(!output_text(&next).contains("pending --reason"));
    assert!(!output_text(&next).contains("finalize"));
    assert_success(
        &run_kvist(
            project.path(),
            &["task", "transition", ".", TASK_ID, "pending"],
        ),
        "follow the displayed reopening command",
    );
    assert_success(
        &run_kvist(project.path(), &["task", "next", "."]),
        "select only after human reopening",
    );
}

#[test]
fn acceptance_refuses_an_older_attempt_and_mismatched_event_identity_without_mutation() {
    for defect in [
        "older-attempt",
        "task-identity",
        "schema",
        "conflicting-disposition",
    ] {
        let project = create_target_project("pending");
        assert_success(
            &run_kvist(project.path(), &["task", "approve-policy"]),
            "approve acceptance identity fixture",
        );
        assert_success(
            &run_kvist(project.path(), &["task", "run", ".", TASK_ID]),
            "record authentic acceptance fixture",
        );
        let path = journal_path(project.path());
        let mut events: Vec<serde_json::Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        match defect {
            "older-attempt" => {
                let mut next = events
                    .iter()
                    .find(|event| event["schema_version"] == 1 && event["phase"] == "prepared")
                    .unwrap()
                    .clone();
                next["attempt_id"] = "attempt-0002".into();
                events.push(next);
            }
            "task-identity" => {
                let event = events
                    .iter_mut()
                    .find(|event| event["attempt_id"] == ATTEMPT_ID)
                    .unwrap();
                event["task_id"] = "different-task".into();
            }
            "schema" => {
                let event = events
                    .iter_mut()
                    .find(|event| event["attempt_id"] == ATTEMPT_ID)
                    .unwrap();
                event["schema_version"] = 2.into();
            }
            "conflicting-disposition" => events.push(serde_json::json!({
                "schema_version": 1, "task_id": TASK_ID, "attempt_id": ATTEMPT_ID,
                "phase": "human-finalized", "disposition": "blocked", "reason": "human rejection"
            })),
            _ => unreachable!(),
        }
        let journal = events
            .iter()
            .map(|event| format!("{event}\n"))
            .collect::<String>();
        fs::write(&path, &journal).unwrap();
        let queue = fs::read(queue_path(project.path())).unwrap();
        let output = run_kvist(
            project.path(),
            &["task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept"],
        );
        assert!(
            !output.status.success(),
            "{defect}: {}",
            output_text(&output)
        );
        assert_eq!(fs::read(queue_path(project.path())).unwrap(), queue);
        assert_eq!(fs::read_to_string(&path).unwrap(), journal);
    }
}

#[test]
fn acceptance_finishes_interrupted_recording_and_repeats_without_overwriting_evidence() {
    let project = create_target_project("pending");
    assert_success(
        &run_kvist(project.path(), &["task", "approve-policy"]),
        "approve acceptance recovery fixture",
    );
    assert_success(
        &run_kvist(project.path(), &["task", "run", ".", TASK_ID]),
        "record acceptance recovery fixture",
    );
    let path = journal_path(project.path());
    let before = fs::read(&path).unwrap();
    let arguments = ["task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept"];
    assert_success(&run_kvist(project.path(), &arguments), "accept fixture");
    let queue = fs::read(queue_path(project.path())).unwrap();
    let journal = fs::read(&path).unwrap();
    assert!(journal.starts_with(&before));
    assert_success(
        &run_kvist(project.path(), &arguments),
        "repeat identical acceptance",
    );
    assert_eq!(fs::read(queue_path(project.path())).unwrap(), queue);
    assert_eq!(fs::read(&path).unwrap(), journal);

    fs::write(&path, &before).unwrap();
    assert_success(
        &run_kvist(project.path(), &arguments),
        "finish queue-first acceptance after interruption before final event",
    );
    assert_eq!(fs::read(queue_path(project.path())).unwrap(), queue);
    assert_eq!(fs::read(&path).unwrap(), journal);
}

#[test]
fn accepting_one_task_cannot_destroy_another_tasks_authenticated_recovery_pre_state() {
    let project = create_target_project("pending");
    add_independent_ready_implementation_task(project.path());
    assert_success(
        &run_kvist(project.path(), &["task", "approve-policy"]),
        "approve sibling recovery fixture",
    );
    assert_success(
        &run_kvist(project.path(), &["task", "run", ".", TASK_ID]),
        "leave first task awaiting disposition",
    );
    fs::remove_file(
        project
            .external_tools_path()
            .join("controlled-galla-runner"),
    )
    .unwrap();
    let failed = run_kvist(
        project.path(),
        &["task", "run", ".", "independent-implementation"],
    );
    assert!(!failed.status.success());
    assert!(read_queue(project.path()).tasks[2].recovery_state.is_some());
    let queue = fs::read(queue_path(project.path())).unwrap();
    let journal = fs::read(journal_path(project.path())).unwrap();
    let sibling_path = project
        .path()
        .join("maerg/.kvist-attempts/independent-implementation.jsonl");
    let sibling = fs::read(&sibling_path).unwrap();
    let accept = ["task", "finalize", ".", TASK_ID, ATTEMPT_ID, "accept"];
    let refused = run_kvist(project.path(), &accept);
    assert!(!refused.status.success(), "{}", output_text(&refused));
    assert!(output_text(&refused).contains("unresolved"));
    assert_eq!(fs::read(queue_path(project.path())).unwrap(), queue);
    assert_eq!(fs::read(journal_path(project.path())).unwrap(), journal);
    assert_eq!(fs::read(&sibling_path).unwrap(), sibling);
    assert_success(
        &run_kvist(
            project.path(),
            &[
                "task",
                "recover",
                ".",
                "independent-implementation",
                ATTEMPT_ID,
                "--disposition",
                "execution-did-not-start",
            ],
        ),
        "reconcile sibling before changing its recovery pre-state",
    );
    assert_success(
        &run_kvist(project.path(), &accept),
        "accept first task after sibling recovery",
    );
}

#[test]
fn starting_or_resuming_requires_transitive_dependencies_even_when_direct_parent_is_completed() {
    for from in [
        TaskStatus::Pending,
        TaskStatus::Blocked,
        TaskStatus::AwaitingDecision,
    ] {
        let project = create_target_project("pending");
        let mut queue = set_status(project.path(), from, true);
        let mut earliest = queue.tasks[0].clone();
        earliest.id = "earliest-test".into();
        earliest.status = TaskStatus::Pending;
        earliest.timestamps.completed_at = None;
        queue.tasks[0].depends_on.push(earliest.id.clone());
        queue.tasks.insert(0, earliest);
        let before = serialize(&queue).unwrap();
        fs::write(queue_path(project.path()), &before).unwrap();
        let output = run_kvist(
            project.path(),
            &["task", "transition", ".", TASK_ID, "in-progress"],
        );
        assert!(!output.status.success());
        assert!(output_text(&output).contains("dependency-chain"));
        assert_eq!(
            fs::read_to_string(queue_path(project.path())).unwrap(),
            before
        );
        assert!(!journal_path(project.path()).exists());
    }
}
