use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    str::FromStr,
    time::{Duration, Instant},
};

use serde_json::{Map, Value, json};

use crate::{
    CancellationToken, Error, FinishReason, LocalModelProvider, ModelMessage, ModelRequest,
    ModelStreamEvent, ModelTransport, ModelTurn, ModelUsage, Result, ToolChoice, ToolIntent,
};

const MAX_DEADLINE: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_MESSAGES: usize = 1024;
const MAX_TOOLS: usize = 128;
const MAX_OUTPUT_SCHEMA_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_SCHEMA_DEPTH: usize = 32;
const MAX_OUTPUT_SCHEMA_NODES: usize = 4096;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const IO_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Direct bounded HTTP transport for local Ollama and llama-server endpoints.
#[derive(Debug, Clone)]
pub struct DirectModelTransport {
    provider: LocalModelProvider,
    endpoint: Endpoint,
    deadline: Duration,
    max_response_bytes: usize,
}

#[derive(Debug, Clone)]
struct Endpoint {
    authority: String,
    addresses: Vec<SocketAddr>,
}

#[derive(Debug)]
struct BufferedSocket {
    stream: TcpStream,
    buffered: VecDeque<u8>,
}

#[derive(Debug, Default)]
struct PartialToolCall {
    provider_id: Option<String>,
    name: String,
    arguments: String,
}

#[derive(Debug)]
struct StreamDecoder {
    pending: Vec<u8>,
    state: StreamState,
}

#[derive(Debug)]
enum StreamState {
    OpenAi(OpenAiStreamState),
    Ollama(OllamaStreamState),
}

#[derive(Debug)]
struct OpenAiStreamState {
    requested_model: String,
    text: String,
    reasoning: String,
    calls: BTreeMap<usize, PartialToolCall>,
    terminal: bool,
    finish_reason: Option<FinishReason>,
    model: Option<String>,
    request_id: Option<String>,
    usage: Option<ModelUsage>,
}

#[derive(Debug)]
struct OllamaStreamState {
    requested_model: String,
    text: String,
    reasoning: String,
    tool_intents: Vec<ToolIntent>,
    terminal: bool,
    finish_reason: Option<FinishReason>,
    model: Option<String>,
    usage: Option<ModelUsage>,
    provider_ids: HashSet<String>,
}

impl DirectModelTransport {
    /// Creates a local-only transport with explicit deadline and response bound.
    pub fn new(
        provider: LocalModelProvider,
        endpoint: &str,
        deadline: Duration,
        max_response_bytes: usize,
    ) -> Result<Self> {
        if deadline.is_zero() || deadline > MAX_DEADLINE {
            return Err(Error::InvalidModelTransport {
                reason: "deadline must be between one nanosecond and 24 hours".to_owned(),
            });
        }

        if max_response_bytes == 0 || max_response_bytes > MAX_RESPONSE_BYTES {
            return Err(Error::InvalidModelTransport {
                reason: format!("response limit must be between 1 and {MAX_RESPONSE_BYTES} bytes"),
            });
        }

        Ok(Self {
            provider,
            endpoint: parse_endpoint(endpoint)?,
            deadline,
            max_response_bytes,
        })
    }

    fn execute_with_body(
        &self,
        request: &ModelRequest,
        stream: bool,
        cancellation: &CancellationToken,
        on_body: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        check_cancelled(cancellation)?;
        validate_request(request)?;
        if self.provider == LocalModelProvider::Ollama
            && request.tool_choice == ToolChoice::Required
        {
            return Err(Error::UnsupportedCapability {
                provider: self.provider.name(),
                capability: "required tool choice",
            });
        }

        let body = encode_request(self.provider, request, stream)?;
        if body.len() > MAX_REQUEST_BYTES {
            return Err(Error::InvalidModelRequest {
                reason: format!("serialized request exceeds {MAX_REQUEST_BYTES} bytes"),
            });
        }

        let deadline = Instant::now() + self.deadline;
        let mut socket = self.connect(cancellation, deadline)?;
        let path = match self.provider {
            LocalModelProvider::Ollama => "/api/chat",
            LocalModelProvider::LlamaServer => "/v1/chat/completions",
        };
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.endpoint.authority,
            body.len()
        );
        write_checked(&mut socket, head.as_bytes(), cancellation, deadline)?;
        write_checked(&mut socket, &body, cancellation, deadline)?;
        read_response(
            socket,
            cancellation,
            deadline,
            self.max_response_bytes,
            on_body,
        )
    }

    fn connect(&self, cancellation: &CancellationToken, deadline: Instant) -> Result<TcpStream> {
        connect_endpoint(&self.endpoint, cancellation, deadline)
    }
}

pub(crate) fn get_bounded(
    endpoint: &str,
    path: &str,
    deadline: Duration,
    max_response_bytes: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>> {
    if deadline.is_zero() || deadline > MAX_DEADLINE {
        return Err(Error::InvalidModelTransport {
            reason: "deadline must be between one nanosecond and 24 hours".to_owned(),
        });
    }
    if max_response_bytes == 0 || max_response_bytes > MAX_RESPONSE_BYTES {
        return Err(Error::InvalidModelTransport {
            reason: format!("response limit must be between 1 and {MAX_RESPONSE_BYTES} bytes"),
        });
    }
    if !path.starts_with('/')
        || !path.is_ascii()
        || path.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(Error::InvalidModelTransport {
            reason: "HTTP request path is invalid".to_owned(),
        });
    }
    let endpoint = parse_endpoint(endpoint)?;
    let expires = Instant::now() + deadline;
    let mut socket = connect_endpoint(&endpoint, cancellation, expires)?;
    let head = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        endpoint.authority
    );
    write_checked(&mut socket, head.as_bytes(), cancellation, expires)?;
    let mut body = Vec::new();
    read_response(
        socket,
        cancellation,
        expires,
        max_response_bytes,
        &mut |chunk| {
            body.extend_from_slice(chunk);
            Ok(())
        },
    )?;
    Ok(body)
}

fn connect_endpoint(
    endpoint: &Endpoint,
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<TcpStream> {
    let mut last_error = None;
    for address in &endpoint.addresses {
        check_cancelled(cancellation)?;
        let timeout = remaining_poll_timeout(deadline)?;
        match TcpStream::connect_timeout(address, timeout) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(IO_POLL_INTERVAL))
                    .map_err(|source| Error::ModelTransportIo {
                        operation: "configuring socket reads",
                        source,
                    })?;
                stream
                    .set_write_timeout(Some(IO_POLL_INTERVAL))
                    .map_err(|source| Error::ModelTransportIo {
                        operation: "configuring socket writes",
                        source,
                    })?;
                return Ok(stream);
            }
            Err(source) => last_error = Some(source),
        }
    }
    Err(Error::ModelTransportIo {
        operation: "connecting to local provider",
        source: last_error.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::AddrNotAvailable, "no loopback address")
        }),
    })
}

