use std::{
    collections::HashSet,
    io::{Read, Write},
    os::{fd::AsFd, unix::process::CommandExt},
    path::PathBuf,
    process::{Child, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

use nix::{
    errno::Errno,
    fcntl::{FcntlArg, OFlag, fcntl},
    poll::{PollFd, PollFlags, poll},
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    CancellationToken, Error, Result, direct_transport::get_bounded, supervisor::SignalCancellation,
};

const MAX_MODELS: usize = 128;
const MAX_MODEL_ID_BYTES: usize = 256;
const MAX_MODEL_NAME_BYTES: usize = 1_024;
const MAX_MODEL_DESCRIPTION_BYTES: usize = 4_096;
const MAX_DISCOVERY_BYTES: usize = 1024 * 1024;
const MAX_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ACP_RECORD_BYTES: usize = 64 * 1024;
const ACP_POLL_MILLISECONDS: u16 = 50;
const ACP_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

/// Provider families with a supported machine-readable model catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogProvider {
    Ollama,
    LlamaServer,
    Copilot,
    Gemini,
}

impl CatalogProvider {
    /// Stable provider value used by the CLI and serialized catalog.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::LlamaServer => "llama-server",
            Self::Copilot => "copilot",
            Self::Gemini => "gemini",
        }
    }

    /// Default numeric-loopback endpoint for HTTP-backed providers.
    pub fn default_endpoint(self) -> Option<&'static str> {
        match self {
            Self::Ollama => Some("http://127.0.0.1:11434"),
            Self::LlamaServer => Some("http://127.0.0.1:9931"),
            Self::Copilot | Self::Gemini => None,
        }
    }

    /// Default executable for ACP-backed providers.
    pub fn default_executable(self) -> Option<&'static str> {
        match self {
            Self::Copilot => Some("copilot"),
            Self::Gemini => Some("gemini"),
            Self::Ollama | Self::LlamaServer => None,
        }
    }
}

/// One validated provider-advertised model descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderModel {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

impl ProviderModel {
    /// Creates one bounded descriptor.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: Option<String>,
    ) -> Result<Self> {
        let model = Self {
            id: id.into(),
            name: name.into(),
            description,
        };
        model.validate()?;
        Ok(model)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn validate(&self) -> Result<()> {
        validate_model_id(&self.id)?;
        validate_display_text(&self.name, MAX_MODEL_NAME_BYTES, "model name", false)?;
        if let Some(description) = &self.description {
            validate_display_text(
                description,
                MAX_MODEL_DESCRIPTION_BYTES,
                "model description",
                true,
            )?;
        }
        Ok(())
    }
}

/// Canonical bounded provider model catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelCatalog {
    format_version: u8,
    provider: CatalogProvider,
    current_model_id: Option<String>,
    models: Vec<ProviderModel>,
}

impl ModelCatalog {
    /// Creates a catalog, retaining the first descriptor for each duplicate ID.
    pub fn new(
        provider: CatalogProvider,
        current_model_id: Option<String>,
        models: Vec<ProviderModel>,
    ) -> Result<Self> {
        let mut seen = HashSet::new();
        let mut retained = Vec::new();
        for model in models {
            model.validate()?;
            if seen.insert(model.id.clone()) {
                if retained.len() == MAX_MODELS {
                    return invalid_catalog("catalog exceeds 128 unique models");
                }
                retained.push(model);
            }
        }
        if retained.is_empty() {
            return invalid_catalog("catalog must contain at least one model");
        }
        let current_model_id =
            current_model_id.filter(|current| retained.iter().any(|model| model.id == *current));
        Ok(Self {
            format_version: 1,
            provider,
            current_model_id,
            models: retained,
        })
    }

    pub fn provider(&self) -> CatalogProvider {
        self.provider
    }

    pub fn current_model_id(&self) -> Option<&str> {
        self.current_model_id.as_deref()
    }

    pub fn models(&self) -> &[ProviderModel] {
        &self.models
    }

    /// Model selected by setup when the provider current model is absent.
    pub fn default_model_id(&self) -> &str {
        self.current_model_id
            .as_deref()
            .unwrap_or_else(|| self.models[0].id())
    }
}

/// Bounded inputs for one catalog discovery operation.
#[derive(Debug, Clone)]
pub struct ModelDiscoveryOptions {
    pub endpoint: Option<String>,
    pub executable: Option<String>,
    pub working_directory: PathBuf,
    pub timeout: Duration,
    pub max_response_bytes: usize,
    pub allow_host_discovery: bool,
}

