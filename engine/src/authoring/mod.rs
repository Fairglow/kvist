//! Authoring tool-intent broker (ADR-0009).
//!
//! The model turn runs on the host and returns untrusted `ToolIntent` values. This
//! module is the broker: it authorizes each intent against a fixed, closed allowlist
//! with a deny-by-default, total policy, emitting only capability-bound
//! `CheckedIntent`s while recording every dropped intent as durable evidence.
//! Authorized effects are applied exclusively by the in-sandbox effect applier in
//! [`apply`](self::apply), which re-validates each [`CheckedIntent`] and writes
//! without following symbolic links. A dropped intent is recorded, never executed:
//! it becomes no sandbox effect and no filesystem change.
//!
//! The security guarantee lives *here*, in this total policy function, not in the
//! sandbox. The sandbox (bubblewrap) is defense-in-depth: it is never the thing we
//! certify. The broker is the source of truth for which authoring actions exist. The
//! fixed [`ALLOWED_TOOLS`] / [`EffectOp`] vocabulary below is the set a model may
//! propose; anything outside it is dropped, so a change to the set of allowed agent
//! actions is a change to *this* code and its tests, never to an untrusted agent.
//! There is no external spec to sync with, and agents cannot extend the set
//! themselves. [`classify_intent`](classify_intent) is the single funnel every
//! proposed action passes through.
//!
//! ## Tool semantics
//!
//! - `write_file`: `destination` + `content`. Creates a new file or overwrites an
//!   existing regular file with the exact supplied bytes. The parent directory must
//!   already exist.
//! - `edit_file`: `destination` + `replace` + `content`. The target file must exist
//!   and `replace` must occur in it exactly once; that one occurrence is replaced
//!   with `content` (empty content deletes it). The identity of the resulting bytes
//!   is bound into the [`CheckedIntent`] so the applier can prove the result.
//! - `propose_decision`: `summary` + `why` + optional `patch`. Surfaces an
//!   impactful, uncovered decision for the user. It never writes a protected
//!   document; the engine records a redacted proposal under the component state
//!   directory and ends the run in the awaiting-decision state.
//! - `request_dependency`: `name` + `origin`. Requests a new dependency. A request
//!   within policy (an exact, pinned revision) is recorded and accepted so the
//!   agent continues; a request outside policy (private, link-local, loopback,
//!   unverified, or unpinned origin) is surfaced as a decision and ends the run in
//!   the awaiting-decision state.

pub mod apply;

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use agent_runtime::{ModelTurn, ToolDefinition, ToolIntent};
use hex::encode as hex_encode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The fixed, closed vocabulary of authoring tools the broker will ever authorize.
///
/// This is the authoritative enumeration of allowed agent authoring actions. It is
/// *not* supplied by agents: every model proposes arbitrary tool calls, and only
/// these names are ever authorized.
pub const ALLOWED_TOOLS: &[&str] = &[
    "write_file",
    "edit_file",
    "propose_decision",
    "request_dependency",
];

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

/// Maximum bytes of a surfaced-decision summary or `why` justification. Bounds the
/// evidence recorded for a decision so a hostile model cannot bloat it.
pub const MAX_DECISION_BYTES: usize = 8 << 10;

/// Maximum bytes of a surfaced-decision patch or dependency origin. Bounds the
/// recorded proposal so a hostile model cannot bloat it.
pub const MAX_PATCH_BYTES: usize = 64 << 10;

/// Maximum length of a dependency crate identity. Bounds the recorded identity.
pub const MAX_DEPENDENCY_NAME_BYTES: usize = 64;

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
    /// `sha256:hex` identity of the bytes the effect produces: the authored content
    /// for `write_file`, or the resulting file content for `edit_file`.
    pub content_identity: String,
    /// Exact substring to replace, for `edit_file`; `None` for `write_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    /// Bytes of the produced content for this effect, for plan-bound accounting.
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

/// A decision the agent surfaced for human intervention.
///
/// Recorded, never applied: it is persisted under the component state directory as
/// durable, inspectable evidence and ends the run in the awaiting-decision state.
/// It never contains bytes written to a protected intent document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedDecision {
    /// Turn-local call identity, echoed from the model intent.
    pub call_id: String,
    /// Bounded, non-secret summary of the uncovered, impactful decision.
    pub summary: String,
    /// Why the issue is decision-worthy rather than a trivial, review-only matter.
    pub why: String,
    /// Optional proposed change, as a redacted patch or draft; never a protected
    /// document write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
}

