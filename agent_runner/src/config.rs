//! Configuration model, loading, and validation.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, io_error};
use crate::toolchain::{ProfileSetting, ToolProfile};

/// Maximum encoded size of a configuration file.
pub const MAX_CONFIG_BYTES: u64 = 64 * 1024;
/// The only accepted configuration schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// Minimum per-turn model deadline in seconds.
const MIN_DEADLINE_SECS: u64 = 1;
/// Maximum per-turn model deadline in seconds.
const MAX_DEADLINE_SECS: u64 = 600;

pub(crate) fn resolve_working_directory(path: &Path) -> Result<PathBuf> {
    let resolved = path.canonicalize().map_err(|source| {
        io_error(
            "resolve working directory",
            Some(&path.to_string_lossy()),
            source,
        )
    })?;
    if !resolved.is_dir() {
        return Err(Error::InvalidPath {
            path: resolved.display().to_string(),
            reason: "not a directory".into(),
        });
    }
    Ok(resolved)
}

/// The local model provider a model talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelProvider {
    /// llama-server's OpenAI-compatible HTTP protocol.
    LlamaServer,
    /// Ollama's native HTTP protocol.
    Ollama,
}

impl ModelProvider {
    /// Maps the configuration provider to the agent-runtime transport provider.
    pub const fn to_agent_provider(self) -> agent_runtime::LocalModelProvider {
        match self {
            ModelProvider::LlamaServer => agent_runtime::LocalModelProvider::LlamaServer,
            ModelProvider::Ollama => agent_runtime::LocalModelProvider::Ollama,
        }
    }

    /// The base URL the provider listens on when no override is configured.
    pub const fn default_endpoint(self) -> &'static str {
        match self {
            ModelProvider::LlamaServer => "http://127.0.0.1:9931",
            ModelProvider::Ollama => "http://127.0.0.1:11434",
        }
    }

    /// Parses the kebab-case provider spelling used in configuration files.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "llama-server" => Some(Self::LlamaServer),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }
}

impl FromStr for ModelProvider {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse(value)
            .ok_or_else(|| format!("invalid provider `{value}`; expected llama-server or ollama"))
    }
}

impl std::fmt::Display for ModelProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ModelProvider::LlamaServer => "llama-server",
            ModelProvider::Ollama => "ollama",
        };
        f.write_str(name)
    }
}

/// One selectable model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    /// The user-facing selector shown and accepted in the UI.
    pub id: String,
    /// The provider the model talks to.
    pub provider: ModelProvider,
    /// The provider base URL. Defaults to the provider's standard endpoint.
    #[serde(default)]
    pub base_url: String,
    /// The provider-facing model selector.
    pub model: String,
    /// Marks this entry as the default model for its provider. Used only as a
    /// fall-back when the (default) provider reports no active model; at most
    /// one entry per provider may be marked default.
    #[serde(default)]
    pub is_default: bool,
    /// Explicit serving context when discovery is unavailable or overridden.
    #[serde(default)]
    pub context_limit: Option<usize>,
    /// Explicit generation reserve; absence chooses a window-aware default.
    #[serde(default)]
    pub response_reserve: Option<u32>,
    /// The per-turn deadline in seconds.
    #[serde(default = "default_deadline")]
    pub deadline_secs: u64,
    /// Total attempts (the initial try plus retries) for one turn before giving
    /// up on a transient, recoverable failure.
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Base backoff delay, in seconds, before the first retry.
    #[serde(default = "default_retry_base_delay_secs")]
    pub retry_base_delay_secs: u64,
    /// Upper bound, in seconds, on any single retry backoff delay.
    #[serde(default = "default_retry_max_delay_secs")]
    pub retry_max_delay_secs: u64,
    /// Inter-token cadence watchdog timeout, in seconds. If the provider sends
    /// no token for longer than this gap after the first token, the turn is
    /// treated as stalled and retried, so a generous per-turn deadline can never
    /// turn a hung provider into a silent multi-minute hang. `0` disables the
    /// watchdog. Defaults to 30s, comfortably above the ~1s gap of a healthy
    /// stream but well below a genuine stall.
    #[serde(default = "default_cadence_timeout_secs")]
    pub cadence_timeout_secs: u64,
}

/// Resolved selected-model budgets and their operational provenance.
#[derive(Debug, Clone, Copy)]
pub struct ModelBudgets {
    pub context_limit: usize,
    pub context_source: &'static str,
    pub response_source: &'static str,
    pub limits: crate::session::RunLimits,
}

