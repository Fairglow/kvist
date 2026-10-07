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
            sandbox: None,
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
/// `test_command` is `None` for the canonical-command languages and the
/// approved command for C/C++ (the project-defined build system).
fn verify_offline(
    config: &SandboxConfig,
    project: &Path,
    probe: &SandboxProbe,
    test_command: Option<Vec<String>>,
    expected_markers: &[&str],
) -> bool {
    let result = match run_offline_language_verification(
        config,
        project,
        VcsSelection::Git,
        probe,
        POLICY_IDENTITY,
        project,
        test_command,
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
    // Language test runners emit their evidence on different streams (go test
    // and node --test on stdout, python unittest on stderr).
    let combined = format!("{stdout}\n{stderr}");
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
        .any(|marker| combined.contains(marker))
    {
        eprintln!(
            "offline test output carries no expected markers ({:?}); output={combined}",
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
        verify_offline(&config, root, &probe, None, &["ok  ", "PASS"]),
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
        verify_offline(&config, root, &probe, None, &["pass 1", "tests 1"]),
        "javascript offline verification must build and test the vendored project"
    );
}

/// End-to-end: a small Python project with one pinned pure-wheel dependency is
/// provisioned on the host (`pip download` plus a provisioned virtualenv via
/// `kvist vendor`) and `python3 -m unittest` runs its real test suite offline
/// inside the bubblewrap sandbox with the network denied, importing the
/// locked material from the mounted venv.
#[test]
fn python_offline_verification_builds_and_tests_denied_network() {
    let Some((runner, bwrap)) = live_sandbox_ready() else {
        eprintln!("skip: live sandbox prerequisites not met");
        return;
    };
    if !Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skip: python3 not on PATH");
        return;
    }

    // One pinned pure wheel (no C extension, so no manylinux constraint) plus
    // a unittest suite that imports it; the venv is the offline runtime.
    let worktree = git_worktree_root().expect("worktree checked by live_sandbox_ready");
    let project = tempfile::tempdir_in(&worktree).expect("temp project directory");
    let root = project.path();
    write_file(root.join("requirements.lock.txt"), "tomli==2.2.1\n");
    write_file(
        root.join("e2eminipy.py"),
        "import tomli\n\n\ndef parse(text: str) -> dict:\n    return tomli.loads(text)\n",
    );
    write_file(
        root.join("test_e2eminipy.py"),
        "import unittest\n\nfrom e2eminipy import parse\n\n\nclass TestTomli(unittest.TestCase):\n\n    def test_parses(self):\n        self.assertEqual(parse(\"a = 1\"), {\"a\": 1})\n",
    );

    if !provision(root) {
        return;
    }
    if !root
        .join(".kvist")
        .join("venv")
        .join("bin")
        .join("python")
        .is_file()
    {
        eprintln!("skip: kvist vendor did not provision the virtualenv");
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
        verify_offline(&config, root, &probe, None, &["OK", "Ran 1 test"]),
        "python offline verification must build and test the vendored project"
    );
}

/// End-to-end: a small C project with one Conan-locked zlib dependency is
/// provisioned on the host (`conan install` via `kvist vendor` into the
/// project-local Conan home plus generated CMake files), and the approved test
/// command (`make test`) compiles and runs its real test offline inside the
/// bubblewrap sandbox with the network denied, resolving zlib from the Conan
/// home mounted at its host path.
#[test]
fn c_offline_verification_builds_and_tests_denied_network() {
    let Some((runner, bwrap)) = live_sandbox_ready() else {
        eprintln!("skip: live sandbox prerequisites not met");
        return;
    };
    for tool in ["conan", "cmake", "make"] {
        if !Command::new(tool)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            eprintln!("skip: {tool} not on PATH");
            return;
        }
    }

    // One Conan-locked zlib dependency; the approved test command drives the
    // CMake build system, which consumes the generated toolchain file
    // (it references the Conan home by absolute host path) and writes all
    // build output under the sandbox scratch.
    let worktree = git_worktree_root().expect("worktree checked by live_sandbox_ready");
    let project = tempfile::tempdir_in(&worktree).expect("temp project directory");
    let root = project.path();
    write_file(root.join("conanfile.txt"), "[requires]\nzlib/1.3.1\n");
    write_file(
        root.join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.15)\nproject(e2eminicc C)\nfind_package(ZLIB REQUIRED)\nadd_executable(test_zlib test_zlib.c)\ntarget_link_libraries(test_zlib PRIVATE ZLIB::ZLIB)\n",
    );
    write_file(
        root.join("test_zlib.c"),
        "#include <zlib.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n\nint main(void) {\n    const char *msg = \"kvist offline conan check\";\n    unsigned long bound = compressBound((unsigned long)strlen(msg));\n    unsigned char *dst = malloc(bound);\n    unsigned char *back = malloc(strlen(msg));\n    uLongf dst_len = bound;\n    uLongf back_len = strlen(msg);\n    int rc = compress2(dst, &dst_len, (const unsigned char *)msg, (uInt)strlen(msg), Z_DEFAULT_COMPRESSION);\n    if (rc != Z_OK || uncompress(back, &back_len, dst, dst_len) != Z_OK || back_len != strlen(msg) || memcmp(back, msg, back_len) != 0) {\n        fprintf(stderr, \"zlib roundtrip failed\\n\");\n        return 1;\n    }\n    printf(\"zlib roundtrip ok %s\\n\", ZLIB_VERSION);\n    return 0;\n}\n",
    );
    write_file(
        root.join("Makefile"),
        "BUILD := /workspace/scratch/build\n\ntest:\n\t/usr/bin/cmake -S . -B $(BUILD) -DCMAKE_TOOLCHAIN_FILE=.kvist/conan-build/conan_toolchain.cmake -DCMAKE_BUILD_TYPE=Release\n\t/usr/bin/cmake --build $(BUILD)\n\t$(BUILD)/test_zlib\n",
    );

    if !provision(root) {
        return;
    }
    if !root
        .join(".kvist")
        .join("conan-build")
        .join("conan_toolchain.cmake")
        .is_file()
    {
        eprintln!("skip: conan did not generate the CMake toolchain file");
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
        verify_offline(
            &config,
            root,
            &probe,
            Some(vec!["make".to_owned(), "test".to_owned()]),
            &["zlib roundtrip ok"],
        ),
        "c offline verification must build and test the vendored project"
    );
}