/// A dependency the agent requested mid-task that is within the dependency
/// policy. Out-of-policy requests are surfaced as [`ProposedDecision`]s instead,
/// so a recorded request is by definition accepted for acquisition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyRequest {
    /// Turn-local call identity, echoed from the model intent.
    pub call_id: String,
    /// Crate or spec identity requested.
    pub name: String,
    /// Exact, declared origin (a pinned revision or a public VCS origin).
    pub origin: String,
    /// Non-secret policy justification recorded as evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The class of a brokered turn outcome. A single funnel routes every intent to
/// exactly one of these; the broker is total and infallible, so no untrusted
/// intent can panic, wedge, or silently vanish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerOutcome {
    /// An authorized write effect.
    Effect(CheckedIntent),
    /// A decision worthy of human intervention.
    Decision(ProposedDecision),
    /// A dependency request evaluated against policy.
    Request(DependencyRequest),
    /// A refused intent, with a non-secret reason.
    Drop(String),
}

/// The brokered result of authoring every intent from one model turn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoringPlan {
    /// Authorized, capability-bound effects, in proposal order.
    pub effects: Vec<CheckedIntent>,
    /// Surfaced decisions, in proposal order, for durable evidence.
    pub decisions: Vec<ProposedDecision>,
    /// Accepted dependency requests, in proposal order, for acquisition.
    pub dependency_requests: Vec<DependencyRequest>,
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

    /// True when any decision worthy of human intervention was surfaced.
    pub fn has_decision(&self) -> bool {
        !self.decisions.is_empty()
    }

    /// Number of surfaced decisions.
    pub fn decision_count(&self) -> usize {
        self.decisions.len()
    }

    /// Number of accepted dependency requests.
    pub fn dependency_request_count(&self) -> usize {
        self.dependency_requests.len()
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

/// One authorized effect staged for in-sandbox application. The host writes it
/// under the component state directory; the effect applier reads it back through a
/// read-only mount, re-validates every bound, proves the content identity, and
/// applies the effect without following symbolic links. Staging and application
/// share this exact shape so nothing the model proposed can change in transit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedIntent {
    /// Staged-intent schema version; only version 1 is recognized.
    pub schema_version: u32,
    /// Turn-local call identity, echoed from the model intent.
    pub call_id: String,
    /// Authorized tool name (one of [`ALLOWED_TOOLS`]).
    pub tool: String,
    /// Destination relative to the component directory.
    pub destination: String,
    /// `sha256:hex` identity the applier must prove before writing.
    pub content_identity: String,
    /// Authored bytes: the file content for `write_file`, the replacement text
    /// for `edit_file`.
    pub content: String,
    /// Exact substring to replace, for `edit_file`; `None` for `write_file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
}

/// Schema version of [`StagedIntent`].
pub const STAGED_INTENT_SCHEMA_VERSION: u32 = 1;

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
            BrokerOutcome::Effect(checked) => plan.effects.push(checked),
            BrokerOutcome::Decision(decision) => plan.decisions.push(decision),
            BrokerOutcome::Request(request) => plan.dependency_requests.push(request),
            BrokerOutcome::Drop(reason) => plan.dropped.push(DroppedIntent {
                call_id: intent.id.clone(),
                tool: intent.name.clone(),
                reason,
            }),
        }
    }
    // The plan-wide bound is enforced here so no caller can observe an
    // over-budget plan, applied or otherwise.
    plan.enforce_bound(policy);
    plan
}

/// Bounds a non-secret, user-facing string into bounded, redactable evidence.
fn bounded_text(value: &str, max: usize, label: &str) -> Result<String, String> {
    if value.is_empty() {
        return Err(format!("argument `{label}` must be non-empty"));
    }
    if value.len() > max {
        return Err(format!("argument `{label}` exceeds the {max}-byte bound"));
    }
    Ok(value.to_owned())
}