impl Model {
    /// Resolves CLI/configured budgets or bounded selected-model discovery.
    pub fn resolve_budgets(
        &self,
        context_override: Option<usize>,
        response_override: Option<u32>,
        mut limits: crate::session::RunLimits,
    ) -> Result<ModelBudgets> {
        let (context_limit, context_source) = if let Some(limit) = context_override {
            (limit, "CLI")
        } else if let Some(limit) = self.context_limit {
            (limit, "configuration")
        } else {
            let discovery = self.transport()?.context_limit(
                &self.model, &agent_runtime::CancellationToken::new(),
            ).map_err(|error| Error::Config {
                path: None,
                reason: format!("could not discover serving capacity for `{}` ({error}); set models.context_limit or --context-limit to the server's actual capacity", self.id),
            })?;
            (discovery.ok_or_else(|| Error::Config {
                path: None,
                reason: format!("serving capacity for `{}` is unavailable; configure models.context_limit or --context-limit (no 8192-token default is assumed)", self.id),
            })?, "provider")
        };
        if !(2..=1_048_576).contains(&context_limit) {
            return Err(Error::Config {
                path: None,
                reason: "context_limit must be in 2..=1048576 tokens".into(),
            });
        }
        let (reserve, response_source) = if let Some(reserve) = response_override {
            (reserve, "CLI")
        } else if let Some(reserve) = self.response_reserve {
            (reserve, "configuration")
        } else {
            ((context_limit / 4).clamp(1, 8192) as u32, "automatic")
        };
        limits.response_reserve = reserve;
        limits.validate()?;
        if context_limit <= reserve as usize {
            return Err(Error::Config {
                path: None,
                reason: "context_limit must exceed response_reserve".into(),
            });
        }
        Ok(ModelBudgets {
            context_limit,
            context_source,
            response_source,
            limits,
        })
    }

    /// Builds the sole direct transport with this model's bounded watchdogs.
    ///
    /// The provider may have to load (or switch to) this model and prefill the
    /// complete prompt before it accepts a request or streams its first token,
    /// and that switch legitimately takes much longer than an ordinary request.
    /// The slot-allocation phase (waiting for the provider to accept) and the
    /// time-to-first-token watchdog are therefore each granted the turn's own
    /// `deadline_secs` budget: a slow model switch is waited for to completion
    /// instead of being retried into failure. A genuinely hung provider is
    /// still bounded by the per-attempt deadline, and a stall after the first
    /// token is still caught by the inter-token cadence watchdog.
    pub fn transport(&self) -> Result<agent_runtime::DirectModelTransport> {
        let deadline = std::time::Duration::from_secs(self.deadline_secs);
        let mut transport = agent_runtime::DirectModelTransport::with_watchdogs(
            self.provider.to_agent_provider(),
            &self.base_url,
            deadline,
            8 * 1024 * 1024,
            deadline,
            deadline,
        )
        .map_err(|error| Error::ModelTransport {
            model: Some(self.id.clone()),
            reason: error.to_string(),
        })?;
        if self.cadence_timeout_secs > 0 {
            transport = transport
                .with_cadence_timeout(std::time::Duration::from_secs(self.cadence_timeout_secs));
        }
        Ok(transport)
    }

    /// Retry limits shared by both terminal and headless session construction.
    pub fn retry_policy(&self) -> crate::retry::RetryPolicy {
        crate::retry::RetryPolicy::new(
            self.max_attempts,
            std::time::Duration::from_secs(self.retry_base_delay_secs),
            std::time::Duration::from_secs(self.retry_max_delay_secs),
        )
    }
}

fn default_deadline() -> u64 {
    // A single long-context turn (large prefill plus thousands of generated
    // tokens) can take over a couple of minutes, so the default per-turn budget
    // comfortably covers one attempt. A stalled provider is still caught quickly
    // by the slot/TTFT/cadence watchdogs, and retries add further headroom.
    300
}

fn default_max_attempts() -> u32 {
    crate::retry::DEFAULT_MAX_ATTEMPTS
}

fn default_retry_base_delay_secs() -> u64 {
    2
}

fn default_retry_max_delay_secs() -> u64 {
    30
}

fn default_cadence_timeout_secs() -> u64 {
    30
}

/// Paths to the independent sandbox enforcement boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxPaths {
    /// Path to the `kvist-sandbox-runner` executable.
    pub runner: PathBuf,
    /// Path to the Bubblewrap backend executable.
    pub backend: PathBuf,
}

