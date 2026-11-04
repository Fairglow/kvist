//! Filesystem-only, credential-free model and role inspection.
//!
//! Capability lists are evidence labels, never execution or protected-task
//! approval. Revisions identify complete source bytes, including comments.

mod storage;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use sav::registry::{Backend, Registry, RegistryPaths, ResolvedProfile};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{KvistError, Result, config, config::Role, file_io::read_text_bounded};

pub use storage::{MutationReport, RegistryChange, mutate_registry, remove_local_profile};

/// Actual definition/configuration scope, independent of the selected component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    Project,
    User,
    System,
    Shared,
    BuiltIn,
}

/// Exact source whose bytes were inspected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    pub path: PathBuf,
    pub revision: String,
    pub scope: Scope,
}

/// A declared role reference, including inactive project/user configurations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoleReference {
    pub role: Role,
    pub profile: String,
    pub source: Source,
}

/// The complete next definition exposed by deleting a shadowing definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FallbackDefinition {
    pub source: Source,
    pub enabled: bool,
    pub backend: String,
    pub provider: String,
    pub model: Option<String>,
}

/// Safe metadata only: no commands, arguments, prompts, endpoints or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileInspection {
    pub name: String,
    pub enabled: bool,
    /// False for a definition only available in a different configuration scope.
    pub effective: bool,
    pub backend: String,
    pub provider: String,
    pub model: Option<String>,
    pub source: Source,
    pub advertised: Vec<String>,
    pub tested: Vec<String>,
    pub policy_enabled: Vec<String>,
    pub supports_thinking: Option<bool>,
    pub default_effort: Option<sav::ReasoningEffort>,
    pub references: Vec<RoleReference>,
    pub fallback: Option<FallbackDefinition>,
    /// Native backend eligibility is not qualification or policy approval.
    pub native_backend: bool,
    pub registry_backed: bool,
}

/// All three roles, resolved using the execution configuration's precedence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoleInspection {
    pub role: Role,
    /// Explicit binding, if present; built-in defaults are not assignments.
    pub assigned_profile: Option<String>,
    pub effective_profile: String,
    pub enabled: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    /// None means inherit, Some(ReasoningEffort::None) means explicit none.
    pub role_override: Option<sav::ReasoningEffort>,
    /// None means no known configured baseline.
    pub effective_effort: Option<sav::ReasoningEffort>,
    pub configuration: Source,
    pub profile_source: Option<Source>,
}

/// Raw binding in a specific project/user configuration, without losing effort
/// inheritance when another configuration takes precedence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoleBindingInspection {
    pub role: Role,
    pub profile: Option<String>,
    pub role_override: Option<sav::ReasoningEffort>,
    pub configuration: Source,
}

/// Bounded local settings snapshot. Reading this never probes or authorizes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingsInspection {
    pub configuration: Source,
    /// Inspected configuration inputs; useful for pinning a specific edit scope.
    pub sources: Vec<Source>,
    pub bindings: Vec<RoleBindingInspection>,
    pub roles: Vec<RoleInspection>,
    /// Includes disabled definitions and definitions in user configuration.
    pub profiles: Vec<ProfileInspection>,
}

/// Optional pins for the selected profile when editing a role binding.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProfilePin<'a> {
    pub source: Option<&'a Path>,
    pub revision: Option<&'a str>,
}

struct Document {
    source: Source,
    value: toml::Value,
}

impl Document {
    fn lookup(&self) -> &toml::Value {
        self.value.get("agent").unwrap_or(&self.value)
    }

    fn role(&self, role: Role) -> Option<&toml::Value> {
        self.lookup()
            .get("roles")?
            .as_table()?
            .iter()
            .find_map(|(name, binding)| {
                (Role::from_str_case_insensitive(name) == Some(role)).then_some(binding)
            })
    }
}

/// Inspect effective settings, plus local/user references and disabled models.
pub fn inspect(project_dir: &Path) -> Result<SettingsInspection> {
    inspect_with_registry(project_dir, None)
}

