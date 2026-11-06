use std::fs;

use skott::config::Config;

fn standalone_command(directory: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_skott"));
    command
        .current_dir(directory)
        .env("HOME", directory)
        .env("XDG_CONFIG_HOME", directory.join("user-config"))
        .env("XDG_CONFIG_DIRS", directory.join("system-config"))
        .stdin(std::process::Stdio::null());
    command
}

#[test]
fn missing_configuration_explains_creation_setup_and_profile_reuse() {
    let directory = tempfile::tempdir().unwrap();
    for args in [vec![], vec!["--config", "absent.toml"]] {
        let output = standalone_command(directory.path())
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        for hint in ["--init-config", "--setup", "--registry", "--model"] {
            assert!(error.contains(hint), "missing {hint}: {error}");
        }
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}

#[test]
fn initialization_creates_private_unpinned_starter_without_running_setup() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    fs::create_dir(&project).unwrap();
    let output = standalone_command(directory.path())
        .arg("--init-config")
        .arg("--cwd")
        .arg(&project)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let path = project.join("skott.toml");
    let text = fs::read_to_string(&path).unwrap();
    let document: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(document["schema_version"].as_integer(), Some(1));
    for absent in ["working_directory", "sandbox", "tool_policy", "models"] {
        assert!(document.get(absent).is_none(), "{text}");
    }
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let message = String::from_utf8_lossy(&output.stdout);
    assert!(message.contains("--setup"), "{message}");
    assert!(message.contains("default_provider"), "{message}");
    assert_eq!(fs::read_dir(&project).unwrap().count(), 1);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn initialization_refuses_existing_paths_and_incompatible_modes() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("existing.toml"), "original").unwrap();
    fs::create_dir(directory.path().join("directory.toml")).unwrap();
    symlink("existing.toml", directory.path().join("link.toml")).unwrap();
    symlink("missing.toml", directory.path().join("dangling.toml")).unwrap();
    for path in [
        "existing.toml",
        "directory.toml",
        "link.toml",
        "dangling.toml",
        "missing/parent.toml",
    ] {
        let output = standalone_command(directory.path())
            .args(["--init-config", "--config", path])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{path}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(path),
            "{output:?}"
        );
    }
    assert_eq!(
        fs::read_to_string(directory.path().join("existing.toml")).unwrap(),
        "original"
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 4);
    for args in [
        vec!["--init-config", "--setup"],
        vec!["--init-config", "--list-models"],
        vec!["--init-config", "--headless", "task"],
        vec!["--init-config", "task"],
        vec!["--init-config", "--attach", "journal"],
    ] {
        let output = standalone_command(directory.path())
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
    }
    assert!(!directory.path().join("skott.toml").exists());
}

#[test]
fn initialization_reuses_selected_profile_from_another_project_without_copying_facts() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original \"project\"");
    let project = directory.path().join("project");
    fs::create_dir(&original).unwrap();
    fs::create_dir(&project).unwrap();
    let registry = original.join("providers.toml");
    let facts = "schema_version = 1\n[profiles.coder.backend]\nkind = \"native\"\nprovider = \"ollama\"\nmodel = \"shared-model\"\ncontext_limit = 4096\n";
    fs::write(&registry, facts).unwrap();
    let output = standalone_command(&project)
        .args(["--init-config", "--model", "coder", "--registry"])
        .arg(&registry)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let config = project.join("skott.toml");
    let text = fs::read_to_string(&config).unwrap();
    assert!(!text.contains("shared-model"), "{text}");
    let document: toml::Value = toml::from_str(&text).unwrap();
    assert!(document.get("working_directory").is_none());
    assert_eq!(document["default_provider"].as_str(), Some("ollama"));
    assert_eq!(
        document["registry"]["profiles"].as_array().unwrap().len(),
        1
    );
    assert_eq!(document["registry"]["profiles"][0].as_str(), Some("coder"));
    assert_eq!(
        document["registry"]["default_model"].as_str(),
        Some("coder")
    );
    assert_eq!(document["registry"]["path"].as_str(), registry.to_str());
    let loaded = Config::load(&config).unwrap();
    assert_eq!(loaded.default_model().unwrap().id, "coder");
    assert_eq!(loaded.default_model().unwrap().model, "shared-model");
    assert_eq!(fs::read_to_string(&registry).unwrap(), facts);
    fs::write(&registry, facts.replace("shared-model", "updated-model")).unwrap();
    let listing = standalone_command(&project)
        .arg("--list-models")
        .output()
        .unwrap();
    assert!(listing.status.success(), "{listing:?}");
    assert!(String::from_utf8_lossy(&listing.stdout).contains("updated-model"));
    assert_eq!(fs::read_dir(&project).unwrap().count(), 1);
}

