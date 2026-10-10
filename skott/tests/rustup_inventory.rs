//! Real Skott preparation and execution evidence for the complete Rust inventory.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use sav::{CancellationToken, ToolIntent};
use serde_json::json;
use sha2::{Digest, Sha256};
use skott::session::ToolExecutor;
use skott::toolchain::HostProbe;
use skott::{ProfileSetting, SandboxExecutor, SandboxPaths, ToolPolicy, ToolProfile, ToolRegistry};

fn host(home: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("/usr/bin/rustup")
        .args(args)
        .current_dir("/")
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .env("RUSTUP_AUTO_INSTALL", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn lines(bytes: &[u8]) -> Vec<String> {
    let mut lines: Vec<_> = String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    lines.sort();
    lines
}

fn names(bytes: &[u8]) -> Vec<String> {
    lines(bytes)
        .iter()
        .map(|line| line.split_whitespace().next().unwrap().to_owned())
        .collect()
}

fn shell(executor: &SandboxExecutor, script: &str) -> skott::ToolOutcome {
    executor
        .execute(
            &ToolIntent {
                id: "inventory".into(),
                provider_id: None,
                name: "shell".into(),
                arguments: json!({"command": script}),
            },
            &CancellationToken::new(),
        )
        .unwrap()
}

fn checked(executor: &SandboxExecutor, script: &str) -> Vec<u8> {
    let outcome = shell(executor, script);
    assert!(
        !outcome.failed(),
        "{script}\n{}\n{}",
        outcome.output_text(65536),
        outcome.error_text(65536)
    );
    outcome.stdout
}

type HostState = BTreeMap<PathBuf, (u32, u64, i64, i64, i64, i64, String)>;

fn snapshot(path: &Path, state: &mut HostState) {
    let meta = fs::symlink_metadata(path).unwrap();
    let content = if meta.is_symlink() {
        format!("link:{:?}", fs::read_link(path).unwrap())
    } else if meta.is_file() {
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
        assert!(meta.is_dir());
        "directory".into()
    };
    state.insert(
        path.to_owned(),
        (
            meta.mode(),
            meta.len(),
            meta.mtime(),
            meta.mtime_nsec(),
            meta.ctime(),
            meta.ctime_nsec(),
            content,
        ),
    );
    if meta.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            snapshot(&entry.unwrap().path(), state);
        }
    }
}

