//! Strict loading of project-local Kvist configuration.

use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

use crate::{KvistError, Result, artifacts::CONFIGURATION_VERSION, filesystem::is_link_like};

/// Maximum supported size of `kvist.toml`.
pub const MAX_CONFIGURATION_BYTES: u64 = 64 * 1024;

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

/// Configuration for the external agent execution runners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    pub architect: AgentProfile,
    pub developer: AgentProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProfile {
    pub command_template: String,
    pub token_limit: Option<usize>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            architect: AgentProfile {
                command_template: "claude --non-interactive --dangerously-skip-permissions --message '{prompt}' {context_files}".to_owned(),
                token_limit: None,
            },
            developer: AgentProfile {
                command_template: "gemini-cli --prompt '{prompt}' --files {context_files}".to_owned(),
                token_limit: None,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxConfig {
    /// Executable invoked directly, never through a shell.
    pub runner: String,
    /// Environment names inherited by the runner and declared for its child.
    pub environment_allowlist: Vec<String>,
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
        agent: load_agent_config(project_root, table)?,
        test_policy: parse_test_policy(config_path, table)?,
        sandbox: parse_sandbox_config(config_path, table)?,
    })
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
    let mut environment_allowlist = Vec::with_capacity(values.len());
    for value in values {
        let name = value
            .as_str()
            .filter(|name| !name.is_empty() && name.bytes().all(|byte| byte == b'_' || byte.is_ascii_alphanumeric()))
            .ok_or_else(|| invalid_configuration(config_path, "`sandbox.environment_allowlist` must contain only nonblank ASCII environment names"))?;
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
    Ok(Some(SandboxConfig {
        runner: runner.to_owned(),
        environment_allowlist,
    }))
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

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                return Err(invalid_configuration(
                    config_path,
                    "`component_root` may contain only normal path segments",
                ));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
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
    table: &toml::map::Map<String, toml::Value>,
) -> Result<AgentConfig> {
    // Priority 1: Check if `agent` table exists directly in the project-root `kvist.toml`
    if table.contains_key("agent") {
        return parse_agent_config_from_table(Path::new("kvist.toml"), table);
    }

    // Priority 2: Check if project-local `.kvist/config.toml` exists
    let local_path = project_root.join(".kvist").join("config.toml");
    if local_path.is_file() {
        let contents = fs::read_to_string(&local_path).map_err(|source| KvistError::Io {
            operation: "read project-local agent configuration",
            path: local_path.clone(),
            source,
        })?;
        let parsed_table = toml_table_from_str(&local_path, &contents)?;
        return parse_agent_config_from_table(&local_path, &parsed_table);
    }

    // Priority 3: Check global user-specific configuration path
    if let Some(user_path) = global_user_config_path() {
        if user_path.is_file() {
            let contents = fs::read_to_string(&user_path).map_err(|source| KvistError::Io {
                operation: "read global user configuration",
                path: user_path.clone(),
                source,
            })?;
            let parsed_table = toml_table_from_str(&user_path, &contents)?;
            return parse_agent_config_from_table(&user_path, &parsed_table);
        }
    }

    // Priority 4: Check global system-wide configuration path
    if let Some(system_path) = global_system_config_path() {
        if system_path.is_file() {
            let contents = fs::read_to_string(&system_path).map_err(|source| KvistError::Io {
                operation: "read global system configuration",
                path: system_path.clone(),
                source,
            })?;
            let parsed_table = toml_table_from_str(&system_path, &contents)?;
            return parse_agent_config_from_table(&system_path, &parsed_table);
        }
    }

    // Priority 5: Fallback - Attempt to initialize the global config file on disk
    if let Some(user_path) = global_user_config_path() {
        if let Some(parent) = user_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::write(&user_path, DEFAULT_GLOBAL_CONFIG_TEMPLATE).is_ok() {
            let parsed_table = toml_table_from_str(&user_path, DEFAULT_GLOBAL_CONFIG_TEMPLATE)?;
            return parse_agent_config_from_table(&user_path, &parsed_table);
        }
    }

    // Ultima ratio: parse default template from memory if writing file fails
    let parsed_table = toml_table_from_str(Path::new("default"), DEFAULT_GLOBAL_CONFIG_TEMPLATE)?;
    parse_agent_config_from_table(Path::new("default"), &parsed_table)
}

fn parse_agent_config_from_table(
    config_path: &Path,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<AgentConfig> {
    let mut default_config = AgentConfig::default();
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
            default_config.architect.command_template = template
                .as_str()
                .ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        "`agent.profiles.architect.command_template` must be a string",
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
    }

    if let Some(developer) = profiles.get("developer") {
        let developer = developer.as_table().ok_or_else(|| {
            invalid_configuration(
                config_path,
                "`agent.profiles.developer` must be a TOML table",
            )
        })?;
        if let Some(template) = developer.get("command_template") {
            default_config.developer.command_template = template
                .as_str()
                .ok_or_else(|| {
                    invalid_configuration(
                        config_path,
                        "`agent.profiles.developer.command_template` must be a string",
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
    }

    Ok(default_config)
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
    format!("sha256:{:x}", hasher.finalize())
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