/// Reduce a single untrusted intent to a brokered outcome or a drop reason.
///
/// Deny-by-default: the tool name must be in [`ALLOWED_TOOLS`]; arguments must be a
/// JSON object. Write tools additionally require a normalized destination and
/// bounded content; the decision tool requires bounded summary and `why`; the
/// dependency tool validates its identity and origin against the dependency
/// policy. Any failure is a drop reason, never a panic, so a hostile or buggy
/// model cannot wedge the broker.
fn classify_intent(
    component_root: &Path,
    intent: &ToolIntent,
    policy: &BrokerPolicy,
) -> BrokerOutcome {
    if intent.name.trim().is_empty() {
        return BrokerOutcome::Drop("tool name is empty".to_owned());
    }
    if !ALLOWED_TOOLS.contains(&intent.name.as_str()) {
        return BrokerOutcome::Drop(format!(
            "tool `{}` is not in the authorized authoring set {:?}",
            intent.name, ALLOWED_TOOLS
        ));
    }
    let Some(obj) = intent.arguments.as_object() else {
        return BrokerOutcome::Drop(format!(
            "tool `{}` arguments must be a JSON object",
            intent.name
        ));
    };
    match intent.name.as_str() {
        "write_file" => match authorize_write(component_root, intent, obj, policy) {
            Ok(effect) => BrokerOutcome::Effect(effect),
            Err(reason) => BrokerOutcome::Drop(reason),
        },
        "edit_file" => match authorize_edit(component_root, intent, obj, policy) {
            Ok(effect) => BrokerOutcome::Effect(effect),
            Err(reason) => BrokerOutcome::Drop(reason),
        },
        "propose_decision" => match classify_propose(intent, obj) {
            Ok(decision) => BrokerOutcome::Decision(decision),
            Err(reason) => BrokerOutcome::Drop(reason),
        },
        "request_dependency" => classify_dependency(intent, obj),
        // The allowlist check above makes this arm unreachable; it is kept as a
        // fail-closed default rather than a panic.
        _ => BrokerOutcome::Drop(format!(
            "tool `{}` is not in the authorized authoring set {:?}",
            intent.name, ALLOWED_TOOLS
        )),
    }
}