impl ModelTransport for DirectModelTransport {
    fn complete(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
    ) -> Result<ModelTurn> {
        let mut body = Vec::new();
        self.execute_with_body(request, false, cancellation, &mut |chunk| {
            body.extend_from_slice(chunk);
            Ok(())
        })?;
        match self.provider {
            LocalModelProvider::Ollama => parse_ollama_unary(&body, request),
            LocalModelProvider::LlamaServer => parse_openai_unary(&body, request),
        }
    }

    fn stream(
        &self,
        request: &ModelRequest,
        cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>,
    ) -> Result<ModelTurn> {
        let mut decoder = StreamDecoder::new(self.provider, request);
        let decode_deadline = Instant::now() + self.deadline;
        let mut deliver = |event| {
            check_cancelled(cancellation)?;
            check_deadline(decode_deadline)?;
            let result = on_event(event);
            check_cancelled(cancellation)?;
            check_deadline(decode_deadline)?;
            result
        };
        self.execute_with_body(request, true, cancellation, &mut |chunk| {
            decoder.push(chunk, cancellation, decode_deadline, &mut deliver)
        })?;
        check_cancelled(cancellation)?;
        check_deadline(decode_deadline)?;
        let turn = decoder.finish(&mut deliver)?;
        check_cancelled(cancellation)?;
        check_deadline(decode_deadline)?;
        Ok(turn)
    }

    fn deadline(&self) -> Duration {
        self.deadline
    }
}

fn parse_endpoint(value: &str) -> Result<Endpoint> {
    if !value.is_ascii()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        || !value.starts_with("http://")
    {
        return invalid_endpoint("only bounded ASCII HTTP loopback endpoints are supported");
    }

    let remainder = &value["http://".len()..];
    if remainder.contains(['@', '?', '#']) {
        return invalid_endpoint("user information, query, and fragment are forbidden");
    }
    let authority = match remainder.split_once('/') {
        Some((authority, "")) => authority,
        Some(_) => return invalid_endpoint("provider paths are selected by provider kind"),
        None => remainder,
    };
    if authority.is_empty() {
        return invalid_endpoint("host is required");
    }

    let (host, port) = split_authority(authority)?;
    let ip = IpAddr::from_str(host).map_err(|_| Error::InvalidModelTransport {
        reason: "host must be a numeric loopback address".to_owned(),
    })?;
    if !ip.is_loopback() {
        return invalid_endpoint("host must be a numeric loopback address");
    }

    Ok(Endpoint {
        authority: authority.to_owned(),
        addresses: vec![SocketAddr::new(ip, port)],
    })
}

fn split_authority(authority: &str) -> Result<(&str, u16)> {
    if let Some(remainder) = authority.strip_prefix('[') {
        let (host, port) =
            remainder
                .split_once("]:")
                .ok_or_else(|| Error::InvalidModelTransport {
                    reason: "IPv6 endpoint must include a port".to_owned(),
                })?;
        return parse_port(host, port);
    }
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| Error::InvalidModelTransport {
            reason: "endpoint must include an explicit port".to_owned(),
        })?;
    if host.contains(':') {
        return invalid_endpoint("IPv6 hosts must use brackets");
    }
    parse_port(host, port)
}

fn parse_port<'a>(host: &'a str, port: &str) -> Result<(&'a str, u16)> {
    if host.is_empty() {
        return invalid_endpoint("host is required");
    }
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| Error::InvalidModelTransport {
            reason: "endpoint port must be between 1 and 65535".to_owned(),
        })?;
    Ok((host, port))
}

fn invalid_endpoint<T>(reason: &str) -> Result<T> {
    Err(Error::InvalidModelTransport {
        reason: reason.to_owned(),
    })
}

pub(crate) fn validate_request(request: &ModelRequest) -> Result<()> {
    if request.model.trim().is_empty() || request.model.len() > 256 || !request.model.is_ascii() {
        return invalid_request("model must be nonblank ASCII no longer than 256 bytes");
    }
    if request.messages.is_empty() || request.messages.len() > MAX_MESSAGES {
        return invalid_request("message count must be between 1 and 1024");
    }
    if request.tools.len() > MAX_TOOLS {
        return invalid_request("tool count exceeds 128");
    }

    let mut text_bytes = 0_usize;
    for message in &request.messages {
        match message {
            ModelMessage::System(text) | ModelMessage::User(text) => {
                checked_text_total(&mut text_bytes, text.len())?;
            }
            ModelMessage::Assistant { text, tool_intents } => {
                checked_text_total(&mut text_bytes, text.len())?;
                validate_intents(tool_intents)?;
            }
            ModelMessage::ToolResult {
                call_id,
                name,
                content,
            } => {
                validate_identity(call_id, "tool result call identity")?;
                validate_tool_name(name)?;
                checked_text_total(&mut text_bytes, content.len())?;
            }
        }
    }

    let mut names = HashSet::new();
    for tool in &request.tools {
        validate_tool_name(&tool.name)?;
        if !names.insert(tool.name.as_str()) {
            return invalid_request("tool names must be unique");
        }
        if tool.description.len() > 16 * 1024 {
            return invalid_request("tool description exceeds 16384 bytes");
        }
        if !tool.parameters.is_object() {
            return invalid_request("tool parameters must be a JSON object schema");
        }
    }
    if request.tool_choice != ToolChoice::None && request.tools.is_empty() {
        return invalid_request("tool choice requires at least one tool");
    }
    if let Some(schema) = &request.output_schema {
        validate_output_schema(schema)?;
        if request.tool_choice != ToolChoice::None {
            return invalid_request("output schema cannot be combined with callable tools");
        }
    }
    Ok(())
}

fn validate_output_schema(schema: &Value) -> Result<()> {
    let encoded = serde_json::to_vec(schema).map_err(|_| Error::InvalidModelRequest {
        reason: "output schema cannot be serialized".to_owned(),
    })?;
    if encoded.len() > MAX_OUTPUT_SCHEMA_BYTES {
        return invalid_request("output schema exceeds 262144 bytes");
    }

    let mut nodes = 0_usize;
    validate_output_schema_node(schema, 0, &mut nodes)
}