/// Inspect assignments and available profiles in the selected edit scope.
///
/// User scope resolves the user configuration independently of a project's
/// active source. Known project and user references remain included.
pub fn inspect_scope(project_dir: &Path, global: bool) -> Result<SettingsInspection> {
    inspect_inner(project_dir, None, global)
}

/// Inspect with an explicit registry overlay, using Sav's public precedence.
///
/// The overlay changes library inspection only, never persisted role authority.
pub fn inspect_with_registry(
    project_dir: &Path,
    explicit: Option<&Path>,
) -> Result<SettingsInspection> {
    inspect_inner(project_dir, explicit, false)
}

fn inspect_inner(
    project_dir: &Path,
    explicit: Option<&Path>,
    global: bool,
) -> Result<SettingsInspection> {
    let project_dir = absolute(project_dir)?;
    let mut documents = Vec::new();
    for (path, scope) in [
        (Some(project_dir.join("kvist.toml")), Scope::Project),
        (Some(project_dir.join(".kvist/config.toml")), Scope::Project),
        (config::global_user_config_path(), Scope::User),
        (config::global_system_config_path(), Scope::System),
    ] {
        let Some(path) = path else { continue };
        let path = absolute(&path)?;
        let Some(contents) = read_optional(&path)? else {
            continue;
        };
        let value: toml::Value = toml::from_str(&contents).map_err(|_| {
            failure(format!(
                "invalid TOML configuration `{}`; correct it before inspecting settings",
                path.display()
            ))
        })?;
        let document = Document {
            source: Source {
                path,
                revision: revision(&contents),
                scope,
            },
            value,
        };
        // Validate every considered source, including otherwise shadowed user
        // input. Preflight registry paths before the configuration resolver reads
        // them, retaining nonfollowing bounds for ancestor directories as well.
        if let Some(paths) = registry_paths(Some(&document), &project_dir, None)? {
            append_registry(&mut Vec::new(), &paths, &project_dir, &[], false)?;
        }
        validate_metadata(&document)?;
        config::validate_agent_configuration_contents(&document.source.path, &contents)?;
        documents.push(document);
    }
    let effective = if global {
        if let Some(document) = documents.iter().find(|d| d.source.scope == Scope::User) {
            config::load_agent_file(&document.source.path)?
        } else {
            config::AgentConfig::default()
        }
    } else if documents
        .iter()
        .any(|d| d.source.path == project_dir.join("kvist.toml"))
    {
        config::load(&project_dir)?.agent
    } else if let Some(document) = documents
        .iter()
        .find(|d| d.source.scope == Scope::User)
        .or_else(|| documents.iter().find(|d| d.source.scope == Scope::System))
    {
        config::load_agent_file(&document.source.path)?
    } else {
        config::AgentConfig::default()
    };
    let selected_document = documents.iter().find(|d| {
        d.source.path == Path::new(&effective.source.identity)
            || d.source.path.canonicalize().ok().as_deref()
                == Some(Path::new(&effective.source.identity))
    });
    let configuration = selected_document
        .map(|d| d.source.clone())
        .unwrap_or(Source {
            path: PathBuf::from("built-in:agent-default-v1"),
            revision: effective.source.digest.clone(),
            scope: Scope::BuiltIn,
        });
    let references = references(&documents)?;
    let mut profiles = Vec::new();
    let selected_paths = registry_paths(selected_document, &project_dir, explicit)?;
    if let Some(paths) = selected_paths {
        append_registry(&mut profiles, &paths, &project_dir, &references, true)?;
    } else {
        for profile in effective.profiles.values() {
            let Some(source) = local_profile_source(&documents, selected_document, &profile.name)
            else {
                // Execution defaults are not configured model-library entries.
                continue;
            };
            let provider = effective.providers.get(&profile.provider);
            profiles.push(ProfileInspection {
                name: profile.name.clone(),
                enabled: true,
                effective: true,
                // Never infer a native execution backend from a command string.
                backend: "local-command".into(),
                provider: provider
                    .map(|p| p.provider_type.clone())
                    .unwrap_or_else(|| profile.provider.clone()),
                model: Some(
                    profile
                        .model
                        .clone()
                        .unwrap_or_else(|| profile.name.clone()),
                ),
                source,
                advertised: Vec::new(),
                tested: Vec::new(),
                policy_enabled: Vec::new(),
                supports_thinking: profile.supports_thinking,
                default_effort: profile.default_thinking_effort,
                references: references
                    .iter()
                    .filter(|r| r.profile == profile.name)
                    .cloned()
                    .collect(),
                fallback: None,
                native_backend: false,
                registry_backed: false,
            });
        }
    }
    // User configuration remains visible even when project configuration wins.
    for document in documents.iter().filter(|d| d.source.scope == Scope::User) {
        if let Some(paths) = registry_paths(Some(document), &project_dir, None)? {
            append_registry(&mut profiles, &paths, &project_dir, &references, false)?;
        } else {
            let user = config::load_agent_file(&document.source.path)?;
            for profile in user.profiles.values().filter(|p| {
                document
                    .lookup()
                    .get("profiles")
                    .and_then(|profiles| profiles.get(&p.name))
                    .is_some()
            }) {
                if profiles
                    .iter()
                    .any(|p| p.name == profile.name && p.source == document.source)
                {
                    continue;
                }
                profiles.push(ProfileInspection {
                    name: profile.name.clone(),
                    enabled: true,
                    effective: false,
                    backend: "local-command".into(),
                    provider: user
                        .providers
                        .get(&profile.provider)
                        .map(|p| p.provider_type.clone())
                        .unwrap_or_else(|| profile.provider.clone()),
                    model: Some(
                        profile
                            .model
                            .clone()
                            .unwrap_or_else(|| profile.name.clone()),
                    ),
                    source: document.source.clone(),
                    advertised: Vec::new(),
                    tested: Vec::new(),
                    policy_enabled: Vec::new(),
                    supports_thinking: profile.supports_thinking,
                    default_effort: profile.default_thinking_effort,
                    references: references
                        .iter()
                        .filter(|r| r.profile == profile.name)
                        .cloned()
                        .collect(),
                    fallback: None,
                    native_backend: false,
                    registry_backed: false,
                });
            }
        }
    }
    // Nonempty local profile tables replace built-ins. Only deleting their
    // final entry can expose defaults, whether or not a user source exists.
    let defaults = config::AgentConfig::default();
    for index in 0..profiles.len() {
        if profiles[index].registry_backed {
            continue;
        }
        let name = &profiles[index].name;
        let defaults_resurface = documents
            .iter()
            .find(|document| document.source.path == profiles[index].source.path)
            .and_then(|document| document.lookup().get("profiles"))
            .and_then(toml::Value::as_table)
            .is_some_and(|definitions| definitions.len() == 1 && definitions.contains_key(name));
        let fallback = if let Some(default) =
            defaults.profiles.get(name).filter(|_| defaults_resurface)
        {
            Some(FallbackDefinition {
                source: Source {
                    path: PathBuf::from("built-in:agent-default-v1"),
                    revision: defaults.source.digest.clone(),
                    scope: Scope::BuiltIn,
                },
                enabled: true,
                backend: "local-command".into(),
                provider: defaults
                    .providers
                    .get(&default.provider)
                    .map(|p| p.provider_type.clone())
                    .unwrap_or_else(|| default.provider.clone()),
                model: Some(default.model.clone().unwrap_or_else(|| name.clone())),
            })
        } else if profiles[index].source.path == project_dir.join("kvist.toml") {
            profiles
                .iter()
                .find(|p| p.name == *name && p.source.scope == Scope::User && !p.registry_backed)
                .map(|p| FallbackDefinition {
                    source: p.source.clone(),
                    enabled: p.enabled,
                    backend: p.backend.clone(),
                    provider: p.provider.clone(),
                    model: p.model.clone(),
                })
        } else {
            None
        };
        profiles[index].fallback = fallback;
    }
    profiles.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| b.effective.cmp(&a.effective))
            .then_with(|| a.source.path.cmp(&b.source.path))
    });
    let roles = [Role::Developer, Role::Architect, Role::SecurityReviewer]
        .into_iter()
        .map(|role| {
            let binding = effective.role(role);
            let profile = profiles
                .iter()
                .find(|p| p.name == binding.profile && p.effective);
            let declared = selected_document
                .filter(|d| {
                    d.role(role)
                        .and_then(|r| r.get("profile").or_else(|| r.get("model")))
                        .and_then(toml::Value::as_str)
                        == Some(binding.profile.as_str())
                })
                .or_else(|| {
                    documents.iter().find(|d| {
                        d.source.scope == Scope::User
                            && d.role(role)
                                .and_then(|r| r.get("profile").or_else(|| r.get("model")))
                                .and_then(toml::Value::as_str)
                                == Some(binding.profile.as_str())
                    })
                });
            RoleInspection {
                role,
                assigned_profile: declared.map(|_| binding.profile.clone()),
                effective_profile: binding.profile.clone(),
                enabled: profile.is_some_and(|p| p.enabled),
                provider: profile.map(|p| p.provider.clone()),
                model: profile.and_then(|p| p.model.clone()),
                role_override: binding.thinking_effort,
                effective_effort: binding
                    .thinking_effort
                    .or_else(|| profile.and_then(|p| p.default_effort)),
                configuration: declared
                    .map(|d| d.source.clone())
                    .unwrap_or_else(|| configuration.clone()),
                profile_source: profile.map(|p| p.source.clone()),
            }
        })
        .collect();
    let mut bindings = Vec::new();
    for document in documents
        .iter()
        .filter(|d| matches!(d.source.scope, Scope::Project | Scope::User))
    {
        for role in [Role::Developer, Role::Architect, Role::SecurityReviewer] {
            if let Some(binding) = document.role(role) {
                bindings.push(RoleBindingInspection {
                    role,
                    profile: binding
                        .get("profile")
                        .or_else(|| binding.get("model"))
                        .and_then(toml::Value::as_str)
                        .map(str::to_owned),
                    role_override: binding
                        .get("thinking_effort")
                        .or_else(|| binding.get("reasoning_effort"))
                        .and_then(toml::Value::as_str)
                        .and_then(sav::ReasoningEffort::parse_effort),
                    configuration: document.source.clone(),
                });
            }
        }
    }
    Ok(SettingsInspection {
        configuration,
        sources: documents.iter().map(|d| d.source.clone()).collect(),
        bindings,
        roles,
        profiles,
    })
}

