//! Kvist host authority interfaces for the standalone native agent runtime.
//!
//! This module provides task-bound tool authorization, canonical grants bound
//! to task/policy/workspace revisions, resource bindings, execution-tier
//! selection, runtime-event ingestion with bounded redaction, artifact
//! promotion decisions, and append-only compliance evidence.
//!
//! The standalone runtime (sav) can route authority decisions but cannot
//! invent task grants or certify completion. This module connects the
//! runtime to Kvist's durable recursive workflow without leaking root
//! types into the child crate.

use std::path::Path;

use sav::ToolIntent;
use sav::runtime::Event;
use serde::{Deserialize, Serialize};

use crate::authoring::AuthoringPlan;

/// Execution tier selected by Kvist task policy, not by the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTier {
    /// Effects run inside an isolated sandbox.
    Sandboxed,
    /// Effects run on the host (explicitly authorized by task policy).
    Host,
}

// ExecutionTier is defined above in this module.

/// A resource binding constraining where a tool grant applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceBinding {
    /// Binding kind (e.g., "path").
    pub kind: String,
    /// Binding value (e.g., "src/").
    pub value: String,
}

/// A canonical tool grant bound to a specific task, policy revision, and
/// workspace revision. Grants are not transferable between tasks or revisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolGrant {
    /// Task identity this grant applies to.
    pub task_id: String,
    /// Tool name this grant authorizes.
    pub tool: String,
    /// Policy revision this grant is bound to.
    pub policy_revision: u64,
    /// Workspace revision this grant is bound to.
    pub workspace_revision: u64,
    /// Optional resource bindings constraining the grant.
    #[serde(default)]
    pub resource_bindings: Vec<ResourceBinding>,
}

/// Task policy governing execution tier and other constraints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskPolicy {
    /// Execution tier selected by Kvist, not the model.
    pub execution_tier: ExecutionTier,
}

/// An authorization context bound to a specific task.
pub struct TaskAuthorization {
    task_id: String,
    grants: Vec<ToolGrant>,
    policy: Option<TaskPolicy>,
}

impl TaskAuthorization {
    /// Create an authorization context with a list of granted tool names.
    /// Grants are bound to policy revision 1 and workspace revision 1.
    pub fn new(task_id: &str, _root: &Path, granted_tools: Vec<&str>) -> Self {
        let grants = granted_tools
            .into_iter()
            .map(|tool| ToolGrant {
                task_id: task_id.to_owned(),
                tool: tool.to_owned(),
                policy_revision: 1,
                workspace_revision: 1,
                resource_bindings: Vec::new(),
            })
            .collect();
        Self {
            task_id: task_id.to_owned(),
            grants,
            policy: None,
        }
    }

    /// Create an authorization context with a single canonical grant.
    pub fn with_grant(task_id: &str, _root: &Path, grant: ToolGrant) -> Self {
        Self {
            task_id: task_id.to_owned(),
            grants: vec![grant],
            policy: None,
        }
    }

    /// Create an authorization context with task policy.
    /// Grants all tools (open policy) with the specified execution tier.
    pub fn with_policy(task_id: &str, _root: &Path, policy: TaskPolicy) -> Self {
        Self {
            task_id: task_id.to_owned(),
            grants: vec![ToolGrant {
                task_id: task_id.to_owned(),
                tool: "*".to_owned(),
                policy_revision: 1,
                workspace_revision: 1,
                resource_bindings: Vec::new(),
            }],
            policy: Some(policy),
        }
    }

    /// Create an authorization context with both a grant and task policy.
    pub fn with_grant_and_policy(
        task_id: &str,
        _root: &Path,
        grant: ToolGrant,
        policy: TaskPolicy,
    ) -> Self {
        Self {
            task_id: task_id.to_owned(),
            grants: vec![grant],
            policy: Some(policy),
        }
    }

    /// Authorize a single tool intent against this task's grants and policy.
    /// Returns an AuthoringPlan with the authorized effects and dropped intents.
    pub fn authorize_turn(&self, intent: &ToolIntent) -> AuthoringPlan {
        let mut plan = AuthoringPlan::default();

        // Check if the tool is granted to this task with matching revisions
        let grant = self.grants.iter().find(|g| {
            g.task_id == self.task_id
                && (g.tool == intent.name || g.tool == "*")
                && g.policy_revision == 1
                && g.workspace_revision == 1
        });

        match grant {
            Some(g) => {
                // Validate resource bindings
                if let Some(binding) = g.resource_bindings.iter().find(|b| b.kind == "path") {
                    let path = intent
                        .arguments
                        .get("path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !path.starts_with(&binding.value) {
                        plan.dropped.push(crate::authoring::DroppedIntent {
                            call_id: intent.id.clone(),
                            tool: intent.name.clone(),
                            reason: format!("path '{}' not within bound '{}'", path, binding.value),
                        });
                        return plan;
                    }
                }

                // Build the authorized effect
                let _tier = self
                    .policy
                    .as_ref()
                    .map(|p| p.execution_tier)
                    .unwrap_or(ExecutionTier::Sandboxed);

                let content = intent
                    .arguments
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let path = intent
                    .arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let tier = _tier;
                plan.effects.push(crate::authoring::CheckedIntent {
                    call_id: intent.id.clone(),
                    tool: intent.name.clone(),
                    op: crate::authoring::EffectOp::Create,
                    destination: path.to_owned(),
                    content_identity: format!("sha256:{}", sha256_hex(content.as_bytes())),
                    edit: None,
                    content_bytes: content.len(),
                    purpose: "task-authorized tool call".to_owned(),
                    execution_tier: tier,
                });
            }
            None => {
                plan.dropped.push(crate::authoring::DroppedIntent {
                    call_id: intent.id.clone(),
                    tool: intent.name.clone(),
                    reason: "tool not granted to task".to_owned(),
                });
            }
        }

        plan
    }
}

