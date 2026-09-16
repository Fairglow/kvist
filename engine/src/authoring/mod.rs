//! Authoring tool-intent broker (ADR-0007 step 3).
//!
//! The model turn runs on the host and returns untrusted `ToolIntent` values. This
//! module is the broker: it authorizes each intent against a fixed, closed allowlist
//! with a deny-by-default, total policy, emitting only capability-bound
//! `CheckedIntent`s while recording every dropped intent as durable evidence.
//!
//! The security guarantee lives *here*, in this total policy function, not in the
//! sandbox. The sandbox (bubblewrap) is best-effort defense-in-depth: it is never
//! the thing we certify. The broker is the source of truth for which authoring
//! actions exist. The fixed [`ALLOWED_TOOLS`] / [`EffectOp`] vocabulary below is the
//! set a model may propose; anything outside it is dropped, so a change to the set
//! of allowed agent actions is a change to *this* code and its tests, never to an
//! untrusted agent. There is no external spec to sync with, and agents cannot
//! extend the set themselves.
//!
//! ## Detecting a change to the allowed action set
//!
//! Agents propose arbitrary tool names; the broker decides which are authorized.
//! The canonical, shared definition of "agent actions" is therefore the enumeration
//! in this module, not per-agent handling. When the allowed set must grow or shrink,
//! the change is made here (and in the tests), reviewed as an authority change, and
//! the [`crate::sandbox`] grants are kept consistent. Detection is simply: review and
//! test this module. [`classify_intent`](classify_intent) is the single funnel every
//! proposed action passes through.

use std::path::{Component, Path, PathBuf};

use agent_runtime::{ModelTurn, ToolIntent};
use hex::encode as hex_encode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The fixed, closed vocabulary of authoring tools the broker will ever authorize.
///
/// This is the authoritative enumeration of allowed agent authoring actions. It is
/// *not* supplied by agents: every model proposes arbitrary tool calls, and only
/// these names are ever authorized.
pub const ALLOWED_TOOLS: &[&str] = &["write_file", "edit_file"];

/// Documents that must never be writable inside an authoring ancestor. These are
/// mounted read-only context; the broker refuses to authorize effects targeting them,
/// mirroring [`crate::sandbox`] intent-document handling.
pub const PROTECTED_DOCUMENTS: &[&str] = &[
    "REQUIREMENTS.md",
    "CONTRACT.md",
    "DESIGN.md",
    "TODOS.yaml",
    "IMPL.md",
];

/// Writable roots an authoring agent may modify, relative to the component directory.
/// Only these are ever granted read-write; everything else on the host stays
/// inaccessible as a guarantee of the broker, independent of the sandbox.
pub const WRITABLE_ROOTS: &[&str] = &["src", "tests"];

/// Maximum nesting depth (path components below the component root) of an authored
/// destination. Bounds how deep an agent may write.
pub const MAX_DESTINATION_DEPTH: usize = 12;

/// Maximum bytes of authored content permitted per effect.
pub const MAX_CONTENT_BYTES: usize = 1 << 20;

/// Maximum total authored-content bytes across a single plan.
pub const MAX_PLAN_CONTENT_BYTES: usize = 8 << 20;

/// The class of an authorized authoring effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectOp {
    /// Create a new file, or overwrite an existing one with new content.
    Create,
    /// Modify an already-existing file.
    Modify,
}

/// A broker-authorized effect. This is the only thing the broker ever permits to be
/// applied; everything the model proposes is reduced to [`CheckedIntent`]s or dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckedIntent {
    /// Turn-local call identity, echoed from the model intent.
    pub call_id: String,
    /// Authorized tool name (one of [`ALLOWED_TOOLS`]).
    pub tool: String,
    /// Whether the effect creates or modifies.
    pub op: EffectOp,
    /// Destination relative to the component directory. Never absolute, never
    /// contains `..`, never names a protected document, always under a writable root.
    pub destination: String,
    /// `sha256:hex` identity of the authored bytes when supplied, else `None` for a
    /// targeted modification whose result identity is rehashed by the writer.
    pub content_identity: Option<String>,
    /// Bytes of authored content for this effect, for plan-bound accounting.
    pub content_bytes: usize,
    /// Human-readable purpose bound to the grant.
    pub purpose: String,
}

