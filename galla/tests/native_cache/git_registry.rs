use super::{execute_request, metadata_request, validate};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const NAME: &str = "native-git-fixture-dep";
const VERSION: &str = "0.1.0";
const INDEX: &str = "https://native-git-registry.example.invalid/index.git";
const ENTRY: &str = "na/ti/native-git-fixture-dep";

struct ArchiveServer {
    origin: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), String>>>,
}

impl ArchiveServer {
    fn start(archive: Vec<u8>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if stopping.load(Ordering::Relaxed) || Instant::now() >= deadline {
                    return Err("fixture archive server cancelled or timed out".into());
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .map_err(|error| error.to_string())?;
                        stream
                            .set_write_timeout(Some(Duration::from_secs(5)))
                            .map_err(|error| error.to_string())?;
                        let mut header = Vec::new();
                        let mut buffer = [0_u8; 1024];
                        while !header.windows(4).any(|window| window == b"\r\n\r\n") {
                            if stopping.load(Ordering::Relaxed) || Instant::now() >= deadline {
                                return Err("fixture archive header cancelled or timed out".into());
                            }
                            let count = stream
                                .read(&mut buffer)
                                .map_err(|error| error.to_string())?;
                            if count == 0 || header.len() + count > 8192 {
                                return Err("invalid fixture archive request".into());
                            }
                            header.extend_from_slice(&buffer[..count]);
                        }
                        if !header
                            .starts_with(format!("GET /{NAME}/{VERSION}/download ").as_bytes())
                        {
                            return Err("unexpected fixture archive path or method".into());
                        }
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            archive.len()
                        )
                        .map_err(|error| error.to_string())?;
                        stream
                            .write_all(&archive)
                            .map_err(|error| error.to_string())?;
                        return Ok(());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
        });
        Self {
            origin,
            stop,
            worker: Some(worker),
        }
    }

    fn finish(&mut self) {
        self.worker.take().unwrap().join().unwrap().unwrap();
    }
}

