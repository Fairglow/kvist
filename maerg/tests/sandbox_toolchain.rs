//! End-to-end validation that the Rust toolchain is fully available
//! inside the sandbox, including supporting binaries (rustc, rustdoc)
//! and the standard library, and that Clippy can run.
//!
//! Tests drive the real production path: runner_identity, backend_identity,
//! ensure_available, and execute_with_timeout with a sandboxed Rust command.
//!
//! Self-skips when the live sandbox cannot run (no built runner, no bwrap,
//! no cargo, no git worktree), so it never fails in an environment that
//! lacks the bwrap runner.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use kvist::config::{AcquisitionConfig, SandboxConfig, VcsSelection};
use kvist::sandbox::{
    ExecutionOptions, ExecutionPhase, ExecutionRequest, backend_identity, ensure_available,
    execute_with_timeout, runner_identity,
};

/// A well-formed policy identity (sha256 digest) for the live test.
const POLICY_IDENTITY: &str =
    "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// The workspace root is the parent of the maerg crate manifest.
fn workspace_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)
}

/// Candidate roots that may hold the built sandbox runner.
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
    if let Ok(path) = std::env::var("KVIST_GALLA") {
        let candidate = Path::new(&path);
        if candidate.is_file() && outside_worktree(candidate) {
            return Some(candidate.to_path_buf());
        }
    }
    for root in candidate_target_roots() {
        for profile in ["debug", "release"] {
            let candidate = root.join(profile).join("galla-runner");
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

/// Root of the enclosing git worktree.
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

/// Resolve the sandbox config from the test environment.
fn resolve_sandbox_config(runner: &Path, backend: &Path) -> SandboxConfig {
    SandboxConfig {
        runner: runner.to_string_lossy().into_owned(),
        backend: backend.to_string_lossy().into_owned(),
        environment_allowlist: vec![
            "PATH".to_owned(),
            "RUST_BACKTRACE".to_owned(),
            "CARGO_HOME".to_owned(),
            "RUSTC".to_owned(),
            "TERM".to_owned(),
        ],
        acquisition: AcquisitionConfig::default(),
    }
}

/// Run a sandboxed command and return the result, or skip the test.
fn run_sandboxed(
    config: &SandboxConfig,
    worktree: &Path,
    program: &str,
    arguments: &[String],
    expected_runner: &kvist::sandbox::RunnerIdentity,
    expected_backend: &kvist::sandbox::BackendIdentity,
) -> Option<kvist::sandbox::ExecutionResult> {
    let selected = Command::new("/usr/bin/rustup")
        .args(["which", program])
        .current_dir("/")
        .env_remove("RUSTUP_TOOLCHAIN")
        .output()
        .expect("resolve a concrete installed tool for the native trial");
    assert!(
        selected.status.success(),
        "provision {program} on the host: {}",
        String::from_utf8_lossy(&selected.stderr)
    );
    let concrete = String::from_utf8(selected.stdout).unwrap();
    let concrete = concrete.trim();
    let bin = Path::new(concrete).parent().unwrap();
    let mut env = kvist::sandbox::allowed_environment(config, None);
    env.insert("PATH".into(), format!("{}:/usr/bin:/bin", bin.display()));
    env.insert("HOME".into(), "/tmp".into());
    env.insert("CARGO_HOME".into(), "/tmp/cargo-home".into());
    env.insert("CARGO_TARGET_DIR".into(), "/tmp/target".into());
    env.insert("CARGO_NET_OFFLINE".into(), "true".into());
    env.insert(
        "RUSTC".into(),
        bin.join("rustc").to_string_lossy().into_owned(),
    );

    // These generic verification trials explicitly grant the concrete
    // executable. They do not rely on ambient rustup proxies or demonstrate
    // the separate pinned/vendor-manifest Cargo authority.
    let request = ExecutionRequest {
        project_root: worktree,
        vcs_selection: VcsSelection::Auto,
        component_dir: worktree,
        phase: ExecutionPhase::Verification,
        program: concrete,
        arguments,
        environment: env,
        read_only_mounts: &[],
        scratch_host_dir: None,
        backend: expected_backend,
        policy_identity: POLICY_IDENTITY,
    };

    match execute_with_timeout(
        config,
        request,
        ExecutionOptions {
            timeout: Some(Duration::from_secs(60)),
            output_limit: Some(262144),
            live_stdout: None,
        },
        expected_runner,
    ) {
        Ok(result) => Some(result),
        Err(e) => {
            eprintln!("execute_with_timeout failed: {e}");
            None
        }
    }
}

#[test]
fn rust_authoring_has_host_inventory_private_defaults_and_offline_builds() {
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;

    type State = BTreeMap<PathBuf, (u32, u64, i64, i64, i64, i64, String)>;
    fn snapshot(path: &Path, state: &mut State) {
        let metadata = fs::symlink_metadata(path).unwrap();
        let content = if metadata.is_symlink() {
            format!("{:?}", fs::read_link(path).unwrap())
        } else if metadata.is_file() {
            let mut hash = Sha256::new();
            let mut file = fs::File::open(path).unwrap();
            let mut buffer = [0; 65536];
            loop {
                let count = file.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            hex::encode(hash.finalize())
        } else {
            assert!(metadata.is_dir());
            "directory".into()
        };
        state.insert(
            path.into(),
            (
                metadata.mode(),
                metadata.len(),
                metadata.mtime(),
                metadata.mtime_nsec(),
                metadata.ctime(),
                metadata.ctime_nsec(),
                content,
            ),
        );
        if metadata.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                snapshot(&entry.unwrap().path(), state);
            }
        }
    }
    fn host(args: &[&str]) -> Vec<u8> {
        let output = Command::new("/usr/bin/rustup")
            .args(args)
            .current_dir("/")
            .env_clear()
            .env("HOME", std::env::var_os("HOME").unwrap())
            .env("PATH", "/usr/bin:/bin")
            .env("RUSTUP_AUTO_INSTALL", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".rustup");
    let mut before = State::new();
    snapshot(&home, &mut before);
    let runner = locate_runner().expect("build galla-runner outside the worktree");
    let backend = locate_backend().expect("install Bubblewrap on the host");
    let worktree = git_worktree_root().expect("run inside a worktree");
    let container = tempfile::tempdir_in(&worktree).unwrap();
    let project = tempfile::tempdir_in(container.path()).unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::create_dir_all(project.path().join("tests/expected")).unwrap();
    for name in [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        fs::write(project.path().join(name), "protected context").unwrap();
    }
    fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname=\"maerg_rust_trial\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    fs::write(
        project.path().join("Cargo.lock"),
        "version = 3\n[[package]]\nname = \"maerg_rust_trial\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(project.path().join("src/lib.rs"),
            "/// ```\n/// assert_eq!(maerg_rust_trial::answer(), 42);\n/// ```\npub fn answer() -> u8 { 42 }\n#[test] fn works() { assert_eq!(answer(), 42); }\n").unwrap();
    let installed = String::from_utf8(host(&["toolchain", "list"])).unwrap();
    let names: Vec<_> = installed
        .lines()
        .map(|line| line.split_whitespace().next().unwrap())
        .collect();
    assert!(
        names.len() >= 2,
        "provision multiple host toolchains separately"
    );
    let expected = project.path().join("tests/expected");
    fs::write(expected.join("names"), format!("{}\n", names.join("\n"))).unwrap();
    fs::write(expected.join("default"), host(&["default"])).unwrap();
    for name in &names {
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        );
        for (suffix, args) in [
            ("version", vec!["run", name, "rustc", "--version"]),
            ("doc", vec!["run", name, "rustdoc", "--version"]),
            (
                "targets",
                vec!["target", "list", "--installed", "--toolchain", name],
            ),
            ("catalogue", vec!["target", "list", "--toolchain", name]),
            (
                "components",
                vec!["component", "list", "--installed", "--toolchain", name],
            ),
        ] {
            fs::write(expected.join(format!("{name}.{suffix}")), host(&args)).unwrap();
        }
    }
    let script = project.path().join("rust-proof.sh");
    fs::write(&script, r#"set -eu
    test "$RUSTUP_HOME" = /tmp/rustup-home
    test "$HOME" = /tmp && test "$CARGO_HOME" = /tmp/cargo-home
    test "$CARGO_TARGET_DIR" = /tmp/target && test "$CARGO_NET_OFFLINE" = true
    test "$RUSTUP_AUTO_INSTALL" = 0 && test ! -e /home && test ! -e /root
    test ! -w "$RUSTUP_HOME/toolchains" && test ! -w Cargo.toml && test ! -w Cargo.lock
    rustup default | cmp - tests/expected/default
    rustup toolchain list | cut -d' ' -f1 | sort > /tmp/names
    sort tests/expected/names | cmp - /tmp/names
    rustup default stable
    for name in $(cat tests/expected/names); do
      rustup default "$name"
      rustc --version | cmp - "tests/expected/$name.version"
      rustdoc --version | cmp - "tests/expected/$name.doc"
      rustup target list --toolchain "$name" | cmp - "tests/expected/$name.catalogue"
      rustup target list --installed --toolchain "$name" | cmp - "tests/expected/$name.targets"
      rustup component list --installed --toolchain "$name" | cmp - "tests/expected/$name.components"
      for target in $(cat "tests/expected/$name.targets"); do
        rustc --crate-type lib --target "$target" src/lib.rs -o /tmp/trial.rlib
      done
      cargo test --lib
      cargo test --doc
      test ! -w "$(rustup which rustc)"
      if rustup toolchain uninstall "$name"; then exit 1; fi
    done
    if cargo +9.99.99 --version; then exit 1; fi
    if touch /rust/rustup-home/settings.toml; then exit 1; fi
    printf offline-maerg-proof
    "#).unwrap();
    let config = resolve_sandbox_config(&runner, &backend);
    let expected_runner = runner_identity(&config, project.path(), VcsSelection::Auto).unwrap();
    let expected_backend = backend_identity(&config, project.path(), VcsSelection::Auto).unwrap();
    ensure_available(
        &config,
        project.path(),
        VcsSelection::Auto,
        &expected_runner,
        &expected_backend,
    )
    .unwrap();
    let result = execute_with_timeout(
        &config,
        ExecutionRequest {
            project_root: container.path(),
            vcs_selection: VcsSelection::Auto,
            component_dir: project.path(),
            phase: ExecutionPhase::Authoring,
            program: "/usr/bin/bash",
            arguments: &["/workspace/context/rust-proof.sh".into()],
            environment: [("PATH".into(), "/usr/bin:/bin".into())].into(),
            read_only_mounts: &[kvist::sandbox::ReadOnlyMount::file(
                &script,
                "/workspace/context/rust-proof.sh",
            )],
            scratch_host_dir: None,
            backend: &expected_backend,
            policy_identity: POLICY_IDENTITY,
        },
        ExecutionOptions {
            timeout: Some(Duration::from_secs(120)),
            output_limit: Some(65536),
            live_stdout: None,
        },
        &expected_runner,
    )
    .unwrap();
    let direct = execute_with_timeout(
        &config,
        ExecutionRequest {
            project_root: container.path(),
            vcs_selection: VcsSelection::Auto,
            component_dir: project.path(),
            phase: ExecutionPhase::Authoring,
            program: "cargo",
            arguments: &["--version".into()],
            environment: [("PATH".into(), "/usr/bin:/bin".into())].into(),
            read_only_mounts: &[],
            scratch_host_dir: None,
            backend: &expected_backend,
            policy_identity: POLICY_IDENTITY,
        },
        ExecutionOptions {
            timeout: Some(Duration::from_secs(30)),
            output_limit: Some(8192),
            live_stdout: None,
        },
        &expected_runner,
    )
    .unwrap();
    assert!(
        direct.output.status.success(),
        "{}",
        String::from_utf8_lossy(&direct.output.stderr)
    );
    let default = String::from_utf8(host(&["default"])).unwrap();
    assert_eq!(
        direct.output.stdout,
        host(&[
            "run",
            default.split_whitespace().next().unwrap(),
            "cargo",
            "--version"
        ])
    );
    let mut after = State::new();
    snapshot(&home, &mut after);
    assert_eq!(before, after, "host Rust state changed");
    assert!(
        result.output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.output.stdout),
        String::from_utf8_lossy(&result.output.stderr)
    );
    assert!(String::from_utf8_lossy(&result.output.stdout).ends_with("offline-maerg-proof"));
}

