use std::{
    collections::BTreeSet,
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, Value, value};

use crate::{Error, Result, split_raw_command};

/// Maximum encoded size of the standalone profile configuration.
pub const MAX_PROFILE_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_PROFILES: usize = 128;
const MAX_PROFILE_NAME_BYTES: usize = 128;
const MAX_PROFILE_COMMAND_BYTES: usize = 16 * 1024;

/// A reusable provider command independent of application-specific roles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelProfile {
    /// Case-sensitive identifier selected by callers.
    pub name: String,
    /// Provider family used to create the command.
    pub provider: String,
    /// Shell-free command template rendered for each attempt.
    pub command: String,
}

/// Returns the Linux user profile configuration path.
pub fn default_profile_config_path() -> Option<PathBuf> {
    let base = absolute_environment_path("XDG_CONFIG_HOME")
        .or_else(|| absolute_environment_path("HOME").map(|home| home.join(".config")))?;
    Some(base.join("supervised-agent").join("config.toml"))
}

fn absolute_environment_path(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(env::var_os(name)?);
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return None;
    }
    Some(path)
}

/// Loads and validates every profile from one configuration file.
pub fn load_profiles(path: &Path) -> Result<Vec<ModelProfile>> {
    let contents = read_configuration(path)?;
    let document = parse_document(path, &contents)?;
    profiles_from_document(path, &document)
}

/// Loads one case-sensitive named profile.
pub fn load_profile(path: &Path, name: &str) -> Result<ModelProfile> {
    load_profiles(path)?
        .into_iter()
        .find(|profile| profile.name == name)
        .ok_or_else(|| Error::ProfileNotFound {
            name: name.to_owned(),
            path: path.to_path_buf(),
        })
}

/// Creates or formatting-preservingly replaces one named profile.
pub fn upsert_profile(path: &Path, profile: &ModelProfile) -> Result<()> {
    validate_profile(profile)?;
    let (mut document, existed) = match fs::symlink_metadata(path) {
        Ok(_) => {
            let contents = read_configuration(path)?;
            let document = parse_document(path, &contents)?;
            profiles_from_document(path, &document)?;
            (document, true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let document = "schema_version = 1\n"
                .parse::<DocumentMut>()
                .map_err(|error| invalid_configuration(path, error.to_string()))?;
            (document, false)
        }
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect profile configuration",
                path: path.to_path_buf(),
                source,
            });
        }
    };

    if document.get("profiles").is_none() {
        document["profiles"] = Item::ArrayOfTables(ArrayOfTables::new());
    }
    let profiles = document["profiles"]
        .as_array_of_tables_mut()
        .ok_or_else(|| invalid_configuration(path, "`profiles` must be an array of tables"))?;
    if let Some(existing) = profiles
        .iter_mut()
        .find(|table| table.get("name").and_then(Item::as_str) == Some(profile.name.as_str()))
    {
        existing["provider"] = value(&profile.provider);
        existing["command"] = value(&profile.command);
    } else {
        if profiles.len() >= MAX_PROFILES {
            return Err(invalid_configuration(
                path,
                format!("configuration may contain at most {MAX_PROFILES} profiles"),
            ));
        }
        let mut table = Table::new();
        table["name"] = value(&profile.name);
        table["provider"] = value(&profile.provider);
        table["command"] = value(&profile.command);
        profiles.push(table);
    }

    let contents = document.to_string();
    if contents.len() as u64 > MAX_PROFILE_CONFIG_BYTES {
        return Err(invalid_configuration(
            path,
            format!("configuration exceeds the {MAX_PROFILE_CONFIG_BYTES}-byte limit"),
        ));
    }
    let verified = parse_document(path, &contents)?;
    profiles_from_document(path, &verified)?;
    persist_configuration(path, contents.as_bytes(), existed)
}

fn parse_document(path: &Path, contents: &str) -> Result<DocumentMut> {
    contents
        .parse::<DocumentMut>()
        .map_err(|error| invalid_configuration(path, error.to_string()))
}

fn profiles_from_document(path: &Path, document: &DocumentMut) -> Result<Vec<ModelProfile>> {
    if document.get("schema_version").and_then(Item::as_integer) != Some(1) {
        return Err(invalid_configuration(
            path,
            "`schema_version` must be the integer 1",
        ));
    }
    let Some(item) = document.get("profiles") else {
        return Ok(Vec::new());
    };
    let profiles = item
        .as_array_of_tables()
        .ok_or_else(|| invalid_configuration(path, "`profiles` must be an array of tables"))?;
    if profiles.len() > MAX_PROFILES {
        return Err(invalid_configuration(
            path,
            format!("configuration may contain at most {MAX_PROFILES} profiles"),
        ));
    }

    let mut names = BTreeSet::new();
    let mut parsed = Vec::with_capacity(profiles.len());
    for (index, table) in profiles.iter().enumerate() {
        let profile = ModelProfile {
            name: required_string(path, table, index, "name")?.to_owned(),
            provider: required_string(path, table, index, "provider")?.to_owned(),
            command: required_string(path, table, index, "command")?.to_owned(),
        };
        validate_profile(&profile).map_err(|error| {
            invalid_configuration(path, format!("invalid `profiles[{index}]`: {error}"))
        })?;
        if !names.insert(profile.name.clone()) {
            return Err(invalid_configuration(
                path,
                format!("duplicate profile name `{}`", profile.name),
            ));
        }
        parsed.push(profile);
    }
    Ok(parsed)
}

