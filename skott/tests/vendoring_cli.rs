//! Explicit operator provisioning and real offline workspace consumption.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sav::{CancellationToken, ToolIntent};
use serde_json::json;
use skott::session::ToolExecutor;
use skott::{ProfileSetting, SandboxExecutor, SandboxPaths, ToolPolicy, ToolProfile, ToolRegistry};

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"vendor-cli-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("Cargo.lock"),
        "version=4\n[[package]]\nname=\"vendor-cli-trial\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "#[test] fn works() { assert_eq!(2+2,4); }\n",
    )
    .unwrap();
    dir
}

#[test]
fn shared_vendor_policy_is_used_without_model_configuration() {
    let dir = project();
    fs::create_dir(dir.path().join(".kvist")).unwrap();
    fs::write(
        dir.path().join(".kvist/vendor.toml"),
        "[limits]\nmax_entries=0\n",
    )
    .unwrap();
    for args in [&["--vendor", "--check"][..], &["--vendor", "--offline"][..]] {
        let out = cli(dir.path(), args);
        assert!(!out.status.success(), "{out:?}");
        assert!(!dir.path().join(".kvist/vendor.json").exists());
        assert!(!dir.path().join(".kvist/vendored").exists());
    }
}
fn cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skott"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CONFIG_DIRS", dir.join("system-config"))
        .output()
        .unwrap()
}

fn success(out: &Output) {
    assert!(out.status.success(), "{out:?}");
}

fn rust_registry(root: &Path) -> skott::Result<ToolRegistry> {
    let settings = ToolProfile::CONFIGURABLE
        .iter()
        .map(|profile| {
            (
                *profile,
                if *profile == ToolProfile::Rust {
                    ProfileSetting::On
                } else {
                    ProfileSetting::Off
                },
            )
        })
        .collect();
    ToolRegistry::resolve_for_workspace(
        ToolPolicy::minimum(),
        &settings,
        &skott::toolchain::HostProbe,
        None,
        root,
    )
}

#[test]
fn managed_publication_fences_and_missing_locks_do_not_block_workspace_tools() {
    let dir = project();
    success(&cli(dir.path(), &["--vendor", "--offline"]));
    let root = dir.path().canonicalize().unwrap();
    let operation = root.join(".kvist/vendor-operation.lock");
    fs::write(&operation, "active or interrupted operator").unwrap();
    let registry = rust_registry(&root).unwrap();
    assert!(registry.profiles().contains(&"rust"));
    assert_eq!(
        fs::read_to_string(&operation).unwrap(),
        "active or interrupted operator"
    );
    let check = cli(&root, &["--vendor", "--check"]);
    assert!(
        !check.status.success(),
        "explicit check retains its publication boundary: {check:?}"
    );
    fs::remove_file(&operation).unwrap();
    fs::remove_file(root.join("Cargo.lock")).unwrap();
    let registry = rust_registry(&root).unwrap();
    assert!(registry.profiles().contains(&"rust"));
    assert!(
        !root.join("Cargo.lock").exists(),
        "preparation itself must not rewrite the project"
    );
}

#[test]
fn recovery_only_and_empty_resolver_state_do_not_hide_independent_rust() {
    for directory in [false, true] {
        let dir = project();
        fs::remove_file(dir.path().join("Cargo.lock")).unwrap();
        fs::create_dir(dir.path().join(".kvist")).unwrap();
        if directory {
            fs::create_dir(dir.path().join(".kvist/sandbox-cargo")).unwrap();
        } else {
            fs::write(
                dir.path().join(".kvist/vendor-publication"),
                "recovery fence",
            )
            .unwrap();
        }
        let root = dir.path().canonicalize().unwrap();
        let registry = rust_registry(&root).unwrap();
        assert!(registry.profiles().contains(&"rust"));
        assert!(!root.join("Cargo.lock").exists());
    }
}

