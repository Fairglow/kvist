//! Toolchain discoverability tests for all supported toolchains.
//!
//! These tests verify that each toolchain is discoverable and usable inside
//! the sandbox via PATH and version commands, ensuring an agent can find and
//! use the toolchain without needing to know the exact location.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

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
    if let Some(path) = std::env::var_os("KVIST_GALLA") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return path;
        }
    }
    // Check standard installation locations first
    if let Some(path) = std::env::var_os("HOME") {
        let user_runner = PathBuf::from(path).join(".kvist/bin/galla-runner");
        if user_runner.is_file() {
            return user_runner;
        }
    }
    let system_runner = PathBuf::from("/usr/local/bin/galla-runner");
    if system_runner.is_file() {
        return system_runner;
    }
    // Fall back to build directories
    let mut roots = Vec::new();
    if let Some(path) = std::env::var_os("CARGO_TARGET_DIR") {
        roots.push(PathBuf::from(path));
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target"));
    roots.push(PathBuf::from("/opt/target"));
    roots
        .iter()
        .flat_map(|root| {
            ["debug", "release"].map(|profile| root.join(profile).join("galla-runner"))
        })
        .find(|path| path.is_file())
        .expect("build galla-runner first or set KVIST_GALLA; native coverage must not skip")
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

fn host_gcc_version() -> String {
    let output = Command::new("/usr/bin/gcc")
        .arg("--version")
        .output()
        .expect("provision the system GCC compiler before running native tests");
    assert!(
        output.status.success(),
        "host gcc --version failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("host GCC version output must be UTF-8")
        .lines()
        .find(|line| !line.trim().is_empty())
        .expect("host GCC must report its version")
        .to_owned()
}

fn gcc_version_matches(output: &str, expected: &str) -> bool {
    output.lines().any(|line| line == expected)
}

#[test]
fn gcc_version_evidence_accepts_distribution_banners_and_rejects_mismatches() {
    for expected in [
        "gcc (Ubuntu 13.3.0-6ubuntu2~24.04) 13.3.0",
        "gcc (Debian 12.2.0-14) 12.2.0",
        "gcc (GCC) 16.2.1 20260810",
    ] {
        for prefix in ["/usr/bin/gcc\n", "Python 3.12.3\nrustc 1.95.0\n"] {
            let output = format!("{prefix}{expected}\nCopyright (C) Free Software Foundation\n");
            assert!(
                gcc_version_matches(&output, expected),
                "valid compiler evidence rejected: {expected}"
            );
        }
        for output in [
            "",
            "/usr/bin/gcc\n",
            "gcc (GCC) 1.0.0\n",
            "not the compiler version (GCC)\n",
        ] {
            assert!(!gcc_version_matches(output, expected), "{output}");
        }
    }
}

#[test]
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
fn javascript_toolchain_is_discoverable_via_path_and_version_commands() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    let registry = resolve(vec![ToolProfile::Rust, ToolProfile::JavaScript], &workdir);
    assert!(registry.profiles().contains(&"rust"));
    assert!(registry.profiles().contains(&"javascript"));
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
        "test -x /rust/runtime/bin/cargo && case \":$PATH:\" in *:/usr/local/bin:*) ;; *) printf '%s\\n' 'sandbox PATH omits /usr/local/bin' >&2; exit 1 ;; esac && which node && node --version",
    );
    assert!(
        !result.failed(),
        "sandbox Node discovery/version command failed: {}",
        result.error_text(8192)
    );
    let output = result.output_text(8192);
    assert!(output.contains("node"), "node not found");
    assert!(
        output.starts_with("v") || output.contains("."),
        "node version output missing"
    );
}

#[test]
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
    let expected = host_gcc_version();
    assert!(
        gcc_version_matches(&output, &expected),
        "sandbox GCC version must match host `{expected}`; got:\n{output}"
    );
}

#[test]
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
    let expected = host_gcc_version();
    assert!(
        gcc_version_matches(&output, &expected),
        "sandbox GCC version must match host `{expected}`; got:\n{output}"
    );
}