impl Default for ModelDiscoveryOptions {
    fn default() -> Self {
        Self {
            endpoint: None,
            executable: None,
            working_directory: PathBuf::from("."),
            timeout: Duration::from_secs(5),
            max_response_bytes: 64 * 1024,
            allow_host_discovery: false,
        }
    }
}

/// Discovers one provider catalog without sending an inference prompt.
pub fn discover_models(
    provider: CatalogProvider,
    options: &ModelDiscoveryOptions,
    cancellation: &CancellationToken,
) -> Result<ModelCatalog> {
    validate_options(options)?;
    let _signals = SignalCancellation::new_with_token(cancellation.clone())?;
    match provider {
        CatalogProvider::Ollama | CatalogProvider::LlamaServer => {
            discover_http(provider, options, cancellation)
        }
        CatalogProvider::Copilot | CatalogProvider::Gemini => {
            if !options.allow_host_discovery {
                return Err(Error::HostDiscoveryNotAcknowledged);
            }
            discover_acp(provider, options, cancellation)
        }
    }
}

fn validate_options(options: &ModelDiscoveryOptions) -> Result<()> {
    if options.timeout.is_zero() || options.timeout > MAX_DISCOVERY_TIMEOUT {
        return invalid_catalog("discovery timeout must be positive and at most 30 seconds");
    }
    if options.max_response_bytes == 0 || options.max_response_bytes > MAX_DISCOVERY_BYTES {
        return invalid_catalog("discovery response limit must be between 1 and 1048576 bytes");
    }
    Ok(())
}

fn discover_http(
    provider: CatalogProvider,
    options: &ModelDiscoveryOptions,
    cancellation: &CancellationToken,
) -> Result<ModelCatalog> {
    let endpoint = options
        .endpoint
        .as_deref()
        .or_else(|| provider.default_endpoint())
        .ok_or_else(|| Error::ModelCatalogInvalid {
            reason: "HTTP catalog provider has no endpoint".to_owned(),
        })?;
    let path = match provider {
        CatalogProvider::Ollama => "/api/tags",
        CatalogProvider::LlamaServer => "/v1/models",
        CatalogProvider::Copilot | CatalogProvider::Gemini => {
            return invalid_catalog("ACP provider cannot use HTTP discovery");
        }
    };
    let body = get_bounded(
        endpoint,
        path,
        options.timeout,
        options.max_response_bytes,
        cancellation,
    )?;
    let root = serde_json::from_slice::<Value>(&body)
        .map_err(|_| catalog_malformed(provider, "response is not valid JSON"))?;
    let models = match provider {
        CatalogProvider::Ollama => parse_ollama_models(&root)?,
        CatalogProvider::LlamaServer => parse_llama_server_models(&root)?,
        CatalogProvider::Copilot | CatalogProvider::Gemini => {
            return invalid_catalog("ACP provider cannot use HTTP catalog parsing");
        }
    };
    ModelCatalog::new(provider, None, models)
}

fn parse_ollama_models(root: &Value) -> Result<Vec<ProviderModel>> {
    let entries = root
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| catalog_malformed(CatalogProvider::Ollama, "missing models array"))?;
    entries
        .iter()
        .map(|entry| {
            let id = entry.get("name").and_then(Value::as_str).ok_or_else(|| {
                catalog_malformed(CatalogProvider::Ollama, "model entry has no string name")
            })?;
            ProviderModel::new(id, id, ollama_description(entry.get("details")))
        })
        .collect()
}

fn ollama_description(details: Option<&Value>) -> Option<String> {
    let details = details?.as_object()?;
    let mut values = Vec::new();
    for (label, key) in [
        ("family", "family"),
        ("parameters", "parameter_size"),
        ("quantization", "quantization_level"),
        ("format", "format"),
    ] {
        if let Some(value) = details.get(key).and_then(Value::as_str)
            && !value.is_empty()
        {
            values.push(format!("{label}: {value}"));
        }
    }
    (!values.is_empty()).then(|| values.join(", "))
}

fn parse_llama_server_models(root: &Value) -> Result<Vec<ProviderModel>> {
    let entries = root
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| catalog_malformed(CatalogProvider::LlamaServer, "missing data array"))?;
    entries
        .iter()
        .map(|entry| {
            let id = entry.get("id").and_then(Value::as_str).ok_or_else(|| {
                catalog_malformed(CatalogProvider::LlamaServer, "model entry has no string id")
            })?;
            ProviderModel::new(id, id, None)
        })
        .collect()
}