fn validate_metadata(document: &Document) -> Result<()> {
    let Some(profiles) = document
        .lookup()
        .get("profiles")
        .and_then(toml::Value::as_table)
    else {
        return Ok(());
    };
    for (name, profile) in profiles {
        for field in ["provider", "model"] {
            if let Some(value) = profile.get(field)
                && !value
                    .as_str()
                    .is_some_and(|v| !v.trim().is_empty() && !v.chars().any(char::is_control))
            {
                return Err(failure(format!(
                    "profile `{name}` has invalid {field} metadata in `{}`",
                    document.source.path.display()
                )));
            }
        }
        if profile
            .get("supports_thinking")
            .is_some_and(|v| !v.is_bool())
        {
            return Err(failure(format!(
                "profile `{name}` thinking support must be boolean"
            )));
        }
        if profile.get("default_thinking_effort").is_some_and(|v| {
            v.as_str()
                .and_then(sav::ReasoningEffort::parse_effort)
                .is_none()
        }) {
            return Err(failure(format!(
                "profile `{name}` has invalid default thinking effort"
            )));
        }
    }
    Ok(())
}

fn local_profile_source(
    documents: &[Document],
    selected: Option<&Document>,
    name: &str,
) -> Option<Source> {
    let declared = selected.filter(|d| {
        d.lookup()
            .get("profiles")
            .and_then(|p| p.get(name))
            .is_some()
    });
    if let Some(document) = declared {
        return Some(document.source.clone());
    }
    if selected.is_some_and(|d| d.source.scope != Scope::User)
        && config::AgentConfig::default().profiles.contains_key(name)
    {
        return None;
    }
    documents
        .iter()
        .find(|d| {
            d.source.scope == Scope::User
                && d.lookup()
                    .get("profiles")
                    .and_then(|p| p.get(name))
                    .is_some()
        })
        .map(|d| d.source.clone())
}