#[test]
fn vendor_and_check_work_without_application_configuration_or_project_artifacts() {
    let dir = project();
    let lock = fs::read(dir.path().join("Cargo.lock")).unwrap();
    let check = cli(dir.path(), &["--vendor", "--check"]);
    success(&check);
    assert!(
        !dir.path().join(".kvist").exists(),
        "--check must not write"
    );
    let out = cli(dir.path(), &["--vendor", "--offline"]);
    success(&out);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("host"),
        "{out:?}"
    );
    let manifest = dir.path().join(".kvist/vendor.json");
    assert!(manifest.is_file(), "{out:?}");
    let recorded = fs::read(&manifest).unwrap();
    success(&cli(dir.path(), &["--vendor", "--offline"]));
    assert_eq!(
        fs::read(&manifest).unwrap(),
        recorded,
        "ready reuse must not rewrite state"
    );
    success(&cli(dir.path(), &["--vendor", "--check"]));
    use std::os::unix::fs::MetadataExt;
    let previous = fs::metadata(dir.path().join(".kvist/vendored"))
        .unwrap()
        .ino();
    success(&cli(dir.path(), &["--vendor", "--offline", "--force"]));
    assert_ne!(
        fs::metadata(dir.path().join(".kvist/vendored"))
            .unwrap()
            .ino(),
        previous
    );
    assert_eq!(fs::read(dir.path().join("Cargo.lock")).unwrap(), lock);
    for absent in ["skott.toml", "kvist.toml", "VISION.md", ".skott", ".cargo"] {
        assert!(!dir.path().join(absent).exists(), "{absent}");
    }
}

#[test]
fn explicit_vendor_directory_does_not_use_a_foreign_configuration_workspace() {
    let launch = tempfile::tempdir().unwrap();
    let dir = project();
    let out = cli(
        launch.path(),
        &[
            "--vendor",
            "--offline",
            "--cwd",
            dir.path().to_str().unwrap(),
        ],
    );
    success(&out);
    assert!(dir.path().join(".kvist/vendor.json").is_file());
    assert_eq!(fs::read_dir(launch.path()).unwrap().count(), 0);
    let invalid = cli(
        launch.path(),
        &["--vendor", "--config", "/absent/foreign.toml"],
    );
    assert!(!invalid.status.success());
    assert!(!dir.path().join("skott.toml").exists());
}

#[test]
fn missing_malformed_stale_and_nonrust_vendor_states_are_actionable_and_read_only() {
    let dir = project();
    fs::remove_file(dir.path().join("Cargo.lock")).unwrap();
    for args in [&["--vendor", "--check"][..], &["--vendor", "--offline"][..]] {
        let out = cli(dir.path(), args);
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("Cargo.lock"),
            "{out:?}"
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("Resolve Cargo.lock"),
            "{out:?}"
        );
        assert!(!dir.path().join(".kvist").exists());
    }
    fs::write(dir.path().join("Cargo.lock"), "not TOML").unwrap();
    let out = cli(dir.path(), &["--vendor", "--check"]);
    assert!(!out.status.success());
    assert!(!dir.path().join(".kvist").exists());
    fs::write(dir.path().join("Cargo.lock"), "version=4\n").unwrap();
    success(&cli(dir.path(), &["--vendor", "--offline"]));
    let manifest = fs::read(dir.path().join(".kvist/vendor.json")).unwrap();
    fs::write(dir.path().join("Cargo.lock"), "version=4\n# changed\n").unwrap();
    let out = cli(dir.path(), &["--vendor", "--check"]);
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("stale"),
        "{out:?}"
    );
    assert_eq!(
        fs::read(dir.path().join(".kvist/vendor.json")).unwrap(),
        manifest
    );
    let nonrust = tempfile::tempdir().unwrap();
    fs::write(nonrust.path().join("package-lock.json"), "{}").unwrap();
    let out = cli(nonrust.path(), &["--vendor"]);
    assert!(!out.status.success());
    assert!(!nonrust.path().join(".kvist").exists());
}

#[test]
fn provisioning_flags_cannot_be_used_as_agent_execution_flags() {
    let dir = project();
    for args in [
        vec!["--check"],
        vec!["--offline"],
        vec!["--vendor", "--check", "--offline"],
        vec!["--vendor", "--check", "--force"],
        vec!["--vendor", "--setup"],
        vec!["--vendor", "--init-config"],
        vec!["--vendor", "--headless", "task"],
        vec!["--vendor", "--allow-host-execution"],
        vec!["--vendor", "--model", "coder"],
        vec!["--vendor", "--attach", "journal"],
        vec!["--vendor", "task"],
    ] {
        let out = cli(dir.path(), &args);
        assert!(!out.status.success(), "{args:?}: {out:?}");
    }
    assert!(!dir.path().join(".kvist").exists());
}