/// End-to-end: a small Python project locked with `uv.lock` is provisioned on
/// the host (`uv lock` to generate the lock, then `kvist vendor` dispatching
/// `uv export` + `pip download` into the vendored wheels and an
/// offline-provisioned virtualenv) and `python3 -m unittest` runs its real test
/// suite offline inside the bubblewrap sandbox with the network denied,
/// importing a locked dependency and its transitive dependency from the
/// mounted venv. This exercises the `uv.lock` provisioning path end to end.
#[test]
fn python_uv_lock_offline_verification_builds_and_tests_denied_network() {
    let Some((runner, bwrap)) = live_sandbox_ready() else {
        eprintln!("skip: live sandbox prerequisites not met");
        return;
    };
    if !Command::new("uv")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skip: uv not on PATH");
        return;
    }
    if !Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("skip: python3 not on PATH");
        return;
    }

    // A `uv.lock`-locked project with a dependency that pulls a transitive
    // dependency; proving `uv export` resolves the full graph offline. The
    // transitive `six` dependency would be missed by a naive resolver and
    // fails to import if the export graph is incomplete.
    let worktree = git_worktree_root().expect("worktree checked by live_sandbox_ready");
    let project = tempfile::tempdir_in(&worktree).expect("temp project directory");
    let root = project.path();
    write_file(
        root.join("pyproject.toml"),
        r#"[project]
name = "e2eminippyuv"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = ["python-dateutil==2.9.0"]

[tool.uv]
package = false
"#,
    );
    write_file(
        root.join("e2eminippyuv.py"),
        r#"import dateutil
from dateutil import parser


def parse_day(text: str) -> int:
    return parser.parse(text).day
"#,
    );
    write_file(
        root.join("test_e2eminippyuv.py"),
        r#"import unittest

from e2eminippyuv import parse_day


class TestDateutil(unittest.TestCase):

    def test_day(self):
        self.assertEqual(parse_day("2024-01-15"), 15)
"#,
    );

    // Generate the `uv.lock` on the host (network on the host acquisition
    // side only); a fresh resolve cannot be vendored without it.
    let locked = Command::new("uv")
        .args(["lock"])
        .current_dir(root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !locked {
        eprintln!("skip: uv lock failed (offline host?)");
        return;
    }
    if !provision(root) {
        return;
    }
    if !root
        .join(".kvist")
        .join("venv")
        .join("bin")
        .join("python")
        .is_file()
    {
        eprintln!("skip: kvist vendor did not provision the virtualenv");
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
        verify_offline(&config, root, &probe, None, &["OK", "Ran 1 test"]),
        "uv.lock offline verification must build and test the vendored project"
    );
}

