use std::{
    collections::{HashMap, HashSet},
    error::Error as StdError,
    fmt,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    str::FromStr,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures::{Stream, StreamExt};
use rig_core::http_client::sse::BoxedStream;
use rig_core::http_client::{
    self as rig_http, HttpClientExt, LazyBody, MultipartForm, Request as RigHttpRequest,
    Response as RigHttpResponse, StreamingResponse,
};
use rig_core::{
    client::{CompletionClient, Nothing},
    completion::{
        CompletionError, CompletionModel as RigCompletionModel,
        CompletionRequest as RigCompletionRequest, CompletionResponse as RigCompletionResponse,
        FinishReason as RigFinishReason, ToolDefinition as RigToolDefinition,
    },
    message::{
        AssistantContent, Message as RigMessage, ProviderCallId, ToolCall as RigToolCall,
        ToolCallId, ToolFunction, ToolResultContent, UserContent,
    },
    providers::{llamafile, ollama},
    streaming::{StreamFinal, StreamedAssistantContent},
};

use crate::{
    CancellationToken, Error, FinishReason, LocalModelProvider, ModelMessage, ModelRequest,
    ModelStreamEvent, ModelTransport, ModelTurn, ModelUsage, Result, ToolChoice, ToolIntent,
    direct_transport::validate_request,
};

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MALFORMED_OLLAMA_MARKER: &str = "kvist malformed Ollama response";
const REQUEST_LIMIT_MARKER: &str = "kvist Rig request limit exceeded";
const RESPONSE_LIMIT_MARKER: &str = "kvist Rig response limit exceeded";

/// Optional Rig-backed local model transport.
///
/// Rig performs provider protocol conversion only. Tool calls are returned as
/// untrusted [`ToolIntent`] values and are never registered as executable Rig
/// tools.
#[derive(Clone)]
pub struct RigModelTransport {
    provider: LocalModelProvider,
    endpoint: String,
    deadline: Duration,
    max_response_bytes: usize,
    http_client: BoundedHttpClient,
}

impl fmt::Debug for RigModelTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RigModelTransport")
            .field("provider", &self.provider)
            .field("deadline", &self.deadline)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

impl RigModelTransport {
    /// Creates a Rig-backed transport for one numeric loopback endpoint.
    pub fn new(
        provider: LocalModelProvider,
        endpoint: &str,
        deadline: Duration,
        max_response_bytes: usize,
    ) -> Result<Self> {
        let endpoint = validate_endpoint(endpoint)?;
        if deadline.is_zero() || deadline > Duration::from_secs(24 * 60 * 60) {
            return invalid_transport("deadline must be between 1 nanosecond and 24 hours");
        }
        if max_response_bytes == 0 || max_response_bytes > MAX_RESPONSE_BYTES {
            return invalid_transport("response limit must be between 1 byte and 16777216 bytes");
        }

        let http_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| Error::InvalidModelTransport {
                reason: "cannot initialize the restricted Rig HTTP client".to_owned(),
            })?;

        Ok(Self {
            provider,
            endpoint,
            deadline,
            max_response_bytes,
            http_client: BoundedHttpClient::new(http_client, max_response_bytes),
        })
    }

    fn run_with_limits<T>(
        &self,
        cancellation: &CancellationToken,
        future: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        if cancellation.is_cancelled() {
            return Err(Error::ModelTransportCancelled);
        }
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(Error::ModelTransportFramework {
                operation: "running synchronous transport inside an async runtime",
            });
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| Error::ModelTransportFramework {
                operation: "initializing the async runtime",
            })?;

        let dispatch = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
        tracing::dispatcher::with_default(&dispatch, || {
            runtime.block_on(async {
                tokio::select! {
                    _ = wait_for_cancellation(cancellation) => Err(Error::ModelTransportCancelled),
                    result = tokio::time::timeout(self.deadline, future) => {
                        result.map_err(|_| Error::ModelTransportTimedOut)?
                    }
                }
            })
        })
    }

    async fn complete_async(&self, request: &ModelRequest) -> Result<ModelTurn> {
        let rig_request = convert_request(self.provider, request)?;
        let response = match self.provider {
            LocalModelProvider::Ollama => {
                let client = ollama::Client::builder()
                    .api_key(Nothing)
                    .base_url(&self.endpoint)
                    .http_client(self.http_client.clone())
                    .build()
                    .map_err(|_| Error::InvalidModelTransport {
                        reason: "cannot configure the Rig Ollama client".to_owned(),
                    })?;
                client
                    .completion_model(&request.model)
                    .completion(rig_request)
                    .await
                    .map_err(|error| map_rig_error(error, self.max_response_bytes))?
            }
            LocalModelProvider::LlamaServer => {
                let client = llamafile::Client::builder()
                    .api_key(Nothing)
                    .base_url(&self.endpoint)
                    .http_client(self.http_client.clone())
                    .build()
                    .map_err(|_| Error::InvalidModelTransport {
                        reason: "cannot configure the Rig llama-server client".to_owned(),
                    })?;
                client
                    .completion_model(&request.model)
                    .completion(rig_request)
                    .await
                    .map_err(|error| map_rig_error(error, self.max_response_bytes))?
            }
        };

        convert_response(
            self.provider,
            &request.model,
            response,
            self.max_response_bytes,
        )
    }

    async fn stream_async(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
        started_at: Instant,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>,
    ) -> Result<ModelTurn> {
        let rig_request = convert_request(self.provider, request)?;
        let mut stream = match self.provider {
            LocalModelProvider::Ollama => {
                let client = ollama::Client::builder()
                    .api_key(Nothing)
                    .base_url(&self.endpoint)
                    .http_client(self.http_client.clone())
                    .build()
                    .map_err(|_| Error::InvalidModelTransport {
                        reason: "cannot configure the Rig Ollama client".to_owned(),
                    })?;
                client
                    .completion_model(&request.model)
                    .stream(rig_request)
                    .await
                    .map_err(|error| map_rig_error(error, self.max_response_bytes))?
            }
            LocalModelProvider::LlamaServer => {
                let client = llamafile::Client::builder()
                    .api_key(Nothing)
                    .base_url(&self.endpoint)
                    .http_client(self.http_client.clone())
                    .build()
                    .map_err(|_| Error::InvalidModelTransport {
                        reason: "cannot configure the Rig llama-server client".to_owned(),
                    })?;
                client
                    .completion_model(&request.model)
                    .stream(rig_request)
                    .await
                    .map_err(|error| map_rig_error(error, self.max_response_bytes))?
            }
        };

        let mut text = String::new();
        let mut tool_intents = Vec::new();
        let mut seen_ids = HashSet::new();
        let mut terminal = None;
        let mut output_bytes = 0_usize;

        while let Some(item) = stream.next().await {
            match item.map_err(|error| map_rig_error(error, self.max_response_bytes))? {
                StreamedAssistantContent::Text(delta) => {
                    check_stream_limits(cancellation, started_at, self.deadline)?;
                    add_response_bytes(
                        &mut output_bytes,
                        delta.text.len(),
                        self.max_response_bytes,
                    )?;
                    text.push_str(&delta.text);
                    on_event(ModelStreamEvent::TextDelta(delta.text))?;
                    check_stream_limits(cancellation, started_at, self.deadline)?;
                }
                StreamedAssistantContent::ToolCall { tool_call, .. } => {
                    check_stream_limits(cancellation, started_at, self.deadline)?;
                    let intent = convert_tool_call(
                        self.provider,
                        tool_call,
                        tool_intents.len(),
                        &mut seen_ids,
                    )?;
                    add_intent_bytes(&mut output_bytes, &intent, self.max_response_bytes)?;
                    on_event(ModelStreamEvent::ToolIntent(intent.clone()))?;
                    check_stream_limits(cancellation, started_at, self.deadline)?;
                    tool_intents.push(intent);
                }
                StreamedAssistantContent::ToolCallDelta { .. } => {}
                StreamedAssistantContent::Final(response) => terminal = Some(response),
                StreamedAssistantContent::Reasoning { .. }
                | StreamedAssistantContent::ReasoningDelta { .. }
                | StreamedAssistantContent::Unknown(_) => {
                    return Err(Error::MalformedModelResponse {
                        reason: "Rig returned unsupported non-text provider content".to_owned(),
                    });
                }
            }
        }

        let terminal = terminal.ok_or_else(|| Error::MalformedModelResponse {
            reason: "Rig stream ended without a provider terminal record".to_owned(),
        })?;
        turn_from_stream(self.provider, &request.model, text, tool_intents, terminal)
    }
}