fn discover_acp(
    provider: CatalogProvider,
    options: &ModelDiscoveryOptions,
    cancellation: &CancellationToken,
) -> Result<ModelCatalog> {
    if cancellation.is_cancelled() {
        return Err(Error::ModelTransportCancelled);
    }
    let executable = options
        .executable
        .as_deref()
        .or_else(|| provider.default_executable())
        .ok_or_else(|| Error::ModelCatalogInvalid {
            reason: "ACP catalog provider has no executable".to_owned(),
        })?;
    if executable.is_empty() || executable.as_bytes().contains(&0) {
        return invalid_catalog("ACP executable must not be empty or contain NUL");
    }
    let cwd = std::fs::canonicalize(&options.working_directory).map_err(|source| Error::Io {
        operation: "resolve ACP discovery working directory",
        path: options.working_directory.clone(),
        source,
    })?;
    if !cwd.is_absolute() {
        return invalid_catalog("ACP discovery working directory must resolve absolutely");
    }

    let mut command = Command::new(executable);
    command
        .arg("--acp")
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command.spawn().map_err(|source| Error::Io {
        operation: "spawn ACP catalog provider",
        path: PathBuf::from(executable),
        source,
    })?;
    let stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            terminate_child(&mut child, executable)?;
            return Err(Error::Io {
                operation: "open ACP provider stdin",
                path: PathBuf::from(executable),
                source: std::io::Error::other("ACP stdin was not piped"),
            });
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_child(&mut child, executable)?;
            return Err(Error::Io {
                operation: "open ACP provider stdout",
                path: PathBuf::from(executable),
                source: std::io::Error::other("ACP stdout was not piped"),
            });
        }
    };
    let mut process =
        AcpProcess::new(child, stdin, stdout, executable, options.max_response_bytes)?;
    let deadline = Instant::now() + options.timeout;

    let result = (|| {
        process.send(&json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": 1,
                "clientCapabilities": {}
            }
        }))?;
        let initialize = process.read_response(0, deadline, cancellation)?;
        if initialize
            .get("result")
            .and_then(|result| result.get("protocolVersion"))
            .and_then(Value::as_u64)
            != Some(1)
        {
            return Err(catalog_malformed(
                provider,
                "initialize response did not confirm protocolVersion 1",
            ));
        }

        process.send(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "session/new",
            "params": {
                "cwd": cwd.to_string_lossy(),
                "mcpServers": []
            }
        }))?;
        let session = process.read_response(1, deadline, cancellation)?;
        parse_acp_catalog(provider, &session)
    })();

    let cleanup = process.terminate();
    match (result, cleanup) {
        (Ok(catalog), Ok(())) => Ok(catalog),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn parse_acp_catalog(provider: CatalogProvider, response: &Value) -> Result<ModelCatalog> {
    let result = response
        .get("result")
        .and_then(Value::as_object)
        .ok_or_else(|| catalog_malformed(provider, "session/new response has no result object"))?;
    let models = result
        .get("models")
        .and_then(Value::as_object)
        .ok_or_else(|| catalog_malformed(provider, "session/new response has no models object"))?;
    let entries = models
        .get("availableModels")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            catalog_malformed(provider, "session/new models has no availableModels array")
        })?;
    let parsed = entries
        .iter()
        .map(|entry| {
            let id = entry
                .get("modelId")
                .and_then(Value::as_str)
                .ok_or_else(|| catalog_malformed(provider, "ACP model has no string modelId"))?;
            let name = entry.get("name").and_then(Value::as_str).unwrap_or(id);
            let description = match entry.get("description") {
                None | Some(Value::Null) => None,
                Some(Value::String(value)) => Some(value.clone()),
                Some(_) => {
                    return Err(catalog_malformed(
                        provider,
                        "ACP model description is not a string",
                    ));
                }
            };
            ProviderModel::new(id, name, description)
        })
        .collect::<Result<Vec<_>>>()?;
    let current = match models.get("currentModelId") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return Err(catalog_malformed(
                provider,
                "ACP currentModelId is not a string or null",
            ));
        }
    };
    ModelCatalog::new(provider, current, parsed)
}

struct AcpProcess {
    child: Option<Child>,
    stdin: std::process::ChildStdin,
    stdout: ChildStdout,
    pending: Vec<u8>,
    received: usize,
    max_received: usize,
    executable: PathBuf,
}

