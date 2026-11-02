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
//!   document; the maerg records a redacted proposal under the component state
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

use hex::encode as hex_encode;
use sav::{ModelTurn, ToolDefinition, ToolIntent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The fixed, closed vocabulary of authoring tools the broker will ever authorize.
///
/// This is the authoritative enumeration of allowed agent authoring actions. It is
/// *not* supplied by agents: every model proposes arbitrary tool calls, and only
/// these names are ever authorized.
pub const ALLOWED_TOOLS: &[&str] = &[
    "read_file",
    "list_files",
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

/// A bounded, read-only file or directory inspection performed on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadRecord {
    /// Turn-local call identity, echoed from the model intent.
    pub call_id: String,
    /// Authorized read tool name (`read_file` or `list_files`).
    pub tool: String,
    /// Normalized component-relative path inspected.
    pub path: String,
    /// Bounded UTF-8 result returned to the model.
    pub content: String,
}

/// The class of a brokered turn outcome. A single funnel routes every intent to
/// exactly one of these; the broker is total and infallible, so no untrusted
/// intent can panic, wedge, or silently vanish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerOutcome {
    /// A bounded, read-only inspection of component files.
    Read(ReadRecord),
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
    /// Bounded read-only inspections, in proposal order.
    #[serde(default)]
    pub reads: Vec<ReadRecord>,
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
    let mut plan = authorize_intents(component_root, &turn.tool_intents, policy);
    let mut retained_decisions = Vec::new();
    for decision in plan.decisions.drain(..) {
        if is_completion_report_proposal(&decision) {
            plan.reads.push(ReadRecord {
                call_id: decision.call_id,
                tool: "propose_decision".to_owned(),
                path: "completion-note".to_owned(),
                content: "Task completion noted (`propose_decision` is only for blocking contradictions that prevent implementation, never for reporting task completion or routine status). Reply now with your final plain-text summary and NO tool calls so Kvist can run `cargo test` and complete the task.".to_owned(),
            });
        } else if is_routine_internal_proposal(&decision) {
            plan.reads.push(ReadRecord {
                call_id: decision.call_id,
                tool: "propose_decision".to_owned(),
                path: "auto-resolved-decision".to_owned(),
                content: "Auto-resolved without pausing: this task is already approved by the user, and routine implementation, test, or module-export checks do NOT require a human decision (everything is reviewed at the end). You have full authority to inspect files with `read_file` and modify `src/` and `tests/` directly with `edit_file` or `write_file`. Complete any remaining code/test edits now, or if your changes are complete, reply with a plain-text summary and NO tool calls.".to_owned(),
            });
        } else {
            retained_decisions.push(decision);
        }
    }
    plan.decisions = retained_decisions;

    let mut retained_drops = Vec::new();
    for dropped in plan.dropped.drain(..) {
        if matches!(dropped.tool.as_str(), "edit_file" | "write_file") {
            let path = if dropped.reason.contains("names a protected intent document")
                || dropped.reason.contains("is not under a writable root")
            {
                "write-scope-error"
            } else {
                "edit-error"
            };
            plan.reads.push(ReadRecord {
                call_id: dropped.call_id,
                tool: dropped.tool,
                path: path.to_owned(),
                content: format!(
                    "error: {} (not applied). Protected documents (`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`) are read-only; only files under `src/` and `tests/` may be modified. For `edit_file`, both `replace` and `destination` must be non-empty and `replace` must occur once in the target file. Check the target file with `read_file` or reply with a plain-text summary and NO tool calls if your changes are complete.",
                    dropped.reason
                ),
            });
        } else {
            retained_drops.push(dropped);
        }
    }
    plan.dropped = retained_drops;
    plan
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
            BrokerOutcome::Read(read) => plan.reads.push(read),
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
    if let Some(err) = obj
        .get("_malformed_arguments_error")
        .and_then(serde_json::Value::as_str)
    {
        let preview = obj
            .get("_malformed_arguments_preview")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let path = if intent.name == "propose_decision" {
            "completion-note"
        } else {
            "malformed-tool-arguments"
        };
        return BrokerOutcome::Read(ReadRecord {
            call_id: intent.id.clone(),
            tool: intent.name.clone(),
            path: path.to_owned(),
            content: format!(
                "warning: tool call to `{}` had malformed or truncated JSON arguments ({err}; preview: `{preview}`) and was not applied. If you still need to call `{}`, resend it with valid, smaller JSON arguments; if your task changes are already complete, reply now with a plain-text summary and NO tool calls.",
                intent.name, intent.name
            ),
        });
    }
    warn_extra_tool_arguments(&intent.name, obj);
    match intent.name.as_str() {
        "read_file" => match authorize_read_file(component_root, intent, obj) {
            Ok(read) => BrokerOutcome::Read(read),
            Err(reason) => BrokerOutcome::Drop(reason),
        },
        "list_files" => match authorize_list_files(component_root, intent, obj) {
            Ok(read) => BrokerOutcome::Read(read),
            Err(reason) => BrokerOutcome::Drop(reason),
        },
        "write_file" => {
            if let Some(outcome) = check_manifest_decision(component_root, intent, obj) {
                return outcome;
            }
            if let Some(noop_or_guard) =
                check_write_noop_or_destructive_overwrite(component_root, intent, obj)
            {
                return noop_or_guard;
            }
            match authorize_write(component_root, intent, obj, policy) {
                Ok(effect) => BrokerOutcome::Effect(effect),
                Err(reason) => BrokerOutcome::Drop(reason),
            }
        }
        "edit_file" => {
            if let Some(outcome) = check_manifest_decision(component_root, intent, obj) {
                return outcome;
            }
            if let Some(brace_err) = check_edit_rust_brace_balance(component_root, intent, obj) {
                return brace_err;
            }
            match authorize_edit(component_root, intent, obj, policy) {
                Ok(effect) => BrokerOutcome::Effect(effect),
                Err(reason) => BrokerOutcome::Drop(reason),
            }
        }
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
            name: "read_file".to_owned(),
            description:
                "Read a file in the component (such as Cargo.toml, src/..., or tests/...) \
             with 1-based line numbers. Always read existing files before modifying them so you \
             preserve existing code."
                    .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Component-relative file path to read (for example Cargo.toml, src/lib.rs, or tests/conformance.rs)"
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "Optional 1-based first line to read (defaults to 1)"
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Optional 1-based last line to read (inclusive; defaults to start_line + 1199)"
                    },
                    "search": {
                        "type": "string",
                        "description": "Optional substring to search for inside the file; returns matching lines with surrounding context and 1-based line numbers"
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "list_files".to_owned(),
            description: "List existing files and directories inside the component (excluding \
             .kvist, .git, and target). Call this first to discover existing source and test files."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Optional component-relative directory path to list (defaults to \".\")"
                    }
                },
                "additionalProperties": false
            }),
        },
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
            description: "Surface an irreconcilable architectural contradiction that is not \
             covered by REQUIREMENTS.md, CONTRACT.md, DESIGN.md, or TODOS.yaml and prevents \
             completing the task without human choice between concrete alternatives. NEVER \
             call this to report task completion, summarize implemented tests/code, or \
             confirm that no manifest changes are needed — when your work is done, simply \
             reply with plain text and NO tool calls."
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
                        "description": "Human-readable Markdown brief (at most 8192 bytes): situation and why human input is necessary, design implications, alternatives with tradeoffs, recommended approach and rationale, and missing evidence/questions. Recommendations are advisory, not approval; distinguish assumptions from facts."
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