impl ModelTransport for RigModelTransport {
    fn complete(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
    ) -> Result<ModelTurn> {
        validate_request(request)?;
        validate_provider_capabilities(self.provider, request)?;
        self.run_with_limits(cancellation, self.complete_async(request))
    }

    fn stream(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>,
    ) -> Result<ModelTurn> {
        validate_request(request)?;
        validate_provider_capabilities(self.provider, request)?;
        let started_at = Instant::now();
        self.run_with_limits(
            cancellation,
            self.stream_async(request, cancellation, started_at, on_event),
        )
    }

    fn deadline(&self) -> Duration {
        self.deadline
    }
}

async fn wait_for_cancellation(cancellation: &CancellationToken) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(CANCELLATION_POLL_INTERVAL).await;
    }
}

fn check_stream_limits(
    cancellation: &CancellationToken,
    started_at: Instant,
    deadline: Duration,
) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(Error::ModelTransportCancelled)
    } else if started_at.elapsed() >= deadline {
        Err(Error::ModelTransportTimedOut)
    } else {
        Ok(())
    }
}

fn validate_endpoint(endpoint: &str) -> Result<String> {
    if !endpoint.is_ascii()
        || endpoint.chars().any(char::is_whitespace)
        || endpoint.contains(['?', '#', '@'])
    {
        return invalid_transport("Rig endpoint must be a bounded ASCII loopback URL");
    }
    let authority =
        endpoint
            .strip_prefix("http://")
            .ok_or_else(|| Error::InvalidModelTransport {
                reason: "Rig endpoint must use http://".to_owned(),
            })?;
    if authority.is_empty() || authority.contains('/') {
        return invalid_transport("Rig endpoint cannot contain a path");
    }
    let address = SocketAddr::from_str(authority).map_err(|_| Error::InvalidModelTransport {
        reason: "Rig endpoint must contain a numeric IP address and explicit port".to_owned(),
    })?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return invalid_transport("Rig endpoint must be a numeric loopback address");
    }
    Ok(format!("http://{address}"))
}