/// Authorizes a `propose_decision` intent into a surfaced decision. A malformed
/// proposal is dropped (the model misused the tool); a well-formed one is recorded
/// as durable evidence and will end the run in the awaiting-decision state.
fn classify_propose(
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<ProposedDecision, String> {
    let summary = bounded_text(
        obj.get("summary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default(),
        MAX_DECISION_BYTES,
        "summary",
    )?;
    let why = bounded_text(
        obj.get("why")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default(),
        MAX_DECISION_BYTES,
        "why",
    )?;
    let patch = match obj.get("patch").and_then(serde_json::Value::as_str) {
        None | Some("") => None,
        Some(value) => Some(bounded_text(value, MAX_PATCH_BYTES, "patch")?),
    };
    Ok(ProposedDecision {
        call_id: intent.id.clone(),
        summary,
        why,
        patch,
    })
}

/// The set of host substrings that mark an origin private, link-local, loopback,
/// or otherwise unverified-remote and therefore outside the dependency policy.
const UNVERIFIED_ORIGIN_PREFIXES: [&str; 8] = [
    "127.",
    "10.",
    "192.168.",
    "169.254.",
    "172.16.",
    "172.17.",
    "localhost",
    "file://",
];

/// Rejects an origin that is private, link-local, loopback, a local path, or a
/// VCS/registry scheme pointing at a non-public host. Public crates.io and public
/// VCS hosts are not rejected here; the caller additionally requires an exact,
/// pinned revision.
fn is_unverified_origin(origin: &str) -> bool {
    UNVERIFIED_ORIGIN_PREFIXES
        .iter()
        .any(|prefix| origin.starts_with(prefix))
    || origin.starts_with("./")
    || origin.starts_with("/")
    // Any scheme other than a public http(s) or git-over-http(s)/ssh VCS origin
    // (path, ftp, custom, git+http) is treated as unverified here.
    || origin.contains("://")
        && !origin.starts_with("https://")
        && !origin.starts_with("git+https://")
        && !origin.starts_with("git+ssh://")
        && !origin.starts_with("ssh://")
}

/// Whether a crate identity looks like a plausible registry name (letters,
/// digits, `-`, `_`, `.`), used to reject malformed dependency names.
fn looks_like_crate_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_DEPENDENCY_NAME_BYTES
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Whether the leading character of an unpinned-precedence operator marks the
/// origin as a range/comparator/wildcard rather than an exact revision.
fn has_version_precedence_operator(origin: &str) -> bool {
    matches!(
        origin.bytes().next(),
        Some(b'^' | b'~' | b'>' | b'<' | b'=')
    )
}

/// Whether an origin is an exact, three-part semver revision with an optional
/// pre-release/build suffix. Wildcards (`1.x`, `*`), ranges, comparators, and
/// partials (`1`, `1.2`) are rejected.
fn is_exact_semver(value: &str) -> bool {
    if value
        .bytes()
        .any(|byte| matches!(byte, b'*' | b'>' | b'<' | b'=' | b'^' | b'~'))
    {
        return false;
    }
    let mut numbers = value.split('.');
    let core = matches!(
        (numbers.next(), numbers.next(), numbers.next()),
        (
            Some(major),
            Some(minor),
            Some(patch)
        ) if !major.is_empty()
            && !minor.is_empty()
            && !patch.is_empty()
            && numbers.next().is_none()
            && major.bytes().all(|byte| byte.is_ascii_digit())
            && minor.bytes().all(|byte| byte.is_ascii_digit())
            && patch.bytes().all(|byte| byte.is_ascii_digit())
    );
    if !core {
        return false;
    }
    // The pre-release/build suffix, if present after `-` or `+`, may contain
    // dotted alphanumeric identifiers; an empty identifier is rejected.
    for tail in value.split(['-', '+']).skip(1) {
        if tail.is_empty()
            || !tail
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
        {
            return false;
        }
    }
    true
}

/// Whether a VCS `?rev=`/`?tag=` fragment pins the origin to a specific, non-empty
/// reference (a long commit-ish or a tag), rather than an empty or wildcard ref.
fn vcs_ref_is_pinned(value: &str) -> bool {
    let value = value.split('&').next().unwrap_or("").trim();
    !value.is_empty()
        && value.len() >= 7
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
}

/// Whether an origin is an exact, pinned revision rather than a wildcard, range,
/// comparator, branch, or unpinned partial. Crates.io revisions are `x.y.z` with
/// an optional pre-release/build suffix; public VCS origins require an explicit
/// `?rev=` or `?tag=` fragment.
fn looks_pinned(origin: &str) -> bool {
    if has_version_precedence_operator(origin) {
        return false;
    }
    if is_exact_semver(origin) {
        return true;
    }
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "https" | "git+https" | "git+ssh" | "ssh") {
        return false;
    }
    match rest.split_once("?rev=") {
        Some((_, rev)) => vcs_ref_is_pinned(rev),
        None => match rest.split_once("?tag=") {
            Some((_, tag)) => vcs_ref_is_pinned(tag),
            None => false,
        },
    }
}

/// Evaluates a dependency request against policy. Returns the recorded request
/// when the origin is an exact, pinned, non-private revision, with a policy
/// justification; returns a surfaced decision otherwise.
fn classify_dependency(
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> BrokerOutcome {
    let name = match obj.get("name").and_then(serde_json::Value::as_str) {
        None | Some("") => {
            return BrokerOutcome::Drop("argument `name` must be non-empty".to_owned());
        }
        Some(name) if !looks_like_crate_name(name) => {
            return BrokerOutcome::Drop(format!(
                "dependency `{name}` is not a plausible crate identity"
            ));
        }
        Some(name) => name.to_owned(),
    };
    let origin = match obj.get("origin").and_then(serde_json::Value::as_str) {
        None | Some("") => {
            return BrokerOutcome::Drop("argument `origin` must be non-empty".to_owned());
        }
        Some(origin) => origin,
    };
    if is_unverified_origin(origin) {
        return BrokerOutcome::Decision(ProposedDecision {
            call_id: intent.id.clone(),
            summary: format!(
                "dependency `{name}` origin `{origin}` is private, link-local, loopback, a local path, or an unverified remote"
            ),
            why: "a dependency from an untrusted origin can inject unreviewed code".to_owned(),
            patch: None,
        });
    }
    if !looks_pinned(origin) {
        return BrokerOutcome::Decision(ProposedDecision {
            call_id: intent.id.clone(),
            summary: format!(
                "dependency `{name}` origin `{origin}` is not an exact, pinned revision"
            ),
            why: "an unpinned or wildcard origin can resolve to an unreviewed revision".to_owned(),
            patch: None,
        });
    }
    BrokerOutcome::Request(DependencyRequest {
        call_id: intent.id.clone(),
        name,
        origin: origin.to_owned(),
        reason: Some("exact, pinned, non-private origin".to_owned()),
    })
}

/// Model-facing descriptors for the closed authoring tool set. The schemas mirror
/// exactly what the broker accepts, so a conforming model never proposes an
/// intent the broker must drop on a shape error.
pub fn authoring_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "write_file".to_owned(),
            description: "Create or overwrite a single file under src/ or tests/ with \
             the exact supplied content."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "destination": {
                        "type": "string",
                        "description": "Component-relative path under src/ or tests/; the parent directory must exist"
                    },
                    "content": {
                        "type": "string",
                        "description": "Exact new file content"
                    }
                },
                "required": ["destination", "content"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "edit_file".to_owned(),
            description: "Replace exactly one occurrence of an exact substring in an \
             existing file under src/ or tests/ with new content."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "destination": {
                        "type": "string",
                        "description": "Component-relative path of the existing file"
                    },
                    "replace": {
                        "type": "string",
                        "description": "Exact substring; must occur in the file exactly once"
                    },
                    "content": {
                        "type": "string",
                        "description": "Replacement text; empty deletes the occurrence"
                    }
                },
                "required": ["destination", "replace", "content"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "propose_decision".to_owned(),
            description: "Surface an impactful decision that is not already covered by \
             the component's REQUIREMENTS, CONTRACT, DESIGN, or TODOS. This ends the \
             task run and pauses it for a human; use it only for decisions that \
             substantially alter the implementation, never for trivial issues. Do \
             NOT use it to write REQUIREMENTS.md, CONTRACT.md, DESIGN.md, TODOS.yaml, \
             or IMPL.md; Kvist maintains those separately."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "Non-secret summary of the uncovered, impactful decision"
                    },
                    "why": {
                        "type": "string",
                        "description": "Why the issue is decision-worthy rather than a trivial, review-only matter"
                    },
                    "patch": {
                        "type": "string",
                        "description": "Optional proposed change as a redacted draft; never bytes for a protected document"
                    }
                },
                "required": ["summary", "why"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "request_dependency".to_owned(),
            description: "Request a new or changed dependency mid-task. An exact, pinned \
             revision from a public source is accepted automatically and you continue; \
             a private, link-local, loopback, unverified, or unpinned origin is \
             surfaced as a decision and pauses the run. Provide an exact version such \
             as 1.2.3, or a public URL with an explicit ?rev= or ?tag=."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Crate or spec identity requested"
                    },
                    "origin": {
                        "type": "string",
                        "description": "Exact, pinned origin: a registry revision (1.2.3) or a public VCS URL with ?rev= or ?tag="
                    }
                },
                "required": ["name", "origin"],
                "additionalProperties": false
            }),
        },
    ]
}

