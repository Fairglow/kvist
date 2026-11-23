//! Strict loading of project-local Kvist configuration.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{KvistError, Result, artifacts::CONFIGURATION_VERSION, filesystem::is_link_like};

/// Maximum supported size of `kvist.toml`.
pub const MAX_CONFIGURATION_BYTES: u64 = 64 * 1024;
/// Runner-compatible bounds for `[sandbox].environment_allowlist`. These are
/// deliberately kept with configuration parsing so an invalid environment name
/// never reaches request construction.
pub const MAX_SANDBOX_ENVIRONMENT_ENTRIES: usize = 256;
pub const MAX_SANDBOX_ENVIRONMENT_NAME_BYTES: usize = 4096;

/// Bounded, deterministic limits applied while discovering components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveryLimits {
    /// Directory levels below the component root.
    pub max_depth: usize,
    /// Directories whose entries may be read, including the component root.
    pub max_directories: usize,
    /// Recognized components, including the component root.
    pub max_components: usize,
    /// Entries read from any one scanned directory.
    pub max_entries_per_directory: usize,
    /// Encoded bytes in a component path relative to the component root.
    pub max_relative_path_bytes: usize,
}

impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            max_depth: 64,
            max_directories: 10_000,
            max_components: 10_000,
            max_entries_per_directory: 10_000,
            max_relative_path_bytes: 4_096,
        }
    }
}

/// Largest accepted discovery limits. These prevent configuration from
/// disabling resource bounds.
pub const MAX_DISCOVERY_LIMITS: DiscoveryLimits = DiscoveryLimits {
    max_depth: 256,
    max_directories: 100_000,
    max_components: 100_000,
    max_entries_per_directory: 100_000,
    max_relative_path_bytes: 32_768,
};

/// An agent role (e.g., "architect", "developer", "security-reviewer").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Role {
    /// The architect role handles compliance review, architecture decisions,
    /// and high-level design.
    Architect,
    /// The developer role handles test generation, implementation, and refactoring.
    Developer,
    /// The security reviewer role handles specialized security audits.
    SecurityReviewer,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Role::Architect => "architect",
            Role::Developer => "developer",
            Role::SecurityReviewer => "security-reviewer",
        }
    }

    pub fn from_str_case_insensitive(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "architect" => Some(Role::Architect),
            "developer" => Some(Role::Developer),
            "security-reviewer" => Some(Role::SecurityReviewer),
            _ => None,
        }
    }
}

/// A model configuration for a role (retained for backward-compatible inspections).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    /// A model identifier string (e.g., "llama-cli", "ollama", "mcp", "none").
    pub name: String,
    /// The command template for this model, with `{prompt}`, `{context_files}`, etc.
    pub command: String,
    /// A system prompt injected at the start of the message.
    pub system_prompt: Option<String>,
}

/// Configuration for an agent provider (wire protocol and transport settings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Unique provider identifier (e.g. "local-llama", "ollama", "gemini").
    pub name: String,
    /// Provider type / protocol (e.g. "llama-server", "ollama", "gemini-cli", "claude", "custom").
    pub provider_type: String,
    /// Optional HTTP base URL (e.g. "http://127.0.0.1:9931").
    pub base_url: Option<String>,
    /// Optional command template override.
    pub command_template: Option<String>,
    /// Optional connection / execution timeout in seconds.
    pub timeout_seconds: Option<u64>,
}

impl ProviderConfig {
    /// Returns the effective command template for this provider, synthesizing defaults
    /// according to the provider type if an explicit command_template is not configured.
    pub fn effective_command_template(&self) -> String {
        if let Some(template) = &self.command_template
            && !template.trim().is_empty()
        {
            return template.clone();
        }
        match self.provider_type.as_str() {
            "llama-server" => {
                let url = self
                    .base_url
                    .as_deref()
                    .unwrap_or("http://127.0.0.1:9931")
                    .trim_end_matches('/');
                format!(
                    "curl --disable --silent --show-error --fail-with-body --request POST --json '{{\"model\":{{model_json}},\"messages\":[{{\"role\":\"user\",\"content\":{{prompt_json}}}}]}}' -- '{url}/v1/chat/completions'"
                )
            }
            "ollama" => {
                let url = self
                    .base_url
                    .as_deref()
                    .unwrap_or("http://127.0.0.1:11434")
                    .trim_end_matches('/');
                format!(
                    "curl --disable --silent --show-error --fail-with-body --request POST --json '{{\"model\":{{model_json}},\"messages\":[{{\"role\":\"user\",\"content\":{{prompt_json}}}}]}}' -- '{url}/api/chat'"
                )
            }
            "llama-cli" => {
                "llama-cli --model '{model}' --prompt '{prompt}' --single-turn --simple-io --no-display-prompt --predict 4096".to_owned()
            }
            "gemini" | "gemini-cli" => {
                "gemini-cli --prompt '{prompt}' --files {context_files}".to_owned()
            }
            "claude" => {
                "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}".to_owned()
            }
            "copilot" => {
                "copilot --prompt '{prompt}'".to_owned()
            }
            _ => "{prompt} {context_files}".to_owned(),
        }
    }
}

/// Configuration for a specific model profile (model identifier and intrinsic capabilities).
///
/// Equality is total: the `temperature` field is compared by its IEEE-754 bit
/// pattern rather than `f64::eq`, so a `NaN` temperature is reflexive and two
/// `NaN`s compare equal. This keeps the `Eq` implementation sound — a role
/// selects a profile by equality, and an ill-defined comparison could otherwise
/// change that selection without any recorded intent change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    /// Unique profile name (e.g. "qwen-coder-32b", "claude-default").
    pub name: String,
    /// Provider name this profile connects through.
    pub provider: String,
    /// Model identifier or filename for the provider. Defaults to profile name if omitted.
    pub model: Option<String>,
    /// Context window size in tokens.
    pub context_window: Option<usize>,
    /// Whether this model supports reasoning / thinking tokens.
    pub supports_thinking: Option<bool>,
    /// Default thinking effort baseline for this model.
    pub default_thinking_effort: Option<sav::ReasoningEffort>,
    /// Sampling temperature.
    pub temperature: Option<f64>,
    /// Optional system prompt injected at the start of prompts.
    pub system_prompt: Option<String>,
    /// Optional command template directly overriding provider template.
    pub command: Option<String>,
}

/// Compares the `temperature` field by its IEEE-754 bit pattern so that a
/// `NaN` temperature is reflexive and two `NaN`s compare equal. Every other
/// field is compared normally, which is already total because the remaining
/// fields are all `Eq`.
fn profile_temperatures_equal(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.to_bits() == right.to_bits(),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

impl PartialEq for ProfileConfig {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.provider == other.provider
            && self.model == other.model
            && self.context_window == other.context_window
            && self.supports_thinking == other.supports_thinking
            && self.default_thinking_effort == other.default_thinking_effort
            && profile_temperatures_equal(self.temperature, other.temperature)
            && self.system_prompt == other.system_prompt
            && self.command == other.command
    }
}

impl Eq for ProfileConfig {}

/// Configuration for an agent role (task intent, persona, policy, and profile binding).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleConfig {
    /// Typed selection pinned by shared registry loading, never command-inferred.
    #[serde(default)]
    pub resolved_profile: Option<sav::registry::ResolvedProfile>,
    /// The role (Architect, Developer, SecurityReviewer).
    pub role: Role,
    /// Name of the assigned model profile.
    pub profile: String,
    /// Effective command template rendered for this role.
    pub command_template: String,
    /// Available models list for backwards-compatible inspection.
    pub models: Vec<Model>,
    /// Default model name for backwards-compatible inspection.
    pub default_model: String,
    /// Optional model override.
    pub model: Option<String>,
    /// Thinking effort policy for this role (overrides profile default).
    pub thinking_effort: Option<sav::ReasoningEffort>,
    /// Optional hard limit for the whole protected authoring task.
    pub timeout_seconds: Option<u64>,
    /// Token budget limit for this role.
    pub token_limit: Option<usize>,
    /// Per-response HTTP, per-effect capture, and retained agent-log byte ceiling.
    pub max_output_bytes: usize,
    /// Literal values replaced before agent evidence reaches any sink.
    pub redaction_values: Vec<String>,
}

/// Retained alias for role configurations.
pub type AgentProfile = RoleConfig;

impl RoleConfig {
    pub fn default_for_role(role: Role) -> Self {
        let (profile, command_template) = match role {
            Role::Architect => (
                "claude-default".to_owned(),
                "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}".to_owned(),
            ),
            Role::Developer => (
                "gemini-default".to_owned(),
                "gemini-cli --prompt '{prompt}' --files {context_files}".to_owned(),
            ),
            Role::SecurityReviewer => (
                "claude-default".to_owned(),
                "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}".to_owned(),
            ),
        };
        Self {
            resolved_profile: None,
            role,
            profile: profile.clone(),
            command_template: command_template.clone(),
            models: vec![Model {
                name: profile.clone(),
                command: command_template,
                system_prompt: None,
            }],
            default_model: profile,
            model: None,
            thinking_effort: None,
            timeout_seconds: None,
            token_limit: None,
            max_output_bytes: DEFAULT_AGENT_MAX_OUTPUT_BYTES,
            redaction_values: Vec::new(),
        }
    }
}

pub const DEFAULT_REVIEW_TIMEOUT_SECONDS: u64 = 2 * 60 * 60;
pub const MAX_AGENT_TIMEOUT_SECONDS: u64 = 30 * 24 * 60 * 60;
pub const DEFAULT_AGENT_MAX_OUTPUT_BYTES: usize = MAX_AGENT_OUTPUT_BYTES;
pub const MAX_AGENT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// The resolved agent configuration input, retained for execution approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentConfigSource {
    pub identity: String,
    pub digest: String,
}

/// Provider detection / discovery configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct AgentDiscoveryConfig {
    /// Base URL for Ollama endpoints (default: "http://127.0.0.1:11434").
    pub ollama_url: Option<String>,
    /// Base URL for llama-server endpoints (default: "http://127.0.0.1:8080").
    pub llama_server_url: Option<String>,
    /// Discovery timeout in seconds (default: 5).
    pub timeout_seconds: Option<u64>,
    /// Whether to allow host discovery over loopback (default: true).
    pub allow_host_discovery: Option<bool>,
}

/// Configuration for external agent execution runners, structured with
/// separated providers, model profiles, and role policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentConfig {
    /// Shared definitions pinned at load time; empty for explicit local configs.
    pub shared_profiles: BTreeMap<String, sav::registry::ResolvedProfile>,
    /// Configured providers: wire protocols and transport endpoints.
    pub providers: BTreeMap<String, ProviderConfig>,
    /// Configured model profiles: model identifiers and metadata.
    pub profiles: BTreeMap<String, ProfileConfig>,
    /// Configured roles: task intent, persona, and profile bindings.
    pub roles: BTreeMap<Role, RoleConfig>,
    /// Architectural review role.
    pub architect: RoleConfig,
    /// Test & code implementation role.
    pub developer: RoleConfig,
    /// Security review role.
    pub security_reviewer: RoleConfig,
    /// Discovery configuration for endpoint probing.
    pub discovery: AgentDiscoveryConfig,
    /// Identity and content digest of the resolver input that selected this config.
    pub source: AgentConfigSource,
}

