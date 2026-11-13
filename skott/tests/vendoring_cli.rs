//! Explicit native cache provisioning and actual offline sandbox consumption.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sav::{CancellationToken, ToolIntent};
use serde_json::json;
use skott::session::ToolExecutor;
use skott::{ProfileSetting, SandboxExecutor, SandboxPaths, ToolPolicy, ToolProfile, ToolRegistry};

fn fixture() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".native-cache-cli-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap()
}

fn project() -> tempfile::TempDir {
    let dir = fixture();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname=\"cache-cli-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("Cargo.lock"),
        "version=4\n[[package]]\nname=\"cache-cli-trial\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("src/lib.rs"), "#[test] fn works() {}\n").unwrap();
    dir
}

fn cli(dir: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skott"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("CARGO_HOME", cache)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CONFIG_DIRS", dir.join("system-config"))
        .output()
        .unwrap()
}

fn success(out: &Output) {
    assert!(out.status.success(), "{out:?}");
}

fn no_vendor_state(root: &Path) {
    for path in [
        ".kvist/vendored",
        ".kvist/vendor-operation.lock",
        ".kvist/sandbox-cargo/config.toml",
        ".kvist/vendor.json",
    ] {
        assert!(!root.join(path).exists(), "{path}");
    }
}

fn rust_registry(root: &Path) -> ToolRegistry {
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
    .unwrap()
}

#[test]
fn native_cache_reuse_does_not_create_project_vendor_state() {
    let dir = project();
    let cache = fixture();
    let lock = fs::read(dir.path().join("Cargo.lock")).unwrap();
    for args in [
        &["--vendor", "--check"][..],
        &["--vendor", "--offline"][..],
        &["--vendor", "--offline"][..],
        &["--vendor", "--check"][..],
        &["--vendor", "--offline", "--force"][..],
    ] {
        success(&cli(dir.path(), cache.path(), args));
        no_vendor_state(dir.path());
    }
    assert_eq!(fs::read(dir.path().join("Cargo.lock")).unwrap(), lock);
    assert_eq!(
        fs::read_dir(cache.path()).unwrap().count(),
        0,
        "dependency-free reuse must not create administrative host state"
    );
}

#[test]
fn explicit_cache_directory_ignores_a_foreign_configuration_workspace() {
    let launch = fixture();
    let dir = project();
    let cache = fixture();
    success(&cli(
        launch.path(),
        cache.path(),
        &[
            "--vendor",
            "--offline",
            "--cwd",
            dir.path().to_str().unwrap(),
        ],
    ));
    assert_eq!(fs::read_dir(launch.path()).unwrap().count(), 0);
    no_vendor_state(dir.path());
    assert!(
        !cli(
            launch.path(),
            cache.path(),
            &["--vendor", "--config", "/absent/foreign.toml"]
        )
        .status
        .success()
    );
}

#[test]
fn missing_malformed_and_nonrust_locks_are_actionable_without_state_writes() {
    let dir = project();
    let cache = fixture();
    for contents in [None, Some("not TOML")] {
        if let Some(contents) = contents {
            fs::write(dir.path().join("Cargo.lock"), contents).unwrap();
        } else {
            fs::remove_file(dir.path().join("Cargo.lock")).unwrap();
        }
        for args in [&["--vendor", "--check"][..], &["--vendor", "--offline"][..]] {
            let out = cli(dir.path(), cache.path(), args);
            assert!(!out.status.success());
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("Cargo.lock"),
                "{out:?}"
            );
            assert!(!dir.path().join(".kvist").exists());
        }
    }
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
}

#[test]
fn native_cache_check_fails_for_missing_sources_without_acquisition() {
    let dir = project();
    let cache = fixture();
    fs::write(dir.path().join("Cargo.lock"),
        "version=4\n[[package]]\nname=\"cache-cli-trial\"\nversion=\"0.1.0\"\ndependencies=[\"absent-native-source\"]\n[[package]]\nname=\"absent-native-source\"\nversion=\"99.99.99\"\nsource=\"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum=\"0000000000000000000000000000000000000000000000000000000000000000\"\n").unwrap();
    let out = cli(dir.path(), cache.path(), &["--vendor", "--check"]);
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("absent-native-source"),
        "{out:?}"
    );
    assert!(!dir.path().join(".kvist").exists());
    assert_eq!(fs::read_dir(cache.path()).unwrap().count(), 0);
}

#[test]
fn provisioning_flags_are_not_agent_execution_flags() {
    let dir = project();
    let cache = fixture();
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
        assert!(
            !cli(dir.path(), cache.path(), &args).status.success(),
            "{args:?}"
        );
    }
    assert!(!dir.path().join(".kvist").exists());
}