fn validate_output_schema_node(schema: &Value, depth: usize, nodes: &mut usize) -> Result<()> {
    if depth > MAX_OUTPUT_SCHEMA_DEPTH {
        return invalid_request("output schema exceeds 32 levels");
    }
    *nodes = nodes
        .checked_add(1)
        .ok_or_else(|| Error::InvalidModelRequest {
            reason: "output schema node count overflow".to_owned(),
        })?;
    if *nodes > MAX_OUTPUT_SCHEMA_NODES {
        return invalid_request("output schema exceeds 4096 nodes");
    }

    let object = schema
        .as_object()
        .ok_or_else(|| Error::InvalidModelRequest {
            reason: "output schema nodes must be JSON objects".to_owned(),
        })?;
    const ALLOWED_KEYWORDS: &[&str] = &[
        "type",
        "title",
        "description",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "enum",
        "const",
        "anyOf",
        "allOf",
        "minimum",
        "maximum",
        "minLength",
        "maxLength",
        "minItems",
        "maxItems",
    ];
    if let Some(keyword) = object
        .keys()
        .find(|keyword| !ALLOWED_KEYWORDS.contains(&keyword.as_str()))
    {
        return invalid_request(&format!(
            "output schema keyword `{keyword}` is outside the supported provider subset"
        ));
    }

    if let Some(types) = object.get("type") {
        let valid_type = |value: &str| {
            matches!(
                value,
                "null" | "boolean" | "object" | "array" | "number" | "string" | "integer"
            )
        };
        match types {
            Value::String(value) if valid_type(value) => {}
            Value::Array(values)
                if !values.is_empty()
                    && values
                        .iter()
                        .all(|value| value.as_str().is_some_and(valid_type)) => {}
            _ => return invalid_request("output schema `type` is invalid"),
        }
    }
    for keyword in ["title", "description"] {
        if object.get(keyword).is_some_and(|value| !value.is_string()) {
            return invalid_request(&format!("output schema `{keyword}` must be text"));
        }
    }
    for keyword in ["minimum", "maximum"] {
        if object.get(keyword).is_some_and(|value| !value.is_number()) {
            return invalid_request(&format!("output schema `{keyword}` must be a number"));
        }
    }
    for keyword in ["minLength", "maxLength", "minItems", "maxItems"] {
        if object
            .get(keyword)
            .is_some_and(|value| value.as_u64().is_none())
        {
            return invalid_request(&format!(
                "output schema `{keyword}` must be a nonnegative integer"
            ));
        }
    }
    if let Some(values) = object.get("enum")
        && values.as_array().is_none_or(Vec::is_empty)
    {
        return invalid_request("output schema `enum` must be a nonempty array");
    }

    let properties = object.get("properties").map(|value| {
        value.as_object().ok_or_else(|| Error::InvalidModelRequest {
            reason: "output schema `properties` must be an object".to_owned(),
        })
    });
    let properties = properties.transpose()?;
    if let Some(properties) = properties {
        for child in properties.values() {
            validate_output_schema_node(child, depth + 1, nodes)?;
        }
    }
    if let Some(required) = object.get("required") {
        let required = required
            .as_array()
            .filter(|values| {
                values.iter().all(Value::is_string)
                    && values
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<HashSet<_>>()
                        .len()
                        == values.len()
            })
            .ok_or_else(|| Error::InvalidModelRequest {
                reason: "output schema `required` must contain unique property names".to_owned(),
            })?;
        let Some(properties) = properties else {
            return invalid_request("output schema `required` needs `properties`");
        };
        if required
            .iter()
            .filter_map(Value::as_str)
            .any(|name| !properties.contains_key(name))
        {
            return invalid_request("output schema `required` names an absent property");
        }
    }
    if let Some(additional) = object.get("additionalProperties") {
        match additional {
            Value::Bool(_) => {}
            Value::Object(_) => validate_output_schema_node(additional, depth + 1, nodes)?,
            _ => {
                return invalid_request(
                    "output schema `additionalProperties` must be a boolean or schema",
                );
            }
        }
    }
    if let Some(items) = object.get("items") {
        validate_output_schema_node(items, depth + 1, nodes)?;
    }
    for keyword in ["anyOf", "allOf"] {
        if let Some(branches) = object.get(keyword) {
            let branches = branches
                .as_array()
                .filter(|branches| !branches.is_empty())
                .ok_or_else(|| Error::InvalidModelRequest {
                    reason: format!("output schema `{keyword}` must be a nonempty array"),
                })?;
            for branch in branches {
                validate_output_schema_node(branch, depth + 1, nodes)?;
            }
        }
    }
    Ok(())
}

fn checked_text_total(total: &mut usize, next: usize) -> Result<()> {
    *total = total
        .checked_add(next)
        .ok_or_else(|| Error::InvalidModelRequest {
            reason: "message text size overflow".to_owned(),
        })?;
    if *total > MAX_MESSAGE_BYTES {
        return invalid_request("canonical message text exceeds 8388608 bytes");
    }
    Ok(())
}

fn validate_intents(intents: &[ToolIntent]) -> Result<()> {
    let mut ids = HashSet::new();
    for intent in intents {
        validate_identity(&intent.id, "tool call identity")?;
        validate_tool_name(&intent.name)?;
        if !intent.arguments.is_object() {
            return invalid_request("tool arguments must be a JSON object");
        }
        if !ids.insert(intent.id.as_str()) {
            return Err(Error::DuplicateToolCall);
        }
    }
    Ok(())
}

fn validate_identity(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return invalid_request(&format!(
            "{label} must be nonblank and no longer than 256 bytes"
        ));
    }
    Ok(())
}

fn validate_tool_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return invalid_request("tool name must use 1-128 ASCII letters, digits, `_`, `-`, or `.`");
    }
    Ok(())
}

fn invalid_request<T>(reason: &str) -> Result<T> {
    Err(Error::InvalidModelRequest {
        reason: reason.to_owned(),
    })
}

