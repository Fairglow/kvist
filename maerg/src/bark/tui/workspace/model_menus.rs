//! Role/model presentation over the shared typed, filesystem-only inspection.

use std::{io, path::Path};

use crate::{
    config::Role,
    model_settings::{self, ProfileInspection, RoleInspection, Scope, SettingsInspection},
};

use super::{
    Workspace,
    menus::{Action, Entry, Menu, ModelKey, Page},
};

fn scope(scope: Scope) -> &'static str {
    match scope {
        Scope::Project => "project",
        Scope::User => "user",
        Scope::System => "system",
        Scope::Shared => "shared",
        Scope::BuiltIn => "built-in",
    }
}

fn effort(value: Option<sav::ReasoningEffort>) -> &'static str {
    value
        .map(|value| value.as_str())
        .unwrap_or("provider default (unknown)")
}

fn role_info(snapshot: &SettingsInspection, role: Role) -> io::Result<&RoleInspection> {
    snapshot
        .roles
        .iter()
        .find(|entry| entry.role == role)
        .ok_or_else(|| io::Error::other(format!("{} role is unavailable", role.as_str())))
}

fn target_path(project: &Path, global: bool) -> io::Result<std::path::PathBuf> {
    if global {
        crate::config::global_user_config_path()
            .ok_or_else(|| io::Error::other("user configuration location is unavailable"))
    } else {
        let path = project.join("kvist.toml");
        if !path.try_exists()? {
            return Err(io::Error::other(
                "initialize a project before editing project role settings",
            ));
        }
        Ok(path)
    }
}

fn role_arguments(
    snapshot: &SettingsInspection,
    path: &Path,
    global: bool,
    verb: &str,
    role: Role,
) -> Vec<String> {
    let mut args = vec![
        "agent".into(),
        "role".into(),
        verb.into(),
        role.as_str().into(),
    ];
    if let Some(source) = snapshot.sources.iter().find(|source| source.path == path) {
        args.extend(["--expected-revision".into(), source.revision.clone()]);
    }
    if global {
        args.push("--global".into());
    }
    args
}

fn profile_for_role<'a>(
    snapshot: &'a SettingsInspection,
    name: &str,
) -> Option<&'a ProfileInspection> {
    snapshot
        .profiles
        .iter()
        .find(|profile| profile.name == name && profile.effective)
}

fn key(profile: &ProfileInspection) -> ModelKey {
    ModelKey {
        name: profile.name.clone(),
        source: profile.source.path.clone(),
        revision: profile.source.revision.clone(),
    }
}

fn profile_arguments(verb: &str, profile: &ProfileInspection) -> Vec<String> {
    let mut args = vec!["agent".into(), verb.into(), profile.name.clone()];
    if profile.registry_backed {
        args.extend([
            "--registry".into(),
            profile.source.path.to_string_lossy().into_owned(),
        ]);
    }
    args.extend([
        "--expected-revision".into(),
        profile.source.revision.clone(),
    ]);
    if profile.source.scope == Scope::User {
        args.push("--global".into());
    }
    args
}