/// A model intent the broker refused. Recorded, never executed: it becomes no sandbox
/// effect and no filesystem change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroppedIntent {
    /// Turn-local call identity of the refused intent.
    pub call_id: String,
    /// Model-selected tool name.
    pub tool: String,
    /// Non-secret, human-readable reason the intent was not authorized.
    pub reason: String,
}

/// The brokered result of authorizing every intent from one model turn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoringPlan {
    /// Authorized, capability-bound effects, in proposal order.
    pub effects: Vec<CheckedIntent>,
    /// Refused intents, in proposal order, for the audit trail.
    pub dropped: Vec<DroppedIntent>,
}

impl AuthoringPlan {
    /// True when no effect was authorized.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// Number of refused intents.
    pub fn dropped_count(&self) -> usize {
        self.dropped.len()
    }

    /// Total authored content bytes across all authorized effects.
    pub fn content_bytes(&self) -> usize {
        self.effects.iter().map(|effect| effect.content_bytes).sum()
    }

    /// Enforce the plan-wide content bound. Fails closed: when exceeded, every effect
    /// is dropped rather than applying a partial, over-budget plan.
    pub fn enforce_bound(&mut self, policy: &BrokerPolicy) {
        if self.content_bytes() > policy.max_plan_content_bytes {
            for effect in self.effects.drain(..) {
                self.dropped.push(DroppedIntent {
                    call_id: effect.call_id.clone(),
                    tool: effect.tool.clone(),
                    reason: format!(
                        "authorized plan content exceeds the {}-byte plan bound",
                        policy.max_plan_content_bytes
                    ),
                });
            }
        }
    }
}

/// The broker authorization policy. Constructed once from project constraints and
/// reused across turns; it carries only bounds and the writable-root set.
#[derive(Debug, Clone)]
pub struct BrokerPolicy {
    /// Writable roots relative to the component directory.
    pub writable_roots: Vec<String>,
    /// Per-effect content byte bound.
    pub max_content_bytes: usize,
    /// Per-effect destination depth bound.
    pub max_destination_depth: usize,
    /// Plan-wide authored-content byte bound.
    pub max_plan_content_bytes: usize,
}

impl Default for BrokerPolicy {
    fn default() -> Self {
        Self {
            writable_roots: WRITABLE_ROOTS.iter().map(|&root| root.to_owned()).collect(),
            max_content_bytes: MAX_CONTENT_BYTES,
            max_destination_depth: MAX_DESTINATION_DEPTH,
            max_plan_content_bytes: MAX_PLAN_CONTENT_BYTES,
        }
    }
}

/// Authorize every untrusted intent returned by one model turn.
///
/// `component_root` anchors the writable roots; destinations are normalized relative
/// to it. Total and infallible: no intent can panic, and a malformed, suspect, or
/// disallowed intent is recorded in `dropped` rather than propagated, so a hostile or
/// buggy model can neither wedge the broker nor escape through an error.
pub fn authorize_turn(
    component_root: &Path,
    turn: &ModelTurn,
    policy: &BrokerPolicy,
) -> AuthoringPlan {
    authorize_intents(component_root, &turn.tool_intents, policy)
}

/// Authorize a bare slice of untrusted intents (used by the host turn and tests).
pub fn authorize_intents(
    component_root: &Path,
    intents: &[ToolIntent],
    policy: &BrokerPolicy,
) -> AuthoringPlan {
    let mut plan = AuthoringPlan::default();
    for intent in intents {
        match classify_intent(component_root, intent, policy) {
            Ok(checked) => plan.effects.push(checked),
            Err(reason) => plan.dropped.push(DroppedIntent {
                call_id: intent.id.clone(),
                tool: intent.name.clone(),
                reason,
            }),
        }
    }
    plan
}