#[test]
fn initialization_can_reference_a_user_profile_and_create_a_user_wide_config() {
    let directory = tempfile::tempdir().unwrap();
    let user_config = directory.path().join("user-config");
    fs::create_dir_all(user_config.join("sav")).unwrap();
    fs::create_dir(user_config.join("skott")).unwrap();
    let registry = user_config.join("sav/registry.toml");
    fs::write(&registry, "schema_version = 1\n[profiles.coder.backend]\nkind = \"native\"\nprovider = \"ollama\"\nmodel = \"user-model\"\n").unwrap();
    let config = user_config.join("skott/config.toml");
    let output = standalone_command(directory.path())
        .args(["--init-config", "--model", "coder", "--config"])
        .arg(&config)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let other_project = directory.path().join("other-project");
    fs::create_dir(&other_project).unwrap();
    let output = standalone_command(directory.path())
        .current_dir(&other_project)
        .arg("--list-models")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("user-model"));
    assert!(!directory.path().join("skott.toml").exists());
    assert_eq!(fs::read_dir(&other_project).unwrap().count(), 0);
    let text = fs::read_to_string(config).unwrap();
    assert!(!text.contains("working_directory"), "{text}");
}

#[test]
fn initialization_rejects_bad_shared_selections_without_creating_configuration() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("providers.toml"),
        r#"schema_version = 1
[profiles.disabled]
enabled = false
[profiles.disabled.backend]
kind = "native"
provider = "ollama"
model = "disabled-model"
[profiles.external.backend]
kind = "external-agent"
provider = "copilot"
executable = "/never/executed"
"#,
    )
    .unwrap();
    for name in [None, Some("unknown"), Some("disabled"), Some("external")] {
        let mut command = standalone_command(directory.path());
        command.args(["--init-config", "--registry", "providers.toml"]);
        if let Some(name) = name {
            command.args(["--model", name]);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success(), "{name:?}: {output:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        let expected = match name {
            None => "--model",
            Some("unknown") => "not found",
            Some("disabled") => "disabled",
            _ => "external-agent",
        };
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("unexpected argument"), "{error}");
        assert!(!directory.path().join("skott.toml").exists());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn configuration_references_shared_profiles_without_copying_facts() {
    let directory = tempfile::tempdir_in(".").unwrap();
    let registry = directory.path().join("providers.toml");
    fs::write(&registry, "schema_version = 1\n[profiles.coder.backend]\nkind = \"native\"\nprovider = \"ollama\"\nmodel = \"shared-model\"\ncontext_limit = 4096\n").unwrap();
    let config = directory.path().join("skott.toml");
    fs::write(&config, "schema_version = 1\ndefault_provider = \"ollama\"\n[registry]\npath = \"providers.toml\"\nprofiles = [\"coder\"]\n").unwrap();
    let loaded = Config::load(&config).unwrap();
    assert_eq!(loaded.model("coder").unwrap().model, "shared-model");
    assert_eq!(loaded.model("coder").unwrap().context_limit, Some(4096));
    fs::write(
        &registry,
        fs::read_to_string(&registry)
            .unwrap()
            .replace("shared-model", "changed"),
    )
    .unwrap();
    assert_eq!(loaded.model("coder").unwrap().model, "shared-model");
    assert_eq!(
        Config::load(&config).unwrap().model("coder").unwrap().model,
        "changed"
    );
}