fn validate_provider_capabilities(
    provider: LocalModelProvider,
    request: &ModelRequest,
) -> Result<()> {
    if request.reasoning_effort.is_some() {
        return Err(Error::UnsupportedCapability {
            provider: "rig-transport",
            capability: "reasoning effort",
        });
    }
    if provider == LocalModelProvider::Ollama && request.tool_choice == ToolChoice::Required {
        return Err(Error::UnsupportedCapability {
            provider: "ollama",
            capability: "required tool choice",
        });
    }
    Ok(())
}

fn convert_request(
    provider: LocalModelProvider,
    request: &ModelRequest,
) -> Result<RigCompletionRequest> {
    let mut provider_ids = HashMap::new();
    let mut chat_history = Vec::with_capacity(request.messages.len());
    for message in &request.messages {
        chat_history.push(convert_message(message, &mut provider_ids)?);
    }

    let tools = if request.tool_choice == ToolChoice::None {
        Vec::new()
    } else {
        request
            .tools
            .iter()
            .map(|tool| RigToolDefinition {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            })
            .collect()
    };
    let tool_choice = match (provider, request.tool_choice) {
        (LocalModelProvider::Ollama, _) => None,
        (_, ToolChoice::None) => Some(rig_core::message::ToolChoice::None),
        (_, ToolChoice::Auto) => Some(rig_core::message::ToolChoice::Auto),
        (_, ToolChoice::Required) => Some(rig_core::message::ToolChoice::Required),
    };

    Ok(RigCompletionRequest {
        model: Some(request.model.clone()),
        preamble: None,
        chat_history,
        documents: Vec::new(),
        tools,
        temperature: None,
        max_tokens: None,
        tool_choice,
        additional_params: None,
        output_schema: None,
        record_telemetry_content: false,
    })
}