impl Default for SandboxPaths {
    fn default() -> Self {
        SandboxPaths {
            runner: SandboxPaths::default_runner(),
            backend: SandboxPaths::default_backend(),
        }
    }
}

impl SandboxPaths {
    /// The default sandbox runner path when not configured.
    pub fn default_runner() -> PathBuf {
        PathBuf::from("/usr/local/bin/kvist-sandbox-runner")
    }

    /// The default Bubblewrap backend path when not configured.
    pub fn default_backend() -> PathBuf {
        PathBuf::from("/usr/bin/bwrap")
    }
}

/// The default sandbox write root: everything the agent writes stays under it.
pub const DEFAULT_WRITE_ROOT: &str = "/workspace";

/// The tool authority policy: a safe-by-default shell denylist plus the sandbox
/// write root. User configuration may only widen the denylist, never remove the
/// built-in minimum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPolicy {
    /// Shell command strings containing any of these substrings are forbidden.
    pub shell_deny_substrings: Vec<String>,
    /// Shell command strings starting with any of these prefixes are forbidden.
    pub shell_deny_prefixes: Vec<String>,
    /// The sandbox write root under which all writes are confined.
    pub write_root: String,
}

impl ToolPolicy {
    /// The safe minimum denylist applied to every configuration.
    pub fn minimum() -> Self {
        ToolPolicy {
            shell_deny_substrings: vec![
                "rm -rf".to_owned(),
                "rm -fr".to_owned(),
                "mkfs".to_owned(),
                "dd if=".to_owned(),
                "dd bs=".to_owned(),
                "> /dev/".to_owned(),
                ":() {".to_owned(),
                "exec 9<>".to_owned(),
                "reboot".to_owned(),
                "shutdown".to_owned(),
            ],
            shell_deny_prefixes: vec!["mknod ".to_owned()],
            write_root: DEFAULT_WRITE_ROOT.to_owned(),
        }
    }

    /// Parses the `[tool_policy]` table on top of the built-in minimum.
    fn from_policy_table(raw: &RawToolPolicy) -> Result<Self> {
        let mut policy = Self::minimum();
        policy
            .shell_deny_substrings
            .extend(raw.shell_deny_substrings.iter().cloned());
        policy
            .shell_deny_prefixes
            .extend(raw.shell_deny_prefixes.iter().cloned());
        if let Some(write_root) = &raw.write_root {
            if write_root.is_empty() {
                return Err(Error::Config {
                    path: None,
                    reason: "tool_policy.write_root must not be empty".to_owned(),
                });
            }
            policy.write_root = write_root.clone();
        }
        Ok(policy)
    }

    /// A stable identity derived from the enforced policy, bound into every
    /// sandbox request so the request can be traced back to the policy it ran
    /// under.
    pub fn identity(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut entries = Vec::new();
        entries.push(("write_root".to_owned(), self.write_root.clone()));
        for substring in &self.shell_deny_substrings {
            entries.push(("deny-substring".to_owned(), substring.clone()));
        }
        for prefix in &self.shell_deny_prefixes {
            entries.push(("deny-prefix".to_owned(), prefix.clone()));
        }
        entries.sort();
        let mut bytes = Vec::new();
        for (key, value) in entries {
            bytes.extend_from_slice(key.as_bytes());
            bytes.push(0u8);
            bytes.extend_from_slice(value.as_bytes());
            bytes.push(0u8);
        }
        format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
    }

    /// Reports whether a shell command string is permitted by this policy.
    pub fn shell_permitted(&self, command: &str) -> bool {
        if self
            .shell_deny_prefixes
            .iter()
            .any(|prefix| command.trim_start().starts_with(prefix.as_str()))
        {
            return false;
        }
        !self
            .shell_deny_substrings
            .iter()
            .any(|substring| command.contains(substring.as_str()))
    }
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self::minimum()
    }
}

