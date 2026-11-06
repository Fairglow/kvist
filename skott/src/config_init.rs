//! Explicit, offline creation of portable Skott configuration.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{ModelProvider, SCHEMA_VERSION, resolve_working_directory};
use crate::error::{Error, Result, io_error};

#[derive(Serialize)]
struct InitialConfig {
    schema_version: u32,
    default_provider: ModelProvider,
    #[serde(skip_serializing_if = "Option::is_none")]
    registry: Option<InitialRegistry>,
}

#[derive(Serialize)]
struct InitialRegistry {
    path: PathBuf,
    profiles: [String; 1],
    default_model: String,
}

const STARTER: &str = r#"
# No working_directory is pinned: use the launch directory, or --cwd DIR.
# Configure shared profiles with `skott --setup`, then set default_provider
# above to "llama-server" or "ollama" to match your native profiles.
# Alternatively uncomment and edit this explicit local model:
#
# [[models]]
# id = "local"
# provider = "llama-server"
# base_url = "http://127.0.0.1:9931"
# model = "REPLACE_WITH_YOUR_MODEL"
# is_default = true
# context_limit = 32768 # Set to the server's actual capacity if undiscoverable.
#
# To reference an existing shared profile instead, initialize a new file with:
# skott --init-config --model NAME --registry /absolute/path/providers.toml
"#;

/// Creates a private configuration atomically, never replacing a destination.
///
/// The destination parent must exist. `model` selects an enabled native shared
/// profile by reference; without it the result is an unconfigured starter.
/// Registry resolution is filesystem-only and grants no execution authority.
pub fn create(
    path: &Path,
    project: &Path,
    model: Option<&str>,
    registry: Option<&Path>,
) -> Result<()> {
    let project = resolve_working_directory(project)?;
    let mut config = InitialConfig {
        schema_version: SCHEMA_VERSION,
        default_provider: ModelProvider::LlamaServer,
        registry: None,
    };
    if let Some(name) = model {
        let mut paths = sav::registry::RegistryPaths::for_project(&project);
        paths.explicit = registry.map(Path::to_path_buf);
        let profile = paths.load()?.resolve(name)?;
        config.default_provider = profile.native()?.provider;
        let source = profile.source.canonicalize().map_err(|source| {
            io_error(
                "resolve shared registry source",
                Some(&profile.source.to_string_lossy()),
                source,
            )
        })?;
        config.registry = Some(InitialRegistry {
            path: source,
            profiles: [name.to_owned()],
            default_model: name.to_owned(),
        });
    } else if registry.is_some() {
        return Err(Error::Config {
            path: Some(path.display().to_string()),
            reason: "--init-config --registry PATH requires --model NAME to select a native shared profile".into(),
        });
    }
    let mut contents = toml::to_string_pretty(&config).map_err(|_| Error::Config {
        path: Some(path.display().to_string()),
        reason: "could not encode initial configuration".into(),
    })?;
    if model.is_none() {
        contents.push_str(STARTER);
    } else {
        contents.push_str("\n# Uses the launch directory unless --cwd DIR is specified.\n");
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize().map_err(|source| {
        io_error(
            "resolve configuration parent (create the parent directory first)",
            Some(&path.to_string_lossy()),
            source,
        )
    })?;
    let name = path.file_name().ok_or_else(|| Error::Config {
        path: Some(path.display().to_string()),
        reason: "configuration destination must name a file".into(),
    })?;
    let destination = parent.join(name);
    let mut file = tempfile::NamedTempFile::new_in(&parent).map_err(|source| {
        io_error(
            "stage private configuration",
            Some(&path.to_string_lossy()),
            source,
        )
    })?;
    file.write_all(contents.as_bytes())
        .and_then(|()| file.as_file().sync_all())
        .map_err(|source| io_error("write configuration", Some(&path.to_string_lossy()), source))?;
    file.persist_noclobber(&destination).map_err(|error| {
        io_error(
            "create configuration without overwriting (choose another --config PATH if it exists)",
            Some(&path.to_string_lossy()),
            error.error,
        )
    })?;
    std::fs::File::open(&parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| {
            io_error(
                "synchronize configuration directory (file was created; inspect before retrying)",
                Some(&path.to_string_lossy()),
                source,
            )
        })?;
    Ok(())
}