fn encode_request(
    provider: LocalModelProvider,
    request: &ModelRequest,
    stream: bool,
) -> Result<Vec<u8>> {
    let mut root = Map::new();
    root.insert("model".to_owned(), Value::String(request.model.clone()));
    root.insert(
        "messages".to_owned(),
        Value::Array(
            request
                .messages
                .iter()
                .map(|message| encode_message(provider, message))
                .collect(),
        ),
    );
    root.insert("stream".to_owned(), Value::Bool(stream));
    if let Some(effort) = request.reasoning_effort {
        match provider {
            LocalModelProvider::LlamaServer => {
                root.insert(
                    "reasoning_effort".to_owned(),
                    Value::String(effort.as_str().to_owned()),
                );
            }
            LocalModelProvider::Ollama => {
                root.insert(
                    "think".to_owned(),
                    if effort == crate::ReasoningEffort::None {
                        Value::Bool(false)
                    } else {
                        Value::String(effort.as_str().to_owned())
                    },
                );
            }
        }
    }
    if let Some(schema) = &request.output_schema {
        match provider {
            LocalModelProvider::Ollama => {
                root.insert("format".to_owned(), schema.clone());
            }
            LocalModelProvider::LlamaServer => {
                root.insert(
                    "response_format".to_owned(),
                    json!({
                        "type": "json_schema",
                        "json_schema": {
                            "name": "kvist_output",
                            "strict": true,
                            "schema": schema
                        }
                    }),
                );
            }
        }
    }

    if request.tool_choice != ToolChoice::None {
        root.insert(
            "tools".to_owned(),
            Value::Array(
                request
                    .tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.parameters
                            }
                        })
                    })
                    .collect(),
            ),
        );
    }
    if provider == LocalModelProvider::LlamaServer {
        let choice = match request.tool_choice {
            ToolChoice::None => "none",
            ToolChoice::Auto => "auto",
            ToolChoice::Required => "required",
        };
        root.insert("tool_choice".to_owned(), Value::String(choice.to_owned()));
    }

    serde_json::to_vec(&root).map_err(|_| Error::InvalidModelRequest {
        reason: "canonical request cannot be serialized".to_owned(),
    })
}

fn encode_message(provider: LocalModelProvider, message: &ModelMessage) -> Value {
    match message {
        ModelMessage::System(content) => json!({"role": "system", "content": content}),
        ModelMessage::User(content) => json!({"role": "user", "content": content}),
        ModelMessage::Assistant { text, tool_intents } => {
            let calls = tool_intents
                .iter()
                .map(|intent| {
                    let arguments = match provider {
                        LocalModelProvider::Ollama => intent.arguments.clone(),
                        LocalModelProvider::LlamaServer => {
                            Value::String(intent.arguments.to_string())
                        }
                    };
                    json!({
                        "id": intent.provider_id.as_ref().unwrap_or(&intent.id),
                        "type": "function",
                        "function": {"name": intent.name, "arguments": arguments}
                    })
                })
                .collect::<Vec<_>>();
            if calls.is_empty() {
                json!({"role": "assistant", "content": text})
            } else {
                json!({"role": "assistant", "content": text, "tool_calls": calls})
            }
        }
        ModelMessage::ToolResult {
            call_id,
            name,
            content,
        } => match provider {
            LocalModelProvider::Ollama => json!({
                "role": "tool",
                "content": content,
                "tool_name": name,
                "tool_call_id": call_id
            }),
            LocalModelProvider::LlamaServer => {
                json!({"role": "tool", "content": content, "tool_call_id": call_id, "name": name})
            }
        },
    }
}

fn write_checked(
    stream: &mut TcpStream,
    mut data: &[u8],
    cancellation: &CancellationToken,
    deadline: Instant,
) -> Result<()> {
    while !data.is_empty() {
        check_cancelled(cancellation)?;
        check_deadline(deadline)?;
        match stream.write(data) {
            Ok(0) => {
                return Err(Error::ModelTransportIo {
                    operation: "writing request",
                    source: io::Error::new(io::ErrorKind::WriteZero, "socket closed"),
                });
            }
            Ok(count) => data = &data[count..],
            Err(error) if is_retryable_timeout(&error) => {}
            Err(source) => {
                return Err(Error::ModelTransportIo {
                    operation: "writing request",
                    source,
                });
            }
        }
    }
    Ok(())
}

fn read_response(
    mut stream: TcpStream,
    cancellation: &CancellationToken,
    deadline: Instant,
    max_response_bytes: usize,
    on_body: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let mut received = Vec::new();
    let header_end = loop {
        if let Some(index) = received.windows(4).position(|value| value == b"\r\n\r\n") {
            break index + 4;
        }
        if received.len() >= MAX_HEADER_BYTES {
            return Err(Error::ModelResponseLimitExceeded {
                max_bytes: MAX_HEADER_BYTES,
            });
        }
        let mut buffer = [0_u8; 4096];
        let count = read_checked(
            &mut stream,
            &mut buffer,
            cancellation,
            deadline,
            "reading response headers",
        )?;
        if count == 0 {
            return malformed("response ended before HTTP headers completed");
        }
        received.extend_from_slice(&buffer[..count]);
    };

    let head = std::str::from_utf8(&received[..header_end]).map_err(|_| {
        Error::MalformedModelResponse {
            reason: "HTTP headers are not UTF-8".to_owned(),
        }
    })?;
    let (status, content_length, chunked) = parse_response_head(head)?;
    if !(200..300).contains(&status) {
        return Err(Error::ModelProviderStatus { status });
    }

    let initial = received[header_end..].to_vec();
    if chunked {
        let mut socket = BufferedSocket {
            stream,
            buffered: initial.into(),
        };
        read_chunked_body(
            &mut socket,
            cancellation,
            deadline,
            max_response_bytes,
            on_body,
        )?;
    } else if let Some(length) = content_length {
        if length > max_response_bytes {
            return Err(Error::ModelResponseLimitExceeded {
                max_bytes: max_response_bytes,
            });
        }
        if initial.len() > length {
            return malformed("response contains bytes beyond Content-Length");
        }
        let mut received_body = initial.len();
        if !initial.is_empty() {
            on_body(&initial)?;
        }
        while received_body < length {
            let mut buffer = [0_u8; 8192];
            let count = read_checked(
                &mut stream,
                &mut buffer,
                cancellation,
                deadline,
                "reading response body",
            )?;
            if count == 0 {
                return malformed("response body ended before Content-Length");
            }
            let remaining = length - received_body;
            if count > remaining {
                return malformed("response contains bytes beyond Content-Length");
            }
            on_body(&buffer[..count])?;
            received_body += count;
        }
    } else {
        let mut received_body = initial.len();
        if received_body > max_response_bytes {
            return Err(Error::ModelResponseLimitExceeded {
                max_bytes: max_response_bytes,
            });
        }
        if !initial.is_empty() {
            on_body(&initial)?;
        }
        loop {
            if received_body > max_response_bytes {
                return Err(Error::ModelResponseLimitExceeded {
                    max_bytes: max_response_bytes,
                });
            }
            let mut buffer = [0_u8; 8192];
            let count = read_checked(
                &mut stream,
                &mut buffer,
                cancellation,
                deadline,
                "reading response body",
            )?;
            if count == 0 {
                break;
            }
            if received_body.saturating_add(count) > max_response_bytes {
                return Err(Error::ModelResponseLimitExceeded {
                    max_bytes: max_response_bytes,
                });
            }
            on_body(&buffer[..count])?;
            received_body += count;
        }
    }

    Ok(())
}