const EXCLUDED_READ_DIRS: &[&str] = &[".git", ".kvist", ".kvist-attempts", "target"];
const MAX_READ_SLICE_BYTES: usize = 64 * 1024;
const DEFAULT_READ_LINES: usize = 1200;
const MAX_LIST_ENTRIES: usize = 512;
const MAX_LIST_DEPTH: usize = 8;

fn check_manifest_decision(
    component_root: &Path,
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Option<BrokerOutcome> {
    let destination = obj.get("destination").and_then(serde_json::Value::as_str)?;
    let normalized = destination.trim().trim_start_matches("./");
    if normalized != "Cargo.toml" {
        return None;
    }
    let content = obj
        .get("content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let replace = obj.get("replace").and_then(serde_json::Value::as_str);
    let existing = fs::read_to_string(component_root.join("Cargo.toml")).unwrap_or_default();
    let proposed = match replace {
        Some(before) => {
            if let Some(replaced) = replace_single_occurrence(
                existing.as_bytes(),
                before.as_bytes(),
                content.as_bytes(),
            ) {
                String::from_utf8_lossy(&replaced).into_owned()
            } else {
                format!("--- Cargo.toml (replace)\n{before}\n+++ Cargo.toml (with)\n{content}")
            }
        }
        None => content.to_owned(),
    };
    if !existing.is_empty() && proposed.trim() == existing.trim() {
        return Some(BrokerOutcome::Read(ReadRecord {
            call_id: intent.id.clone(),
            tool: intent.name.clone(),
            path: "Cargo.toml".to_owned(),
            content: "`Cargo.toml` already has that exact content (no changes needed). Note: Kvist runs `cargo test` automatically in the verification sandbox after you finish. If your changes for this task are complete, reply with a plain-text summary and NO tool calls.".to_owned(),
        }));
    }
    let mut patch = if !existing.is_empty() {
        format_line_diff("Cargo.toml", &existing, &proposed)
    } else {
        proposed
    };
    if patch.len() > MAX_PATCH_BYTES {
        patch.truncate(patch.floor_char_boundary(MAX_PATCH_BYTES));
    }
    Some(BrokerOutcome::Decision(ProposedDecision {
        call_id: intent.id.clone(),
        summary: "Proposed modification to component Cargo.toml".to_owned(),
        why: "The agent proposed modifying `Cargo.toml`, which is outside the automatic `src/` and `tests/` write scope because manifest and dependency changes require human review. Inspect the proposed `Cargo.toml` diff and decide whether to approve it or redirect the task.".to_owned(),
        patch: (!patch.is_empty()).then_some(patch),
    }))
}

fn format_line_diff(label: &str, before: &str, after: &str) -> String {
    let old_lines: Vec<&str> = before.lines().collect();
    let new_lines: Vec<&str> = after.lines().collect();
    let mut prefix = 0;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    let mut old_end = old_lines.len();
    let mut new_end = new_lines.len();
    while old_end > prefix && new_end > prefix && old_lines[old_end - 1] == new_lines[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }
    let ctx_start = prefix.saturating_sub(2);
    let mut diff = format!(
        "--- a/{label}\n+++ b/{label}\n@@ -{},{} +{},{} @@\n",
        ctx_start + 1,
        old_end.saturating_sub(ctx_start),
        ctx_start + 1,
        new_end.saturating_sub(ctx_start)
    );
    for line in &old_lines[ctx_start..prefix] {
        diff.push_str(&format!(" {line}\n"));
    }
    for line in &old_lines[prefix..old_end] {
        diff.push_str(&format!("-{line}\n"));
    }
    for line in &new_lines[prefix..new_end] {
        diff.push_str(&format!("+{line}\n"));
    }
    let suffix_end = (old_end + 2).min(old_lines.len());
    for line in &old_lines[old_end..suffix_end] {
        diff.push_str(&format!(" {line}\n"));
    }
    diff
}

fn check_write_noop_or_destructive_overwrite(
    component_root: &Path,
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Option<BrokerOutcome> {
    let destination = obj.get("destination").and_then(serde_json::Value::as_str)?;
    let content = obj.get("content").and_then(serde_json::Value::as_str)?;
    let (resolved, rel) = resolve_safe_read_path(component_root, destination, false).ok()?;
    if rel.ends_with(".rs") {
        let balance = rust_brace_balance(content);
        if balance != 0 {
            return Some(BrokerOutcome::Read(ReadRecord {
                call_id: intent.id.clone(),
                tool: "write_file".to_owned(),
                path: rel.clone(),
                content: format!(
                    "error: `write_file` on `{rel}` was not applied because it has unbalanced `{{` / `}}` delimiters (net brace balance = {balance:+}). Ensure all blocks and functions have matching braces and retry."
                ),
            }));
        }
    }
    let existing = fs::read_to_string(&resolved).ok()?;
    if existing.trim() == content.trim() {
        return Some(BrokerOutcome::Read(ReadRecord {
            call_id: intent.id.clone(),
            tool: "write_file".to_owned(),
            path: rel.clone(),
            content: format!(
                "`{rel}` already has that exact content (no write needed). Note: Kvist runs `cargo test` automatically in the verification sandbox after you finish. If your changes for this task are complete, reply with a plain-text summary and NO tool calls."
            ),
        }));
    }
    if resolved.starts_with(component_root.join("src"))
        && existing.len() >= 256
        && content.len().saturating_mul(2) < existing.len()
    {
        return Some(BrokerOutcome::Read(ReadRecord {
            call_id: intent.id.clone(),
            tool: "write_file".to_owned(),
            path: rel.clone(),
            content: format!(
                "refused destructive overwrite of existing module `{rel}` ({} bytes -> {} bytes). Do not replace existing `src/` modules with stubs; call `read_file` on `{rel}` and use `edit_file` for targeted changes. If your task changes are complete, reply with a plain-text summary and NO tool calls so Kvist can run `cargo test`.",
                existing.len(),
                content.len()
            ),
        }));
    }
    None
}

fn is_completion_report_proposal(decision: &ProposedDecision) -> bool {
    let summary_lower = decision.summary.to_ascii_lowercase();
    let why_lower = decision.why.to_ascii_lowercase();
    let patch_lower = decision
        .patch
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let completion_phrases = [
        "task has been completed",
        "tests have been added",
        "tests implemented",
        "implementation complete",
        "successfully added",
        "successfully implemented",
        "no additional manifest changes",
        "no manifest changes needed",
        "no changes needed",
        "all tests follow",
    ];
    let has_completion_phrase = completion_phrases.iter().any(|phrase| {
        summary_lower.contains(phrase) || why_lower.contains(phrase) || patch_lower.contains(phrase)
    });
    let has_alternatives = why_lower.contains("## alternatives")
        || why_lower.contains("option a")
        || why_lower.contains("option 1")
        || why_lower.contains("alternative 1");
    has_completion_phrase && !has_alternatives
}

/// True when a `propose_decision` call is a routine internal developer note,
/// code/test verification question, or status update rather than a genuine
/// architectural contradiction presenting alternatives for human arbitration.
pub fn is_routine_internal_proposal(decision: &ProposedDecision) -> bool {
    if decision.summary == "Proposed modification to component Cargo.toml"
        || decision.summary.starts_with("dependency `")
    {
        return false;
    }
    let summary_lower = decision.summary.to_ascii_lowercase();
    let why_lower = decision.why.to_ascii_lowercase();
    let patch_lower = decision
        .patch
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let has_alternatives = why_lower.contains("## alternatives")
        || why_lower.contains("option a")
        || why_lower.contains("option 1")
        || why_lower.contains("alternative 1");
    if has_alternatives {
        return false;
    }
    let internal_phrases = [
        "need to verify",
        "need to check",
        "i need to",
        "let me check",
        "check if",
        "verify public",
        "public api",
        "pub export",
        "publicly exported",
        "test file",
        "in src/",
        "in tests/",
        "check src/",
        "cargo test",
        "cargo check",
        "whether these are pub",
        "add pub exports",
        "compile",
    ];
    let has_internal_phrase = internal_phrases.iter().any(|phrase| {
        summary_lower.contains(phrase) || why_lower.contains(phrase) || patch_lower.contains(phrase)
    });
    let comment_only_patch = decision.patch.as_deref().is_some_and(|patch| {
        let trimmed = patch.trim();
        !trimmed.is_empty()
            && trimmed
                .lines()
                .filter(|line| !line.trim().is_empty())
                .all(|line| {
                    let t = line.trim_start();
                    t.starts_with("//") || t.starts_with('#')
                })
    });
    has_internal_phrase || comment_only_patch
}

fn check_edit_rust_brace_balance(
    component_root: &Path,
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Option<BrokerOutcome> {
    let destination = obj.get("destination").and_then(serde_json::Value::as_str)?;
    if !destination.trim().ends_with(".rs") {
        return None;
    }
    let replacement = obj.get("replace").and_then(serde_json::Value::as_str)?;
    let content = obj.get("content").and_then(serde_json::Value::as_str)?;
    let (resolved, rel) = resolve_safe_read_path(component_root, destination, false).ok()?;
    let existing = fs::read_to_string(&resolved).ok()?;
    if count_occurrences(existing.as_bytes(), replacement.as_bytes()) != 1 {
        return None;
    }
    let new_bytes = replace_single_occurrence(
        existing.as_bytes(),
        replacement.as_bytes(),
        content.as_bytes(),
    )?;
    let new_text = std::str::from_utf8(&new_bytes).ok()?;
    let old_balance = rust_brace_balance(&existing);
    let new_balance = rust_brace_balance(new_text);
    if old_balance == 0 && new_balance != 0 {
        return Some(BrokerOutcome::Read(ReadRecord {
            call_id: intent.id.clone(),
            tool: "edit_file".to_owned(),
            path: rel.clone(),
            content: format!(
                "error: `edit_file` on `{rel}` was not applied because it leaves unbalanced `{{` / `}}` delimiters (net brace balance = {new_balance:+}; did you omit the closing `}}` of `mod tests` or an enclosing block at the end of `content`?). Preserve balanced braces and retry `edit_file`."
            ),
        }));
    }
    if let Some((symbol, existing_line)) =
        find_duplicate_rust_top_level_symbol(&existing, replacement, content)
    {
        return Some(BrokerOutcome::Read(ReadRecord {
            call_id: intent.id.clone(),
            tool: "edit_file".to_owned(),
            path: rel.clone(),
            content: format!(
                "error: `edit_file` on `{rel}` was not applied because top-level item `{symbol}` is ALREADY defined in `{rel}` (around line {existing_line}). Do not add duplicate definitions; call `read_file` on `{rel}` with `start_line: {}` or `search: \"{symbol}\"` to inspect the existing implementation. If the required functionality is already in place, reply with a plain-text summary and NO tool calls.",
                existing_line.saturating_sub(10).max(1)
            ),
        }));
    }
    None
}

fn warn_extra_tool_arguments(tool: &str, obj: &serde_json::Map<String, serde_json::Value>) {
    let known: &[&str] = match tool {
        "read_file" => &["path", "start_line", "end_line", "search"],
        "list_files" => &["path"],
        "write_file" => &["destination", "content"],
        "edit_file" => &["destination", "replace", "content"],
        "propose_decision" => &["summary", "why", "patch"],
        "request_dependency" => &["name", "origin", "reason"],
        _ => &[],
    };
    for (key, value) in obj {
        if !known.contains(&key.as_str()) {
            let mut preview = value.to_string();
            if preview.len() > 160 {
                preview.truncate(preview.floor_char_boundary(160));
                preview.push_str("...");
            }
            tracing::warn!(
                tool,
                argument = %key,
                value_preview = %preview,
                "authoring tool call included an unrecognized argument field; ignoring extra field and continuing"
            );
        }
    }
}

fn extract_top_level_rust_symbol(line: &str) -> Option<String> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let mut rest = line.trim_start();
    if let Some(after_pub) = rest.strip_prefix("pub ") {
        rest = after_pub.trim_start();
    } else if let Some(after_pub_crate) = rest.strip_prefix("pub(crate) ") {
        rest = after_pub_crate.trim_start();
    } else if let Some(after_pub_super) = rest.strip_prefix("pub(super) ") {
        rest = after_pub_super.trim_start();
    }
    for kw in [
        "async fn ",
        "unsafe fn ",
        "const fn ",
        "fn ",
        "struct ",
        "enum ",
    ] {
        if let Some(after_kw) = rest.strip_prefix(kw) {
            let ident: String = after_kw
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                let kind = kw.split_whitespace().last().unwrap_or("item");
                return Some(format!("{kind} {ident}"));
            }
        }
    }
    None
}

fn find_duplicate_rust_top_level_symbol(
    existing: &str,
    replacement: &str,
    content: &str,
) -> Option<(String, usize)> {
    let replaced_symbols: std::collections::BTreeSet<String> = replacement
        .lines()
        .filter_map(extract_top_level_rust_symbol)
        .collect();
    let added_symbols: Vec<String> = content
        .lines()
        .filter_map(extract_top_level_rust_symbol)
        .filter(|sym| !replaced_symbols.contains(sym))
        .collect();
    if added_symbols.is_empty() {
        return None;
    }
    for (idx, line) in existing.lines().enumerate() {
        if let Some(sym) = extract_top_level_rust_symbol(line)
            && added_symbols.contains(&sym)
        {
            return Some((sym, idx + 1));
        }
    }
    None
}

fn rust_brace_balance(source: &str) -> i64 {
    let mut balance = 0i64;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let mut in_string = false;
        let mut escaped = false;
        for ch in trimmed.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            match ch {
                '"' => in_string = true,
                '{' => balance += 1,
                '}' => balance -= 1,
                _ => {}
            }
        }
    }
    balance
}

