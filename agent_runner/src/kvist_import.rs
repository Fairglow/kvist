//! Importing agent profiles declared in a Kvist `kvist.toml`.
//!
//! Kvist declares reusable agent profiles under `[agent.profiles]`. This module
//! derives ready-to-paste `[[models]]` entries for the agent-runner
//! configuration from those profiles, so a Kvist project's agents can be
//! reused without retyping them. The mapping is conservative: the provider and
//! the provider-facing model name are taken from the profile, the base URL
//! defaults to the provider's standard endpoint (an explicit `base_url` in the
//! profile wins), and the per-turn deadline defaults to 120 seconds.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;

use crate::config::{MAX_CONFIG_BYTES, ModelProvider};
use crate::error::{Error, Result, io_error};

/// The Kvist project configuration file name searched in the current directory.
pub const KVIST_CONFIG_FILE: &str = "kvist.toml";

/// Resolves the Kvist project configuration path: an explicit path wins,
/// otherwise `kvist.toml` in the current directory. Subdirectories are never
/// searched.
pub fn resolve_kvist_config_path(
    explicit: Option<PathBuf>,
) -> std::result::Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let cwd = std::env::current_dir()
        .map_err(|source| io_error("resolve current working directory", None, source))
        .map_err(|error| error.describe())?;
    let local = cwd.join(KVIST_CONFIG_FILE);
    if local.is_file() {
        return Ok(local);
    }
    Err(format!(
        "no {KVIST_CONFIG_FILE} found in the current directory; pass --kvist-config PATH"
    ))
}

/// The minimal Kvist configuration shape needed for profile import.
#[derive(Debug, Default, Deserialize)]
struct KvistConfig {
    #[serde(default)]
    agent: Option<KvistAgent>,
}

#[derive(Debug, Default, Deserialize)]
struct KvistAgent {
    #[serde(default)]
    profiles: BTreeMap<String, KvistProfile>,
}

/// One `[agent.profiles.<id>]` entry. `command` is Kvist's own rendering and is
/// ignored; unknown fields are tolerated so newer Kvist schemas stay importable.
#[derive(Debug, Default, Deserialize)]
struct KvistProfile {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    deadline_secs: Option<u64>,
}

/// Renders `[[models]]` entries, in stable key order, for every agent profile
/// declared in the Kvist configuration at `path`.
pub fn import_models(path: &Path) -> Result<String> {
    let contents = read_untrusted_config(path)?;
    let raw: KvistConfig = toml::from_str(&contents).map_err(|error| Error::Config {
        path: Some(path.to_string_lossy().into_owned()),
        reason: format!("invalid {KVIST_CONFIG_FILE}: {error}"),
    })?;
    let profiles = raw.agent.map(|agent| agent.profiles).unwrap_or_default();
    if profiles.is_empty() {
        return Err(Error::Config {
            path: Some(path.to_string_lossy().into_owned()),
            reason: format!("{KVIST_CONFIG_FILE} declares no [agent.profiles] entries to import"),
        });
    }

    let mut out = String::from(
        "# models imported from kvist.toml [agent.profiles]\n\
         # paste these entries into your agent-runner configuration\n",
    );
    for (id, profile) in profiles {
        let provider_name = profile.provider.as_deref().ok_or_else(|| Error::Config {
            path: Some(path.to_string_lossy().into_owned()),
            reason: format!("[agent.profiles.{id}] is missing `provider`"),
        })?;
        let provider = ModelProvider::from_str(provider_name).map_err(|_| Error::Config {
            path: Some(path.to_string_lossy().into_owned()),
            reason: format!(
                "[agent.profiles.{id}].provider `{provider_name}` is not supported for import; \
                 expected llama-server or ollama"
            ),
        })?;
        let model = profile.model.as_deref().ok_or_else(|| Error::Config {
            path: Some(path.to_string_lossy().into_owned()),
            reason: format!("[agent.profiles.{id}] is missing `model`"),
        })?;
        if model.trim().is_empty() {
            return Err(Error::Config {
                path: Some(path.to_string_lossy().into_owned()),
                reason: format!("[agent.profiles.{id}].model must not be empty"),
            });
        }
        let base_url = profile
            .base_url
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| provider.default_endpoint().to_owned());
        let deadline_secs = profile
            .deadline_secs
            .filter(|value| *value >= 1)
            .unwrap_or(120);
        out.push_str(&format!(
            "\n[[models]]\nid = {}\nprovider = {}\nbase_url = {}\nmodel = {}\ndeadline_secs = {deadline_secs}\n",
            toml_string(&id),
            toml_string(provider_name),
            toml_string(&base_url),
            toml_string(model),
        ));
    }
    Ok(out)
}