/// The validated, in-memory configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The configuration schema version.
    pub schema_version: u32,
    /// The working directory the agent operates in.
    pub working_directory: PathBuf,
    /// The provider to fall back to when no model is active. Used to find a
    /// default model for that provider when the provider reports no active
    /// model; a fall-back only, never overriding an active model, an explicit
    /// selection, or a single configured model.
    pub default_provider: ModelProvider,
    /// The default thinking effort.
    pub default_thinking_effort: agent_runtime::ReasoningEffort,
    /// The selectable models.
    pub models: Vec<Model>,
    /// The tool authority policy.
    pub tool_policy: crate::tools::ToolPolicy,
    /// Per-language tool-chain enablement. `Generic` is always present and is
    /// never represented here; every configurable profile defaults to `Auto`.
    pub tool_profiles: BTreeMap<ToolProfile, ProfileSetting>,
    /// The sandbox boundary paths.
    pub sandbox: SandboxPaths,
    /// The UI theme for the terminal shell. Defaults to `dark`; `light` is
    /// the built-in alternative for light terminals.
    pub theme: crate::tui::Theme,
}

/// The intermediate TOML shape; validated into [`Config`] by [`Config::validate`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema_version: u32,
    #[serde(default)]
    working_directory: Option<String>,
    /// The provider to fall back to when no model is active. Required; the
    /// fall-back is resolved to a model by querying that provider for its active
    /// model and, when there is none, using the provider's configured default
    /// model.
    default_provider: ModelProvider,
    #[serde(default)]
    default_thinking_effort: Option<String>,
    #[serde(default)]
    models: Vec<RawModel>,
    #[serde(default)]
    sandbox: RawSandbox,
    #[serde(default)]
    tool_policy: RawToolPolicy,
    // Language tool profiles keyed by their id (`python`, `rust`, ...); `generic`
    // is always on and never configured. Unknown names are rejected in `validate`.
    #[serde(default)]
    tool_profiles: BTreeMap<String, ProfileSetting>,
    /// The UI theme name (`dark` by default, `light` for a light terminal).
    #[serde(default)]
    theme: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    id: String,
    provider: ModelProvider,
    #[serde(default)]
    base_url: String,
    model: String,
    #[serde(default)]
    is_default: bool,
    #[serde(default)]
    context_limit: Option<usize>,
    #[serde(default)]
    response_reserve: Option<u32>,
    #[serde(default = "default_deadline")]
    deadline_secs: u64,
    #[serde(default = "default_max_attempts")]
    max_attempts: u32,
    #[serde(default = "default_retry_base_delay_secs")]
    retry_base_delay_secs: u64,
    #[serde(default = "default_retry_max_delay_secs")]
    retry_max_delay_secs: u64,
    #[serde(default = "default_cadence_timeout_secs")]
    cadence_timeout_secs: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSandbox {
    #[serde(default)]
    runner: Option<String>,
    #[serde(default)]
    backend: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawToolPolicy {
    #[serde(default)]
    shell_deny_substrings: Vec<String>,
    #[serde(default)]
    shell_deny_prefixes: Vec<String>,
    #[serde(default)]
    write_root: Option<String>,
}

impl Config {
    /// Loads and validates the configuration at `path`.
    pub fn load(path: &Path) -> Result<Config> {
        let contents = read_configuration(path)?;
        let raw: RawConfig = toml::from_str(&contents).map_err(|error: toml::de::Error| {
            configuration_parse_error(path, &contents, error.span())
        })?;
        Config::validate(raw, path)
    }

    /// Validates the parsed configuration against the working directory and the
    /// model/effort/tool-policy invariants.
    fn validate(raw: RawConfig, path: &Path) -> Result<Config> {
        if raw.schema_version != SCHEMA_VERSION {
            return Err(Error::Config {
                path: Some(path.to_string_lossy().into_owned()),
                reason: format!(
                    "schema_version must be {SCHEMA_VERSION}, not {}",
                    raw.schema_version
                ),
            });
        }

        let working_directory = match &raw.working_directory {
            Some(value) => PathBuf::from(value),
            None => std::env::current_dir()
                .map_err(|source| io_error("resolve current working directory", None, source))?,
        };
        if !working_directory.is_absolute() {
            return Err(Error::Config {
                path: Some(path.to_string_lossy().into_owned()),
                reason: format!(
                    "working_directory `{}` must be an absolute path",
                    working_directory.display()
                ),
            });
        }
        if !working_directory.is_dir() {
            return Err(Error::Config {
                path: Some(path.to_string_lossy().into_owned()),
                reason: format!(
                    "working_directory `{}` must be an existing directory",
                    working_directory.display()
                ),
            });
        }

        if raw.models.is_empty() {
            return Err(Error::Config {
                path: Some(path.to_string_lossy().into_owned()),
                reason: "at least one `[[models]]` entry is required".to_owned(),
            });
        }

        let mut models = Vec::with_capacity(raw.models.len());
        let mut seen_ids = std::collections::BTreeSet::new();
        for (index, model) in raw.models.iter().enumerate() {
            if !seen_ids.insert(model.id.clone()) {
                return Err(Error::Config {
                    path: Some(path.to_string_lossy().into_owned()),
                    reason: format!("duplicate model id `{}` at models[{index}]", model.id),
                });
            }
            let base_url = if model.base_url.is_empty() {
                model.provider.default_endpoint().to_owned()
            } else {
                model.base_url.clone()
            };
            if model.model.trim().is_empty() {
                return Err(Error::Config {
                    path: Some(path.to_string_lossy().into_owned()),
                    reason: format!("models[{index}].model must not be empty"),
                });
            }
            if !(MIN_DEADLINE_SECS..=MAX_DEADLINE_SECS).contains(&model.deadline_secs) {
                return Err(Error::Config {
                    path: Some(path.to_string_lossy().into_owned()),
                    reason: format!(
                        "models[{index}].deadline_secs must be between {MIN_DEADLINE_SECS} and {MAX_DEADLINE_SECS}"
                    ),
                });
            }
            if model
                .context_limit
                .is_some_and(|limit| !(2..=1_048_576).contains(&limit))
                || model
                    .response_reserve
                    .is_some_and(|reserve| !(1..=1_048_576).contains(&reserve))
                || model
                    .context_limit
                    .zip(model.response_reserve)
                    .is_some_and(|(limit, reserve)| limit <= reserve as usize)
            {
                return Err(Error::Config {
                    path: Some(path.to_string_lossy().into_owned()),
                    reason: format!(
                        "models[{index}] needs bounded context_limit greater than response_reserve"
                    ),
                });
            }
            if model.retry_base_delay_secs > model.retry_max_delay_secs {
                return Err(Error::Config {
                    path: Some(path.to_string_lossy().into_owned()),
                    reason: format!(
                        "models[{index}].retry_base_delay_secs must not exceed retry_max_delay_secs"
                    ),
                });
            }
            if model.max_attempts < 1 {
                return Err(Error::Config {
                    path: Some(path.to_string_lossy().into_owned()),
                    reason: format!("models[{index}].max_attempts must be at least 1"),
                });
            }
            if !(0..=MAX_DEADLINE_SECS).contains(&model.cadence_timeout_secs) {
                return Err(Error::Config {
                    path: Some(path.to_string_lossy().into_owned()),
                    reason: format!(
                        "models[{index}].cadence_timeout_secs must be between 0 and {MAX_DEADLINE_SECS}"
                    ),
                });
            }
            const MAX_RETRY_DELAY_SECS: u64 = 3600;
            if model.retry_base_delay_secs > MAX_RETRY_DELAY_SECS
                || model.retry_max_delay_secs > MAX_RETRY_DELAY_SECS
            {
                return Err(Error::Config {
                    path: Some(path.to_string_lossy().into_owned()),
                    reason: format!(
                        "models[{index}].retry_*_delay_secs must be between 0 and {MAX_RETRY_DELAY_SECS}"
                    ),
                });
            }
            models.push(Model {
                id: model.id.clone(),
                provider: model.provider,
                base_url,
                model: model.model.clone(),
                is_default: model.is_default,
                context_limit: model.context_limit,
                response_reserve: model.response_reserve,
                deadline_secs: model.deadline_secs,
                max_attempts: model.max_attempts,
                retry_base_delay_secs: model.retry_base_delay_secs,
                retry_max_delay_secs: model.retry_max_delay_secs,
                cadence_timeout_secs: model.cadence_timeout_secs,
            });
        }

        let default_thinking_effort = match &raw.default_thinking_effort {
            Some(value) => agent_runtime::ReasoningEffort::from_str(value).map_err(|_| {
                Error::Config {
                    path: Some(path.to_string_lossy().into_owned()),
                    reason: format!(
                        "default_thinking_effort `{value}` is invalid; expected none, minimal, low, medium, high, xhigh, or max"
                    ),
                }
            })?,
            None => agent_runtime::ReasoningEffort::Medium,
        };

        // At most one default model per provider: the per-provider default is a
        // fall-back for that provider's endpoint, so two defaults for one
        // provider would be ambiguous.
        let mut default_seen = std::collections::BTreeSet::new();
        for model in models.iter() {
            if model.is_default && !default_seen.insert(model.provider.to_string()) {
                return Err(Error::Config {
                    path: Some(path.to_string_lossy().into_owned()),
                    reason: format!(
                        "duplicate is_default model for provider `{}`; at most one \
                         [[models]] entry per provider may be marked default",
                        model.provider
                    ),
                });
            }
        }
        // The default provider must name at least one configured model, or the
        // fall-back can never resolve to a model.
        if !models
            .iter()
            .any(|model| model.provider == raw.default_provider)
        {
            return Err(Error::Config {
                path: Some(path.to_string_lossy().into_owned()),
                reason: format!(
                    "default_provider `{}` has no [[models]] entry; configure a model for \
                     it or set default_provider to a provider that has one",
                    raw.default_provider
                ),
            });
        }

        let tool_policy = crate::tools::ToolPolicy::from_policy_table(&raw.tool_policy)?;

        let mut tool_profiles: BTreeMap<ToolProfile, ProfileSetting> = BTreeMap::new();
        for (name, setting) in &raw.tool_profiles {
            match ToolProfile::from_id(name) {
                Some(ToolProfile::Generic) => {}
                Some(profile) => {
                    tool_profiles.insert(profile, *setting);
                }
                None => {
                    return Err(Error::Config {
                        path: Some(path.to_string_lossy().into_owned()),
                        reason: format!(
                            "unknown tool profile `{name}` in `[tool_profiles]`; expected one of
                             generic, python, rust, javascript, go, c"
                        ),
                    });
                }
            }
        }

        let theme = match raw.theme.as_deref() {
            None => crate::tui::Theme::default(),
            Some(name) => crate::tui::Theme::by_name(name).ok_or_else(|| Error::Config {
                path: Some(path.to_string_lossy().into_owned()),
                reason: format!(
                    "theme `{name}` is not recognized; expected one of: {}",
                    crate::tui::THEME_NAMES.join(", ")
                ),
            })?,
        };

        Ok(Config {
            schema_version: SCHEMA_VERSION,
            working_directory,
            default_provider: raw.default_provider,
            default_thinking_effort,
            models,
            tool_policy,
            theme,
            sandbox: SandboxPaths {
                runner: raw
                    .sandbox
                    .runner
                    .map(PathBuf::from)
                    .unwrap_or_else(SandboxPaths::default_runner),
                backend: raw
                    .sandbox
                    .backend
                    .map(PathBuf::from)
                    .unwrap_or_else(SandboxPaths::default_backend),
            },
            tool_profiles,
        })
    }

    /// Builds an in-memory configuration, used by tests and by the CLI overrides.
    pub fn from_parts(
        working_directory: PathBuf,
        default_provider: ModelProvider,
        models: Vec<Model>,
        tool_policy: crate::tools::ToolPolicy,
    ) -> Result<Config> {
        if !models
            .iter()
            .any(|model| model.provider == default_provider)
        {
            return Err(Error::Config {
                path: None,
                reason: format!("default_provider `{default_provider}` has no [[models]] entry"),
            });
        }
        let tool_profiles = ToolProfile::CONFIGURABLE
            .iter()
            .copied()
            .map(|profile| (profile, ProfileSetting::Auto))
            .collect();
        Ok(Config {
            schema_version: SCHEMA_VERSION,
            working_directory,
            default_provider,
            default_thinking_effort: agent_runtime::ReasoningEffort::Medium,
            models,
            tool_policy,
            tool_profiles,
            sandbox: SandboxPaths::default(),
            theme: crate::tui::Theme::default(),
        })
    }

    /// The model matching a selector, when present.
    pub fn model(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.id == id)
    }

    /// The configured models for the default provider, in configuration order.
    /// Used to resolve the per-provider default-model fall-back.
    pub fn default_provider_models(&self) -> Vec<&Model> {
        self.models
            .iter()
            .filter(|model| model.provider == self.default_provider)
            .collect()
    }

    /// The default provider's default model, when exactly one entry for that
    /// provider is marked `is_default`. `None` when there is none (several
    /// models with no active one defer to the user).
    pub fn default_model(&self) -> Option<&Model> {
        self.models
            .iter()
            .find(|model| model.provider == self.default_provider && model.is_default)
    }
}