fn resolve_safe_read_path(
    component_root: &Path,
    raw_path: &str,
    allow_root: bool,
) -> Result<(PathBuf, String), String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("path must be non-empty".to_owned());
    }
    if Path::new(trimmed).is_absolute() {
        return Err(format!(
            "path `{trimmed}` must be relative to the component"
        ));
    }
    let mut resolved = PathBuf::from(component_root);
    let mut rel_parts = Vec::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                let name = part
                    .to_str()
                    .ok_or_else(|| "path must be valid UTF-8".to_owned())?;
                if EXCLUDED_READ_DIRS.contains(&name) {
                    return Err(format!(
                        "path `{trimmed}` accesses excluded directory `{name}`"
                    ));
                }
                resolved.push(part);
                rel_parts.push(name.to_owned());
                if rel_parts.len() > MAX_DESTINATION_DEPTH {
                    return Err(format!(
                        "path `{trimmed}` exceeds the {MAX_DESTINATION_DEPTH}-depth bound"
                    ));
                }
                if let Ok(metadata) = fs::symlink_metadata(&resolved)
                    && metadata.file_type().is_symlink()
                {
                    return Err(format!("path `{trimmed}` traverses a symbolic link"));
                }
            }
            Component::ParentDir => {
                return Err(format!("path `{trimmed}` must not contain `..`"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "path `{trimmed}` must be relative to the component"
                ));
            }
        }
    }
    if rel_parts.is_empty() && !allow_root {
        return Err("path must name a file inside the component".to_owned());
    }
    let rel = if rel_parts.is_empty() {
        ".".to_owned()
    } else {
        rel_parts.join("/")
    };
    Ok((resolved, rel))
}