fn parse_response_head(head: &str) -> Result<(u16, Option<usize>, bool)> {
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or_else(|| Error::MalformedModelResponse {
        reason: "missing HTTP status line".to_owned(),
    })?;
    let mut status_parts = status_line.split_whitespace();
    let version = status_parts.next().unwrap_or_default();
    let status = status_parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| Error::MalformedModelResponse {
            reason: "invalid HTTP status line".to_owned(),
        })?;
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return malformed("unsupported HTTP version");
    }

    let mut content_length = None;
    let mut chunked = false;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| Error::MalformedModelResponse {
                reason: "malformed HTTP header".to_owned(),
            })?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| Error::MalformedModelResponse {
                    reason: "invalid Content-Length".to_owned(),
                })?;
            if content_length.replace(parsed).is_some() {
                return malformed("duplicate Content-Length");
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if !value.eq_ignore_ascii_case("chunked") {
                return malformed("unsupported Transfer-Encoding");
            }
            chunked = true;
        }
    }
    if chunked && content_length.is_some() {
        return malformed("conflicting HTTP response framing");
    }
    Ok((status, content_length, chunked))
}

fn read_chunked_body(
    socket: &mut BufferedSocket,
    cancellation: &CancellationToken,
    deadline: Instant,
    max_response_bytes: usize,
    on_body: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let mut body_bytes = 0_usize;
    loop {
        let line = socket.read_crlf_line(cancellation, deadline, 1024)?;
        let size_text = line.split(|byte| *byte == b';').next().unwrap_or_default();
        let size_text =
            std::str::from_utf8(size_text).map_err(|_| Error::MalformedModelResponse {
                reason: "chunk size is not ASCII".to_owned(),
            })?;
        let size = usize::from_str_radix(size_text.trim(), 16).map_err(|_| {
            Error::MalformedModelResponse {
                reason: "invalid HTTP chunk size".to_owned(),
            }
        })?;
        if size == 0 {
            let mut trailer_bytes = 0_usize;
            loop {
                let trailer = socket.read_crlf_line(cancellation, deadline, MAX_HEADER_BYTES)?;
                trailer_bytes = trailer_bytes.saturating_add(trailer.len() + 2);
                if trailer_bytes > MAX_HEADER_BYTES {
                    return Err(Error::ModelResponseLimitExceeded {
                        max_bytes: MAX_HEADER_BYTES,
                    });
                }
                if trailer.is_empty() {
                    return Ok(());
                }
            }
        }
        if body_bytes.saturating_add(size) > max_response_bytes {
            return Err(Error::ModelResponseLimitExceeded {
                max_bytes: max_response_bytes,
            });
        }
        let mut remaining = size;
        while remaining > 0 {
            let count = remaining.min(8192);
            let chunk = socket.read_exact(cancellation, deadline, count)?;
            on_body(&chunk)?;
            body_bytes += count;
            remaining -= count;
        }
        if socket.read_exact(cancellation, deadline, 2)? != b"\r\n" {
            return malformed("HTTP chunk is missing CRLF");
        }
    }
}

impl BufferedSocket {
    fn read_byte(&mut self, cancellation: &CancellationToken, deadline: Instant) -> Result<u8> {
        if let Some(byte) = self.buffered.pop_front() {
            return Ok(byte);
        }
        let mut byte = [0_u8; 1];
        let count = read_checked(
            &mut self.stream,
            &mut byte,
            cancellation,
            deadline,
            "reading chunked response",
        )?;
        if count == 0 {
            return malformed("chunked response ended before terminal chunk");
        }
        Ok(byte[0])
    }

    fn read_exact(
        &mut self,
        cancellation: &CancellationToken,
        deadline: Instant,
        length: usize,
    ) -> Result<Vec<u8>> {
        let mut output = Vec::with_capacity(length);
        while output.len() < length {
            let Some(byte) = self.buffered.pop_front() else {
                break;
            };
            output.push(byte);
        }
        while output.len() < length {
            let remaining = length - output.len();
            let mut buffer = [0_u8; 8192];
            let count = read_checked(
                &mut self.stream,
                &mut buffer[..remaining.min(8192)],
                cancellation,
                deadline,
                "reading chunked response body",
            )?;
            if count == 0 {
                return malformed("chunked response ended before chunk completed");
            }
            output.extend_from_slice(&buffer[..count]);
        }
        Ok(output)
    }

    fn read_crlf_line(
        &mut self,
        cancellation: &CancellationToken,
        deadline: Instant,
        max_bytes: usize,
    ) -> Result<Vec<u8>> {
        let mut line = Vec::new();
        loop {
            if line.len() >= max_bytes {
                return Err(Error::ModelResponseLimitExceeded { max_bytes });
            }
            let byte = self.read_byte(cancellation, deadline)?;
            if byte == b'\r' {
                if self.read_byte(cancellation, deadline)? != b'\n' {
                    return malformed("chunked line is missing LF");
                }
                return Ok(line);
            }
            line.push(byte);
        }
    }
}

fn read_checked(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    cancellation: &CancellationToken,
    deadline: Instant,
    operation: &'static str,
) -> Result<usize> {
    loop {
        check_cancelled(cancellation)?;
        check_deadline(deadline)?;
        match stream.read(buffer) {
            Ok(count) => return Ok(count),
            Err(error) if is_retryable_timeout(&error) => {}
            Err(source) => return Err(Error::ModelTransportIo { operation, source }),
        }
    }
}

fn is_retryable_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(Error::ModelTransportCancelled)
    } else {
        Ok(())
    }
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        Err(Error::ModelTransportTimedOut)
    } else {
        Ok(())
    }
}

fn remaining_poll_timeout(deadline: Instant) -> Result<Duration> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or(Error::ModelTransportTimedOut)?;
    Ok(remaining.min(IO_POLL_INTERVAL))
}

fn parse_openai_unary(body: &[u8], request: &ModelRequest) -> Result<ModelTurn> {
    let root = parse_json(body)?;
    let choice = root
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| Error::MalformedModelResponse {
            reason: "OpenAI-compatible response has no choice".to_owned(),
        })?;
    let message = choice
        .get("message")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::MalformedModelResponse {
            reason: "OpenAI-compatible choice has no message".to_owned(),
        })?;
    let text = optional_string(message.get("content"), "message content")?;
    let reasoning = parse_reasoning(message, &["reasoning_content", "reasoning", "thinking"])?;
    let tool_intents = parse_openai_tool_calls(message.get("tool_calls"))?;
    let finish_reason = parse_finish_reason(choice.get("finish_reason"), !tool_intents.is_empty())?;

    Ok(ModelTurn {
        text,
        reasoning,
        tool_intents,
        finish_reason,
        provider: LocalModelProvider::LlamaServer,
        model: optional_bounded_string(root.get("model"), "model identity", 256)?
            .unwrap_or_else(|| request.model.clone()),
        response_id: optional_bounded_string(root.get("id"), "response identity", 256)?,
        provider_request_id: None,
        usage: parse_openai_usage(root.get("usage"))?,
    })
}