/// Extract a required string argument. Missing or wrong-type arguments are drop
/// reasons, not panics. Set `allow_empty` to accept the empty string (used for
/// `edit_file` content, where empty means deletion).
fn string_arg(
    obj: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    allow_empty: bool,
) -> Result<String, String> {
    match obj.get(key) {
        Some(serde_json::Value::String(value)) if !value.is_empty() || allow_empty => {
            Ok(value.clone())
        }
        Some(serde_json::Value::String(_)) => {
            Err(format!("argument `{key}` must be a non-empty string"))
        }
        Some(_) => Err(format!("argument `{key}` must be a string")),
        None => Err(format!("missing required argument `{key}`")),
    }
}

/// Normalize a relative destination to a component-root-anchored path, rejecting
/// absolute paths, `..`, protected documents, non-writable roots, over-deep paths,
/// and any escape from the component root. Reused by the effect applier so host
/// authorization and in-sandbox application share one normalization.
pub(crate) fn normalize_destination(
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
    content_identity_bytes(content.as_bytes(), policy)
}

/// Compute the `sha256:hex` content identity of authored bytes, honoring the
/// per-effect content bound. Returns `(identity, byte_len)` or a drop reason.
fn content_identity_bytes(bytes: &[u8], policy: &BrokerPolicy) -> Result<(String, usize), String> {
    if bytes.len() > policy.max_content_bytes {
        return Err(format!(
            "content exceeds the {}-byte per-effect bound",
            policy.max_content_bytes
        ));
    }
    let digest = Sha256::digest(bytes);
    Ok((format!("sha256:{}", hex_encode(digest)), bytes.len()))
}

/// Counts non-overlapping occurrences of `needle` in `haystack`. An empty
/// needle counts as zero occurrences.
fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// Replaces the single occurrence of `needle` in `haystack` with `replacement`.
/// Returns `None` when `needle` does not occur (callers bound the count first),
/// so the replacement cannot silently no-op or hit an unintended occurrence.
fn replace_single_occurrence(
    haystack: &[u8],
    needle: &[u8],
    replacement: &[u8],
) -> Option<Vec<u8>> {
    let position = haystack
        .windows(needle.len())
        .position(|window| window == needle)?;
    let mut result = haystack.to_vec();
    result.splice(
        position..position + needle.len(),
        replacement.iter().copied(),
    );
    Some(result)
}

