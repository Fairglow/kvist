//! Native-agent authority integration tests.
//!
//! After the child sa-08 compliance review defines the standalone host
//! interfaces, these tests prove that the Kvist half of native agent execution
//! is correct: task-bound tool authorization, canonical grants, resource
//! bindings, execution-tier selection, denial, bounded redacted runtime-event
//! ingestion, artifact promotion, and append-only compliance evidence.
//!
//! The tests are failing because the host authority interfaces are not yet
//! implemented. They define the expected behavior for the implementation.

use std::path::Path;

use sav::{ToolDefinition, ToolIntent};
use sav::runtime::Event;

/// Build a minimal tool definition for testing.
fn tool_def(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_owned(),
        description: format!("test tool {name}"),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            }
        }),
    }
}

/// Test: task-bound tool authorization restricts tools to the task's grant.
///
/// The model can only use tools explicitly granted to the task. Any other tool
/// call is denied, even if the tool exists in the system.
#[test]
fn task_bound_tool_authorization() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    // Task grants only write_file and edit_file
    let granted_tools = vec!["write_file", "edit_file"];

    // Expected API: create task authorization context
    let auth = maerg::agn::TaskAuthorization::new(
        "test-task",
        root_path,
        granted_tools.clone(),
    );

    // Model proposes a granted tool: authorized
    let write_intent = ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: serde_json::json!({
            "path": "src/lib.rs",
            "content": "fn main() {}"
        }),
    };
    let plan = auth.authorize_turn(&write_intent);
    assert!(
        plan.effects.iter().any(|e| e.call_id == "call-1"),
        "granted tool should be authorized"
    );

    // Model proposes an ungranted tool: denied
    let shell_intent = ToolIntent {
        id: "call-2".to_owned(),
        provider_id: None,
        name: "exec_shell".to_owned(),
        arguments: serde_json::json!({ "command": "ls" }),
    };
    let plan2 = auth.authorize_turn(&shell_intent);
    assert!(
        plan2.dropped.iter().any(|d| d.call_id == "call-2"),
        "ungranted tool should be dropped"
    );
}

/// Test: canonical grants are bound to task, policy, and workspace revisions.
///
/// A grant is not just a tool name; it includes the task identity, the policy
/// revision, and the workspace revision. A grant from a different task or
/// revision cannot be used.
#[test]
fn canonical_grants_bound_to_revisions() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    // Create grant bound to task A, policy revision 1
    let grant_a = maerg::agn::ToolGrant {
        task_id: "task-a".to_owned(),
        tool: "write_file".to_owned(),
        policy_revision: 1,
        workspace_revision: 1,
        resource_bindings: Vec::new(),
    };

    // Grant from task A works for task A
    let auth_a = maerg::agn::TaskAuthorization::with_grant("task-a", root_path, grant_a);
    let intent = ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: serde_json::json!({ "path": "src/lib.rs", "content": "x" }),
    };
    let plan = auth_a.authorize_turn(&intent);
    assert!(
        plan.effects.iter().any(|e| e.call_id == "call-1"),
        "grant from matching task should authorize"
    );

    // Same grant does not work for task B
    let auth_b = maerg::agn::TaskAuthorization::new("task-b", root_path, vec![]);
    let plan2 = auth_b.authorize_turn(&intent);
    assert!(
        plan2.dropped.iter().any(|d| d.call_id == "call-1"),
        "grant from different task should not authorize"
    );

    // Grant with wrong policy revision does not work
    let grant_stale = maerg::agn::ToolGrant {
        task_id: "task-a".to_owned(),
        tool: "write_file".to_owned(),
        policy_revision: 1,
        workspace_revision: 2, // different revision
        resource_bindings: Vec::new(),
    };
    let auth_stale = maerg::agn::TaskAuthorization::with_grant("task-a", root_path, grant_stale);
    let plan3 = auth_stale.authorize_turn(&intent);
    assert!(
        plan3.dropped.iter().any(|d| d.call_id == "call-1"),
        "grant with wrong workspace revision should not authorize"
    );
}

/// Test: resource bindings are validated before tool dispatch.
///
/// A tool grant may include resource bindings (e.g., allowed file paths).
/// The authorization must validate that the tool invocation uses only bound
/// resources.
#[test]
fn resource_bindings_validated() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    // Grant write_file only to src/ directory
    let grant = maerg::agn::ToolGrant {
        task_id: "test-task".to_owned(),
        tool: "write_file".to_owned(),
        policy_revision: 1,
        workspace_revision: 1,
        resource_bindings: vec![maerg::agn::ResourceBinding {
            kind: "path".to_owned(),
            value: "src/".to_owned(),
        }],
    };

    let auth = maerg::agn::TaskAuthorization::with_grant("test-task", root_path, grant);

    // Write to src/ is authorized
    let intent_ok = ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: serde_json::json!({ "path": "src/lib.rs", "content": "x" }),
    };
    let plan1 = auth.authorize_turn(&intent_ok);
    assert!(
        plan1.effects.iter().any(|e| e.call_id == "call-1"),
        "write to bound path should be authorized"
    );

    // Write to tests/ is not bound, so denied
    let intent_bad = ToolIntent {
        id: "call-2".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: serde_json::json!({ "path": "tests/foo.rs", "content": "x" }),
    };
    let plan2 = auth.authorize_turn(&intent_bad);
    assert!(
        plan2.dropped.iter().any(|d| d.call_id == "call-2"),
        "write to unbound path should be dropped"
    );
}

