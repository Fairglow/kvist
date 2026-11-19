//! Tests that define the runner installation boundary before it is trusted on
//! another host.
//!
//! The project `kvist.toml` pins a machine-local absolute `[sandbox] runner`
//! path. A fresh host or a fresh checkout has no runner at that path, so the
//! installation procedure must be explicit and testable before the policy is
//! documented and relied on. These tests define two required behaviors:
//!
//! 1. An absent configured runner produces an actionable diagnostic that names
//!    the installation step and the configured `[sandbox] runner` path.
//! 2. The documented installation policy references the configured `[sandbox]`
//!    runner path and the descriptor-bound validation that follows it.
//!
//! Both behaviors are defined here so the dependent implementation task
//! (`galla-install-policy`) has an executable contract. They fail while the
//! diagnostic is generic and the policy is undocumented, and pass once the
//! installation step is named and the policy is documented.

use std::fs;
use std::path::PathBuf;

use maerg::config::{AcquisitionConfig, SandboxConfig, VcsSelection};
use maerg::{KvistError, sandbox::runner_identity};

/// The maerg component directory, where the component documentation lives.
fn component_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The workspace root is the parent of the maerg component directory and
/// holds the project `kvist.toml` that pins the configured runner path.
fn workspace_root() -> PathBuf {
    component_dir().parent().unwrap().to_path_buf()
}

/// A minimal sandbox configuration whose runner path does not exist on this
/// host, mirroring a fresh checkout where the configured runner has not been
/// installed.
fn absent_runner_config() -> SandboxConfig {
    SandboxConfig {
        runner: "/nonexistent/galla-install-policy-runner".to_owned(),
        backend: "/usr/bin/bwrap".to_owned(),
        environment_allowlist: Vec::new(),
        acquisition: AcquisitionConfig::default(),
    }
}

/// The component documentation files that may carry the installation policy.
fn component_documents() -> Vec<PathBuf> {
    let root = component_dir();
    [
        "README.md",
        "CONTRACT.md",
        "DESIGN.md",
        "REQUIREMENTS.md",
        "IMPL.md",
    ]
    .iter()
    .map(|name| root.join(name))
    .filter(|path| path.is_file())
    .collect()
}

/// An absent configured runner must produce an actionable diagnostic that
/// names the installation step and the configured `[sandbox] runner` path.
///
/// The current diagnostic is the generic `sandbox error in inspect trusted
/// sandbox runner: No such file or directory (os error 2)`, which does not
/// name the installation step. This test defines the required behavior: the
/// diagnostic must tell the operator to install the runner and must name the
/// configured runner path so the operator knows exactly what to install.
#[test]
fn absent_configured_runner_diagnostic_names_installation_step_and_path() {
    let config = absent_runner_config();
    let project_root = component_dir();
    let error = runner_identity(&config, &project_root, VcsSelection::Auto)
        .expect_err("an absent configured runner must fail closed");
    let KvistError::SandboxUnavailable { runner, reason } = error else {
        panic!(
            "an absent configured runner must fail with SandboxUnavailable, not `{error:?}`"
        );
    };
    // The diagnostic must name the installation step.
    assert!(
        reason.to_lowercase().contains("install"),
        "the absent-runner diagnostic must name the installation step; \
         it did not mention installing the runner: `{reason}`"
    );
    // The diagnostic must name the configured runner path.
    assert!(
        reason.contains(&config.runner),
        "the absent-runner diagnostic must name the configured [sandbox] runner \
         path `{}` so the operator knows what to install; it named `{runner}` \
         with reason `{reason}`",
        config.runner
    );
}

/// The documented installation policy must reference the configured
/// `[sandbox]` runner path and the descriptor-bound validation that follows it.
///
/// The installation policy is not yet documented. This test defines the
/// required documentation behavior: the policy must reference the configured
/// `[sandbox]` runner path (so a fresh host can reproduce the runner) and the
/// descriptor-bound validation that follows installation (so the policy does
/// not weaken the approval-bound identity check).
#[test]
fn documented_installation_policy_references_runner_path_and_descriptor_bound_validation() {
    // Read the configured [sandbox] runner path from the workspace kvist.toml.
    let kvist_toml = workspace_root().join("kvist.toml");
    let toml_text = fs::read_to_string(&kvist_toml)
        .expect("read the workspace kvist.toml to learn the configured runner path");
    let runner_path = toml_text
        .lines()
        .find_map(|line| {
            let line = line.trim();
            let value = line.strip_prefix("runner")?.trim_start().strip_prefix('=')?;
            Some(value.trim().trim_matches('"').to_owned())
        })
        .expect("the workspace kvist.toml must configure a [sandbox] runner path");

    // Collect the component documentation text.
    let documents = component_documents();
    assert!(
        !documents.is_empty(),
        "the component must have documentation files to carry the installation policy"
    );
    let docs_text: String = documents
        .iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .collect::<Vec<_>>()
        .join("\n");

    // The installation policy must reference the configured [sandbox] runner path.
    assert!(
        docs_text.contains(&runner_path),
        "the documented installation policy must reference the configured \
         [sandbox] runner path `{runner_path}` so a fresh host can reproduce \
         the runner"
    );

    // The installation policy must reference the descriptor-bound validation
    // that follows installation.
    assert!(
        docs_text.to_lowercase().contains("descriptor"),
        "the documented installation policy must reference the descriptor-bound \
         validation that follows installation, so the policy does not weaken \
         the approval-bound identity check"
    );
}
