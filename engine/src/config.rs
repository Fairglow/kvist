//! Strict loading of project-local Kvist configuration.

use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

use serde::Serialize;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Architect => "architect",
            Role::Developer => "developer",
            Role::SecurityReviewer => "security-reviewer",
        }
    }
}

/// A model configuration for a role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Model {
    /// A model identifier string (e.g., "llama-cli", "ollama", "mcp", "none").
    pub name: String,
    /// The command template for this model, with `{prompt}`, `{context_files}`, etc.
    pub command: String,
    /// A system prompt injected at the start of the message.
    pub system_prompt: Option<String>,
}

/// Configuration for the external agent execution runners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentConfig {
    pub architect: AgentProfile,
    pub developer: AgentProfile,
    pub security_reviewer: AgentProfile,
    /// Identity and content digest of the resolver input that selected this config.
    pub source: AgentConfigSource,
}

/// The resolved agent configuration input, retained for execution approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentConfigResolved {
    pub architect: AgentProfile,
    pub developer: AgentProfile,
    pub security_reviewer: AgentProfile,
    /// Identity and content digest of the resolver input that selected this config.
    pub source: AgentConfigSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentProfile {
    /// Legacy command template retained for backwards-compatible configuration.
    pub command_template: String,
    /// Maps model identifiers (e.g. "llama-cli", "ollama", "mcp", "none")
    /// to their command templates. Use "default" or "default-model" for the
    /// fallback template.
    pub models: Vec<Model>,
    /// The default model name to use when no explicit model is selected.
    /// Defaults to "none" (no model, uses `command` directly as the template).
    pub default_model: String,
    /// Which model name to use for this role. Set to `None` to use `default_model`.
    /// Use "default" or "default-model" to select the first model in the `models` list.
    /// Use "none" to use the raw `command` field without interpolation.
    pub model: Option<String>,
    /// Maximum number of tokens per request. `None` means unlimited.
    pub token_limit: Option<usize>,
    /// Maximum duration for one sandbox runner request.
    pub timeout_seconds: u64,
    /// Combined stdout/stderr capture budget for one request.
    pub max_output_bytes: usize,
    /// Literal values replaced before agent evidence reaches any sink.
    pub redaction_values: Vec<String>,
}

pub const DEFAULT_AGENT_TIMEOUT_SECONDS: u64 = 300;
pub const MAX_AGENT_TIMEOUT_SECONDS: u64 = 3_600;
pub const DEFAULT_AGENT_MAX_OUTPUT_BYTES: usize = 65_536;
pub const MAX_AGENT_OUTPUT_BYTES: usize = 1_048_576;

