use std::fs;

use agent_runtime::{ModelProfile, load_profiles, upsert_profile};
use tempfile::TempDir;

fn profile(name: &str, command: &str) -> ModelProfile {
    ModelProfile {
        name: name.to_owned(),
        provider: "custom-script".to_owned(),
        command: command.to_owned(),
    }
}

#[test]
fn creates_and_loads_a_named_profile() {
    let directory = TempDir::new().expect("configuration directory");
    let path = directory.path().join("config.toml");

    upsert_profile(&path, &profile("local", "/bin/echo '{prompt}'")).expect("persist profile");

    let profiles = load_profiles(&path).expect("load profiles");
    assert_eq!(profiles, [profile("local", "/bin/echo '{prompt}'")]);
}

#[test]
fn update_preserves_comments_unrelated_values_and_profiles() {
    let directory = TempDir::new().expect("configuration directory");
    let path = directory.path().join("config.toml");
    fs::write(
        &path,
        r#"# Keep this comment.
schema_version = 1
custom = "preserved"

[[profiles]]
name = "existing"
provider = "ollama"
command = "ollama run old '{prompt}'"
"#,
    )
    .expect("write configuration");

    upsert_profile(&path, &profile("new", "/bin/echo '{prompt}'")).expect("append profile");
    upsert_profile(&path, &profile("existing", "ollama run new '{prompt}'"))
        .expect("replace profile");

    let contents = fs::read_to_string(&path).expect("read configuration");
    assert!(contents.contains("# Keep this comment."));
    assert!(contents.contains("custom = \"preserved\""));
    assert_eq!(contents.matches("name = \"existing\"").count(), 1);

    let profiles = load_profiles(&path).expect("load profiles");
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].command, "ollama run new '{prompt}'");
    assert_eq!(profiles[1].name, "new");
}

#[test]
fn invalid_existing_configuration_remains_unchanged() {
    let directory = TempDir::new().expect("configuration directory");
    let path = directory.path().join("config.toml");
    let original = "schema_version = 2\n";
    fs::write(&path, original).expect("write invalid configuration");

    let error = upsert_profile(&path, &profile("local", "/bin/echo '{prompt}'"))
        .expect_err("unsupported schema must fail");

    assert!(error.to_string().contains("schema_version"));
    assert_eq!(
        fs::read_to_string(path).expect("read configuration"),
        original
    );
}

#[test]
fn rejects_invalid_profile_names_before_writing() {
    let directory = TempDir::new().expect("configuration directory");
    let path = directory.path().join("config.toml");

    let error = upsert_profile(&path, &profile("contains spaces", "agent '{prompt}'"))
        .expect_err("invalid name must fail");

    assert!(error.to_string().contains("profile name"));
    assert!(!path.exists());
}