fn authorize_read_file(
    component_root: &Path,
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<ReadRecord, String> {
    let raw_path = string_arg(obj, "path", false)?;
    let (resolved, rel) = resolve_safe_read_path(component_root, &raw_path, false)?;
    let start_line = obj
        .get("start_line")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let end_line = obj
        .get("end_line")
        .and_then(serde_json::Value::as_u64)
        .map(|line| (line as usize).max(start_line))
        .unwrap_or_else(|| start_line.saturating_add(DEFAULT_READ_LINES - 1));

    let metadata = match fs::symlink_metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ReadRecord {
                call_id: intent.id.clone(),
                tool: "read_file".to_owned(),
                path: rel.clone(),
                content: format!(
                    "error: file `{rel}` does not exist; call `list_files` to see existing files."
                ),
            });
        }
        Err(error) => {
            return Err(format!("cannot inspect `{rel}`: {error}"));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(format!("path `{rel}` is a symbolic link"));
    }
    if !metadata.is_file() {
        return Ok(ReadRecord {
            call_id: intent.id.clone(),
            tool: "read_file".to_owned(),
            path: rel.clone(),
            content: format!(
                "error: `{rel}` is a directory, not a regular file; use `list_files`."
            ),
        });
    }
    if metadata.len() > MAX_CONTENT_BYTES as u64 {
        return Err(format!(
            "file `{rel}` exceeds the {MAX_CONTENT_BYTES}-byte read bound"
        ));
    }
    let text = fs::read_to_string(&resolved)
        .map_err(|error| format!("file `{rel}` could not be read as UTF-8: {error}"))?;
    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len();
    if total_lines == 0 {
        return Ok(ReadRecord {
            call_id: intent.id.clone(),
            tool: "read_file".to_owned(),
            path: rel.clone(),
            content: format!("File: {rel} (empty, 0 lines)\n"),
        });
    }
    if let Some(query) = obj
        .get("search")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        let mut match_lines = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            if line.contains(query) {
                match_lines.push(idx + 1);
            }
        }
        if match_lines.is_empty() {
            return Ok(ReadRecord {
                call_id: intent.id.clone(),
                tool: "read_file".to_owned(),
                path: rel.clone(),
                content: format!(
                    "File: {rel} ({total_lines} lines) — no occurrences of `{query}` found.\n"
                ),
            });
        }
        let mut out = format!(
            "File: {rel} ({total_lines} lines) — found {} match(es) for `{query}` at line(s) {:?}:\n",
            match_lines.len(),
            match_lines
        );
        let mut printed_up_to = 0usize;
        for &m in &match_lines {
            let ctx_from = m.saturating_sub(8).max(1).max(printed_up_to + 1);
            let ctx_to = (m + 12).min(total_lines);
            if ctx_from > printed_up_to + 1 && printed_up_to > 0 {
                out.push_str("   ...\n");
            }
            for line_no in ctx_from..=ctx_to {
                let formatted = format!("{:>6}\t{}\n", line_no, lines[line_no - 1]);
                if out.len().saturating_add(formatted.len()) > MAX_READ_SLICE_BYTES {
                    break;
                }
                out.push_str(&formatted);
                printed_up_to = line_no;
            }
        }
        return Ok(ReadRecord {
            call_id: intent.id.clone(),
            tool: "read_file".to_owned(),
            path: rel,
            content: out,
        });
    }
    let from = start_line.min(total_lines);
    let to = end_line.min(total_lines);
    let mut output = String::new();
    let mut last_included = from;
    for (idx, line) in lines.iter().enumerate().skip(from - 1).take(to - from + 1) {
        let formatted = format!("{:>6}\t{line}\n", idx + 1);
        if output.len().saturating_add(formatted.len()) > MAX_READ_SLICE_BYTES {
            break;
        }
        output.push_str(&formatted);
        last_included = idx + 1;
    }
    let mut header =
        format!("File: {rel} (showing lines {from}..={last_included} of {total_lines})");
    if last_included < total_lines {
        header.push_str(&format!(
            " — call read_file with start_line={} to read more",
            last_included + 1
        ));
    }
    header.push('\n');
    header.push_str(&output);
    Ok(ReadRecord {
        call_id: intent.id.clone(),
        tool: "read_file".to_owned(),
        path: rel,
        content: header,
    })
}