/// The outcome of active-model-first selection for a session.
///
/// The precedence is: an explicit selection (rule 1), a loaded provider model
/// matching a configured entry (rule 2), a single configured model (rule 3), and
/// otherwise a fall-back (rule 4). A fall-back selects the default provider's
/// default model when the provider has no active model; when the default
/// provider has several configured models and no active one, the session must
/// defer (interactive) or fail (headless) rather than guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Select {
    /// The user explicitly chose this model; it is used exactly as selected.
    Explicit(String),
    /// A configured entry whose provider model name matches the provider's
    /// already-loaded model: selected with no load/switch, announced via
    /// `reason` and `loaded`.
    Active {
        model_id: String,
        reason: String,
        loaded: String,
    },
    /// Exactly one model is configured: auto-selected as a convenience.
    Single { model_id: String },
    /// The default provider reports no active model and its default model is
    /// configured: selected as the fall-back, announcing that the default model
    /// is loaded (the provider may need to load it; the session waits for it).
    /// `reason` is the announcement; `loaded` is the provider-facing model name.
    Default {
        model_id: String,
        reason: String,
        loaded: String,
    },
    /// The default provider reports no active model and there is no single
    /// default model to fall back to: the session must defer (interactive) or
    /// fail (headless) rather than guess.
    NeedsSelection,
}