fn references(documents: &[Document]) -> Result<Vec<RoleReference>> {
    let mut references = Vec::new();
    for document in documents
        .iter()
        .filter(|d| matches!(d.source.scope, Scope::Project | Scope::User))
    {
        let parsed = config::load_agent_file(&document.source.path)?;
        for role in [Role::Developer, Role::Architect, Role::SecurityReviewer] {
            let explicit = document
                .role(role)
                .and_then(|r| r.get("profile").or_else(|| r.get("model")))
                .and_then(toml::Value::as_str);
            let implicit = parsed.role(role).profile.as_str();
            let profile = explicit.or_else(|| {
                (parsed.shared_profiles.contains_key(implicit)
                    || document
                        .lookup()
                        .get("profiles")
                        .and_then(|p| p.get(implicit))
                        .is_some())
                .then_some(implicit)
            });
            if let Some(profile) = profile {
                references.push(RoleReference {
                    role,
                    profile: profile.into(),
                    source: document.source.clone(),
                });
            }
        }
    }
    Ok(references)
}

fn registry_paths(
    document: Option<&Document>,
    project: &Path,
    explicit: Option<&Path>,
) -> Result<Option<RegistryPaths>> {
    let declaration = document.and_then(|d| d.lookup().get("registry"));
    if declaration.is_none() && explicit.is_none() {
        return Ok(None);
    }
    let base = document
        .and_then(|d| d.source.path.parent())
        .unwrap_or(project);
    let registry_project = if base.file_name().is_some_and(|name| name == ".kvist") {
        base.parent().unwrap_or(base)
    } else {
        base
    };
    let mut paths = RegistryPaths::for_project(registry_project);
    if let Some(path) = explicit {
        paths.explicit = Some(absolute(path)?);
    } else if let Some(path) = declaration
        .and_then(|d| d.get("path"))
        .and_then(toml::Value::as_str)
    {
        paths.explicit = Some(absolute(&base.join(path))?);
    }
    Ok(Some(paths))
}