fn convert_message(
    message: &ModelMessage,
    provider_ids: &mut HashMap<String, ProviderCallId>,
) -> Result<RigMessage> {
    match message {
        ModelMessage::System(text) => Ok(RigMessage::system(text)),
        ModelMessage::User(text) => Ok(RigMessage::user(text)),
        ModelMessage::Assistant { text, tool_intents } => {
            let mut content = Vec::new();
            if !text.is_empty() {
                content.push(AssistantContent::text(text));
            }
            for intent in tool_intents {
                let id = ToolCallId::new(intent.id.clone()).ok_or_else(|| {
                    Error::InvalidModelRequest {
                        reason: "tool call identity cannot be empty".to_owned(),
                    }
                })?;
                let provider = intent
                    .provider_id
                    .as_ref()
                    .and_then(|provider_id| ProviderCallId::new(provider_id.clone()));
                if let Some(provider) = &provider {
                    provider_ids.insert(intent.id.clone(), provider.clone());
                }
                content.push(AssistantContent::ToolCall(RigToolCall {
                    id,
                    provider,
                    function: ToolFunction::new(intent.name.clone(), intent.arguments.clone()),
                    signature: None,
                    additional_params: None,
                }));
            }
            Ok(RigMessage::Assistant { id: None, content })
        }
        ModelMessage::ToolResult {
            call_id,
            name,
            content,
        } => {
            let call = ToolCallId::new_or_mint(call_id);
            let provider = provider_ids.get(call_id).cloned();
            Ok(RigMessage::User {
                content: vec![UserContent::tool_result_for(
                    call,
                    provider,
                    name,
                    vec![ToolResultContent::text(content)],
                )],
            })
        }
    }
}

fn convert_response(
    provider: LocalModelProvider,
    requested_model: &str,
    response: RigCompletionResponse,
    max_response_bytes: usize,
) -> Result<ModelTurn> {
    let finish_reason = response.finish_reason();
    let mut text = String::new();
    let mut tool_intents = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut output_bytes = 0_usize;

    for content in response.choice {
        match content {
            AssistantContent::Text(value) => {
                add_response_bytes(&mut output_bytes, value.text.len(), max_response_bytes)?;
                text.push_str(&value.text);
            }
            AssistantContent::ToolCall(tool_call) => {
                let intent =
                    convert_tool_call(provider, tool_call, tool_intents.len(), &mut seen_ids)?;
                add_intent_bytes(&mut output_bytes, &intent, max_response_bytes)?;
                tool_intents.push(intent);
            }
            AssistantContent::Reasoning(_) | AssistantContent::Image(_) => {
                return Err(Error::MalformedModelResponse {
                    reason: "Rig returned unsupported non-text provider content".to_owned(),
                });
            }
        }
    }

    let finish_reason = map_finish_reason(finish_reason, !tool_intents.is_empty())?;
    let model = validate_metadata(
        response.model.as_deref().unwrap_or(requested_model),
        "model identity",
        256,
    )?;
    let response_id = validate_optional_metadata(response.response_id, "response identity", 256)?;
    let provider_request_id = validate_optional_metadata(
        response.provider_request_id,
        "provider request identity",
        256,
    )?;
    Ok(ModelTurn {
        text,
        reasoning: None,
        tool_intents,
        finish_reason,
        provider,
        model,
        response_id,
        provider_request_id,
        usage: convert_usage(response.usage),
    })
}