/// Classify a resolved destination for `write_file`: a symlink or a non-regular
/// file is refused, a missing destination requires a real existing parent directory,
/// and an existing regular file is a modify.
fn classify_write_destination(resolved: &Path) -> Result<EffectOp, String> {
    match fs::symlink_metadata(resolved) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "destination `{}` is a symbolic link",
            resolved.display()
        )),
        Ok(metadata) if metadata.is_file() => Ok(EffectOp::Modify),
        Ok(_) => Err(format!(
            "destination `{}` exists and is not a regular file",
            resolved.display()
        )),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            match resolved
                .parent()
                .and_then(|parent| fs::symlink_metadata(parent).ok())
            {
                Some(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    Ok(EffectOp::Create)
                }
                _ => Err(format!(
                    "parent directory of `{}` is missing or not a real directory",
                    resolved.display()
                )),
            }
        }
        Err(source) => Err(format!(
            "destination `{}` cannot be inspected: {source}",
            resolved.display()
        )),
    }
}

/// Authorize a `write_file` intent.
fn authorize_write(
    component_root: &Path,
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
    policy: &BrokerPolicy,
) -> Result<CheckedIntent, String> {
    let destination = string_arg(obj, "destination", false)?;
    let content = string_arg(obj, "content", false)?;
    let resolved = normalize_destination(component_root, &destination, policy)?;
    let op = classify_write_destination(&resolved)?;
    let (content_identity, byte_len) = content_identity(&content, policy)?;

    Ok(CheckedIntent {
        call_id: intent.id.clone(),
        tool: "write_file".to_owned(),
        op,
        destination: normalized_relative(component_root, &resolved),
        content_identity,
        replacement: None,
        content_bytes: byte_len,
        purpose: format!("authoring: {}", intent.id),
    })
}

