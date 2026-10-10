//! Opt-in native isolation trials; no provider or host dependency acquisition.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use sav::{CancellationToken, ToolIntent};
use serde_json::json;
use skott::session::ToolExecutor;
use skott::toolchain::HostProbe;
use skott::{ProfileSetting, SandboxExecutor, SandboxPaths, ToolPolicy, ToolProfile, ToolRegistry};

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

fn checked_shell(executor: &SandboxExecutor, command: &str) -> skott::ToolOutcome {
    let result = shell(
        executor,
        &format!(
            "stage() {{ label=\"$1\"; shift; printf 'stage: %s\\n' \"$label\"; \"$@\" || {{ printf 'FAILED stage: %s\\n' \"$label\" >&2; exit 1; }}; }}\n{command}"
        ),
    );
    assert!(
        !result.failed(),
        "stdout:\n{}\nstderr:\n{}",
        result.output_text(16384),
        result.error_text(16384)
    );
    result
}

#[derive(Clone)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn diagnostics_cover_the_chain_without_command_or_output_payloads() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let capture = LogCapture(bytes.clone());
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || capture.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let executor = SandboxExecutor::new(
            resolve(&workdir).unwrap(),
            SandboxPaths {
                runner: runner(),
                backend: "/usr/bin/bwrap".into(),
            },
            workdir,
        );
        let result = shell(&executor, "printf PRIVATE_COMMAND_OUTPUT_SENTINEL");
        assert!(!result.failed());
        assert_eq!(result.stdout, b"PRIVATE_COMMAND_OUTPUT_SENTINEL");
        let failed = shell(&executor, "exit 7");
        assert_eq!(failed.status, Some(7));
    });
    let logs = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    for stage in [
        "preparing offline Rust resources",
        "querying installed Rust selection",
        "installed Rust selection query completed",
        "resolved concrete installed Rust toolchain",
        "offline vendor snapshot prepared",
        "offline Rust resources prepared",
        "offline Rust resources validated",
        "sandbox request validated",
        "sandbox grant",
        "sandbox environment entry",
        "dispatching sandbox runner",
        "sandbox runner completed",
        "destination=/rust/toolchain",
        "status=Some(7)",
        "failed=true",
    ] {
        assert!(logs.contains(stage), "missing diagnostic {stage}: {logs}");
    }
    assert!(!logs.contains("PRIVATE_COMMAND_OUTPUT_SENTINEL"));
    assert!(!logs.contains("exit 7"));
    assert!(!logs.contains("RUSTUP_TOOLCHAIN=stable"));
}

