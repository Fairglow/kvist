use std::fs;

use skott::config::Config;

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
