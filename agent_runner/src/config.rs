//! Configuration model, loading, and validation.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result, io_error};

/// Maximum encoded size of a configuration file.
pub const MAX_CONFIG_BYTES: u64 = 64 * 1024;
/// The only accepted configuration schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// Minimum per-turn model deadline in seconds.
const MIN_DEADLINE_SECS: u64 = 1;
/// Maximum per-turn model deadline in seconds.
const MAX_DEADLINE_SECS: u64 = 600;

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
    /// The default model selector.
    pub default_model: String,
    /// The default thinking effort.
    pub default_thinking_effort: agent_runtime::ReasoningEffort,
    /// The selectable models.
    pub models: Vec<Model>,
    /// The tool authority policy.
    pub tool_policy: crate::tools::ToolPolicy,
    /// The sandbox boundary paths.
    pub sandbox: SandboxPaths,
}

/// The intermediate TOML shape; validated into [`Config`] by [`Config::validate`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema_version: u32,
    #[serde(default)]
    working_directory: Option<String>,
    default_model: String,
    #[serde(default)]
    default_thinking_effort: Option<String>,
    #[serde(default)]
    models: Vec<RawModel>,
    #[serde(default)]
    sandbox: RawSandbox,
    #[serde(default)]
    tool_policy: RawToolPolicy,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModel {
    id: String,
    provider: ModelProvider,
    #[serde(default)]
    base_url: String,
    model: String,
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
        let raw: RawConfig = toml::from_str(&contents).map_err(|error| Error::Config {
            path: Some(path.to_string_lossy().into_owned()),
            reason: error.to_string(),
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
        if !working_directory.exists() {
            return Err(Error::Config {
                path: Some(path.to_string_lossy().into_owned()),
                reason: format!(
                    "working_directory `{}` does not exist",
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

        if !models.iter().any(|model| model.id == raw.default_model) {
            return Err(Error::ModelNotFound {
                requested: raw.default_model.clone(),
                available: models.iter().map(|model| model.id.clone()).collect(),
            });
        }

        let tool_policy = crate::tools::ToolPolicy::from_policy_table(&raw.tool_policy)?;

        Ok(Config {
            schema_version: SCHEMA_VERSION,
            working_directory,
            default_model: raw.default_model,
            default_thinking_effort,
            models,
            tool_policy,
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
        })
    }

    /// Builds an in-memory configuration, used by tests and by the CLI overrides.
    pub fn from_parts(
        working_directory: PathBuf,
        default_model: &str,
        models: Vec<Model>,
        tool_policy: crate::tools::ToolPolicy,
    ) -> Result<Config> {
        if !models.iter().any(|model| model.id == default_model) {
            return Err(Error::ModelNotFound {
                requested: default_model.to_owned(),
                available: models.iter().map(|model| model.id.clone()).collect(),
            });
        }
        Ok(Config {
            schema_version: SCHEMA_VERSION,
            working_directory,
            default_model: default_model.to_owned(),
            default_thinking_effort: agent_runtime::ReasoningEffort::Medium,
            models,
            tool_policy,
            sandbox: SandboxPaths::default(),
        })
    }

    /// The model matching a selector, when present.
    pub fn model(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.id == id)
    }
}

fn read_configuration(path: &Path) -> Result<String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|source| {
        io_error(
            "inspect configuration",
            Some(&path.to_string_lossy()),
            source,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
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
    // `read_to_string` already rejects non-UTF-8 input.
    std::fs::read_to_string(path)
        .map_err(|source| io_error("read configuration", Some(&path.to_string_lossy()), source))
}