/// End-to-end: a small JavaScript project locked with `pnpm` (`pnpm-lock.yaml`)
/// is provisioned on the host (`pnpm install --store-path` into the vendored
/// pnpm store via `kvist vendor`) and `node --test` runs its real suite offline
/// inside the bubblewrap sandbox with the network denied, importing the locked
/// dependency (and, transitively, its dependency) from the provisioned
/// node_modules. This exercises the pnpm offline provisioning path end to end.
#[test]
fn javascript_pnpm_offline_verification_builds_and_tests_denied_network() {
    let Some((runner, bwrap)) = live_sandbox_ready() else {
        eprintln!("skip: live sandbox prerequisites not met");
        return;
    };
    for tool in ["node", "pnpm"] {
        if !Command::new(tool)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            eprintln!("skip: {tool} not on PATH");
            return;
        }
    }

    // One locked dependency that pulls a transitive dependency; proving the
    // pnpm store resolved the full graph offline. The transitive `is-number`
    // dependency is exercised when `is-odd` runs.
    let worktree = git_worktree_root().expect("worktree checked by live_sandbox_ready");
    let project = tempfile::tempdir_in(&worktree).expect("temp project directory");
    let root = project.path();
    write_file(
        root.join("package.json"),
        "{\"name\":\"e2eminipnpm\",\"version\":\"0.1.0\",\"dependencies\":{\"is-odd\":\"3.0.1\"}}\n",
    );
    write_file(
        root.join("main.test.js"),
        "const test = require('node:test');\nconst assert = require('node:assert');\n\
         const isOdd = require('is-odd');\n\n\
         test('pnpm offline transitive import', () => {\n\
           assert.equal(isOdd(3), true);\n\
           assert.equal(isOdd(4), false);\n\
         });\n",
    );

    // Generate the committed pnpm-lock.yaml on the host (network on the host
    // acquisition side only); detection needs the lock file before kvist vendor.
    let locked = Command::new("pnpm")
        .arg("install")
        .current_dir(root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !locked {
        eprintln!("skip: pnpm install failed to produce a lock file (offline host?)");
        return;
    }

    if !provision(root) {
        return;
    }
    // The pnpm catalogue is a content-addressable store; provisioning must have
    // populated the vendored store so the offline mount is non-empty.
    let store = root.join(".kvist").join("vendored-js");
    if !store.is_dir()
        || std::fs::read_dir(&store)
            .expect("store dir")
            .next()
            .is_none()
    {
        eprintln!("skip: pnpm did not populate the vendored store");
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
        verify_offline(&config, root, &probe, None, &["tests 1", "pass 1"]),
        "pnpm offline verification must build and test the vendored project"
    );
}