#[test]
fn external_profiles_cannot_be_used_as_native_models() {
    let directory = tempfile::tempdir_in(".").unwrap();
    fs::write(directory.path().join("providers.toml"), "schema_version = 1\n[profiles.agent.backend]\nkind = \"external-agent\"\nprovider = \"copilot\"\nexecutable = \"/not/executed\"\n").unwrap();
    let config = directory.path().join("skott.toml");
    fs::write(&config, "schema_version = 1\ndefault_provider = \"ollama\"\n[registry]\npath = \"providers.toml\"\nprofiles = [\"agent\"]\n").unwrap();
    let loaded = Config::load(&config).unwrap();
    assert!(loaded.model("agent").is_none());
    assert!(loaded.shared_profile("agent").unwrap().native().is_err());
}

#[test]
fn setup_precedes_skott_configuration_discovery_and_import_is_retired() {
    let directory = tempfile::tempdir_in(".").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_skott"))
        .args(["--setup", "--registry", "providers.toml"])
        .current_dir(directory.path())
        .stdin(std::process::Stdio::null())
        .env("HOME", directory.path())
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("Shared provider setup"));
    let retired = std::process::Command::new(env!("CARGO_BIN_EXE_skott"))
        .arg("--import-kvist")
        .output()
        .unwrap();
    assert!(!retired.status.success());
    assert!(String::from_utf8_lossy(&retired.stderr).contains("unexpected argument"));
}

fn default_binding_fixture(
    default: &str,
    profiles: Option<&str>,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir_in(".").unwrap();
    fs::write(
        directory.path().join("providers.toml"),
        r#"schema_version = 1
[profiles.first.backend]
kind = "native"
provider = "ollama"
model = "native-first"
[profiles.fallback.backend]
kind = "native"
provider = "ollama"
model = "native-fallback"
context_limit = 4096
response_reserve = 512
[profiles.other.backend]
kind = "native"
provider = "llama-server"
model = "native-other"
[profiles.external.backend]
kind = "external-agent"
provider = "custom-script"
executable = "/not/executed"
[profiles.disabled]
enabled = false
[profiles.disabled.backend]
kind = "native"
provider = "ollama"
model = "native-disabled"
"#,
    )
    .unwrap();
    let config = directory.path().join("skott.toml");
    fs::write(
        &config,
        format!(
            "schema_version = 1\ndefault_provider = \"ollama\"\n\
             [registry]\npath = \"providers.toml\"\ndefault_model = \"{default}\"\n{}\n",
            profiles.unwrap_or("")
        ),
    )
    .unwrap();
    (directory, config)
}

#[test]
fn registry_default_binds_one_native_profile_without_copying_or_mutating_facts() {
    for profiles in [None, Some("profiles = [\"first\", \"fallback\"]")] {
        let (directory, config) = default_binding_fixture("fallback", profiles);
        let registry = directory.path().join("providers.toml");
        let registry_before = fs::read(&registry).unwrap();
        let config_before = fs::read(&config).unwrap();
        let loaded = Config::load(&config).unwrap();
        assert_eq!(loaded.default_model().unwrap().id, "fallback");
        assert_eq!(
            loaded
                .models
                .iter()
                .filter(|model| model.is_default)
                .count(),
            1
        );
        assert!(!loaded.model("first").unwrap().is_default);
        let shared = loaded.shared_profile("fallback").unwrap().native().unwrap();
        assert_eq!(&loaded.model("fallback").unwrap().native(), shared);
        assert_eq!(shared.model, "native-fallback");
        assert_eq!(shared.context_limit, Some(4096));
        assert_eq!(fs::read(&registry).unwrap(), registry_before);
        assert_eq!(fs::read(&config).unwrap(), config_before);
    }
}