fn convert_tool_call(
    provider: LocalModelProvider,
    tool_call: RigToolCall,
    index: usize,
    seen_ids: &mut HashSet<String>,
) -> Result<ToolIntent> {
    if tool_call.function.name.is_empty()
        || tool_call.function.name.len() > 128
        || !tool_call
            .function
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(Error::MalformedModelResponse {
            reason: "Rig returned an invalid tool name".to_owned(),
        });
    }
    if !tool_call.function.arguments.is_object() {
        return Err(Error::MalformedModelResponse {
            reason: "Rig returned tool arguments that are not a JSON object".to_owned(),
        });
    }

    let provider_id = tool_call.provider.map(|identity| identity.call_id);
    let id = provider_id
        .clone()
        .unwrap_or_else(|| format!("rig-{}-call-{index}", provider.name()));
    if id.len() > 256 || id.chars().any(char::is_control) {
        return Err(Error::MalformedModelResponse {
            reason: "Rig returned an invalid tool call identity".to_owned(),
        });
    }
    if !seen_ids.insert(id.clone()) {
        return Err(Error::DuplicateToolCall);
    }

    Ok(ToolIntent {
        id,
        provider_id,
        name: tool_call.function.name,
        arguments: tool_call.function.arguments,
    })
}

fn add_intent_bytes(total: &mut usize, intent: &ToolIntent, maximum: usize) -> Result<()> {
    let arguments =
        serde_json::to_vec(&intent.arguments).map_err(|_| Error::MalformedModelResponse {
            reason: "Rig returned unserializable tool arguments".to_owned(),
        })?;
    let size = intent
        .id
        .len()
        .checked_add(intent.provider_id.as_ref().map_or(0, String::len))
        .and_then(|size| size.checked_add(intent.name.len()))
        .and_then(|size| size.checked_add(arguments.len()))
        .ok_or(Error::ModelResponseLimitExceeded { max_bytes: maximum })?;
    add_response_bytes(total, size, maximum)
}

fn add_response_bytes(total: &mut usize, size: usize, maximum: usize) -> Result<()> {
    *total = total
        .checked_add(size)
        .ok_or(Error::ModelResponseLimitExceeded { max_bytes: maximum })?;
    if *total > maximum {
        return Err(Error::ModelResponseLimitExceeded { max_bytes: maximum });
    }
    Ok(())
}

fn turn_from_stream(
    provider: LocalModelProvider,
    requested_model: &str,
    text: String,
    tool_intents: Vec<ToolIntent>,
    terminal: StreamFinal,
) -> Result<ModelTurn> {
    let finish_reason = map_finish_reason(terminal.finish_reason, !tool_intents.is_empty())?;
    let model = validate_metadata(
        terminal.model.as_deref().unwrap_or(requested_model),
        "model identity",
        256,
    )?;
    let response_id = validate_optional_metadata(terminal.response_id, "response identity", 256)?;
    let provider_request_id = validate_optional_metadata(
        terminal.provider_request_id,
        "provider request identity",
        256,
    )?;
    Ok(ModelTurn {
        text,
        reasoning: None,
        finish_reason,
        tool_intents,
        provider,
        model,
        response_id,
        provider_request_id,
        usage: convert_usage(terminal.usage),
    })
}

fn map_finish_reason(reason: Option<RigFinishReason>, has_tools: bool) -> Result<FinishReason> {
    Ok(match reason {
        Some(RigFinishReason::Stop) => {
            if has_tools {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            }
        }
        Some(RigFinishReason::Length) => FinishReason::Length,
        Some(RigFinishReason::ToolCalls) => FinishReason::ToolCalls,
        Some(RigFinishReason::ContentFilter) => FinishReason::ContentFilter,
        Some(RigFinishReason::Other(reason)) => {
            FinishReason::Other(validate_metadata(&reason, "finish reason", 128)?)
        }
        None if has_tools => FinishReason::ToolCalls,
        None => FinishReason::Stop,
    })
}

fn convert_usage(usage: rig_core::completion::Usage) -> Option<ModelUsage> {
    if usage.input_tokens == 0 && usage.output_tokens == 0 && usage.total_tokens == 0 {
        None
    } else {
        Some(ModelUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
        })
    }
}

