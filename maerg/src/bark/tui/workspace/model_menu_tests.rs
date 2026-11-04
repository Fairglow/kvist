use crate::config::Role;

use super::{
    Overlay, Workspace,
    menus::{Action, Menu, ModelKey, Page},
};

fn configured() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    crate::init::initialize(dir.path()).unwrap();
    std::fs::create_dir(dir.path().join(".kvist")).unwrap();
    let registry = dir.path().join(".kvist/providers.toml");
    std::fs::write(&registry, "schema_version = 1\n\
        [profiles.settings-a.backend]\nkind = \"native\"\nprovider = \"ollama\"\nmodel = \"fixture-a\"\n\
        [profiles.settings-b.backend]\nkind = \"native\"\nprovider = \"ollama\"\nmodel = \"fixture-b\"\n\
        [profiles.settings-disabled]\nenabled = false\n\
        [profiles.settings-disabled.backend]\nkind = \"native\"\nprovider = \"ollama\"\nmodel = \"old-fixture\"\n").unwrap();
    let path = dir.path().join("kvist.toml");
    let configuration = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        format!(
            "{configuration}\n\
        [agent.registry]\npath = \".kvist/providers.toml\"\n\
        [agent.roles.developer]\nprofile = \"settings-a\"\nthinking_effort = \"high\"\n\
        [agent.roles.architect]\nprofile = \"settings-a\"\n\
        [agent.roles.security-reviewer]\nprofile = \"settings-a\"\nthinking_effort = \"none\"\n"
        ),
    )
    .unwrap();
    let workspace = Workspace::new(dir.path()).unwrap();
    (dir, workspace)
}

fn menu(workspace: &Workspace) -> &Menu {
    let Some(Overlay::Menu(menu)) = &workspace.overlay else {
        panic!("expected menu");
    };
    menu
}

#[test]
fn role_pages_start_in_user_scope_outside_an_initialized_project() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new(dir.path()).unwrap();
    workspace.open_menu(Page::Roles).unwrap();
    let developer = menu(&workspace)
        .choices
        .iter()
        .find(|entry| entry.label.starts_with("developer:"))
        .unwrap();
    assert!(matches!(
        developer.action,
        Action::Open(Page::Role {
            role: Role::Developer,
            global: true
        })
    ));
}

#[test]
fn all_roles_have_correct_current_effort_including_inherit_and_explicit_none() {
    let (_dir, mut workspace) = configured();
    for (role, current) in [
        (Role::Developer, "high"),
        (Role::Architect, "Default / inherit"),
        (Role::SecurityReviewer, "none"),
    ] {
        workspace
            .open_menu(Page::Effort {
                role,
                global: false,
            })
            .unwrap();
        let menu = menu(&workspace);
        assert!(menu.choices[menu.selected].current);
        assert!(
            menu.choices[menu.selected].label.starts_with(current),
            "{}",
            menu.choices[menu.selected].label
        );
        assert!(matches!(menu.choices[menu.selected].action, Action::Back));
    }
}

#[test]
fn role_model_commands_pin_both_configuration_and_definition_and_keep_the_role() {
    let (_dir, mut workspace) = configured();
    workspace
        .open_menu(Page::RoleModel {
            role: Role::Architect,
            global: false,
        })
        .unwrap();
    let menu = menu(&workspace);
    assert!(menu.choices[menu.selected].label.starts_with("settings-a"));
    assert!(
        menu.choices
            .iter()
            .all(|entry| !entry.label.contains("settings-disabled"))
    );
    let next = menu
        .choices
        .iter()
        .find(|entry| entry.label.starts_with("settings-b"))
        .unwrap();
    let Action::Command {
        arguments,
        return_to,
        ..
    } = &next.action
    else {
        panic!("expected role update");
    };
    assert_eq!(
        &arguments[..5],
        ["agent", "role", "set", "architect", "settings-b"]
    );
    for flag in [
        "--expected-revision",
        "--profile-source",
        "--profile-revision",
    ] {
        assert!(arguments.iter().any(|arg| arg == flag));
    }
    assert_eq!(
        *return_to,
        Page::Role {
            role: Role::Architect,
            global: false
        }
    );
}

