use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use serde::Serialize;
use toml_edit::{DocumentMut, Item, value};

use super::{
    FallbackDefinition, absolute, failure, inspect_with_registry, read_optional, revision,
    safe_path,
};
use crate::{KvistError, Result, file_io::replace_file_atomically};

/// A source-specific registry edit, never a role/policy edit.
#[derive(Debug, Clone, Copy)]
pub enum RegistryChange {
    Enable,
    Disable,
    Remove,
}

/// Mutation disclosure suitable for JSON and ordinary output.
#[derive(Debug, Serialize)]
pub struct MutationReport {
    pub message: String,
    pub warnings: Vec<String>,
    pub fallback: Option<FallbackDefinition>,
    pub source: PathBuf,
    pub previous_revision: String,
    pub revision: String,
}

/// Edit one explicitly acknowledged registry source at an inspected revision.
///
/// A bounded sidecar lock serializes Kvist settings writers across
/// atomic renames. Source bytes and role-reference inputs are rechecked directly
/// before replacement; unrelated, noncooperating writers cause a stale refusal
/// when observed, not an automatic retry or a guessed merge.
pub fn mutate_registry(
    project: &Path,
    registry: &Path,
    name: &str,
    expected_revision: &str,
    change: RegistryChange,
) -> Result<MutationReport> {
    sav::registry::validate_profile_name(name)?;
    let path = absolute(registry)?;
    safe_path(&path)?;
    let _lock = SourceLock::acquire(&path)?;
    let before =
        read_optional(&path)?.ok_or_else(|| failure("selected registry source does not exist"))?;
    if revision(&before) != expected_revision {
        return Err(failure(
            "stale registry selection: source revision changed; inspect again before editing",
        ));
    }
    let registry = sav::registry::Registry::parse(&path, &before)?;
    if !registry.profiles().any(|profile| profile.name == name) {
        return Err(failure(format!(
            "profile `{name}` is not defined in selected source `{}`",
            path.display()
        )));
    }
    // Keep cooperating role assignment writers from adding a reference between
    // the reference check and the registry replacement.
    let _role_locks = lock_role_sources(project, &path)?;
    let snapshot = inspect_with_registry(project, Some(&path))?;
    let selected = snapshot
        .profiles
        .iter()
        .find(|p| p.name == name && p.source.path == path)
        .ok_or_else(|| failure("selected definition is unavailable; inspect again"))?;
    if !matches!(change, RegistryChange::Enable) && !selected.references.is_empty() {
        let references = selected
            .references
            .iter()
            .map(|r| format!("{} in `{}`", r.role.as_str(), r.source.path.display()))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(failure(format!(
            "profile `{name}` is referenced by {references}; reassign or unassign these roles first with `kvist agent role set` or `kvist agent role clear` (use --global for user roles). No references were cleared."
        )));
    }
    let mut document = before
        .parse::<DocumentMut>()
        .map_err(|_| failure("invalid registry TOML"))?;
    match change {
        RegistryChange::Remove => {
            document
                .get_mut("profiles")
                .and_then(Item::as_table_like_mut)
                .ok_or_else(|| failure("registry profiles must be a table"))?
                .remove(name);
        }
        RegistryChange::Enable | RegistryChange::Disable => {
            let profile = document
                .get_mut("profiles")
                .and_then(|p| p.get_mut(name))
                .and_then(Item::as_table_like_mut)
                .ok_or_else(|| failure("selected registry profile must be a table"))?;
            profile.insert("enabled", value(matches!(change, RegistryChange::Enable)));
        }
    }
    let after = document.to_string();
    sav::registry::Registry::parse(&path, &after)?;
    // Re-inspection detects a newly assigned role or changed fallback/config.
    let fresh = inspect_with_registry(project, Some(&path))?;
    if fresh != snapshot || read_optional(&path)?.as_deref() != Some(before.as_str()) {
        return Err(failure(
            "settings changed during mutation; inspect again before editing",
        ));
    }
    replace_file_atomically(&path, &after)?;
    let verb = match change {
        RegistryChange::Enable => "Enabled",
        RegistryChange::Disable => "Disabled",
        RegistryChange::Remove => "Removed",
    };
    let mut warnings = vec!["Unknown external consumers of shared/user registries cannot be enumerated; this source change may affect them. Capability qualification is not execution or protected-task approval.".into()];
    if matches!(change, RegistryChange::Remove)
        && let Some(fallback) = &selected.fallback
    {
        warnings.push(format!(
            "Removal exposes fallback from `{}`: provider {}, model {}, {}.",
            fallback.source.path.display(),
            fallback.provider,
            fallback.model.as_deref().unwrap_or("unknown"),
            if fallback.enabled {
                "enabled"
            } else {
                "disabled"
            },
        ));
    }
    Ok(MutationReport {
        message: format!("{verb} model profile `{name}` in `{}`.", path.display()),
        warnings,
        fallback: selected.fallback.clone(),
        source: path,
        previous_revision: expected_revision.into(),
        revision: revision(&after),
    })
}