#[test]
fn registry_default_rejects_unknown_filtered_external_disabled_and_wrong_provider() {
    for (default, profiles) in [
        ("unknown", None),
        ("fallback", Some("profiles = [\"first\"]")),
        ("external", None),
        ("disabled", None),
        ("other", None),
        ("", None),
    ] {
        let (_directory, config) = default_binding_fixture(default, profiles);
        let error = Config::load(&config).unwrap_err().to_string();
        assert!(
            error.contains("registry.default_model"),
            "{default}: {error}"
        );
        assert!(error.contains("retained"), "{default}: {error}");
    }
}

#[test]
fn registry_default_cannot_bind_a_local_model_instead_of_a_retained_shared_profile() {
    for default in ["local", "fallback"] {
        let (_directory, config) = default_binding_fixture(default, Some("profiles = [\"first\"]"));
        let contents = fs::read_to_string(&config).unwrap()
            + &format!(
                "\n[[models]]\nid = \"{default}\"\nprovider = \"ollama\"\nmodel = \"local-facts\"\n"
            );
        fs::write(&config, contents).unwrap();
        let error = Config::load(&config).unwrap_err().to_string();
        assert!(error.contains("registry.default_model"), "{error}");
        assert!(error.contains("retained"), "{error}");
    }
}

#[test]
fn registry_default_preserves_existing_duplicate_default_validation() {
    let (_directory, config) = default_binding_fixture("fallback", None);
    let contents = fs::read_to_string(&config).unwrap()
        + "\n[[models]]\nid = \"local\"\nprovider = \"ollama\"\n\
           model = \"local-facts\"\nis_default = true\n";
    fs::write(&config, contents).unwrap();
    let error = Config::load(&config).unwrap_err().to_string();
    assert!(error.contains("duplicate is_default model"), "{error}");
}

#[test]
fn registry_default_remains_below_explicit_and_loaded_model_selection() {
    use skott::config::{Select, select_active_model};
    use std::collections::BTreeMap;

    let (_directory, config) = default_binding_fixture("fallback", None);
    let loaded = Config::load(&config).unwrap();
    assert!(matches!(
        select_active_model(&loaded, None, &BTreeMap::new()),
        Select::Default { model_id, .. } if model_id == "fallback"
    ));
    let first = loaded.model("first").unwrap();
    let active = BTreeMap::from([(first.base_url.clone(), first.model.clone())]);
    assert!(matches!(
        select_active_model(&loaded, None, &active),
        Select::Active { model_id, .. } if model_id == "first"
    ));
    assert_eq!(
        select_active_model(&loaded, Some("other"), &active),
        Select::Explicit("other".into())
    );
}

#[test]
fn list_models_renders_each_retained_id_once_with_external_execution_boundary() {
    let (directory, config) = default_binding_fixture("fallback", None);
    let root = directory.path().canonicalize().unwrap();
    let registry = root.join("providers.toml");
    let original = fs::read(&registry).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_skott"))
        .arg("--config")
        .arg(config.canonicalize().unwrap())
        .arg("--list-models")
        .current_dir(&root)
        .env("HOME", &root)
        .env("XDG_CONFIG_HOME", root.join("user-config"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let listing = String::from_utf8(output.stdout).unwrap();
    for id in ["first", "fallback", "other", "external"] {
        assert_eq!(
            listing
                .lines()
                .filter(|line| {
                    line.split_whitespace()
                        .next()
                        .is_some_and(|value| value.trim_start_matches('*') == id)
                })
                .count(),
            1,
            "{id}: {listing}"
        );
    }
    let external = listing
        .lines()
        .find(|line| line.split_whitespace().next() == Some("external"))
        .unwrap();
    assert!(
        external.contains("explicit host execution only"),
        "{listing}"
    );
    assert!(external.contains("/not/executed"), "{listing}");
    assert!(!listing.contains("disabled"), "{listing}");
    assert_eq!(fs::read(registry).unwrap(), original);
}
