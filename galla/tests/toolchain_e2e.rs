//! Native executable evidence for explicit toolchain grants, not consumer discovery.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn successful(output: Output, label: &str) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{label}: status={}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

struct Sandbox {
    _directory: tempfile::TempDir,
    workspace: PathBuf,
    request: Value,
    runner: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let runner = PathBuf::from(
            std::env::var_os("KVIST_GALLA")
                .expect("set KVIST_GALLA to the built galla-runner outside the worktree"),
        )
        .canonicalize()
        .unwrap();
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        assert!(
            !runner.starts_with(repository),
            "runner must be outside the worktree"
        );
        let probe = successful(
            Command::new(&runner)
                .arg("--kvist-sandbox-probe-v1")
                .env_clear()
                .output()
                .unwrap(),
            "native enforcement probe",
        );
        let probe: Value = serde_json::from_slice(&probe).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        let scratch = directory.path().join("scratch");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&scratch).unwrap();
        let identity = digest(b"native-toolchain-fixture");
        let request = json!({
            "protocol": "kvist-sandbox-request-v1",
            "protocol_version": 1,
            "phase": "authoring",
            "argv": ["/usr/bin/true"],
            "working_directory": "/workspace/component",
            "environment": {
                "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
                "HOME": "/tmp",
                "TMPDIR": "/tmp",
                "CARGO_HOME": "/tmp/cargo-home",
                "CARGO_TARGET_DIR": "/tmp/target",
                "CARGO_NET_OFFLINE": "true",
                "RUSTUP_AUTO_INSTALL": "0",
                "GOPATH": "/tmp/go",
                "GOCACHE": "/tmp/go-cache",
                "GOTOOLCHAIN": "local",
                "GOPROXY": "off",
                "GOSUMDB": "off"
            },
            "network": {"mode": "deny", "allowed_sources": []},
            "resources": {
                "wall_time_ms": 120000,
                "max_output_bytes": 1048576,
                "max_processes": 1024,
                "max_files": 65536,
                "max_file_bytes": 268435456,
                "max_scratch_bytes": 1073741824
            },
            "identities": {
                "runner": digest(&fs::read(&runner).unwrap()),
                "backend": probe["backend"],
                "policy": identity,
                "toolchain": identity,
                "command": identity,
                "mount_plan": identity
            },
            "grants": [
                {
                    "source": workspace.canonicalize().unwrap(),
                    "destination": "/workspace/component",
                    "access": "read-write",
                    "purpose": "authoring",
                    "identity": identity
                },
                {
                    "source": scratch.canonicalize().unwrap(),
                    "destination": "/workspace/scratch",
                    "access": "read-write",
                    "purpose": "scratch",
                    "identity": identity
                }
            ],
            "toolchain": {"kind": "system", "root": "/usr", "identity": identity},
            "cache": null,
            "scratch": {"destination": "/workspace/scratch", "identity": identity}
        });
        Self {
            _directory: directory,
            workspace,
            request,
            runner,
        }
    }

    fn grant(&mut self, source: &Path, destination: &str) {
        self.request["grants"].as_array_mut().unwrap().push(json!({
            "source": source.canonicalize().unwrap(),
            "destination": destination,
            "access": "read-only",
            "purpose": "toolchain",
            "identity": digest(destination.as_bytes())
        }));
    }

    fn run(&self, argv: &[&str]) -> Output {
        let mut request = self.request.clone();
        request["argv"] = json!(argv);
        let bytes = serde_json::to_vec(&request).unwrap();
        galla::validation::parse_and_validate(&bytes).unwrap();
        let mut child = Command::new(&self.runner)
            .arg("--kvist-sandbox-request-v1")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&bytes).unwrap();
        child.wait_with_output().unwrap()
    }

    fn checked(&self, argv: &[&str]) -> Vec<u8> {
        successful(self.run(argv), &format!("sandbox {argv:?}"))
    }

    fn shell(&self, script: &str) -> Vec<u8> {
        self.checked(&["/usr/bin/sh", "-ec", script])
    }
}