/// Remove one unused canonical local profile from an explicitly scoped Kvist
/// configuration, pinned to its complete inspected source revision.
pub fn remove_local_profile(
    project: &Path,
    configuration: &Path,
    project_local: bool,
    name: &str,
    expected_revision: &str,
) -> Result<MutationReport> {
    let path = absolute(configuration)?;
    safe_path(&path)?;
    let _lock = SourceLock::acquire(&path)?;
    let before = read_optional(&path)?
        .ok_or_else(|| failure("selected configuration source does not exist"))?;
    if revision(&before) != expected_revision {
        return Err(failure(
            "stale configuration selection: inspect the model source again before removal",
        ));
    }
    let _role_locks = lock_role_sources(project, &path)?;
    let snapshot = super::inspect(project)?;
    let selected = snapshot.profiles.iter().find(|p| p.name == name && p.source.path == path && !p.registry_backed)
        .ok_or_else(|| failure(format!("profile `{name}` is not a local definition in `{}`; select its actual source/scope", path.display())))?;
    if !selected.references.is_empty() {
        return Err(failure(format!(
            "profile `{name}` has current-project or user role references; reassign or unassign these roles first with `kvist agent role set` or `kvist agent role clear` (use --global for user roles). No references were cleared."
        )));
    }
    let mut document = before
        .parse::<DocumentMut>()
        .map_err(|_| failure("invalid local configuration TOML"))?;
    let lookup = if document.contains_key("agent") {
        document
            .get_mut("agent")
            .ok_or_else(|| failure("missing agent table"))?
    } else {
        document.as_item_mut()
    };
    if lookup.get("registry").is_some() {
        return Err(failure(
            "registry-backed definitions require --registry PATH and its inspected revision",
        ));
    }
    let removed = lookup
        .get_mut("profiles")
        .and_then(Item::as_table_like_mut)
        .and_then(|profiles| profiles.remove(name));
    if removed.is_none() {
        return Err(failure(
            "selected source does not declare this canonical local profile",
        ));
    }
    let after = document.to_string();
    if project_local {
        crate::config::validate_project_configuration_contents(&path, project, &after)?;
    } else {
        crate::config::validate_agent_configuration_contents(&path, &after)?;
    }
    if super::inspect(project)? != snapshot
        || read_optional(&path)?.as_deref() != Some(before.as_str())
    {
        return Err(failure(
            "settings changed during profile removal; inspect again before editing",
        ));
    }
    replace_file_atomically(&path, &after)?;
    let mut warnings = vec!["Unknown external consumers of user/shared configuration cannot be enumerated; this source change may affect them. No role references or policy approvals were changed.".into()];
    if let Some(fallback) = &selected.fallback {
        warnings.push(format!(
            "Removal exposes fallback from `{}`: provider {}, model {}.",
            fallback.source.path.display(),
            fallback.provider,
            fallback.model.as_deref().unwrap_or("unknown"),
        ));
    }
    Ok(MutationReport {
        message: format!(
            "Removed local model profile `{name}` from `{}`.",
            path.display()
        ),
        warnings,
        fallback: selected.fallback.clone(),
        source: path,
        previous_revision: expected_revision.into(),
        revision: revision(&after),
    })
}

pub(super) struct SourceLock {
    path: PathBuf,
    _file: File,
}

fn lock_role_sources(project: &Path, except: &Path) -> Result<Vec<SourceLock>> {
    let mut sources = vec![
        absolute(&project.join("kvist.toml"))?,
        absolute(&project.join(".kvist/config.toml"))?,
    ];
    if let Some(user) = crate::config::global_user_config_path() {
        sources.push(absolute(&user)?);
    }
    sources.sort();
    sources.dedup();
    let mut locks = Vec::new();
    for source in sources {
        if source != except && read_optional(&source)?.is_some() {
            locks.push(SourceLock::acquire(&source)?);
        }
    }
    Ok(locks)
}

impl SourceLock {
    pub(super) fn acquire(source: &Path) -> Result<Self> {
        let filename = source
            .file_name()
            .ok_or_else(|| failure("registry source has no filename"))?;
        let path = source.with_file_name(format!(
            ".{}.kvist-settings.lock",
            filename.to_string_lossy()
        ));
        let started = std::time::Instant::now();
        let file = loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
                .open(&path)
            {
                Ok(file) => break file,
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists
                        && started.elapsed() < std::time::Duration::from_millis(250) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(source) => {
                    return Err(KvistError::Io {
                        operation: "lock settings source (if busy, retry; inspect orphaned locks before removing)",
                        path: path.clone(),
                        source,
                    });
                }
            }
        };
        Ok(Self { path, _file: file })
    }
}

impl Drop for SourceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
