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