fn map_rig_error(error: CompletionError, max_response_bytes: usize) -> Error {
    if let Some(status) = error.provider_response_status() {
        return Error::ModelProviderStatus {
            status: status.as_u16(),
        };
    }
    match error {
        CompletionError::RequestError(_) | CompletionError::UrlError(_) => {
            Error::InvalidModelRequest {
                reason: "Rig rejected the canonical model request".to_owned(),
            }
        }
        CompletionError::HttpError(error) => match bounded_error(&error) {
            Some(BoundedClientError::MalformedOllama) => Error::MalformedModelResponse {
                reason: "Rig received an invalid Ollama terminal or usage record".to_owned(),
            },
            Some(BoundedClientError::RequestLimit) => Error::InvalidModelRequest {
                reason: "serialized model request exceeds 2097152 bytes".to_owned(),
            },
            Some(BoundedClientError::ResponseLimit) => Error::ModelResponseLimitExceeded {
                max_bytes: max_response_bytes,
            },
            _ => Error::ModelTransportFramework {
                operation: "sending the provider request",
            },
        },
        CompletionError::ProviderError(reason) => {
            if reason.contains(MALFORMED_OLLAMA_MARKER) {
                Error::MalformedModelResponse {
                    reason: "Rig received an invalid Ollama terminal or usage record".to_owned(),
                }
            } else if reason.contains(REQUEST_LIMIT_MARKER) {
                Error::InvalidModelRequest {
                    reason: "serialized model request exceeds 2097152 bytes".to_owned(),
                }
            } else if reason.contains(RESPONSE_LIMIT_MARKER) {
                Error::ModelResponseLimitExceeded {
                    max_bytes: max_response_bytes,
                }
            } else {
                Error::ModelTransportFramework {
                    operation: "sending the provider request",
                }
            }
        }
        CompletionError::JsonError(_)
        | CompletionError::ResponseError(_)
        | CompletionError::ProviderResponse(_) => Error::MalformedModelResponse {
            reason: "Rig could not decode the provider response".to_owned(),
        },
    }
}

fn bounded_error<'a>(error: &'a (dyn StdError + 'static)) -> Option<&'a BoundedClientError> {
    let mut current = Some(error);
    while let Some(source) = current {
        if let Some(error) = source.downcast_ref::<BoundedClientError>() {
            return Some(error);
        }
        current = source.source();
    }
    None
}

fn validate_optional_metadata(
    value: Option<String>,
    label: &str,
    maximum: usize,
) -> Result<Option<String>> {
    value
        .map(|value| validate_metadata(&value, label, maximum))
        .transpose()
}

fn validate_metadata(value: &str, label: &str, maximum: usize) -> Result<String> {
    if value.trim().is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(Error::MalformedModelResponse {
            reason: format!("provider response contained an invalid {label}"),
        });
    }
    Ok(value.to_owned())
}

fn invalid_transport<T>(reason: &str) -> Result<T> {
    Err(Error::InvalidModelTransport {
        reason: reason.to_owned(),
    })
}

#[derive(Clone, Debug, Default)]
struct BoundedHttpClient {
    client: Option<reqwest::Client>,
    max_response_bytes: usize,
}

impl BoundedHttpClient {
    fn new(client: reqwest::Client, max_response_bytes: usize) -> Self {
        Self {
            client: Some(client),
            max_response_bytes,
        }
    }

    fn client(&self) -> rig_http::Result<reqwest::Client> {
        self.client.clone().ok_or_else(uninitialized_client)
    }
}