fn parse_ollama_unary(body: &[u8], request: &ModelRequest) -> Result<ModelTurn> {
    let root = parse_json(body)?;
    if root.get("done").and_then(Value::as_bool) != Some(true) {
        return malformed("Ollama unary response is not terminal");
    }
    let message = root
        .get("message")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::MalformedModelResponse {
            reason: "Ollama response has no message".to_owned(),
        })?;
    let text = optional_string(message.get("content"), "message content")?;
    let reasoning = parse_reasoning(message, &["thinking", "reasoning"])?;
    let tool_intents = parse_ollama_tool_calls(message.get("tool_calls"), 0)?;
    let finish_reason = parse_finish_reason(root.get("done_reason"), !tool_intents.is_empty())?;

    Ok(ModelTurn {
        text,
        reasoning,
        tool_intents,
        finish_reason,
        provider: LocalModelProvider::Ollama,
        model: optional_bounded_string(root.get("model"), "model identity", 256)?
            .unwrap_or_else(|| request.model.clone()),
        response_id: None,
        provider_request_id: None,
        usage: parse_ollama_usage(&root)?,
    })
}

impl StreamDecoder {
    fn new(provider: LocalModelProvider, request: &ModelRequest) -> Self {
        let state = match provider {
            LocalModelProvider::LlamaServer => StreamState::OpenAi(OpenAiStreamState {
                requested_model: request.model.clone(),
                text: String::new(),
                reasoning: String::new(),
                calls: BTreeMap::new(),
                terminal: false,
                finish_reason: None,
                model: None,
                request_id: None,
                usage: None,
            }),
            LocalModelProvider::Ollama => StreamState::Ollama(OllamaStreamState {
                requested_model: request.model.clone(),
                text: String::new(),
                reasoning: String::new(),
                tool_intents: Vec::new(),
                terminal: false,
                finish_reason: None,
                model: None,
                usage: None,
                provider_ids: HashSet::new(),
            }),
        };
        Self {
            pending: Vec::new(),
            state,
        }
    }

    fn push(
        &mut self,
        bytes: &[u8],
        cancellation: &CancellationToken,
        deadline: Instant,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>,
    ) -> Result<()> {
        self.pending.extend_from_slice(bytes);
        let mut consumed = 0_usize;
        loop {
            check_cancelled(cancellation)?;
            check_deadline(deadline)?;
            let Some(relative_newline) = self.pending[consumed..]
                .iter()
                .position(|byte| *byte == b'\n')
            else {
                if self.pending.len() - consumed > MAX_RECORD_BYTES {
                    return Err(Error::ModelResponseLimitExceeded {
                        max_bytes: MAX_RECORD_BYTES,
                    });
                }
                break;
            };
            if relative_newline > MAX_RECORD_BYTES {
                return Err(Error::ModelResponseLimitExceeded {
                    max_bytes: MAX_RECORD_BYTES,
                });
            }
            let newline = consumed + relative_newline;
            let mut line = self.pending[consumed..newline].to_vec();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.process_line(&line, on_event)?;
            consumed = newline + 1;
        }
        if consumed > 0 {
            self.pending.copy_within(consumed.., 0);
            self.pending.truncate(self.pending.len() - consumed);
        }
        Ok(())
    }

    fn finish(
        mut self,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>,
    ) -> Result<ModelTurn> {
        if !self.pending.is_empty() {
            if self.pending.len() > MAX_RECORD_BYTES {
                return Err(Error::ModelResponseLimitExceeded {
                    max_bytes: MAX_RECORD_BYTES,
                });
            }
            let line = std::mem::take(&mut self.pending);
            self.process_line(&line, on_event)?;
        }
        match self.state {
            StreamState::OpenAi(state) => state.finish(on_event),
            StreamState::Ollama(state) => state.finish(on_event),
        }
    }

    fn process_line(
        &mut self,
        line: &[u8],
        on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>,
    ) -> Result<()> {
        let line = std::str::from_utf8(line).map_err(|_| Error::MalformedModelResponse {
            reason: "stream record is not UTF-8".to_owned(),
        })?;
        match &mut self.state {
            StreamState::OpenAi(state) => state.process_line(line, on_event),
            StreamState::Ollama(state) => state.process_line(line, on_event),
        }
    }
}

impl OpenAiStreamState {
    fn process_line(
        &mut self,
        line: &str,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>,
    ) -> Result<()> {
        if line.is_empty() || line.starts_with(':') {
            return Ok(());
        }
        let Some(data) = line.strip_prefix("data:") else {
            return malformed("SSE record contains an unsupported field");
        };
        let data = data.strip_prefix(' ').unwrap_or(data);
        if data == "[DONE]" {
            self.terminal = true;
            return Ok(());
        }
        if self.terminal {
            return malformed("SSE data followed terminal marker");
        }

        let chunk = parse_json(data.as_bytes())?;
        self.model = optional_bounded_string(chunk.get("model"), "model identity", 256)?
            .or(self.model.take());
        self.request_id = optional_bounded_string(chunk.get("id"), "request identity", 256)?
            .or(self.request_id.take());
        if let Some(usage) = parse_openai_usage(chunk.get("usage"))? {
            self.usage = Some(usage);
        }
        let choices = chunk
            .get("choices")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::MalformedModelResponse {
                reason: "streaming response has no choices array".to_owned(),
            })?;
        for choice in choices {
            if let Some(reason) = choice.get("finish_reason").filter(|value| !value.is_null()) {
                self.finish_reason =
                    Some(parse_finish_reason(Some(reason), !self.calls.is_empty())?);
            }
            let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
                continue;
            };
            if let Some(fragment) = delta.get("content").and_then(Value::as_str) {
                self.text.push_str(fragment);
                on_event(ModelStreamEvent::TextDelta(fragment.to_owned()))?;
            }
            if let Some(fragment) =
                first_string(delta, &["reasoning_content", "reasoning", "thinking"])?
                && !fragment.is_empty()
            {
                self.reasoning.push_str(fragment);
                on_event(ModelStreamEvent::ReasoningDelta(fragment.to_owned()))?;
            }
            merge_openai_tool_deltas(delta.get("tool_calls"), &mut self.calls)?;
        }
        Ok(())
    }

    fn finish(self, on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>) -> Result<ModelTurn> {
        if !self.terminal {
            return malformed("stream ended without terminal record");
        }
        let tool_intents = finish_openai_tool_calls(self.calls)?;
        for intent in &tool_intents {
            on_event(ModelStreamEvent::ToolIntent(intent.clone()))?;
        }
        Ok(ModelTurn {
            text: self.text,
            reasoning: (!self.reasoning.is_empty()).then_some(self.reasoning),
            finish_reason: self
                .finish_reason
                .unwrap_or_else(|| infer_finish_reason(!tool_intents.is_empty())),
            tool_intents,
            provider: LocalModelProvider::LlamaServer,
            model: self.model.unwrap_or(self.requested_model),
            response_id: self.request_id,
            provider_request_id: None,
            usage: self.usage,
        })
    }
}