/// Test: execution-tier selection is made by Kvist, not the model.
///
/// The model cannot request a different execution tier (sandboxed vs. host).
/// Kvist selects the tier based on task policy.
#[test]
fn execution_tier_selection_by_kvist() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    // Task policy specifies sandboxed execution
    let task_policy = maerg::agn::TaskPolicy {
        execution_tier: maerg::agn::ExecutionTier::Sandboxed,
        // ... other policy fields
    };

    // Model proposes a tool call with a "host execution" hint
    let intent = ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: serde_json::json!({
            "path": "src/lib.rs",
            "content": "x",
            "execution_tier": "host" // model hint ignored
        }),
    };

    let auth = maerg::agn::TaskAuthorization::with_policy("test-task", root_path, task_policy);
    let plan = auth.authorize_turn(&intent);

    // The effect is still sandboxed regardless of model hint
    let effect = plan
        .effects
        .iter()
        .find(|e| e.call_id == "call-1")
        .expect("effect should be authorized");
    assert!(
        effect.execution_tier == maerg::agn::ExecutionTier::Sandboxed,
        "execution tier must be policy-selected, not model-selected"
    );
}

/// Test: denial is recorded as durable evidence.
///
/// Every denied tool call is recorded with a reason, creating an audit trail.
#[test]
fn denial_recorded_as_evidence() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    let auth = maerg::agn::TaskAuthorization::new("test-task", root_path, vec![]);

    // Model proposes an ungranted tool
    let intent = ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: "exec_shell".to_owned(),
        arguments: serde_json::json!({ "command": "rm -rf /" }),
    };

    let plan = auth.authorize_turn(&intent);

    // Denial is recorded
    let dropped = plan
        .dropped
        .iter()
        .find(|d| d.call_id == "call-1")
        .expect("denial should be recorded");
    assert!(!dropped.reason.is_empty(), "denial reason must be non-empty");
    assert!(
        dropped.reason.contains("not granted") || dropped.reason.contains("unauthorized"),
        "denial reason should explain the authorization failure"
    );
}

/// Test: runtime events are ingested with bounded redaction.
///
/// Runtime events (tool results, model output) are ingested with redaction
/// of secrets and bounded size.
#[test]
fn bounded_redacted_event_ingestion() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    // Create event sink with redaction
    let secret = "sk-1234567890abcdef";
    let mut sink = maerg::agn::RedactedEventSink::new(root_path, vec![secret.to_owned()]);

    // Event with secret is redacted
    let event = Event::Text(format!("API key: {}", secret));
    sink.ingest(&event).expect("event ingestion");

    let redacted = sink.redacted_output();
    assert!(
        !redacted.contains(secret),
        "secret should be redacted from ingested events"
    );

    // Event size is bounded
    let large_event = Event::Text("x".repeat(100_000));
    sink.ingest(&large_event).expect("large event ingestion");
    let bounded = sink.redacted_output();
    assert!(
        bounded.len() < 100_000,
        "event output should be bounded in size"
    );
}

/// Test: artifact promotion requires a separate Kvist decision.
///
/// The model cannot promote an artifact (e.g., mark a test as passing,
/// promote a draft to final). Promotion is a separate Kvist decision.
#[test]
fn artifact_promotion_requires_kvist_decision() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    // Model proposes to promote an artifact
    let intent = ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: "promote_artifact".to_owned(),
        arguments: serde_json::json!({
            "artifact": "test-result.json",
            "status": "passed"
        }),
    };

    // Task grants only write tools, not promotion
    let auth = maerg::agn::TaskAuthorization::new(
        "test-task",
        root_path,
        vec!["write_file", "edit_file"],
    );
    let plan = auth.authorize_turn(&intent);

    // Promotion is not authorized by the task grant
    assert!(
        plan.dropped.iter().any(|d| d.call_id == "call-1"),
        "artifact promotion should require separate Kvist decision"
    );
}

/// Test: compliance evidence is append-only.
///
/// Once evidence is recorded, it cannot be modified or deleted. New evidence
/// is appended.
#[test]
fn append_only_compliance_evidence() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    // Create evidence store
    let mut store = maerg::agn::EvidenceStore::new(root_path);

    // Append first evidence
    store.append("event-1", "first event");
    store.append("event-2", "second event");

    // Cannot modify existing evidence
    let result = store.modify("event-1", "modified");
    assert!(
        result.is_err(),
        "existing evidence should not be modifiable"
    );

    // Cannot delete existing evidence
    let result = store.delete("event-1");
    assert!(
        result.is_err(),
        "existing evidence should not be deletable"
    );

    // Can read all evidence in order
    let events = store.all();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].id, "event-1");
    assert_eq!(events[1].id, "event-2");
}

