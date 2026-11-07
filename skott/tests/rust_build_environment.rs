//! Opt-in native isolation trials; no provider or host dependency acquisition.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use sav::{CancellationToken, ToolIntent};
use serde_json::json;
use serial_test::serial;
use skott::session::ToolExecutor;
use skott::toolchain::HostProbe;
use skott::{ProfileSetting, SandboxExecutor, SandboxPaths, ToolPolicy, ToolProfile, ToolRegistry};

/// The manifest directory of the crate under test for the current
/// invocation.
///
/// Cargo sets `CARGO_MANIFEST_DIR` in the test process environment at run
/// time, so it stays correct even when a previously built test binary is
/// reused after the repository has moved. The compile-time `env!` value is
/// only a fallback for running the test binary outside of cargo.
fn manifest_dir() -> PathBuf {
    std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

fn fixture() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".rust-environment-test-")
        .tempdir_in(manifest_dir())
        .unwrap()
}

fn runner() -> PathBuf {
    if let Some(path) = std::env::var_os("KVIST_GALLA") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return path;
        }
    }
    // Test the current build before a potentially older installed runner.
    if let Some(path) = std::env::current_exe().ok().and_then(|executable| {
        executable
            .parent()
            .and_then(std::path::Path::parent)
            .map(|directory| directory.join("galla-runner"))
    }) && path.is_file()
    {
        return path;
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
    roots.push(manifest_dir().join("../target"));
    roots.push(PathBuf::from("/opt/target"));
    roots
        .iter()
        .flat_map(|root| {
            ["debug", "release"].map(|profile| root.join(profile).join("galla-runner"))
        })
        .find(|path| path.is_file())
        .expect("build galla-runner first or set KVIST_GALLA; native coverage must not skip")
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

fn unavailable_rust(workdir: &std::path::Path) -> ToolRegistry {
    let registry = resolve(workdir).expect("unavailable Rust must not block independent tools");
    assert!(!registry.profiles().contains(&"rust"));
    assert!(!registry.diagnostics().is_empty());
    assert!(
        registry
            .tool_definitions()
            .iter()
            .any(|tool| tool.name == "shell")
    );
    registry
}

#[test]
#[serial]
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
        unavailable_rust(directory.path());
    }
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        " ".repeat(65537),
    )
    .unwrap();
    unavailable_rust(directory.path());
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel=\"stable\"",
    )
    .unwrap();
    fs::write(directory.path().join("rust-toolchain"), "stable").unwrap();
    unavailable_rust(directory.path());
}

#[test]
#[serial]
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
    let error = unavailable_rust(directory.path()).diagnostics().join("\n");
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
#[serial]
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
    unavailable_rust(directory.path());
    fs::remove_file(directory.path().join("rust-toolchain")).unwrap();
    fs::create_dir(directory.path().join(".kvist")).unwrap();
    symlink(
        outside.path().canonicalize().unwrap(),
        directory.path().join(".kvist/vendored"),
    )
    .unwrap();
    let registry = resolve(directory.path())
        .expect("unsupported vendor pointer must not disable independent compiler tools");
    assert!(
        registry
            .diagnostics()
            .join("\n")
            .contains("no extra origin")
    );
}

#[test]
#[serial]
fn unusable_vendor_nodes_do_not_hide_the_compiler() {
    for relative in [".kvist", ".kvist/vendored"] {
        let directory = fixture();
        if relative.contains('/') {
            fs::create_dir(directory.path().join(".kvist")).unwrap();
        }
        fs::write(
            directory.path().join(relative),
            "operator-created ordinary file",
        )
        .unwrap();
        let registry =
            resolve(directory.path()).expect("compiler is independent of vendor node shape");
        let executor = SandboxExecutor::new(
            registry,
            SandboxPaths {
                runner: runner(),
                backend: "/usr/bin/bwrap".into(),
            },
            directory.path().to_owned(),
        );
        checked_shell(&executor, "rustc --version");
    }
}