/// Probes each distinct configured provider endpoint for its already-loaded
/// model, returning `base_url -> loaded provider model name`.
///
/// The probe is bounded, read-only, and loopback-only: it reuses the transport's
/// endpoint authority (numeric loopback only) and a short probe deadline, and it
/// sends no inference and requests no model switch. A down, timed-out, malformed,
/// or empty provider yields no entry for that endpoint, so the caller falls
/// through to the next selection rule rather than failing startup.
pub fn probe_active_models(config: &Config) -> std::collections::BTreeMap<String, String> {
    // A single configured model auto-selects (the default provider's only
    // model) identically whether or not it is active, so no active probe is
    // needed — and skipping it keeps a single-model startup free of provider I/O
    // (and its pinned request sequence). When the single model is already
    // loaded, the provider reuses it on the first request and pays no switch;
    // the probe would only add up to its deadline to startup to announce that.
    // Several configured models need the probe to find an active one to reuse.
    if config.models.len() <= 1 {
        return std::collections::BTreeMap::new();
    }
    const PROBE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
    const PROBE_RESPONSE_BYTES: usize = 64 * 1024;
    let mut active = std::collections::BTreeMap::new();
    for model in config.models.iter() {
        if active.contains_key(&model.base_url) {
            continue;
        }
        // A dedicated, short-deadline transport bounds the probe so a wedged
        // provider cannot stall startup; it carries no watchdogs and is used
        // only for this single read-only metadata GET.
        let probe = match agent_runtime::DirectModelTransport::new(
            model.provider.to_agent_provider(),
            &model.base_url,
            PROBE_DEADLINE,
            PROBE_RESPONSE_BYTES,
        ) {
            Ok(transport) => transport,
            Err(_) => continue,
        };
        if let Ok(Some(loaded)) = probe.loaded_model(&agent_runtime::CancellationToken::new()) {
            active.insert(model.base_url.clone(), loaded);
        }
    }
    active
}

