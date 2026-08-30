use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;

/// Local HTTP provider protocols supported by the direct transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalModelProvider {
    /// Ollama's native `/api/chat` protocol.
    Ollama,
    /// llama-server's OpenAI-compatible `/v1/chat/completions` protocol.
    LlamaServer,
}

impl LocalModelProvider {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::LlamaServer => "llama-server",
        }
    }
}

/// Ordered canonical conversation messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelMessage {
    /// Instructions supplied by the embedding host.
    System(String),
    /// User or task input.
    User(String),
    /// A prior model turn, including any untrusted tool intents it proposed.
    Assistant {
        /// Text returned with the turn.
        text: String,
        /// Tool intents returned with the turn.
        tool_intents: Vec<ToolIntent>,
    },
    /// A host-mediated tool result returned to the model.
    ToolResult {
        /// Canonical call identity being answered.
        call_id: String,
        /// Stable tool name.
        name: String,
        /// Bounded redacted result content.
        content: String,
    },
}

/// A model-facing tool descriptor. It contains no executable implementation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Stable model-facing tool name.
    pub name: String,
    /// Bounded human-readable purpose.
    pub description: String,
    /// JSON object schema for arguments.
    pub parameters: Value,
}

/// Tool selection requested from a model transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolChoice {
    /// Do not expose tools for this turn.
    None,
    /// Let the model decide whether to propose a tool call.
    Auto,
    /// Require a tool call when the provider supports that constraint.
    Required,
}

/// Provider-neutral input for one model turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    /// Provider model selector.
    pub model: String,
    /// Ordered conversation history.
    pub messages: Vec<ModelMessage>,
    /// Model-facing tools available for proposal.
    pub tools: Vec<ToolDefinition>,
    /// Requested tool selection behavior.
    pub tool_choice: ToolChoice,
}

/// A complete untrusted tool proposal returned by a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolIntent {
    /// Canonical turn-local call identity.
    pub id: String,
    /// Provider call identity when one was supplied.
    pub provider_id: Option<String>,
    /// Model-selected tool name.
    pub name: String,
    /// Parsed JSON object arguments.
    pub arguments: Value,
}

/// Normalized reason that a model turn ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FinishReason {
    /// Natural or explicit stop.
    Stop,
    /// Provider output limit.
    Length,
    /// One or more tool intents were returned.
    ToolCalls,
    /// Provider content policy stopped output.
    ContentFilter,
    /// Provider-specific terminal reason retained in bounded form.
    Other(String),
}

/// Normalized token accounting when supplied by the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelUsage {
    /// Tokens consumed by input.
    pub input_tokens: u64,
    /// Tokens generated as output.
    pub output_tokens: u64,
    /// Provider total, or the checked sum when only the parts are supplied.
    pub total_tokens: u64,
}

/// Complete provider-neutral result for one model turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTurn {
    /// Ordered text content.
    pub text: String,
    /// Complete untrusted tool proposals.
    pub tool_intents: Vec<ToolIntent>,
    /// Normalized terminal reason.
    pub finish_reason: FinishReason,
    /// Provider kind used for the turn.
    pub provider: LocalModelProvider,
    /// Model identity reported by the provider or requested by the host.
    pub model: String,
    /// Provider response identity when supplied in the response body.
    #[serde(default)]
    pub response_id: Option<String>,
    /// Provider request identity when supplied.
    pub provider_request_id: Option<String>,
    /// Token accounting when supplied.
    pub usage: Option<ModelUsage>,
}

/// Ordered events emitted while decoding a streaming model turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "kebab-case")]
pub enum ModelStreamEvent {
    /// A text fragment in provider order.
    TextDelta(String),
    /// A complete tool intent after all argument fragments validate.
    ToolIntent(ToolIntent),
}

/// Cloneable cooperative cancellation source.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Creates a token in the active state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Reports whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Provider-neutral model transport boundary.
pub trait ModelTransport {
    /// Performs one non-streaming model turn.
    fn complete(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
    ) -> Result<ModelTurn>;

    /// Performs one streaming model turn and returns its assembled terminal value.
    fn stream(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>,
    ) -> Result<ModelTurn>;

    /// Overall deadline applied to each transport call.
    fn deadline(&self) -> Duration;
}
