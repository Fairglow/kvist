//! Provider-independent runtime contract types.
//!
//! These types form the stable seam between the standalone runtime and any
//! embedding host. They carry no Kvist, Rig, or provider-SDK type and confer
//! no execution, credential, resource, promotion, or compliance authority.
//! The host owns authorization, effect execution, artifact promotion, and
//! canonical compliance evidence; the runtime owns only the protocol shapes
//! that both sides must agree on.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Provider-independent classification of the backend a run dispatches to.
///
/// The runtime must reason about *which kind* of backend it is driving without
/// naming a specific provider, SDK, or framework. This closed classification is
/// the only backend identity the protocol types carry: a host broker or a
/// direct transport adapter both present as one of these two classes, and no
/// contract type below reaches into a provider library to learn more.
///
/// The classification confers no execution authority. It tells the loop how to
/// shape a turn and how to interpret a result; the host still decides whether
/// any proposed effect is allowed to run.
///
/// ```rust
/// use sav::runtime::BackendClass;
///
/// // The native model loop mediates every intent; an external agent is
/// // opaque and only observed by its outer execution backend.
/// assert!(BackendClass::NativeModel.mediates_tool_intents());
/// assert!(!BackendClass::ExternalAgent.mediates_tool_intents());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendClass {
    /// A provider model reached through a model transport that returns
    /// canonical turns and untrusted tool intents. The loop mediates every
    /// intent through the host broker before any effect.
    NativeModel,
    /// An opaque external agent process constrained by an outer execution
    /// backend. Its output is observed, not mediated, and it never becomes a
    /// native transport or gains tool authority on its own.
    ExternalAgent,
}

impl BackendClass {
    /// Whether the loop mediates the backend's tool intents through the host
    /// broker before effects.
    ///
    /// [`BackendClass::NativeModel`] intents are mediated; an
    /// [`BackendClass::ExternalAgent`] is opaque and its effects are only
    /// observed by the outer backend.
    pub const fn mediates_tool_intents(self) -> bool {
        matches!(self, Self::NativeModel)
    }
}

/// The verification tier of a declared capability.
///
/// A capability moves monotonically from *advertised* (the provider claims it)
/// to *tested* (a conformance probe confirmed it) to *policy-enabled* (the
/// host has explicitly turned it on). An unknown capability must never be
/// enabled; absence of a descriptor is the safe default.
///
/// ```rust
/// use sav::runtime::CapabilityState;
///
/// // Only an explicit host policy decision enables a capability.
/// assert!(!CapabilityState::Advertised.enabled());
/// assert!(!CapabilityState::Tested.enabled());
/// assert!(CapabilityState::PolicyEnabled.enabled());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    /// The provider advertises the capability but no conformance probe has
    /// confirmed it.
    Advertised,
    /// A bounded conformance probe confirmed the capability.
    Tested,
    /// The host policy has explicitly enabled the capability.
    PolicyEnabled,
}

impl CapabilityState {
    /// Whether this state permits the capability to be used by the runtime.
    ///
    /// Only [`PolicyEnabled`] grants use; [`Advertised`] and [`Tested`] are
    /// evidence tiers that the host may consult before enabling.
    pub const fn enabled(self) -> bool {
        matches!(self, Self::PolicyEnabled)
    }

    /// The next tier in the monotonic promotion order, if any.
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Advertised => Some(Self::Tested),
            Self::Tested => Some(Self::PolicyEnabled),
            Self::PolicyEnabled => None,
        }
    }
}

/// A bounded descriptor for one declared capability and its current tier.
///
/// The descriptor is pure data: it names a capability and records the highest
/// tier reached. It carries no executable implementation, no credential, and
/// no resource grant. The host decides whether to act on it.
///
/// ```rust
/// use sav::runtime::{CapabilityDescriptor, CapabilityState};
///
/// // A freshly advertised capability is not yet usable; only an explicit
/// // host policy decision enables it.
/// let descriptor = CapabilityDescriptor::advertised("tools");
/// assert!(!descriptor.is_enabled());
///
/// let enabled = CapabilityDescriptor {
///     name: "streaming".into(),
///     state: CapabilityState::PolicyEnabled,
/// };
/// assert!(enabled.is_enabled());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    /// Stable snake-case capability identifier (for example `tools`,
    /// `streaming`, `reasoning`).
    pub name: String,
    /// The highest verification tier reached for this capability.
    pub state: CapabilityState,
}