#[test]
fn rust_authoring_builds_workspace_members_with_readonly_provider_context() {
    use std::fs;
    let runner = locate_runner().expect("build galla-runner outside the worktree");
    let backend = locate_backend().expect("install Bubblewrap on the host");
    let worktree = git_worktree_root().unwrap();
    let project = tempfile::tempdir_in(&worktree).unwrap();
    let component = project.path().join("member");
    let provider = project.path().join("provider");
    fs::create_dir_all(component.join("src")).unwrap();
    fs::create_dir_all(component.join("tests")).unwrap();
    fs::write(component.join("tests/CONTRACT.md"), "local test context").unwrap();
    fs::create_dir_all(provider.join("src")).unwrap();
    for name in [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        fs::write(component.join(name), "protected intent").unwrap();
        fs::write(provider.join(name), "excluded provider intent").unwrap();
    }
    fs::create_dir(provider.join(".kvist")).unwrap();
    fs::write(provider.join(".kvist/private"), "excluded state").unwrap();
    fs::write(provider.join("src/CONTRACT.md"), "excluded nested intent").unwrap();
    fs::create_dir(provider.join("src/.kvist")).unwrap();
    fs::write(provider.join("src/.kvist/private"), "excluded nested state").unwrap();
    fs::write(project.path().join("Cargo.toml"),
            "[workspace]\nresolver=\"2\"\nmembers=[\"member\",\"provider\"]\n[workspace.package]\nedition=\"2021\"\n[workspace.dependencies]\nprovider={path=\"provider\"}\n").unwrap();
    fs::write(component.join("Cargo.toml"),
            "[package]\nname=\"member\"\nversion=\"0.1.0\"\nedition.workspace=true\n[dependencies]\nprovider.workspace=true\n").unwrap();
    fs::write(
        provider.join("Cargo.toml"),
        "[package]\nname=\"provider\"\nversion=\"0.1.0\"\nedition.workspace=true\n",
    )
    .unwrap();
    fs::write(component.join("src/lib.rs"),
            "pub fn answer() -> u8 { provider::answer() }\n#[test] fn works() { assert_eq!(answer(), 42); }\n").unwrap();
    fs::write(
        provider.join("src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )
    .unwrap();
    let lock = "version = 3\n[[package]]\nname=\"member\"\nversion=\"0.1.0\"\ndependencies=[\"provider\"]\n[[package]]\nname=\"provider\"\nversion=\"0.1.0\"\n";
    fs::write(project.path().join("Cargo.lock"), lock).unwrap();
    let config = resolve_sandbox_config(&runner, &backend);
    let expected_runner = runner_identity(&config, project.path(), VcsSelection::Auto).unwrap();
    let expected_backend = backend_identity(&config, project.path(), VcsSelection::Auto).unwrap();
    let default = Command::new("/usr/bin/rustup")
        .arg("default")
        .current_dir("/")
        .env_clear()
        .env("HOME", std::env::var_os("HOME").unwrap())
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(default.status.success());
    let default = String::from_utf8(default.stdout).unwrap();
    let has_formatter = PathBuf::from(std::env::var_os("HOME").unwrap())
        .join(".rustup/toolchains")
        .join(default.split_whitespace().next().unwrap())
        .join("bin/rustfmt")
        .is_file();
    let formatting = if has_formatter {
        "cargo fmt; cargo fmt --check;"
    } else {
        ""
    };
    let result = execute_with_timeout(&config, ExecutionRequest {
            project_root: project.path(), vcs_selection: VcsSelection::Auto, component_dir: &component,
            phase: ExecutionPhase::Authoring, program: "/usr/bin/bash",
            arguments: &["-c".into(), format!("set -eu; {formatting} cargo test --lib; printf '\\n#[test] fn edited_source() {{ assert_eq!(answer(), 42); }}\\n' >> src/lib.rs; printf '#[test] fn newly_authored_integration() {{ assert_eq!(member::answer(), 42); }}\\n' > tests/new.rs; cargo test; test \"$(pwd)\" = /workspace/component; test ! -w /rust/project/provider/src/lib.rs; test ! -w /rust/project/Cargo.lock; test ! -e /rust/project/provider/CONTRACT.md; test ! -e /rust/project/provider/.kvist; test ! -e /rust/project/provider/src/CONTRACT.md; test ! -e /rust/project/provider/src/.kvist; if printf forbidden >> /rust/project/provider/src/lib.rs; then exit 1; fi; printf workspace-proof")],
            environment: [("PATH".into(), "/usr/bin:/bin".into())].into(), read_only_mounts: &[],
            scratch_host_dir: None, backend: &expected_backend, policy_identity: POLICY_IDENTITY,
        }, ExecutionOptions { timeout: Some(Duration::from_secs(30)), output_limit: Some(16384),
            live_stdout: None }, &expected_runner).unwrap();
    assert!(
        result.output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.output.stdout),
        String::from_utf8_lossy(&result.output.stderr)
    );
    assert!(String::from_utf8_lossy(&result.output.stdout).ends_with("workspace-proof"));
    assert_eq!(
        fs::read_to_string(project.path().join("Cargo.lock")).unwrap(),
        lock
    );
    assert!(!provider.join("src/forbidden").exists());
    assert_eq!(
        fs::read_to_string(provider.join("src/lib.rs")).unwrap(),
        "pub fn answer() -> u8 { 42 }\n"
    );
    assert!(String::from_utf8_lossy(&result.output.stdout).contains("edited_source"));
    assert!(String::from_utf8_lossy(&result.output.stdout).contains("newly_authored_integration"));
}

#[test]
fn generic_authoring_preserves_environment_without_rustup_state() {
    let runner = locate_runner().expect("install galla-runner outside the worktree");
    let backend = locate_backend().expect("install Bubblewrap");
    let worktree = git_worktree_root().expect("run inside a git worktree");
    let container = tempfile::tempdir_in(&worktree).unwrap();
    std::fs::write(container.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let project = tempfile::tempdir_in(container.path()).unwrap();
    std::fs::create_dir(project.path().join("src")).unwrap();
    std::fs::write(project.path().join("src/input"), "input").unwrap();
    for name in [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        std::fs::write(project.path().join(name), "protected context").unwrap();
    }
    let config = resolve_sandbox_config(&runner, &backend);
    let expected_runner = runner_identity(&config, project.path(), VcsSelection::Auto).unwrap();
    let expected_backend = backend_identity(&config, project.path(), VcsSelection::Auto).unwrap();
    ensure_available(
        &config,
        project.path(),
        VcsSelection::Auto,
        &expected_runner,
        &expected_backend,
    )
    .unwrap();
    let arguments = vec![
        "-c".into(),
        "test \"$PATH\" = /usr/bin:/bin && test \"$HOME\" = /tmp && test -z \"$RUSTUP_HOME\" && test ! -e /rust && printf repaired > src/output".into(),
    ];
    let result = execute_with_timeout(
        &config,
        ExecutionRequest {
            project_root: container.path(),
            vcs_selection: VcsSelection::Auto,
            component_dir: project.path(),
            phase: ExecutionPhase::Authoring,
            program: "/usr/bin/bash",
            arguments: &arguments,
            environment: [
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("HOME".into(), "/tmp".into()),
            ]
            .into(),
            read_only_mounts: &[],
            scratch_host_dir: None,
            backend: &expected_backend,
            policy_identity: POLICY_IDENTITY,
        },
        ExecutionOptions {
            timeout: Some(Duration::from_secs(30)),
            output_limit: Some(16384),
            live_stdout: None,
        },
        &expected_runner,
    )
    .unwrap();
    assert!(
        result.output.status.success(),
        "generic authoring failed: {}",
        String::from_utf8_lossy(&result.output.stderr)
    );
    assert_eq!(
        std::fs::read(project.path().join("src/output")).unwrap(),
        b"repaired"
    );
}

#[test]
#[ignore = "requires live sandbox environment"]
fn sandbox_rustc_version_is_available() {
    let Some(runner) = locate_runner() else {
        eprintln!("skip: no built sandbox runner; build it first");
        return;
    };
    let Some(bwrap) = locate_backend() else {
        eprintln!("skip: no bubblewrap backend on PATH");
        return;
    };
    if !Command::new("rustc").arg("--version").output().is_ok() {
        eprintln!("skip: rustc not on PATH");
        return;
    }
    let Some(worktree) = git_worktree_root() else {
        eprintln!("skip: not inside a git worktree");
        return;
    };
    let config = resolve_sandbox_config(&runner, &bwrap);

    let expected_runner = match runner_identity(&config, &worktree, VcsSelection::Auto) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("skip: could not resolve runner identity: {e}");
            return;
        }
    };
    let expected_backend = match backend_identity(&config, &worktree, VcsSelection::Auto) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("skip: could not resolve backend identity: {e}");
            return;
        }
    };
    if let Err(e) = ensure_available(
        &config,
        &worktree,
        VcsSelection::Auto,
        &expected_runner,
        &expected_backend,
    ) {
        eprintln!("skip: sandbox not available: {e}");
        return;
    }

    let result = run_sandboxed(
        &config,
        &worktree,
        "rustc",
        &["--version".to_owned()],
        &expected_runner,
        &expected_backend,
    );

    let result = match result {
        Some(r) => r,
        None => panic!("sandbox execution failed"),
    };

    assert!(
        result.output.status.success(),
        "rustc --version failed in sandbox: {}",
        String::from_utf8_lossy(&result.output.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.output.stdout);
    assert!(
        stdout.contains("rustc"),
        "expected rustc version string, got: {}",
        stdout
    );
    eprintln!("rustc version in sandbox: {}", stdout.trim());
}