fn append_registry(
    profiles: &mut Vec<ProfileInspection>,
    paths: &RegistryPaths,
    project: &Path,
    references: &[RoleReference],
    effective: bool,
) -> Result<()> {
    let mut definitions: BTreeMap<String, Vec<ResolvedProfile>> = BTreeMap::new();
    for (path, required) in [
        (&paths.user, false),
        (&paths.project, false),
        (&paths.explicit, true),
    ] {
        let Some(path) = path else { continue };
        let path = absolute(path)?;
        let Some(contents) = read_optional(&path)? else {
            if required {
                return Err(failure(format!(
                    "explicit registry `{}` does not exist",
                    path.display()
                )));
            }
            continue;
        };
        for profile in Registry::parse(&path, &contents)?.profiles() {
            let layers = definitions.entry(profile.name.clone()).or_default();
            layers.retain(|p| p.source != profile.source);
            layers.push(profile.clone());
        }
    }
    if definitions.len() > sav::registry::MAX_PROFILES {
        return Err(failure("merged registry exceeds 128 profiles"));
    }
    for (name, mut layers) in definitions {
        let Some(selected) = layers.pop() else {
            continue;
        };
        if profiles
            .iter()
            .any(|p| p.name == name && p.source.path == selected.source)
        {
            continue;
        }
        let (backend, provider, model, native) = backend_metadata(&selected);
        profiles.push(ProfileInspection {
            name: name.clone(),
            enabled: selected.profile.enabled,
            effective,
            backend,
            provider,
            model,
            source: registry_source(&selected, project)?,
            advertised: selected.profile.advertised.clone(),
            tested: selected.profile.tested.clone(),
            policy_enabled: selected.profile.policy_enabled.clone(),
            supports_thinking: None,
            default_effort: None,
            references: references
                .iter()
                .filter(|r| r.profile == name)
                .cloned()
                .collect(),
            fallback: layers
                .pop()
                .map(|fallback| -> Result<FallbackDefinition> {
                    let (backend, provider, model, _) = backend_metadata(&fallback);
                    Ok(FallbackDefinition {
                        source: registry_source(&fallback, project)?,
                        enabled: fallback.profile.enabled,
                        backend,
                        provider,
                        model,
                    })
                })
                .transpose()?,
            native_backend: native,
            registry_backed: true,
        });
    }
    Ok(())
}