impl OllamaStreamState {
    fn process_line(
        &mut self,
        line: &str,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>,
    ) -> Result<()> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(());
        }
        if self.terminal {
            return malformed("NDJSON record followed terminal record");
        }
        let chunk = parse_json(line.as_bytes())?;
        self.model = optional_bounded_string(chunk.get("model"), "model identity", 256)?
            .or(self.model.take());
        if let Some(message) = chunk.get("message").and_then(Value::as_object) {
            if let Some(fragment) = first_string(message, &["thinking", "reasoning"])?
                && !fragment.is_empty()
            {
                self.reasoning.push_str(fragment);
                on_event(ModelStreamEvent::ReasoningDelta(fragment.to_owned()))?;
            }
            if let Some(fragment) = message.get("content").and_then(Value::as_str) {
                self.text.push_str(fragment);
                if !fragment.is_empty() {
                    on_event(ModelStreamEvent::TextDelta(fragment.to_owned()))?;
                }
            }
            let mut calls =
                parse_ollama_tool_calls(message.get("tool_calls"), self.tool_intents.len())?;
            for call in &calls {
                if let Some(provider_id) = &call.provider_id
                    && !self.provider_ids.insert(provider_id.clone())
                {
                    return Err(Error::DuplicateToolCall);
                }
            }
            self.tool_intents.append(&mut calls);
        }
        if chunk.get("done").and_then(Value::as_bool) == Some(true) {
            self.terminal = true;
            self.finish_reason = Some(parse_finish_reason(
                chunk.get("done_reason"),
                !self.tool_intents.is_empty(),
            )?);
            self.usage = parse_ollama_usage(&chunk)?;
        }
        Ok(())
    }

    fn finish(self, on_event: &mut dyn FnMut(ModelStreamEvent) -> Result<()>) -> Result<ModelTurn> {
        if !self.terminal {
            return malformed("stream ended without terminal record");
        }
        for intent in &self.tool_intents {
            on_event(ModelStreamEvent::ToolIntent(intent.clone()))?;
        }
        Ok(ModelTurn {
            text: self.text,
            reasoning: (!self.reasoning.is_empty()).then_some(self.reasoning),
            finish_reason: self
                .finish_reason
                .unwrap_or_else(|| infer_finish_reason(!self.tool_intents.is_empty())),
            tool_intents: self.tool_intents,
            provider: LocalModelProvider::Ollama,
            model: self.model.unwrap_or(self.requested_model),
            response_id: None,
            provider_request_id: None,
            usage: self.usage,
        })
    }
}

fn parse_json(body: &[u8]) -> Result<Value> {
    serde_json::from_slice(body).map_err(|_| Error::MalformedModelResponse {
        reason: "provider returned invalid JSON".to_owned(),
    })
}

fn optional_string(value: Option<&Value>, label: &str) -> Result<String> {
    match value {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => malformed(&format!("{label} must be text or null")),
    }
}

fn parse_reasoning(object: &Map<String, Value>, keys: &[&str]) -> Result<Option<String>> {
    first_string(object, keys).map(|value| value.filter(|text| !text.is_empty()).cloned())
}

fn first_string<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Result<Option<&'a String>> {
    for key in keys {
        match object.get(*key) {
            None | Some(Value::Null) => {}
            Some(Value::String(value)) => return Ok(Some(value)),
            Some(_) => return malformed(&format!("message {key} must be text or null")),
        }
    }
    Ok(None)
}

fn optional_bounded_string(
    value: Option<&Value>,
    label: &str,
    max_bytes: usize,
) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value))
            if !value.trim().is_empty()
                && value.len() <= max_bytes
                && !value.chars().any(char::is_control) =>
        {
            Ok(Some(value.clone()))
        }
        Some(Value::String(_)) => malformed(&format!("{label} is invalid")),
        Some(_) => malformed(&format!("{label} must be text")),
    }
}

fn parse_openai_tool_calls(value: Option<&Value>) -> Result<Vec<ToolIntent>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let calls = value
        .as_array()
        .ok_or_else(|| Error::MalformedModelResponse {
            reason: "tool calls must be an array".to_owned(),
        })?;
    let mut output = Vec::with_capacity(calls.len());
    let mut ids = HashSet::new();
    for call in calls {
        let id = required_bounded_string(call.get("id"), "tool call identity", 256)?;
        if !ids.insert(id.clone()) {
            return Err(Error::DuplicateToolCall);
        }
        let function = call
            .get("function")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::MalformedModelResponse {
                reason: "tool call has no function".to_owned(),
            })?;
        let name = required_bounded_string(function.get("name"), "tool name", 128)?;
        validate_provider_tool_name(&name)?;
        let arguments = parse_openai_arguments(function.get("arguments"))?;
        output.push(ToolIntent {
            id: id.clone(),
            provider_id: Some(id),
            name,
            arguments,
        });
    }
    Ok(output)
}

fn merge_openai_tool_deltas(
    value: Option<&Value>,
    calls: &mut BTreeMap<usize, PartialToolCall>,
) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let deltas = value
        .as_array()
        .ok_or_else(|| Error::MalformedModelResponse {
            reason: "stream tool calls must be an array".to_owned(),
        })?;
    for delta in deltas {
        let index = delta
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| Error::MalformedModelResponse {
                reason: "stream tool call has no valid index".to_owned(),
            })?;
        if index >= MAX_TOOLS {
            return malformed("stream tool call index exceeds limit");
        }
        let entry = calls.entry(index).or_default();
        if let Some(id) = optional_bounded_string(delta.get("id"), "tool call identity", 256)? {
            if entry
                .provider_id
                .as_ref()
                .is_some_and(|existing| existing != &id)
            {
                return malformed("stream tool call identity changed");
            }
            entry.provider_id = Some(id);
        }
        if let Some(function) = delta.get("function").and_then(Value::as_object) {
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                entry.name.push_str(name);
                if entry.name.len() > 128 {
                    return malformed("stream tool name exceeds limit");
                }
            }
            if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                entry.arguments.push_str(arguments);
                if entry.arguments.len() > MAX_RECORD_BYTES {
                    return Err(Error::ModelResponseLimitExceeded {
                        max_bytes: MAX_RECORD_BYTES,
                    });
                }
            }
        }
    }
    Ok(())
}

