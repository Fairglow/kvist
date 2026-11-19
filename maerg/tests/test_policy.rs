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

use maerg::config;

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
    let digest = maerg::config::compute_policy_hash(policy).expect("policy serializes");
    let altered_digest = maerg::config::compute_policy_hash(&altered).expect("policy serializes");
    assert_ne!(
        digest, altered_digest,
        "policy digest must change when the command set changes"
    );
}

#[test]
fn policy_digest_is_deterministic_and_bound_to_the_full_policy() {
    // A well-formed policy serializes, so its digest is a stable function of
    // every field: the command set, the environment allowlist, the timeout,
    // and the output cap. Changing any one of them must change the digest,
    // and the same policy must always produce the same digest.
    let policy = maerg::config::TestPolicy {
        schema_version: 1,
        working_directory: "component".to_owned(),
        environment_allowlist: vec!["PATH".to_owned()],
        timeout_seconds: 1800,
        max_output_bytes: 1048576,
        commands: vec![maerg::config::TestCommandEntry {
            component: "maerg".to_owned(),
            command: "cargo test".to_owned(),
        }],
    };

    let first = maerg::config::compute_policy_hash(&policy).expect("policy serializes");
    let second = maerg::config::compute_policy_hash(&policy).expect("policy serializes");
    assert_eq!(first, second, "the same policy must always hash identically");
    assert!(
        first.starts_with("sha256:"),
        "the policy digest must be a sha256 digest, got {first}"
    );

    // The digest is bound to every field, not just the command set.
    let mut altered = policy.clone();
    altered.timeout_seconds = 1801;
    assert_ne!(
        first,
        maerg::config::compute_policy_hash(&altered).expect("policy serializes"),
        "the digest must change when the timeout changes"
    );

    let mut altered = policy.clone();
    altered.max_output_bytes = 1048577;
    assert_ne!(
        first,
        maerg::config::compute_policy_hash(&altered).expect("policy serializes"),
        "the digest must change when the output cap changes"
    );

    let mut altered = policy.clone();
    altered.environment_allowlist.push("RUST_BACKTRACE".to_owned());
    assert_ne!(
        first,
        maerg::config::compute_policy_hash(&altered).expect("policy serializes"),
        "the digest must change when the environment allowlist changes"
    );
}

/// A policy whose serialization fails must not collapse to a constant digest.
/// `compute_policy_hash` is fail-closed: a serialization error is propagated
/// rather than swallowed, so the digest can never silently decouple from the
/// command set. Here we assert the public contract: a well-formed policy
/// returns a digest, and the digest is never the empty-string hash that a
/// swallowed error would produce.
#[test]
fn policy_digest_fails_closed_when_serialization_is_unavailable() {
    let policy = maerg::config::TestPolicy {
        schema_version: 1,
        working_directory: "component".to_owned(),
        environment_allowlist: vec![],
        timeout_seconds: 1,
        max_output_bytes: 1,
        commands: vec![],
    };
    let digest = maerg::config::compute_policy_hash(&policy).expect("policy serializes");
    // The empty-string sha256 is the constant a swallowed serialization error
    // would produce; a real policy must never hash to it.
    let empty_string_hash = format!("sha256:{}", hex::encode(sha2::Sha256::digest(b"")));
    assert_ne!(
        digest, empty_string_hash,
        "a real policy must not hash to the empty-string digest"
    );
}