fn backend_metadata(profile: &ResolvedProfile) -> (String, String, Option<String>, bool) {
    match &profile.profile.backend {
        Backend::Native(native) => (
            "native".into(),
            native.provider.to_string(),
            Some(native.model.clone()),
            true,
        ),
        Backend::ExternalAgent(agent) => (
            "external-agent".into(),
            agent.provider.as_str().into(),
            agent.model.clone(),
            false,
        ),
    }
}

fn registry_source(profile: &ResolvedProfile, project: &Path) -> Result<Source> {
    let path = absolute(&profile.source)?;
    let scope = if path == project.join(".kvist/providers.toml") {
        Scope::Project
    } else if sav::registry::default_registry_path()
        .as_deref()
        .map(absolute)
        .transpose()?
        .as_ref()
        == Some(&path)
    {
        Scope::User
    } else {
        Scope::Shared
    };
    Ok(Source {
        path,
        revision: profile.digest.clone(),
        scope,
    })
}

pub(super) fn failure(reason: impl Into<String>) -> KvistError {
    KvistError::AgentSetupFailed {
        reason: reason.into(),
    }
}

pub(super) fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| KvistError::Io {
                operation: "resolve settings path",
                path: path.into(),
                source,
            })?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

/// Refuse symlink traversal, including ancestor directories, before reading.
pub(super) fn safe_path(path: &Path) -> Result<()> {
    let mut ancestor = PathBuf::new();
    for component in absolute(path)?.components() {
        ancestor.push(component.as_os_str());
        match std::fs::symlink_metadata(&ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(failure(format!(
                    "settings source `{}` traverses a symbolic link",
                    path.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect settings path",
                    path: ancestor,
                    source,
                });
            }
        }
    }
    Ok(())
}

pub(super) fn read_optional(path: &Path) -> Result<Option<String>> {
    safe_path(path)?;
    match read_text_bounded(path, config::MAX_CONFIGURATION_BYTES as usize) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(KvistError::Io {
            operation: "read bounded settings source",
            path: path.into(),
            source,
        }),
    }
}

pub(super) fn revision(contents: &str) -> String {
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(contents.as_bytes()))
    )
}

/// Check a caller's optional configuration revision without creating files.
pub(crate) fn check_revision(path: &Path, expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        let contents = read_optional(path)?.ok_or_else(|| {
            failure("stale configuration selection: source no longer exists; inspect again")
        })?;
        if revision(&contents) != expected {
            return Err(failure(
                "stale configuration selection: source revision changed; inspect again",
            ));
        }
    }
    Ok(())
}

pub(crate) fn lock_source(path: &Path) -> Result<impl Drop> {
    safe_path(path)?;
    storage::SourceLock::acquire(path)
}

fn display(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .fold(String::new(), |mut output, ch| {
            if "\\`*_{}[]<>#|!".contains(ch) {
                output.push('\\');
            }
            output.push(ch);
            output
        })
}