#[test]
#[ignore = "requires live sandbox environment"]
fn sandbox_cargo_version_is_available() {
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
    let Some(worktree) = git_worktree_root() else {
        eprintln!("skip: not inside a git worktree");
        return;
    };
    let config = resolve_sandbox_config(&runner, &bwrap);

    let expected_runner = match runner_identity(&config, &worktree, VcsSelection::Auto) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("skip: could not resolve runner identity: {e}");
            return;
        }
    };
    let expected_backend = match backend_identity(&config, &worktree, VcsSelection::Auto) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("skip: could not resolve backend identity: {e}");
            return;
        }
    };
    if let Err(e) = ensure_available(
        &config,
        &worktree,
        VcsSelection::Auto,
        &expected_runner,
        &expected_backend,
    ) {
        eprintln!("skip: sandbox not available: {e}");
        return;
    }

    let result = run_sandboxed(
        &config,
        &worktree,
        "cargo",
        &["--version".to_owned()],
        &expected_runner,
        &expected_backend,
    );

    let result = match result {
        Some(r) => r,
        None => panic!("sandbox execution failed"),
    };

    assert!(
        result.output.status.success(),
        "cargo --version failed in sandbox: {}",
        String::from_utf8_lossy(&result.output.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.output.stdout);
    assert!(
        stdout.contains("cargo"),
        "expected cargo version string, got: {}",
        stdout
    );
    eprintln!("cargo version in sandbox: {}", stdout.trim());
}

