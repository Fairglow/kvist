//! Shared, filesystem-only provider/profile configuration and pinned resolution.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{DirectModelTransport, Error, LocalModelProvider, Result};

#[path = "registry_toml.rs"]
mod encoding;

/// Hard bound for both encoded registries and frontend native overrides.
pub const MAX_REGISTRY_BYTES: u64 = crate::MAX_PROFILE_CONFIG_BYTES;
/// Maximum number of named profiles after source precedence is applied.
pub const MAX_PROFILES: usize = 128;

/// Validates the shared selector syntax used by registry names and local aliases.
pub fn validate_profile_name(name: &str) -> Result<()> {
    if crate::profile::is_valid_profile_name(name) {
        Ok(())
    } else {
        Err(invalid(
            "invalid profile name; use 1-128 ASCII letters, digits, '.', '_', '-' or ':'",
        ))
    }
}

/// Native HTTP options shared by all consumers, including explicit overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeModel {
    pub provider: LocalModelProvider,
    #[serde(default)]
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub context_limit: Option<usize>,
    #[serde(default)]
    pub response_reserve: Option<u32>,
    #[serde(default = "deadline")]
    pub deadline_secs: u64,
    #[serde(default = "attempts")]
    pub max_attempts: u32,
    #[serde(default = "base_delay")]
    pub retry_base_delay_secs: u64,
    #[serde(default = "max_delay")]
    pub retry_max_delay_secs: u64,
    #[serde(default = "cadence")]
    pub cadence_timeout_secs: u64,
}

const fn deadline() -> u64 {
    300
}
const fn attempts() -> u32 {
    3
}
const fn base_delay() -> u64 {
    2
}
const fn max_delay() -> u64 {
    30
}
const fn cadence() -> u64 {
    30
}

impl NativeModel {
    /// Builds defaults without probing the endpoint or executable.
    pub fn new(provider: LocalModelProvider, base_url: String, model: String) -> Self {
        Self {
            provider,
            base_url,
            model,
            context_limit: None,
            response_reserve: None,
            deadline_secs: deadline(),
            max_attempts: attempts(),
            retry_base_delay_secs: base_delay(),
            retry_max_delay_secs: max_delay(),
            cadence_timeout_secs: cadence(),
        }
    }

    /// Normalizes the default endpoint and checks all shared transport limits.
    pub fn validate(&mut self) -> Result<()> {
        if self.base_url.is_empty() {
            self.base_url = self.provider.default_endpoint().to_owned();
        }
        crate::direct_transport::validate_model_selector(&self.model)?;
        if !(1..=600).contains(&self.deadline_secs)
            || self.cadence_timeout_secs > 600
            || !(1..=32).contains(&self.max_attempts)
            || self.retry_base_delay_secs > self.retry_max_delay_secs
            || self.retry_max_delay_secs > 3600
        {
            return Err(invalid("invalid native deadline, cadence or retry bounds"));
        }
        if self
            .context_limit
            .is_some_and(|n| !(2..=1_048_576).contains(&n))
            || self
                .response_reserve
                .is_some_and(|n| !(1..=1_048_576).contains(&n))
            || self
                .context_limit
                .zip(self.response_reserve)
                .is_some_and(|(context, reserve)| context <= reserve as usize)
        {
            return Err(invalid(
                "context_limit must be bounded and exceed response_reserve",
            ));
        }
        self.build_transport().map(|_| ())
    }

    /// Constructs a native transport; performs validation but no network I/O.
    pub fn transport(&self) -> Result<DirectModelTransport> {
        let mut validated = self.clone();
        validated.validate()?;
        validated.build_transport()
    }

    /// Applies stricter host wall/output limits without changing the pinned backend.
    pub fn transport_bounded(
        &self,
        remaining: Duration,
        max_output_bytes: usize,
    ) -> Result<DirectModelTransport> {
        let mut validated = self.clone();
        validated.validate()?;
        validated.build_transport_bounded(remaining, max_output_bytes)
    }