/// Newer cargo writes test and dependency artifacts to
/// `debug/build/<package>/<hash>/out/` instead of `debug/deps/`; accept any
/// non-empty `debug` directory as evidence that the build produced artifacts.
fn debug_artifacts(target: &Path) -> bool {
    fs::read_dir(target.join("debug"))
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
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

fn shell(executor: &SandboxExecutor, command: &str) -> skott::ToolOutcome {
    executor
        .execute(
            &ToolIntent {
                id: "native-cache-runtime".into(),
                provider_id: None,
                name: "shell".into(),
                arguments: json!({"command": command}),
            },
            &CancellationToken::new(),
        )
        .unwrap()
}

#[test]
fn local_git_cache_builds_offline_with_private_admin_and_live_roots() {
    const PROJECT: &str = "SKOTT_NATIVE_CACHE_FIXTURE_PROJECT";
    if let Some(root) = std::env::var_os(PROJECT) {
        let root = PathBuf::from(root);
        let cache = PathBuf::from(std::env::var_os("CARGO_HOME").unwrap());
        let executor = SandboxExecutor::new(
            rust_registry(&root),
            SandboxPaths {
                runner: std::env::var_os("KVIST_GALLA")
                    .map(PathBuf::from)
                    .unwrap_or_else(SandboxPaths::default_runner),
                backend: SandboxPaths::default_backend(),
            },
            root.clone(),
        );
        let host_admin = fs::read(cache.join(".package-cache")).unwrap();
        let out = shell(
            &executor,
            "cargo test && cargo clippy --all-targets && cargo doc --no-deps && test -L \"$CARGO_HOME/git\" && test \"$(readlink \"$CARGO_HOME/git\")\" = /rust/cargo-git && test -w \"$CARGO_HOME\" && test ! -w /rust/cargo-git && touch \"$CARGO_HOME/admin-proof\" && test ! -e /rust/runtime/vendor-config.toml && test ! -e /rust/vendor && test ! -e \"$CARGO_HOME/credentials.toml\" && test ! -e \"$CARGO_HOME/config.toml\"",
        );
        assert!(!out.failed(), "{}", out.error_text(16384));
        assert!(debug_artifacts(&root.join("target")));
        assert_eq!(fs::read(cache.join(".package-cache")).unwrap(), host_admin);
        assert!(!cache.join("admin-proof").exists());
        no_vendor_state(&root);
        let git = cache.join("git");
        let retired = cache.join("retired-git");
        fs::rename(&git, &retired).unwrap();
        fs::create_dir(&git).unwrap();
        let missing = shell(&executor, "cargo clean && cargo test");
        assert!(missing.failed(), "missing Git data must fail offline");
        assert!(
            missing.error_text(16384).contains("offline"),
            "{}",
            missing.error_text(16384)
        );
        assert!(!shell(&executor, "rustc --version").failed());
        fs::remove_dir(&git).unwrap();
        fs::rename(retired, git).unwrap();
        let restored = shell(&executor, "cargo test");
        assert!(!restored.failed(), "{}", restored.error_text(16384));
        return;
    }

    let infrastructure = fixture();
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
    let dir = project();
    let mut manifest = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    manifest.push_str(&format!(
        "\n[dependencies]\noffline-local-git={{git=\"file://{}\",rev=\"{}\"}}\n",
        dep.display(),
        String::from_utf8(rev.stdout).unwrap().trim()
    ));
    fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "#[test] fn works(){assert_eq!(offline_local_git::answer(),42);}\n",
    )
    .unwrap();
    let cache = infrastructure.path().join("cargo-home");
    // Acquisition is explicitly confined to this owned local Git fixture.
    checked(
        Command::new(host_cargo())
            .args(["generate-lockfile"])
            .env("CARGO_HOME", &cache)
            .env("CARGO_NET_OFFLINE", "false")
            .current_dir(dir.path()),
    );
    fs::write(cache.join("credentials.toml"), "private sentinel").unwrap();
    fs::write(cache.join("config.toml"), "[alias]\nprivate=\"version\"\n").unwrap();
    let lock = fs::read(dir.path().join("Cargo.lock")).unwrap();
    success(&cli(dir.path(), &cache, &["--vendor", "--offline"]));
    let receipt = fs::read(dir.path().join(".kvist/cargo-cache.json")).unwrap();
    let admitted = cli(dir.path(), &cache, &["--vendor", "--check"]);
    success(&admitted);
    assert!(
        String::from_utf8_lossy(&admitted.stderr).contains("metadata --offline --locked"),
        "{admitted:?}"
    );
    assert_eq!(
        fs::read(dir.path().join(".kvist/cargo-cache.json")).unwrap(),
        receipt
    );
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "local_git_cache_builds_offline_with_private_admin_and_live_roots",
            "--nocapture",
        ])
        .env(PROJECT, dir.path().canonicalize().unwrap())
        .env("CARGO_HOME", &cache)
        .output()
        .unwrap();
    success(&out);
    assert_eq!(fs::read(dir.path().join("Cargo.lock")).unwrap(), lock);
    assert_eq!(
        fs::read_to_string(cache.join("credentials.toml")).unwrap(),
        "private sentinel"
    );
    fs::remove_dir_all(cache.join("git")).unwrap();
    let missing = cli(dir.path(), &cache, &["--vendor", "--check"]);
    assert!(
        !missing.status.success(),
        "a matching receipt cannot admit missing Git data"
    );
    assert_eq!(
        fs::read(dir.path().join(".kvist/cargo-cache.json")).unwrap(),
        receipt
    );
    assert_eq!(fs::read(dir.path().join("Cargo.lock")).unwrap(), lock);
}
