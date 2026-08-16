use kvist::task_queue::{TaskStatus, parse, serialize};

const VALID_QUEUE: &str = r#"schema_version: 2
component:
  specification_revision: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  parent_specification: null
  revalidation:
    state: current
    checked_at: 2026-08-13T20:19:53Z
    stale_since: null
    causes: []
tasks:
  - id: write-tests
    title: Write queue tests
    description: Define executable coverage for the queue contract.
    context: The queue format needs specification-derived tests before implementation.
    purpose: Prevent task execution from depending on an unvalidated YAML structure.
    expected_outcome: The schema behavior is covered by deterministic tests.
    kind: test
    status: completed
    depends_on: []
    requirements:
      - SPEC.md#TODO-queue-schema-and-validation
    timestamps:
      created_at: 2026-08-13T20:19:53Z
      updated_at: 2026-08-13T20:19:54Z
      completed_at: 2026-08-13T20:19:54Z
    blocked_reason: null
  - id: implement-code
    title: Implement queue validation
    description: Add the typed parser and validator.
    context: The parsed queue will become workflow control data.
    purpose: Share one trusted queue representation across tools.
    expected_outcome: Valid queues load and invalid queues report diagnostics.
    kind: implementation
    status: pending
    depends_on:
      - write-tests
    requirements:
      - SPEC.md#TODO-queue-schema-and-validation
    timestamps:
      created_at: 2026-08-13T20:19:53Z
      updated_at: 2026-08-13T20:19:53Z
      completed_at: null
    blocked_reason: null
"#;

#[test]
fn parses_and_serializes_a_canonical_queue_deterministically() {
    let queue = parse(VALID_QUEUE).expect("valid queue");
    let serialized = serialize(&queue).expect("serialize queue");
    let reparsed = parse(&serialized).expect("parse canonical queue");

    assert!(serialized.contains("tasks:\n  - id:"));
    assert_eq!(serialize(&reparsed).expect("serialize again"), serialized);
}

#[test]
fn rejects_duplicate_ids_unknown_dependencies_and_cycles() {
    let duplicate = VALID_QUEUE.replacen("id: implement-code", "id: write-tests", 1);
    assert!(
        parse(&duplicate)
            .expect_err("duplicate ID must fail")
            .to_string()
            .contains("duplicate task ID")
    );

    let unknown = VALID_QUEUE.replacen("write-tests", "missing-task", 1);
    assert!(
        parse(&unknown)
            .expect_err("unknown dependency must fail")
            .to_string()
            .contains("unknown task")
    );

    let cyclic = VALID_QUEUE.replacen("depends_on: []", "depends_on:\n      - implement-code", 1);
    assert!(
        parse(&cyclic)
            .expect_err("cycle must fail")
            .to_string()
            .contains("dependency cycle")
    );
}

#[test]
fn rejects_state_metadata_that_cannot_explain_workflow_progress() {
    let blocked_without_reason = VALID_QUEUE.replacen("status: pending", "status: blocked", 1);
    assert!(
        parse(&blocked_without_reason)
            .expect_err("blocked task needs a reason")
            .to_string()
            .contains("blocked_reason")
    );

    let completed_without_timestamp = VALID_QUEUE.replacen(
        "completed_at: 2026-08-13T20:19:54Z",
        "completed_at: null",
        1,
    );
    assert!(
        parse(&completed_without_timestamp)
            .expect_err("completed task needs timestamp")
            .to_string()
            .contains("completed_at")
    );
}

#[test]
fn rejects_malformed_yaml_unknown_fields_and_unexplained_staleness() {
    assert!(
        parse("schema_version: [")
            .expect_err("malformed YAML must fail")
            .to_string()
            .contains("YAML")
    );

    let unknown_field = VALID_QUEUE.replacen("tasks:", "unexpected: true\ntasks:", 1);
    assert!(
        parse(&unknown_field)
            .expect_err("unknown fields must fail")
            .to_string()
            .contains("unknown field")
    );

    let stale_without_cause = VALID_QUEUE
        .replacen("state: current", "state: stale", 1)
        .replacen("stale_since: null", "stale_since: 2026-08-13T20:19:53Z", 1);
    assert!(
        parse(&stale_without_cause)
            .expect_err("staleness must retain evidence")
            .to_string()
            .contains("at least one cause")
    );
}

#[test]
fn exposes_only_the_documented_task_state_transitions() {
    assert!(TaskStatus::Pending.can_transition_to(TaskStatus::InProgress));
    assert!(TaskStatus::Pending.can_transition_to(TaskStatus::Blocked));
    assert!(!TaskStatus::Pending.can_transition_to(TaskStatus::Completed));
    assert!(TaskStatus::InProgress.can_transition_to(TaskStatus::Completed));
    assert!(TaskStatus::Blocked.can_transition_to(TaskStatus::Pending));
    assert!(!TaskStatus::Completed.can_transition_to(TaskStatus::Pending));
}