#[test]
#[ignore = "requires KVIST_GALLA, native Bubblewrap and system Python/Node/Go/C/C++"]
fn system_toolchains_are_found_and_execute_real_programs() {
    let sandbox = Sandbox::new();
    for (name, contents) in [
        (
            "trial.py",
            "assert sum([20, 22]) == 42\nprint('python-e2e=42')\n",
        ),
        (
            "trial.js",
            "if (20 + 22 !== 42) process.exit(1); console.log('node-e2e=42');\n",
        ),
        (
            "trial.go",
            "package main\nimport \"fmt\"\nfunc main() { fmt.Println(\"go-e2e=42\") }\n",
        ),
        (
            "trial.c",
            "#include <stdio.h>\nint main(void) { puts(\"c-e2e=42\"); return 0; }\n",
        ),
        (
            "trial.cpp",
            "#include <iostream>\nint main() { std::cout << \"cpp-e2e=42\\n\"; }\n",
        ),
    ] {
        fs::write(sandbox.workspace.join(name), contents).unwrap();
    }
    let output = sandbox.shell(
        "for tool in python3 node go gcc g++; do command -v \"$tool\"; done\n\
         python3 trial.py\n\
         node trial.js\n\
         go build -o /tmp/go-trial trial.go\n\
         /tmp/go-trial\n\
         gcc -Wall -Werror trial.c -o /tmp/c-trial\n\
         /tmp/c-trial\n\
         g++ -Wall -Werror trial.cpp -o /tmp/cpp-trial\n\
         /tmp/cpp-trial",
    );
    let output = String::from_utf8(output).unwrap();
    for marker in [
        "python-e2e=42",
        "node-e2e=42",
        "go-e2e=42",
        "c-e2e=42",
        "cpp-e2e=42",
    ] {
        assert!(
            output.lines().any(|line| line == marker),
            "{marker}: {output}"
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct EntryState {
    mode: u32,
    length: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    content: String,
}

type Snapshot = BTreeMap<PathBuf, EntryState>;

fn snapshot_tree(path: &Path, snapshot: &mut Snapshot) {
    let metadata = fs::symlink_metadata(path).unwrap();
    let content = if metadata.is_symlink() {
        format!("link:{:?}", fs::read_link(path).unwrap())
    } else if metadata.is_file() {
        let mut file = fs::File::open(path).unwrap();
        let mut hash = Sha256::new();
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
        assert!(
            metadata.is_dir(),
            "unexpected special file: {}",
            path.display()
        );
        "directory".into()
    };
    snapshot.insert(
        path.to_owned(),
        EntryState {
            mode: metadata.mode(),
            length: metadata.len(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            content,
        },
    );
    if metadata.is_dir() {
        for entry in fs::read_dir(path).unwrap() {
            snapshot_tree(&entry.unwrap().path(), snapshot);
        }
    }
}

fn snapshot(roots: &[PathBuf]) -> Snapshot {
    let mut state = Snapshot::new();
    for root in roots {
        snapshot_tree(root, &mut state);
    }
    state
}

fn host_rustup(home: &Path, directory: &Path, args: &[&str]) -> Vec<u8> {
    successful(
        Command::new("/usr/bin/rustup")
            .args(args)
            .current_dir(directory)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("RUSTUP_HOME", home)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .output()
            .unwrap(),
        &format!("host rustup {args:?}"),
    )
}

fn lines(bytes: &[u8]) -> Vec<String> {
    let mut result: Vec<_> = String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    result.sort();
    result
}

fn toolchain_names(bytes: &[u8]) -> Vec<String> {
    let mut names: Vec<_> = lines(bytes)
        .iter()
        .map(|line| line.split_whitespace().next().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

fn assert_read_only_failure(output: Output, label: &str) {
    assert!(!output.status.success(), "{label} must fail");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("Read-only file system"),
        "{label} must fail because the installation is read-only, not for an unrelated reason: {error}"
    );
}

#[test]
#[ignore = "requires KVIST_GALLA, native Bubblewrap and installed /usr/bin/rustup toolchains"]
fn rustup_reports_and_uses_every_host_toolchain_without_host_mutation() {
    let mut sandbox = Sandbox::new();
    let host_home = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("HOME or RUSTUP_HOME required"))
                .join(".rustup")
        })
        .canonicalize()
        .unwrap();
    // Capture before even the host inventory queries, not just sandbox execution.
    let mut roots = vec![host_home.clone()];
    let before_home = snapshot(&roots);
    let names = toolchain_names(&host_rustup(
        &host_home,
        &sandbox.workspace,
        &["toolchain", "list"],
    ));
    assert!(
        names.len() >= 2,
        "provision at least two toolchains on the host to exercise full-inventory visibility"
    );
    let private_home = sandbox._directory.path().join("rustup-home");
    fs::create_dir_all(private_home.join("toolchains")).unwrap();
    fs::write(
        private_home.join("settings.toml"),
        format!(
            "version = \"12\"\ndefault_toolchain = {}\nprofile = \"minimal\"\n",
            serde_json::to_string(&names[0]).unwrap()
        ),
    )
    .unwrap();
    sandbox.request["environment"]["RUSTUP_HOME"] = json!("/tooling/rustup-home");
    sandbox.request["environment"]["RUSTUP_TOOLCHAIN"] = json!(&names[0]);
    for (index, name) in names.iter().enumerate() {
        assert!(
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
            "unsupported installed toolchain name: {name}"
        );
        let compiler = host_rustup(
            &host_home,
            &sandbox.workspace,
            &["which", "--toolchain", name, "rustc"],
        );
        let compiler = PathBuf::from(String::from_utf8(compiler).unwrap().trim());
        let root = compiler
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        if !root.starts_with(&host_home) && !roots.contains(&root) {
            roots.push(root.clone());
        }
        let destination = format!("/tooling/toolchains/{index}");
        symlink(&destination, private_home.join("toolchains").join(name)).unwrap();
        sandbox.grant(&root, &destination);
    }
    let mut before = before_home;
    for root in roots.iter().skip(1) {
        snapshot_tree(root, &mut before);
    }
    sandbox.grant(&private_home, "/tooling/rustup-home");
    fs::write(
        sandbox.workspace.join("trial.rs"),
        "fn main() { assert_eq!(20 + 22, 42); println!(\"rust-e2e=42\"); }\n",
    )
    .unwrap();
    fs::write(
        sandbox.workspace.join("library.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )
    .unwrap();
    fs::create_dir(sandbox.workspace.join("src")).unwrap();
    fs::write(
        sandbox.workspace.join("Cargo.toml"),
        "[package]\nname=\"inventory_trial\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    fs::write(
        sandbox.workspace.join("Cargo.lock"),
        "version = 3\n[[package]]\nname = \"inventory_trial\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        sandbox.workspace.join("src/lib.rs"),
        "#[test] fn real_test() { assert_eq!(20 + 22, 42); }\n",
    )
    .unwrap();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut saw_cross_target = false;
        assert_eq!(
            names,
            toolchain_names(&sandbox.checked(&["/usr/bin/rustup", "toolchain", "list"]))
        );
        sandbox.shell("command -v rustup; test ! -e /root; test ! -e /home");
        for name in &names {
            for arguments in [
                vec!["run", name, "rustc", "-Vv"],
                vec!["run", name, "cargo", "--version"],
                vec!["run", name, "rustdoc", "--version"],
                vec!["target", "list", "--toolchain", name],
                vec!["target", "list", "--installed", "--toolchain", name],
                vec!["component", "list", "--installed", "--toolchain", name],
            ] {
                let expected = host_rustup(&host_home, &sandbox.workspace, &arguments);
                let argv: Vec<_> = std::iter::once("/usr/bin/rustup")
                    .chain(arguments.iter().copied())
                    .collect();
                assert_eq!(
                    lines(&expected),
                    lines(&sandbox.checked(&argv)),
                    "{name}: {arguments:?}"
                );
            }
            let targets = lines(&host_rustup(
                &host_home,
                &sandbox.workspace,
                &["target", "list", "--installed", "--toolchain", name],
            ));
            assert!(!targets.is_empty(), "{name}: no installed targets");
            let compiler = host_rustup(
                &host_home,
                &sandbox.workspace,
                &["run", name, "rustc", "-Vv"],
            );
            let native_target = String::from_utf8(compiler)
                .unwrap()
                .lines()
                .find_map(|line| line.strip_prefix("host: ").map(str::to_owned))
                .expect("rustc -Vv must report a host target");
            saw_cross_target |= targets.iter().any(|target| target != &native_target);
            for target in &targets {
                sandbox.checked(&[
                    "/usr/bin/rustup",
                    "run",
                    name,
                    "rustc",
                    "--crate-type",
                    "lib",
                    "--target",
                    target,
                    "library.rs",
                    "-o",
                    "/tmp/library.rlib",
                ]);
            }
            assert!(
                saw_cross_target,
                "provision at least one non-host target to exercise installed-target visibility"
            );
            assert_eq!(
                sandbox.checked(&[
                    "/usr/bin/rustup",
                    "run",
                    name,
                    "rustc",
                    "trial.rs",
                    "-o",
                    "/workspace/scratch/rust-trial"
                ]),
                b""
            );
            assert_eq!(
                sandbox.checked(&["/workspace/scratch/rust-trial"]),
                b"rust-e2e=42\n"
            );
            let tests = sandbox.checked(&[
                "/usr/bin/rustup",
                "run",
                name,
                "cargo",
                "test",
                "--offline",
                "--locked",
            ]);
            assert!(
                String::from_utf8(tests)
                    .unwrap()
                    .contains("1 passed; 0 failed")
            );
            assert_read_only_failure(
                sandbox.run(&["/usr/bin/rustup", "toolchain", "uninstall", name]),
                &format!("uninstall {name}"),
            );
            assert_read_only_failure(
                sandbox.run(&[
                    "/usr/bin/rustup",
                    "target",
                    "remove",
                    "--toolchain",
                    name,
                    &targets[0],
                ]),
                &format!("target removal for {name}"),
            );
        }
        assert_read_only_failure(
            sandbox.run(&["/usr/bin/rustup", "set", "profile", "complete"]),
            "private rustup settings mutation",
        );
        assert_eq!(
            names,
            toolchain_names(&sandbox.checked(&["/usr/bin/rustup", "toolchain", "list"]))
        );
    }));
    let after = snapshot(&roots);
    let changed: Vec<_> = before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(*key) != after.get(*key))
        .collect();
    assert!(
        changed.is_empty(),
        "host rustup/toolchain state changed: {changed:?}"
    );
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