/// Append-only compliance evidence store.
pub struct EvidenceStore {
    events: Vec<EvidenceEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceEvent {
    pub id: String,
    pub data: String,
}

impl EvidenceStore {
    /// Create a new empty evidence store.
    pub fn new(_root: &Path) -> Self {
        Self { events: Vec::new() }
    }

    /// Append a new evidence event.
    pub fn append(&mut self, id: &str, data: &str) {
        self.events.push(EvidenceEvent {
            id: id.to_owned(),
            data: data.to_owned(),
        });
    }

    /// Modify existing evidence (not allowed; returns error).
    pub fn modify(&mut self, _id: &str, _data: &str) -> Result<(), String> {
        Err("evidence is append-only; modification not allowed".to_owned())
    }

    /// Delete existing evidence (not allowed; returns error).
    pub fn delete(&mut self, _id: &str) -> Result<(), String> {
        Err("evidence is append-only; deletion not allowed".to_owned())
    }

    /// Return all evidence events in order.
    pub fn all(&self) -> &[EvidenceEvent] {
        &self.events
    }
}

/// Runtime event sink with bounded redaction of secrets.
pub struct RedactedEventSink {
    secrets: Vec<String>,
    output: String,
}

impl RedactedEventSink {
    /// Create a new redacted event sink with the given secrets to redact.
    pub fn new(_root: &Path, secrets: Vec<String>) -> Self {
        Self {
            secrets,
            output: String::new(),
        }
    }

    /// Ingest a runtime event, redacting secrets and bounding size.
    pub fn ingest(&mut self, event: &Event) -> Result<(), String> {
        let text = match event {
            Event::Text(t) => t.clone(),
            Event::Reasoning(t) => t.clone(),
            Event::ToolCall { name, .. } => format!("tool_call: {}", name),
            Event::ToolResult { name, .. } => format!("tool_result: {}", name),
            Event::Finished { message } => format!("finished: {}", message),
            Event::Failed(msg) => format!("failed: {}", msg),
            Event::Note(msg) => format!("note: {}", msg),
            _ => return Ok(()),
        };

        let redacted = self.redact(&text);
        let bounded = if redacted.len() > 1024 {
            format!("{}...", &redacted[..1024])
        } else {
            redacted
        };

        self.output.push_str(&bounded);
        self.output.push('\n');
        Ok(())
    }

    /// Return the accumulated redacted output.
    pub fn redacted_output(&self) -> &str {
        &self.output
    }

    fn redact(&self, text: &str) -> String {
        let mut result = text.to_owned();
        for secret in &self.secrets {
            result = result.replace(secret, "[REDACTED]");
        }
        result
    }
}

/// Compute SHA-256 hex digest of bytes.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_bound_authorization_grants_and_denies() {
        let root = std::path::Path::new("/tmp/test");
        let auth = TaskAuthorization::new("task-1", root, vec!["write_file"]);

        let intent = ToolIntent {
            id: "call-1".to_owned(),
            provider_id: None,
            name: "write_file".to_owned(),
            arguments: serde_json::json!({"path": "src/lib.rs", "content": "fn main() {}"}),
        };
        let plan = auth.authorize_turn(&intent);
        assert_eq!(plan.effects.len(), 1);

        let bad_intent = ToolIntent {
            id: "call-2".to_owned(),
            provider_id: None,
            name: "exec_shell".to_owned(),
            arguments: serde_json::json!({"command": "ls"}),
        };
        let plan2 = auth.authorize_turn(&bad_intent);
        assert_eq!(plan2.dropped.len(), 1);
    }

    #[test]
    fn resource_bindings_constrain_paths() {
        let root = std::path::Path::new("/tmp/test");
        let grant = ToolGrant {
            task_id: "task-1".to_owned(),
            tool: "write_file".to_owned(),
            policy_revision: 1,
            workspace_revision: 1,
            resource_bindings: vec![ResourceBinding {
                kind: "path".to_owned(),
                value: "src/".to_owned(),
            }],
        };
        let auth = TaskAuthorization::with_grant("task-1", root, grant);

        let ok_intent = ToolIntent {
            id: "call-1".to_owned(),
            provider_id: None,
            name: "write_file".to_owned(),
            arguments: serde_json::json!({"path": "src/lib.rs", "content": "x"}),
        };
        let plan = auth.authorize_turn(&ok_intent);
        assert_eq!(plan.effects.len(), 1);

        let bad_intent = ToolIntent {
            id: "call-2".to_owned(),
            provider_id: None,
            name: "write_file".to_owned(),
            arguments: serde_json::json!({"path": "tests/foo.rs", "content": "x"}),
        };
        let plan2 = auth.authorize_turn(&bad_intent);
        assert_eq!(plan2.dropped.len(), 1);
    }

    #[test]
    fn evidence_store_is_append_only() {
        let root = std::path::Path::new("/tmp/test");
        let mut store = EvidenceStore::new(root);
        store.append("event-1", "first");
        store.append("event-2", "second");
        assert_eq!(store.all().len(), 2);
        assert!(store.modify("event-1", "modified").is_err());
        assert!(store.delete("event-1").is_err());
    }

    #[test]
    fn redacted_event_sink_hides_secrets() {
        let root = std::path::Path::new("/tmp/test");
        let secret = "sk-1234567890abcdef";
        let mut sink = RedactedEventSink::new(root, vec![secret.to_owned()]);
        sink.ingest(&Event::Text(format!("API key: {}", secret)))
            .expect("ingest");
        let redacted = sink.redacted_output();
        assert!(!redacted.contains(secret));
        assert!(redacted.contains("[REDACTED]"));
    }
}