fn authorize_list_files(
    component_root: &Path,
    intent: &ToolIntent,
    obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<ReadRecord, String> {
    let raw_path = obj
        .get("path")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".");
    let (resolved, rel) = resolve_safe_read_path(component_root, raw_path, true)?;
    let metadata = match fs::symlink_metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ReadRecord {
                call_id: intent.id.clone(),
                tool: "list_files".to_owned(),
                path: rel.clone(),
                content: format!("error: directory `{rel}` does not exist."),
            });
        }
        Err(error) => return Err(format!("cannot inspect `{rel}`: {error}")),
    };
    if metadata.file_type().is_symlink() {
        return Err(format!("path `{rel}` is a symbolic link"));
    }
    if !metadata.is_dir() {
        return Ok(ReadRecord {
            call_id: intent.id.clone(),
            tool: "list_files".to_owned(),
            path: rel.clone(),
            content: format!(
                "`{rel}` is a file ({} bytes); use `read_file` to read it.",
                metadata.len()
            ),
        });
    }
    let mut entries = Vec::new();
    collect_directory_entries(component_root, &resolved, 0, &mut entries)?;
    let mut output = format!(
        "Directory listing for `{rel}` ({} entries):\n",
        entries.len()
    );
    for entry in entries {
        output.push_str(&entry);
        output.push('\n');
    }
    Ok(ReadRecord {
        call_id: intent.id.clone(),
        tool: "list_files".to_owned(),
        path: rel,
        content: output,
    })
}