/// The resolved agent configuration input, retained for execution approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentConfigSource {
    pub identity: String,
    pub digest: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            architect: AgentProfile {
                command_template: "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}".to_owned(),
                models: vec![
                    Model {
                        name: "default".to_owned(),
                        command: "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "llama-cli".to_owned(),
                        command: "llama-cli --prompt '{prompt}' --context '{context_files}' --format json".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "ollama".to_owned(),
                        command: "ollama run --stream=false '{model}' --prompt '{prompt}' --context '{context_files}'".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "none".to_owned(),
                        command: "{prompt} {context_files}".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "mcp".to_owned(),
                        command: "mcp run --prompt '{prompt}' --context '{context_files}'".to_owned(),
                        system_prompt: None,
                    },
                ],
                default_model: "default".to_owned(),
                model: None,
                token_limit: None,
                timeout_seconds: DEFAULT_AGENT_TIMEOUT_SECONDS,
                max_output_bytes: DEFAULT_AGENT_MAX_OUTPUT_BYTES,
                redaction_values: Vec::new(),
            },
            developer: AgentProfile {
                command_template: "gemini-cli --prompt '{prompt}' --files {context_files}".to_owned(),
                models: vec![
                    Model {
                        name: "default".to_owned(),
                        command: "gemini-cli --prompt '{prompt}' --files {context_files}".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "llama-cli".to_owned(),
                        command: "llama-cli --prompt '{prompt}' --context '{context_files}' --format json".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "ollama".to_owned(),
                        command: "ollama run --stream=false '{model}' --prompt '{prompt}' --context '{context_files}'".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "none".to_owned(),
                        command: "{prompt} {context_files}".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "mcp".to_owned(),
                        command: "mcp run --prompt '{prompt}' --context '{context_files}'".to_owned(),
                        system_prompt: None,
                    },
                ],
                default_model: "default".to_owned(),
                model: None,
                token_limit: None,
                timeout_seconds: DEFAULT_AGENT_TIMEOUT_SECONDS,
                max_output_bytes: DEFAULT_AGENT_MAX_OUTPUT_BYTES,
                redaction_values: Vec::new(),
            },
            security_reviewer: AgentProfile {
                command_template: "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}".to_owned(),
                models: vec![
                    Model {
                        name: "default".to_owned(),
                        command: "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "llama-cli".to_owned(),
                        command: "llama-cli --prompt '{prompt}' --context '{context_files}' --format json".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "ollama".to_owned(),
                        command: "ollama run --stream=false '{model}' --prompt '{prompt}' --context '{context_files}'".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "none".to_owned(),
                        command: "{prompt} {context_files}".to_owned(),
                        system_prompt: None,
                    },
                    Model {
                        name: "mcp".to_owned(),
                        command: "mcp run --prompt '{prompt}' --context '{context_files}'".to_owned(),
                        system_prompt: None,
                    },
                ],
                default_model: "default".to_owned(),
                model: None,
                token_limit: None,
                timeout_seconds: DEFAULT_AGENT_TIMEOUT_SECONDS,
                max_output_bytes: DEFAULT_AGENT_MAX_OUTPUT_BYTES,
                redaction_values: Vec::new(),
            },
            source: AgentConfigSource {
                identity: "built-in:agent-default-v1".to_owned(),
                digest: sha256(DEFAULT_GLOBAL_CONFIG_TEMPLATE.as_bytes()),
            },
        }
    }
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

    Ok(ProjectConfig {
        component_root: normalize_component_root(config_path, component_root)?,
        discovery: parse_discovery_limits(config_path, table)?,
        vcs: parse_vcs_selection(config_path, table)?,
        agent: load_agent_config(project_root, config_path, contents, table)?,
        test_policy: parse_test_policy(config_path, table)?,
        sandbox: parse_sandbox_config(config_path, table)?,
    })
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
pub fn global_user_config_path() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("kvist").join("config.toml"))
    } else {
        // Unix / macOS conforming to XDG standards
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join(".config"))
            });
        base.map(|path| path.join("kvist").join("config.toml"))
    }
}

/// Returns the standard system-wide global configuration path for Kvist.
pub fn global_system_config_path() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .map(|path| path.join("kvist").join("config.toml"))
    } else {
        Some(PathBuf::from("/etc/kvist/config.toml"))
    }
}

const DEFAULT_GLOBAL_CONFIG_TEMPLATE: &str = r#"# Kvist Global User Configuration

[agent.profiles.architect]
command_template = "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}"