/// Reduce a single untrusted intent to a checked effect, or a drop reason.
///
/// Deny-by-default: the tool name must be in [`ALLOWED_TOOLS`]; arguments must be a
/// JSON object; the destination must normalize under a writable root without
/// escaping; content must be within bound. Any failure is a drop reason.
fn classify_intent(
    component_root: &Path,
    intent: &ToolIntent,
    policy: &BrokerPolicy,
) -> Result<CheckedIntent, String> {
    if intent.name.trim().is_empty() {
        return Err("tool name is empty".to_owned());
    }
    if !ALLOWED_TOOLS.contains(&intent.name.as_str()) {
        return Err(format!(
            "tool `{}` is not in the authorized authoring set {:?}",
            intent.name, ALLOWED_TOOLS
        ));
    }
    if !intent.arguments.is_object() {
        return Err(format!(
            "tool `{}` arguments must be a JSON object",
            intent.name
        ));
    }
    let obj = intent
        .arguments
        .as_object()
        .expect("classify_intent already confirmed arguments is an object");
    match intent.name.as_str() {
        "write_file" => authorize_write(component_root, intent, obj, policy),
        "edit_file" => authorize_edit(component_root, intent, obj, policy),
        _ => unreachable!("classify_intent only dispatches to matched ALLOWED_TOOLS entries"),
    }
}

