use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use maerg::{
    config::Role,
    model_settings::{self, RegistryChange, Scope},
};
use sav::registry::Registry;

fn project() -> tempfile::TempDir {
    let project = tempfile::tempdir_in(".").unwrap();
    fs::create_dir(project.path().join(".kvist")).unwrap();
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n[agent.registry]\n",
    )
    .unwrap();
    project
}

fn registry(path: &Path, model: &str, enabled: bool) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!(
            "# preserve registry comment\nschema_version = 1\n\
             [profiles.selected]\nenabled = {enabled}\n\
             advertised = [\"tools\"]\ntested = []\npolicy_enabled = []\n\
             [profiles.selected.backend]\nkind = \"native\"\nprovider = \"ollama\"\n\
             base_url = \"http://127.0.0.1:11434\"\nmodel = \"{model}\"\n\
             # preserve unrelated entry\n[profiles.unrelated.backend]\n\
             kind = \"native\"\nprovider = \"ollama\"\n\
             base_url = \"http://127.0.0.1:11434\"\nmodel = \"other\"\n"
        ),
    )
    .unwrap();
}

fn command(project: &Path, args: &[&str]) -> Output {
    let project = project.canonicalize().unwrap();
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(args)
        .current_dir(&project)
        .env("KVIST_CONFIG_PATH", project.join("user.toml"))
        .env("HOME", &project)
        .env("XDG_CONFIG_HOME", project.join("config"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn json(output: &Output) -> serde_json::Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn project_registry(project: &Path) -> PathBuf {
    project.join(".kvist/providers.toml")
}

fn isolated(name: &str) -> bool {
    const KEY: &str = "KVIST_MODEL_SETTINGS_ISOLATED_TEST";
    if std::env::var(KEY).as_deref() == Ok(name) {
        return false;
    }
    let home = tempfile::tempdir_in(".").unwrap();
    let home = home.path().canonicalize().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(KEY, name)
        .env("KVIST_CONFIG_PATH", home.join("user.toml"))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    true
}

#[test]
fn inspection_is_typed_bounded_and_includes_disabled_capability_evidence() {
    if isolated("inspection_is_typed_bounded_and_includes_disabled_capability_evidence") {
        return;
    }
    let project = project();
    registry(&project_registry(project.path()), "selected-model", false);
    let inspection = model_settings::inspect(project.path()).unwrap();
    assert_eq!(inspection.roles.len(), 3);
    let profile = inspection
        .profiles
        .iter()
        .find(|p| p.name == "selected")
        .unwrap();
    assert!(!profile.enabled);
    assert_eq!(profile.model.as_deref(), Some("selected-model"));
    assert_eq!(profile.source.scope, Scope::Project);
    assert_eq!(profile.advertised, ["tools"]);
    assert!(profile.tested.is_empty());
    assert!(profile.policy_enabled.is_empty());
    assert!(profile.details_markdown().contains("not approval"));
    assert!(
        inspection
            .roles
            .iter()
            .any(|role| role.role == Role::SecurityReviewer)
    );
}

#[test]
fn removal_edits_actual_source_and_refuses_stale_revision_without_writes() {
    if isolated("removal_edits_actual_source_and_refuses_stale_revision_without_writes") {
        return;
    }
    let project = project();
    let path = project_registry(project.path());
    registry(&path, "selected-model", true);
    let selected = model_settings::inspect(project.path())
        .unwrap()
        .profiles
        .into_iter()
        .find(|p| p.name == "selected")
        .unwrap();
    let before = fs::read_to_string(&path).unwrap();
    assert!(
        model_settings::mutate_registry(
            project.path(),
            &path,
            "selected",
            "sha256:stale",
            RegistryChange::Remove
        )
        .is_err()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    model_settings::mutate_registry(
        project.path(),
        &path,
        "selected",
        &selected.source.revision,
        RegistryChange::Remove,
    )
    .unwrap();
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("# preserve registry comment"));
    assert!(after.contains("# preserve unrelated entry"));
    assert!(Registry::load(&path).unwrap().resolve("selected").is_err());
    assert!(Registry::load(&path).unwrap().resolve("unrelated").is_ok());
}

#[test]
fn referenced_disable_and_remove_refuse_without_clearing_role() {
    if isolated("referenced_disable_and_remove_refuse_without_clearing_role") {
        return;
    }
    let project = project();
    let path = project_registry(project.path());
    registry(&path, "selected-model", true);
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n[agent.registry]\n\
         [agent.roles.developer]\nprofile = \"selected\"\n",
    )
    .unwrap();
    let before = fs::read_to_string(&path).unwrap();
    let selected = model_settings::inspect(project.path())
        .unwrap()
        .profiles
        .into_iter()
        .find(|p| p.name == "selected")
        .unwrap();
    for action in [RegistryChange::Disable, RegistryChange::Remove] {
        let error = model_settings::mutate_registry(
            project.path(),
            &path,
            "selected",
            &selected.source.revision,
            action,
        )
        .unwrap_err();
        assert!(error.to_string().contains("reassign"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }
}

#[test]
fn cli_details_and_registry_lifecycle_preserve_structured_output() {
    let project = project();
    let path = project_registry(project.path());
    registry(&path, "selected-model", true);
    let output = command(project.path(), &["--json", "agent", "show", "selected"]);
    assert!(output.status.success(), "{:?}", output);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "success");
    let revision = json["profile"]["source"]["revision"].as_str().unwrap();
    let output = command(
        project.path(),
        &[
            "--json",
            "agent",
            "disable",
            "selected",
            "--registry",
            path.to_str().unwrap(),
            "--expected-revision",
            revision,
        ],
    );
    assert!(output.status.success(), "{:?}", output);
    assert!(Registry::load(&path).unwrap().resolve("selected").is_err());
    let output = command(project.path(), &["agent", "show", "selected"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("# "));
    assert!(text.contains("selected-model"));
    assert!(text.contains("external consumers"));
    assert!(text.contains("disabled"));
    let output = command(
        project.path(),
        &[
            "--json",
            "agent",
            "remove",
            "selected",
            "--registry",
            path.to_str().unwrap(),
            "--expected-revision",
            revision,
        ],
    );
    assert!(!output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(json["status"], "error");
}

#[test]
fn role_effort_inherit_is_distinct_from_explicit_none_and_preserves_policy() {
    let project = project();
    registry(&project_registry(project.path()), "selected-model", true);
    for (flag, expected) in [
        ("--thinking-effort=none", Some("none")),
        ("--inherit-effort", None),
    ] {
        let output = command(
            project.path(),
            &["agent", "role", "set", "developer", "selected", flag],
        );
        assert!(output.status.success(), "{:?}", output);
        let contents = fs::read_to_string(project.path().join("kvist.toml")).unwrap();
        let doc: toml::Value = toml::from_str(&contents).unwrap();
        assert_eq!(
            doc["agent"]["roles"]["developer"]
                .get("thinking_effort")
                .and_then(toml::Value::as_str),
            expected
        );
        assert_eq!(
            doc["agent"]["roles"]["developer"]["profile"].as_str(),
            Some("selected")
        );
    }
    let output = command(
        project.path(),
        &[
            "agent",
            "role",
            "set",
            "developer",
            "selected",
            "--thinking-effort=none",
            "--inherit-effort",
        ],
    );
    assert!(!output.status.success());
}

#[test]
fn disabled_selection_and_malformed_configuration_never_write() {
    let project = project();
    registry(&project_registry(project.path()), "selected-model", false);
    let path = project.path().join("kvist.toml");
    let before = fs::read_to_string(&path).unwrap();
    let output = command(
        project.path(),
        &["agent", "role", "set", "developer", "selected"],
    );
    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    fs::write(&path, "this is not valid TOML").unwrap();
    assert!(model_settings::inspect(project.path()).is_err());
}

#[test]
fn inspection_refuses_symlink_and_oversize_registry() {
    if isolated("inspection_refuses_symlink_and_oversize_registry") {
        return;
    }
    let project = project();
    let path = project_registry(project.path());
    fs::write(project.path().join("real.toml"), "schema_version = 1\n").unwrap();
    std::os::unix::fs::symlink(
        project.path().canonicalize().unwrap().join("real.toml"),
        &path,
    )
    .unwrap();
    assert!(model_settings::inspect(project.path()).is_err());
    fs::remove_file(&path).unwrap();
    fs::write(&path, " ".repeat(65_537)).unwrap();
    assert!(model_settings::inspect(project.path()).is_err());
}

#[test]
fn effective_user_fallback_and_user_references_are_inspected_without_probing() {
    let project = project();
    let project = project.path().canonicalize().unwrap();
    fs::write(
        project.join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n",
    )
    .unwrap();
    let registry_path = project.join("shared.toml");
    registry(&registry_path, "user-model", true);
    fs::write(
        project.join("user.toml"),
        format!(
            "[agent.registry]\npath = {:?}\n[agent.roles.architect]\nprofile = \"selected\"\nthinking_effort = \"none\"\n",
            registry_path.to_str().unwrap(),
        ),
    ).unwrap();
    let output = command(&project, &["--json", "agent", "role", "list"]);
    let data = json(&output);
    let roles = data["inspection"]["roles"].as_array().unwrap();
    let role = roles.iter().find(|r| r["role"] == "Architect").unwrap();
    assert_eq!(role["effective_profile"], "selected");
    assert_eq!(role["model"], "user-model");
    assert_eq!(role["configuration"]["scope"], "user");
    assert_eq!(role["role_override"], "none");
    let output = command(&project, &["--json", "agent", "show", "selected"]);
    let data = json(&output);
    assert_eq!(data["profile"]["source"]["scope"], "shared");
    assert_eq!(data["profile"]["references"].as_array().unwrap().len(), 1);
    let revision = data["profile"]["source"]["revision"].as_str().unwrap();
    let before = fs::read_to_string(&registry_path).unwrap();
    let output = command(
        &project,
        &[
            "--json",
            "agent",
            "remove",
            "selected",
            "--registry",
            registry_path.to_str().unwrap(),
            "--expected-revision",
            revision,
        ],
    );
    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(&registry_path).unwrap(), before);
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(error["error"].as_str().unwrap().contains("--global"));
}

#[test]
fn registry_precedence_and_shadowed_fallback_are_source_specific() {
    let project = project();
    let project = project.path().canonicalize().unwrap();
    let user = project.join("config/sav/registry.toml");
    let path = project_registry(&project);
    registry(&user, "user-fallback", true);
    registry(&path, "project-winner", true);
    let output = command(&project, &["--json", "agent", "show", "selected"]);
    let data = json(&output);
    assert_eq!(data["profile"]["model"], "project-winner");
    assert_eq!(data["profile"]["source"]["scope"], "project");
    assert_eq!(data["profile"]["fallback"]["model"], "user-fallback");
    let revision = data["profile"]["source"]["revision"].as_str().unwrap();
    let output = command(
        &project,
        &[
            "--json",
            "agent",
            "remove",
            "selected",
            "--registry",
            path.to_str().unwrap(),
            "--expected-revision",
            revision,
        ],
    );
    let data = json(&output);
    assert!(
        data["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("fallback"))
    );
    assert_eq!(
        Registry::load(&user)
            .unwrap()
            .resolve("selected")
            .unwrap()
            .native()
            .unwrap()
            .model,
        "user-fallback",
    );
    let output = command(&project, &["--json", "agent", "show", "selected"]);
    assert_eq!(json(&output)["profile"]["model"], "user-fallback");
}

#[test]
fn shared_remove_requires_explicit_source_and_edits_only_that_file() {
    let project = project();
    let project = project.path().canonicalize().unwrap();
    let shared = project.join("shared.toml");
    registry(&shared, "shared-model", true);
    fs::write(
        project.join("kvist.toml"),
        format!(
            "schema_version = 1\ncomponent_root = \".\"\n[agent.registry]\npath = {:?}\n",
            shared.to_str().unwrap()
        ),
    )
    .unwrap();
    let before = fs::read_to_string(&shared).unwrap();
    let output = command(&project, &["--json", "agent", "remove", "selected"]);
    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(&shared).unwrap(), before);
    let output = command(&project, &["--json", "agent", "show", "selected"]);
    let data = json(&output);
    assert_eq!(data["profile"]["source"]["scope"], "shared");
    let revision = data["profile"]["source"]["revision"].as_str().unwrap();
    let config_before = fs::read_to_string(project.join("kvist.toml")).unwrap();
    let output = command(
        &project,
        &[
            "--json",
            "agent",
            "remove",
            "selected",
            "--registry",
            shared.to_str().unwrap(),
            "--expected-revision",
            revision,
        ],
    );
    json(&output);
    assert!(
        Registry::load(&shared)
            .unwrap()
            .resolve("selected")
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(project.join("kvist.toml")).unwrap(),
        config_before
    );
}

#[test]
fn invalid_shadowed_user_config_fails_and_detail_excludes_external_arguments() {
    let project = project();
    let project = project.path().canonicalize().unwrap();
    let path = project_registry(&project);
    fs::write(&path, "schema_version = 1\n[profiles.selected.backend]\nkind = \"external-agent\"\nprovider = \"custom-script\"\nexecutable = \"/never/probed\"\narguments = [\"--opaque-metadata\", \"argument-that-must-not-be-shown\", \"{prompt}\"]\n").unwrap();
    let output = command(&project, &["agent", "show", "selected"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains("argument-that-must-not-be-shown"));
    assert!(!text.contains("/never/probed"));
    assert!(text.contains("not eligible"));
    fs::write(project.join("user.toml"), "not valid toml").unwrap();
    let output = command(&project, &["--json", "agent", "show", "selected"]);
    assert!(!output.status.success());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["status"], "error");
}

#[test]
fn global_role_edit_is_scoped_and_inheritance_preserves_other_policy() {
    let project = project();
    let project = project.path().canonicalize().unwrap();
    registry(&project_registry(&project), "project-model", true);
    let shared = project.join("shared.toml");
    registry(&shared, "user-model", true);
    fs::write(project.join("user.toml"), format!(
        "[agent.registry]\npath = {:?}\n[agent.roles.architect]\nprofile = \"selected\"\nthinking_effort = \"high\"\ntimeout_seconds = 913\ntoken_limit = 222\n",
        shared.to_str().unwrap()
    )).unwrap();
    let project_before = fs::read_to_string(project.join("kvist.toml")).unwrap();
    let output = command(
        &project,
        &[
            "agent",
            "role",
            "set",
            "architect",
            "selected",
            "--inherit-effort",
            "--global",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let user = fs::read_to_string(project.join("user.toml")).unwrap();
    assert!(!user.contains("thinking_effort"));
    assert!(user.contains("timeout_seconds = 913"));
    assert!(user.contains("token_limit = 222"));
    assert!(user.contains("profile = \"selected\""));
    assert_eq!(
        fs::read_to_string(project.join("kvist.toml")).unwrap(),
        project_before
    );
    let before = user;
    let output = command(
        &project,
        &[
            "--json",
            "agent",
            "role",
            "set",
            "architect",
            "selected",
            "--global",
            "--expected-revision",
            "sha256:stale",
        ],
    );
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(project.join("user.toml")).unwrap(),
        before
    );
}

#[test]
fn initialized_no_agent_project_has_empty_library_and_explicit_unknowns() {
    let project = project();
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n",
    )
    .unwrap();
    let output = command(project.path(), &["--json", "agent", "role", "list"]);
    let data = json(&output);
    assert!(
        data["inspection"]["profiles"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for role in data["inspection"]["roles"].as_array().unwrap() {
        assert!(role["assigned_profile"].is_null());
        assert!(role["model"].is_null());
        assert_eq!(role["enabled"], false);
    }
}

#[test]
fn clearing_shared_role_reference_enables_later_removal_without_policy_changes() {
    let project = project();
    let project = project.path().canonicalize().unwrap();
    let path = project_registry(&project);
    registry(&path, "selected-model", true);
    fs::write(project.join("kvist.toml"),
            "schema_version = 1\ncomponent_root = \".\"\n[agent.registry]\n[agent.roles.developer]\nprofile = \"selected\"\nthinking_effort = \"high\"\ntimeout_seconds = 412\n").unwrap();
    let output = command(&project, &["agent", "role", "clear", "developer"]);
    assert!(output.status.success(), "{output:?}");
    let contents = fs::read_to_string(project.join("kvist.toml")).unwrap();
    assert!(!contents.contains("profile = \"selected\""));
    assert!(contents.contains("thinking_effort = \"high\""));
    assert!(contents.contains("timeout_seconds = 412"));
    let output = command(&project, &["--json", "agent", "show", "selected"]);
    let data = json(&output);
    assert!(data["profile"]["references"].as_array().unwrap().is_empty());
    let output = command(
        &project,
        &[
            "--json",
            "agent",
            "remove",
            "selected",
            "--registry",
            path.to_str().unwrap(),
            "--expected-revision",
            data["profile"]["source"]["revision"].as_str().unwrap(),
        ],
    );
    json(&output);
    assert!(Registry::load(&path).unwrap().resolve("selected").is_err());
}

#[test]
fn profile_selection_pin_refuses_changed_definition_even_when_role_config_is_unchanged() {
    let project = project();
    let project = project.path().canonicalize().unwrap();
    let path = project_registry(&project);
    registry(&path, "first", true);
    let output = command(&project, &["--json", "agent", "show", "selected"]);
    let data = json(&output);
    let pin = data["profile"]["source"]["revision"].as_str().unwrap();
    let before = fs::read_to_string(project.join("kvist.toml")).unwrap();
    registry(&path, "changed", true);
    let output = command(
        &project,
        &[
            "--json",
            "agent",
            "role",
            "set",
            "developer",
            "selected",
            "--profile-source",
            path.to_str().unwrap(),
            "--profile-revision",
            pin,
        ],
    );
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(project.join("kvist.toml")).unwrap(),
        before
    );
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(error["error"].as_str().unwrap().contains("stale"));
}

#[test]
fn enable_disable_preserve_capabilities_and_busy_lock_never_changes_source() {
    if isolated("enable_disable_preserve_capabilities_and_busy_lock_never_changes_source") {
        return;
    }
    let project = project();
    let path = project_registry(project.path());
    registry(&path, "selected-model", false);
    let selected = model_settings::inspect(project.path())
        .unwrap()
        .profiles
        .into_iter()
        .find(|p| p.name == "selected")
        .unwrap();
    let lock = path.with_file_name(".providers.toml.kvist-settings.lock");
    fs::write(&lock, "another writer").unwrap();
    let before = fs::read_to_string(&path).unwrap();
    assert!(
        model_settings::mutate_registry(
            project.path(),
            &path,
            "selected",
            &selected.source.revision,
            RegistryChange::Enable
        )
        .is_err()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert_eq!(fs::read_to_string(&lock).unwrap(), "another writer");
    fs::remove_file(lock).unwrap();
    let report = model_settings::mutate_registry(
        project.path(),
        &path,
        "selected",
        &selected.source.revision,
        RegistryChange::Enable,
    )
    .unwrap();
    assert_ne!(report.revision, report.previous_revision);
    let profile = Registry::load(&path).unwrap().resolve("selected").unwrap();
    assert_eq!(profile.profile.advertised, ["tools"]);
    assert!(profile.profile.tested.is_empty());
    assert!(profile.profile.policy_enabled.is_empty());
    model_settings::mutate_registry(
        project.path(),
        &path,
        "selected",
        &report.revision,
        RegistryChange::Disable,
    )
    .unwrap();
    assert!(Registry::load(&path).unwrap().resolve("selected").is_err());
}

#[test]
fn inspection_refuses_ancestor_symlinks_nonregular_sources_and_invalid_metadata() {
    if isolated("inspection_refuses_ancestor_symlinks_nonregular_sources_and_invalid_metadata") {
        return;
    }
    let project = project();
    let project = project.path().canonicalize().unwrap();
    let path = project_registry(&project);
    nix::unistd::mkfifo(&path, nix::sys::stat::Mode::S_IRUSR).unwrap();
    assert!(model_settings::inspect(&project).is_err());
    fs::remove_file(&path).unwrap();
    fs::remove_dir(project.join(".kvist")).unwrap();
    fs::create_dir(project.join("real")).unwrap();
    std::os::unix::fs::symlink(project.join("real"), project.join(".kvist")).unwrap();
    registry(&project.join("real/providers.toml"), "selected-model", true);
    assert!(model_settings::inspect(&project).is_err());
    fs::remove_file(project.join(".kvist")).unwrap();
    fs::write(project.join("kvist.toml"),
            "schema_version = 1\ncomponent_root = \".\"\n[agent.profiles.invalid]\nprovider = \"custom\"\ndefault_thinking_effort = \"impossible\"\n").unwrap();
    assert!(model_settings::inspect(&project).is_err());
}

#[test]
fn first_project_role_edit_preserves_effective_user_bindings_and_resource_policies() {
    let project = project();
    let project = project.path().canonicalize().unwrap();
    fs::write(
        project.join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n",
    )
    .unwrap();
    let shared = project.join("shared.toml");
    registry(&shared, "user-selected-model", true);
    fs::write(project.join("user.toml"), format!(
            "[agent.registry]\npath = {:?}\n\
             [agent.roles.developer]\nprofile = \"selected\"\nthinking_effort = \"high\"\ntimeout_seconds = 913\ntoken_limit = 222\n\
             [agent.roles.architect]\nprofile = \"unrelated\"\nthinking_effort = \"low\"\ntimeout_seconds = 1441\nmax_output_bytes = 9999\n\
             [agent.roles.security-reviewer]\nprofile = \"selected\"\nthinking_effort = \"medium\"\ntimeout_seconds = 813\n",
            shared.to_str().unwrap()
        )).unwrap();
    let user_before = fs::read_to_string(project.join("user.toml")).unwrap();
    let output = command(
        &project,
        &[
            "agent",
            "role",
            "set",
            "developer",
            "selected",
            "--inherit-effort",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("preserving all effective"));
    let contents = fs::read_to_string(project.join("kvist.toml")).unwrap();
    let document: toml::Value = toml::from_str(&contents).unwrap();
    let roles = &document["agent"]["roles"];
    assert_eq!(
        roles["developer"]["timeout_seconds"].as_integer(),
        Some(913)
    );
    assert_eq!(roles["developer"]["token_limit"].as_integer(), Some(222));
    assert!(roles["developer"].get("thinking_effort").is_none());
    assert_eq!(roles["architect"]["profile"].as_str(), Some("unrelated"));
    assert_eq!(roles["architect"]["thinking_effort"].as_str(), Some("low"));
    assert_eq!(
        roles["architect"]["timeout_seconds"].as_integer(),
        Some(1441)
    );
    assert_eq!(
        roles["architect"]["max_output_bytes"].as_integer(),
        Some(9999)
    );
    assert_eq!(
        roles["security-reviewer"]["profile"].as_str(),
        Some("selected")
    );
    assert_eq!(
        roles["security-reviewer"]["thinking_effort"].as_str(),
        Some("medium")
    );
    assert_eq!(
        roles["security-reviewer"]["timeout_seconds"].as_integer(),
        Some(813)
    );
    assert_eq!(
        fs::read_to_string(project.join("user.toml")).unwrap(),
        user_before
    );
    let output = command(&project, &["--json", "agent", "role", "list"]);
    let data = json(&output);
    assert_eq!(
        data["inspection"]["roles"][0]["model"],
        "user-selected-model"
    );
}

#[test]
fn first_project_fork_never_copies_sensitive_redaction_values() {
    let project = project();
    let project = project.path().canonicalize().unwrap();
    fs::write(
        project.join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \".\"\n",
    )
    .unwrap();
    let shared = project.join("shared.toml");
    registry(&shared, "selected-model", true);
    fs::write(project.join("user.toml"), format!(
            "[agent.registry]\npath = {:?}\n[agent.roles.developer]\nprofile = \"selected\"\nredaction_values = [\"private-value-must-not-be-copied\"]\n", shared.to_str().unwrap()
        )).unwrap();
    let before = fs::read_to_string(project.join("kvist.toml")).unwrap();
    let output = command(
        &project,
        &["--json", "agent", "role", "set", "developer", "selected"],
    );
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private-value-must-not-be-copied"));
    assert_eq!(
        fs::read_to_string(project.join("kvist.toml")).unwrap(),
        before
    );
}

#[test]
fn scoped_user_inspection_resolves_its_shared_registry_instead_of_project_selection() {
    const KEY: &str = "KVIST_MODEL_SETTINGS_SCOPE_TEST_PROJECT";
    if let Some(project) = std::env::var_os(KEY) {
        let project = PathBuf::from(project);
        let project_scope = model_settings::inspect_scope(&project, false).unwrap();
        let user_scope = model_settings::inspect_scope(&project, true).unwrap();
        let selected = |snapshot: model_settings::SettingsInspection| {
            snapshot
                .profiles
                .into_iter()
                .find(|p| p.name == "selected" && p.effective)
                .unwrap()
        };
        assert_eq!(
            selected(project_scope).model.as_deref(),
            Some("project-model")
        );
        let selected = selected(user_scope);
        assert_eq!(selected.model.as_deref(), Some("shared-user-model"));
        assert_eq!(selected.source.scope, Scope::Shared);
        return;
    }
    let project = project();
    let project = project.path().canonicalize().unwrap();
    registry(&project_registry(&project), "project-model", true);
    let user_registry = project.join("shared-user.toml");
    registry(&user_registry, "shared-user-model", true);
    fs::write(
        project.join("user.toml"),
        format!(
            "[agent.registry]\npath = {:?}\n[agent.roles.architect]\nprofile = \"selected\"\n",
            user_registry.to_str().unwrap()
        ),
    )
    .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "scoped_user_inspection_resolves_its_shared_registry_instead_of_project_selection",
            "--nocapture",
        ])
        .env(KEY, &project)
        .env("KVIST_CONFIG_PATH", project.join("user.toml"))
        .env("HOME", &project)
        .env("XDG_CONFIG_HOME", project.join("config"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn pinned_local_removal_refuses_stale_and_user_references_then_removes_only_selected_definition() {
    let project = project();
    let project = project.path().canonicalize().unwrap();
    let path = project.join("kvist.toml");
    fs::write(&path,
        "schema_version = 1\ncomponent_root = \".\"\n\
         # keep provider comment\n[agent.providers.custom]\ntype = \"custom\"\ncommand_template = \"/never/probed {prompt}\"\n\
         [agent.profiles.selected]\nprovider = \"custom\"\nmodel = \"project-model\"\n\
         # keep unrelated comment\n[agent.profiles.unrelated]\nprovider = \"custom\"\nmodel = \"other\"\n").unwrap();
    fs::write(project.join("user.toml"),
        "[agent.providers.custom]\ntype = \"custom\"\ncommand_template = \"/never/probed {prompt}\"\n\
         [agent.profiles.selected]\nprovider = \"custom\"\nmodel = \"user-fallback\"\n\
         [agent.roles.architect]\nprofile = \"selected\"\n").unwrap();
    let before = fs::read_to_string(&path).unwrap();
    let output = command(&project, &["--json", "agent", "show", "selected"]);
    let data = json(&output);
    assert_eq!(data["profile"]["fallback"]["model"], "user-fallback");
    let revision = data["profile"]["source"]["revision"].as_str().unwrap();
    for pin in ["sha256:stale", revision] {
        let output = command(
            &project,
            &[
                "--json",
                "agent",
                "remove",
                "selected",
                "--expected-revision",
                pin,
            ],
        );
        assert!(!output.status.success(), "{output:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    let output = command(
        &project,
        &["agent", "role", "clear", "architect", "--global"],
    );
    assert!(output.status.success(), "{output:?}");
    let user_before = fs::read_to_string(project.join("user.toml")).unwrap();
    let output = command(
        &project,
        &[
            "--json",
            "agent",
            "remove",
            "selected",
            "--expected-revision",
            revision,
        ],
    );
    let data = json(&output);
    assert!(data["message"].as_str().unwrap().contains("fallback"));
    let after = fs::read_to_string(&path).unwrap();
    assert!(!after.contains("[agent.profiles.selected]"));
    assert!(after.contains("# keep provider comment"));
    assert!(after.contains("# keep unrelated comment"));
    assert!(after.contains("[agent.profiles.unrelated]"));
    assert_eq!(
        fs::read_to_string(project.join("user.toml")).unwrap(),
        user_before
    );
}

#[test]
fn changing_models_refuses_retained_unsupported_effort_without_writing() {
    let project = project();
    let path = project.path().join("kvist.toml");
    fs::write(&path,
        "schema_version = 1\ncomponent_root = \".\"\n\
         [agent.providers.custom]\ntype = \"custom\"\ncommand_template = \"/never/probed {prompt} {reasoning_effort}\"\n\
         [agent.profiles.thinker]\nprovider = \"custom\"\nsupports_thinking = true\n\
         [agent.profiles.plain]\nprovider = \"custom\"\nsupports_thinking = false\n\
         [agent.roles.developer]\nprofile = \"thinker\"\nthinking_effort = \"high\"\ntoken_limit = 222\n").unwrap();
    let before = fs::read_to_string(&path).unwrap();
    let output = command(
        project.path(),
        &["--json", "agent", "role", "set", "developer", "plain"],
    );
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(error["error"].as_str().unwrap().contains("unsupported"));
    let output = command(
        project.path(),
        &[
            "agent",
            "role",
            "set",
            "developer",
            "plain",
            "--inherit-effort",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("profile = \"plain\""));
    assert!(after.contains("token_limit = 222"));
    assert!(!after.contains("thinking_effort"));
}

#[test]
fn clearing_roles_preserves_canonical_role_named_and_unrelated_model_definitions() {
    let project = project();
    let path = project.path().join("kvist.toml");
    let contents = "schema_version = 1\ncomponent_root = \".\"\n\
        [agent.providers.custom]\ntype = \"custom\"\ncommand_template = \"/never/probed {prompt}\"\n\
        [agent.profiles.developer]\nprovider = \"custom\"\nmodel = \"configured-developer-model\"\n\
        [agent.profiles.library]\nprovider = \"custom\"\nmodel = \"configured-library-model\"\n\
        [agent.roles.developer]\nprofile = \"developer\"\nthinking_effort = \"high\"\n\
        [agent.roles.architect]\nprofile = \"library\"\n";
    let before: toml::Value = toml::from_str(contents).unwrap();
    for selector in ["developer", "--all"] {
        fs::write(&path, contents).unwrap();
        let output = command(project.path(), &["agent", "role", "clear", selector]);
        assert!(output.status.success(), "{output:?}");
        let after: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after["agent"]["profiles"], before["agent"]["profiles"]);
        assert!(
            after["agent"]["roles"]["developer"]
                .get("profile")
                .is_none()
        );
        assert_eq!(
            after["agent"]["roles"]["developer"]["thinking_effort"].as_str(),
            Some("high")
        );
        if selector == "--all" {
            assert!(
                after["agent"]["roles"]["architect"]
                    .get("profile")
                    .is_none()
            );
        } else {
            assert_eq!(
                after["agent"]["roles"]["architect"],
                before["agent"]["roles"]["architect"]
            );
        }
    }
}

#[test]
fn clearing_without_a_binding_never_reinterprets_canonical_profile_model_as_an_assignment() {
    let project = project();
    let path = project.path().join("kvist.toml");
    let contents = "schema_version = 1\ncomponent_root = \".\"\n\
        [agent.profiles.developer]\nmodel = \"canonical-model\"\ncommand = \"/never/probed {prompt}\"\n";
    fs::write(&path, contents).unwrap();
    let output = command(project.path(), &["agent", "role", "clear", "developer"]);
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), contents);
}

#[test]
fn local_builtin_fallback_is_disclosed_without_user_configuration_only_when_defaults_resurface() {
    let project = project();
    let path = project.path().join("kvist.toml");
    let contents = "schema_version = 1\ncomponent_root = \".\"\n\
        [agent.profiles.claude-default]\nprovider = \"claude\"\nmodel = \"overridden-model\"\n";
    assert!(!project.path().join("user.toml").exists());
    fs::write(&path, contents).unwrap();
    let output = command(
        project.path(),
        &["--json", "agent", "show", "claude-default"],
    );
    let data = json(&output);
    assert_eq!(data["profile"]["fallback"]["source"]["scope"], "built-in");
    assert_eq!(data["profile"]["fallback"]["model"], "claude-default");
    assert!(
        data["output"]
            .as_str()
            .unwrap()
            .contains("Removal exposes fallback")
    );
    fs::write(
        &path,
        format!(
            "{contents}\n[agent.profiles.unrelated]\nprovider = \"claude\"\nmodel = \"other\"\n"
        ),
    )
    .unwrap();
    let output = command(
        project.path(),
        &["--json", "agent", "show", "claude-default"],
    );
    assert!(json(&output)["profile"]["fallback"].is_null());
}

#[test]
fn legacy_role_clear_requires_unambiguous_role_assignment_fields() {
    let project = project();
    let path = project.path().join("kvist.toml");
    let contents = "schema_version = 1\ncomponent_root = \".\"\n\
        [agent.profiles.developer]\nmodel = \"selected\"\n\
        models = [{ name = \"selected\", command = \"/never/probed {prompt}\" }]\n";
    fs::write(&path, contents).unwrap();
    let output = command(project.path(), &["agent", "role", "clear", "developer"]);
    assert!(output.status.success(), "{output:?}");
    let after: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(
        after["agent"]["profiles"]["developer"]
            .get("model")
            .is_none()
    );
    assert!(
        after["agent"]["profiles"]["developer"]
            .get("models")
            .is_some()
    );
    let ambiguous = format!("{contents}\nprovider = \"custom\"\n");
    fs::write(&path, &ambiguous).unwrap();
    let output = command(project.path(), &["agent", "role", "clear", "--all"]);
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), ambiguous);
}
