//! End-to-end validation of the offline cargo verification topology
//! (ADR-0011).
//!
//! This drives the real production path: runner_identity,
//! backend_identity, ensure_available (the version-one bubblewrap
//! capability probe), and finally run_offline_cargo_verification,
//! against a small vendored Rust project, so a network-denied
//! cargo test --locked actually executes inside bubblewrap.
//!
//! It self-skips when the live sandbox cannot run here (no built
//! runner outside the git worktree, no bubblewrap backend, no cargo
//! or rustup, no git worktree, or no network for the initial
//! vendoring pass), so it never fails in an environment that lacks
//! the bwrap runner.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use kvist::config::{AcquisitionConfig, SandboxConfig, VcsSelection};
use kvist::sandbox::{
    ExecutionOptions, SandboxProbe, backend_identity, ensure_available,
    run_offline_cargo_verification, runner_identity,
};
use kvist::vendor_command::{VendorOptions, vendor_project};

/// A well-formed policy identity (sha256 digest). The live test carries no
/// authenticated execution approval, so a valid-format digest stands in for the
/// approval digest the production path passes; the runner validates its format.
const POLICY_IDENTITY: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The workspace root is the parent of the engine crate manifest.
fn workspace_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
}

/// Candidate roots that may hold the built sandbox runner. The target
/// directory is cargo-configured to `/opt/target` for this project, so the
/// workspace-local `target` and the configured root are both checked.
fn candidate_target_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        roots.push(PathBuf::from(dir));
    }
    if let Some(root) = workspace_root() {
        roots.push(root.join("target"));
    }
    roots.push(PathBuf::from("/opt/target"));
    roots
}