impl CapabilityDescriptor {
    /// Creates a descriptor in the [`CapabilityState::Advertised`] tier.
    pub fn advertised(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            state: CapabilityState::Advertised,
        }
    }

    /// Whether the host has policy-enabled this capability.
    pub fn is_enabled(self) -> bool {
        self.state.enabled()
    }
}

/// A bounded, typed provider extension.
///
/// Providers may attach structured, provider-specific data to a turn or
/// request. The extension is *typed* by its stable `name` and *bounded* by a
/// fixed payload ceiling so an untrusted provider cannot flood the runtime.
/// The payload is opaque JSON; the runtime never interprets it. Only the host
/// may decode an extension it recognizes, and decoding one never authorizes an
/// effect.
///
/// ```rust
/// use sav::runtime::ProviderExtension;
/// use serde_json::json;
///
/// // A stable snake-case name with a bounded payload is accepted.
/// let extension = ProviderExtension::bounded("reasoning", json!("low")).unwrap();
/// assert_eq!(extension.name, "reasoning");
///
/// // An unstable name is rejected rather than silently normalized.
/// assert!(ProviderExtension::bounded("Bad-Name", json!(null)).is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderExtension {
    /// Stable snake-case extension identifier.
    pub name: String,
    /// Bounded opaque provider payload.
    pub payload: Value,
}

/// The maximum accepted serialized size of one provider extension payload.
pub const MAX_EXTENSION_PAYLOAD_BYTES: usize = 64 * 1024;

impl ProviderExtension {
    /// Validates that the extension name is a stable identifier and the
    /// payload is bounded. Returns `None` when the extension exceeds its
    /// bound or carries an empty name.
    pub fn bounded(name: impl Into<String>, payload: Value) -> Option<Self> {
        let name = name.into();
        if name.is_empty()
            || name.len() > 128
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return None;
        }
        let bytes = serde_json::to_vec(&payload).ok()?;
        if bytes.len() > MAX_EXTENSION_PAYLOAD_BYTES {
            return None;
        }
        Some(Self { name, payload })
    }
}

/// A redacted runtime event safe to hand to a host recorder.
///
/// The session [`super::Event`] carries untrusted model text, reasoning, and
/// tool descriptions that a host may not want in durable evidence.
/// [`RedactedEvent`] retains only the structural facts of a run (which turn,
/// which tool, whether it failed, how the prompt ended) and drops every
/// free-text payload. Converting an event to its redacted form is a pure,
/// lossy projection: no model output, reasoning, or tool argument survives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RedactedEvent {
    /// A model turn began.
    TurnStart {
        /// Host-facing model identity.
        model: String,
    },
    /// A fresh model attempt started.
    AttemptStart {
        /// Attempt ordinal within the turn.
        attempt: u32,
    },
    /// A tool call was proposed by the model.
    ToolCall {
        /// Stable tool name.
        name: String,
    },
    /// A tool call finished.
    ToolResult {
        /// Stable tool name.
        name: String,
        /// Whether the tool result was a failure.
        failed: bool,
    },
    /// The session produced a final answer.
    Finished,
    /// The prompt loop has exited and control returned to the caller.
    PromptEnd {
        /// A budget cap cut the run short.
        exhausted: bool,
        /// Cancellation stopped the run.
        cancelled: bool,
    },
    /// The session failed before producing an answer.
    Failed,
    /// A non-terminal notice (for example a context compaction).
    Note,
}