/// Authorize an `edit_file` intent. The target must exist as a regular non-symlink
/// file; `replace` must occur in it exactly once; the identity of the resulting
/// bytes is bound into the effect so the applier can prove it applied exactly the
/// authorized result.
fn authorize_edit(
    component_root: &Path,
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
    policy: &BrokerPolicy,
) -> Result<CheckedIntent, String> {
    let destination = string_arg(obj, "destination", false)?;
    let replacement = string_arg(obj, "replace", false)?;
    let content = string_arg(obj, "content", true)?;
    let resolved = normalize_destination(component_root, &destination, policy)?;

    match fs::symlink_metadata(&resolved) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "edit destination `{destination}` is a symbolic link"
            ));
        }
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return Err(format!(
                "edit destination `{destination}` is not a regular file"
            ));
        }
        Err(source) => {
            return Err(format!(
                "edit destination `{destination}` does not exist or cannot be inspected: {source}"
            ));
        }
    }

    let existing = fs::read(&resolved)
        .map_err(|source| format!("edit destination `{destination}` cannot be read: {source}"))?;
    if existing.len() > policy.max_content_bytes {
        return Err(format!(
            "edit destination `{destination}` exceeds the {}-byte per-effect bound",
            policy.max_content_bytes
        ));
    }
    let occurrences = count_occurrences(&existing, replacement.as_bytes());
    if occurrences != 1 {
        return Err(format!(
            "`replace` must occur exactly once in `{destination}` (found {occurrences})"
        ));
    }
    let Some(new_content) =
        replace_single_occurrence(&existing, replacement.as_bytes(), content.as_bytes())
    else {
        return Err(format!(
            "`replace` must occur exactly once in `{destination}` (found {occurrences})"
        ));
    };
    let (content_identity, byte_len) = content_identity_bytes(&new_content, policy)?;

    Ok(CheckedIntent {
        call_id: intent.id.clone(),
        tool: "edit_file".to_owned(),
        op: EffectOp::Modify,
        destination: normalized_relative(component_root, &resolved),
        content_identity,
        replacement: Some(replacement),
        content_bytes: byte_len,
        purpose: format!("authoring: {}", intent.id),
    })
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

    fn edit_intent(destination: &str, replace: &str, content: &str) -> ToolIntent {
        intent(
            "edit_file",
            serde_json::json!({
                "destination": destination,
                "replace": replace,
                "content": content
            }),
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
        assert!(effect.content_identity.starts_with("sha256:"));
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
        let plan = authorize_intents(
            dir.path(),
            &[edit_intent("src/absent.rs", "x", "y")],
            &policy(),
        );
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn edit_of_existing_file_binds_result_identity() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/ex.rs"), "original").unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[edit_intent("src/ex.rs", "original", "changed")],
            &policy(),
        );
        assert_eq!(plan.effects.len(), 1);
        let effect = &plan.effects[0];
        assert_eq!(effect.op, EffectOp::Modify);
        assert_eq!(effect.replacement.as_deref(), Some("original"));
        let expected = format!("sha256:{}", hex_encode(Sha256::digest(b"changed")));
        assert_eq!(effect.content_identity, expected);
        assert_eq!(effect.content_bytes, 7);
    }

    #[test]
    fn edit_with_ambiguous_replacement_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/ex.rs"), "aa middle aa").unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[edit_intent("src/ex.rs", "aa", "b")],
            &policy(),
        );
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains("exactly once"));
    }

    #[test]
    fn edit_with_empty_content_deletes_the_occurrence() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/ex.rs"), "keep drop keep").unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[edit_intent("src/ex.rs", "drop ", "")],
            &policy(),
        );
        assert_eq!(plan.effects.len(), 1);
        let expected = format!("sha256:{}", hex_encode(Sha256::digest(b"keep keep")));
        assert_eq!(plan.effects[0].content_identity, expected);
    }

    #[test]
    fn edit_of_symlink_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("target.txt"), "outside").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            dir.path().join("target.txt"),
            dir.path().join("src/link.rs"),
        )
        .unwrap();
        #[cfg(unix)]
        let plan = authorize_intents(
            dir.path(),
            &[edit_intent("src/link.rs", "outside", "changed")],
            &policy(),
        );
        #[cfg(unix)]
        {
            assert!(plan.effects.is_empty());
            assert_eq!(plan.dropped_count(), 1);
            assert!(plan.dropped[0].reason.contains("symbolic link"));
        }
    }

    #[test]
    fn write_of_symlink_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("target.txt"), "outside").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            dir.path().join("target.txt"),
            dir.path().join("src/link.rs"),
        )
        .unwrap();
        #[cfg(unix)]
        let plan = authorize_intents(dir.path(), &[write_intent("src/link.rs", "x")], &policy());
        #[cfg(unix)]
        {
            assert!(plan.effects.is_empty());
            assert_eq!(plan.dropped_count(), 1);
            assert!(plan.dropped[0].reason.contains("symbolic link"));
        }
    }

    #[test]
    fn write_with_missing_parent_is_dropped() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[write_intent("src/nested/x.rs", "x")],
            &policy(),
        );
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains("parent directory"));
    }

    #[test]
    fn tool_definitions_cover_exactly_the_allowed_set() {
        let tools = authoring_tool_definitions();
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names, ALLOWED_TOOLS);
        // The advertised tool set is the authority: every advertised tool is
        // authorized and every authorized tool is advertised, so a tool the
        // broker would refuse can never be offered to the model.
        for tool in &tools {
            assert!(ALLOWED_TOOLS.contains(&tool.name.as_str()));
            assert!(tool.parameters.is_object());
        }
        for allowed in ALLOWED_TOOLS {
            assert!(names.contains(allowed));
        }
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
        // The two effects total four bytes, over the three-byte plan bound.
        let plan = authorize_intents(
            dir.path(),
            &[
                write_intent("src/a.rs", "ab"),
                write_intent("src/b.rs", "cd"),
            ],
            &policy,
        );
        // The plan-wide bound is enforced inside authorization itself: no
        // caller can ever observe an over-budget plan, applied or otherwise.
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 2);
        assert!(
            plan.dropped
                .iter()
                .all(|dropped| dropped.reason.contains("plan bound"))
        );
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
            edit_intent("src/ex.rs", "x", "y"),
            write_intent("../escape", "z"),
        ];
        let plan = authorize_intents(dir.path(), &intents, &policy());
        assert_eq!(plan.effects.len(), 2);
        assert_eq!(plan.dropped_count(), 2);
        let tools: Vec<&str> = plan.dropped.iter().map(|d| d.tool.as_str()).collect();
        assert!(tools.contains(&"mount"));
        assert!(tools.contains(&"write_file"));
    }

    fn propose_intent(summary: &str, why: &str) -> ToolIntent {
        intent(
            "propose_decision",
            serde_json::json!({ "summary": summary, "why": why }),
        )
    }

    fn dependency_intent(name: &str, origin: &str) -> ToolIntent {
        intent(
            "request_dependency",
            serde_json::json!({ "name": name, "origin": origin }),
        )
    }

    #[test]
    fn valid_propose_becomes_a_surfaced_decision() {
        let plan = authorize_intents(
            tempdir().unwrap().path(),
            &[propose_intent(
                "use a different crate",
                "the recorded fit is worse",
            )],
            &policy(),
        );
        assert!(plan.has_decision());
        assert_eq!(plan.decision_count(), 1);
        assert!(plan.effects.is_empty());
        assert_eq!(plan.dropped_count(), 0);
        assert_eq!(plan.decisions[0].summary, "use a different crate");
        assert_eq!(plan.decisions[0].why, "the recorded fit is worse");
    }

    fn propose_intent_raw(args: serde_json::Value) -> ToolIntent {
        intent("propose_decision", args)
    }

    #[test]
    fn propose_with_a_patch_records_the_patch_only() {
        let plan = authorize_intents(
            tempdir().unwrap().path(),
            &[propose_intent_raw(serde_json::json!({
                "summary": "refactor",
                "why": "style",
                "patch": "s/old/new",
            }))],
            &policy(),
        );
        assert_eq!(plan.decision_count(), 1);
        assert_eq!(plan.decisions[0].patch.as_deref(), Some("s/old/new"));
    }

    #[test]
    fn propose_without_a_patch_leaves_patch_none() {
        let plan = authorize_intents(
            tempdir().unwrap().path(),
            &[propose_intent("refactor", "style")],
            &policy(),
        );
        assert_eq!(plan.decision_count(), 1);
        assert!(plan.decisions[0].patch.is_none());
    }

    #[test]
    fn propose_missing_summary_is_dropped() {
        let plan = authorize_intents(
            tempdir().unwrap().path(),
            &[intent(
                "propose_decision",
                serde_json::json!({ "why": "style" }),
            )],
            &policy(),
        );
        assert!(plan.decisions.is_empty());
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn propose_missing_why_is_dropped() {
        let plan = authorize_intents(
            tempdir().unwrap().path(),
            &[intent(
                "propose_decision",
                serde_json::json!({ "summary": "x" }),
            )],
            &policy(),
        );
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn propose_summary_over_the_bound_is_dropped() {
        let huge = "a".repeat(MAX_DECISION_BYTES + 1);
        let plan = authorize_intents(
            tempdir().unwrap().path(),
            &[propose_intent(&huge, "ok")],
            &policy(),
        );
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn exact_registry_revision_dependency_is_accepted() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let plan = authorize_intents(
            dir.path(),
            &[dependency_intent("serde", "1.0.200")],
            &policy(),
        );
        assert_eq!(plan.dependency_request_count(), 1);
        assert!(plan.decisions.is_empty());
        assert_eq!(plan.dependency_requests[0].name, "serde");
        assert_eq!(plan.dependency_requests[0].origin, "1.0.200");
    }

    #[test]
    fn wildcard_dependency_is_surfaced_as_a_decision() {
        let plan = authorize_intents(
            tempdir().unwrap().path(),
            &[dependency_intent("serde", "*")],
            &policy(),
        );
        assert!(plan.has_decision());
        assert_eq!(plan.decision_count(), 1);
        assert_eq!(plan.dependency_request_count(), 0);
    }

    #[test]
    fn loopback_dependency_origin_is_surfaced_as_a_decision() {
        let plan = authorize_intents(
            tempdir().unwrap().path(),
            &[dependency_intent(
                "local-crate",
                "git+https://127.0.0.1:8443/some/crate.git",
            )],
            &policy(),
        );
        assert_eq!(plan.decision_count(), 1);
        assert_eq!(plan.dependency_request_count(), 0);
    }

    #[test]
    fn dependency_missing_name_is_dropped() {
        let plan = authorize_intents(
            tempdir().unwrap().path(),
            &[intent(
                "request_dependency",
                serde_json::json!({ "origin": "1.0.0" }),
            )],
            &policy(),
        );
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn dependency_missing_origin_is_dropped() {
        let plan = authorize_intents(
            tempdir().unwrap().path(),
            &[intent(
                "request_dependency",
                serde_json::json!({ "name": "serde" }),
            )],
            &policy(),
        );
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn decision_and_dependency_and_write_partition_into_their_outcomes() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let intents = vec![
            write_intent("src/a.rs", "a"),
            propose_intent("change approach", "not covered"),
            dependency_intent("tokio", "1.38.0"),
            dependency_intent("bad", "*"),
            intent("mount", serde_json::json!({})),
        ];
        let plan = authorize_intents(dir.path(), &intents, &policy());
        assert_eq!(plan.effects.len(), 1);
        assert_eq!(plan.dependency_request_count(), 1);
        assert_eq!(plan.decision_count(), 2);
        assert_eq!(plan.dropped_count(), 1);
    }

    #[test]
    fn all_closed_tools_are_advertised_in_the_tool_definitions() {
        let names: Vec<String> = authoring_tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert!(names.contains(&"write_file".to_owned()));
        assert!(names.contains(&"edit_file".to_owned()));
        assert!(names.contains(&"propose_decision".to_owned()));
        assert!(names.contains(&"request_dependency".to_owned()));
    }
}