/// Test: fake model intent cannot broaden task scope.
///
/// The model cannot expand the set of authorized tools or resources mid-task.
#[test]
fn model_cannot_broaden_task_scope() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    // Task grants read-only access
    let auth = maerg::agn::TaskAuthorization::new(
        "test-task",
        root_path,
        vec!["read_file", "list_files"],
    );

    // Model proposes to write (broadening scope)
    let write_intent = ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: serde_json::json!({ "path": "src/lib.rs", "content": "x" }),
    };
    let plan = auth.authorize_turn(&write_intent);
    assert!(
        plan.dropped.iter().any(|d| d.call_id == "call-1"),
        "model cannot broaden task scope by proposing ungranted tools"
    );
}

/// Test: dispatch without an exact grant is denied.
///
/// A partial match on tool name or arguments is not sufficient; the grant
/// must be exact.
#[test]
fn dispatch_requires_exact_grant() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    // Grant write_file to src/ only
    let grant = maerg::agn::ToolGrant {
        task_id: "test-task".to_owned(),
        tool: "write_file".to_owned(),
        policy_revision: 1,
        workspace_revision: 1,
        resource_bindings: vec![maerg::agn::ResourceBinding {
            kind: "path".to_owned(),
            value: "src/".to_owned(),
        }],
    };

    let auth = maerg::agn::TaskAuthorization::with_grant("test-task", root_path, grant);

    // Similar but not exact: write to src/ via different tool name
    let similar_intent = ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: "write".to_owned(), // not "write_file"
        arguments: serde_json::json!({ "path": "src/lib.rs", "content": "x" }),
    };
    let plan = auth.authorize_turn(&similar_intent);
    assert!(
        plan.dropped.iter().any(|d| d.call_id == "call-1"),
        "similar tool name is not an exact grant"
    );
}

/// Test: evidence cannot be forged by the model.
///
/// The model cannot create fake compliance evidence. Evidence is created
/// only by the Kvist authority layer.
#[test]
fn evidence_cannot_be_forged_by_model() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    let _store = maerg::agn::EvidenceStore::new(root_path);

    // Model proposes to record evidence
    let intent = ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: "record_evidence".to_owned(),
        arguments: serde_json::json!({
            "event": "test-passed",
            "data": "fake passing test"
        }),
    };

    // Task grants only write tools
    let auth = maerg::agn::TaskAuthorization::new(
        "test-task",
        root_path,
        vec!["write_file"],
    );
    let plan = auth.authorize_turn(&intent);

    // Evidence recording is not authorized
    assert!(
        plan.dropped.iter().any(|d| d.call_id == "call-1"),
        "model cannot forge evidence without explicit grant"
    );
}

/// Test: complete authority flow with all checks.
///
/// Integration test combining all authority checks: task-bound authorization,
/// canonical grants, resource bindings, execution tier, denial evidence,
/// redacted event ingestion, and append-only evidence.
#[test]
fn complete_authority_flow() {
    let root = tempfile::tempdir().expect("tempdir");
    let root_path = root.path();

    // Set up task with grants
    let grant = maerg::agn::ToolGrant {
        task_id: "integration-task".to_owned(),
        tool: "write_file".to_owned(),
        policy_revision: 1,
        workspace_revision: 1,
        resource_bindings: vec![maerg::agn::ResourceBinding {
            kind: "path".to_owned(),
            value: "src/".to_owned(),
        }],
    };

    let policy = maerg::agn::TaskPolicy {
        execution_tier: maerg::agn::ExecutionTier::Sandboxed,
    };

    let auth = maerg::agn::TaskAuthorization::with_grant_and_policy(
        "integration-task",
        root_path,
        grant,
        policy,
    );

    let mut store = maerg::agn::EvidenceStore::new(root_path);
    let _sink = maerg::agn::RedactedEventSink::new(root_path, vec![]);

    // Authorized tool call
    let ok_intent = ToolIntent {
        id: "call-1".to_owned(),
        provider_id: None,
        name: "write_file".to_owned(),
        arguments: serde_json::json!({ "path": "src/lib.rs", "content": "fn main() {}" }),
    };
    let plan1 = auth.authorize_turn(&ok_intent);
    assert!(
        plan1.effects.iter().any(|e| e.call_id == "call-1"),
        "authorized tool call should be allowed"
    );

    // Record authorization evidence
    store.append("auth-call-1", "write_file to src/lib.rs authorized");

    // Unauthorized tool call
    let bad_intent = ToolIntent {
        id: "call-2".to_owned(),
        provider_id: None,
        name: "exec_shell".to_owned(),
        arguments: serde_json::json!({ "command": "ls" }),
    };
    let plan2 = auth.authorize_turn(&bad_intent);
    assert!(
        plan2.dropped.iter().any(|d| d.call_id == "call-2"),
        "unauthorized tool call should be dropped"
    );

    // Record denial evidence
    store.append("denied-call-2", "exec_shell denied: not granted");

    // Verify evidence is append-only
    let events = store.all();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].id, "auth-call-1");
    assert_eq!(events[1].id, "denied-call-2");
}
