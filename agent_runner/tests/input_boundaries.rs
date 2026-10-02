use agent_runner::{Config, ToolPolicy, ToolProfile, ToolRegistry};
use std::os::unix::fs::PermissionsExt;

#[test]
fn pure_registry_does_not_claim_unprobed_language_tools() {
    assert_eq!(
        ToolRegistry::new(ToolPolicy::minimum()).profiles(),
        ["generic"]
    );
}

#[test]
fn nonexecutable_candidates_do_not_enable_a_language_profile() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("python3");
    std::fs::write(&path, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!agent_runner::toolchain::language_available(
        ToolProfile::Python,
        &[root.path().to_owned()],
        &[root.path().to_str().unwrap()]
    ));
}

#[test]
fn malformed_configuration_diagnostics_do_not_echo_secret_values() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    std::fs::write(&path, "schema_version='secret-sentinel-value'\n").unwrap();
    let error = Config::load(&path).unwrap_err();
    assert!(!error.describe().contains("secret-sentinel-value"));
    assert!(error.describe().contains("line"));
}