    fn build_transport(&self) -> Result<DirectModelTransport> {
        self.build_transport_bounded(Duration::from_secs(self.deadline_secs), 8 * 1024 * 1024)
    }

    fn build_transport_bounded(
        &self,
        remaining: Duration,
        max_output_bytes: usize,
    ) -> Result<DirectModelTransport> {
        let deadline = Duration::from_secs(self.deadline_secs).min(remaining);
        let mut transport = DirectModelTransport::with_watchdogs(
            self.provider,
            &self.base_url,
            deadline,
            max_output_bytes,
            deadline,
            deadline,
        )?;
        if self.cadence_timeout_secs > 0 {
            transport =
                transport.with_cadence_timeout(Duration::from_secs(self.cadence_timeout_secs));
        }
        Ok(transport)
    }
}

impl LocalModelProvider {
    /// Standard local endpoint; no discovery is performed.
    pub const fn default_endpoint(self) -> &'static str {
        match self {
            Self::LlamaServer => "http://127.0.0.1:9931",
            Self::Ollama => "http://127.0.0.1:11434",
        }
    }

    /// Shared transport identity (also retained for frontend callers).
    pub const fn to_agent_provider(self) -> Self {
        self
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ollama" => Some(Self::Ollama),
            "llama-server" => Some(Self::LlamaServer),
            _ => None,
        }
    }
}

impl std::fmt::Display for LocalModelProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ollama => "ollama",
            Self::LlamaServer => "llama-server",
        })
    }
}

impl std::str::FromStr for LocalModelProvider {
    type Err = String;
    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse(value).ok_or_else(|| format!("invalid local provider `{value}`"))
    }
}

/// External agents own their loop and are never model transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalProvider {
    Copilot,
    Gemini,
    Claude,
    LlamaCli,
    CustomScript,
}

impl ExternalProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Copilot => "copilot",
            Self::Gemini => "gemini",
            Self::Claude => "claude",
            Self::LlamaCli => "llama-cli",
            Self::CustomScript => "custom-script",
        }
    }
}

/// Literal executable and argv. Only an explicit execution host may run these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalAgent {
    pub provider: ExternalProvider,
    pub executable: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

impl ExternalAgent {
    /// Renders literal argv without a shell. Construction grants no execution
    /// authority: the caller still owns supervision and acknowledgement.
    pub fn command_spec(
        &self,
        prompt: &str,
        context: &[PathBuf],
        directory: &Path,
        effort: Option<crate::ReasoningEffort>,
    ) -> Result<crate::CommandSpec> {
        Profile::new(Backend::ExternalAgent(self.clone())).validate()?;
        if prompt.len() as u64 > crate::MAX_PROMPT_BYTES || prompt.contains('\0') {
            return Err(invalid("external prompt exceeds its bound or contains NUL"));
        }
        let path_text = |path: &Path| {
            path.to_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("external argv paths must be UTF-8"))
        };
        let mut arguments = Vec::new();
        for argument in &self.arguments {
            if argument == "{context_files}" {
                for path in context {
                    arguments.push(path_text(path)?);
                }
            } else {
                let mut rendered = String::new();
                let mut remaining = argument.as_str();
                while let Some(start) = remaining.find('{') {
                    rendered.push_str(&remaining[..start]);
                    let tail = &remaining[start..];
                    let Some(end) = tail.find('}') else {
                        remaining = tail;
                        break;
                    };
                    let placeholder = &tail[..=end];
                    match placeholder {
                        "{prompt}" => rendered.push_str(prompt),
                        "{model}" => rendered.push_str(self.model.as_deref().unwrap_or("")),
                        "{cwd}" => rendered.push_str(&path_text(directory)?),
                        "{reasoning_effort}" => rendered
                            .push_str(effort.unwrap_or(crate::ReasoningEffort::Medium).as_str()),
                        _ => rendered.push_str(placeholder),
                    }
                    remaining = &tail[end + 1..];
                }
                rendered.push_str(remaining);
                arguments.push(rendered);
            }
        }
        if arguments.len() > 256
            || arguments.iter().map(String::len).sum::<usize>() > 2 * 1024 * 1024
            || arguments.iter().any(|argument| argument.contains('\0'))
        {
            return Err(invalid(
                "rendered external argv exceeds its bounds or contains NUL",
            ));
        }
        Ok(crate::CommandSpec::new(self.executable.clone(), arguments).in_directory(directory))
    }
}