impl AgentConfig {
    /// Returns a pinned shared selection without discovery or command inference.
    pub fn resolve_named_profile(&self, name: &str) -> Result<&sav::registry::ResolvedProfile> {
        self.shared_profiles.get(name).ok_or_else(|| invalid_configuration(
            Path::new(&self.source.identity),
            &format!("shared profile `{name}` is unavailable; configure [agent.registry] and an enabled profile"),
        ))
    }

    /// Resolves exactly the role's configured profile, never a loaded default.
    pub fn resolve_role_profile(&self, role: Role) -> Result<&sav::registry::ResolvedProfile> {
        self.resolve_named_profile(&self.role(role).profile)
    }

    pub fn role(&self, role: Role) -> &RoleConfig {
        match role {
            Role::Architect => &self.architect,
            Role::Developer => &self.developer,
            Role::SecurityReviewer => &self.security_reviewer,
        }
    }

    pub fn role_mut(&mut self, role: Role) -> &mut RoleConfig {
        match role {
            Role::Architect => &mut self.architect,
            Role::Developer => &mut self.developer,
            Role::SecurityReviewer => &mut self.security_reviewer,
        }
    }

    pub fn sync_roles_and_profiles(&mut self) {
        let models_list: Vec<Model> = self
            .profiles
            .iter()
            .map(|(name, prof)| {
                let cmd = if let Some(c) = &prof.command {
                    c.clone()
                } else if let Some(provider) = self.providers.get(&prof.provider) {
                    let mut template = provider.effective_command_template();
                    let model_name = prof.model.as_deref().unwrap_or(&prof.name);
                    template = template.replace("{model}", model_name);
                    let model_json = serde_json::to_string(model_name)
                        .unwrap_or_else(|_| format!("\"{model_name}\""));
                    template = template.replace("{model_json}", &model_json);
                    let base_url = provider.base_url.as_deref().unwrap_or("");
                    template = template.replace("{base_url}", base_url);
                    template
                } else {
                    "{prompt} {context_files}".to_string()
                };
                Model {
                    name: name.clone(),
                    command: cmd,
                    system_prompt: prof.system_prompt.clone(),
                }
            })
            .collect();

        for role in [Role::Architect, Role::Developer, Role::SecurityReviewer] {
            let role_cfg = self
                .roles
                .entry(role)
                .or_insert_with(|| RoleConfig::default_for_role(role));
            role_cfg.models = models_list.clone();
            role_cfg.default_model = role_cfg.profile.clone();
            role_cfg.resolved_profile = self.shared_profiles.get(&role_cfg.profile).cloned();

            if let Some(prof) = self.profiles.get(&role_cfg.profile) {
                if let Some(cmd) = &prof.command {
                    role_cfg.command_template = cmd.clone();
                } else if let Some(provider) = self.providers.get(&prof.provider) {
                    let mut cmd = provider.effective_command_template();
                    let model_name = prof.model.as_deref().unwrap_or(&prof.name);
                    cmd = cmd.replace("{model}", model_name);
                    let model_json = serde_json::to_string(model_name)
                        .unwrap_or_else(|_| format!("\"{model_name}\""));
                    cmd = cmd.replace("{model_json}", &model_json);
                    let base_url = provider.base_url.as_deref().unwrap_or("");
                    cmd = cmd.replace("{base_url}", base_url);
                    role_cfg.command_template = cmd;
                }
            }
        }

        if let Some(arch) = self.roles.get(&Role::Architect) {
            self.architect = arch.clone();
        }
        if let Some(dev) = self.roles.get(&Role::Developer) {
            self.developer = dev.clone();
        }
        if let Some(sec) = self.roles.get(&Role::SecurityReviewer) {
            self.security_reviewer = sec.clone();
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        let mut providers = BTreeMap::new();
        providers.insert(
            "claude".to_owned(),
            ProviderConfig {
                name: "claude".to_owned(),
                provider_type: "claude".to_owned(),
                base_url: None,
                command_template: Some("claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}".to_owned()),
                timeout_seconds: None,
            },
        );
        providers.insert(
            "gemini".to_owned(),
            ProviderConfig {
                name: "gemini".to_owned(),
                provider_type: "gemini-cli".to_owned(),
                base_url: None,
                command_template: Some(
                    "gemini-cli --prompt '{prompt}' --files {context_files}".to_owned(),
                ),
                timeout_seconds: None,
            },
        );

        let mut profiles = BTreeMap::new();
        profiles.insert(
            "claude-default".to_owned(),
            ProfileConfig {
                name: "claude-default".to_owned(),
                provider: "claude".to_owned(),
                model: None,
                context_window: None,
                supports_thinking: None,
                default_thinking_effort: None,
                temperature: None,
                system_prompt: None,
                command: None,
            },
        );
        profiles.insert(
            "gemini-default".to_owned(),
            ProfileConfig {
                name: "gemini-default".to_owned(),
                provider: "gemini".to_owned(),
                model: None,
                context_window: None,
                supports_thinking: None,
                default_thinking_effort: None,
                temperature: None,
                system_prompt: None,
                command: None,
            },
        );

        let architect = RoleConfig::default_for_role(Role::Architect);
        let developer = RoleConfig::default_for_role(Role::Developer);
        let security_reviewer = RoleConfig::default_for_role(Role::SecurityReviewer);

        let mut roles = BTreeMap::new();
        roles.insert(Role::Architect, architect.clone());
        roles.insert(Role::Developer, developer.clone());
        roles.insert(Role::SecurityReviewer, security_reviewer.clone());

        Self {
            shared_profiles: BTreeMap::new(),
            providers,
            profiles,
            roles,
            architect,
            developer,
            security_reviewer,
            discovery: AgentDiscoveryConfig::default(),
            source: AgentConfigSource {
                identity: "built-in:agent-default-v1".to_owned(),
                digest: sha256(DEFAULT_GLOBAL_CONFIG_TEMPLATE.as_bytes()),
            },
        }
    }
}

/// The resolved agent configuration input, retained for execution approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentConfigResolved {
    pub architect: RoleConfig,
    pub developer: RoleConfig,
    pub security_reviewer: RoleConfig,
    /// Identity and content digest of the resolver input that selected this config.
    pub source: AgentConfigSource,
}

/// Parsed project configuration required by Phase 1 commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectConfig {
    /// Component root relative to the project root.
    pub component_root: PathBuf,
    /// Resource bounds for component discovery.
    pub discovery: DiscoveryLimits,
    /// VCS selected for durable-artifact tracking inspection.
    pub vcs: VcsSelection,
    /// Configuration for external agent CLI execution.
    pub agent: AgentConfig,
    /// Test command execution policy.
    pub test_policy: Option<TestPolicy>,
    /// Required project-local isolation boundary for task execution.
    pub sandbox: Option<SandboxConfig>,
    /// Local checkpoint frequency for continuous component execution.
    pub task_run: TaskRunConfig,
    /// Component advisory review policy.
    pub review: ReviewConfig,
}

/// Approval-bound continuous execution policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TaskRunConfig {
    /// Checkpoint complete items by default, never human acceptance.
    pub commit: CommitFrequency,
}

/// Component advisory review policy.
///
/// When `required` is `true`, component acceptance requires a
/// current review receipt plus explicit human acknowledgement, or a
/// deliberate exception bound to the exact bundle digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewConfig {
    /// Whether component acceptance requires an acknowledged review receipt.
    /// Omit the entire section for optional review; a present section defaults to required.
    #[serde(default = "review_required_default")]
    pub required: bool,
}

fn review_required_default() -> bool {
    true
}

/// When continuous work creates local unaccepted checkpoint commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CommitFrequency {
    /// Commit after all lifecycle tasks for each item complete.
    #[default]
    Item,
    /// Commit after each successfully completed task.
    Task,
    /// Commit after all component tasks complete.
    Component,
    /// Leave changes for an explicit human commit.
    Manual,
}

impl CommitFrequency {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Task => "task",
            Self::Component => "component",
            Self::Manual => "manual",
        }
    }
}

/// Project-selected version-1 external sandbox runner contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SandboxConfig {
    /// Executable invoked directly, never through a shell.
    pub runner: String,
    /// Approval-bound absolute path to the canonical enforcement backend
    /// executable (Bubblewrap). Its identity is verified and bound by approval.
    pub backend: String,
    /// Environment names inherited by the runner and declared for its child.
    pub environment_allowlist: Vec<String>,
    /// Mediated Cargo dependency-acquisition policy. Canonical crates.io is
    /// always available; additional registries and Git sources are explicit.
    pub acquisition: AcquisitionConfig,
}

/// Bounded, deterministic mediated dependency-acquisition policy.
///
/// The canonical crates.io source is always available and is not represented in
/// [`AcquisitionConfig::additional_sources`]. Any additional registry or Git
/// source is explicit, exact, and bounded, mirroring the sandbox runner's own
/// acquisition source policy. This is part of the serialized sandbox
/// configuration, so it is bound into the authenticated execution approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct AcquisitionConfig {
    /// Bounds enforced on a promoted cache.
    pub cache_bounds: AcquisitionCacheBounds,
    /// Additional exact, bounded package sources beyond canonical crates.io.
    pub additional_sources: Vec<PackageSource>,
}

/// Bounds enforced on a mediated dependency cache during inspection and
/// promotion. Each value is nonzero and never exceeds the runner's safe maxima.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct AcquisitionCacheBounds {
    /// Maximum number of files eligible for promotion.
    pub max_files: u64,
    /// Maximum size in bytes of any single promoted file.
    pub max_file_bytes: u64,
    /// Maximum aggregate size in bytes of a promoted cache.
    pub max_cache_bytes: u64,
}

/// Default cache file count. Reconciled with the protocol promotion-manifest
/// maximum of 4096 entries, which bounds how many files a generation may hold.
pub const DEFAULT_ACQUISITION_MAX_FILES: u64 = 4096;
/// Default per-file size bound (256 MiB).
pub const DEFAULT_ACQUISITION_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// Default aggregate cache size bound (2 GiB).
pub const DEFAULT_ACQUISITION_MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Runner safe maxima the configured bounds must never exceed. `max_cache_files`
/// is bounded by the protocol promotion-manifest maximum (4096 entries), so a
/// configured file count can never exceed the number of manifest entries the
/// runner admits; the byte bounds mirror the runner's `MAX_*` resource ceilings.
const RUNNER_MAX_FILES: u64 = 4096;
const RUNNER_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const RUNNER_MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

impl Default for AcquisitionCacheBounds {
    fn default() -> Self {
        AcquisitionCacheBounds {
            max_files: DEFAULT_ACQUISITION_MAX_FILES,
            max_file_bytes: DEFAULT_ACQUISITION_MAX_FILE_BYTES,
            max_cache_bytes: DEFAULT_ACQUISITION_MAX_CACHE_BYTES,
        }
    }
}

/// One explicit, exact mediated dependency source beyond canonical crates.io.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PackageSource {
    /// An additional Cargo registry with exact canonical index and download
    /// origins.
    CargoRegistry {
        /// The registry name (never `crates-io`).
        name: String,
        /// The exact canonical index origin URL.
        index_origin: String,
        /// The exact canonical download origin URL.
        download_origin: String,
    },
    /// A Cargo Git dependency pinned to an exact repository and immutable
    /// revision.
    CargoGit {
        /// The exact canonical HTTPS repository URL.
        repository: String,
        /// The immutable 40-hex revision.
        revision: String,
    },
}