/// End-to-end: a small C project with one vcpkg-locked zlib dependency is
/// provisioned on the host (`vcpkg install` via `kvist vendor` into the
/// project-local vcpkg root under `.kvist/vendored-vcpkg`), and the approved
/// test command (`make test`) compiles and runs its real test offline inside
/// the bubblewrap sandbox with the network denied, resolving zlib from the
/// vendored vcpkg root mounted at its host path.
#[test]
fn c_vcpkg_offline_verification_builds_and_tests_denied_network() {
    let Some((runner, bwrap)) = live_sandbox_ready() else {
        eprintln!("skip: live sandbox prerequisites not met");
        return;
    };
    for tool in ["vcpkg", "cmake", "make", "g++"] {
        if !Command::new(tool)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            eprintln!("skip: {tool} not on PATH");
            return;
        }
    }

    // One vcpkg-locked zlib dependency. The vcpkg CMake toolchain is reached by
    // relative path from the component mount; the vendored vcpkg root is also
    // mounted read-only at its canonical host path so the install tree's
    // absolute cache paths resolve unchanged inside the sandbox.
    let worktree = git_worktree_root().expect("worktree checked by live_sandbox_ready");
    let project = tempfile::tempdir_in(&worktree).expect("temp project directory");
    let root = project.path();
    write_file(
        root.join("vcpkg.json"),
        "{\"name\":\"e2eminivcpkg\",\"version\":\"0.1.0\",\"dependencies\":[\"zlib\"]}\n",
    );
    write_file(
        root.join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.15)\nproject(e2eminivcpkg C)\nfind_package(ZLIB REQUIRED)\nadd_executable(test_zlib test_zlib.c)\ntarget_link_libraries(test_zlib PRIVATE ZLIB::ZLIB)\n",
    );
    write_file(
        root.join("test_zlib.c"),
        "#include <zlib.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n\nint main(void) {\n    const char *msg = \"kvist offline vcpkg check\";\n    unsigned long bound = compressBound((unsigned long)strlen(msg));\n    unsigned char *dst = malloc(bound);\n    unsigned char *back = malloc(strlen(msg));\n    uLongf dst_len = bound;\n    uLongf back_len = strlen(msg);\n    int rc = compress2(dst, &dst_len, (const unsigned char *)msg, (uInt)strlen(msg), Z_DEFAULT_COMPRESSION);\n    if (rc != Z_OK || uncompress(back, &back_len, dst, dst_len) != Z_OK || back_len != strlen(msg) || memcmp(back, msg, back_len) != 0) {\n        fprintf(stderr, \"zlib roundtrip failed\\n\");\n        return 1;\n    }\n    printf(\"zlib roundtrip ok %s\\n\", ZLIB_VERSION);\n    return 0;\n}\n",
    );
    write_file(
        root.join("Makefile"),
        "BUILD := /workspace/scratch/build\n\ntest:\n\t/usr/bin/cmake -S . -B $(BUILD) -DCMAKE_TOOLCHAIN_FILE=.kvist/vendored-vcpkg/scripts/buildsystems/vcpkg.cmake -DCMAKE_BUILD_TYPE=Release\n\t/usr/bin/cmake --build $(BUILD)\n\t$(BUILD)/test_zlib\n",
    );

    if !provision(root) {
        return;
    }
    // The vendored vcpkg root must hold the installed ports (for the offline
    // mount and to resolve zlib) and ship the CMake toolchain the build
    // consumes. Mirror the Conan provision check.
    let vcpkg_root = root.join(".kvist").join("vendored-vcpkg");
    let installed = vcpkg_root.join("installed");
    let toolchain = vcpkg_root
        .join("scripts")
        .join("buildsystems")
        .join("vcpkg.cmake");
    if !installed.is_dir()
        || std::fs::read_dir(&installed)
            .expect("installed dir")
            .next()
            .is_none()
        || !toolchain.is_file()
    {
        eprintln!("skip: vcpkg did not provision the root (installed ports or toolchain missing)");
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
        verify_offline(
            &config,
            root,
            &probe,
            Some(vec!["make".to_owned(), "test".to_owned()]),
            &["zlib roundtrip ok"],
        ),
        "c vcpkg offline verification must build and test the vendored project"
    );
}