fn mutation_confirmation(verb: &str, profile: &ProfileInspection) -> String {
    let fallback = if verb == "remove" {
        match &profile.fallback {
            Some(fallback) => format!(
                "\nRemoval exposes {} / {} from {} ({}).",
                fallback.provider,
                fallback.model.as_deref().unwrap_or("unknown model"),
                fallback.source.path.display(),
                if fallback.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
            None => "\nNo known fallback will be exposed.".into(),
        }
    } else {
        String::new()
    };
    format!(
        "{verb} model `{}` in {} ({}, revision {})?{}\n\n\
        Known role references: {}. Reassign/unassign them first. Shared/user definitions \
        may have consumers outside this project; these cannot be enumerated. This does not approve execution.",
        profile.name,
        profile.source.path.display(),
        scope(profile.source.scope),
        profile.source.revision,
        fallback,
        profile.references.len()
    )
}

fn mutation_entry(label: &str, verb: &str, profile: &ProfileInspection) -> Entry {
    if verb != "enable" && !profile.references.is_empty() {
        return Entry::new(
            format!("{label} (in use; unassign role references first)"),
            Action::Details {
                title: "Model is in use".into(),
                markdown: format!(
                    "Reassign or unassign these roles through Role assignments before {verb}.\n\n{}",
                    profile.details_markdown()
                ),
            },
        );
    }
    Entry::new(
        label,
        Action::Command {
            arguments: profile_arguments(verb, profile),
            confirm: Some(mutation_confirmation(verb, profile)),
            return_to: Page::Models,
        },
    )
}

impl Workspace {
    pub(super) fn model_menu(&self, page: Page) -> io::Result<Menu> {
        let global = matches!(
            &page,
            Page::Role { global: true, .. }
                | Page::RoleModel { global: true, .. }
                | Page::Effort { global: true, .. }
        );
        let snapshot =
            model_settings::inspect_scope(&self.project, global).map_err(io::Error::other)?;
        match &page {
            Page::Roles => {
                let mut entries = Vec::new();
                let global = !self.project.join("kvist.toml").try_exists()?;
                for role in [Role::Developer, Role::Architect, Role::SecurityReviewer] {
                    let assignment = role_info(&snapshot, role)?;
                    entries.push(Entry::new(
                        format!(
                            "{}: {} | effort: {} | {}",
                            role.as_str(),
                            assignment.effective_profile,
                            effort(assignment.effective_effort),
                            scope(assignment.configuration.scope)
                        ),
                        Action::Open(Page::Role { role, global }),
                    ));
                }
                entries.push(Entry::new(
                    "All assignments, providers and sources",
                    Action::Details {
                        title: "Role assignments".into(),
                        markdown: snapshot.roles_markdown(),
                    },
                ));
                Ok(Menu::new(
                    page,
                    "Role assignments | settings never approve policy",
                    entries,
                ))
            }
            Page::Role { role, global }
            | Page::RoleModel { role, global }
            | Page::Effort { role, global } => {
                self.role_menu(page.clone(), *role, *global, &snapshot)
            }
            Page::Models => {
                let mut entries = vec![
                    Entry::new("Add model (provider discovery and qualification)", Action::Command {
                        arguments: vec!["agent".into(), "setup".into()],
                        confirm: Some("Add a model using the setup wizard? Discovery/qualification executes bounded host commands or contacts configured providers and may incur billing. This acknowledgement does not authorize later prompts or protected tasks.".into()),
                        return_to: Page::Models,
                    }),
                    Entry::new("Check project-configured models (explicit host acknowledgement)", Action::Command {
                        arguments: vec!["agent".into(), "check".into()], confirm: None, return_to: Page::Models,
                    }),
                    Entry::new("Check user-configured models (explicit host acknowledgement)", Action::Command {
                        arguments: vec!["agent".into(), "check".into(), "--global".into()],
                        confirm: None, return_to: Page::Models,
                    }),
                ];
                entries.extend(snapshot.profiles.iter().map(|profile| {
                    Entry::new(
                        format!(
                            "{} | {}/{} | {}{}{}",
                            profile.name,
                            profile.provider,
                            profile.model.as_deref().unwrap_or("unknown"),
                            scope(profile.source.scope),
                            if profile.enabled { "" } else { " [disabled]" },
                            if profile.effective {
                                ""
                            } else {
                                " [other scope/shadowed]"
                            }
                        ),
                        Action::Open(Page::Model(key(profile))),
                    )
                }));
                Ok(Menu::new(
                    page,
                    "Models | local inspection; type to filter",
                    entries,
                ))
            }
            Page::Model(pin) => {
                let profile = snapshot
                    .profiles
                    .iter()
                    .find(|profile| profile.name == pin.name && profile.source.path == pin.source)
                    .ok_or_else(|| {
                        io::Error::other("model definition no longer exists; reopen Models")
                    })?;
                if profile.source.revision != pin.revision {
                    return Err(io::Error::other(
                        "model definition changed; reopen Models before editing",
                    ));
                }
                let mut entries = vec![Entry::new(
                    "Details, capabilities, references and fallback",
                    Action::Details {
                        title: format!("Model: {}", profile.name),
                        markdown: profile.details_markdown(),
                    },
                )];
                if profile.registry_backed {
                    let verb = if profile.enabled { "disable" } else { "enable" };
                    entries.push(mutation_entry(
                        if profile.enabled {
                            "Disable model"
                        } else {
                            "Enable model"
                        },
                        verb,
                        profile,
                    ));
                } else {
                    entries.push(Entry::new("Enable/disable requires a registry-backed profile",
                        Action::Details { title: "Profile format".into(),
                            markdown: "This definition is in a Kvist configuration, not the shared registry. Use Add model to configure a registry-backed profile. No enabled state is being invented for this definition.".into() }));
                }
                if !matches!(profile.source.scope, Scope::BuiltIn | Scope::System) {
                    entries.push(mutation_entry("Remove model", "remove", profile));
                }
                Ok(Menu::new(
                    page.clone(),
                    format!("Models / {} ({})", pin.name, scope(profile.source.scope)),
                    entries,
                ))
            }
            _ => Err(io::Error::other("not a role/model page")),
        }
    }

    pub(super) fn role_menu(
        &self,
        page: Page,
        role: Role,
        global: bool,
        snapshot: &SettingsInspection,
    ) -> io::Result<Menu> {
        let path = target_path(&self.project, global)?;
        let assignment = role_info(snapshot, role)?;
        let binding = snapshot
            .bindings
            .iter()
            .find(|entry| entry.role == role && entry.configuration.path == path);
        let name = binding
            .and_then(|entry| entry.profile.as_deref())
            .unwrap_or(&assignment.effective_profile);
        let override_effort = binding.map_or(assignment.role_override, |entry| entry.role_override);
        let profile = profile_for_role(snapshot, name);
        let effective = override_effort.or_else(|| profile.and_then(|entry| entry.default_effort));
        let role_page = Page::Role { role, global };
        let source = if global { "user" } else { "project" };
        let title = format!(
            "Roles / {} | editing {source} | policy reapproval may be required",
            role.as_str()
        );
        match &page {
            Page::Role { .. } => {
                let clear = role_arguments(snapshot, &path, global, "clear", role);
                let entries = vec![
                    Entry::new(
                        format!("Model: {name}"),
                        Action::Open(Page::RoleModel { role, global }),
                    ),
                    Entry::new(
                        format!(
                            "Reasoning effort: {}{}",
                            effort(effective),
                            if override_effort.is_none() {
                                " [inherited]"
                            } else {
                                ""
                            }
                        ),
                        Action::Open(Page::Effort { role, global }),
                    ),
                    Entry::new(
                        format!(
                            "Editing {source} settings (switch to {})",
                            if global { "project" } else { "user" }
                        ),
                        Action::Replace(Page::Role {
                            role,
                            global: !global,
                        }),
                    ),
                    Entry::new(
                        "Unassign this role in this scope",
                        Action::Command {
                            arguments: clear,
                            confirm: Some(format!(
                                "Clear {} binding in {}? Task kinds and other role policies are unchanged; effective defaults may change. Execution remains subject to policy reapproval.",
                                role.as_str(),
                                path.display()
                            )),
                            return_to: role_page.clone(),
                        },
                    ),
                    Entry::new(
                        "Assignment sources and effective values",
                        Action::Details {
                            title: role.as_str().into(),
                            markdown: format!(
                                "{}\n\n## Editing scope\n\n{} configuration: {}\n\nEditing this scope does not change task-kind role selection. User settings can be shadowed by a project's agent configuration. Changes apply to subsequent execution, never the active command.",
                                snapshot.roles_markdown(),
                                source,
                                crate::presentation::literal(&path.display().to_string())
                            ),
                        },
                    ),
                ];
                Ok(Menu::new(page, title, entries))
            }
            Page::RoleModel { .. } => {
                let mut entries = Vec::new();
                for candidate in snapshot
                    .profiles
                    .iter()
                    .filter(|candidate| candidate.enabled && candidate.effective)
                {
                    let current = candidate.name == name
                        && profile
                            .is_some_and(|profile| profile.source.path == candidate.source.path);
                    let mut arguments = role_arguments(snapshot, &path, global, "set", role);
                    arguments.insert(4, candidate.name.clone());
                    arguments.extend([
                        "--profile-source".into(),
                        candidate.source.path.to_string_lossy().into_owned(),
                        "--profile-revision".into(),
                        candidate.source.revision.clone(),
                    ]);
                    let warning = (!candidate.native_backend).then(|| format!(
                        "Assign external/local command profile `{}` to {}? This profile is not a native protected-task executor. Qualification does not approve task execution.",
                        candidate.name, role.as_str()));
                    entries.push(
                        Entry::new(
                            format!(
                                "{} | {}/{} [{}]{}",
                                candidate.name,
                                candidate.provider,
                                candidate.model.as_deref().unwrap_or("unknown"),
                                scope(candidate.source.scope),
                                if candidate.native_backend {
                                    ""
                                } else {
                                    " [not native task execution]"
                                }
                            ),
                            if current {
                                Action::Back
                            } else {
                                Action::Command {
                                    arguments,
                                    confirm: warning,
                                    return_to: role_page.clone(),
                                }
                            },
                        )
                        .current(current),
                    );
                }
                if entries.is_empty() {
                    entries.push(Entry::new("No enabled models; use Models / Add model", Action::Details {
                        title: "No assignable model".into(), markdown: "Add a model using Models / Add model, then return to this role. Disabled definitions cannot be assigned.".into(),
                    }));
                }
                Ok(Menu::new(page, format!("{title} / Model"), entries))
            }
            Page::Effort { .. } => {
                let mut entries = Vec::new();
                let mut inherited = role_arguments(snapshot, &path, global, "set", role);
                inherited.insert(4, name.to_owned());
                inherited.push("--inherit-effort".into());
                if let Some(profile) = profile {
                    inherited.extend([
                        "--profile-source".into(),
                        profile.source.path.to_string_lossy().into_owned(),
                        "--profile-revision".into(),
                        profile.source.revision.clone(),
                    ]);
                }
                entries.push(
                    Entry::new(
                        format!(
                            "Default / inherit (profile: {})",
                            effort(profile.and_then(|entry| entry.default_effort))
                        ),
                        if override_effort.is_none() {
                            Action::Back
                        } else {
                            Action::Command {
                                arguments: inherited,
                                confirm: None,
                                return_to: role_page.clone(),
                            }
                        },
                    )
                    .current(override_effort.is_none()),
                );
                let support = profile.and_then(|entry| entry.supports_thinking);
                if support != Some(false) && profile.is_some_and(|profile| profile.enabled) {
                    for value in [
                        sav::ReasoningEffort::None,
                        sav::ReasoningEffort::Minimal,
                        sav::ReasoningEffort::Low,
                        sav::ReasoningEffort::Medium,
                        sav::ReasoningEffort::High,
                        sav::ReasoningEffort::Xhigh,
                        sav::ReasoningEffort::Max,
                    ] {
                        let current = override_effort == Some(value);
                        let mut arguments = role_arguments(snapshot, &path, global, "set", role);
                        arguments.insert(4, name.to_owned());
                        arguments.extend(["--thinking-effort".into(), value.as_str().into()]);
                        if let Some(profile) = profile {
                            arguments.extend([
                                "--profile-source".into(),
                                profile.source.path.to_string_lossy().into_owned(),
                                "--profile-revision".into(),
                                profile.source.revision.clone(),
                            ]);
                        }
                        entries.push(
                            Entry::new(
                                format!(
                                    "{}{}",
                                    value.as_str(),
                                    if support.is_none() {
                                        " (support unverified)"
                                    } else {
                                        ""
                                    }
                                ),
                                if current {
                                    Action::Back
                                } else {
                                    Action::Command {
                                        arguments,
                                        confirm: None,
                                        return_to: role_page.clone(),
                                    }
                                },
                            )
                            .current(current),
                        );
                    }
                } else {
                    entries.push(Entry::new("Explicit effort unavailable for this model", Action::Details {
                        title: "Reasoning support".into(),
                        markdown: format!("The configured profile does not expose supported reasoning effort. Current role override: {}. Choose Default / inherit to remove an existing override, or assign a capable model first.", effort(override_effort)),
                    }).current(override_effort.is_some()));
                }
                Ok(Menu::new(
                    page,
                    format!("{title} / Reasoning effort"),
                    entries,
                ))
            }
            _ => Err(io::Error::other("not a role settings page")),
        }
    }
}