/// Supported VCS selection for durable-artifact tracking inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsSelection {
    /// Detect exactly one supported VCS; colocated Git/jj repositories require
    /// an explicit selection.
    Auto,
    /// Use Git's index and ignore semantics.
    Git,
    /// Use Jujutsu's current working-copy snapshot.
    Jujutsu,
}

/// Loads and validates `kvist.toml` from an explicit project root.
pub fn load(project_root: &Path) -> Result<ProjectConfig> {
    validate_project_root(project_root)?;
    let config_path = project_root.join("kvist.toml");
    let metadata = match fs::symlink_metadata(&config_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(KvistError::ProjectConfigurationMissing { path: config_path });
        }
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect project configuration",
                path: config_path,
                source,
            });
        }
    };
    let file_type = metadata.file_type();
    if is_link_like(&metadata) {
        return Err(KvistError::ProjectConfigurationIsSymlink { path: config_path });
    }
    if !file_type.is_file() {
        return Err(KvistError::ProjectConfigurationNotFile { path: config_path });
    }
    if metadata.len() > MAX_CONFIGURATION_BYTES {
        return Err(KvistError::ProjectConfigurationTooLarge {
            path: config_path,
            max_bytes: MAX_CONFIGURATION_BYTES,
        });
    }

    let contents = fs::read_to_string(&config_path).map_err(|source| KvistError::Io {
        operation: "read project configuration",
        path: config_path.clone(),
        source,
    })?;
    parse(&config_path, project_root, &contents)
}

fn validate_project_root(project_root: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(project_root).map_err(|source| KvistError::Io {
        operation: "inspect project root",
        path: project_root.to_path_buf(),
        source,
    })?;
    let file_type = metadata.file_type();
    if is_link_like(&metadata) {
        return Err(KvistError::ProjectRootIsSymlink {
            path: project_root.to_path_buf(),
        });
    }
    if !file_type.is_dir() {
        return Err(KvistError::ProjectRootNotDirectory {
            path: project_root.to_path_buf(),
        });
    }

    Ok(())
}

fn parse(config_path: &Path, project_root: &Path, contents: &str) -> Result<ProjectConfig> {
    let value: toml::Value =
        toml::from_str(contents).map_err(|error| KvistError::InvalidProjectConfiguration {
            path: config_path.to_path_buf(),
            reason: error.to_string(),
        })?;
    let table = value
        .as_table()
        .ok_or_else(|| invalid_configuration(config_path, "root value must be a TOML table"))?;
    let schema_version = table
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| invalid_configuration(config_path, "`schema_version` must be an integer"))?;
    if schema_version <= 0 {
        return Err(invalid_configuration(
            config_path,
            "`schema_version` must be a positive integer",
        ));
    }

    let supported_version = i64::from(CONFIGURATION_VERSION);
    if schema_version != supported_version {
        return Err(KvistError::UnsupportedProjectConfigurationVersion {
            path: config_path.to_path_buf(),
            version: schema_version,
            supported_version,
        });
    }
    let component_root = table
        .get("component_root")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`component_root` must be a relative string path",
            )
        })?;

    // The declared agent configuration surface nests provider, profile, and
    // role tables under `[agent]`. Top-level tables of those names are not
    // part of the accepted surface and must be rejected with a diagnostic that
    // names the offending table and points the user to the `agent` table, so a
    // silently accepted undeclared surface can never change which profile a
    // role selects without a recorded intent change.
    for name in ["providers", "profiles", "roles"] {
        if table.contains_key(name) {
            return Err(invalid_configuration(
                config_path,
                &format!(
                    "`{name}` is not a declared top-level configuration table; move it under `[agent.{name}]`"
                ),
            ));
        }
    }

    Ok(ProjectConfig {
        component_root: normalize_component_root(config_path, component_root)?,
        discovery: parse_discovery_limits(config_path, table)?,
        vcs: parse_vcs_selection(config_path, table)?,
        agent: load_agent_config(project_root, config_path, contents, table)?,
        test_policy: parse_test_policy(config_path, table)?,
        sandbox: parse_sandbox_config(config_path, table)?,
        task_run: table
            .get("task_run")
            .map(|value| {
                value.clone().try_into().map_err(|error| {
                    invalid_configuration(config_path, &format!("invalid task_run policy: {error}"))
                })
            })
            .transpose()?
            .unwrap_or_default(),
        review: table
            .get("review")
            .map(|value| {
                value.clone().try_into().map_err(|error| {
                    invalid_configuration(config_path, &format!("invalid review policy: {error}"))
                })
            })
            .transpose()?
            .unwrap_or_default(),
    })
}

/// Reads and normalizes only the `component_root` field of `kvist.toml`.
///
/// This is the minimal trust surface for layout classification: it applies the
/// same file hygiene and normalization as [`load`] (regular non-symlink file,
/// bounded size, relative path with only normal segments) without validating
/// unrelated sections such as `agent`, `test_policy`, or `sandbox`. A project
/// whose `component_root` is valid must be classified the same way even when
/// another section is malformed; full configuration validation is the caller's
/// responsibility.
pub(crate) fn load_component_root(project_root: &Path) -> Result<Option<PathBuf>> {
    let config_path = project_root.join("kvist.toml");
    let metadata = match fs::symlink_metadata(&config_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect project configuration",
                path: config_path,
                source,
            });
        }
    };
    let file_type = metadata.file_type();
    if is_link_like(&metadata) {
        return Err(KvistError::ProjectConfigurationIsSymlink { path: config_path });
    }
    if !file_type.is_file() {
        return Err(KvistError::ProjectConfigurationNotFile { path: config_path });
    }
    if metadata.len() > MAX_CONFIGURATION_BYTES {
        return Err(KvistError::ProjectConfigurationTooLarge {
            path: config_path,
            max_bytes: MAX_CONFIGURATION_BYTES,
        });
    }

    let contents = fs::read_to_string(&config_path).map_err(|source| KvistError::Io {
        operation: "read project configuration",
        path: config_path.clone(),
        source,
    })?;
    let value: toml::Value =
        toml::from_str(&contents).map_err(|error| KvistError::InvalidProjectConfiguration {
            path: config_path.clone(),
            reason: error.to_string(),
        })?;
    let component_root = value
        .get("component_root")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            invalid_configuration(
                &config_path,
                "`component_root` must be a relative string path",
            )
        })?;

    Ok(Some(normalize_component_root(
        &config_path,
        component_root,
    )?))
}

pub(crate) fn validate_project_configuration_contents(
    config_path: &Path,
    project_root: &Path,
    contents: &str,
) -> Result<()> {
    parse(config_path, project_root, contents).map(|_| ())
}

pub(crate) fn validate_agent_configuration_contents(
    config_path: &Path,
    contents: &str,
) -> Result<()> {
    let table = toml_table_from_str(config_path, contents)?;
    let source_path = if config_path.exists() {
        config_path
    } else {
        Path::new("built-in:agent-default-v1")
    };
    parse_agent_config_from_table(config_path, contents, source_path, &table).map(|_| ())
}

/// Loads one explicit agent configuration, including pinned shared references.
/// No discovery, probing, execution or filesystem writes occur.
pub fn load_agent_file(config_path: &Path) -> Result<AgentConfig> {
    let contents = read_agent_config_candidate(config_path, "explicit")?
        .ok_or_else(|| invalid_configuration(config_path, "agent configuration does not exist"))?;
    let table = toml_table_from_str(config_path, &contents)?;
    parse_agent_config_from_table(config_path, &contents, config_path, &table)
}

fn parse_sandbox_config(
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<Option<SandboxConfig>> {
    let Some(value) = table.get("sandbox") else {
        return Ok(None);
    };
    let sandbox = value
        .as_table()
        .ok_or_else(|| invalid_configuration(config_path, "`sandbox` must be a TOML table"))?;
    let schema_version = sandbox
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| {
            invalid_configuration(config_path, "`sandbox.schema_version` must be an integer")
        })?;
    if schema_version != 1 {
        return Err(invalid_configuration(
            config_path,
            "`sandbox.schema_version` must be 1",
        ));
    }
    let runner = sandbox
        .get("runner")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.trim().is_empty() && Path::new(value).is_absolute())
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`sandbox.runner` must be a nonblank absolute executable path",
            )
        })?;
    let backend = sandbox
        .get("backend")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.trim().is_empty() && Path::new(value).is_absolute())
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`sandbox.backend` must be a nonblank absolute enforcement-backend executable path",
            )
        })?;
    let network = sandbox
        .get("network")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| invalid_configuration(config_path, "`sandbox.network` must be `deny`"))?;
    if network != "deny" {
        return Err(invalid_configuration(
            config_path,
            "`sandbox.network` must be `deny`",
        ));
    }
    let mount = sandbox
        .get("mount")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| invalid_configuration(config_path, "`sandbox.mount` must be `component`"))?;
    if mount != "component" {
        return Err(invalid_configuration(
            config_path,
            "`sandbox.mount` must be `component`",
        ));
    }
    let values = sandbox
        .get("environment_allowlist")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`sandbox.environment_allowlist` must be an array of nonblank strings",
            )
        })?;
    if values.len() > MAX_SANDBOX_ENVIRONMENT_ENTRIES {
        return Err(invalid_configuration(
            config_path,
            "`sandbox.environment_allowlist` exceeds the 256-entry runner limit",
        ));
    }
    let mut environment_allowlist = Vec::with_capacity(values.len());
    for value in values {
        let name = value
            .as_str()
            .filter(|name| {
                name.len() <= MAX_SANDBOX_ENVIRONMENT_NAME_BYTES
                    && is_portable_environment_name(name)
            })
            .ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`sandbox.environment_allowlist` must contain portable identifiers no longer than 4096 bytes",
                )
            })?;
        if environment_allowlist
            .iter()
            .any(|existing| existing == name)
        {
            return Err(invalid_configuration(
                config_path,
                "`sandbox.environment_allowlist` must not contain duplicates",
            ));
        }
        environment_allowlist.push(name.to_owned());
    }
    let acquisition = parse_acquisition_config(config_path, sandbox)?;
    Ok(Some(SandboxConfig {
        runner: runner.to_owned(),
        backend: backend.to_owned(),
        environment_allowlist,
        acquisition,
    }))
}