/// Projects a session event into its redacted, recorder-safe form.
///
/// Every free-text payload (reasoning, answer text, tool descriptions,
/// failure messages) is dropped. Only the structural identifiers and boolean
/// outcomes remain, so a host journal can record *that* a tool ran and *that*
/// it failed without retaining *what* the model said.
///
/// ```rust
/// use sav::runtime::{Event, RedactedEvent, redact_event};
///
/// // The tool description is untrusted free text and does not survive.
/// let event = Event::ToolResult {
///     description: "secret description".into(),
///     name: "workspace.read".into(),
///     failed: true,
/// };
/// assert_eq!(
///     redact_event(&event),
///     RedactedEvent::ToolResult {
///         name: "workspace.read".into(),
///         failed: true,
///     }
/// );
/// ```
pub fn redact_event(event: &super::Event) -> RedactedEvent {
    use super::Event;
    match event {
        Event::TurnStart { model } => RedactedEvent::TurnStart {
            model: model.clone(),
        },
        Event::AttemptStart { attempt } => RedactedEvent::AttemptStart { attempt: *attempt },
        Event::ToolCall { name, .. } => RedactedEvent::ToolCall { name: name.clone() },
        Event::ToolResult { name, failed, .. } => RedactedEvent::ToolResult {
            name: name.clone(),
            failed: *failed,
        },
        Event::Finished { .. } => RedactedEvent::Finished,
        Event::PromptEnd {
            exhausted,
            cancelled,
            ..
        } => RedactedEvent::PromptEnd {
            exhausted: *exhausted,
            cancelled: *cancelled,
        },
        Event::Failed(_) => RedactedEvent::Failed,
        Event::Note(_) => RedactedEvent::Note,
        // Control, reasoning, text, tool-failure, and progress events carry
        // untrusted free text or live accounting; they redact to a note so
        // the structural record stays honest without leaking payloads.
        Event::Control(_)
        | Event::Reasoning(_)
        | Event::Text(_)
        | Event::ToolFailure { .. }
        | Event::Progress { .. } => RedactedEvent::Note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Event;

    #[test]
    fn backend_class_is_provider_independent_and_closed() {
        // Only the two closed classes exist; neither names a provider, SDK,
        // or transport framework.
        assert!(BackendClass::NativeModel.mediates_tool_intents());
        assert!(!BackendClass::ExternalAgent.mediates_tool_intents());

        // The classification is stable serialized data, not an executable
        // handle: it round-trips through its wire form unchanged.
        let wire = serde_json::to_string(&BackendClass::ExternalAgent).unwrap();
        assert_eq!(wire, "\"external_agent\"");
        let parsed: BackendClass = serde_json::from_str(&wire).unwrap();
        assert_eq!(parsed, BackendClass::ExternalAgent);
    }

    #[test]
    fn capability_state_promotes_monotonically_and_only_policy_enables() {
        assert!(!CapabilityState::Advertised.enabled());
        assert!(!CapabilityState::Tested.enabled());
        assert!(CapabilityState::PolicyEnabled.enabled());
        assert_eq!(
            CapabilityState::Advertised.next(),
            Some(CapabilityState::Tested)
        );
        assert_eq!(
            CapabilityState::Tested.next(),
            Some(CapabilityState::PolicyEnabled)
        );
        assert_eq!(CapabilityState::PolicyEnabled.next(), None);
    }

    #[test]
    fn capability_descriptor_starts_advertised_and_reports_enabled() {
        let descriptor = CapabilityDescriptor::advertised("tools");
        assert_eq!(descriptor.name, "tools");
        assert_eq!(descriptor.state, CapabilityState::Advertised);
        assert!(!descriptor.is_enabled());

        let enabled = CapabilityDescriptor {
            name: "streaming".into(),
            state: CapabilityState::PolicyEnabled,
        };
        assert!(enabled.is_enabled());
    }

    #[test]
    fn provider_extension_is_typed_and_bounded() {
        let extension = ProviderExtension::bounded("reasoning", Value::String("low".into()));
        assert!(extension.is_some());
        let extension = extension.unwrap();
        assert_eq!(extension.name, "reasoning");

        // An empty name is rejected.
        assert!(ProviderExtension::bounded("", Value::Null).is_none());
        // A name with uppercase or a dash is rejected.
        assert!(ProviderExtension::bounded("Bad-Name", Value::Null).is_none());
        // An oversized payload is rejected.
        let huge = Value::String("x".repeat(MAX_EXTENSION_PAYLOAD_BYTES + 1));
        assert!(ProviderExtension::bounded("big", huge).is_none());
    }

    #[test]
    fn redaction_drops_free_text_and_keeps_structure() {
        let event = Event::ToolResult {
            description: "secret tool description".into(),
            name: "workspace.read".into(),
            failed: true,
        };
        let redacted = redact_event(&event);
        assert_eq!(
            redacted,
            RedactedEvent::ToolResult {
                name: "workspace.read".into(),
                failed: true,
            }
        );
        // The serialized redacted form must not contain the dropped text.
        let json = serde_json::to_string(&redacted).unwrap();
        assert!(!json.contains("secret"));

        let reasoning = Event::Reasoning("hidden chain of thought".into());
        assert_eq!(redact_event(&reasoning), RedactedEvent::Note);

        let prompt_end = Event::PromptEnd {
            exhausted: true,
            cancelled: false,
            reason: crate::runtime::StopReason::Completed,
        };
        assert_eq!(
            redact_event(&prompt_end),
            RedactedEvent::PromptEnd {
                exhausted: true,
                cancelled: false,
            }
        );
    }
}
