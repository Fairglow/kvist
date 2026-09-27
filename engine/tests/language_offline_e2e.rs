//! End-to-end validation of the per-language offline verification topologies
//! (ADR-0011, ADR-0013).
//!
//! Each test drives the real production path — host provisioning through
//! `kvist vendor` dispatch, vendoring enforcement, and
//! `run_offline_language_verification` against the version-one bubblewrap
//! runner — so a real project builds and tests with the network denied inside
//! the sandbox. Tests self-skip when the live sandbox or the language
//! toolchain is not available on the host, so they never fail in an
//! environment that lacks them.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use kvist::config::{AcquisitionConfig, SandboxConfig, VcsSelection};
use kvist::language_verification::run_offline_language_verification;
use kvist::sandbox::{
    ExecutionOptions, SandboxProbe, backend_identity, ensure_available, runner_identity,
};
use kvist::vendor_command::{VendorOptions, vendor_project};

/// A well-formed policy identity (sha256 digest). The live tests carry no
/// authenticated execution approval, so a valid-format digest stands in for
/// the approval digest the production path passes; the runner validates its
/// format.
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

/// Locate the built sandbox runner, preferring an explicit override. The
/// validator requires the runner outside the selected VCS worktree, so a
/// candidate inside it is treated as absent and the test self-skips.
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
            return Some(PathBuf::from(candidate));
        }
    }
    None
}

/// Root of the enclosing git worktree, so the verification project shares the
/// worktree the validator expects.
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

/// The shared live-sandbox preconditions; `None` (skip) when unmet. A git
/// worktree is required: the verification project lives inside it and the
/// runner must sit outside it.
fn live_sandbox_ready() -> Option<(PathBuf, PathBuf)> {
    let runner = locate_runner()?;
    let bwrap = locate_backend()?;
    git_worktree_root()?;
    Some((runner, bwrap))
}

/// Build the sandbox configuration used by every language e2e test.
fn sandbox_config(runner: &Path, bwrap: &Path) -> SandboxConfig {
    SandboxConfig {
        runner: runner.to_string_lossy().into_owned(),
        backend: bwrap.to_string_lossy().into_owned(),
        environment_allowlist: vec!["PATH".to_owned(), "TERM".to_owned()],
        acquisition: AcquisitionConfig::default(),
    }
}

/// Provision a project with `kvist vendor`, requiring the readiness report.
fn provision(project: &Path) -> bool {
    match vendor_project(
        project,
        VendorOptions {
            populate: true,
            vendored_dir: None,
        },
    ) {
        Ok(report) => {
            if !report.ready() {
                eprintln!("skip: vendoring not ready: {report}");
            }
            report.ready()
        }
        Err(source) => {
            eprintln!("skip: vendoring unavailable: {source}");
            false
        }
    }
}

/// Run offline language verification and assert a successful test run whose
/// output contains at least one of the expected evidence markers.
fn verify_offline(
    config: &SandboxConfig,
    project: &Path,
    probe: &SandboxProbe,
    expected_markers: &[&str],
) -> bool {
    let result = match run_offline_language_verification(
        config,
        project,
        VcsSelection::Git,
        probe,
        POLICY_IDENTITY,
        project,
        None,
        ExecutionOptions {
            timeout: Some(Duration::from_secs(240)),
            output_limit: Some(1 << 20),
            live_stdout: None,
        },
    ) {
        Ok(result) => result,
        Err(source) => {
            eprintln!("offline language verification failed: {source}");
            return false;
        }
    };
    let stdout = String::from_utf8_lossy(&result.output.stdout);
    let stderr = String::from_utf8_lossy(&result.output.stderr);
    if result.timed_out || result.output_limit_exceeded || result.cancelled {
        eprintln!("offline verification did not complete cleanly");
        return false;
    }
    if !result.output.status.success() {
        eprintln!("offline test failed; stdout={stdout}; stderr={stderr}");
        return false;
    }
    if !expected_markers
        .iter()
        .any(|marker| stdout.contains(marker))
    {
        eprintln!(
            "offline test output carries no expected markers ({:?}); stdout={stdout}",
            expected_markers
        );
        return false;
    }
    true
}

/// Write bytes to a path, panicking on io error (test fixture helper).
fn write_file(path: PathBuf, contents: &str) {
    std::fs::write(path, contents).expect("write fixture file");
}

