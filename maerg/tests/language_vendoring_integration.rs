//! Native default-runtime tests use isolated Cargo homes in child processes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use maerg::language_vendoring::{detect_language_strategy, enforce_vendoring, lockfile_digest};

fn project() -> tempfile::TempDir {
    let project = tempfile::Builder::new()
        .prefix(".native-language-test-")
        .tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("owned native fixture");
    fs::write(
        project.path().join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"native-language-fixture\"\nversion = \"0.1.0\"\n",
    )
    .expect("path-only lock");
    fs::create_dir(project.path().join("cargo-home")).expect("isolated native home");
    project
}

fn child(project: &Path, mode: &str, home: &Path) {
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "native_language_child", "--nocapture"])
        .env("KVIST_NATIVE_LANGUAGE_TEST", mode)
        .env("KVIST_NATIVE_LANGUAGE_PROJECT", project)
        .env("CARGO_HOME", home)
        .output()
        .expect("isolated child process");
    assert!(
        output.status.success(),
        "native child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn native_language_ignores_export_state_and_needs_no_generated_config() {
    let project = project();
    fs::create_dir(project.path().join(".kvist")).expect("fixture state");
    fs::write(
        project.path().join(".kvist/vendor.json"),
        "unrelated export receipt",
    )
    .expect("receipt");
    fs::write(
        project.path().join(".kvist/vendor-operation.lock"),
        "retained export state",
    )
    .expect("export state");
    child(
        project.path(),
        "path-only",
        &project.path().join("cargo-home"),
    );
    assert_eq!(
        fs::read_to_string(project.path().join(".kvist/vendor.json")).expect("retained receipt"),
        "unrelated export receipt"
    );
    assert!(!project.path().join(".kvist/sandbox-cargo").exists());
    assert!(!project.path().join(".kvist/cargo-cache.json").exists());
}

#[test]
fn native_language_mounts_only_existing_data_roots_not_home_or_credentials() {
    for namespaces in [&["registry"][..], &["git"][..], &["registry", "git"][..]] {
        let project = project();
        let home = project.path().join("cargo-home");
        for namespace in namespaces {
            fs::create_dir(home.join(namespace)).expect("empty native namespace");
        }
        fs::write(home.join("config.toml"), "# private host configuration\n").expect("host config");
        fs::write(
            home.join("credentials.toml"),
            "# private host credentials\n",
        )
        .expect("host credentials");
        child(project.path(), "mounts", &home);
        assert!(!project.path().join(".kvist").exists());
    }
}

#[test]
fn native_language_reports_uncached_real_locked_package() {
    let project = project();
    let workspace_lock = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("Cargo.lock");
    let lock: toml::Value =
        toml::from_str(&fs::read_to_string(workspace_lock).expect("actual workspace lock"))
            .expect("valid lock");
    let package = lock["package"]
        .as_array()
        .expect("locked packages")
        .iter()
        .find(|package| package["name"].as_str() == Some("serde"))
        .expect("real locked serde");
    let mut package = package.as_table().expect("package table").clone();
    package.remove("dependencies");
    fs::write(
        project.path().join("Cargo.lock"),
        format!(
            "version = 4\n[[package]]\n{}",
            toml::to_string(&package).expect("real lock entry")
        ),
    )
    .expect("actual registry identity, not fabricated checksum");
    child(
        project.path(),
        "missing",
        &project.path().join("cargo-home"),
    );
}

#[test]
fn native_language_refuses_changed_lock_mount_identity() {
    let project = project();
    child(
        project.path(),
        "wrong-identity",
        &project.path().join("cargo-home"),
    );
}

#[test]
fn native_language_invalid_explicit_home_never_falls_back() {
    let project = project();
    child(
        project.path(),
        "invalid-home",
        Path::new("relative-cargo-home"),
    );
}

