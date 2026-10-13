//! Toolchain discoverability tests for all supported toolchains.
//!
//! These tests verify that each toolchain is discoverable and usable inside
//! the sandbox via PATH and version commands, ensuring an agent can find and
//! use the toolchain without needing to know the exact location.

use std::collections::BTreeMap;
use std::path::PathBuf;

use sav::{CancellationToken, ToolIntent};
use serde_json::json;
use skott::session::ToolExecutor;
use skott::toolchain::HostProbe;
use skott::{ProfileSetting, SandboxExecutor, SandboxPaths, ToolPolicy, ToolProfile, ToolRegistry};

fn fixture() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".toolchain-discover-test-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap()
}

fn runner() -> PathBuf {
    std::env::var_os("KVIST_RUST_TEST_RUNNER")
        .map(PathBuf::from)
        .expect("set KVIST_RUST_TEST_RUNNER to an independently built runner outside the fixture")
}

fn shell(executor: &SandboxExecutor, command: &str) -> skott::ToolOutcome {
    executor
        .execute(
            &ToolIntent {
                id: "discover".into(),
                provider_id: None,
                name: "shell".into(),
                arguments: json!({"command": command}),
            },
            &CancellationToken::new(),
        )
        .unwrap()
}

fn resolve(profiles: Vec<ToolProfile>, workdir: &std::path::Path) -> ToolRegistry {
    let settings = profiles
        .iter()
        .map(|p| (*p, ProfileSetting::On))
        .collect::<BTreeMap<_, _>>();
    ToolRegistry::resolve_for_workspace(ToolPolicy::default(), &settings, &HostProbe, None, workdir)
        .unwrap()
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn python_toolchain_is_discoverable_via_path_and_version_commands() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    let registry = resolve(vec![ToolProfile::Python], &workdir);
    assert!(registry.profiles().contains(&"python"));
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir,
    );
    let result = shell(&executor, "which python3 && python3 --version");
    assert!(!result.failed(), "{}", result.error_text(8192));
    let output = result.output_text(8192);
    assert!(output.contains("python3"), "python3 not found");
    assert!(
        output.contains("Python 3."),
        "python3 version output missing"
    );
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn javascript_toolchain_is_discoverable_via_path_and_version_commands() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    let registry = resolve(vec![ToolProfile::JavaScript], &workdir);
    assert!(registry.profiles().contains(&"javascript"));
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir,
    );
    let result = shell(&executor, "which node && node --version");
    assert!(!result.failed(), "{}", result.error_text(8192));
    let output = result.output_text(8192);
    assert!(output.contains("node"), "node not found");
    assert!(
        output.starts_with("v") || output.contains("."),
        "node version output missing"
    );
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn go_toolchain_is_discoverable_via_path_and_version_commands() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    let registry = resolve(vec![ToolProfile::Go], &workdir);
    assert!(registry.profiles().contains(&"go"));
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir,
    );
    let result = shell(&executor, "which go && go version");
    assert!(!result.failed(), "{}", result.error_text(8192));
    let output = result.output_text(8192);
    assert!(output.contains("go"), "go not found");
    assert!(output.contains("go version"), "go version output missing");
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn c_toolchain_is_discoverable_via_path_and_version_commands() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    let registry = resolve(vec![ToolProfile::C], &workdir);
    assert!(registry.profiles().contains(&"c"));
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir,
    );
    let result = shell(&executor, "which gcc && gcc --version");
    assert!(!result.failed(), "{}", result.error_text(8192));
    let output = result.output_text(8192);
    assert!(output.contains("gcc"), "gcc not found");
    assert!(output.contains("(GCC)"), "gcc version output missing");
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn all_toolchains_are_discoverable_together() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    let registry = resolve(
        vec![
            ToolProfile::Python,
            ToolProfile::Rust,
            ToolProfile::JavaScript,
            ToolProfile::Go,
            ToolProfile::C,
        ],
        &workdir,
    );
    assert!(registry.profiles().contains(&"python"));
    assert!(registry.profiles().contains(&"rust"));
    assert!(registry.profiles().contains(&"javascript"));
    assert!(registry.profiles().contains(&"go"));
    assert!(registry.profiles().contains(&"c"));
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir,
    );
    let result = shell(
        &executor,
        "python3 --version && rustc --version && node --version && go version && gcc --version",
    );
    assert!(!result.failed(), "{}", result.error_text(8192));
    let output = result.output_text(8192);
    assert!(
        output.contains("Python 3."),
        "python3 version output missing"
    );
    assert!(output.contains("rustc 1."), "rustc version output missing");
    assert!(
        output.starts_with("v") || output.contains("."),
        "node version output missing"
    );
    assert!(output.contains("go version"), "go version output missing");
    assert!(output.contains("(GCC)"), "gcc version output missing");
}