impl HttpClientExt for BoundedHttpClient {
    fn send<T, U>(
        &self,
        request: RigHttpRequest<T>,
    ) -> impl Future<Output = rig_http::Result<RigHttpResponse<LazyBody<U>>>> + Send + 'static
    where
        T: Into<bytes::Bytes> + Send,
        U: From<bytes::Bytes> + Send + 'static,
    {
        let client = self.client();
        let maximum = self.max_response_bytes;
        let (parts, body) = request.into_parts();
        let body: bytes::Bytes = body.into();
        let validate_ollama = parts.uri.path() == "/api/chat";
        async move {
            reject_oversized_request(body.len())?;
            let client = client?;
            let response = client
                .request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .body(body)
                .send()
                .await
                .map_err(instance_error)?;
            let status = response.status();
            if !status.is_success() {
                return Err(rig_http::Error::InvalidStatusCode(status));
            }
            reject_declared_oversize(&response, maximum)?;

            let version = response.version();
            let headers = response.headers().clone();
            let body: LazyBody<U> = Box::pin(async move {
                let body = read_bounded_response(response, maximum).await?;
                if validate_ollama {
                    validate_ollama_unary(&body)?;
                }
                Ok(U::from(body))
            });
            build_rig_response(status, version, headers, body)
        }
    }

    // Rig requires a `'static` future; `async fn` would capture `&self`.
    #[allow(clippy::manual_async_fn)]
    fn send_multipart<U>(
        &self,
        _request: RigHttpRequest<MultipartForm>,
    ) -> impl Future<Output = rig_http::Result<RigHttpResponse<LazyBody<U>>>> + Send + 'static
    where
        U: From<bytes::Bytes> + Send + 'static,
    {
        async { Err(instance_error(BoundedClientError::UnsupportedMultipart)) }
    }

    fn send_streaming<T>(
        &self,
        request: RigHttpRequest<T>,
    ) -> impl Future<Output = rig_http::Result<StreamingResponse>> + Send
    where
        T: Into<bytes::Bytes> + Send,
    {
        let client = self.client();
        let maximum = self.max_response_bytes;
        let (parts, body) = request.into_parts();
        let body: bytes::Bytes = body.into();
        let validate_ollama = parts.uri.path() == "/api/chat";
        async move {
            reject_oversized_request(body.len())?;
            let client = client?;
            let response = client
                .request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .body(body)
                .send()
                .await
                .map_err(instance_error)?;
            let status = response.status();
            if !status.is_success() {
                return Err(rig_http::Error::InvalidStatusCode(status));
            }
            reject_declared_oversize(&response, maximum)?;

            let version = response.version();
            let headers = response.headers().clone();
            let stream: BoxedStream = if validate_ollama {
                Box::pin(OllamaResponseStream::new(response, maximum))
            } else {
                let mut total = 0_usize;
                Box::pin(response.bytes_stream().map(move |chunk| {
                    let chunk = chunk.map_err(instance_error)?;
                    total = total
                        .checked_add(chunk.len())
                        .ok_or_else(response_limit_error)?;
                    if total > maximum {
                        return Err(response_limit_error());
                    }
                    Ok(chunk)
                }))
            };
            build_rig_response(status, version, headers, stream)
        }
    }
}

async fn read_bounded_response(
    response: reqwest::Response,
    maximum: usize,
) -> rig_http::Result<bytes::Bytes> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(instance_error)?;
        let total = body
            .len()
            .checked_add(chunk.len())
            .ok_or_else(response_limit_error)?;
        if total > maximum {
            return Err(response_limit_error());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(bytes::Bytes::from(body))
}

fn validate_ollama_unary(body: &[u8]) -> rig_http::Result<()> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Ok(());
    };
    validate_ollama_usage(&value)?;
    if value.get("done").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(malformed_ollama_error());
    }
    Ok(())
}

fn validate_ollama_stream_record(record: &[u8]) -> rig_http::Result<()> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(record) else {
        return Ok(());
    };
    validate_ollama_usage(&value)
}

fn validate_ollama_usage(value: &serde_json::Value) -> rig_http::Result<()> {
    let input = value
        .get("prompt_eval_count")
        .and_then(serde_json::Value::as_u64);
    let output = value.get("eval_count").and_then(serde_json::Value::as_u64);
    if input
        .zip(output)
        .is_some_and(|(input, output)| input.checked_add(output).is_none())
    {
        return Err(malformed_ollama_error());
    }
    Ok(())
}