/// Resolves the session model by the documented precedence from
/// `Config::models` and the active provider models.
///
/// `active` maps `base_url -> loaded provider model name` (see
/// [`probe_active_models`]). An explicit selection is returned unchanged; a
/// single configured model is returned as [`Select::Single`]; a unique active
/// match by provider model name is returned as [`Select::Active`]; the default
/// provider's default model (used only when the provider reports no active
/// model) is returned as [`Select::Default`]; and several configured models for
/// the default provider with no active match and no single default return
/// [`Select::NeedsSelection`].
pub fn select_active_model(
    config: &Config,
    explicit: Option<&str>,
    active: &std::collections::BTreeMap<String, String>,
) -> Select {
    if let Some(id) = explicit {
        return Select::Explicit(id.to_owned());
    }
    if config.models.len() == 1 {
        return Select::Single {
            model_id: config.models[0].id.clone(),
        };
    }
    // Only models configured for the default provider are ever auto-selected:
    // the default provider is the provider the session falls back to, so its
    // already-loaded model is the one reused (with no switch) and its default
    // model is the one loaded as a fall-back. A model on any other provider is
    // an explicit choice only.
    let default_provider = config.default_provider;
    let default_provider_models: Vec<&Model> = config
        .models
        .iter()
        .filter(|model| model.provider == default_provider)
        .collect();
    if default_provider_models.len() == 1 {
        // The default provider has exactly one model. It is selected whether or
        // not it is active: when already loaded the provider reuses it with no
        // switch, and when not it is the single model the session loads.
        let model = &default_provider_models[0];
        return Select::Single {
            model_id: model.id.clone(),
        };
    }
    // Collect default-provider entries that match a loaded provider model,
    // grouped by the (base_url, provider model name) they matched. A unique
    // match selects that entry with no switch; more than one candidate is
    // ambiguous and falls to the default-model fall-back.
    let mut matched: Vec<&Model> = Vec::new();
    let mut ambiguous = false;
    for model in default_provider_models.iter() {
        if let Some(loaded) = active.get(&model.base_url)
            && loaded == &model.model
        {
            matched.push(model);
        }
    }
    if !matched.is_empty() {
        // Two distinct entries can match the same loaded model (e.g. duplicated
        // provider model name on one server); that is ambiguous.
        {
            let mut seen = std::collections::BTreeSet::new();
            for model in &matched {
                if !seen.insert((model.base_url.as_str(), model.model.as_str())) {
                    ambiguous = true;
                }
            }
        }
        if !ambiguous {
            // A single unique match: reuse it with no switch.
            let model = matched[0];
            return Select::Active {
                model_id: model.id.clone(),
                reason: format!(
                    "using already-loaded model `{}` (no switch)",
                    active
                        .get(&model.base_url)
                        .expect("matched entry has an active model")
                ),
                loaded: model.model.clone(),
            };
        }
    }
    // No active model to reuse: fall back to the default provider's default
    // model when it is configured, otherwise defer to the user.
    match config.default_model() {
        Some(default) => Select::Default {
            model_id: default.id.clone(),
            reason: format!(
                "no active model detected; using default model `{}` for `{}`",
                default.id, default_provider
            ),
            loaded: default.model.clone(),
        },
        None => Select::NeedsSelection,
    }
}