/// End-to-end: a small Go module is vendored on the host (`go mod vendor` via
/// `kvist vendor`) and `go test -mod=vendor ./...` builds and runs offline
/// inside the bubblewrap sandbox with the network denied.
#[test]
fn go_offline_verification_builds_and_tests_denied_network() {
    let Some((runner, bwrap)) = live_sandbox_ready() else {
        eprintln!("skip: live sandbox prerequisites not met");
        return;
    };
    if !Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skip: go not on PATH");
        return;
    }

    // A throwaway Go module depending on one small, dependency-free registry
    // module, vendored inside the worktree so the runner stays outside it.
    let worktree = git_worktree_root().expect("worktree checked by live_sandbox_ready");
    let project = tempfile::tempdir_in(&worktree).expect("temp project directory");
    let root = project.path();
    write_file(
        root.join("go.mod"),
        "module e2eminigo\n\ngo 1.24\n\nrequire github.com/google/uuid v1.6.0\n",
    );
    write_file(
        root.join("main_test.go"),
        "package e2eminigo\n\nimport (\n\t\"testing\"\n\n\t\"github.com/google/uuid\"\n)\n\nfunc TestUsesVendoredModule(t *testing.T) {\n\tif uuid.NewString() == \"\" {\n\t\tt.Fatal(\"expected a generated uuid\")\n\t}\n}\n",
    );

    // Generate the checksum lockfile (network on the host acquisition side).
    let tidy = Command::new("go")
        .args(["mod", "tidy"])
        .current_dir(root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !tidy {
        eprintln!("skip: go mod tidy failed (offline host?)");
        return;
    }
    if !provision(root) {
        return;
    }
    if !root.join("vendor").join("modules.txt").is_file() {
        eprintln!("skip: go mod vendor did not produce vendor/modules.txt");
        return;
    }

    let config = sandbox_config(&runner, &bwrap);
    let approved_runner =
        runner_identity(&config, root, VcsSelection::Git).expect("identify the runner");
    let approved_backend =
        backend_identity(&config, root, VcsSelection::Git).expect("identify the backend");
    let probe: SandboxProbe = ensure_available(
        &config,
        root,
        VcsSelection::Git,
        &approved_runner,
        &approved_backend,
    )
    .expect("the bwrap capability probe confirms production isolation");

    assert!(
        verify_offline(&config, root, &probe, &["ok  ", "PASS"]),
        "go offline verification must build and test the vendored project"
    );
}

/// End-to-end: a zero-dependency Node project is provisioned on the host
/// (`npm ci` via `kvist vendor`) and `node --test` runs its real test suite
/// offline inside the bubblewrap sandbox with the network denied.
#[test]
fn javascript_offline_verification_builds_and_tests_denied_network() {
    let Some((runner, bwrap)) = live_sandbox_ready() else {
        eprintln!("skip: live sandbox prerequisites not met");
        return;
    };
    if !Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skip: node not on PATH");
        return;
    }
    if !Command::new("npm")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skip: npm not on PATH");
        return;
    }

    // A zero-dependency project with a committed lock file: `npm ci`
    // reconciles exactly from it and never needs the network, and
    // `node --test` runs the suite from the project alone.
    let worktree = git_worktree_root().expect("worktree checked by live_sandbox_ready");
    let project = tempfile::tempdir_in(&worktree).expect("temp project directory");
    let root = project.path();
    write_file(
        root.join("package.json"),
        "{\"name\":\"e2eminibs\",\"version\":\"0.1.0\"}\n",
    );
    write_file(
        root.join("package-lock.json"),
        "{\"name\":\"e2eminibs\",\"version\":\"0.1.0\",\"lockfileVersion\":3,\
         \"requires\":true,\"packages\":{\"\":{\"name\":\"e2eminibs\",\"version\":\"0.1.0\"}}}\n",
    );
    write_file(
        root.join("main.test.js"),
        "const test = require('node:test');\nconst assert = require('node:assert');\n\n\
         test('zero-dep offline check', () => {\n  assert.ok(true);\n});\n",
    );

    if !provision(root) {
        return;
    }
    if !root.join(".kvist").join("vendored-js").is_dir() {
        eprintln!("skip: npm ci did not produce the vendored package cache");
        return;
    }

    let config = sandbox_config(&runner, &bwrap);
    let approved_runner =
        runner_identity(&config, root, VcsSelection::Git).expect("identify the runner");
    let approved_backend =
        backend_identity(&config, root, VcsSelection::Git).expect("identify the backend");
    let probe: SandboxProbe = ensure_available(
        &config,
        root,
        VcsSelection::Git,
        &approved_runner,
        &approved_backend,
    )
    .expect("the bwrap capability probe confirms production isolation");

    assert!(
        verify_offline(&config, root, &probe, &["pass 1", "tests 1"]),
        "javascript offline verification must build and test the vendored project"
    );
}