impl AcpProcess {
    fn new(
        child: Child,
        stdin: std::process::ChildStdin,
        stdout: ChildStdout,
        executable: &str,
        max_received: usize,
    ) -> Result<Self> {
        let process = Self {
            child: Some(child),
            stdin,
            stdout,
            pending: Vec::new(),
            received: 0,
            max_received,
            executable: PathBuf::from(executable),
        };
        let flags = fcntl(&process.stdout, FcntlArg::F_GETFL).map_err(|error| Error::Io {
            operation: "inspect ACP provider stdout",
            path: process.executable.clone(),
            source: std::io::Error::from_raw_os_error(error as i32),
        })?;
        fcntl(
            &process.stdout,
            FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
        )
        .map_err(|error| Error::Io {
            operation: "configure ACP provider stdout",
            path: process.executable.clone(),
            source: std::io::Error::from_raw_os_error(error as i32),
        })?;
        Ok(process)
    }

    fn send(&mut self, value: &Value) -> Result<()> {
        let mut record = serde_json::to_vec(value).map_err(|_| Error::ModelCatalogInvalid {
            reason: "cannot serialize ACP request".to_owned(),
        })?;
        record.push(b'\n');
        self.stdin
            .write_all(&record)
            .and_then(|()| self.stdin.flush())
            .map_err(|source| Error::Io {
                operation: "write ACP provider request",
                path: self.executable.clone(),
                source,
            })
    }

    fn read_response(
        &mut self,
        expected_id: u64,
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> Result<Value> {
        loop {
            let value = self.read_record(deadline, cancellation)?;
            if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
                return Err(Error::ModelCatalogInvalid {
                    reason: "ACP response does not declare jsonrpc 2.0".to_owned(),
                });
            }
            if value.get("method").is_some() && value.get("id").is_none() {
                continue;
            }
            if value.get("method").is_some() {
                return Err(Error::ModelCatalogInvalid {
                    reason: "ACP provider-to-client requests are not supported during discovery"
                        .to_owned(),
                });
            }
            let id = value.get("id").and_then(Value::as_u64).ok_or_else(|| {
                Error::ModelCatalogInvalid {
                    reason: "ACP response has no numeric response id".to_owned(),
                }
            })?;
            if id != expected_id {
                return Err(Error::ModelCatalogInvalid {
                    reason: format!(
                        "ACP response id {id} did not match expected response id {expected_id}"
                    ),
                });
            }
            if value.get("error").is_some() {
                return Err(Error::ModelCatalogInvalid {
                    reason: "ACP provider returned a protocol error".to_owned(),
                });
            }
            return Ok(value);
        }
    }