#[test]
fn model_library_contains_disabled_profiles_and_inspection_is_offline() {
    let (_dir, mut workspace) = configured();
    workspace.open_menu(Page::Models).unwrap();
    assert!(menu(&workspace).choices.iter().any(
        |entry| entry.label.contains("settings-disabled") && entry.label.contains("[disabled]")
    ));
    assert!(workspace.running.is_none());
    workspace.paste("settings-b");
    let visible = menu(&workspace).visible();
    assert_eq!(visible.len(), 1);
    assert!(
        menu(&workspace).choices[visible[0]]
            .label
            .starts_with("settings-b")
    );
}

#[test]
fn model_actions_pin_source_and_revision_and_stale_details_are_refused() {
    let (_dir, workspace) = configured();
    let snapshot = crate::model_settings::inspect(&workspace.project).unwrap();
    let profile = snapshot
        .profiles
        .iter()
        .find(|profile| profile.name == "settings-b")
        .unwrap();
    let key = ModelKey {
        name: profile.name.clone(),
        source: profile.source.path.clone(),
        revision: profile.source.revision.clone(),
    };
    let model_menu = workspace.build_menu(Page::Model(key.clone())).unwrap();
    let remove = model_menu
        .choices
        .iter()
        .find(|entry| entry.label == "Remove model")
        .unwrap();
    let Action::Command {
        arguments, confirm, ..
    } = &remove.action
    else {
        panic!("expected pinned removal");
    };
    assert!(
        arguments
            .windows(2)
            .any(|pair| pair[0] == "--registry" && pair[1] == key.source.to_string_lossy())
    );
    assert!(
        arguments
            .windows(2)
            .any(|pair| pair[0] == "--expected-revision" && pair[1] == key.revision)
    );
    assert!(confirm.as_ref().unwrap().contains("outside this project"));
    let original = std::fs::read_to_string(&key.source).unwrap();
    std::fs::write(&key.source, format!("# changed\n{original}")).unwrap();
    let error = workspace.build_menu(Page::Model(key)).err().unwrap();
    assert!(error.to_string().contains("changed"), "{error}");
}

#[test]
fn referenced_model_actions_explain_required_unassignment_instead_of_attempting_mutation() {
    let (_dir, workspace) = configured();
    let snapshot = crate::model_settings::inspect(&workspace.project).unwrap();
    let profile = snapshot
        .profiles
        .iter()
        .find(|profile| profile.name == "settings-a")
        .unwrap();
    let model_menu = workspace
        .build_menu(Page::Model(ModelKey {
            name: profile.name.clone(),
            source: profile.source.path.clone(),
            revision: profile.source.revision.clone(),
        }))
        .unwrap();
    let remove = model_menu
        .choices
        .iter()
        .find(|entry| entry.label.starts_with("Remove"))
        .unwrap();
    assert!(matches!(remove.action, Action::Details { .. }));
    assert!(remove.label.contains("in use"));
}

#[test]
fn scoped_role_picker_uses_effective_availability_not_shared_path_heuristics() {
    let (_dir, workspace) = configured();
    let mut snapshot = crate::model_settings::inspect(&workspace.project).unwrap();
    let mut shadowed = snapshot
        .profiles
        .iter()
        .find(|profile| profile.name == "settings-a")
        .unwrap()
        .clone();
    shadowed.name = "shadowed-shared".into();
    shadowed.source.scope = crate::model_settings::Scope::Shared;
    shadowed.effective = false;
    snapshot.profiles.push(shadowed);
    let menu = workspace
        .role_menu(
            Page::RoleModel {
                role: Role::Developer,
                global: true,
            },
            Role::Developer,
            true,
            &snapshot,
        )
        .unwrap();
    assert!(
        menu.choices
            .iter()
            .any(|entry| entry.label.starts_with("settings-a"))
    );
    assert!(
        !menu
            .choices
            .iter()
            .any(|entry| entry.label.starts_with("shadowed-shared"))
    );
}

#[test]
fn first_project_edit_preselects_the_effective_effort_that_will_be_preserved() {
    let (_dir, workspace) = configured();
    let mut snapshot = crate::model_settings::inspect(&workspace.project).unwrap();
    snapshot
        .bindings
        .retain(|binding| binding.role != Role::Developer);
    let menu = workspace
        .role_menu(
            Page::Effort {
                role: Role::Developer,
                global: false,
            },
            Role::Developer,
            false,
            &snapshot,
        )
        .unwrap();
    assert!(menu.choices[menu.selected].label.starts_with("high"));
    assert!(menu.choices[menu.selected].current);
}