fn collect_directory_entries(
    component_root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<String>,
) -> Result<(), String> {
    if depth >= MAX_LIST_DEPTH || out.len() >= MAX_LIST_ENTRIES {
        return Ok(());
    }
    let read_dir = fs::read_dir(dir).map_err(|error| format!("cannot read directory: {error}"))?;
    let mut children: Vec<PathBuf> = read_dir
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    children.sort();
    for child in children {
        if out.len() >= MAX_LIST_ENTRIES {
            break;
        }
        let Some(name) = child.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if EXCLUDED_READ_DIRS.contains(&name) {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&child) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        let rel = normalized_relative(component_root, &child);
        if metadata.is_dir() {
            out.push(format!("{rel}/"));
            collect_directory_entries(component_root, &child, depth + 1, out)?;
        } else if metadata.is_file() {
            out.push(format!("{rel} ({} bytes)", metadata.len()));
        }
    }
    Ok(())
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
        assert!(names.contains(&"read_file".to_owned()));
        assert!(names.contains(&"list_files".to_owned()));
        assert!(names.contains(&"write_file".to_owned()));
        assert!(names.contains(&"edit_file".to_owned()));
        assert!(names.contains(&"propose_decision".to_owned()));
        assert!(names.contains(&"request_dependency".to_owned()));
    }

    #[test]
    fn read_file_and_list_files_inspect_component_and_exclude_protected_state_dirs() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join(".kvist")).unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"galla\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "pub mod cache;\n").unwrap();
        std::fs::write(dir.path().join(".kvist/secret"), "hidden").unwrap();

        let list = intent("list_files", serde_json::json!({}));
        let read_cargo = intent(
            "read_file",
            serde_json::json!({ "path": "Cargo.toml", "start_line": 1, "end_line": 10 }),
        );
        let read_secret = intent("read_file", serde_json::json!({ "path": ".kvist/secret" }));
        let plan = authorize_intents(dir.path(), &[list, read_cargo, read_secret], &policy());
        assert_eq!(plan.reads.len(), 2);
        assert!(plan.reads[0].content.contains("Cargo.toml"));
        assert!(plan.reads[0].content.contains("src/lib.rs"));
        assert!(!plan.reads[0].content.contains(".kvist"));
        assert!(plan.reads[1].content.contains("name = \"galla\""));
        assert_eq!(plan.dropped_count(), 1);
        assert!(plan.dropped[0].reason.contains("excluded directory"));
    }

    #[test]
    fn cargo_toml_write_or_edit_surfaces_a_decision_instead_of_dropping() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"galla\"\nversion = \"0.3.0\"\n",
        )
        .unwrap();

        let noop_cargo = write_intent(
            "Cargo.toml",
            "[package]\nname = \"galla\"\nversion = \"0.3.0\"\n",
        );
        let noop_plan = authorize_intents(dir.path(), &[noop_cargo], &policy());
        assert_eq!(noop_plan.decision_count(), 0);
        assert_eq!(noop_plan.reads.len(), 1);
        assert!(
            noop_plan.reads[0]
                .content
                .contains("already has that exact content")
        );

        let changed_cargo = write_intent(
            "Cargo.toml",
            "[package]\nname = \"galla\"\nversion = \"0.4.0\"\n",
        );
        let plan = authorize_intents(dir.path(), &[changed_cargo], &policy());
        assert_eq!(plan.decision_count(), 1);
        assert_eq!(plan.dropped_count(), 0);
        assert!(plan.effects.is_empty());
        assert!(plan.decisions[0].summary.contains("Cargo.toml"));
        let patch = plan.decisions[0].patch.as_deref().expect("diff patch");
        assert!(patch.contains("--- a/Cargo.toml"));
        assert!(patch.contains("-version = \"0.3.0\""));
        assert!(patch.contains("+version = \"0.4.0\""));

        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/validation.rs"), "x".repeat(512)).unwrap();
        let stub_overwrite = write_intent("src/validation.rs", "fn stub() {}");
        let guard_plan = authorize_intents(dir.path(), &[stub_overwrite], &policy());
        assert!(guard_plan.effects.is_empty());
        assert_eq!(guard_plan.reads.len(), 1);
        assert!(
            guard_plan.reads[0]
                .content
                .contains("refused destructive overwrite")
        );

        std::fs::write(
            dir.path().join("src/cache.rs"),
            "mod tests {\n    #[test]\n    fn a() {}\n}\n",
        )
        .unwrap();
        let unbalanced_edit = edit_intent(
            "src/cache.rs",
            "    #[test]\n    fn a() {}\n}",
            "    #[test]\n    fn a() {}\n    #[test]\n    fn b() {}",
        );
        let brace_plan = authorize_intents(dir.path(), &[unbalanced_edit], &policy());
        assert!(brace_plan.effects.is_empty());
        assert_eq!(brace_plan.reads.len(), 1);
        assert!(brace_plan.reads[0].content.contains("unbalanced `{` / `}`"));

        let completion_turn = ModelTurn {
            text: "The tests have been successfully added.".into(),
            reasoning: None,
            tool_intents: vec![propose_intent_raw(serde_json::json!({
                "summary": "Cache promotion atomicity and checksum verification tests implemented",
                "why": "The task has been completed with comprehensive tests added to src/cache.rs.",
                "patch": "No additional manifest changes needed - tests use existing tempfile dependency"
            }))],
            finish_reason: sav::FinishReason::ToolCalls,
            provider: sav::LocalModelProvider::LlamaServer,
            model: "fixture".into(),
            response_id: None,
            provider_request_id: None,
            usage: None,
        };
        let turn_plan = authorize_turn(dir.path(), &completion_turn, &policy());
        assert_eq!(turn_plan.decision_count(), 0);
        assert_eq!(turn_plan.reads.len(), 1);
        assert_eq!(turn_plan.reads[0].path, "completion-note");

        let internal_note_turn = ModelTurn {
            text: String::new(),
            reasoning: None,
            tool_intents: vec![propose_intent_raw(serde_json::json!({
                "summary": "Need to verify public API exports in src/cache.rs for test file",
                "why": "The test file I created uses types like PromotionPlan, promote, inspect_source which need to be publicly exported from the cache module. I need to check if these are pub or if I need to add pub exports.",
                "patch": "// Check src/cache.rs for pub exports of:\n// - PromotionPlan\n// - promote function"
            }))],
            finish_reason: sav::FinishReason::ToolCalls,
            provider: sav::LocalModelProvider::LlamaServer,
            model: "fixture".into(),
            response_id: None,
            provider_request_id: None,
            usage: None,
        };
        let internal_plan = authorize_turn(dir.path(), &internal_note_turn, &policy());
        assert_eq!(internal_plan.decision_count(), 0);
        assert_eq!(internal_plan.reads.len(), 1);
        assert_eq!(internal_plan.reads[0].path, "auto-resolved-decision");

        std::fs::write(
            dir.path().join("src/cache.rs"),
            "pub struct Staging;\n\npub fn promote() {}\n",
        )
        .unwrap();
        let dup_edit = edit_intent(
            "src/cache.rs",
            "pub struct Staging;",
            "pub struct Staging;\npub fn promote() {}",
        );
        let dup_plan = authorize_intents(dir.path(), &[dup_edit], &policy());
        assert!(dup_plan.effects.is_empty());
        assert_eq!(dup_plan.reads.len(), 1);
        assert!(dup_plan.reads[0].content.contains("ALREADY defined"));

        let search_read = intent(
            "read_file",
            serde_json::json!({ "path": "src/cache.rs", "search": "fn promote" }),
        );
        let search_plan = authorize_intents(dir.path(), &[search_read], &policy());
        assert_eq!(search_plan.reads.len(), 1);
        assert!(search_plan.reads[0].content.contains("found 1 match(es)"));

        let malformed_arg_call = intent(
            "edit_file",
            serde_json::json!({
                "_malformed_arguments_error": "EOF while parsing a string",
                "_malformed_arguments_preview": "{\"destination\":\"src/cache.rs\""
            }),
        );
        let malformed_plan = authorize_intents(dir.path(), &[malformed_arg_call], &policy());
        assert_eq!(malformed_plan.dropped_count(), 0);
        assert_eq!(malformed_plan.reads.len(), 1);
        assert_eq!(malformed_plan.reads[0].path, "malformed-tool-arguments");
    }
}