#[test]
fn native_language_noncanonical_inspection_requires_explicit_admission_context() {
    let project = project();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let output = Command::new("git")
        .args([
            "-C",
            workspace.to_str().expect("workspace path"),
            "rev-parse",
            "HEAD",
        ])
        .output()
        .expect("real repository revision");
    assert!(output.status.success());
    let revision = String::from_utf8(output.stdout)
        .expect("revision")
        .trim()
        .to_owned();
    fs::write(project.path().join("Cargo.lock"), format!(
        "version = 4\n[[package]]\nname = \"kvist\"\nversion = \"0.3.1\"\nsource = \"git+file://{}?rev={revision}#{revision}\"\n",
        workspace.display(),
    )).expect("actual pinned repository identity");
    child(
        project.path(),
        "needs-context",
        &project.path().join("cargo-home"),
    );
    fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname = \"native-language-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("native manifest");
    for namespace in ["git/db", "git/checkouts"] {
        fs::create_dir_all(project.path().join("cargo-home").join(namespace))
            .expect("partial native data roots");
    }
    child(
        project.path(),
        "configured-context",
        &project.path().join("cargo-home"),
    );
}

#[test]
fn native_language_child() {
    let Ok(mode) = std::env::var("KVIST_NATIVE_LANGUAGE_TEST") else {
        return;
    };
    let root =
        PathBuf::from(std::env::var_os("KVIST_NATIVE_LANGUAGE_PROJECT").expect("fixture root"));
    let home = PathBuf::from(std::env::var_os("CARGO_HOME").expect("isolated home"));
    let strategy = detect_language_strategy(&root).expect("Rust strategy");
    match mode.as_str() {
        "path-only" | "mounts" => {
            let enforcement = enforce_vendoring(&root, false, None).expect("native enforcement");
            assert_eq!(enforcement.language, "rust");
            assert_eq!(enforcement.vendored_dir, home);
            assert_eq!(
                enforcement.lockfile_digest,
                lockfile_digest(&fs::read(root.join("Cargo.lock")).expect("lock"))
            );
            let expected: Vec<_> = ["registry", "git"]
                .into_iter()
                .filter(|namespace| home.join(namespace).is_dir())
                .collect();
            assert_eq!(enforcement.mounts.len(), expected.len());
            for namespace in expected {
                let mount = enforcement
                    .mounts
                    .iter()
                    .find(|mount| mount.source == home.join(namespace))
                    .expect("native data mount");
                assert_eq!(mount.destination, format!("/workspace/cargo-{namespace}"));
                assert_eq!(mount.identity, enforcement.lockfile_digest);
            }
        }
        "missing" => {
            let report = strategy
                .enforce(&root)
                .expect("incomplete native catalogue is a report");
            assert!(!report.ready());
            assert_eq!(report.present_dependencies, 0);
            assert!(!report.missing_dependencies.is_empty());
            assert_eq!(Path::new(&report.vendored_dir), home);
        }
        "wrong-identity" => {
            let error = strategy
                .mounts(&root, "sha256:wrong")
                .expect_err("lock identity must match native view");
            assert!(error.to_string().contains("lock"), "{error}");
        }
        "invalid-home" => {
            let error = strategy
                .enforce(&root)
                .expect_err("relative explicit home is invalid");
            assert!(error.to_string().contains("CARGO_HOME"), "{error}");
        }
        "needs-context" => {
            let error = strategy
                .enforce(&root)
                .expect_err("direct inspection must not authorize host source admission");
            assert!(
                error
                    .to_string()
                    .contains("explicit native source admission context"),
                "{error}"
            );
        }
        "configured-context" => {
            let sandbox = maerg::config::SandboxConfig {
                runner: root
                    .join("missing-native-runner")
                    .to_string_lossy()
                    .into_owned(),
                backend: "/usr/bin/bwrap".into(),
                environment_allowlist: Vec::new(),
                acquisition: Default::default(),
            };
            let error = enforce_vendoring(&root, false, Some(&sandbox))
                .err()
                .expect("configured admission cannot use a host fallback");
            assert!(
                error.to_string().contains("missing-native-runner"),
                "selected sandbox context was not honored: {error}"
            );
        }
        _ => panic!("unknown child mode"),
    }
}
