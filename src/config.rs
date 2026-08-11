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

/// Parsed project configuration required by Phase 1 commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectConfig {
    /// Component root relative to the project root.
    pub component_root: PathBuf,
    /// Resource bounds for component discovery.
    pub discovery: DiscoveryLimits,
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
    parse(&config_path, &contents)
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

fn parse(config_path: &Path, contents: &str) -> Result<ProjectConfig> {
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
    })
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