#[test]
#[ignore = "requires live sandbox environment and clippy component"]
fn sandbox_clippy_is_available() {
    let Some(runner) = locate_runner() else {
        eprintln!("skip: no built sandbox runner; build it first");
        return;
    };
    let Some(bwrap) = locate_backend() else {
        eprintln!("skip: no bubblewrap backend on PATH");
        return;
    };
    if !Command::new("cargo")
        .arg("clippy")
        .arg("--version")
        .output()
        .is_ok()
    {
        eprintln!("skip: cargo clippy not available");
        return;
    }
    let Some(worktree) = git_worktree_root() else {
        eprintln!("skip: not inside a git worktree");
        return;
    };
    let config = resolve_sandbox_config(&runner, &bwrap);

    let expected_runner = match runner_identity(&config, &worktree, VcsSelection::Auto) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("skip: could not resolve runner identity: {e}");
            return;
        }
    };
    let expected_backend = match backend_identity(&config, &worktree, VcsSelection::Auto) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("skip: could not resolve backend identity: {e}");
            return;
        }
    };
    if let Err(e) = ensure_available(
        &config,
        &worktree,
        VcsSelection::Auto,
        &expected_runner,
        &expected_backend,
    ) {
        eprintln!("skip: sandbox not available: {e}");
        return;
    }

    let result = run_sandboxed(
        &config,
        &worktree,
        "cargo",
        &["clippy".to_owned(), "--version".to_owned()],
        &expected_runner,
        &expected_backend,
    );

    let result = match result {
        Some(r) => r,
        None => panic!("sandbox execution failed"),
    };

    assert!(
        result.output.status.success(),
        "cargo clippy --version failed in sandbox: {}",
        String::from_utf8_lossy(&result.output.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.output.stdout);
    assert!(
        stdout.contains("clippy"),
        "expected clippy version string, got: {}",
        stdout
    );
    eprintln!("clippy version in sandbox: {}", stdout.trim());
}

