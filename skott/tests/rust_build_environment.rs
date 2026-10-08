//! Opt-in native isolation trials; no provider or host dependency acquisition.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use skott::session::ToolExecutor;
use skott::toolchain::HostProbe;
use skott::{
    ProfileSetting, SandboxExecutor, SandboxPaths, ToolPolicy, ToolProfile, ToolRegistry,
};
use sav::{CancellationToken, ToolIntent};
use serde_json::json;

fn fixture() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".rust-environment-test-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap()
}

fn runner() -> PathBuf {
    std::env::var_os("KVIST_RUST_TEST_RUNNER")
        .map(PathBuf::from)
        .expect("set KVIST_RUST_TEST_RUNNER to an independently built runner outside the fixture")
}

/// Whether the trusted system rustup that `RustEnvironment::resolve` requires is
/// present. The "absent install" trial asserts a version-specific, actionable
/// error for a pin whose toolchain is not installed; that error is only produced
/// once an installed host rustup at `/usr/bin/rustup` has been queried. CI's
/// toolcache rustup (dtolnay/rust-toolchain) does not live there, so the trial
/// gates on this and skips rather than failing for the wrong reason.
fn host_rustup_resolves() -> bool {
    std::fs::canonicalize("/usr/bin/rustup").is_ok()
}

fn resolve(workdir: &std::path::Path) -> skott::Result<ToolRegistry> {
    let settings = ToolProfile::CONFIGURABLE
        .iter()
        .map(|p| {
            (
                *p,
                if *p == ToolProfile::Rust {
                    ProfileSetting::On
                } else {
                    ProfileSetting::Off
                },
            )
        })
        .collect();
    ToolRegistry::resolve_for_workspace(ToolPolicy::default(), &settings, &HostProbe, None, workdir)
}

#[test]
fn rejects_malicious_ambiguous_and_oversized_pins() {
    for pin in [
        "[toolchain]\nchannel = \"../../outside\"",
        "[toolchain]\nchannel = \"/outside/toolchain\"",
        "[toolchain]\nchannel = \"--help\"",
        "[toolchain]\nchannel = \"stable\"\npath = \"/outside\"",
        "[toolchain]\nchannel = \"stable\"\ntargets = [\"../../outside\"]",
        "[toolchain]\nchannel = \"stable\"\ncomponents = [\"../../outside\"]",
    ] {
        let directory = fixture();
        fs::write(directory.path().join("rust-toolchain.toml"), pin).unwrap();
        assert!(resolve(directory.path()).is_err(), "{pin}");
    }
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        " ".repeat(65537),
    )
    .unwrap();
    assert!(resolve(directory.path()).is_err());
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel=\"stable\"",
    )
    .unwrap();
    fs::write(directory.path().join("rust-toolchain"), "stable").unwrap();
    assert!(resolve(directory.path()).is_err());
}

#[test]
fn absent_install_is_actionable_and_auto_is_honest() {
    if !host_rustup_resolves() {
        eprintln!(
            "skip absent_install_is_actionable_and_auto_is_honest: no /usr/bin/rustup; \
             asserting a version-specific 'absent install' error requires an installed host rustup"
        );
        return;
    }
    let directory = fixture();
    fs::write(directory.path().join("rust-toolchain"), "9.99.99").unwrap();
    let error = resolve(directory.path()).unwrap_err().to_string();
    assert!(
        error.contains("host") && error.contains("9.99.99"),
        "{error}"
    );
    let registry = ToolRegistry::resolve_for_workspace(
        ToolPolicy::default(),
        &BTreeMap::new(),
        &HostProbe,
        None,
        directory.path(),
    )
    .unwrap();
    assert!(!registry.profiles().contains(&"rust"));
    assert!(registry.diagnostics().iter().any(|s| s.contains("9.99.99")));
}