impl Drop for ArchiveServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn cargo(sysroot: &Path, home: &Path, workspace: &Path, args: &[&str]) -> Output {
    let output = Command::new(sysroot.join("bin/cargo"))
        .args(args)
        .current_dir(workspace)
        .env_clear()
        .env("HOME", home)
        .env("CARGO_HOME", home)
        .env("CARGO_TARGET_DIR", home.join("target"))
        .env("CARGO_CACHE_AUTO_CLEAN_FREQUENCY", "never")
        .env("CARGO_NET_GIT_FETCH_WITH_CLI", "true")
        .env("CARGO_NET_RETRY", "0")
        .env("CARGO_HTTP_PROXY", "")
        .env("CARGO_HTTP_TIMEOUT", "10")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "PATH",
            format!("{}:/usr/bin", sysroot.join("bin").display()),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Cargo {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn git(home: &Path, repository: &Path, args: &[&str]) -> Output {
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PATH", "/usr/bin")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn sole_directory(parent: &Path) -> PathBuf {
    let mut entries = std::fs::read_dir(parent).unwrap();
    let directory = entries.next().unwrap().unwrap().path();
    assert!(directory.is_dir());
    assert!(
        entries.next().is_none(),
        "fixture must have only its declared registry"
    );
    directory
}

#[test]
#[ignore = "explicit real Git-index trial requires installed Rust, Git and Bubblewrap"]
fn native_git_registry_metadata_resolves_cargo_populated_index_without_network() {
    let root = tempfile::Builder::new()
        .prefix(".native-git-registry-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let home = root.path().join("preparation-home");
    let package = root.path().join("package");
    let workspace = root.path().join("component");
    let repository = root.path().join("registry-index");
    let runtime = root.path().join("runtime");
    let scratch = root.path().join("scratch");
    for directory in [&home, &package, &workspace, &repository, &runtime, &scratch] {
        std::fs::create_dir(directory).unwrap();
    }
    for directory in [&package, &workspace] {
        std::fs::create_dir(directory.join("src")).unwrap();
        std::fs::write(directory.join("src/lib.rs"), "").unwrap();
    }
    std::fs::write(
        package.join("Cargo.toml"),
        format!("[workspace]\n[package]\nname='{NAME}'\nversion='{VERSION}'\nedition='2024'\n"),
    )
    .unwrap();
    std::fs::write(workspace.join("Cargo.toml"), format!(
        "[workspace]\n[package]\nname='native-git-registry-consumer'\nversion='0.0.0'\nedition='2024'\n\
        [dependencies]\n{NAME}={{version='={VERSION}',registry='git-alt'}}\n")).unwrap();
    let query = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    assert!(query.status.success());
    let sysroot = PathBuf::from(String::from_utf8(query.stdout).unwrap().trim())
        .canonicalize()
        .unwrap();
    cargo(
        &sysroot,
        &home,
        &package,
        &["generate-lockfile", "--offline"],
    );
    cargo(
        &sysroot,
        &home,
        &package,
        &[
            "package",
            "--offline",
            "--locked",
            "--allow-dirty",
            "--no-verify",
        ],
    );
    let archive =
        std::fs::read(home.join(format!("target/package/{NAME}-{VERSION}.crate"))).unwrap();
    let mut server = ArchiveServer::start(archive.clone());
    std::fs::create_dir_all(repository.join("na/ti")).unwrap();
    std::fs::write(
        repository.join("config.json"),
        serde_json::to_vec(&json!({
            "dl": format!("{}/{{crate}}/{{version}}/download", server.origin)
        }))
        .unwrap(),
    )
    .unwrap();
    let entry = serde_json::to_vec(&json!({
        "name": NAME, "vers": VERSION, "deps": [], "features": {},
        "cksum": hex::encode(Sha256::digest(&archive)), "yanked": false
    }))
    .unwrap();
    std::fs::write(repository.join(ENTRY), &entry).unwrap();
    git(
        &home,
        &repository,
        &["-c", "init.defaultBranch=main", "init", "--quiet"],
    );
    git(&home, &repository, &["add", "."]);
    git(
        &home,
        &repository,
        &[
            "-c",
            "user.name=Native Git Registry Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "Publish fixture registry index",
            "-m",
            "Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>",
        ],
    );
    let commit = git(&home, &repository, &["rev-parse", "HEAD"]);
    let commit = String::from_utf8(commit.stdout).unwrap().trim().to_owned();
    // This isolated preparation transport is not part of the protected request.
    // Git can access only the owned file repository; Cargo downloads one archive
    // from the owned loopback server and authors the genuine cache/lock layout.
    std::fs::write(
        home.join(".gitconfig"),
        format!(
            "[url \"file://{}\"]\n\tinsteadOf = {INDEX}\n\
        [protocol]\n\tallow = never\n[protocol \"file\"]\n\tallow = always\n",
            repository.display()
        ),
    )
    .unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "[registries.\"git-alt\"]\nindex={}\n",
            serde_json::to_string(INDEX).unwrap()
        ),
    )
    .unwrap();
    cargo(&sysroot, &home, &workspace, &["generate-lockfile"]);
    cargo(&sysroot, &home, &workspace, &["fetch", "--locked"]);
    server.finish();
    let prepared = cargo(
        &sysroot,
        &home,
        &workspace,
        &[
            "metadata",
            "--offline",
            "--locked",
            "--format-version",
            "1",
            "--all-features",
        ],
    );
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    let package = prepared["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == NAME)
        .unwrap();
    assert_eq!(package["source"], json!(format!("registry+{INDEX}")));
    let native_manifest = PathBuf::from(package["manifest_path"].as_str().unwrap());
    let manifest_before = std::fs::read(&native_manifest).unwrap();
    let lock_before = std::fs::read(workspace.join("Cargo.lock")).unwrap();
    let registry = home.join("registry").canonicalize().unwrap();
    let native_index = sole_directory(&registry.join("index"));
    let observed_entry = git(
        &home,
        &native_index,
        &["show", &format!("{commit}:{ENTRY}")],
    );
    assert_eq!(observed_entry.stdout, entry);
    let cached_archive =
        sole_directory(&registry.join("cache")).join(format!("{NAME}-{VERSION}.crate"));
    assert_eq!(std::fs::read(&cached_archive).unwrap(), archive);
    for name in ["cargo", "rustc", "rustdoc"] {
        symlink(
            format!("/workspace/toolchain/bin/{name}"),
            runtime.join(name),
        )
        .unwrap();
    }
    let probe = Command::new(env!("CARGO_BIN_EXE_galla-runner"))
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .unwrap();
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let probe: Value = serde_json::from_slice(&probe.stdout).unwrap();
    let mut value = metadata_request(true, false);
    value["identities"]["backend"] = json!({
        "kind":"bubblewrap", "path":probe["backend"]["path"], "digest":probe["backend"]["digest"]
    });
    for (index, source) in [
        (0, &sysroot),
        (1, &runtime),
        (2, &scratch),
        (3, &workspace),
        (4, &registry),
    ] {
        value["grants"][index]["source"] = json!(source);
    }
    value["cache"]["native"]["registries"] = json!([{
        "name":"git-alt", "index_origin":INDEX, "protocol":"git"
    }]);
    validate(&value).unwrap();
    let output = execute_request(&value);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == NAME)
        .unwrap();
    assert_eq!(package["source"], json!(format!("registry+{INDEX}")));
    assert!(
        package["manifest_path"]
            .as_str()
            .unwrap()
            .starts_with("/workspace/scratch/cargo-home/registry/src/")
    );
    assert_eq!(std::fs::read(&cached_archive).unwrap(), archive);
    assert_eq!(std::fs::read(native_manifest).unwrap(), manifest_before);
    assert_eq!(
        std::fs::read(workspace.join("Cargo.lock")).unwrap(),
        lock_before
    );
    assert!(!scratch.join("cargo-home/config.toml").exists());
    assert!(!scratch.join("home/.gitconfig").exists());
    assert!(!scratch.join("target/debug").exists());
    eprintln!("actual native Git index: {}", native_index.display());
    eprintln!(
        "Cargo Git-index metadata: exit=0, source={}, manifest={}",
        package["source"], package["manifest_path"]
    );
}