/// Typed backend identity; no command-string inference or implicit execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Backend {
    Native(NativeModel),
    ExternalAgent(ExternalAgent),
}

/// One canonical reusable definition, independent of role/tool authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub advertised: Vec<String>,
    #[serde(default)]
    pub tested: Vec<String>,
    #[serde(default)]
    pub policy_enabled: Vec<String>,
    pub backend: Backend,
}

const fn enabled() -> bool {
    true
}

impl Profile {
    pub fn new(backend: Backend) -> Self {
        Self {
            enabled: true,
            advertised: Vec::new(),
            tested: Vec::new(),
            policy_enabled: Vec::new(),
            backend,
        }
    }

    pub fn validate(&mut self) -> Result<()> {
        for list in [&self.advertised, &self.tested, &self.policy_enabled] {
            if list.len() > 32 || list.iter().collect::<BTreeSet<_>>().len() != list.len() {
                return Err(invalid(
                    "capability lists require at most 32 unique identifiers",
                ));
            }
            for capability in list {
                if !crate::profile::is_valid_profile_name(capability) {
                    return Err(invalid("invalid capability identifier"));
                }
            }
        }
        match &mut self.backend {
            Backend::Native(model) => model.validate(),
            Backend::ExternalAgent(agent) => {
                bounded_text(&agent.executable, 4096, "external executable")?;
                if agent.arguments.len() > 128 {
                    return Err(invalid("external argv exceeds 128 entries"));
                }
                for argument in &agent.arguments {
                    if argument.len() > 16_384
                        || argument.contains('\0')
                        || secret_argument(argument)
                    {
                        return Err(invalid(
                            "external arguments must be bounded, NUL-free and contain no credentials",
                        ));
                    }
                }
                if let Some(model) = &agent.model {
                    bounded_text(model, 1024, "external model")?;
                }
                Ok(())
            }
        }
    }
}

fn secret_argument(argument: &str) -> bool {
    let argument = argument.to_ascii_lowercase();
    [
        "api-key",
        "api_key",
        "access-token",
        "access_token",
        "password",
        "authorization",
        "bearer ",
        "--token",
        "--secret",
        "credential",
        "private_key",
        "private-key",
        "client_secret",
        "client-secret",
        "cookie",
    ]
    .iter()
    .any(|secret| argument.contains(secret))
}

fn bounded_text(value: &str, limit: usize, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{field} must be nonblank, bounded and control-free"
        )));
    }
    Ok(())
}

/// Immutable selection with exact source identity retained for approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedProfile {
    pub name: String,
    pub profile: Profile,
    pub source: PathBuf,
    pub digest: String,
}

impl ResolvedProfile {
    /// Native options or an explicit external-agent refusal.
    pub fn native(&self) -> Result<&NativeModel> {
        match &self.profile.backend {
            Backend::Native(model) => Ok(model),
            Backend::ExternalAgent(_) => Err(invalid(
                "external-agent profile is not a native model transport",
            )),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema_version: u32,
    #[serde(default)]
    profiles: BTreeMap<String, Profile>,
}

/// Validated ordered profiles. Loading is a pure filesystem operation.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    profiles: BTreeMap<String, ResolvedProfile>,
}

impl Registry {
    pub fn parse(path: &Path, contents: &str) -> Result<Self> {
        Self::parse_inner(path, contents).map_err(|error| Error::InvalidProfileConfiguration {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })
    }

