use super::*;
use serde_json::json;

fn draft(reference: &str) -> Value {
    let stage = json!({
        "description":"Limit work to the local status renderer and its tests.",
        "expected_outcome":"Record the named check for empty queue output and its result."
    });
    json!({
        "schema_version":1,
        "rationale":"Keep empty and blocked queues distinguishable.",
        "items":[{
            "id":"empty-queue-output",
            "title":"Distinguish empty queue output",
            "context":"The current task listing has no explicit empty-state fixture.",
            "purpose":"Help operators know why there is no ready work.",
            "requirements":[reference],
            "write_tests":stage,
            "implement_code":stage,
            "security_audit":stage,
            "compliance_review":stage
        }]
    })
}

fn stage(fixture: &Fixture, body: Value) -> Result<super::super::super::Output> {
    let file = fixture.project.path().join("idea.json");
    fs::write(&file, body.to_string()).unwrap();
    super::super::super::planning::stage(
        fixture.project.path(),
        Path::new("."),
        None,
        Some(&file),
        Some("Explicit offline review exception"),
    )
}

#[test]
fn fresh_queue_plans_require_exact_approval_and_schedule_only() {
    let fixture = Fixture::new();
    let path = fixture.component().join("TODOS.yaml");
    let mut before = fixture.input().queue;
    before.tasks.clear();
    let before_text = serialize(&before).unwrap();
    fs::write(&path, &before_text).unwrap();
    let staged = stage(&fixture, draft("REQUIREMENTS.md#Functional-requirements")).unwrap();
    let id = staged.plan_id.as_deref().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), before_text);
    assert_eq!(staged.actions.len(), 1);
    assert!(staged.message.contains("kvist decision . do "));
    assert!(staged.message.contains("empty-queue-output-write-tests"));
    assert!(apply(fixture.project.path(), Path::new("."), id, "wrong").is_err());
    let plan = load_plan(fixture.project.path(), Path::new("."), id).unwrap();
    assert_eq!(plan.input.files["TODOS.yaml"], before_text);
    assert_eq!(plan.review.report, "Explicit offline review exception");
    assert!(plan.review.exception);
    super::super::super::execute(
        fixture.project.path(),
        Path::new("."),
        Some(Action::Do {
            number: staged.actions[0].number,
        }),
    )
    .unwrap();
    let queue = parse(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        queue
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        [
            "empty-queue-output-write-tests",
            "empty-queue-output-implement-code",
            "empty-queue-output-security-audit",
            "empty-queue-output-compliance-review",
        ]
    );
    for (index, task) in queue.tasks.iter().enumerate() {
        assert_eq!(task.status, TaskStatus::Pending);
        assert!(task.acceptance_id.is_none());
        assert!(task.timestamps.completed_at.is_none());
        if index != 0 {
            assert_eq!(task.depends_on, [queue.tasks[index - 1].id.clone()]);
        }
    }
}

#[test]
fn standalone_preflight_rejects_unknown_references_and_model_owned_state() {
    let fixture = Fixture::new();
    for reference in [
        "REQUIREMENTS.md#Missing",
        "../REQUIREMENTS.md#Functional-requirements",
        "CONTRACT.md#Consumer-visible-contract",
    ] {
        assert!(stage(&fixture, draft(reference)).is_err(), "{reference}");
    }
    let mut invalid = draft("REQUIREMENTS.md#Functional-requirements");
    invalid["items"][0]["write_tests"]["status"] = json!("completed");
    assert!(stage(&fixture, invalid).is_err());
    assert_eq!(fixture.input().queue.tasks.len(), 4);
}