struct OllamaResponseStream {
    inner: Pin<
        Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send + 'static>,
    >,
    buffer: bytes::BytesMut,
    total: usize,
    maximum: usize,
    ended: bool,
}

impl OllamaResponseStream {
    fn new(response: reqwest::Response, maximum: usize) -> Self {
        Self {
            inner: Box::pin(response.bytes_stream()),
            buffer: bytes::BytesMut::new(),
            total: 0,
            maximum,
            ended: false,
        }
    }
}

impl Stream for OllamaResponseStream {
    type Item = rig_http::Result<bytes::Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let record = self.buffer.split_to(position + 1).freeze();
                if let Err(error) = validate_ollama_stream_record(&record) {
                    return Poll::Ready(Some(Err(error)));
                }
                return Poll::Ready(Some(Ok(record)));
            }
            if self.ended {
                if self.buffer.is_empty() {
                    return Poll::Ready(None);
                }
                let record = self.buffer.split().freeze();
                if let Err(error) = validate_ollama_stream_record(&record) {
                    return Poll::Ready(Some(Err(error)));
                }
                return Poll::Ready(Some(Ok(record)));
            }

            match self.inner.as_mut().poll_next(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => self.ended = true,
                Poll::Ready(Some(Err(error))) => {
                    return Poll::Ready(Some(Err(instance_error(error))));
                }
                Poll::Ready(Some(Ok(chunk))) => {
                    self.total = match self.total.checked_add(chunk.len()) {
                        Some(total) if total <= self.maximum => total,
                        _ => return Poll::Ready(Some(Err(response_limit_error()))),
                    };
                    self.buffer.extend_from_slice(&chunk);
                }
            }
        }
    }
}

fn reject_declared_oversize(response: &reqwest::Response, maximum: usize) -> rig_http::Result<()> {
    if response
        .content_length()
        .is_some_and(|length| length > maximum as u64)
    {
        return Err(response_limit_error());
    }
    Ok(())
}

fn build_rig_response<T>(
    status: reqwest::StatusCode,
    version: reqwest::Version,
    headers: reqwest::header::HeaderMap,
    body: T,
) -> rig_http::Result<RigHttpResponse<T>> {
    let mut response = RigHttpResponse::builder().status(status).version(version);
    let target_headers = response.headers_mut().ok_or(rig_http::Error::NoHeaders)?;
    *target_headers = headers;
    response.body(body).map_err(rig_http::Error::Protocol)
}

fn instance_error(error: impl StdError + Send + Sync + 'static) -> rig_http::Error {
    rig_http::Error::Instance(Box::new(error))
}

fn response_limit_error() -> rig_http::Error {
    instance_error(BoundedClientError::ResponseLimit)
}

fn malformed_ollama_error() -> rig_http::Error {
    instance_error(BoundedClientError::MalformedOllama)
}

fn request_limit_error() -> rig_http::Error {
    instance_error(BoundedClientError::RequestLimit)
}

fn reject_oversized_request(size: usize) -> rig_http::Result<()> {
    if size > MAX_REQUEST_BYTES {
        return Err(request_limit_error());
    }
    Ok(())
}

fn uninitialized_client() -> rig_http::Error {
    instance_error(BoundedClientError::Uninitialized)
}

#[derive(Debug)]
enum BoundedClientError {
    MalformedOllama,
    RequestLimit,
    ResponseLimit,
    Uninitialized,
    UnsupportedMultipart,
}

impl fmt::Display for BoundedClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedOllama => formatter.write_str(MALFORMED_OLLAMA_MARKER),
            Self::RequestLimit => formatter.write_str(REQUEST_LIMIT_MARKER),
            Self::ResponseLimit => formatter.write_str(RESPONSE_LIMIT_MARKER),
            Self::Uninitialized => formatter.write_str("bounded Rig HTTP client is uninitialized"),
            Self::UnsupportedMultipart => {
                formatter.write_str("bounded Rig HTTP client does not support multipart requests")
            }
        }
    }
}

impl StdError for BoundedClientError {}