/// Parses the optional `[sandbox.acquisition]` mediated dependency policy.
///
/// Canonical crates.io is always available and is never listed. Additional
/// registries and Git sources are exact and bounded; each origin and revision
/// is validated with the same rules the sandbox runner enforces so an approved
/// policy cannot describe a source the runner would reject.
fn parse_acquisition_config(
    config_path: &Path,
    sandbox: &toml::map::Map<String, toml::Value>,
) -> Result<AcquisitionConfig> {
    let Some(value) = sandbox.get("acquisition") else {
        return Ok(AcquisitionConfig::default());
    };
    let table = value.as_table().ok_or_else(|| {
        invalid_configuration(config_path, "`sandbox.acquisition` must be a TOML table")
    })?;

    let cache_bounds = AcquisitionCacheBounds {
        max_files: bounded_acquisition_limit(
            config_path,
            table,
            "max_cache_files",
            DEFAULT_ACQUISITION_MAX_FILES,
            RUNNER_MAX_FILES,
        )?,
        max_file_bytes: bounded_acquisition_limit(
            config_path,
            table,
            "max_cache_file_bytes",
            DEFAULT_ACQUISITION_MAX_FILE_BYTES,
            RUNNER_MAX_FILE_BYTES,
        )?,
        max_cache_bytes: bounded_acquisition_limit(
            config_path,
            table,
            "max_cache_bytes",
            DEFAULT_ACQUISITION_MAX_CACHE_BYTES,
            RUNNER_MAX_CACHE_BYTES,
        )?,
    };

    let mut additional_sources = Vec::new();
    let mut seen_names: Vec<String> = Vec::new();
    if let Some(registries) = table.get("registry") {
        let registries = registries.as_array().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`sandbox.acquisition.registry` must be an array of tables",
            )
        })?;
        for registry in registries {
            let registry = registry.as_table().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "each `sandbox.acquisition.registry` entry must be a table",
                )
            })?;
            let name = acquisition_string(config_path, registry, "registry", "name")?;
            let index_origin =
                acquisition_string(config_path, registry, "registry", "index_origin")?;
            let download_origin =
                acquisition_string(config_path, registry, "registry", "download_origin")?;
            let source = PackageSource::CargoRegistry {
                name: name.clone(),
                index_origin,
                download_origin,
            };
            crate::acquisition::validate_package_source(&source)
                .map_err(|reason| invalid_configuration(config_path, &reason))?;
            if name == crate::acquisition::CANONICAL_CRATES_IO_NAME {
                return Err(invalid_configuration(
                    config_path,
                    "`sandbox.acquisition.registry` must not redefine the built-in `crates-io` source",
                ));
            }
            if seen_names.contains(&name) {
                return Err(invalid_configuration(
                    config_path,
                    "`sandbox.acquisition.registry` names must be unique",
                ));
            }
            seen_names.push(name);
            additional_sources.push(source);
        }
    }
    if let Some(gits) = table.get("git") {
        let gits = gits.as_array().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`sandbox.acquisition.git` must be an array of tables",
            )
        })?;
        for git in gits {
            let git = git.as_table().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "each `sandbox.acquisition.git` entry must be a table",
                )
            })?;
            let repository = acquisition_string(config_path, git, "git", "repository")?;
            let revision = acquisition_string(config_path, git, "git", "revision")?;
            let source = PackageSource::CargoGit {
                repository,
                revision,
            };
            crate::acquisition::validate_package_source(&source)
                .map_err(|reason| invalid_configuration(config_path, &reason))?;
            additional_sources.push(source);
        }
    }

    let acquisition = AcquisitionConfig {
        cache_bounds,
        additional_sources,
    };
    crate::acquisition::validate_acquisition_config(&acquisition)
        .map_err(|reason| invalid_configuration(config_path, &reason.to_string()))?;
    Ok(acquisition)
}

fn bounded_acquisition_limit(
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    default: u64,
    maximum: u64,
) -> Result<u64> {
    let Some(value) = table.get(key) else {
        return Ok(default);
    };
    let integer = value
        .as_integer()
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                &format!("`sandbox.acquisition.{key}` must be a positive integer"),
            )
        })?;
    let integer = u64::try_from(integer).map_err(|_| {
        invalid_configuration(
            config_path,
            &format!("`sandbox.acquisition.{key}` must be a positive integer"),
        )
    })?;
    if integer > maximum {
        return Err(invalid_configuration(
            config_path,
            &format!(
                "`sandbox.acquisition.{key}` of {integer} exceeds the runner maximum of {maximum}"
            ),
        ));
    }
    Ok(integer)
}

fn acquisition_string(
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
    section: &str,
    key: &str,
) -> Result<String> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .filter(|value| !value.trim().is_empty() && value.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                &format!(
                    "`sandbox.acquisition.{section}.{key}` must be a nonblank string no longer than 4096 bytes"
                ),
            )
        })
}

fn is_portable_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(byte) if byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn parse_vcs_selection(
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<VcsSelection> {
    let Some(vcs) = table.get("vcs") else {
        return Ok(VcsSelection::Auto);
    };
    let vcs = vcs
        .as_table()
        .ok_or_else(|| invalid_configuration(config_path, "`vcs` must be a TOML table"))?;
    let kind = match vcs.get("kind") {
        Some(value) => value
            .as_str()
            .ok_or_else(|| invalid_configuration(config_path, "`vcs.kind` must be a string"))?,
        None => "auto",
    };

    match kind {
        "auto" => Ok(VcsSelection::Auto),
        "git" => Ok(VcsSelection::Git),
        "jj" => Ok(VcsSelection::Jujutsu),
        _ => Err(invalid_configuration(
            config_path,
            "`vcs.kind` must be `auto`, `git`, or `jj`",
        )),
    }
}

fn parse_discovery_limits(
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<DiscoveryLimits> {
    let Some(discovery) = table.get("discovery") else {
        return Ok(DiscoveryLimits::default());
    };
    let discovery = discovery
        .as_table()
        .ok_or_else(|| invalid_configuration(config_path, "`discovery` must be a TOML table"))?;
    let defaults = DiscoveryLimits::default();

    Ok(DiscoveryLimits {
        max_depth: parse_limit(
            config_path,
            discovery,
            "max_depth",
            defaults.max_depth,
            MAX_DISCOVERY_LIMITS.max_depth,
        )?,
        max_directories: parse_limit(
            config_path,
            discovery,
            "max_directories",
            defaults.max_directories,
            MAX_DISCOVERY_LIMITS.max_directories,
        )?,
        max_components: parse_limit(
            config_path,
            discovery,
            "max_components",
            defaults.max_components,
            MAX_DISCOVERY_LIMITS.max_components,
        )?,
        max_entries_per_directory: parse_limit(
            config_path,
            discovery,
            "max_entries_per_directory",
            defaults.max_entries_per_directory,
            MAX_DISCOVERY_LIMITS.max_entries_per_directory,
        )?,
        max_relative_path_bytes: parse_limit(
            config_path,
            discovery,
            "max_relative_path_bytes",
            defaults.max_relative_path_bytes,
            MAX_DISCOVERY_LIMITS.max_relative_path_bytes,
        )?,
    })
}

fn parse_limit(
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
    name: &str,
    default: usize,
    maximum: usize,
) -> Result<usize> {
    let Some(value) = table.get(name) else {
        return Ok(default);
    };
    let value = value.as_integer().ok_or_else(|| {
        invalid_configuration(
            config_path,
            &format!("`discovery.{name}` must be a positive integer"),
        )
    })?;
    let value = usize::try_from(value).ok().filter(|value| *value > 0);
    let Some(value) = value else {
        return Err(invalid_configuration(
            config_path,
            &format!("`discovery.{name}` must be a positive integer"),
        ));
    };
    if value > maximum {
        return Err(invalid_configuration(
            config_path,
            &format!("`discovery.{name}` must not exceed its hard maximum of {maximum}"),
        ));
    }
    Ok(value)
}

/// Parses an untrusted TOML integer as a positive `u64` bound.
///
/// TOML integers decode to `i64`, so a negative or oversized value must be
/// rejected rather than cast with `as u64`, which would wrap a negative value
/// into an enormous unsigned bound and silently disable a resource limit.
fn parse_bounded_unsigned(config_path: &Path, value: &toml::Value, field: &str) -> Result<u64> {
    let integer = value.as_integer().ok_or_else(|| {
        invalid_configuration(config_path, &format!("`{field}` must be an integer"))
    })?;
    let unsigned = u64::try_from(integer).map_err(|_| {
        invalid_configuration(
            config_path,
            &format!(
                "`{field}` must be a positive integer no greater than {}",
                u64::MAX
            ),
        )
    })?;
    if unsigned == 0 {
        return Err(invalid_configuration(
            config_path,
            &format!(
                "`{field}` must be a positive integer no greater than {}",
                u64::MAX
            ),
        ));
    }
    Ok(unsigned)
}

fn normalize_component_root(config_path: &Path, value: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(invalid_configuration(
            config_path,
            "`component_root` must be a non-empty relative path",
        ));
    }

    let mut has_cur_dir = false;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => has_cur_dir = true,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid_configuration(
                    config_path,
                    "`component_root` may contain only normal path segments",
                ));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        if has_cur_dir {
            return Ok(PathBuf::from("."));
        }
        return Err(invalid_configuration(
            config_path,
            "`component_root` must contain a directory name",
        ));
    }

    Ok(normalized)
}

fn invalid_configuration(config_path: &Path, reason: &str) -> KvistError {
    KvistError::InvalidProjectConfiguration {
        path: config_path.to_path_buf(),
        reason: reason.to_owned(),
    }
}

/// Returns the standard user-specific global configuration path for Kvist.
///
/// `KVIST_CONFIG_PATH` overrides resolution entirely; otherwise the path is
/// resolved from XDG conventions. Test isolation is the caller's responsibility
/// and is enforced in tests by setting `KVIST_CONFIG_PATH` or `XDG_CONFIG_HOME`
/// on the child process, never by special-casing Cargo inside this resolver.
pub fn global_user_config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("KVIST_CONFIG_PATH") {
        return Some(PathBuf::from(path));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join(".config"))
        });
    base.map(|path| path.join("kvist").join("config.toml"))
}

/// Returns the standard system-wide global configuration path for Kvist.
pub fn global_system_config_path() -> Option<PathBuf> {
    Some(PathBuf::from("/etc/kvist/config.toml"))
}

const DEFAULT_GLOBAL_CONFIG_TEMPLATE: &str = r#"# Kvist Global User Configuration

[agent.providers.claude]
type = "claude"
command_template = "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}"

[agent.providers.gemini]
type = "gemini-cli"
command_template = "gemini-cli --prompt '{prompt}' --files {context_files}"

[agent.profiles.claude-default]
provider = "claude"

[agent.profiles.gemini-default]
provider = "gemini"

[agent.roles.architect]
profile = "claude-default"
max_output_bytes = 16777216

[agent.roles.developer]
profile = "gemini-default"
max_output_bytes = 16777216

[agent.roles.security-reviewer]
profile = "claude-default"
max_output_bytes = 16777216
"#;