#[test]
#[ignore = "requires live sandbox environment"]
fn sandbox_cargo_build_with_rustc() {
    // This test verifies that cargo can invoke rustc to compile code
    // inside the sandbox, which is the key scenario that was failing
    // before the toolchain root mount fix.
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
    let Some(worktree) = git_worktree_root() else {
        eprintln!("skip: not inside a git worktree");
        return;
    };
    let config = resolve_sandbox_config(&runner, &bwrap);

    let expected_runner = match runner_identity(&config, &worktree, VcsSelection::Auto) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("skip: could not resolve runner identity: {e}");
            return;
        }
    };
    let expected_backend = match backend_identity(&config, &worktree, VcsSelection::Auto) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("skip: could not resolve backend identity: {e}");
            return;
        }
    };
    if let Err(e) = ensure_available(
        &config,
        &worktree,
        VcsSelection::Auto,
        &expected_runner,
        &expected_backend,
    ) {
        eprintln!("skip: sandbox not available: {e}");
        return;
    }

    let project = tempfile::tempdir_in(&worktree).expect("create isolated native Cargo fixture");
    let project_dir = project.path();
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir).expect("failed to create src dir");

    let cargo_toml = r#"
[package]
name = "hello-world"
version = "0.1.0"
edition = "2024"
"#;
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml).expect("failed to write Cargo.toml");
    std::fs::write(
        project_dir.join("Cargo.lock"),
        "version=4\n[[package]]\nname=\"hello-world\"\nversion=\"0.1.0\"\n",
    )
    .expect("write the separately provisioned lock");

    let main_rs = r#"
fn main() {
    println!("Hello from sandbox!");
}
"#;
    std::fs::write(src_dir.join("main.rs"), main_rs).expect("failed to write main.rs");

    let result = run_sandboxed(
        &config,
        project_dir,
        "cargo",
        &[
            "build".to_owned(),
            "--offline".to_owned(),
            "--locked".to_owned(),
        ],
        &expected_runner,
        &expected_backend,
    );

    let result = match result {
        Some(r) => r,
        None => panic!("sandbox execution failed"),
    };

    assert!(
        result.output.status.success(),
        "cargo build failed in sandbox: {}",
        String::from_utf8_lossy(&result.output.stderr)
    );
    eprintln!("cargo build succeeded in sandbox");
}
