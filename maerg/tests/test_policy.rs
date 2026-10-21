//! Test-policy placeholder guard.
//!
//! The project `kvist.toml` test policy used to contain `/usr/bin/echo
//! 'Verification passed'` placeholders for every component, so a recorded
//! verification success never executed any component test. While the local
//! project policy contains such placeholders, recorded verification evidence
//! is not evidence at all, so this test fails the suite until real
//! per-component verification commands are committed.
//!
//! `kvist.toml` is project-local (not committed); when it is absent there is
//! no policy to inspect and the test passes.

use std::path::PathBuf;

use kvist::config;

/// Placeholder echo binaries that can never execute component tests.
const PLACEHOLDER_PROGRAMS: [&str; 3] = ["echo", "/bin/echo", "/usr/bin/echo"];

/// The manifest directory of the crate under test for the current
/// invocation.
///
/// Cargo sets `CARGO_MANIFEST_DIR` in the test process environment at run
/// time, so it stays correct even when a previously built test binary is
/// reused after the repository has moved. The compile-time `env!` value is
/// only a fallback for running the test binary outside of cargo.
fn manifest_dir() -> PathBuf {
    std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

fn project_root() -> PathBuf {
    manifest_dir()
        .parent()
        .expect("workspace root exists")
        .to_path_buf()
}

#[test]
fn project_test_policy_has_no_placeholder_commands() {
    let root = project_root();
    let config_path = root.join("kvist.toml");
    if !config_path.exists() {
        // A fresh checkout without the local project config has no policy to
        // inspect; placeholders cannot be present.
        return;
    }

    let project = config::load(&root).expect("project configuration loads");
    let policy = project
        .test_policy
        .as_ref()
        .expect("project configuration defines a test policy");

    for entry in &policy.commands {
        let program = entry.command.split_whitespace().next().unwrap_or("");
        assert!(
            !PLACEHOLDER_PROGRAMS.contains(&program),
            "component `{}` has a placeholder verification command that never \
             executes tests: {}",
            entry.component,
            entry.command
        );
        assert!(
            !entry.command.contains("'Verification passed'"),
            "component `{}` carries the placeholder verification banner: {}",
            entry.component,
            entry.command
        );
        assert!(
            !entry.command.trim().is_empty(),
            "component `{}` has an empty verification command",
            entry.component
        );
    }
}

#[test]
fn project_test_policy_digest_covers_command_set() {
    let root = project_root();
    if !root.join("kvist.toml").exists() {
        return;
    }

    let project = config::load(&root).expect("project configuration loads");
    let policy = project
        .test_policy
        .as_ref()
        .expect("project configuration defines a test policy");

    // The recorded approval digest must be a function of the actual command
    // set: two policies that differ only in one command get different hashes.
    let mut altered = policy.clone();
    if let Some(entry) = altered.commands.first_mut() {
        entry.command.push_str(" --extra");
    }
    let digest = kvist::config::compute_policy_hash(policy);
    let altered_digest = kvist::config::compute_policy_hash(&altered);
    assert_ne!(
        digest, altered_digest,
        "policy digest must change when the command set changes"
    );
}