fn toml_table_from_str(path: &Path, contents: &str) -> Result<toml::map::Map<String, toml::Value>> {
    let value: toml::Value =
        toml::from_str(contents).map_err(|error| KvistError::InvalidProjectConfiguration {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    value
        .as_table()
        .cloned()
        .ok_or_else(|| invalid_configuration(path, "root value must be a TOML table"))
}

fn load_agent_config(
    project_root: &Path,
    project_config_path: &Path,
    project_contents: &str,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<AgentConfig> {
    // Priority 1: Check if `agent` table exists directly in the project-root `kvist.toml`
    if table.contains_key("agent")
        || table.contains_key("providers")
        || table.contains_key("profiles")
        || table.contains_key("roles")
    {
        let mut config = parse_agent_config_from_table(
            project_config_path,
            project_contents,
            project_config_path,
            table,
        )?;
        if table
            .get("agent")
            .and_then(toml::Value::as_table)
            .is_some_and(|agent| agent.contains_key("registry"))
        {
            return Ok(config);
        }
        // Inherit providers from user global config if not defined locally
        if let Some(user_path) = global_user_config_path()
            && let Some(contents) = read_agent_config_candidate(&user_path, "user")?
            && let Ok(user_table) = toml_table_from_str(&user_path, &contents)
        {
            let lookup = user_table
                .get("agent")
                .and_then(toml::Value::as_table)
                .unwrap_or(&user_table);
            if let Some(user_providers) = lookup.get("providers").and_then(toml::Value::as_table) {
                for (name, item) in user_providers {
                    if !config.providers.contains_key(name)
                        && let Some(p_table) = item.as_table()
                    {
                        let provider_type = p_table
                            .get("type")
                            .or_else(|| p_table.get("provider_type"))
                            .and_then(toml::Value::as_str)
                            .unwrap_or(name.as_str())
                            .to_owned();
                        let base_url = p_table
                            .get("base_url")
                            .and_then(toml::Value::as_str)
                            .map(str::to_owned);
                        let command_template = p_table
                            .get("command_template")
                            .and_then(toml::Value::as_str)
                            .map(str::to_owned);
                        let timeout_seconds = match p_table.get("timeout_seconds") {
                            Some(value) => Some(parse_bounded_unsigned(
                                project_config_path,
                                value,
                                &format!("user agent provider {name} timeout_seconds"),
                            )?),
                            None => None,
                        };
                        config.providers.insert(
                            name.clone(),
                            ProviderConfig {
                                name: name.clone(),
                                provider_type,
                                base_url,
                                command_template,
                                timeout_seconds,
                            },
                        );
                    }
                }
            }
            if let Some(user_profiles) = lookup.get("profiles").and_then(toml::Value::as_table) {
                for (name, item) in user_profiles {
                    if !config.profiles.contains_key(name)
                        && let Some(p_table) = item.as_table()
                    {
                        let provider = p_table
                            .get("provider")
                            .and_then(toml::Value::as_str)
                            .unwrap_or("custom")
                            .to_owned();
                        let model = p_table
                            .get("model")
                            .and_then(toml::Value::as_str)
                            .map(str::to_owned);
                        let command = p_table
                            .get("command")
                            .or_else(|| p_table.get("command_template"))
                            .and_then(toml::Value::as_str)
                            .map(str::to_owned);
                        let context_window = match p_table.get("context_window") {
                            Some(value) => Some(parse_bounded_unsigned(
                                project_config_path,
                                value,
                                &format!("user agent profile {name} context_window"),
                            )? as usize),
                            None => None,
                        };
                        let supports_thinking = p_table
                            .get("supports_thinking")
                            .and_then(toml::Value::as_bool);
                        let default_thinking_effort = p_table
                            .get("default_thinking_effort")
                            .and_then(toml::Value::as_str)
                            .and_then(sav::ReasoningEffort::parse_effort);
                        let temperature =
                            p_table.get("temperature").and_then(toml::Value::as_float);
                        let system_prompt = p_table
                            .get("system_prompt")
                            .and_then(toml::Value::as_str)
                            .map(str::to_owned);

                        config.profiles.insert(
                            name.clone(),
                            ProfileConfig {
                                name: name.clone(),
                                provider,
                                model,
                                context_window,
                                supports_thinking,
                                default_thinking_effort,
                                temperature,
                                system_prompt,
                                command,
                            },
                        );
                    }
                }
            }
            if let Some(user_roles) = lookup.get("roles").and_then(toml::Value::as_table) {
                for (name, item) in user_roles {
                    if let Some(role) = Role::from_str_case_insensitive(name) {
                        let role_cfg = config
                            .roles
                            .entry(role)
                            .or_insert_with(|| RoleConfig::default_for_role(role));
                        if (role_cfg.profile.is_empty() || role_cfg.profile.ends_with("-default"))
                            && let Some(r_table) = item.as_table()
                        {
                            if let Some(prof) = r_table
                                .get("profile")
                                .or_else(|| r_table.get("model"))
                                .and_then(toml::Value::as_str)
                            {
                                role_cfg.profile = prof.to_owned();
                            }
                            if let Some(effort_str) =
                                r_table.get("thinking_effort").and_then(toml::Value::as_str)
                            {
                                role_cfg.thinking_effort =
                                    sav::ReasoningEffort::parse_effort(effort_str);
                            }
                        }
                    }
                }
            }
            config.sync_roles_and_profiles();
        }
        return Ok(config);
    }

    // Priority 2: Check if project-local `.kvist/config.toml` exists
    let local_path = project_root.join(".kvist").join("config.toml");
    if let Some(contents) = read_agent_config_candidate(&local_path, "project-local")? {
        let parsed_table = toml_table_from_str(&local_path, &contents)?;
        return parse_agent_config_from_table(&local_path, &contents, &local_path, &parsed_table);
    }

    // Priority 3: Check global user-specific configuration path
    if let Some(user_path) = global_user_config_path()
        && let Some(contents) = read_agent_config_candidate(&user_path, "user")?
    {
        let parsed_table = toml_table_from_str(&user_path, &contents)?;
        return parse_agent_config_from_table(&user_path, &contents, &user_path, &parsed_table);
    }

    // Priority 4: Check global system-wide configuration path
    if let Some(system_path) = global_system_config_path()
        && let Some(contents) = read_agent_config_candidate(&system_path, "system")?
    {
        let parsed_table = toml_table_from_str(&system_path, &contents)?;
        return parse_agent_config_from_table(&system_path, &contents, &system_path, &parsed_table);
    }

    // The built-in default is a source identity, not an implicit filesystem write.
    let parsed_table = toml_table_from_str(Path::new("default"), DEFAULT_GLOBAL_CONFIG_TEMPLATE)?;
    parse_agent_config_from_table(
        Path::new("default"),
        DEFAULT_GLOBAL_CONFIG_TEMPLATE,
        Path::new("built-in:agent-default-v1"),
        &parsed_table,
    )
}

fn parse_agent_config_from_table(
    config_path: &Path,
    contents: &str,
    source_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<AgentConfig> {
    let mut config = AgentConfig {
        source: AgentConfigSource {
            identity: agent_source_identity(source_path)?,
            digest: sha256(contents.as_bytes()),
        },
        ..AgentConfig::default()
    };
    merge_agent_config_from_table(&mut config, config_path, table)?;
    resolve_shared_registry(&mut config, config_path, table)?;
    Ok(config)
}

fn resolve_shared_registry(
    config: &mut AgentConfig,
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<()> {
    use sav::registry::{Backend, RegistryPaths};
    let lookup = table
        .get("agent")
        .and_then(toml::Value::as_table)
        .unwrap_or(table);
    let Some(registry) = lookup.get("registry") else {
        return Ok(());
    };
    let registry = registry
        .as_table()
        .ok_or_else(|| invalid_configuration(config_path, "`agent.registry` must be a table"))?;
    if registry.keys().any(|key| key != "path") {
        return Err(invalid_configuration(
            config_path,
            "unknown agent.registry field",
        ));
    }
    if lookup.contains_key("providers")
        || lookup.contains_key("profiles")
        || table.contains_key("providers")
        || table.contains_key("profiles")
    {
        return Err(invalid_configuration(
            config_path,
            "shared registry references cannot be mixed with copied local providers/profiles",
        ));
    }
    if let Some(roles) = lookup.get("roles").and_then(toml::Value::as_table) {
        for role in roles.values().filter_map(toml::Value::as_table) {
            if role.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "profile"
                        | "thinking_effort"
                        | "timeout_seconds"
                        | "token_limit"
                        | "max_output_bytes"
                        | "redaction_values"
                )
            }) {
                return Err(invalid_configuration(
                    config_path,
                    "shared role bindings accept profile references and role policy only; no backend command/model overrides",
                ));
            }
        }
    }
    let base = config_path.parent().unwrap_or(Path::new("."));
    let project = if base.file_name().is_some_and(|name| name == ".kvist") {
        base.parent().unwrap_or(base)
    } else {
        base
    };
    let mut paths = RegistryPaths::for_project(project);
    if let Some(path) = registry.get("path") {
        let path = path.as_str().ok_or_else(|| {
            invalid_configuration(config_path, "`agent.registry.path` must be a string")
        })?;
        let path = PathBuf::from(path);
        paths.explicit = Some(if path.is_absolute() {
            path
        } else {
            base.join(path)
        });
    }
    let resolved = paths.load()?;
    config.shared_profiles = resolved
        .profiles()
        .filter(|p| p.profile.enabled)
        .map(|p| (p.name.clone(), p.clone()))
        .collect();
    config.providers.clear();
    config.profiles.clear();
    for (name, profile) in &config.shared_profiles {
        let (provider, base_url, model, context_window) = match &profile.profile.backend {
            Backend::Native(native) => (
                native.provider.to_string(),
                Some(native.base_url.clone()),
                Some(native.model.clone()),
                native.context_limit,
            ),
            Backend::ExternalAgent(agent) => (
                agent.provider.as_str().to_owned(),
                None,
                agent.model.clone(),
                None,
            ),
        };
        config
            .providers
            .entry(provider.clone())
            .or_insert_with(|| ProviderConfig {
                name: provider.clone(),
                provider_type: provider.clone(),
                base_url,
                // Inspection only. Execution must select the pinned typed backend.
                command_template: Some(String::new()),
                timeout_seconds: None,
            });
        config.profiles.insert(
            name.clone(),
            ProfileConfig {
                name: name.clone(),
                provider,
                model,
                context_window,
                supports_thinking: None,
                default_thinking_effort: None,
                temperature: None,
                system_prompt: None,
                command: Some(String::new()),
            },
        );
    }
    for role in config.roles.values_mut() {
        role.command_template.clear();
    }
    config.sync_roles_and_profiles();
    let mut binding = config.source.digest.as_bytes().to_vec();
    for profile in resolved.profiles() {
        binding.extend_from_slice(profile.name.as_bytes());
        binding.push(0);
        binding.extend_from_slice(profile.source.as_os_str().as_encoded_bytes());
        binding.push(0);
        binding.extend_from_slice(profile.digest.as_bytes());
        binding.push(0);
    }
    config.source.digest = sha256(&binding);
    Ok(())
}

fn merge_agent_config_from_table(
    config: &mut AgentConfig,
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<()> {
    let agent_table_opt =
        if let Some(agent) = table.get("agent") {
            Some(agent.as_table().ok_or_else(|| {
                invalid_configuration(config_path, "`agent` must be a TOML table")
            })?)
        } else {
            None
        };

    let lookup_table = agent_table_opt.unwrap_or(table);

    // 1. Discovery
    if let Some(discovery_val) = lookup_table.get("discovery") {
        let discovery_table = discovery_val.as_table().ok_or_else(|| {
            invalid_configuration(config_path, "`agent.discovery` must be a TOML table")
        })?;
        if let Some(url) = discovery_table.get("ollama_url") {
            let url_str = url.as_str().ok_or_else(|| {
                invalid_configuration(config_path, "`agent.discovery.ollama_url` must be a string")
            })?;
            config.discovery.ollama_url = Some(url_str.to_owned());
        }
        if let Some(url) = discovery_table.get("llama_server_url") {
            let url_str = url.as_str().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent.discovery.llama_server_url` must be a string",
                )
            })?;
            config.discovery.llama_server_url = Some(url_str.to_owned());
        }
        if let Some(timeout) = discovery_table.get("timeout_seconds") {
            let timeout_val =
                parse_bounded_unsigned(config_path, timeout, "agent.discovery.timeout_seconds")?;
            config.discovery.timeout_seconds = Some(timeout_val);
        }
        if let Some(allow) = discovery_table.get("allow_host_discovery") {
            let allow_val = allow.as_bool().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent.discovery.allow_host_discovery` must be a boolean",
                )
            })?;
            config.discovery.allow_host_discovery = Some(allow_val);
        }
    }

    // 2. Providers: [agent.providers.<name>] or [providers.<name>]
    let providers_item = lookup_table
        .get("providers")
        .or_else(|| table.get("providers"));
    if let Some(providers_val) = providers_item {
        let providers_table = providers_val.as_table().ok_or_else(|| {
            invalid_configuration(config_path, "`agent.providers` must be a TOML table")
        })?;
        if !providers_table.is_empty() {
            config.providers.clear();
        }
        for (name, item) in providers_table {
            let p_table = item.as_table().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    &format!("`agent.providers.{name}` must be a TOML table"),
                )
            })?;
            let provider_type = p_table
                .get("type")
                .or_else(|| p_table.get("provider_type"))
                .and_then(toml::Value::as_str)
                .unwrap_or(name.as_str())
                .to_owned();
            let base_url = p_table
                .get("base_url")
                .and_then(toml::Value::as_str)
                .map(str::to_owned);
            let command_template = p_table
                .get("command_template")
                .and_then(toml::Value::as_str)
                .map(str::to_owned);
            let timeout_seconds = match p_table.get("timeout_seconds") {
                Some(value) => Some(parse_bounded_unsigned(
                    config_path,
                    value,
                    &format!("agent.providers.{name}.timeout_seconds"),
                )?),
                None => None,
            };
            config.providers.insert(
                name.clone(),
                ProviderConfig {
                    name: name.clone(),
                    provider_type,
                    base_url,
                    command_template,
                    timeout_seconds,
                },
            );
        }
    }

    // 3. Profiles: [agent.profiles.<name>] or [profiles.<name>]
    let profiles_item = lookup_table
        .get("profiles")
        .or_else(|| table.get("profiles"));
    if let Some(profiles_val) = profiles_item {
        let profiles_table = profiles_val.as_table().ok_or_else(|| {
            invalid_configuration(config_path, "`agent.profiles` must be a TOML table")
        })?;
        if !profiles_table.is_empty() {
            config.profiles.clear();
        }
        for (name, item) in profiles_table {
            let p_table = item.as_table().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    &format!("`agent.profiles.{name}` must be a TOML table"),
                )
            })?;

            // Legacy role migration, made explicit. A profile table is migrated
            // into a role binding only when it is unambiguously a legacy role
            // definition: it carries at least one role-only key and no
            // unambiguous profile field. Carrying any unambiguous profile field
            // (e.g. `provider`) marks the table as a profile, so it is never
            // silently repointed into a role binding merely because a role-only
            // key is also present. `model` is deliberately *not* a disqualifier:
            // it is ambiguous, naming both a legacy role's default model and a
            // profile's model, so a table with `model` plus a role-only key such
            // as `models` remains an unambiguous legacy role. Only a shape that
            // also carries an unambiguous profile field (e.g. `provider`) is
            // content-ambiguous and is left to be parsed as a profile. This keeps
            // the migration a pure, deterministic function of the declared
            // surface rather than a content-inferred guess.
            let has_profile_field = p_table.keys().any(|key| {
                matches!(
                    key.as_str(),
                    "provider"
                        | "context_window"
                        | "supports_thinking"
                        | "default_thinking_effort"
                        | "temperature"
                        | "system_prompt"
                        | "command"
                )
            });
            let has_role_only_key = p_table.keys().any(|key| {
                matches!(
                    key.as_str(),
                    "command_template"
                        | "models"
                        | "token_limit"
                        | "timeout_seconds"
                        | "thinking_effort"
                        | "reasoning_effort"
                        | "redaction_values"
                        | "redaction"
                        | "max_output_bytes"
                )
            });
            let legacy_role = Role::from_str_case_insensitive(name)
                .filter(|_| has_role_only_key && !has_profile_field);

            if let Some(role) = legacy_role {
                let role_cfg = config
                    .roles
                    .entry(role)
                    .or_insert_with(|| RoleConfig::default_for_role(role));
                if let Some(template) = p_table.get("command_template") {
                    let template_str = template.as_str().ok_or_else(|| {
                        invalid_configuration(
                            config_path,
                            &format!("`agent.profiles.{name}.command_template` must be a string"),
                        )
                    })?;
                    role_cfg.command_template = template_str.to_owned();
                    config.profiles.insert(
                        name.clone(),
                        ProfileConfig {
                            name: name.clone(),
                            provider: "custom".to_owned(),
                            model: None,
                            context_window: None,
                            supports_thinking: None,
                            default_thinking_effort: None,
                            temperature: None,
                            system_prompt: None,
                            command: Some(template_str.to_owned()),
                        },
                    );
                    role_cfg.profile = name.clone();
                }
                if let Some(model) = p_table.get("model").and_then(toml::Value::as_str) {
                    role_cfg.profile = model.to_owned();
                }
                if let Some(models) = p_table.get("models") {
                    let parsed_models = parse_agent_models(config_path, models)?;
                    for m in &parsed_models {
                        config.profiles.insert(
                            m.name.clone(),
                            ProfileConfig {
                                name: m.name.clone(),
                                provider: "custom".to_owned(),
                                model: None,
                                context_window: None,
                                supports_thinking: None,
                                default_thinking_effort: None,
                                temperature: None,
                                system_prompt: m.system_prompt.clone(),
                                command: Some(m.command.clone()),
                            },
                        );
                    }
                }
                parse_agent_resource_policy(config_path, p_table, role_cfg)?;
            } else {
                let provider = p_table
                    .get("provider")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("custom")
                    .to_owned();
                let model = p_table
                    .get("model")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned);
                let command = p_table
                    .get("command")
                    .or_else(|| p_table.get("command_template"))
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned);
                let context_window = match p_table.get("context_window") {
                    Some(value) => Some(parse_bounded_unsigned(
                        config_path,
                        value,
                        &format!("agent.profiles.{name}.context_window"),
                    )? as usize),
                    None => None,
                };
                let supports_thinking = p_table
                    .get("supports_thinking")
                    .and_then(toml::Value::as_bool);
                let default_thinking_effort = p_table
                    .get("default_thinking_effort")
                    .and_then(toml::Value::as_str)
                    .and_then(sav::ReasoningEffort::parse_effort);
                let temperature = p_table.get("temperature").and_then(toml::Value::as_float);
                let system_prompt = p_table
                    .get("system_prompt")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned);

                config.profiles.insert(
                    name.clone(),
                    ProfileConfig {
                        name: name.clone(),
                        provider,
                        model,
                        context_window,
                        supports_thinking,
                        default_thinking_effort,
                        temperature,
                        system_prompt,
                        command,
                    },
                );
            }
        }
    }

    // 4. Roles: [agent.roles.<name>] or [roles.<name>]
    let roles_item = lookup_table.get("roles").or_else(|| table.get("roles"));
    if let Some(roles_val) = roles_item {
        let roles_table = roles_val.as_table().ok_or_else(|| {
            invalid_configuration(config_path, "`agent.roles` must be a TOML table")
        })?;
        for (name, item) in roles_table {
            let role = Role::from_str_case_insensitive(name).ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    &format!(
                        "unknown agent role `{name}`: expected architect, developer, or security-reviewer"
                    ),
                )
            })?;
            let r_table = item.as_table().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    &format!("`agent.roles.{name}` must be a TOML table"),
                )
            })?;

            let role_cfg = config
                .roles
                .entry(role)
                .or_insert_with(|| RoleConfig::default_for_role(role));

            if let Some(profile_val) = r_table.get("profile").or_else(|| r_table.get("model")) {
                let profile_str = profile_val.as_str().ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        &format!("`agent.roles.{name}.profile` must be a string"),
                    )
                })?;
                role_cfg.profile = profile_str.to_owned();
            }

            if let Some(cmd_val) = r_table.get("command_template") {
                let cmd_str = cmd_val.as_str().ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        &format!("`agent.roles.{name}.command_template` must be a string"),
                    )
                })?;
                role_cfg.command_template = cmd_str.to_owned();
                config.profiles.insert(
                    name.clone(),
                    ProfileConfig {
                        name: name.clone(),
                        provider: "custom".to_owned(),
                        model: None,
                        context_window: None,
                        supports_thinking: None,
                        default_thinking_effort: None,
                        temperature: None,
                        system_prompt: None,
                        command: Some(cmd_str.to_owned()),
                    },
                );
                role_cfg.profile = name.clone();
            }

            if let Some(effort_val) = r_table
                .get("thinking_effort")
                .or_else(|| r_table.get("reasoning_effort"))
            {
                let effort_str = effort_val.as_str().ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        &format!("`agent.roles.{name}.thinking_effort` must be a string"),
                    )
                })?;
                let effort = sav::ReasoningEffort::parse_effort(effort_str).ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        &format!("invalid `agent.roles.{name}.thinking_effort`: `{effort_str}`"),
                    )
                })?;
                role_cfg.thinking_effort = Some(effort);
            }

            parse_agent_resource_policy(config_path, r_table, role_cfg)?;
        }
    }

    config.sync_roles_and_profiles();
    Ok(())
}