/// Extract a required, non-empty string argument. Missing, empty, or wrong-type
/// arguments are drop reasons, not panics.
fn require_string_arg(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<String, String> {
    match obj.get(key) {
        Some(serde_json::Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        Some(serde_json::Value::String(_)) => {
            Err(format!("argument `{key}` must be a non-empty string"))
        }
        Some(_) => Err(format!("argument `{key}` must be a string")),
        None => Err(format!("missing required argument `{key}`")),
    }
}

/// Normalize a relative destination to a component-root-anchored path, rejecting
/// absolute paths, `..`, protected documents, non-writable roots, over-deep paths,
/// and any escape from the component root.
fn normalize_destination(
    component_root: &Path,
    destination: &str,
    policy: &BrokerPolicy,
) -> Result<PathBuf, String> {
    if destination.is_empty() {
        return Err("destination is empty".to_owned());
    }
    if Path::new(destination).is_absolute() {
        return Err(format!(
            "destination `{destination}` must be relative to the component"
        ));
    }

    // Rebuild the destination under the component root, rejecting any component
    // that is not a normal path segment. This both validates and canonicalizes the
    // path in a single pass, so a `..` or `.` segment is refused before it can be
    // joined.
    let mut resolved = PathBuf::from(component_root);
    let mut depth = 0usize;
    for component in Path::new(destination).components() {
        match component {
            Component::Normal(part) => {
                resolved.push(part);
                depth += 1;
            }
            Component::CurDir => {
                return Err(format!(
                    "destination `{destination}` must not reference `.`"
                ));
            }
            Component::ParentDir => {
                return Err(format!("destination `{destination}` must not contain `..`"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "destination `{destination}` is not a relative component path"
                ));
            }
        }
    }
    if depth == 0 {
        return Err("destination resolves to the component root".to_owned());
    }

    let filename = resolved
        .file_name()
        .and_then(|name| name.to_str())
        .map(|value| value.to_owned())
        .unwrap_or_default();
    if PROTECTED_DOCUMENTS.contains(&filename.as_str()) {
        return Err(format!(
            "destination `{destination}` names a protected intent document; only reads are allowed"
        ));
    }

    if depth > policy.max_destination_depth {
        return Err(format!(
            "destination `{destination}` (depth {}) exceeds the {}-depth bound",
            depth, policy.max_destination_depth
        ));
    }

    if !resolved.starts_with(component_root) {
        return Err(format!(
            "destination `{destination}` escapes the component root"
        ));
    }

    let under_writable = policy
        .writable_roots
        .iter()
        .any(|root| resolved.starts_with(component_root.join(root)));
    if !under_writable {
        return Err(format!(
            "destination `{destination}` is not under a writable root {:?}",
            policy.writable_roots
        ));
    }

    Ok(resolved)
}

/// Render a resolved component-anchored path back to its component-relative form.
fn normalized_relative(component_root: &Path, resolved: &Path) -> String {
    resolved
        .strip_prefix(component_root)
        .map(|rel| rel.display().to_string())
        .unwrap_or_else(|_| resolved.display().to_string())
}

/// Compute the `sha256:hex` content identity of authored bytes, honoring the
/// per-effect content bound. Returns `(identity, byte_len)` or a drop reason.
fn content_identity(content: &str, policy: &BrokerPolicy) -> Result<(String, usize), String> {
    let bytes = content.as_bytes();
    if bytes.len() > policy.max_content_bytes {
        return Err(format!(
            "content exceeds the {}-byte per-effect bound",
            policy.max_content_bytes
        ));
    }
    let digest = Sha256::digest(bytes);
    Ok((format!("sha256:{}", hex_encode(digest)), bytes.len()))
}

/// Authorize a `write_file` intent.
fn authorize_write(
    component_root: &Path,
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
    policy: &BrokerPolicy,
) -> Result<CheckedIntent, String> {
    let destination = require_string_arg(obj, "destination")?;
    let content = require_string_arg(obj, "content")?;
    let resolved = normalize_destination(component_root, &destination, policy)?;
    let (content_identity, byte_len) = content_identity(&content, policy)?;

    Ok(CheckedIntent {
        call_id: intent.id.clone(),
        tool: "write_file".to_owned(),
        op: if resolved.is_file() {
            EffectOp::Modify
        } else {
            EffectOp::Create
        },
        destination: normalized_relative(component_root, &resolved),
        content_identity: Some(content_identity),
        content_bytes: byte_len,
        purpose: format!("authoring: {}", intent.id),
    })
}

/// Authorize an `edit_file` intent. Requires an existing target and either a
/// replacement substring or new content; without a change there is no effect.
fn authorize_edit(
    component_root: &Path,
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
    policy: &BrokerPolicy,
) -> Result<CheckedIntent, String> {
    let destination = require_string_arg(obj, "destination")?;
    let resolved = normalize_destination(component_root, &destination, policy)?;

    if !resolved.is_file() {
        return Err(format!(
            "edit destination `{destination}` does not exist or is not a file"
        ));
    }
    let replacement = string_value(obj.get("replace"));
    let content = string_value(obj.get("content"));
    match (replacement.is_some(), content.is_some()) {
        (false, false) => {
            return Err("edit_file requires either a non-empty `replace` or `content`".to_owned());
        }
        _ => {}
    }

    let (content_identity, byte_len) = match content.as_deref() {
        Some(new_content) => content_identity(new_content, policy)?,
        None => (String::new(), 0),
    };

    Ok(CheckedIntent {
        call_id: intent.id.clone(),
        tool: "edit_file".to_owned(),
        op: EffectOp::Modify,
        destination: normalized_relative(component_root, &resolved),
        content_identity: if content_identity.is_empty() {
            None
        } else {
            Some(content_identity)
        },
        content_bytes: byte_len,
        purpose: format!("authoring: {}", intent.id),
    })
}

/// Borrow a non-empty string value when present, else `None`.
fn string_value(value: Option<&serde_json::Value>) -> Option<String> {
    match value {
        Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn intent(name: &str, args: serde_json::Value) -> ToolIntent {
        ToolIntent {
            id: "call_1".to_owned(),
            provider_id: None,
            name: name.to_owned(),
            arguments: args,
        }
    }

    fn write_intent(destination: &str, content: &str) -> ToolIntent {
        intent(
            "write_file",
            serde_json::json!({ "destination": destination, "content": content }),
        )
    }

    fn edit_intent(destination: &str, replace: &str) -> ToolIntent {
        intent(
            "edit_file",
            serde_json::json!({ "destination": destination, "replace": replace }),
        )
    }

    fn policy() -> BrokerPolicy {
        BrokerPolicy::default()
    }

    #[test]
    fn default_policy_writable_roots_are_src_and_tests() {
        let policy = policy();
        let expected = vec!["src".to_owned(), "tests".to_owned()];
        assert_eq!(policy.writable_roots, expected);
    }

    #[test]
    fn authorized_relative_write_becomes_checked_effect() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[write_intent("src/lib.rs", "fn main() {}")],
            &policy(),
        );
        assert_eq!(plan.effects.len(), 1);
        assert_eq!(plan.dropped_count(), 0);
        let effect = &plan.effects[0];
        assert_eq!(effect.tool, "write_file");
        assert_eq!(effect.op, EffectOp::Create);
        assert_eq!(effect.destination, "src/lib.rs");
        assert!(
            effect
                .content_identity
                .as_deref()
                .unwrap()
                .starts_with("sha256:")
        );
        assert_eq!(effect.content_bytes, "fn main() {}".len());
    }

    #[test]
    fn absolute_destination_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(dir.path(), &[write_intent("/etc/passwd", "x")], &policy());
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains("relative"));
    }

    #[test]
    fn parent_directory_escape_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(dir.path(), &[write_intent("../secret.txt", "x")], &policy());
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains(".."));
    }

    #[test]
    fn hidden_escape_via_trailing_parent_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let deep = format!("src/{}/escape", "a".repeat(3));
        let plan = authorize_intents(dir.path(), &[write_intent("../x/../y", "z")], &policy());
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn protected_document_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[write_intent("CONTRACT.md", "changed")],
            &policy(),
        );
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains("protected"));
    }

    #[test]
    fn destination_outside_writable_root_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[write_intent("docs/guide.md", "content")],
            &policy(),
        );
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains("writable root"));
    }

    #[test]
    fn unknown_tool_is_dropped_by_default() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let unlink = intent("unlink", serde_json::json!({ "destination": "src/x" }));
        let plan = authorize_intents(dir.path(), &[unlink], &policy());
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(
            plan.dropped[0]
                .reason
                .contains("not in the authorized authoring set")
        );
    }

    #[test]
    fn system_command_tool_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[intent(
                "shell_exec",
                serde_json::json!({ "command": "curl evil" }),
            )],
            &policy(),
        );
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn non_object_arguments_are_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[intent(
                "write_file",
                serde_json::json!(["not", "an", "object"]),
            )],
            &policy(),
        );
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains("JSON object"));
    }

    #[test]
    fn missing_content_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[intent(
                "write_file",
                serde_json::json!({ "destination": "src/x.rs" }),
            )],
            &policy(),
        );
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains("content"));
    }

    #[test]
    fn empty_destination_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[intent(
                "write_file",
                serde_json::json!({ "destination": "", "content": "x" }),
            )],
            &policy(),
        );
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn over_depth_destination_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let deep = format!("src/{}/deep.rs", "a/".repeat(MAX_DESTINATION_DEPTH + 1));
        let plan = authorize_intents(dir.path(), &[write_intent(&deep, "x")], &policy());
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains("depth"));
    }

    #[test]
    fn oversized_content_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let big = "a".repeat(MAX_CONTENT_BYTES + 1);
        let plan = authorize_intents(dir.path(), &[write_intent("src/big.rs", &big)], &policy());
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains("per-effect bound"));
    }

    #[test]
    fn edit_of_missing_file_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(dir.path(), &[edit_intent("src/absent.rs", "x")], &policy());
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn edit_of_existing_file_becomes_effect() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/ex.rs"), "original").unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[edit_intent("src/ex.rs", "replaced")],
            &policy(),
        );
        assert_eq!(plan.effects.len(), 1);
        assert_eq!(plan.effects[0].op, EffectOp::Modify);
        assert!(plan.effects[0].content_identity.is_none());
    }

    #[test]
    fn empty_intent_name_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[intent(
                "   ",
                serde_json::json!({ "destination": "src/x", "content": "y" }),
            )],
            &policy(),
        );
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn plan_enforces_plan_bound_by_dropping_all() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let mut policy = policy();
        policy.max_plan_content_bytes = 3;
        let plan = authorize_intents(
            dir.path(),
            &[
                write_intent("src/a.rs", "ab"),
                write_intent("src/b.rs", "cd"),
            ],
            &policy,
        );
        assert_eq!(plan.content_bytes(), 4);
        let mut bounded = plan;
        bounded.enforce_bound(&policy);
        assert!(bounded.effects.is_empty());
        assert!(bounded.dropped_count() >= 2);
    }

    #[test]
    fn multiple_intents_partition_into_effects_and_drops() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("src/ex.rs"), "x").unwrap();
        let intents = vec![
            write_intent("src/ok.rs", "ok"),
            intent("mount", serde_json::json!({ "src": "x" })),
            edit_intent("src/ex.rs", "y"),
            write_intent("../escape", "z"),
        ];
        let plan = authorize_intents(dir.path(), &intents, &policy());
        assert_eq!(plan.effects.len(), 2);
        assert_eq!(plan.dropped_count(), 2);
        let tools: Vec<&str> = plan.dropped.iter().map(|d| d.tool.as_str()).collect();
        assert!(tools.contains(&"mount"));
        assert!(tools.contains(&"write_file"));
    }
}