fn required_string<'a>(
    path: &Path,
    table: &'a Table,
    index: usize,
    field: &str,
) -> Result<&'a str> {
    table
        .get(field)
        .and_then(Item::as_value)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid_configuration(
                path,
                format!("`profiles[{index}].{field}` must be a string"),
            )
        })
}

pub(crate) fn validate_profile(profile: &ModelProfile) -> Result<()> {
    if profile.name.is_empty()
        || profile.name.len() > MAX_PROFILE_NAME_BYTES
        || !profile
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
    {
        return Err(Error::ProfileSetup {
            reason: format!(
                "profile name must use 1-{MAX_PROFILE_NAME_BYTES} ASCII letters, digits, '.', '_', '-', or ':'"
            ),
        });
    }
    if profile.provider.is_empty()
        || profile.provider.len() > MAX_PROFILE_NAME_BYTES
        || !profile
            .provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(Error::ProfileSetup {
            reason: "provider must be a nonblank ASCII identifier".to_owned(),
        });
    }
    if profile.command.trim().is_empty() || profile.command.len() > MAX_PROFILE_COMMAND_BYTES {
        return Err(Error::ProfileSetup {
            reason: format!("profile command must contain 1-{MAX_PROFILE_COMMAND_BYTES} bytes"),
        });
    }
    split_raw_command(&profile.command)?;
    Ok(())
}

fn read_configuration(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect profile configuration",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(invalid_configuration(
            path,
            "configuration must be a regular non-link file",
        ));
    }
    if metadata.len() > MAX_PROFILE_CONFIG_BYTES {
        return Err(invalid_configuration(
            path,
            format!("configuration exceeds the {MAX_PROFILE_CONFIG_BYTES}-byte limit"),
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|source| Error::Io {
            operation: "open profile configuration without following links",
            path: path.to_path_buf(),
            source,
        })?;
    let opened = file.metadata().map_err(|source| Error::Io {
        operation: "inspect opened profile configuration",
        path: path.to_path_buf(),
        source,
    })?;
    if !opened.file_type().is_file() || opened.len() > MAX_PROFILE_CONFIG_BYTES {
        return Err(invalid_configuration(
            path,
            "opened configuration must remain a bounded regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PROFILE_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| Error::Io {
            operation: "read profile configuration",
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() as u64 > MAX_PROFILE_CONFIG_BYTES {
        return Err(invalid_configuration(
            path,
            format!("configuration exceeds the {MAX_PROFILE_CONFIG_BYTES}-byte limit"),
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| invalid_configuration(path, "configuration must be valid UTF-8"))
}

fn persist_configuration(path: &Path, contents: &[u8], existed: bool) -> Result<()> {
    let parent = match path.parent() {
        Some(parent) if parent.as_os_str().is_empty() => Path::new("."),
        Some(parent) => parent,
        None => {
            return Err(Error::ProfileSetup {
                reason: format!("configuration path `{}` has no parent", path.display()),
            });
        }
    };
    fs::create_dir_all(parent).map_err(|source| Error::Io {
        operation: "create profile configuration directory",
        path: parent.to_path_buf(),
        source,
    })?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|source| Error::Io {
        operation: "inspect profile configuration directory",
        path: parent.to_path_buf(),
        source,
    })?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.file_type().is_dir() {
        return Err(Error::ProfileSetup {
            reason: format!(
                "configuration parent `{}` must be a real directory",
                parent.display()
            ),
        });
    }

    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|source| Error::Io {
        operation: "create temporary profile configuration",
        path: parent.to_path_buf(),
        source,
    })?;
    temporary
        .write_all(contents)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|source| Error::Io {
            operation: "write temporary profile configuration",
            path: temporary.path().to_path_buf(),
            source,
        })?;
    if existed {
        temporary.persist(path).map_err(|error| Error::Io {
            operation: "replace profile configuration",
            path: path.to_path_buf(),
            source: error.error,
        })?;
    } else {
        temporary
            .persist_noclobber(path)
            .map_err(|error| Error::Io {
                operation: "create profile configuration",
                path: path.to_path_buf(),
                source: error.error,
            })?;
    }
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| Error::Io {
            operation: "synchronize profile configuration directory",
            path: parent.to_path_buf(),
            source,
        })
}

fn invalid_configuration(path: &Path, reason: impl Into<String>) -> Error {
    Error::InvalidProfileConfiguration {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}