/// Locate the built sandbox runner, preferring an explicit override.
///
/// The validator requires the runner to be installed outside the
/// selected VCS worktree, so a candidate inside it (such as the
/// default in-worktree cargo target directory) is unusable and is
/// treated as absent: the test self-skips instead of failing.
fn locate_runner() -> Option<PathBuf> {
    let worktree = match git_worktree_root().as_deref() {
        Some(root) => root.canonicalize().ok(),
        None => None,
    };
    let outside_worktree = |candidate: &Path| match &worktree {
        Some(root) => candidate
            .canonicalize()
            .ok()
            .is_some_and(|canonical| !canonical.starts_with(root)),
        None => true,
    };
    if let Ok(path) = std::env::var("KVIST_SANDBOX_RUNNER") {
        let candidate = Path::new(&path);
        if candidate.is_file() && outside_worktree(candidate) {
            return Some(candidate.to_path_buf());
        }
    }
    for root in candidate_target_roots() {
        for profile in ["debug", "release"] {
            let candidate = root.join(profile).join("kvist-sandbox-runner");
            if candidate.is_file() && outside_worktree(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Locate a bubblewrap backend, preferring an explicit override.
fn locate_backend() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("KVIST_SANDBOX_BACKEND") {
        let candidate = Path::new(&path);
        if candidate.is_file() {
            return Some(candidate.to_path_buf());
        }
    }
    let backends = ["/usr/bin/bwrap", "/bin/bwrap", "/usr/local/bin/bwrap"];
    for candidate in backends {
        let path = Path::new(candidate);
        if path.is_file() {
            return Some(path.to_path_buf());
        }
    }
    None
}

/// Root of the enclosing git worktree, so the verification project shares
/// the worktree the validator expects, keeping the runner and backend
/// outside it, as the validator requires.
fn git_worktree_root() -> Option<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("git")
        .args([
            "-C",
            manifest_dir.to_string_lossy().as_ref(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(PathBuf::from(
        String::from_utf8(output.stdout).ok()?.trim().to_owned(),
    ))
}

#[test]
fn offline_cargo_verification_builds_tested_project_denied_network() {
    let Some(runner) = locate_runner() else {
        eprintln!("skip: no built sandbox runner; build it first");
        return;
    };
    let Some(bwrap) = locate_backend() else {
        eprintln!("skip: no bubblewrap backend on PATH");
        return;
    };
    if !Command::new("cargo").arg("--version").output().is_ok() {
        eprintln!("skip: cargo not on PATH");
        return;
    }
    if !Command::new("rustup").arg("--version").output().is_ok() {
        eprintln!("skip: rustup not on PATH");
        return;
    }
    let Some(worktree) = git_worktree_root() else {
        eprintln!("skip: not inside a git worktree");
        return;
    };

    // A throwaway Rust project depending on one registry crate, vendored and
    // built inside the worktree so the runner and backend stay outside it, as
    // the validator requires.
    let project = tempfile::tempdir_in(&worktree).expect("temp project directory");
    write_mini_cargo_project(project.path());

    // Generate the lockfile, then populate and enforce the vendored registry.
    // Both steps may need network; skip (never fail) when the host cannot reach
    // the index, since this test validates live execution only.
    let locked = Command::new("cargo")
        .arg("generate-lockfile")
        .current_dir(project.path())
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !locked {
        eprintln!("skip: cargo generate-lockfile failed (offline host?)");
        return;
    }
    let report = match vendor_project(
        project.path(),
        VendorOptions {
            populate: true,
            vendored_dir: None,
        },
    ) {
        Ok(report) => report,
        Err(source) => {
            eprintln!("skip: vendoring unavailable: {source}");
            return;
        }
    };
    assert!(
        report.ready(),
        "vendoring must report the project ready for offline builds"
    );

    // Identify the trusted runner and backend, confirm the version-one bwrap
    // capability probe, then verify offline.
    let config = SandboxConfig {
        runner: runner.to_string_lossy().into_owned(),
        backend: bwrap.to_string_lossy().into_owned(),
        environment_allowlist: vec![
            "PATH".to_owned(),
            "RUST_BACKTRACE".to_owned(),
            "CARGO_HOME".to_owned(),
            "RUSTC".to_owned(),
            "TERM".to_owned(),
        ],
        acquisition: AcquisitionConfig::default(),
    };

    let approved_runner = runner_identity(&config, project.path(), VcsSelection::Git)
        .expect("identify the trusted sandbox runner");
    let approved_backend = backend_identity(&config, project.path(), VcsSelection::Git)
        .expect("identify the approved bubblewrap backend");
    let probe: SandboxProbe = ensure_available(
        &config,
        project.path(),
        VcsSelection::Git,
        &approved_runner,
        &approved_backend,
    )
    .expect("the bwrap capability probe confirms production isolation");

    let result = run_offline_cargo_verification(
        &config,
        project.path(),
        VcsSelection::Git,
        &probe,
        POLICY_IDENTITY,
        project.path(),
        ExecutionOptions {
            timeout: Some(Duration::from_secs(180)),
            output_limit: Some(1 << 20),
            live_stdout: None,
        },
    )
    .expect("offline cargo verification executes");

    let stdout = String::from_utf8_lossy(&result.output.stdout);
    let stderr = String::from_utf8_lossy(&result.output.stderr);
    assert!(
        !result.timed_out && !result.output_limit_exceeded && !result.cancelled,
        "verification must not time out, overflow, or be cancelled"
    );
    if !result.output.status.success() {
        eprintln!("offline cargo test failed; stdout={stdout}; stderr={stderr}");
        panic!("offline cargo verification failed inside bwrap");
    }
    assert!(
        stdout.contains("test result:") || stderr.contains("test result:"),
        "cargo must have run at least one test"
    );
}

/// The active rustup toolchain channel with the target-triple suffix removed,
/// so the pinned e2e case can pin the toolchain that is already installed.
/// `None` when the channel cannot be derived (skip, never fail).
fn active_rustup_channel() -> Option<String> {
    let output = Command::new("rustup")
        .args(["show", "active-toolchain"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .split_whitespace()
        .next()?
        .to_owned();
    let channel = token
        .strip_suffix("-x86_64-unknown-linux-gnu")
        .or_else(|| token.strip_suffix("-aarch64-unknown-linux-gnu"))?
        .to_owned();
    kvist::toolchain::validate_channel(&channel, "e2e active toolchain").ok()?;
    Some(channel)
}

/// End-to-end: a project pinned via `rust-toolchain.toml` is provisioned by
/// `kvist toolchain ensure` (ADR-0012), and the offline verification resolves
/// that exact pinned toolchain channel-explicitly and builds against it.
#[test]
fn offline_cargo_verification_uses_pinned_toolchain() {
    let Some(runner) = locate_runner() else {
        eprintln!("skip: no built sandbox runner; build it first");
        return;
    };
    let Some(bwrap) = locate_backend() else {
        eprintln!("skip: no bubblewrap backend on PATH");
        return;
    };
    if !Command::new("cargo").arg("--version").output().is_ok() {
        eprintln!("skip: cargo not on PATH");
        return;
    }
    if !Command::new("rustup").arg("--version").output().is_ok() {
        eprintln!("skip: rustup not on PATH");
        return;
    }
    let Some(worktree) = git_worktree_root() else {
        eprintln!("skip: not inside a git worktree");
        return;
    };
    let Some(channel) = active_rustup_channel() else {
        eprintln!("skip: active rustup toolchain channel not derivable");
        return;
    };

    // A throwaway Rust project pinned to the active toolchain, vendored and
    // built inside the worktree so the runner and backend stay outside it.
    let project = tempfile::tempdir_in(&worktree).expect("temp project directory");
    write_mini_cargo_project(project.path());
    write_file(
        project.path().join("rust-toolchain.toml"),
        format!("[toolchain]\nchannel = \"{channel}\"\n").as_bytes(),
    );

    // The host-authorized provisioning step records the durable manifest.
    let manifest = kvist::toolchain::ensure_toolchain(project.path(), "e2e pinned")
        .expect("toolchain ensure records the pinned manifest");
    assert_eq!(manifest.channel, channel);

    let locked = Command::new("cargo")
        .arg("generate-lockfile")
        .current_dir(project.path())
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !locked {
        eprintln!("skip: cargo generate-lockfile failed (offline host?)");
        return;
    }
    let report = match vendor_project(
        project.path(),
        VendorOptions {
            populate: true,
            vendored_dir: None,
        },
    ) {
        Ok(report) => report,
        Err(source) => {
            eprintln!("skip: vendoring unavailable: {source}");
            return;
        }
    };
    assert!(
        report.ready(),
        "vendoring must report the project ready for offline builds"
    );

    let config = SandboxConfig {
        runner: runner.to_string_lossy().into_owned(),
        backend: bwrap.to_string_lossy().into_owned(),
        environment_allowlist: vec![
            "PATH".to_owned(),
            "RUST_BACKTRACE".to_owned(),
            "CARGO_HOME".to_owned(),
            "RUSTC".to_owned(),
            "TERM".to_owned(),
        ],
        acquisition: AcquisitionConfig::default(),
    };

    let approved_runner = runner_identity(&config, project.path(), VcsSelection::Git)
        .expect("identify the trusted sandbox runner");
    let approved_backend = backend_identity(&config, project.path(), VcsSelection::Git)
        .expect("identify the approved bubblewrap backend");
    let probe: SandboxProbe = ensure_available(
        &config,
        project.path(),
        VcsSelection::Git,
        &approved_runner,
        &approved_backend,
    )
    .expect("the bwrap capability probe confirms production isolation");

    let result = run_offline_cargo_verification(
        &config,
        project.path(),
        VcsSelection::Git,
        &probe,
        POLICY_IDENTITY,
        project.path(),
        ExecutionOptions {
            timeout: Some(Duration::from_secs(180)),
            output_limit: Some(1 << 20),
            live_stdout: None,
        },
    )
    .expect("offline cargo verification executes against the pinned toolchain");

    let stdout = String::from_utf8_lossy(&result.output.stdout);
    let stderr = String::from_utf8_lossy(&result.output.stderr);
    assert!(
        !result.timed_out && !result.output_limit_exceeded && !result.cancelled,
        "verification must not time out, overflow, or be cancelled"
    );
    if !result.output.status.success() {
        eprintln!("offline cargo test failed; stdout={stdout}; stderr={stderr}");
        panic!("offline cargo verification failed inside bwrap");
    }
    assert!(
        stdout.contains("test result:") || stderr.contains("test result:"),
        "cargo must have run at least one test"
    );
}

/// Write bytes to a path, failing the test on io error.
fn write_file(path: PathBuf, contents: &[u8]) {
    std::fs::write(path, contents).expect("write file");
}

/// Write a minimal Rust project depending on one registry crate with a test.
fn write_mini_cargo_project(dir: &Path) {
    let manifest = [
        "[package]",
        "name = \"e2e_mini\"",
        "version = \"0.1.0\"",
        "edition = \"2021\"",
        "",
        "[dependencies]",
        "cfg-if = \"1\"",
        // A standalone workspace so cargo does not absorb this throwaway
        // project into the enclosing Kvist workspace while it is vendored.
        "[workspace]",
    ]
    .join("\n");
    write_file(dir.join("Cargo.toml"), format!("{manifest}\n").as_bytes());

    std::fs::create_dir_all(dir.join("src")).expect("create src");
    let lib = [
        "#[cfg(test)]",
        "mod tests {",
        "    #[test]",
        "    fn uses_vendored_cfg_if() {",
        // `cfg_if!` expands to the arm matching a `#[cfg(..)]` condition; under
        // `cargo test` the `#[cfg(test)]` arm is always selected.
        "        cfg_if::cfg_if! {",
        "            if #[cfg(test)] {",
        "                let value = 1;",
        "            } else {",
        "                let value = 0;",
        "            }",
        "        }",
        "        assert_eq!(value, 1);",
        "    }",
        "}",
    ]
    .join("\n");
    let lib_path = dir.join("src").join("lib.rs");
    write_file(lib_path, format!("{lib}\n").as_bytes());
}