#[test]
#[serial]
fn native_cargo_configuration_is_preserved_without_a_shared_manifest() {
    let directory = fixture();
    fs::create_dir_all(directory.path().join(".kvist/sandbox-cargo")).unwrap();
    fs::write(directory.path().join(".kvist/sandbox-cargo/config.toml"), "[alias]\nhello=\"version\"\n[source.crates-io]\nreplace-with=\"local-vendor\"\n[source.local-vendor]\ndirectory=\"/host/not-granted\"\n").unwrap();
    let registry =
        resolve(directory.path()).expect("native Cargo tables need no application manifest");
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        directory.path().to_owned(),
    );
    assert!(
        checked_shell(&executor, "cargo hello")
            .output_text(8192)
            .contains("cargo ")
    );
}

#[test]
#[serial]
fn malformed_cargo_configuration_blocks_cargo_not_the_compiler() {
    let directory = fixture();
    fs::create_dir_all(directory.path().join(".kvist/sandbox-cargo")).unwrap();
    fs::write(
        directory.path().join(".kvist/sandbox-cargo/config.toml"),
        "[malformed\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        "[package]\nname=\"local-config-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(directory.path().join("src/lib.rs"), "pub fn local() {}\n").unwrap();
    let registry = resolve(directory.path())
        .expect("malformed Cargo configuration is not compiler unavailability");
    assert!(registry.diagnostics().join("\n").contains("configuration"));
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        directory.path().to_owned(),
    );
    checked_shell(&executor, "rustc --version");
    let cargo = shell(&executor, "cargo check --lib");
    assert!(cargo.failed());
    assert!(
        cargo.error_text(8192).contains("config"),
        "{}",
        cargo.error_text(8192)
    );
}

#[test]
#[serial]
fn auto_rust_recovers_from_initial_unavailability_without_restart() {
    let directory = fixture();
    fs::write(directory.path().join("rust-toolchain"), "999.99.999").unwrap();
    let settings = ToolProfile::CONFIGURABLE
        .iter()
        .map(|profile| {
            (
                *profile,
                if *profile == ToolProfile::Rust {
                    ProfileSetting::Auto
                } else {
                    ProfileSetting::Off
                },
            )
        })
        .collect();
    let registry = ToolRegistry::resolve_for_workspace(
        ToolPolicy::default(),
        &settings,
        &HostProbe,
        None,
        directory.path(),
    )
    .unwrap();
    assert!(registry.diagnostics().join("\n").contains("unavailable"));
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        directory.path().to_owned(),
    );
    checked_shell(&executor, "printf 'independent work'");
    fs::remove_file(directory.path().join("rust-toolchain")).unwrap();
    assert!(
        checked_shell(&executor, "rustc --version")
            .output_text(8192)
            .contains("rustc ")
    );
}

#[test]
#[serial]
fn requested_rust_unavailability_keeps_independent_workspace_tools() {
    for forced in [None, Some(ToolProfile::Rust)] {
        let directory = fixture();
        fs::write(directory.path().join("rust-toolchain"), "999.99.999").unwrap();
        let settings = [(ToolProfile::Rust, ProfileSetting::On)]
            .into_iter()
            .collect();
        let registry = ToolRegistry::resolve_for_workspace(
            ToolPolicy::default(),
            &settings,
            &HostProbe,
            forced,
            directory.path(),
        )
        .expect("requested unavailable Rust must not disable source work");
        assert!(!registry.profiles().contains(&"rust"));
        assert!(registry.diagnostics().join("\n").contains("unavailable"));
        let executor = SandboxExecutor::new(
            registry,
            SandboxPaths {
                runner: runner(),
                backend: "/usr/bin/bwrap".into(),
            },
            directory.path().to_owned(),
        );
        checked_shell(&executor, "printf 'independent work'");
    }
}