/// Reads a local configuration file, rejecting symlinks, non-files, oversized
/// input, and non-UTF-8 content.
fn read_untrusted_config(path: &Path) -> Result<String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|source| {
        io_error(
            "inspect kvist configuration",
            Some(&path.to_string_lossy()),
            source,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(Error::Config {
            path: Some(path.to_string_lossy().into_owned()),
            reason: "kvist configuration must be a regular non-link file".to_owned(),
        });
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(Error::Config {
            path: Some(path.to_string_lossy().into_owned()),
            reason: format!("kvist configuration exceeds the {MAX_CONFIG_BYTES}-byte limit"),
        });
    }
    std::fs::read_to_string(path).map_err(|source| {
        io_error(
            "read kvist configuration",
            Some(&path.to_string_lossy()),
            source,
        )
    })
}

/// Escapes a value as a TOML basic string.
fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_kvist(dir: &Path, contents: &str) -> PathBuf {
        let path = dir.join(KVIST_CONFIG_FILE);
        std::fs::write(&path, contents).expect("write kvist config");
        path
    }

    const SAMPLE: &str = r#"
schema_version = 1

[agent]

[agent.profiles.b-model]
provider = "ollama"
model = "ollama-model"
command = "curl ..."

[agent.profiles.a-model]
provider = "llama-server"
model = "llama-model"
base_url = "http://127.0.0.1:9931"
command = "curl ..."
"#;

    #[test]
    fn imports_profiles_in_stable_order_with_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_kvist(dir.path(), SAMPLE);
        let out = import_models(&path).expect("import succeeds");
        // Stable (sorted) order: a-model before b-model.
        assert!(
            out.find("a-model").expect("a-model present")
                < out.find("b-model").expect("b-model present"),
            "profiles are emitted in stable order:\n{out}"
        );
        assert!(out.contains("provider = \"llama-server\""));
        assert!(out.contains("provider = \"ollama\""));
        assert!(out.contains("model = \"llama-model\""));
        assert!(out.contains("model = \"ollama-model\""));
        // Ollama profile gets the provider default endpoint; llama keeps the
        // explicit base_url.
        let b_start = out
            .find("[[models]]\nid = \"b-model\"")
            .expect("b-model entry present");
        assert!(out[b_start..].contains("base_url = \"http://127.0.0.1:11434\""));
        let a_start = out
            .find("[[models]]\nid = \"a-model\"")
            .expect("a-model entry present");
        assert!(out[a_start..].contains("base_url = \"http://127.0.0.1:9931\""));
        assert!(out.contains("deadline_secs = 120"));
    }

    #[test]
    fn import_rejects_unknown_provider() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_kvist(
            dir.path(),
            "schema_version = 1\n[agent.profiles.x]\nprovider = \"openai\"\nmodel = \"m\"\n",
        );
        let err = import_models(&path).expect_err("unknown provider must fail");
        assert!(
            err.describe().contains("not supported for import"),
            "actionable message: {}",
            err.describe()
        );
    }

    #[test]
    fn import_fails_without_profiles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_kvist(dir.path(), "schema_version = 1\n[agent]\n");
        let err = import_models(&path).expect_err("no profiles must fail");
        assert!(err.describe().contains("no [agent.profiles]"));
    }

    #[test]
    fn import_rejects_missing_provider_or_model() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_kvist(dir.path(), "[agent.profiles.x]\nmodel = \"m\"\n");
        assert!(
            import_models(&path)
                .expect_err("missing provider must fail")
                .describe()
                .contains("missing `provider`")
        );

        let path = write_kvist(dir.path(), "[agent.profiles.x]\nprovider = \"ollama\"\n");
        assert!(
            import_models(&path)
                .expect_err("missing model must fail")
                .describe()
                .contains("missing `model`")
        );
    }

    #[test]
    fn import_escapes_toml_strings() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A quoted TOML key may contain an escaped quote character.
        let path = write_kvist(
            dir.path(),
            "schema_version = 1\n[agent.profiles.\"we\\\"ird\"]\nprovider = \"ollama\"\nmodel = \"m\"\n",
        );
        let out = import_models(&path).expect("import succeeds");
        assert!(
            out.contains(r#"id = "we\"ird""#),
            "quotes are escaped:\n{out}"
        );
    }
}