#[test]
fn symlink_pin_and_vendor_roots_cannot_acquire_host_authority() {
    use std::os::unix::fs::symlink;
    let directory = fixture();
    let outside = fixture();
    fs::write(outside.path().join("pin"), "stable").unwrap();
    symlink(
        outside.path().join("pin").canonicalize().unwrap(),
        directory.path().join("rust-toolchain"),
    )
    .unwrap();
    assert!(resolve(directory.path()).is_err());
    fs::remove_file(directory.path().join("rust-toolchain")).unwrap();
    fs::create_dir(directory.path().join(".kvist")).unwrap();
    symlink(
        outside.path().canonicalize().unwrap(),
        directory.path().join(".kvist/vendored"),
    )
    .unwrap();
    assert!(resolve(directory.path()).is_err());
}

fn shell(executor: &SandboxExecutor, command: &str) -> skott::ToolOutcome {
    executor
        .execute(
            &ToolIntent {
                id: "rust-trial".into(),
                provider_id: None,
                name: "shell".into(),
                arguments: json!({"command": command}),
            },
            &CancellationToken::new(),
        )
        .unwrap()
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn native_vendor_snapshot_overrides_host_paths_without_credentials_or_mutation() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::create_dir_all(workdir.join("src")).unwrap();
    fs::create_dir_all(workdir.join(".cargo")).unwrap();
    fs::create_dir_all(workdir.join(".kvist/vendored")).unwrap();
    let crate_source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.kvist/vendored/hex");
    fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
        fs::create_dir(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                assert!(entry.file_type().unwrap().is_file());
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    copy_tree(&crate_source, &workdir.join(".kvist/vendored/hex"));
    fs::write(workdir.join("Cargo.toml"), "[package]\nname=\"vendor-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[dependencies]\nhex=\"=0.4.3\"\n").unwrap();
    fs::write(
        workdir.join("src/lib.rs"),
        "#[test] fn dependency() { assert_eq!(hex::encode(b\"ok\"), \"6f6b\"); }\n",
    )
    .unwrap();
    fs::write(workdir.join("Cargo.lock"), "version=4\n[[package]]\nname=\"vendor-trial\"\nversion=\"0.1.0\"\ndependencies=[\"hex\"]\n[[package]]\nname=\"hex\"\nversion=\"0.4.3\"\nsource=\"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum=\"7f24254aa9a54b5c858eaee2f5bccdb46aaf0e486a595ed5fd8f86ba55232a70\"\n").unwrap();
    // Neither this repository path nor a forged maerg manifest authorizes a
    // host bind. The shim replaces the source with its private sandbox path.
    fs::write(workdir.join(".cargo/config.toml"), "[source.crates-io]\nreplace-with=\"vendored-sources\"\n[source.vendored-sources]\ndirectory=\"/home/stefan/.cargo/credentials.toml\"\n").unwrap();
    fs::write(
        workdir.join(".kvist/rust-toolchain.json"),
        "{\"root\":\"/home/stefan\",\"cargo\":\"/home/stefan/.cargo/bin/cargo\"}",
    )
    .unwrap();
    let registry = resolve(&workdir).unwrap();
    let diagnostics = registry.diagnostics().join("\n");
    assert!(diagnostics.contains("read-only snapshot"));
    assert!(diagnostics.contains("vendor snapshot sha256:"));
    // Changes through the writable workspace alias cannot mutate the resource.
    fs::write(
        workdir.join(".kvist/vendored/hex/Cargo.toml"),
        "malicious replacement",
    )
    .unwrap();
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    let host_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let network_trial = shell(
        &executor,
        &format!(
            "if (exec 3<>/dev/tcp/127.0.0.1/{}) 2>/dev/null; then exit 9; fi",
            host_listener.local_addr().unwrap().port()
        ),
    );
    assert!(
        !network_trial.failed(),
        "host networking must remain unreachable"
    );
    let result = shell(
        &executor,
        "test \"$CARGO_NET_OFFLINE\" = true && test \"$CARGO_HOME\" = /tmp/cargo-home && test \"$CARGO_TARGET_DIR\" = /tmp/target && test \"$HOME\" = /tmp && test ! -e /home/stefan/.cargo/credentials.toml && test ! -e /home/stefan/.rustup/settings.toml && test ! -e /root/.cargo && test ! -e /etc/resolv.conf && test -z \"$CARGO_REGISTRY_TOKEN\" && test -z \"$RUSTUP_HOME\" && test -z \"$RUSTUP_TOOLCHAIN\" && ! test -w /rust/runtime/bin/cargo && ! test -w /rust/toolchain/bin/rustc && ! test -w /rust/vendor/hex/Cargo.toml && cargo metadata --offline --locked --format-version=1 >/dev/null && cargo test --offline --locked && cargo doc --offline --locked --no-deps",
    );
    assert!(!result.failed(), "{}", result.error_text(8192));
    assert!(result.output_text(8192).contains("test result: ok"));
    assert!(!workdir.join("target").exists());
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn changing_project_pin_after_resolution_fails_before_effects() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::write(workdir.join("rust-toolchain"), "stable\n").unwrap();
    let registry = resolve(&workdir).unwrap();
    fs::write(workdir.join("rust-toolchain"), "9.99.99\n").unwrap();
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    let result = executor.execute(
        &ToolIntent {
            id: "drift".into(),
            provider_id: None,
            name: "shell".into(),
            arguments: json!({"command":"touch should-not-exist"}),
        },
        &CancellationToken::new(),
    );
    assert!(
        result.is_err(),
        "changed project pin must not silently keep using the old compiler"
    );
    assert!(!workdir.join("should-not-exist").exists());
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn native_installed_rust_compiles_links_and_documents_offline() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::create_dir(workdir.join("src")).unwrap();
    fs::write(
        workdir.join("Cargo.toml"),
        "[package]\nname=\"rust-isolation-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(
        workdir.join("src/lib.rs"),
        "/// ```\n/// assert_eq!(rust_isolation_trial::answer(), 42);\n/// ```\npub fn answer() -> u8 { 42 }\n#[test] fn works() { assert_eq!(answer(), 42); }\n",
    )
    .unwrap();
    fs::write(
        workdir.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"rust-isolation-trial\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let registry = resolve(&workdir).unwrap();
    assert!(registry.profiles().contains(&"rust"));
    assert!(registry.diagnostics().iter().any(|s| s.contains("sha256:")));
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: PathBuf::from("/usr/bin/bwrap"),
        },
        workdir,
    );
    let result = executor
        .execute(
            &ToolIntent {
                id: "native-rust".into(),
                provider_id: None,
                name: "shell".into(),
                arguments: json!({"command": "cargo --version && rustc --version && rustdoc --version && cc --version >/dev/null && cargo test --offline --locked && cargo doc --offline --locked --no-deps"}),
            },
            &CancellationToken::new(),
        )
        .unwrap();
    assert!(!result.failed(), "{}", result.error_text(8192));
    assert!(result.output_text(8192).contains("test result: ok"));
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn normal_cargo_requires_matching_lock_without_creating_or_updating_it() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::create_dir(workdir.join("src")).unwrap();
    fs::write(
        workdir.join("Cargo.toml"),
        "[package]\nname=\"locked-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(workdir.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n").unwrap();
    let executor = SandboxExecutor::new(
        resolve(&workdir).unwrap(),
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    let lock = workdir.join("Cargo.lock");
    let missing = shell(&executor, "cargo check");
    assert!(missing.failed(), "a missing lock must prevent the build");
    assert!(missing.error_text(8192).contains("--locked"));
    assert!(!lock.exists(), "a locked build must not create Cargo.lock");

    let stale = b"version = 4\n[[package]]\nname = \"locked-trial\"\nversion = \"0.0.1\"\n";
    fs::write(&lock, stale).unwrap();
    for command in ["cargo build", "cargo metadata --format-version=1"] {
        let result = shell(&executor, command);
        assert!(result.failed(), "{command} must reject an outdated lock");
        assert!(result.error_text(8192).contains("--locked"));
        assert_eq!(fs::read(&lock).unwrap(), stale);
    }

    let matching = b"version = 4\n[[package]]\nname = \"locked-trial\"\nversion = \"0.1.0\"\n";
    fs::write(&lock, matching).unwrap();
    let result = shell(
        &executor,
        "cargo --version && cargo check && cargo metadata --format-version=1 --no-deps >/dev/null",
    );
    assert!(!result.failed(), "{}", result.error_text(8192));
    assert_eq!(fs::read(&lock).unwrap(), matching);
    assert!(!workdir.join("target").exists());
}