fn finish_openai_tool_calls(calls: BTreeMap<usize, PartialToolCall>) -> Result<Vec<ToolIntent>> {
    let mut output = Vec::with_capacity(calls.len());
    let mut ids = HashSet::new();
    for (_, call) in calls {
        let id = call
            .provider_id
            .ok_or_else(|| Error::MalformedModelResponse {
                reason: "stream tool call has no identity".to_owned(),
            })?;
        if !ids.insert(id.clone()) {
            return Err(Error::DuplicateToolCall);
        }
        validate_provider_tool_name(&call.name)?;
        let arguments = parse_openai_arguments(Some(&Value::String(call.arguments)))?;
        output.push(ToolIntent {
            id: id.clone(),
            provider_id: Some(id),
            name: call.name,
            arguments,
        });
    }
    Ok(output)
}

fn parse_openai_arguments(value: Option<&Value>) -> Result<Value> {
    let arguments = match value {
        Some(Value::String(value)) => {
            serde_json::from_str(value).map_err(|_| Error::MalformedModelResponse {
                reason: "tool arguments are invalid JSON".to_owned(),
            })?
        }
        Some(value @ Value::Object(_)) => value.clone(),
        _ => {
            return malformed("tool arguments must be JSON text or an object");
        }
    };
    if !arguments.is_object() {
        return malformed("tool arguments must be a JSON object");
    }
    Ok(arguments)
}

fn parse_ollama_tool_calls(value: Option<&Value>, offset: usize) -> Result<Vec<ToolIntent>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }
    let calls = value
        .as_array()
        .ok_or_else(|| Error::MalformedModelResponse {
            reason: "tool calls must be an array".to_owned(),
        })?;
    if offset.saturating_add(calls.len()) > MAX_TOOLS {
        return malformed("tool call count exceeds limit");
    }
    let mut output = Vec::with_capacity(calls.len());
    let mut provider_ids = HashSet::new();
    for (index, call) in calls.iter().enumerate() {
        let function = call
            .get("function")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::MalformedModelResponse {
                reason: "tool call has no function".to_owned(),
            })?;
        let name = required_bounded_string(function.get("name"), "tool name", 128)?;
        validate_provider_tool_name(&name)?;
        let arguments = match function.get("arguments") {
            Some(value @ Value::Object(_)) => value.clone(),
            Some(Value::String(value)) => {
                serde_json::from_str(value).map_err(|_| Error::MalformedModelResponse {
                    reason: "tool arguments are invalid JSON".to_owned(),
                })?
            }
            _ => return malformed("tool arguments must be a JSON object"),
        };
        if !arguments.is_object() {
            return malformed("tool arguments must be a JSON object");
        }
        let provider_id = optional_bounded_string(call.get("id"), "tool call identity", 256)?;
        if provider_id
            .as_ref()
            .is_some_and(|id| !provider_ids.insert(id.clone()))
        {
            return Err(Error::DuplicateToolCall);
        }
        let id = provider_id
            .clone()
            .unwrap_or_else(|| format!("ollama-call-{}", offset + index));
        output.push(ToolIntent {
            id,
            provider_id,
            name,
            arguments,
        });
    }
    Ok(output)
}

fn required_bounded_string(value: Option<&Value>, label: &str, max_bytes: usize) -> Result<String> {
    optional_bounded_string(value, label, max_bytes)?.ok_or_else(|| Error::MalformedModelResponse {
        reason: format!("{label} is missing"),
    })
}

fn validate_provider_tool_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return malformed("tool name is invalid");
    }
    Ok(())
}

fn parse_finish_reason(value: Option<&Value>, has_tools: bool) -> Result<FinishReason> {
    let Some(value) = value else {
        return Ok(infer_finish_reason(has_tools));
    };
    if value.is_null() {
        return Ok(infer_finish_reason(has_tools));
    }
    let value = value
        .as_str()
        .ok_or_else(|| Error::MalformedModelResponse {
            reason: "finish reason must be text".to_owned(),
        })?;
    if value.len() > 128 || value.chars().any(char::is_control) {
        return malformed("finish reason is invalid");
    }
    Ok(match value {
        "stop" if has_tools => FinishReason::ToolCalls,
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "tool_calls" | "tool_call" => FinishReason::ToolCalls,
        "content_filter" => FinishReason::ContentFilter,
        other => FinishReason::Other(other.to_owned()),
    })
}

fn infer_finish_reason(has_tools: bool) -> FinishReason {
    if has_tools {
        FinishReason::ToolCalls
    } else {
        FinishReason::Stop
    }
}

fn parse_openai_usage(value: Option<&Value>) -> Result<Option<ModelUsage>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let input_tokens = required_u64(value.get("prompt_tokens"), "prompt token count")?;
    let output_tokens = required_u64(value.get("completion_tokens"), "completion token count")?;
    let total_tokens = value
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| input_tokens.saturating_add(output_tokens));
    Ok(Some(ModelUsage {
        input_tokens,
        output_tokens,
        total_tokens,
    }))
}

fn parse_ollama_usage(root: &Value) -> Result<Option<ModelUsage>> {
    let Some(input_tokens) = root.get("prompt_eval_count").and_then(Value::as_u64) else {
        return Ok(None);
    };
    let output_tokens = required_u64(root.get("eval_count"), "Ollama output token count")?;
    let total_tokens =
        input_tokens
            .checked_add(output_tokens)
            .ok_or_else(|| Error::MalformedModelResponse {
                reason: "token count overflow".to_owned(),
            })?;
    Ok(Some(ModelUsage {
        input_tokens,
        output_tokens,
        total_tokens,
    }))
}

fn required_u64(value: Option<&Value>, label: &str) -> Result<u64> {
    value
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::MalformedModelResponse {
            reason: format!("{label} is missing or invalid"),
        })
}

fn malformed<T>(reason: &str) -> Result<T> {
    Err(Error::MalformedModelResponse {
        reason: reason.to_owned(),
    })
}