/// Reports configured provider models that are loaded but not present in
/// `[[models]]`, so the caller can offer a ready-to-paste `[[models]]` entry.
pub fn unconfigured_active_models(
    config: &Config,
    active: &std::collections::BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut unconfigured = Vec::new();
    for (base_url, loaded) in active.iter() {
        let configured = config
            .models
            .iter()
            .any(|model| &model.base_url == base_url && &model.model == loaded);
        if !configured {
            unconfigured.push((loaded.clone(), base_url.clone()));
        }
    }
    unconfigured
}

pub(crate) fn read_configuration(path: &Path) -> Result<String> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_NONBLOCK).bits())
        .open(path)
        .map_err(|source| io_error("open configuration", Some(&path.to_string_lossy()), source))?;
    let metadata = file
        .metadata()
        .map_err(|source| io_error("inspect configuration descriptor", None, source))?;
    if !metadata.is_file() {
        return Err(Error::Config {
            path: Some(path.to_string_lossy().into_owned()),
            reason: "configuration must be a regular non-link file".to_owned(),
        });
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(Error::Config {
            path: Some(path.to_string_lossy().into_owned()),
            reason: format!("configuration exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        });
    }
    read_configuration_stream(file, path)
}

pub(crate) fn configuration_parse_error(
    path: &Path,
    contents: &str,
    span: Option<std::ops::Range<usize>>,
) -> Error {
    let offset = span.map_or(0, |range| range.start.min(contents.len()));
    let line = contents.as_bytes()[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    Error::Config {
        path: Some(path.to_string_lossy().into_owned()),
        reason: format!(
            "invalid TOML or field types at line {line}; check syntax, field names and the schema-one configuration contract"
        ),
    }
}

fn read_configuration_stream(reader: impl std::io::Read, path: &Path) -> Result<String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| io_error("read configuration", Some(&path.to_string_lossy()), source))?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(Error::Config {
            path: Some(path.to_string_lossy().into_owned()),
            reason: format!("configuration exceeds the {MAX_CONFIG_BYTES}-byte read limit"),
        });
    }
    String::from_utf8(bytes).map_err(|_| Error::Config {
        path: Some(path.to_string_lossy().into_owned()),
        reason: "configuration must be valid UTF-8".into(),
    })
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn configuration_stream_growth_cannot_exceed_the_byte_bound() {
        let bytes = vec![b' '; MAX_CONFIG_BYTES as usize + 1];
        assert!(
            read_configuration_stream(std::io::Cursor::new(bytes), Path::new("config.toml"))
                .is_err()
        );
    }
}