#[test]
#[ignore = "requires KVIST_RUST_TEST_RUNNER, native Bubblewrap and multiple installed Rust toolchains/targets"]
fn skott_discovers_and_uses_all_installed_versions_and_targets_read_only() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let rustup_home = home.join(".rustup");
    let mut before = HostState::new();
    snapshot(&rustup_home, &mut before);
    let directory = tempfile::Builder::new()
        .prefix(".rust-inventory-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let workspace = directory.path().canonicalize().unwrap();
    fs::write(workspace.join("rust-toolchain"), "stable").unwrap();
    fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname=\"inventory_trial\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    fs::write(
        workspace.join("Cargo.lock"),
        "version = 3\n[[package]]\nname = \"inventory_trial\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        workspace.join("library.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )
    .unwrap();
    fs::write(
        workspace.join("program.rs"),
        "fn main() { println!(\"rust-inventory=42\"); }\n",
    )
    .unwrap();
    fs::create_dir(workspace.join("src")).unwrap();
    fs::write(workspace.join("src/lib.rs"),
        "/// ```\n/// assert_eq!(inventory_trial::answer(), 42);\n/// ```\npub fn answer() -> u8 { 42 }\n#[test] fn works() { assert_eq!(answer(), 42); }\n").unwrap();
    fs::write(workspace.join("build.rs"),
        "fn main() { for key in [\"RUSTC\", \"RUSTDOC\"] { let output = std::process::Command::new(std::env::var(key).unwrap()).arg(\"--version\").output().unwrap(); assert!(output.status.success()); println!(\"cargo:warning=inventory-{}={}\", key, String::from_utf8(output.stdout).unwrap().trim()); } }\n").unwrap();
    let lock = fs::read(workspace.join("Cargo.lock")).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let installed = names(&host(&home, &["toolchain", "list"]));
        assert!(
            installed.len() >= 2,
            "provision multiple installed toolchains separately"
        );
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
        let registry = ToolRegistry::resolve_for_workspace(
            ToolPolicy::default(),
            &settings,
            &HostProbe,
            None,
            &workspace,
        )
        .unwrap();
        let executor = SandboxExecutor::new(
            registry,
            SandboxPaths {
                runner: PathBuf::from(
                    std::env::var_os("KVIST_RUST_TEST_RUNNER")
                        .expect("select a real outside-worktree runner"),
                ),
                backend: "/usr/bin/bwrap".into(),
            },
            workspace.clone(),
        );
        assert_eq!(
            installed,
            names(&checked(&executor, "rustup toolchain list"))
        );
        checked(
            &executor,
            "test \"$HOME\" = /tmp && test \"$CARGO_HOME\" = /tmp/cargo-home && test \"$CARGO_TARGET_DIR\" = /tmp/target && test \"$RUSTUP_HOME\" = /rust/rustup-home && test \"$RUSTUP_AUTO_INSTALL\" = 0 && test \"$CARGO_NET_OFFLINE\" = true && test ! -e /home && test ! -e /root && touch /tmp/private-scratch",
        );
        let selected = checked(&executor, "rustup show active-toolchain");
        assert!(String::from_utf8(selected).unwrap().starts_with("stable-"));
        let mut cross_target = false;
        for name in &installed {
            assert!(
                name.bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            );
            for arguments in [
                vec!["run", name, "rustc", "-Vv"],
                vec!["run", name, "cargo", "--version"],
                vec!["run", name, "rustdoc", "--version"],
                vec!["target", "list", "--toolchain", name],
                vec!["target", "list", "--installed", "--toolchain", name],
                vec!["component", "list", "--installed", "--toolchain", name],
            ] {
                assert_eq!(
                    lines(&host(&home, &arguments)),
                    lines(&checked(
                        &executor,
                        &format!("rustup {}", arguments.join(" "))
                    )),
                    "{name}: {arguments:?}"
                );
            }
            let version = host(&home, &["run", name, "rustc", "--version"]);
            let doc_version = host(&home, &["run", name, "rustdoc", "--version"]);
            assert_eq!(
                checked(&executor, &format!("rustc +{name} --version")),
                version
            );
            assert_eq!(
                checked(&executor, &format!("rustdoc +{name} --version")),
                doc_version
            );
            let compiler = String::from_utf8(host(&home, &["run", name, "rustc", "-Vv"])).unwrap();
            let native = compiler
                .lines()
                .find_map(|line| line.strip_prefix("host: "))
                .unwrap();
            let targets = lines(&host(
                &home,
                &["target", "list", "--installed", "--toolchain", name],
            ));
            assert!(!targets.is_empty());
            cross_target |= targets.iter().any(|target| target != native);
            for target in &targets {
                assert!(
                    target
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
                );
                checked(
                    &executor,
                    &format!(
                        "rustup run {name} rustc --crate-type lib --target {target} library.rs -o /tmp/trial.rlib"
                    ),
                );
            }
            assert_eq!(
                checked(
                    &executor,
                    &format!("rustc +{name} program.rs -o /tmp/program && /tmp/program")
                ),
                b"rust-inventory=42\n"
            );
            for command in [
                format!("cargo +{name} test"),
                format!("RUSTUP_TOOLCHAIN={name} cargo test"),
                format!("rustup run {name} cargo test --offline --locked"),
            ] {
                let outcome = shell(&executor, &command);
                assert!(
                    !outcome.failed(),
                    "{command}: {}",
                    outcome.error_text(65536)
                );
                let output = format!(
                    "{}\n{}",
                    outcome.output_text(65536),
                    outcome.error_text(65536)
                );
                assert!(output.contains("1 passed; 0 failed"), "{command}: {output}");
                assert!(
                    output.contains(&format!(
                        "inventory-RUSTC={}",
                        String::from_utf8_lossy(&version).trim()
                    )),
                    "{command}: {output}"
                );
                assert!(
                    output.contains(&format!(
                        "inventory-RUSTDOC={}",
                        String::from_utf8_lossy(&doc_version).trim()
                    )),
                    "{command}: {output}"
                );
            }
            let denied = shell(&executor, &format!("rustup toolchain uninstall {name}"));
            assert!(denied.failed());
            assert!(denied.error_text(8192).contains("Read-only file system"));
            let denied = shell(
                &executor,
                &format!("rustup target remove --toolchain {name} {}", targets[0]),
            );
            assert!(denied.failed());
            assert!(denied.error_text(8192).contains("Read-only file system"));
        }
        assert!(cross_target, "provision a non-host target separately");
        let denied = shell(&executor, "rustup set profile complete");
        assert!(denied.failed());
        assert!(denied.error_text(8192).contains("Read-only file system"));
        let absent = shell(&executor, "cargo +9.99.99 --version");
        assert!(absent.failed());
        let option = shell(&executor, "cargo +--install --version");
        assert!(option.failed());
        assert_eq!(
            installed,
            names(&checked(&executor, "rustup toolchain list"))
        );
        assert_eq!(lock, fs::read(workspace.join("Cargo.lock")).unwrap());
    }));
    let mut after = HostState::new();
    snapshot(&rustup_home, &mut after);
    let changes: Vec<_> = before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .collect();
    assert!(changes.is_empty(), "host installation changed: {changes:?}");
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