fn host_cargo() -> PathBuf {
    let out = Command::new("/usr/bin/rustup")
        .args(["which", "cargo"])
        .env("RUSTUP_AUTO_INSTALL", "0")
        .output()
        .unwrap();
    success(&out);
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

fn checked(command: &mut Command) {
    success(&command.output().unwrap());
}

#[test]
fn local_git_dependency_is_provisioned_once_and_builds_in_the_real_offline_sandbox() {
    let infrastructure = tempfile::tempdir().unwrap();
    let dep = infrastructure.path().join("dependency");
    fs::create_dir(&dep).unwrap();
    fs::write(dep.join("Cargo.toml"), "[package]\nname=\"offline-local-git\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[lib]\npath=\"lib.rs\"\n").unwrap();
    fs::write(dep.join("lib.rs"), "pub fn answer()->u8 {42}\n").unwrap();
    checked(Command::new("git").args(["init", "--quiet"]).arg(&dep));
    checked(
        Command::new("git")
            .args(["add", "Cargo.toml", "lib.rs"])
            .current_dir(&dep),
    );
    checked(
        Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture\n\nCo-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>",
            ])
            .current_dir(&dep),
    );
    let rev = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dep)
        .output()
        .unwrap();
    success(&rev);
    let rev = String::from_utf8(rev.stdout).unwrap();
    let dir = project();
    let uri = format!("file://{}", dep.display());
    let mut manifest = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    manifest.push_str(&format!(
        "\n[dependencies]\noffline-local-git={{git=\"{uri}\",rev=\"{}\"}}\n",
        rev.trim()
    ));
    fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "#[test] fn works(){assert_eq!(offline_local_git::answer(),42);}\n",
    )
    .unwrap();
    let cache = infrastructure.path().join("cargo-home");
    // The only dependency acquisition is from this owned local Git fixture.
    checked(
        Command::new(host_cargo())
            .args(["generate-lockfile"])
            .env("CARGO_HOME", &cache)
            .env("CARGO_NET_OFFLINE", "false")
            .current_dir(dir.path()),
    );
    let lock = fs::read(dir.path().join("Cargo.lock")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_skott"))
        .args(["--vendor", "--offline"])
        .env("CARGO_HOME", &cache)
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    success(&out);
    success(&cli(dir.path(), &["--vendor", "--check"]));
    fs::create_dir(dir.path().join(".cargo")).unwrap();
    fs::write(dir.path().join(".cargo/config.toml"), "[source.crates-io]\nreplace-with=\"vendored-sources\"\n[source.vendored-sources]\ndirectory=\"/not/the/vendor\"\n").unwrap();
    let root = dir.path().canonicalize().unwrap();
    let registry = rust_registry(&root).unwrap();
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: std::env::var_os("KVIST_GALLA")
                .map(PathBuf::from)
                .unwrap_or_else(SandboxPaths::default_runner),
            backend: SandboxPaths::default_backend(),
        },
        root,
    );
    let out = executor.execute(&ToolIntent {
        id: "offline-build".into(), provider_id: None, name: "shell".into(),
        arguments: json!({"command":"cargo test && cargo clippy --all-targets && cargo doc --no-deps && test ! -w /rust/vendor && test -r .kvist/vendored && test ! -w .kvist/vendored"}),
    }, &CancellationToken::new()).unwrap();
    assert!(!out.failed(), "{}", out.error_text(8192));
    assert_eq!(fs::read(dir.path().join("Cargo.lock")).unwrap(), lock);
    assert!(dir.path().join("target/debug/deps").is_dir());
    assert!(!dir.path().join("skott.toml").exists());
    fs::write(
        dir.path().join(".kvist/vendored/offline-local-git/lib.rs"),
        "tampered",
    )
    .unwrap();
    success(&cli(dir.path(), &["--vendor", "--check"]));
    let damaged = executor
        .execute(
            &ToolIntent {
                id: "native-checksum-failure".into(),
                provider_id: None,
                name: "shell".into(),
                arguments: json!({"command":"cargo clean && cargo test"}),
            },
            &CancellationToken::new(),
        )
        .unwrap();
    assert!(damaged.failed());
    assert!(
        damaged.error_text(8192).contains("checksum"),
        "{}",
        damaged.error_text(8192)
    );
    let repaired = Command::new(env!("CARGO_BIN_EXE_skott"))
        .args(["--vendor", "--offline", "--force"])
        .env("CARGO_HOME", &cache)
        .current_dir(dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    success(&repaired);
    assert_eq!(
        fs::read_to_string(dir.path().join(".kvist/vendored/offline-local-git/lib.rs")).unwrap(),
        "pub fn answer()->u8 {42}\n"
    );
}