impl ProfileInspection {
    /// Credential-free detail document; missing evidence remains explicitly unknown.
    pub fn details_markdown(&self) -> String {
        let mut text = format!(
            "# Model: {}\n\n- State: {}\n- Backend: {}\n- Provider: {}\n- Model: {}\n- Source: {} ({:?})\n- Revision: {}\n- Advertised: {}\n- Tested: {}\n- Policy-enabled capabilities: {}\n- Thinking support: {}\n\n",
            display(&self.name),
            if self.enabled { "enabled" } else { "disabled" },
            display(&self.backend),
            display(&self.provider),
            self.model
                .as_deref()
                .map(display)
                .unwrap_or_else(|| "unknown".into()),
            display(&self.source.path.to_string_lossy()),
            self.source.scope,
            display(&self.source.revision),
            evidence(&self.advertised),
            evidence(&self.tested),
            evidence(&self.policy_enabled),
            self.supports_thinking
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".into()),
        );
        if self.native_backend {
            text.push_str("Native backend: a candidate for protected tasks, not approval. Qualification and capability evidence never authorize execution; separate Kvist policy approval is required.\n\n");
        } else {
            text.push_str("External/local command backend: not eligible as a native protected-task executor. Qualification is not approval.\n\n");
        }
        text.push_str("Known current-project and user role references:\n\n");
        if self.references.is_empty() {
            text.push_str("- None found.\n");
        }
        for reference in &self.references {
            text.push_str(&format!(
                "- {} in {}\n",
                reference.role.as_str(),
                display(&reference.source.path.to_string_lossy())
            ));
        }
        text.push_str("\nUnknown external consumers of shared/user registries cannot be enumerated. Changing this definition can affect those consumers; these references are not a complete usage inventory.\n");
        if let Some(fallback) = &self.fallback {
            text.push_str(&format!(
                "\nRemoval exposes fallback: {} / {} / {} ({}, source {}, revision {}).\n",
                display(&fallback.backend),
                display(&fallback.provider),
                fallback
                    .model
                    .as_deref()
                    .map(display)
                    .unwrap_or_else(|| "unknown".into()),
                if fallback.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                display(&fallback.source.path.to_string_lossy()),
                display(&fallback.source.revision),
            ));
        } else {
            text.push_str(
                "\nRemoval exposes no known fallback in this inspected registry resolution.\n",
            );
        }
        text
    }
}

fn evidence(values: &[String]) -> String {
    if values.is_empty() {
        "unknown (no recorded evidence)".into()
    } else {
        values
            .iter()
            .map(|v| display(v))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl SettingsInspection {
    /// Human-readable all-role summary derived from this same typed snapshot.
    pub fn roles_markdown(&self) -> String {
        let mut text = "# Agent Roles\n\n```text\n│  Role                Assigned Model Profile\n│  ────────────────────────────────────────────────────────────────\n".to_owned();
        for role in [Role::Architect, Role::Developer, Role::SecurityReviewer] {
            if let Some(binding) = self.roles.iter().find(|r| r.role == role) {
                text.push_str(&format!(
                    "│  {:<20} {}\n",
                    role.as_str(),
                    display(&binding.effective_profile)
                ));
            }
        }
        text.push_str("```\n\n");
        for role in &self.roles {
            text.push_str(&format!(
                "## {}\n\n- Profile: {}\n- Provider: {}\n- Model: {}\n- Role effort: {}\n- Effective effort: {}\n- Configuration: {} ({:?})\n\n",
                role.role.as_str(), display(&role.effective_profile),
                role.provider.as_deref().map(display).unwrap_or_else(|| "unknown".into()),
                role.model.as_deref().map(display).unwrap_or_else(|| "unknown".into()),
                role.role_override.map(|v| v.as_str()).unwrap_or("inherit"),
                role.effective_effort.map(|v| v.as_str()).unwrap_or("unknown"),
                display(&role.configuration.path.to_string_lossy()), role.configuration.scope,
            ));
        }
        text.push_str("## Available Model Profiles\n\n");
        for profile in &self.profiles {
            text.push_str(&format!(
                "- {} ({})\n",
                display(&profile.name),
                if profile.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            ));
        }
        text
    }
}