#[test]
#[serial]
fn workspace_selector_inside_its_grant_cannot_authorize_another_host_root() {
    use std::os::unix::fs::symlink;
    let directory = fixture();
    let work = directory.path().join("work");
    let outside = directory.path().join("outside");
    fs::create_dir(&work).unwrap();
    fs::create_dir(&outside).unwrap();
    symlink(".", work.join("selector")).unwrap();
    let executor = SandboxExecutor::new(
        ToolRegistry::new(ToolPolicy::default()),
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        work.join("selector"),
    );
    let target = outside
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .replace('\'', "'\\''");
    checked_shell(&executor, &format!("ln -sfn '{target}' selector"));
    checked_shell(&executor, "printf 'authorized work' > retained");
    assert!(
        work.join("retained").exists(),
        "later grants must use the authorized directory origin"
    );
    assert!(
        !outside.join("retained").exists(),
        "a payload-written selector must not widen host authority"
    );
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

fn repository_cargo(arguments: &str) -> String {
    format!("env CARGO_TARGET_DIR=/workspace/target/repository-trial cargo {arguments}")
}

#[test]
#[serial]
fn repository_cargo_does_not_wait_for_enclosing_target_locks() {
    use std::fs::OpenOptions;
    use std::time::Duration;

    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::create_dir(workdir.join("src")).unwrap();
    fs::write(
        workdir.join("Cargo.toml"),
        "[package]\nname=\"target-lock-trial\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(
        workdir.join("Cargo.lock"),
        "version=4\n[[package]]\nname=\"target-lock-trial\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    fs::write(workdir.join("src/lib.rs"), "#[test] fn works() {}\n").unwrap();
    fs::create_dir_all(workdir.join("target/debug")).unwrap();
    let _locks: Vec<_> = [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"]
        .iter()
        .map(|name| {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(workdir.join("target/debug").join(name))
                .unwrap();
            file.lock().unwrap();
            file
        })
        .collect();
    let executor = SandboxExecutor::new(
        resolve(&workdir).unwrap(),
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    let cancellation = CancellationToken::new();
    let result = std::thread::scope(|scope| {
        let (done, finished) = std::sync::mpsc::channel();
        let token = &cancellation;
        scope.spawn(move || {
            if finished.recv_timeout(Duration::from_secs(5)).is_err() {
                token.cancel();
            }
        });
        let result = executor.execute(
            &ToolIntent {
                id: "target-lock-trial".into(),
                provider_id: None,
                name: "shell".into(),
                arguments: json!({"command": repository_cargo("test --lib")}),
            },
            &cancellation,
        );
        let _ = done.send(());
        result.unwrap()
    });
    assert!(
        !result.failed(),
        "nested Cargo waited for an enclosing target lock: cancelled={}, stderr={}",
        result.cancelled,
        result.error_text(16384)
    );
    assert!(result.output_text(16384).contains("1 passed; 0 failed"));
    assert!(workdir.join("target/repository-trial/debug/deps").is_dir());
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
#[serial]
fn diagnostics_cover_the_chain_without_command_or_output_payloads() {
    const CHILD: &str = "SKOTT_DIAGNOSTICS_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Tracing callsite interests are process-global, even for local subscribers.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "diagnostics_cover_the_chain_without_command_or_output_payloads",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "diagnostic subprocess failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
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
        "offline shared vendor view prepared",
        "offline Rust resources prepared",
        "offline Rust resources prepared",
        "sandbox preflight stage started",
        "stage=runner_identity",
        "stage=workspace_scope",
        "stage=rust_resources",
        "stage=request_validation",
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
#[serial]
fn native_repository_tests_execute_inside_the_workspace_sandbox() {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            std::env::var("SKOTT_LOG").unwrap_or_else(|_| "warn".into()),
        ))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let workdir = manifest_dir().parent().unwrap().canonicalize().unwrap();
    assert!(
        workdir.join(".kvist/vendored").is_dir(),
        "provision repository dependencies on the host"
    );
    let executor = SandboxExecutor::new(
        resolve(&workdir)
            .unwrap()
            .with_file_helper(env!("CARGO_BIN_EXE_skott-file-tool")),
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir,
    );
    let inspection = checked_shell(
        &executor,
        "cd /workspace && git status --short && printf '\\n---\\n' && git diff --stat",
    );
    assert!(inspection.output_text(16384).contains("---"));
    let listing = executor
        .execute(
            &ToolIntent {
                id: "repository-root-listing".into(),
                provider_id: None,
                name: "list_dir".into(),
                arguments: json!({"path": "/workspace"}),
            },
            &CancellationToken::new(),
        )
        .unwrap();
    assert!(
        !listing.failed(),
        "native root listing failed: {}",
        listing.error_text(16384)
    );
    assert!(listing.output_text(16384).contains("Cargo.toml"));
    let result = checked_shell(
        &executor,
        &format!(
            "stage metadata {} >/dev/null\nstage repository-tests {}",
            repository_cargo("metadata --format-version=1 --no-deps"),
            repository_cargo("test -p galla --lib -- --test-threads=2"),
        ),
    );
    assert!(result.output_text(16384).contains("test result: ok"));
}

#[test]
fn repository_trial_requires_global_nextest_exclusivity() {
    let path = manifest_dir().join("../.config/nextest.toml");
    let contents = fs::read_to_string(path).expect("read repository nextest configuration");
    let config: toml::Value = toml::from_str(&contents).expect("parse nextest configuration");
    let overrides = config["profile"]["default"]["overrides"]
        .as_array()
        .expect("default-profile nextest overrides");
    let filter = "package(=skott) & binary(=rust_build_environment) & test(=native_repository_tests_execute_inside_the_workspace_sandbox)";
    let rule = overrides
        .iter()
        .find(|rule| rule.get("filter").and_then(toml::Value::as_str) == Some(filter))
        .expect("exact native repository trial override");
    assert_eq!(
        rule["threads-required"].as_str(),
        Some("num-test-threads"),
        "the repository trial must reserve the whole pool, not just one serial group"
    );
}

#[test]
fn repository_nextest_recipe_uses_default_discovery() {
    let recipes = fs::read_to_string(manifest_dir().join("../justfile"))
        .expect("read repository test recipes");
    let test_recipe = recipes
        .split_once("\ntest:\n")
        .expect("repository test recipe")
        .1;
    let command = test_recipe
        .lines()
        .take_while(|line| line.starts_with(' ') || line.starts_with('\t'))
        .find(|line| line.trim_start().starts_with("cargo nextest run "))
        .expect("repository test recipe nextest command");
    let arguments: Vec<_> = command.split_whitespace().collect();
    assert!(
        !arguments.iter().any(|argument| {
            *argument == "--config-file" || argument.starts_with("--config-file=")
        }),
        "the repository recipe must exercise automatic scheduling discovery"
    );
}

#[test]
#[serial]
fn native_shared_vendor_overrides_host_paths_with_native_checks_and_masking() {
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
    assert!(diagnostics.contains("shared read-only"));
    assert!(diagnostics.contains("vendor view sha256:"));
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let host_vendor_mode = fs::metadata(workdir.join(".kvist/vendored"))
        .unwrap()
        .permissions()
        .mode();
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    let vendor_metadata = fs::metadata(workdir.join(".kvist/vendored")).unwrap();
    checked_shell(
        &executor,
        &format!(
            "stage shared-inode test \"$(stat -c '%d:%i' /rust/vendor)\" = '{}:{}'",
            vendor_metadata.dev(),
            vendor_metadata.ino(),
        ),
    );
    let writer = fs::File::open(workdir.join(".kvist/vendored")).unwrap();
    writer.try_lock().unwrap();
    drop(writer);
    std::thread::scope(|scope| {
        let running = scope.spawn(|| checked_shell(&executor, "touch lease-started; sleep 1"));
        let started = std::time::Instant::now();
        while !workdir.join("lease-started").exists() {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(10),
                "native tool did not start"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            fs::File::open(workdir.join(".kvist/vendored"))
                .unwrap()
                .try_lock()
                .is_ok()
        );
        running.join().unwrap();
    });
    fs::File::open(workdir.join(".kvist/vendored"))
        .unwrap()
        .try_lock()
        .unwrap();
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
    checked_shell(
        &executor,
        "stage alias-readable test -r .kvist/vendored\nstage alias-unwritable test ! -w .kvist/vendored\nstage alias-traversable test -x .kvist/vendored\nstage sibling-state test -r .kvist/rust-toolchain.json\nstage alias-listing ls .kvist/vendored\nstage alias-reading cat .kvist/vendored/kvist-sandbox-dependency/Cargo.toml\nif touch .kvist/vendored/forbidden; then printf 'vendor alias write unexpectedly succeeded\\n' >&2; exit 1; fi\nif chmod 755 .kvist/vendored; then printf 'vendor overlay permissions unexpectedly changed\\n' >&2; exit 1; fi",
    );
    let result = checked_shell(
        &executor,
        "stage offline test \"$CARGO_NET_OFFLINE\" = true\nstage cargo-home test \"$CARGO_HOME\" = /tmp/cargo-home\nstage target test \"$CARGO_TARGET_DIR\" = /workspace/target\nstage home test \"$HOME\" = /tmp\nstage home-write touch \"$HOME/scratch-proof\"\nstage credentials test ! -e /home/stefan/.cargo/credentials.toml\nstage host-settings test ! -e /home/stefan/.rustup/settings.toml\nstage root-home test ! -e /root/.cargo\nstage dns test ! -e /etc/resolv.conf\nstage token test -z \"$CARGO_REGISTRY_TOKEN\"\nstage rustup-home test \"$RUSTUP_HOME\" = /tmp/rustup-home\nstage shim-readonly test ! -w /rust/runtime/bin/cargo\nstage compiler-readonly test ! -w /rust/toolchain/bin/rustc\nstage vendor-readonly test ! -w /rust/vendor/kvist-sandbox-dependency/Cargo.toml\nstage metadata cargo metadata --format-version=1\nstage tests cargo test\nstage clippy cargo clippy --all-targets\nstage docs cargo doc --no-deps",
    );
    assert!(!result.failed(), "{}", result.error_text(8192));
    assert!(result.output_text(8192).contains("test result: ok"));
    // The clippy stage must have run and resolved the vendored dependency.
    assert!(result.output_text(8192).contains("stage: clippy"));
    let published: toml::Table =
        toml::from_str(&fs::read_to_string(workdir.join("Cargo.lock")).unwrap()).unwrap();
    let dependency_lock = published["package"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"].as_str() == Some("kvist-sandbox-dependency"))
        .unwrap();
    assert_eq!(
        dependency_lock["checksum"].as_str(),
        Some("0000000000000000000000000000000000000000000000000000000000000000")
    );
    assert!(workdir.join("target/doc/vendor_trial/index.html").is_file());
    assert_eq!(
        fs::read_to_string(dependency.join("Cargo.toml")).unwrap(),
        files[0].1
    );
    assert_eq!(
        fs::metadata(workdir.join(".kvist/vendored"))
            .unwrap()
            .permissions()
            .mode(),
        host_vendor_mode
    );
    assert!(!workdir.join(".kvist/vendored/forbidden").exists());
    fs::write(dependency.join("lib.rs"), "pub fn answer() -> u8 { 43 }\n").unwrap();
    let damaged = shell(&executor, "cargo clean && cargo test");
    assert!(
        damaged.failed(),
        "Cargo must detect changed package content"
    );
    assert!(
        damaged.error_text(8192).contains("checksum"),
        "{}",
        damaged.error_text(8192)
    );
    fs::write(dependency.join("lib.rs"), files[1].1).unwrap();

    let without_vendor = SandboxExecutor::new(
        ToolRegistry::new(ToolPolicy::default()),
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir,
    );
    let result = shell(
        &without_vendor,
        "test ! -e /rust/vendor && test -r .kvist/vendored && test ! -w .kvist/vendored",
    );
    assert!(!result.failed(), "{}", result.error_text(8192));
}

#[test]
#[serial]
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
#[serial]
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
#[serial]
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
#[serial]
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
        "stage home test -w \"$HOME\"\nstage rustup-selection rustup show active-toolchain\nstage rustup-list rustup toolchain list\nstage rustup-cargo rustup which cargo\nstage rustup-rustc rustup which rustc\nstage rustup-sysroot test \"$(rustup run \"$RUSTUP_TOOLCHAIN\" rustc --print sysroot)\" = /rust/toolchain\nstage compiler rustc --version\nstage cargo cargo --version\nstage sysroot test \"$(rustc --print sysroot)\" = /rust/toolchain\nstage formatter cargo fmt --check\nstage check cargo check --all-targets\nstage build cargo build\nstage binary /workspace/target/debug/chain-trial\nstage unit-tests cargo test --lib\nstage integration-tests cargo test --test integration\nstage doctests cargo test --doc\nstage clippy cargo clippy --all-targets -- -D warnings\nstage documentation cargo doc --no-deps\nstage workspace-target test -x target/debug/chain-trial",
    );
    checked_shell(
        &executor,
        "stage persisted-binary /workspace/target/debug/chain-trial\nstage persisted-docs test -f target/doc/chain_trial/index.html",
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
#[serial]
fn installed_minimal_pin_builds_without_optional_components() {
    let channel =
        std::env::var("KVIST_RUST_TEST_MINIMAL_CHANNEL").unwrap_or_else(|_| "1.94.0".into());
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
    let executor = SandboxExecutor::new(
        resolve(&workdir).expect("a missing optional formatter must not disable the compiler"),
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    checked_shell(&executor, "rustc --version && cargo check");
    let formatter = shell(&executor, "cargo fmt --check");
    assert!(
        formatter.failed(),
        "the missing formatter must fail only its own operation"
    );
    assert!(
        formatter.error_text(8192).contains("cargo-fmt"),
        "{}",
        formatter.error_text(8192)
    );
}

#[test]
#[serial]
fn unavailable_live_project_pin_does_not_block_independent_shell_work() {
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
            arguments: json!({"command":"touch useful-work"}),
        },
        &CancellationToken::new(),
    );
    let result = result.unwrap();
    assert!(!result.failed(), "{}", result.error_text(8192));
    assert!(workdir.join("useful-work").exists());
    assert!(
        result.error_text(8192).contains("9.99.99"),
        "{}",
        result.error_text(8192)
    );
    let cargo = shell(&executor, "cargo --version");
    assert!(cargo.failed(), "must not silently use the old compiler");
}

#[test]
#[serial]
fn unavailable_live_rust_diagnostic_preserves_native_write_progress() {
    let directory = fixture();
    let workdir = directory.path().canonicalize().unwrap();
    fs::write(workdir.join("rust-toolchain"), "stable\n").unwrap();
    let registry = resolve(&workdir)
        .unwrap()
        .with_file_helper(env!("CARGO_BIN_EXE_skott-file-tool"));
    assert!(registry.profiles().contains(&"rust"));
    fs::write(workdir.join("rust-toolchain"), "9.99.99\n").unwrap();
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: runner(),
            backend: "/usr/bin/bwrap".into(),
        },
        workdir.clone(),
    );
    let intent = ToolIntent {
        id: "native-write-with-live-rust-diagnostic".into(),
        provider_id: None,
        name: "write_file".into(),
        arguments: json!({
            "path":"/workspace/native-progress",
            "content":"preserved native work\n"
        }),
    };
    let outcome = executor
        .execute(&intent, &CancellationToken::new())
        .unwrap();
    assert!(!outcome.failed(), "{}", outcome.error_text(8192));
    let diagnostic = outcome.error_text(8192);
    assert!(
        diagnostic.contains("9.99.99") && diagnostic.to_lowercase().contains("rust"),
        "{diagnostic}"
    );
    assert_eq!(
        fs::read(workdir.join("native-progress")).unwrap(),
        b"preserved native work\n"
    );
    assert_eq!(
        executor.progress(&intent, &outcome).unwrap(),
        sav::loop_detection::ProgressEvidence::StateChanged
    );
}

#[test]
#[serial]
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
    assert!(!directory.path().join(".kvist").exists());
}

#[test]
#[serial]
fn workspace_cargo_resolves_live_project_locks_offline() {
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
    assert!(!missing.failed(), "{}", missing.error_text(8192));
    assert!(
        lock.exists(),
        "workspace Cargo may create its writable lock"
    );

    let stale = b"version = 4\n[[package]]\nname = \"locked-trial\"\nversion = \"0.0.1\"\n";
    fs::write(&lock, stale).unwrap();
    let guarded = shell(&executor, "cargo build --locked");
    assert!(
        guarded.failed(),
        "explicit locked resolution must remain available"
    );
    assert_eq!(fs::read(&lock).unwrap(), stale);
    for command in ["cargo build", "cargo metadata --format-version=1"] {
        let result = shell(&executor, command);
        assert!(!result.failed(), "{}", result.error_text(8192));
        assert_ne!(fs::read(&lock).unwrap(), stale);
    }

    let matching = b"version = 4\n[[package]]\nname = \"locked-trial\"\nversion = \"0.1.0\"\n";
    fs::write(&lock, matching).unwrap();
    let result = shell(
        &executor,
        "cargo --version && cargo check && cargo metadata --format-version=1 --no-deps >/dev/null",
    );
    assert!(!result.failed(), "{}", result.error_text(8192));
    let resolved: toml::Table = toml::from_str(&fs::read_to_string(&lock).unwrap()).unwrap();
    assert_eq!(
        resolved["package"][0]["name"].as_str(),
        Some("locked-trial")
    );
    assert_eq!(resolved["package"][0]["version"].as_str(), Some("0.1.0"));
    assert!(workdir.join("target/debug/deps").is_dir());
}