#[test]
#[ignore = "requires this repository's host-provisioned vendor tree and native Bubblewrap"]
fn native_repository_tests_execute_inside_the_workspace_sandbox() {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            std::env::var("SKOTT_LOG").unwrap_or_else(|_| "warn".into()),
        ))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let workdir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap();
    assert!(
        workdir.join(".kvist/vendored").is_dir(),
        "provision repository dependencies on the host"
    );
    let executor = SandboxExecutor::new(
        resolve(&workdir).unwrap(),
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir,
    );
    let result = checked_shell(
        &executor,
        "stage metadata cargo metadata --format-version=1 --no-deps >/dev/null\nstage repository-tests cargo test -p galla --lib -- --test-threads=2",
    );
    assert!(result.output_text(16384).contains("test result: ok"));
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn native_vendor_snapshot_overrides_host_paths_without_credentials_or_mutation() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::create_dir_all(workdir.join("src")).unwrap();
    fs::create_dir_all(workdir.join(".cargo")).unwrap();
    fs::create_dir_all(workdir.join(".kvist/vendored")).unwrap();
    let dependency = workdir.join(".kvist/vendored/kvist-sandbox-dependency");
    fs::create_dir(&dependency).unwrap();
    let files = [
        (
            "Cargo.toml",
            "[package]\nname=\"kvist-sandbox-dependency\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[lib]\npath=\"lib.rs\"\n",
        ),
        ("lib.rs", "pub fn answer() -> u8 { 42 }\n"),
    ];
    let checksums: BTreeMap<_, _> = files
        .iter()
        .map(|(name, text)| {
            use sha2::{Digest, Sha256};
            fs::write(dependency.join(name), text).unwrap();
            (*name, hex::encode(Sha256::digest(text.as_bytes())))
        })
        .collect();
    fs::write(
        dependency.join(".cargo-checksum.json"),
        json!({"files": checksums, "package": "0".repeat(64)}).to_string(),
    )
    .unwrap();
    fs::write(workdir.join("Cargo.toml"), "[package]\nname=\"vendor-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[dependencies]\nkvist-sandbox-dependency=\"=0.1.0\"\n").unwrap();
    fs::write(
        workdir.join("src/lib.rs"),
        "#[test] fn dependency() { assert_eq!(kvist_sandbox_dependency::answer(), 42); }\n",
    )
    .unwrap();
    let lock = format!(
        "version=4\n[[package]]\nname=\"vendor-trial\"\nversion=\"0.1.0\"\ndependencies=[\"kvist-sandbox-dependency\"]\n[[package]]\nname=\"kvist-sandbox-dependency\"\nversion=\"0.1.0\"\nsource=\"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum=\"{}\"\n",
        "0".repeat(64)
    );
    fs::write(workdir.join("Cargo.lock"), &lock).unwrap();
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
    fs::write(dependency.join("Cargo.toml"), "malicious replacement").unwrap();
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
    let result = checked_shell(
        &executor,
        "stage offline test \"$CARGO_NET_OFFLINE\" = true\nstage cargo-home test \"$CARGO_HOME\" = /tmp/cargo-home\nstage target test \"$CARGO_TARGET_DIR\" = /tmp/target\nstage home test \"$HOME\" = /tmp\nstage home-write touch \"$HOME/scratch-proof\"\nstage credentials test ! -e /home/stefan/.cargo/credentials.toml\nstage host-settings test ! -e /home/stefan/.rustup/settings.toml\nstage root-home test ! -e /root/.cargo\nstage dns test ! -e /etc/resolv.conf\nstage token test -z \"$CARGO_REGISTRY_TOKEN\"\nstage rustup-home test \"$RUSTUP_HOME\" = /rust/rustup-home\nstage shim-readonly test ! -w /rust/runtime/bin/cargo\nstage compiler-readonly test ! -w /rust/toolchain/bin/rustc\nstage vendor-readonly test ! -w /rust/vendor/kvist-sandbox-dependency/Cargo.toml\nstage metadata cargo metadata --format-version=1\nstage tests cargo test\nstage docs cargo doc --no-deps",
    );
    assert!(!result.failed(), "{}", result.error_text(8192));
    assert!(result.output_text(8192).contains("test result: ok"));
    assert_eq!(
        fs::read_to_string(workdir.join("Cargo.lock")).unwrap(),
        lock
    );
    assert!(!workdir.join("target").exists());

    let without_vendor = SandboxExecutor::new(
        ToolRegistry::new(ToolPolicy::default()),
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir,
    );
    let result = shell(&without_vendor, "test ! -e /rust/vendor");
    assert!(!result.failed());
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn rust_toolchain_is_discoverable_via_path_and_version_commands() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::create_dir(workdir.join("src")).unwrap();
    fs::write(
        workdir.join("Cargo.toml"),
        "[package]\nname=\"discover-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(workdir.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n").unwrap();
    fs::write(
        workdir.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"discover-trial\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let registry = resolve(&workdir).unwrap();
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    // Verify toolchain discoverability via PATH and version commands.
    // This ensures an agent can find and use the toolchain without
    // needing to know the exact mount path. Note that cargo is the
    // offline/locked shim at /rust/runtime/bin/cargo, while rustc and
    // rustdoc use selection-aware wrappers at /rust/runtime/bin/.
    let result = shell(
        &executor,
        "which rustc && which cargo && which rustdoc && rustc --version && cargo --version && rustdoc --version && test ! -d /home/stefan",
    );
    let output = result.output_text(8192);
    let error = result.error_text(8192);
    assert!(!result.failed(), "{}", error);
    assert!(
        output.contains("/rust/runtime/bin/rustc"),
        "rustc not found at expected path"
    );
    assert!(
        output.contains("/rust/runtime/bin/cargo"),
        "cargo shim not found at expected path"
    );
    assert!(
        output.contains("/rust/runtime/bin/rustdoc"),
        "rustdoc not found at expected path"
    );
    assert!(output.contains("rustc 1."), "rustc version output missing");
    assert!(output.contains("cargo 1."), "cargo version output missing");
    assert!(
        output.contains("rustdoc 1."),
        "rustdoc version output missing"
    );
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn cargo_installed_tools_are_available_in_sandbox() {
    let nextest = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cargo/bin/cargo-nextest");
    assert!(
        nextest.is_file(),
        "this optional native trial requires host-provisioned cargo-nextest"
    );
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::create_dir(workdir.join("src")).unwrap();
    fs::write(
        workdir.join("Cargo.toml"),
        "[package]\nname=\"cargo-tools-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(workdir.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n").unwrap();
    fs::write(
        workdir.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"cargo-tools-trial\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let registry = resolve(&workdir).unwrap();
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    // Verify that cargo-installed tools are available via PATH inside the sandbox.
    // This checks that the user's ~/.cargo/bin directory is mounted and accessible.
    let result = shell(&executor, "which cargo-nextest && cargo nextest --version");
    let output = result.output_text(8192);
    let error = result.error_text(8192);
    if result.failed() {
        eprintln!("output: {output}");
        eprintln!("error: {error}");
    }
    // The test should pass if cargo-nextest is installed on the host.
    // If it's not installed, the test should fail with a clear error.
    assert!(
        !result.failed(),
        "cargo-nextest should be available in the sandbox: {error}"
    );
    assert!(
        output.contains("/rust/user-cargo-bin/cargo-nextest"),
        "cargo-nextest not found at expected path"
    );
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn home_directory_is_not_accessible_in_sandbox() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::create_dir(workdir.join("src")).unwrap();
    fs::write(
        workdir.join("Cargo.toml"),
        "[package]\nname=\"home-test-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(workdir.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n").unwrap();
    fs::write(
        workdir.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"home-test-trial\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let registry = resolve(&workdir).unwrap();
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    let host_home = std::env::var("HOME").unwrap();
    assert!(host_home.starts_with("/home/") || host_home == "/root");
    let result = shell(
        &executor,
        &format!("test ! -e '{host_home}' && test -d \"$HOME\" && test -w \"$HOME\""),
    );
    assert!(
        !result.failed(),
        "home directory should not be accessible in the sandbox"
    );
}

#[test]
#[ignore = "requires an explicitly selected runner and native Bubblewrap"]
fn selected_toolchain_runs_the_complete_native_chain() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    let channel = std::env::var("KVIST_RUST_TEST_CHANNEL").unwrap_or_else(|_| "stable".into());
    fs::write(
        workdir.join("rust-toolchain.toml"),
        format!("[toolchain]\nchannel = \"{channel}\"\ncomponents = [\"rustfmt\", \"clippy\"]\n"),
    )
    .unwrap();
    fs::create_dir(workdir.join("src")).unwrap();
    fs::create_dir(workdir.join("tests")).unwrap();
    fs::write(
        workdir.join("Cargo.toml"),
        "[package]\nname=\"chain-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(
        workdir.join("Cargo.lock"),
        "version=4\n[[package]]\nname=\"chain-trial\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        workdir.join("src/lib.rs"),
        "/// ```\n/// assert_eq!(chain_trial::answer(), 42);\n/// ```\npub fn answer() -> u8 {\n    42\n}\n#[test]\nfn unit() {\n    assert_eq!(answer(), 42);\n}\n",
    )
    .unwrap();
    fs::write(
        workdir.join("src/main.rs"),
        "fn main() {\n    println!(\"answer={}\", chain_trial::answer());\n}\n",
    )
    .unwrap();
    fs::write(
        workdir.join("tests/integration.rs"),
        "#[test]\nfn integration() {\n    assert_eq!(chain_trial::answer(), 42);\n}\n",
    )
    .unwrap();
    let expected = std::process::Command::new("/usr/bin/rustup")
        .args(["which", "--toolchain", &channel, "rustc"])
        .current_dir("/")
        .env_remove("RUSTUP_TOOLCHAIN")
        .output()
        .unwrap();
    assert!(expected.status.success(), "provision {channel} on the host");
    let expected = std::process::Command::new(String::from_utf8(expected.stdout).unwrap().trim())
        .arg("--version")
        .output()
        .unwrap();
    assert!(expected.status.success());
    let executor = SandboxExecutor::new(
        resolve(&workdir).unwrap(),
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    let result = checked_shell(
        &executor,
        "stage home test -w \"$HOME\"\nstage rustup-selection rustup show active-toolchain\nstage rustup-list rustup toolchain list\nstage rustup-cargo rustup which cargo\nstage rustup-rustc rustup which rustc\nstage rustup-sysroot test \"$(rustup run \"$RUSTUP_TOOLCHAIN\" rustc --print sysroot)\" = /rust/toolchain\nstage compiler rustc --version\nstage cargo cargo --version\nstage sysroot test \"$(rustc --print sysroot)\" = /rust/toolchain\nstage formatter cargo fmt --check\nstage check cargo check --all-targets\nstage build cargo build\nstage binary /tmp/target/debug/chain-trial\nstage unit-tests cargo test --lib\nstage integration-tests cargo test --test integration\nstage doctests cargo test --doc\nstage clippy cargo clippy --all-targets -- -D warnings\nstage documentation cargo doc --no-deps\nstage isolated-target test ! -e target",
    );
    let output = result.output_text(16384);
    assert!(output.contains(String::from_utf8(expected.stdout).unwrap().trim()));
    assert!(output.contains("answer=42"));
    assert_eq!(output.matches("test result: ok").count(), 3, "{output}");
    assert!(
        !output.contains("/home/"),
        "rustup must report sandbox-native paths"
    );
    let failure = shell(
        &executor,
        "printf '#[test] fn fails() { assert!(false); }\\n' > tests/fails.rs; cargo test --test fails",
    );
    assert!(failure.failed(), "a failing test must propagate a failure");
    assert!(failure.output_text(8192).contains("test result: FAILED"));
    let failure = shell(
        &executor,
        "printf 'not rust syntax\\n' > src/main.rs; cargo build",
    );
    assert!(
        failure.failed(),
        "a compiler failure must propagate a failure"
    );
}

#[test]
#[ignore = "requires an explicitly selected installed minimal toolchain and native Bubblewrap"]
fn installed_minimal_pin_builds_without_optional_components() {
    let channel = std::env::var("KVIST_RUST_TEST_MINIMAL_CHANNEL")
        .expect("set KVIST_RUST_TEST_MINIMAL_CHANNEL to a host-provisioned minimal installation");
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::write(workdir.join("rust-toolchain"), &channel).unwrap();
    fs::create_dir(workdir.join("src")).unwrap();
    fs::write(
        workdir.join("Cargo.toml"),
        "[package]\nname=\"minimal-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(
        workdir.join("Cargo.lock"),
        "version=4\n[[package]]\nname=\"minimal-trial\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        workdir.join("src/lib.rs"),
        "#[test] fn works() { assert_eq!(2 + 2, 4); }\n",
    )
    .unwrap();
    let executor = SandboxExecutor::new(
        resolve(&workdir).unwrap(),
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    let result = checked_shell(
        &executor,
        "stage selection rustup show active-toolchain\nstage compiler rustc --version\nstage build cargo build\nstage tests cargo test\nstage docs cargo doc --no-deps",
    );
    assert!(result.output_text(8192).contains(&channel));
    assert!(result.output_text(8192).contains("test result: ok"));

    fs::remove_file(workdir.join("rust-toolchain")).unwrap();
    fs::write(
        workdir.join("rust-toolchain.toml"),
        format!("[toolchain]\nchannel=\"{channel}\"\ncomponents=[\"rustfmt\"]\n"),
    )
    .unwrap();
    let error = resolve(&workdir).unwrap_err().to_string();
    assert!(
        error.contains("rustfmt") && error.contains("host"),
        "{error}"
    );
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
