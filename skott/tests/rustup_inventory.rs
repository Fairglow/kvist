//! Real Skott preparation and execution evidence for the complete Rust inventory.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use sav::{CancellationToken, ToolIntent};
use serde_json::json;
use sha2::{Digest, Sha256};
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

type HostEntry = (u32, u64, i64, i64, i64, i64, String);
type HostState = BTreeMap<PathBuf, HostEntry>;

#[test]
fn host_snapshots_keep_all_bytes_metadata_and_links_across_worker_counts() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut empty = HostState::new();
    snapshot_with_workers(root, &mut empty, 8);
    assert_eq!(empty.len(), 1);
    assert_eq!(empty[root].6, "directory");
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("empty"), []).unwrap();
    fs::write(root.join("nested/bytes"), b"\0\xffpayload").unwrap();
    symlink("nested/bytes", root.join("link")).unwrap();
    symlink("nested", root.join("directory-link")).unwrap();
    symlink("/nonexistent/inventory-target", root.join("dangling")).unwrap();
    let mut serial = HostState::new();
    snapshot_with_workers(root, &mut serial, 1);
    assert_eq!(serial.len(), 7);
    assert_eq!(
        serial[&root.join("nested/bytes")].6,
        hex::encode(Sha256::digest(b"\0\xffpayload"))
    );
    assert_eq!(
        serial[&root.join("empty")].6,
        hex::encode(Sha256::digest([]))
    );
    assert_eq!(serial[&root.join("link")].6, "link:\"nested/bytes\"");
    assert!(!serial.contains_key(&root.join("directory-link/bytes")));
    for workers in [2, 8] {
        let mut parallel = HostState::new();
        snapshot_with_workers(root, &mut parallel, workers);
        assert_eq!(parallel, serial);
    }
    fs::write(root.join("nested/bytes"), b"changed").unwrap();
    let mut changed = HostState::new();
    snapshot_with_workers(root, &mut changed, 2);
    assert_ne!(changed, serial);
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
            .and_then(Path::parent)
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

fn snapshot(path: &Path, state: &mut HostState) {
    let workers = std::thread::available_parallelism().unwrap().get().min(8);
    let started = Instant::now();
    eprintln!("host inventory snapshot: starting with {workers} hash workers");
    snapshot_with_workers(path, state, workers);
    eprintln!(
        "host inventory snapshot: {} entries in {:.3}s",
        state.len(),
        started.elapsed().as_secs_f64()
    );
}

fn snapshot_with_workers(path: &Path, state: &mut HostState, workers: usize) {
    assert!((1..=8).contains(&workers));
    let mut files = Vec::new();
    enumerate_snapshot(path, state, &mut files);
    let next = AtomicUsize::new(0);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers.min(files.len()))
            .map(|_| {
                let files = &files;
                let next = &next;
                scope.spawn(move || {
                    let mut results = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = files.get(index) else {
                            break;
                        };
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
                        results.push((path.clone(), hex::encode(hash.finalize())));
                    }
                    results
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    for (path, digest) in results {
        let recorded = state.get_mut(&path).unwrap();
        assert_eq!(
            *recorded,
            host_entry(&fs::symlink_metadata(&path).unwrap(), String::new()),
            "host inventory changed while hashing {}",
            path.display()
        );
        recorded.6 = digest;
    }
}

fn host_entry(meta: &fs::Metadata, content: String) -> HostEntry {
    (
        meta.mode(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
        content,
    )
}

fn enumerate_snapshot(path: &Path, state: &mut HostState, files: &mut Vec<PathBuf>) {
    let meta = fs::symlink_metadata(path).unwrap();
    let content = if meta.is_symlink() {
        format!("link:{:?}", fs::read_link(path).unwrap())
    } else if meta.is_file() {
        files.push(path.to_owned());
        String::new()
    } else {
        assert!(meta.is_dir());
        "directory".into()
    };
    state.insert(path.to_owned(), host_entry(&meta, content));
    if meta.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            enumerate_snapshot(&entry.unwrap().path(), state, files);
        }
    }
}

#[test]
fn skott_discovers_and_uses_all_installed_versions_and_targets_read_only() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let rustup_home = home.join(".rustup");
    let mut before = HostState::new();
    snapshot(&rustup_home, &mut before);
    let directory = tempfile::Builder::new()
        .prefix(".rust-inventory-")
        .tempdir_in(manifest_dir())
        .unwrap();
    let workspace = directory.path().canonicalize().unwrap();
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
        let default = names(&host(&home, &["default"]))[0].clone();
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
                runner: runner(),
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
            "test \"$HOME\" = /tmp && test \"$CARGO_HOME\" = /tmp/cargo-home && test \"$CARGO_TARGET_DIR\" = /workspace/target && test \"$RUSTUP_HOME\" = /tmp/rustup-home && test \"$RUSTUP_AUTO_INSTALL\" = 0 && test \"$CARGO_NET_OFFLINE\" = true && test ! -e /home && test ! -e /root && touch /tmp/private-scratch",
        );
        let selected = checked(&executor, "rustup show active-toolchain");
        assert!(String::from_utf8(selected).unwrap().starts_with(&default));
        assert_eq!(
            checked(&executor, "rustup default"),
            host(&home, &["default"])
        );
        checked(&executor, "rustup default stable");
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
                checked(
                    &executor,
                    &format!("rustup default {name} >&2 && rustc --version")
                ),
                version
            );
            assert_eq!(
                checked(
                    &executor,
                    &format!("rustup default {name} >&2 && rustdoc --version")
                ),
                doc_version
            );
            for tool in ["rustfmt", "clippy-driver", "rust-analyzer"] {
                if home
                    .join(".rustup/toolchains")
                    .join(name)
                    .join("bin")
                    .join(tool)
                    .is_file()
                {
                    assert_eq!(
                        checked(
                            &executor,
                            &format!("rustup default {name} >&2 && {tool} --version")
                        ),
                        host(&home, &["run", name, tool, "--version"]),
                        "{name}: companion {tool} follows the invocation-local default"
                    );
                }
            }
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
                format!("rustup default {name} && cargo test"),
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
            assert!(
                denied.error_text(8192).contains("Read-only file system"),
                "{}",
                denied.error_text(8192)
            );
            let denied = shell(
                &executor,
                &format!("rustup target remove --toolchain {name} {}", targets[0]),
            );
            assert!(denied.failed());
            assert!(
                denied.error_text(8192).contains("Read-only file system")
                    || denied
                        .error_text(8192)
                        .contains("Invalid cross-device link"),
                "{}",
                denied.error_text(8192)
            );
            checked(
                &executor,
                &format!(
                    "test ! -w \"$RUSTUP_HOME/toolchains\" && test ! -w \"$(rustup which --toolchain {name} rustc)\""
                ),
            );
        }
        assert!(cross_target, "provision a non-host target separately");
        checked(&executor, "rustup set profile complete");
        assert_eq!(
            checked(&executor, "rustup default"),
            host(&home, &["default"])
        );
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