fn parse_agent_resource_policy(
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
    profile: &mut RoleConfig,
) -> Result<()> {
    if table.contains_key("timeout_seconds") {
        profile.timeout_seconds = Some(parse_agent_limit(
            config_path,
            table,
            "timeout_seconds",
            0,
            MAX_AGENT_TIMEOUT_SECONDS as usize,
        )? as u64);
    }
    profile.max_output_bytes = parse_agent_limit(
        config_path,
        table,
        "max_output_bytes",
        profile.max_output_bytes,
        MAX_AGENT_OUTPUT_BYTES,
    )?;
    if let Some(limit) = table.get("token_limit") {
        let limit = limit.as_integer().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`agent profile token_limit` must be a positive integer",
            )
        })?;
        let limit = usize::try_from(limit)
            .ok()
            .filter(|val| *val > 0)
            .ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent profile token_limit` must be a positive integer",
                )
            })?;
        profile.token_limit = Some(limit);
    }
    let (values, field) = if let Some(values) = table.get("redaction_values") {
        if table.contains_key("redaction") {
            return Err(invalid_configuration(
                config_path,
                "configure only one role redaction form",
            ));
        }
        (values, "redaction_values")
    } else if let Some(redaction) = table.get("redaction") {
        let redaction = redaction.as_table().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`agent profile redaction` must be a TOML table",
            )
        })?;
        let values = redaction.get("values").ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`agent profile redaction.values` must be an array of nonblank strings",
            )
        })?;
        (values, "redaction.values")
    } else {
        return Ok(());
    };
    let values = values.as_array().ok_or_else(|| {
        invalid_configuration(
            config_path,
            &format!("`agent profile {field}` must be an array of nonblank strings"),
        )
    })?;
    let mut parsed = Vec::with_capacity(values.len());
    for value in values {
        let value = value
            .as_str()
            .filter(|value| !value.trim().is_empty() && value.len() <= 4_096)
            .ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    &format!("`agent profile {field}` must contain nonblank strings no longer than 4096 bytes"),
                )
            })?;
        if parsed.iter().any(|existing| existing == value) {
            return Err(invalid_configuration(
                config_path,
                &format!("`agent profile {field}` must not contain duplicates"),
            ));
        }

        parsed.push(value.to_owned());
    }
    if parsed.is_empty() {
        return Err(invalid_configuration(
            config_path,
            &format!("`agent profile {field}` must not be empty"),
        ));
    }
    profile.redaction_values = parsed;
    Ok(())
}

fn parse_agent_models(config_path: &Path, value: &toml::Value) -> Result<Vec<Model>> {
    let models = value
        .as_array()
        .filter(|models| !models.is_empty())
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`agent profile models` must be a nonempty array of tables",
            )
        })?;
    let mut parsed = Vec::with_capacity(models.len());
    for value in models {
        let model = value.as_table().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`agent profile models` must contain only tables",
            )
        })?;
        let name = model
            .get("name")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent profile models[].name` must be a nonblank string",
                )
            })?;
        let command = model
            .get("command")
            .and_then(toml::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent profile models[].command` must be a nonblank string",
                )
            })?;
        if parsed.iter().any(|existing: &Model| existing.name == name) {
            return Err(invalid_configuration(
                config_path,
                "`agent profile models` must not contain duplicate names",
            ));
        }
        let system_prompt = match model.get("system_prompt") {
            Some(value) => Some(
                value
                    .as_str()
                    .ok_or_else(|| {
                        invalid_configuration(
                            config_path,
                            "`agent profile models[].system_prompt` must be a string",
                        )
                    })?
                    .to_owned(),
            ),
            None => None,
        };
        parsed.push(Model {
            name: name.to_owned(),
            command: command.to_owned(),
            system_prompt,
        });
    }
    Ok(parsed)
}

fn parse_agent_limit(
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
    name: &str,
    default: usize,
    maximum: usize,
) -> Result<usize> {
    let Some(value) = table.get(name) else {
        return Ok(default);
    };
    let value = value
        .as_integer()
        .and_then(|value| usize::try_from(value).ok());
    let Some(value) = value.filter(|value| *value > 0 && *value <= maximum) else {
        return Err(invalid_configuration(
            config_path,
            &format!("`agent profile {name}` must be a positive integer no greater than {maximum}"),
        ));
    };
    Ok(value)
}

fn agent_source_identity(path: &Path) -> Result<String> {
    if path == Path::new("built-in:agent-default-v1") {
        return Ok(path.to_string_lossy().into_owned());
    }
    match path.canonicalize() {
        Ok(path) => Ok(path.to_string_lossy().into_owned()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| KvistError::Io {
                operation: "resolve agent configuration parent",
                path: path.to_path_buf(),
                source: io::Error::other("configuration path has no parent"),
            })?;
            let file_name = path.file_name().ok_or_else(|| KvistError::Io {
                operation: "resolve agent configuration filename",
                path: path.to_path_buf(),
                source: io::Error::other("configuration path has no filename"),
            })?;
            parent
                .canonicalize()
                .map(|parent| parent.join(file_name).to_string_lossy().into_owned())
                .map_err(|source| KvistError::Io {
                    operation: "canonicalize agent configuration parent",
                    path: parent.to_path_buf(),
                    source,
                })
        }
        Err(source) => Err(KvistError::Io {
            operation: "canonicalize agent configuration source",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn read_agent_config_candidate(path: &Path, source_name: &'static str) -> Result<Option<String>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect agent configuration source",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if is_link_like(&metadata)
        || !metadata.file_type().is_file()
        || metadata.len() > MAX_CONFIGURATION_BYTES
    {
        return Err(invalid_configuration(
            path,
            &format!(
                "{source_name} agent configuration must be a regular non-link file no larger than {MAX_CONFIGURATION_BYTES} bytes"
            ),
        ));
    }
    fs::read_to_string(path)
        .map(Some)
        .map_err(|source| KvistError::Io {
            operation: "read agent configuration source",
            path: path.to_path_buf(),
            source,
        })
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// Bounded test command policy for explicit trust boundaries.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TestPolicy {
    /// Schema version for the test policy.
    pub schema_version: i64,
    /// Working directory for executing tests ("component" or "project").
    pub working_directory: String,
    /// Allowlist of environment variables to preserve during test execution.
    pub environment_allowlist: Vec<String>,
    /// Subprocess execution timeout in seconds.
    pub timeout_seconds: u64,
    /// Output buffer byte limit cap for stdout/stderr capture.
    pub max_output_bytes: usize,
    /// Approved test-command entries mapped to components.
    pub commands: Vec<TestCommandEntry>,
}

/// A single approved test command entry mapped to a component.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TestCommandEntry {
    /// Target component relative path from component root.
    pub component: String,
    /// Command template to execute for verification.
    pub command: String,
}

/// Computes the canonical SHA-256 hash of a test policy.
///
/// The hash is a function of the fully serialized policy, so any change to the
/// command set, environment allowlist, timeout, or output cap changes the
/// digest. Serialization is fallible: a policy that cannot be serialized
/// cannot be bound to a digest, so the error is propagated rather than
/// swallowed (which would collapse the digest to a constant and decouple it
/// from the command set).
pub fn compute_policy_hash(policy: &TestPolicy) -> Result<String> {
    let serialized =
        serde_json::to_string(policy).map_err(|error| KvistError::UnapprovedExecutionPolicy {
            reason: format!("cannot serialize test policy for digest: {error}"),
        })?;
    Ok(sha256(serialized.as_bytes()))
}

fn parse_test_policy(
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<Option<TestPolicy>> {
    let Some(policy_val) = table.get("test_policy") else {
        return Ok(None);
    };
    let policy = policy_val
        .as_table()
        .ok_or_else(|| invalid_configuration(config_path, "`test_policy` must be a TOML table"))?;

    let schema_version = policy
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`test_policy.schema_version` must be an integer",
            )
        })?;
    if schema_version != 1 {
        return Err(invalid_configuration(
            config_path,
            "`test_policy.schema_version` must be 1",
        ));
    }

    let working_directory = policy
        .get("working_directory")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`test_policy.working_directory` must be a string",
            )
        })?;
    if working_directory != "component" && working_directory != "project" {
        return Err(invalid_configuration(
            config_path,
            "`test_policy.working_directory` must be either 'component' or 'project'",
        ));
    }

    let env_allowlist_val = policy
        .get("environment_allowlist")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`test_policy.environment_allowlist` must be an array of strings",
            )
        })?;
    if env_allowlist_val.len() > MAX_SANDBOX_ENVIRONMENT_ENTRIES {
        return Err(invalid_configuration(
            config_path,
            "`test_policy.environment_allowlist` exceeds the 256-entry runner limit",
        ));
    }
    let mut environment_allowlist = Vec::new();
    for val in env_allowlist_val {
        let s = val.as_str().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`test_policy.environment_allowlist` must be an array of strings",
            )
        })?;
        if s.len() > MAX_SANDBOX_ENVIRONMENT_NAME_BYTES || !is_portable_environment_name(s) {
            return Err(invalid_configuration(
                config_path,
                "`test_policy.environment_allowlist` must contain portable identifiers no longer than 4096 bytes",
            ));
        }
        environment_allowlist.push(s.to_owned());
    }

    let timeout_seconds = policy
        .get("timeout_seconds")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`test_policy.timeout_seconds` must be a positive integer",
            )
        })?;
    let timeout_seconds = u64::try_from(timeout_seconds).map_err(|_| {
        invalid_configuration(
            config_path,
            "`test_policy.timeout_seconds` must be a positive integer",
        )
    })?;
    if timeout_seconds == 0 {
        return Err(invalid_configuration(
            config_path,
            "`test_policy.timeout_seconds` must be a positive integer",
        ));
    }

    let max_output_bytes = policy
        .get("max_output_bytes")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`test_policy.max_output_bytes` must be a positive integer",
            )
        })?;
    let max_output_bytes = usize::try_from(max_output_bytes).map_err(|_| {
        invalid_configuration(
            config_path,
            "`test_policy.max_output_bytes` must be a positive integer",
        )
    })?;
    if max_output_bytes == 0 {
        return Err(invalid_configuration(
            config_path,
            "`test_policy.max_output_bytes` must be a positive integer",
        ));
    }

    let commands_val = policy
        .get("commands")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`test_policy.commands` must be an array of tables",
            )
        })?;
    let mut commands = Vec::new();
    for val in commands_val {
        let cmd_table = val.as_table().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`test_policy.commands` must be an array of tables",
            )
        })?;
        let component = cmd_table
            .get("component")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`test_policy.commands.component` must be a string",
                )
            })?;
        let command = cmd_table
            .get("command")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`test_policy.commands.command` must be a string",
                )
            })?;
        commands.push(TestCommandEntry {
            component: component.to_owned(),
            command: command.to_owned(),
        });
    }

    Ok(Some(TestPolicy {
        schema_version,
        working_directory: working_directory.to_owned(),
        environment_allowlist,
        timeout_seconds,
        max_output_bytes,
        commands,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_role_redaction_values_are_applied_and_validated() {
        let table: toml::Table =
            toml::from_str("redaction_values = [\"first\", \"second\"]").unwrap();
        let mut profile = RoleConfig::default_for_role(Role::Developer);
        parse_agent_resource_policy(Path::new("kvist.toml"), &table, &mut profile).unwrap();
        assert_eq!(profile.redaction_values, ["first", "second"]);
        for invalid in [
            "redaction_values = []",
            "redaction_values = [true]",
            "redaction_values = [\"\"]",
            "redaction_values = [\" \"]",
            "redaction_values = [\"\\t\\n\"]",
            "[redaction]\nvalues = [\" \"]",
            "redaction_values = [\"same\", \"same\"]",
            "redaction_values = [\"one\"]\n[redaction]\nvalues = [\"two\"]",
        ] {
            let table: toml::Table = toml::from_str(invalid).unwrap();
            assert!(
                parse_agent_resource_policy(Path::new("kvist.toml"), &table, &mut profile).is_err()
            );
        }
    }

    #[test]
    fn runtime_budget_defaults_match_generated_configuration() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        std::fs::write(&path, DEFAULT_GLOBAL_CONFIG_TEMPLATE).unwrap();
        let generated = load_agent_file(&path).unwrap();
        for role in [Role::Architect, Role::Developer, Role::SecurityReviewer] {
            for profile in [
                RoleConfig::default_for_role(role),
                generated.role(role).clone(),
            ] {
                assert_eq!(profile.timeout_seconds, None);
                assert_eq!(profile.max_output_bytes, 16 * 1024 * 1024);
            }
        }
    }

    #[test]
    fn runtime_budget_output_boundary_and_explicit_small_caps_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config.toml");
        for bytes in [1, 65_536, 16 * 1024 * 1024] {
            std::fs::write(
                &path,
                format!(
                    "[agent.roles.architect]\ntimeout_seconds = 17\nmax_output_bytes = {bytes}\n"
                ),
            )
            .unwrap();
            let config = load_agent_file(&path).unwrap();
            assert_eq!(config.architect.timeout_seconds, Some(17));
            assert_eq!(config.architect.max_output_bytes, bytes);
        }
        for bytes in [0, 16 * 1024 * 1024 + 1] {
            std::fs::write(
                &path,
                format!("[agent.roles.architect]\nmax_output_bytes = {bytes}\n"),
            )
            .unwrap();
            assert!(load_agent_file(&path).is_err(), "{bytes}");
        }
    }

    /// Writes `contents` to `<root>/kvist.toml` and returns the project root.
    fn write_project_config(root: &Path, contents: &str) {
        std::fs::write(root.join("kvist.toml"), contents).expect("write kvist.toml");
    }

    /// `load` reads purely from the injected project root, so these tests
    /// never touch the real user configuration and stay hermetic under the
    /// parallel runner.
    #[test]
    fn loads_component_root_from_an_injected_config_path() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n",
        );
        let config = load(root.path()).expect("load project config");
        assert_eq!(config.component_root, std::path::PathBuf::from("src"));
    }

    #[test]
    fn reports_a_missing_project_configuration() {
        let root = tempfile::tempdir().expect("tempdir");
        match load(root.path()) {
            Err(KvistError::ProjectConfigurationMissing { .. }) => {}
            other => panic!("expected ProjectConfigurationMissing, got {other:?}"),
        }
    }

    #[test]
    fn rejects_an_unsupported_schema_version() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 999\ncomponent_root = \"src\"\n",
        );
        match load(root.path()) {
            Err(KvistError::UnsupportedProjectConfigurationVersion { .. }) => {}
            other => panic!("expected UnsupportedProjectConfigurationVersion, got {other:?}"),
        }
    }

    #[test]
    fn rejects_a_config_that_exceeds_the_size_limit() {
        let root = tempfile::tempdir().expect("tempdir");
        let padding = "x".repeat(MAX_CONFIGURATION_BYTES as usize + 1);
        write_project_config(
            root.path(),
            &format!("schema_version = 1\ncomponent_root = \"src\"\nbig = \"{padding}\"\n"),
        );
        match load(root.path()) {
            Err(KvistError::ProjectConfigurationTooLarge { .. }) => {}
            other => panic!("expected ProjectConfigurationTooLarge, got {other:?}"),
        }
    }

    // --- Agent configuration surface: pinning the accepted contract ---
    //
    // These tests define the configuration surface that agent loading must
    // accept and reject. They fail on the current (broken) surface and pass
    // once the surface is pinned: top-level provider/profile/role tables are
    // rejected with actionable diagnostics, the legacy role migration is
    // explicit and deterministic, and profile equality is total even for NaN
    // temperatures.

    /// The accepted surface nests provider, profile, and role tables under the
    /// `agent` table. A top-level `[providers.<name>]` table is not part of the
    /// declared surface and must be rejected with a diagnostic that names the
    /// offending table and points the user to the `agent` table.
    #[test]
    fn rejects_a_top_level_provider_table_outside_the_agent_table() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[providers.local-llama]\ntype = \"llama-server\"\nbase_url = \"http://127.0.0.1:9931\"\n",
        );
        match load(root.path()) {
            Err(KvistError::InvalidProjectConfiguration { reason, .. }) => {
                assert!(
                    reason.to_lowercase().contains("providers"),
                    "diagnostic must name the offending table, got: {reason}"
                );
                assert!(
                    reason.to_lowercase().contains("agent"),
                    "diagnostic must point to the agent table, got: {reason}"
                );
            }
            other => panic!("expected InvalidProjectConfiguration, got {other:?}"),
        }
    }

    /// A top-level `[profiles.<name>]` table is not part of the declared
    /// surface and must be rejected with an actionable diagnostic.
    #[test]
    fn rejects_a_top_level_profile_table_outside_the_agent_table() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[profiles.qwen-coder]\nprovider = \"custom\"\nmodel = \"qwen-coder-32b\"\n",
        );
        match load(root.path()) {
            Err(KvistError::InvalidProjectConfiguration { reason, .. }) => {
                assert!(
                    reason.to_lowercase().contains("profiles"),
                    "diagnostic must name the offending table, got: {reason}"
                );
                assert!(
                    reason.to_lowercase().contains("agent"),
                    "diagnostic must point to the agent table, got: {reason}"
                );
            }
            other => panic!("expected InvalidProjectConfiguration, got {other:?}"),
        }
    }

    /// A top-level `[roles.<name>]` table is not part of the declared surface
    /// and must be rejected with an actionable diagnostic.
    #[test]
    fn rejects_a_top_level_role_table_outside_the_agent_table() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[roles.architect]\nprofile = \"claude-default\"\n",
        );
        match load(root.path()) {
            Err(KvistError::InvalidProjectConfiguration { reason, .. }) => {
                assert!(
                    reason.to_lowercase().contains("roles"),
                    "diagnostic must name the offending table, got: {reason}"
                );
                assert!(
                    reason.to_lowercase().contains("agent"),
                    "diagnostic must point to the agent table, got: {reason}"
                );
            }
            other => panic!("expected InvalidProjectConfiguration, got {other:?}"),
        }
    }

    /// The legacy role migration must be deterministic: loading the same legacy
    /// configuration repeatedly yields identical results. The migration is a
    /// pure function of the configuration content, with no dependence on
    /// environment, ordering, or prior state.
    #[test]
    fn legacy_role_migration_is_deterministic() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[agent.profiles.architect]\ncommand_template = \"legacy-architect --prompt '{prompt}'\"\n",
        );
        let first = load(root.path()).expect("first legacy load");
        let second = load(root.path()).expect("second legacy load");
        assert_eq!(
            first.agent, second.agent,
            "legacy migration must be a pure, deterministic function of the configuration"
        );
    }

    /// The legacy role migration must be explicit, not inferred from content.
    /// A profile table that carries profile fields (e.g. `provider`) is a
    /// profile, even if it also carries a role-only key; it must not be
    /// silently migrated into a role binding. The current content-based
    /// heuristic migrates it merely because a role-only key is present.
    #[test]
    fn legacy_role_migration_is_explicit_not_content_inferred() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[agent.profiles.architect]\nprovider = \"custom\"\nmodel = \"architect-model\"\ntimeout_seconds = 900\n",
        );
        match load(root.path()) {
            Ok(config) => {
                // A table with profile fields is a profile, not a legacy role:
                // the architect role must retain its default profile binding
                // rather than being silently repointed.
                assert_eq!(
                    config.agent.role(Role::Architect).profile,
                    "claude-default",
                    "a profile table must not be silently migrated into a role binding"
                );
            }
            // Rejecting the ambiguous table outright is also an explicit,
            // acceptable surface; what is not acceptable is silent migration.
            Err(KvistError::InvalidProjectConfiguration { .. }) => {}
            other => panic!("expected a profile or a rejection, got {other:?}"),
        }
    }

    /// Profile equality must be total: a profile with a NaN temperature is
    /// equal to itself and to a clone. The derived `PartialEq` compares the
    /// `f64` temperature with `==`, which is not reflexive for NaN, so the
    /// manual `Eq` impl is unsound until equality is made NaN-safe.
    #[test]
    fn profile_equality_is_reflexive_for_a_nan_temperature() {
        let profile = ProfileConfig {
            name: "nan-profile".to_owned(),
            provider: "custom".to_owned(),
            model: None,
            context_window: None,
            supports_thinking: None,
            default_thinking_effort: None,
            temperature: Some(f64::NAN),
            system_prompt: None,
            command: None,
        };
        let clone = profile.clone();
        assert_eq!(
            profile, clone,
            "a profile must equal its own clone even with a NaN temperature"
        );
    }

    /// Two profiles that differ only in an unbounded floating-point temperature
    /// that is NaN must compare equal: equality is well-defined (total) rather
    /// than collapsing to `false` under the derived `f64` comparison.
    #[test]
    fn profile_equality_is_total_across_nan_temperatures() {
        let a = ProfileConfig {
            name: "p".to_owned(),
            provider: "custom".to_owned(),
            model: None,
            context_window: None,
            supports_thinking: None,
            default_thinking_effort: None,
            temperature: Some(f64::NAN),
            system_prompt: None,
            command: None,
        };
        let b = ProfileConfig {
            name: "p".to_owned(),
            provider: "custom".to_owned(),
            model: None,
            context_window: None,
            supports_thinking: None,
            default_thinking_effort: None,
            temperature: Some(f64::NAN),
            system_prompt: None,
            command: None,
        };
        assert_eq!(a, b, "two NaN-temperature profiles must compare equal");
    }

    // --- Untrusted integer bounds: negative/zero values must be rejected ---
    //
    // TOML integers decode to `i64`. Casting them with `as u64` / `as usize`
    // would wrap a negative value into an enormous unsigned bound and silently
    // disable a resource limit. These tests pin the strict rejection: a
    // negative or zero discovery/provider timeout or profile context window is
    // an invalid configuration, not a wrapped bound.

    /// A negative `agent.discovery.timeout_seconds` must be rejected rather
    /// than wrapped into `u64::MAX` (which `Duration::from_secs` saturates to
    /// `Duration::MAX`, i.e. no timeout at all).
    #[test]
    fn rejects_a_negative_discovery_timeout_instead_of_wrapping() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[agent.discovery]\ntimeout_seconds = -1\n",
        );
        match load(root.path()) {
            Err(KvistError::InvalidProjectConfiguration { reason, .. }) => {
                assert!(
                    reason.to_lowercase().contains("timeout"),
                    "diagnostic must name the offending field, got: {reason}"
                );
            }
            other => panic!("expected InvalidProjectConfiguration, got {other:?}"),
        }
    }

    /// A zero `agent.discovery.timeout_seconds` is not a valid bound and must
    /// be rejected, matching the role-policy and test-policy bounds.
    #[test]
    fn rejects_a_zero_discovery_timeout() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[agent.discovery]\ntimeout_seconds = 0\n",
        );
        assert!(
            matches!(
                load(root.path()),
                Err(KvistError::InvalidProjectConfiguration { .. })
            ),
            "a zero discovery timeout must be rejected"
        );
    }

    /// A valid positive `agent.discovery.timeout_seconds` is accepted and
    /// recorded, so the strict bound does not over-reject.
    #[test]
    fn accepts_a_positive_discovery_timeout() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[agent.discovery]\ntimeout_seconds = 5\n",
        );
        let config = load(root.path()).expect("positive discovery timeout");
        assert_eq!(config.agent.discovery.timeout_seconds, Some(5));
    }

    /// A negative `[agent.providers.<name>] timeout_seconds` must be rejected
    /// rather than wrapped into an enormous provider timeout.
    #[test]
    fn rejects_a_negative_provider_timeout_instead_of_wrapping() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[agent.providers.local-llama]\ntype = \"llama-server\"\ntimeout_seconds = -1\n",
        );
        match load(root.path()) {
            Err(KvistError::InvalidProjectConfiguration { reason, .. }) => {
                assert!(
                    reason.to_lowercase().contains("timeout"),
                    "diagnostic must name the offending field, got: {reason}"
                );
            }
            other => panic!("expected InvalidProjectConfiguration, got {other:?}"),
        }
    }

    /// A negative `[agent.profiles.<name>] context_window` must be rejected
    /// rather than wrapped into a huge token-window bound.
    #[test]
    fn rejects_a_negative_profile_context_window_instead_of_wrapping() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[agent.profiles.qwen-coder]\nprovider = \"custom\"\ncontext_window = -1\n",
        );
        match load(root.path()) {
            Err(KvistError::InvalidProjectConfiguration { reason, .. }) => {
                assert!(
                    reason.to_lowercase().contains("context_window"),
                    "diagnostic must name the offending field, got: {reason}"
                );
            }
            other => panic!("expected InvalidProjectConfiguration, got {other:?}"),
        }
    }

    /// A valid positive `[agent.profiles.<name>] context_window` is accepted
    /// and recorded, so the strict bound does not over-reject.
    #[test]
    fn accepts_a_positive_profile_context_window() {
        let root = tempfile::tempdir().expect("tempdir");
        write_project_config(
            root.path(),
            "schema_version = 1\ncomponent_root = \"src\"\n[agent.profiles.qwen-coder]\nprovider = \"custom\"\ncontext_window = 8192\n",
        );
        let config = load(root.path()).expect("positive context window");
        assert_eq!(
            config
                .agent
                .profiles
                .get("qwen-coder")
                .and_then(|p| p.context_window),
            Some(8192)
        );
    }
}