#[test]
fn standalone_apply_rejects_drift_and_preserves_a_paused_task() {
    let fixture = Fixture::new();
    let before = fixture.input().queue;
    let staged = stage(&fixture, draft("REQUIREMENTS.md#Functional-requirements")).unwrap();
    let path = fixture.component().join("TODOS.yaml");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{original}\n# External change\n")).unwrap();
    let approval = staged.approval.as_deref().unwrap();
    let id = staged.plan_id.as_deref().unwrap();
    assert!(apply(fixture.project.path(), Path::new("."), id, approval).is_err());
    fs::write(&path, original).unwrap();
    apply(fixture.project.path(), Path::new("."), id, approval).unwrap();
    let after = parse(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(&after.tasks[..before.tasks.len()], before.tasks);
}

#[test]
fn large_realistic_queue_can_stage_and_apply_without_exceeding_input_bounds() {
    let fixture = Fixture::new();
    let path = fixture.component().join("TODOS.yaml");
    let mut before = fixture.input().queue;
    for index in 0..96 {
        let mut task = before.tasks[0].clone();
        task.id = format!("historical-{index}-write-tests");
        task.status = TaskStatus::Completed;
        task.blocked_reason = None;
        task.timestamps.completed_at = Some(task.timestamps.updated_at.clone());
        task.context = "Prior fixture details. ".repeat(120).trim_end().to_owned();
        task.expected_outcome = "Prior outcome details. ".repeat(70).trim_end().to_owned();
        before.tasks.push(task);
    }
    let text = serialize(&before).unwrap();
    assert!(text.len() > 400 * 1024);
    fs::write(&path, text).unwrap();
    let staged = stage(&fixture, draft("REQUIREMENTS.md#Functional-requirements")).unwrap();
    let id = staged.plan_id.as_deref().unwrap();
    assert!(
        fs::metadata(
            fixture
                .component()
                .join(DIRECTORY)
                .join(format!("{id}.json"))
        )
        .unwrap()
        .len()
            > MAX_FILE
    );
    apply(
        fixture.project.path(),
        Path::new("."),
        id,
        staged.approval.as_deref().unwrap(),
    )
    .unwrap();
    let after = parse(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(&after.tasks[..before.tasks.len()], before.tasks);
}

#[test]
fn idea_is_reviewed_after_local_intent_edits_are_materialized() {
    let fixture = Fixture::new();
    let before = fixture.input();
    let mut body = draft("REQUIREMENTS.md#REQ-EMPTY-QUEUE");
    body["document_edits"] = json!([{
        "path":"REQUIREMENTS.md",
        "before":before.files["REQUIREMENTS.md"],
        "after":format!("{}\n### REQ-EMPTY-QUEUE\n\nShow a distinct message for an empty queue.\n", before.files["REQUIREMENTS.md"])
    }]);
    let mut calls = Vec::new();
    let output = super::super::super::planning::stage_with(
        fixture.project.path(),
        Path::new("."),
        Some("Distinguish empty queues"),
        None,
        None,
        |_, _, phase, evidence| {
            calls.push(phase);
            assert!(!evidence.to_string().contains("pub fn retained"));
            match phase {
                "decision-queue-plan" => {
                    assert_eq!(evidence["human_request"], "Distinguish empty queues");
                    Ok(body.to_string())
                }
                "decision-intent-review" => {
                    assert!(
                        evidence["materialized_candidate"]["REQUIREMENTS.md"]
                            .as_str()
                            .unwrap()
                            .contains("### REQ-EMPTY-QUEUE")
                    );
                    assert!(
                        evidence["materialized_candidate"]["TASK_PROJECTION"]
                            .as_str()
                            .unwrap()
                            .contains("empty-queue-output-write-tests")
                    );
                    Ok("Advisory: the scope is testable.".to_owned())
                }
                _ => panic!("unexpected phase {phase}"),
            }
        },
    )
    .unwrap();
    assert_eq!(calls, ["decision-queue-plan", "decision-intent-review"]);
    assert_eq!(fixture.input().files, before.files);
    assert!(output.message.contains("the scope is testable"));
    let id = output.plan_id.as_deref().unwrap();
    let approval = output.approval.as_deref().unwrap();
    assert!(
        transaction::apply(
            fixture.project.path(),
            Path::new("."),
            id,
            approval,
            Some(1)
        )
        .is_err()
    );
    assert!(ensure_no_pending(&fixture.component()).is_err());
    apply(fixture.project.path(), Path::new("."), id, approval).unwrap();
    assert!(ensure_no_pending(&fixture.component()).is_ok());
    let after = fixture.input();
    assert!(after.files["REQUIREMENTS.md"].contains("### REQ-EMPTY-QUEUE"));
    assert_eq!(
        &after.queue.tasks[..before.queue.tasks.len()],
        before.queue.tasks
    );
}