[agent.profiles.developer]
command_template = "gemini-cli --prompt '{prompt}' --files {context_files}"
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
    if table.contains_key("agent") {
        return parse_agent_config_from_table(
            project_config_path,
            project_contents,
            project_config_path,
            table,
        );
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
    let mut default_config = AgentConfig {
        source: AgentConfigSource {
            identity: agent_source_identity(source_path)?,
            digest: sha256(contents.as_bytes()),
        },
        ..AgentConfig::default()
    };
    let Some(agent) = table.get("agent") else {
        return Ok(default_config);
    };
    let agent = agent
        .as_table()
        .ok_or_else(|| invalid_configuration(config_path, "`agent` must be a TOML table"))?;

    let Some(profiles) = agent.get("profiles") else {
        return Ok(default_config);
    };
    let profiles = profiles.as_table().ok_or_else(|| {
        invalid_configuration(config_path, "`agent.profiles` must be a TOML table")
    })?;

    if let Some(architect) = profiles.get("architect") {
        let architect = architect.as_table().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`agent.profiles.architect` must be a TOML table",
            )
        })?;
        if let Some(template) = architect.get("command_template") {
            let template = template.as_str().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent.profiles.architect.command_template` must be a string",
                )
            })?;
            default_config.architect.command_template = template.to_owned();
            if let Some(model) = default_config
                .architect
                .models
                .iter_mut()
                .find(|model| model.name == "default")
            {
                model.command = template.to_owned();
            }
        }
        if let Some(model) = architect.get("model") {
            default_config.architect.model = Some(
                model
                    .as_str()
                    .ok_or_else(|| {
                        invalid_configuration(
                            config_path,
                            "`agent.profiles.architect.model` must be a string",
                        )
                    })?
                    .to_owned(),
            );
        }
        if let Some(models) = architect.get("models") {
            default_config.architect.models = parse_agent_models(config_path, models)?;
        }
        if let Some(default_model) = architect.get("default_model") {
            default_config.architect.default_model = default_model
                .as_str()
                .ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        "`agent.profiles.architect.default_model` must be a string",
                    )
                })?
                .to_owned();
        }
        if let Some(limit) = architect.get("token_limit") {
            let limit = limit.as_integer().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent.profiles.architect.token_limit` must be a positive integer",
                )
            })?;
            let limit = usize::try_from(limit)
                .ok()
                .filter(|val| *val > 0)
                .ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        "`agent.profiles.architect.token_limit` must be a positive integer",
                    )
                })?;
            default_config.architect.token_limit = Some(limit);
        }
        parse_agent_resource_policy(config_path, architect, &mut default_config.architect)?;
    }

    if let Some(developer) = profiles.get("developer") {
        let developer = developer.as_table().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`agent.profiles.developer` must be a TOML table",
            )
        })?;
        if let Some(template) = developer.get("command_template") {
            let template = template.as_str().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent.profiles.developer.command_template` must be a string",
                )
            })?;
            default_config.developer.command_template = template.to_owned();
            if let Some(model) = default_config
                .developer
                .models
                .iter_mut()
                .find(|model| model.name == "default")
            {
                model.command = template.to_owned();
            }
        }
        if let Some(model) = developer.get("model") {
            default_config.developer.model = Some(
                model
                    .as_str()
                    .ok_or_else(|| {
                        invalid_configuration(
                            config_path,
                            "`agent.profiles.developer.model` must be a string",
                        )
                    })?
                    .to_owned(),
            );
        }
        if let Some(models) = developer.get("models") {
            default_config.developer.models = parse_agent_models(config_path, models)?;
        }
        if let Some(default_model) = developer.get("default_model") {
            default_config.developer.default_model = default_model
                .as_str()
                .ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        "`agent.profiles.developer.default_model` must be a string",
                    )
                })?
                .to_owned();
        }
        if let Some(limit) = developer.get("token_limit") {
            let limit = limit.as_integer().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent.profiles.developer.token_limit` must be a positive integer",
                )
            })?;
            let limit = usize::try_from(limit)
                .ok()
                .filter(|val| *val > 0)
                .ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        "`agent.profiles.developer.token_limit` must be a positive integer",
                    )
                })?;
            default_config.developer.token_limit = Some(limit);
        }
        parse_agent_resource_policy(config_path, developer, &mut default_config.developer)?;
    }

    if let Some(security_reviewer) = profiles
        .get("security-reviewer")
        .or_else(|| profiles.get("security_reviewer"))
    {
        let security_reviewer = security_reviewer.as_table().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`agent.profiles.security-reviewer` must be a TOML table",
            )
        })?;
        if let Some(template) = security_reviewer.get("command_template") {
            let template = template.as_str().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent.profiles.security-reviewer.command_template` must be a string",
                )
            })?;
            default_config.security_reviewer.command_template = template.to_owned();
            if let Some(model) = default_config
                .security_reviewer
                .models
                .iter_mut()
                .find(|model| model.name == "default")
            {
                model.command = template.to_owned();
            }
        }
        if let Some(model) = security_reviewer.get("model") {
            default_config.security_reviewer.model = Some(
                model
                    .as_str()
                    .ok_or_else(|| {
                        invalid_configuration(
                            config_path,
                            "`agent.profiles.security-reviewer.model` must be a string",
                        )
                    })?
                    .to_owned(),
            );
        }
        if let Some(models) = security_reviewer.get("models") {
            default_config.security_reviewer.models = parse_agent_models(config_path, models)?;
        }
        if let Some(default_model) = security_reviewer.get("default_model") {
            default_config.security_reviewer.default_model = default_model
                .as_str()
                .ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        "`agent.profiles.security-reviewer.default_model` must be a string",
                    )
                })?
                .to_owned();
        }
        if let Some(limit) = security_reviewer.get("token_limit") {
            let limit = limit.as_integer().ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent.profiles.security-reviewer.token_limit` must be a positive integer",
                )
            })?;
            let limit = usize::try_from(limit)
                .ok()
                .filter(|val| *val > 0)
                .ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        "`agent.profiles.security-reviewer.token_limit` must be a positive integer",
                    )
                })?;
            default_config.security_reviewer.token_limit = Some(limit);
        }
        parse_agent_resource_policy(
            config_path,
            security_reviewer,
            &mut default_config.security_reviewer,
        )?;
    }

    fn parse_agent_resource_policy(
        config_path: &Path,
        table: &toml::map::Map<String, toml::Value>,
        profile: &mut AgentProfile,
    ) -> Result<()> {
        profile.timeout_seconds = parse_agent_limit(
            config_path,
            table,
            "timeout_seconds",
            profile.timeout_seconds as usize,
            MAX_AGENT_TIMEOUT_SECONDS as usize,
        )? as u64;
        profile.max_output_bytes = parse_agent_limit(
            config_path,
            table,
            "max_output_bytes",
            profile.max_output_bytes,
            MAX_AGENT_OUTPUT_BYTES,
        )?;
        let Some(redaction) = table.get("redaction") else {
            return Ok(());
        };
        let redaction = redaction.as_table().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`agent profile redaction` must be a TOML table",
            )
        })?;
        let values = redaction
            .get("values")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent profile redaction.values` must be an array of nonblank strings",
                )
            })?;
        let mut parsed = Vec::with_capacity(values.len());
        for value in values {
            let value = value.as_str().filter(|value| {
                !value.is_empty() && value.len() <= 4_096
            }).ok_or_else(|| {
                invalid_configuration(
                    config_path,
                    "`agent profile redaction.values` must contain nonblank strings no longer than 4096 bytes",
                )
            })?;
            if parsed.iter().any(|existing| existing == value) {
                return Err(invalid_configuration(
                    config_path,
                    "`agent profile redaction.values` must not contain duplicates",
                ));
            }

            parsed.push(value.to_owned());
        }
        if parsed.is_empty() {
            return Err(invalid_configuration(
                config_path,
                "`agent profile redaction.values` must not be empty",
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
                &format!(
                    "`agent profile {name}` must be a positive integer no greater than {maximum}"
                ),
            ));
        };
        Ok(value)
    }

    Ok(default_config)
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
pub fn compute_policy_hash(policy: &TestPolicy) -> String {
    let serialized = serde_json::to_string(policy).unwrap_or_default();
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
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
    let mut environment_allowlist = Vec::new();
    for val in env_allowlist_val {
        let s = val.as_str().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`test_policy.environment_allowlist` must be an array of strings",
            )
        })?;
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