    fn parse_inner(path: &Path, contents: &str) -> Result<Self> {
        if contents.len() as u64 > MAX_REGISTRY_BYTES {
            return Err(invalid("registry exceeds 64 KiB"));
        }
        let document = contents
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| invalid("invalid registry TOML"))?;
        let decoded: Document = serde_json::from_value(encoding::decode(document.as_item())?)
            .map_err(|_| {
                invalid("invalid registry fields or types; check the schema-one registry contract")
            })?;
        if decoded.schema_version != 1 || decoded.profiles.len() > MAX_PROFILES {
            return Err(invalid(
                "registry requires schema_version = 1 and at most 128 profiles",
            ));
        }
        let digest = format!(
            "sha256:{}",
            hex::encode(Sha256::digest(contents.as_bytes()))
        );
        let mut profiles = BTreeMap::new();
        for (name, mut profile) in decoded.profiles {
            if !crate::profile::is_valid_profile_name(&name) {
                return Err(invalid("invalid profile name"));
            }
            profile.validate()?;
            profiles.insert(
                name.clone(),
                ResolvedProfile {
                    name,
                    profile,
                    source: path.to_path_buf(),
                    digest: digest.clone(),
                },
            );
        }
        Ok(Self { profiles })
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::parse(path, &crate::profile::read_configuration(path)?)
    }

    pub fn resolve(&self, name: &str) -> Result<ResolvedProfile> {
        let profile = self
            .profiles
            .get(name)
            .ok_or_else(|| invalid(format!("shared profile `{name}` not found")))?;
        if !profile.profile.enabled {
            return Err(invalid(format!("shared profile `{name}` is disabled")));
        }
        Ok(profile.clone())
    }

    /// Includes disabled profiles for configuration inspection, in name order.
    pub fn profiles(&self) -> impl Iterator<Item = &ResolvedProfile> {
        self.profiles.values()
    }
}

/// Complete definitions override by name: explicit > project > user.
#[derive(Debug, Clone, Default)]
pub struct RegistryPaths {
    pub explicit: Option<PathBuf>,
    pub project: Option<PathBuf>,
    pub user: Option<PathBuf>,
}

impl RegistryPaths {
    pub fn for_project(project: &Path) -> Self {
        Self {
            explicit: None,
            project: Some(project.join(".kvist/providers.toml")),
            user: default_registry_path(),
        }
    }

    pub fn load(&self) -> Result<Registry> {
        let mut registry = Registry::default();
        for (path, required) in [
            (&self.user, false),
            (&self.project, false),
            (&self.explicit, true),
        ] {
            let Some(path) = path else { continue };
            match std::fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => continue,
                _ => {}
            }
            registry.profiles.extend(Registry::load(path)?.profiles);
            if registry.profiles.len() > MAX_PROFILES {
                return Err(invalid("merged registry exceeds 128 profiles"));
            }
        }
        Ok(registry)
    }
}

/// Default canonical user store; no retired location fallback.
pub fn default_registry_path() -> Option<PathBuf> {
    let absolute = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    absolute("XDG_CONFIG_HOME")
        .or_else(|| absolute("HOME").map(|p| p.join(".config")))
        .map(|base| base.join("sav/registry.toml"))
}

/// Atomically create or replace a complete named profile.
pub fn upsert(path: &Path, name: &str, profile: &Profile) -> Result<()> {
    if !crate::profile::is_valid_profile_name(name) {
        return Err(invalid("invalid profile name"));
    }
    let mut profile = profile.clone();
    profile.validate()?;
    let (mut document, existed) = match std::fs::symlink_metadata(path) {
        Ok(_) => {
            let contents = crate::profile::read_configuration(path)?;
            Registry::parse(path, &contents)?;
            (
                contents
                    .parse::<toml_edit::DocumentMut>()
                    .map_err(|_| invalid("invalid registry TOML"))?,
                true,
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut document = toml_edit::DocumentMut::new();
            document["schema_version"] = toml_edit::value(1);
            (document, false)
        }
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect registry",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if document.get("profiles").is_none() {
        document["profiles"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    document["profiles"][name] = encoding::encode(
        serde_json::to_value(profile).map_err(|_| invalid("cannot encode profile"))?,
    )?;
    let contents = document.to_string();
    Registry::parse(path, &contents)?;
    crate::profile::persist_configuration(path, contents.as_bytes(), existed)
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::ProfileSetup {
        reason: reason.into(),
    }
}