    fn read_record(
        &mut self,
        deadline: Instant,
        cancellation: &CancellationToken,
    ) -> Result<Value> {
        loop {
            if cancellation.is_cancelled() {
                return Err(Error::ModelTransportCancelled);
            }
            if Instant::now() >= deadline {
                return Err(Error::ModelDiscoveryTimedOut);
            }
            if let Some(index) = self.pending.iter().position(|byte| *byte == b'\n') {
                let mut record = self.pending.drain(..=index).collect::<Vec<_>>();
                record.pop();
                if record.last() == Some(&b'\r') {
                    record.pop();
                }
                if record.is_empty() {
                    continue;
                }
                if record.len() > MAX_ACP_RECORD_BYTES {
                    return Err(Error::ModelCatalogInvalid {
                        reason: format!("ACP record exceeded {MAX_ACP_RECORD_BYTES} bytes"),
                    });
                }
                return serde_json::from_slice(&record).map_err(|_| Error::ModelCatalogInvalid {
                    reason: "ACP provider emitted malformed JSON".to_owned(),
                });
            }
            if self.pending.len() > MAX_ACP_RECORD_BYTES {
                return Err(Error::ModelCatalogInvalid {
                    reason: format!("ACP record exceeded {MAX_ACP_RECORD_BYTES} bytes"),
                });
            }
            let mut descriptors = [PollFd::new(
                self.stdout.as_fd(),
                PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR,
            )];
            match poll(&mut descriptors, ACP_POLL_MILLISECONDS) {
                Ok(0) | Err(Errno::EINTR) => continue,
                Err(error) => {
                    return Err(Error::Io {
                        operation: "poll ACP provider stdout",
                        path: self.executable.clone(),
                        source: std::io::Error::from_raw_os_error(error as i32),
                    });
                }
                Ok(_) => {}
            }
            let mut buffer = [0_u8; 4096];
            match self.stdout.read(&mut buffer) {
                Ok(0) => {
                    return Err(Error::ModelCatalogInvalid {
                        reason: "ACP provider ended before a complete correlated response"
                            .to_owned(),
                    });
                }
                Ok(count) => {
                    self.received = self.received.saturating_add(count);
                    if self.received > self.max_received {
                        return Err(Error::ModelCatalogInvalid {
                            reason: format!(
                                "ACP output exceeded the {}-byte discovery limit",
                                self.max_received
                            ),
                        });
                    }
                    self.pending.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(source) => {
                    return Err(Error::Io {
                        operation: "read ACP provider stdout",
                        path: self.executable.clone(),
                        source,
                    });
                }
            }
        }
    }

    fn terminate(&mut self) -> Result<()> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        let pid = i32::try_from(child.id()).map_err(|_| Error::Io {
            operation: "identify ACP provider process group",
            path: self.executable.clone(),
            source: std::io::Error::other("ACP process identifier exceeds Linux pid range"),
        })?;
        match killpg(Pid::from_raw(pid), Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Io {
                    operation: "terminate ACP provider process group",
                    path: self.executable.clone(),
                    source: std::io::Error::from_raw_os_error(error as i32),
                });
            }
        }
        child.wait().map_err(|source| Error::Io {
            operation: "reap ACP provider process group",
            path: self.executable.clone(),
            source,
        })?;
        self.drain_terminated_output()
    }

    fn drain_terminated_output(&mut self) -> Result<()> {
        let deadline = Instant::now() + ACP_OUTPUT_DRAIN_TIMEOUT;
        let mut buffer = [0_u8; 4096];
        loop {
            if Instant::now() >= deadline {
                return Err(Error::OutputStreamsRetained);
            }
            match self.stdout.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(source) => {
                    return Err(Error::Io {
                        operation: "drain terminated ACP provider stdout",
                        path: self.executable.clone(),
                        source,
                    });
                }
            }
            let mut descriptors = [PollFd::new(
                self.stdout.as_fd(),
                PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR,
            )];
            match poll(&mut descriptors, ACP_POLL_MILLISECONDS) {
                Ok(_) | Err(Errno::EINTR) => {}
                Err(error) => {
                    return Err(Error::Io {
                        operation: "poll terminated ACP provider stdout",
                        path: self.executable.clone(),
                        source: std::io::Error::from_raw_os_error(error as i32),
                    });
                }
            }
        }
    }
}

impl Drop for AcpProcess {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

fn validate_model_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_MODEL_ID_BYTES
        || !value
            .bytes()
            .all(|byte| (b' '..=b'~').contains(&byte) && !matches!(byte, b'{' | b'}'))
    {
        return invalid_catalog("model ID must be 1-256 printable ASCII bytes without braces");
    }
    Ok(())
}

fn validate_display_text(
    value: &str,
    max_bytes: usize,
    label: &str,
    allow_empty: bool,
) -> Result<()> {
    if (!allow_empty && value.is_empty())
        || value.len() > max_bytes
        || value.chars().any(char::is_control)
    {
        return invalid_catalog(&format!(
            "{label} must be nonempty bounded UTF-8 without control characters"
        ));
    }
    Ok(())
}

fn catalog_malformed(provider: CatalogProvider, reason: &str) -> Error {
    Error::ModelCatalogInvalid {
        reason: format!("{} catalog is malformed: {reason}", provider.as_str()),
    }
}

fn terminate_child(child: &mut Child, executable: &str) -> Result<()> {
    let pid = i32::try_from(child.id()).map_err(|_| Error::Io {
        operation: "identify ACP provider process group",
        path: PathBuf::from(executable),
        source: std::io::Error::other("ACP process identifier exceeds Linux pid range"),
    })?;
    match killpg(Pid::from_raw(pid), Signal::SIGKILL) {
        Ok(()) | Err(Errno::ESRCH) => {}
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Io {
                operation: "terminate ACP provider process group",
                path: PathBuf::from(executable),
                source: std::io::Error::from_raw_os_error(error as i32),
            });
        }
    }
    child.wait().map_err(|source| Error::Io {
        operation: "reap ACP provider process group",
        path: PathBuf::from(executable),
        source,
    })?;
    Ok(())
}

fn invalid_catalog<T>(reason: &str) -> Result<T> {
    Err(Error::ModelCatalogInvalid {
        reason: reason.to_owned(),
    })
}
