//! Closed native Cargo data topology and private administrative state.

use galla::validation;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[path = "native_cache/registry_aliases.rs"]
mod registry_aliases;

fn digest(value: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value.as_bytes())))
}

fn grant(destination: &str, purpose: &str, access: &str) -> Value {
    json!({
        "source": format!("/host{}", destination),
        "destination": destination, "purpose": purpose, "access": access,
        "identity": digest(destination)
    })
}

fn endpoint(destination: &str) -> Value {
    json!({"destination": destination, "identity": digest(destination)})
}

fn request(acquisition: bool, registry: bool, git: bool) -> Value {
    let workspace = "/workspace/component";
    let scratch = "/workspace/scratch";
    let toolchain = "/workspace/toolchain";
    let mut grants = vec![
        grant(toolchain, "toolchain", "read-only"),
        grant("/workspace/bin", "runtime", "read-only"),
        grant(scratch, "scratch", "read-write"),
        grant(
            workspace,
            if acquisition {
                "lockfile"
            } else {
                "verification"
            },
            if acquisition {
                "read-write"
            } else {
                "read-only"
            },
        ),
    ];
    let access = if acquisition {
        "read-write"
    } else {
        "read-only"
    };
    let mut native = json!({});
    if registry {
        grants.push(grant("/workspace/cargo-registry", "registry", access));
        native["registry"] = endpoint("/workspace/cargo-registry");
    }
    if git {
        grants.push(grant("/workspace/cargo-git", "dependency-cache", access));
        native["git"] = endpoint("/workspace/cargo-git");
    }
    let mut cache = json!({
        "cargo_home": "/workspace/scratch/cargo-home",
        "registry": "/workspace/cargo-registry", "git": "/workspace/cargo-git",
        "native": native
    });
    let mut environment = json!({
        "HOME": "/workspace/scratch/home", "PATH": "/workspace/bin",
        "CARGO_HOME": "/workspace/scratch/cargo-home",
        "CARGO_TARGET_DIR": "/workspace/scratch/target"
    });
    let mut sources = vec![];
    if acquisition {
        environment["CARGO_NET_GIT_FETCH_WITH_CLI"] = json!("false");
        cache["lockfile"] = json!({"destination": workspace, "before_identity": digest(workspace)});
        sources.push(json!({
            "kind": "cargo-registry", "name": "crates-io",
            "index_origin": "https://index.crates.io/",
            "download_origin": "https://static.crates.io/",
            "identity": validation::registry_identity(
                "crates-io", "https://index.crates.io/", "https://static.crates.io/")
        }));
    } else {
        environment["CARGO_NET_OFFLINE"] = json!("true");
    }
    json!({
        "protocol": "kvist-sandbox-request-v1", "protocol_version": 1,
        "phase": if acquisition { "dependency-acquisition" } else { "verification" },
        "argv": if acquisition {
            vec!["/workspace/toolchain/bin/cargo", "fetch", "--locked"]
        } else {
            vec!["/workspace/toolchain/bin/cargo", "test", "--offline", "--locked"]
        },
        "working_directory": workspace, "environment": environment,
        "network": {
            "mode": if acquisition { "package-sources" } else { "deny" },
            "allowed_sources": sources
        },
        "resources": {
            "wall_time_ms": 30000, "max_output_bytes": 1048576,
            "max_processes": 128, "max_files": 1024,
            "max_scratch_bytes": 67108864
        },
        "identities": {
            "runner": digest("runner"),
            "backend": {"kind": "bubblewrap", "path": "/usr/bin/bwrap", "digest": digest("backend")},
            "policy": digest("policy"), "toolchain": digest(toolchain),
            "command": digest("command"), "mount_plan": digest("mounts")
        },
        "toolchain": {
            "kind": "cargo", "identity": digest(toolchain),
            "root": toolchain, "cargo": "/workspace/toolchain/bin/cargo"
        },
        "cache": cache, "scratch": endpoint(scratch), "grants": grants
    })
}

fn validate(value: &Value) -> Result<galla::protocol::SandboxRequest, validation::ProtocolError> {
    validation::parse_and_validate(&serde_json::to_vec(value).unwrap())
}

fn execute_request(value: &Value) -> std::process::Output {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut runner = Command::new(env!("CARGO_BIN_EXE_galla"))
        .arg("--kvist-sandbox-request-v1")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    runner
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(value).unwrap())
        .unwrap();
    runner.wait_with_output().unwrap()
}

fn metadata_request(registry: bool, git: bool) -> Value {
    let mut value = request(false, registry, git);
    value["argv"] = json!([
        "/workspace/toolchain/bin/cargo",
        "metadata",
        "--offline",
        "--locked",
        "--format-version",
        "1",
        "--all-features"
    ]);
    value
}

#[test]
fn native_metadata_accepts_exact_verification_with_optional_data_roots() {
    for registry in [false, true] {
        for git in [false, true] {
            validate(&metadata_request(registry, git)).unwrap();
        }
    }
}

#[test]
fn native_metadata_rejects_other_commands_flags_authority_and_environment() {
    let original = metadata_request(true, true);
    let mut mutations = vec![];
    for argv in [
        vec!["metadata", "--offline", "--locked", "--format-version", "1"],
        vec![
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--all-features",
        ],
        vec![
            "metadata",
            "--offline",
            "--format-version",
            "1",
            "--all-features",
        ],
        vec![
            "metadata",
            "--offline",
            "--locked",
            "--format-version",
            "2",
            "--all-features",
        ],
        vec![
            "metadata",
            "--offline",
            "--locked",
            "--format-version=1",
            "--all-features",
        ],
        vec![
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--all-features",
        ],
        vec![
            "metadata",
            "--offline",
            "--locked",
            "--format-version",
            "1",
            "--all-features",
            "--no-deps",
        ],
        vec![
            "check",
            "--offline",
            "--locked",
            "--format-version",
            "1",
            "--all-features",
        ],
    ] {
        let mut changed = original.clone();
        changed["argv"] = json!(
            std::iter::once("/workspace/toolchain/bin/cargo")
                .chain(argv)
                .collect::<Vec<_>>()
        );
        mutations.push(changed);
    }
    for index in [0, 1, 3, 4, 5] {
        let mut changed = original.clone();
        changed["grants"][index]["access"] = json!("read-write");
        mutations.push(changed);
    }
    for (name, value) in [
        ("HOME", "/host/home"),
        ("PATH", "/usr/bin"),
        ("CARGO_HOME", "/workspace/cargo-home"),
        ("CARGO_TARGET_DIR", "/workspace/other"),
        ("CARGO_NET_OFFLINE", "false"),
        ("RUSTFLAGS", "-C target-cpu=native"),
    ] {
        let mut changed = original.clone();
        changed["environment"][name] = json!(value);
        mutations.push(changed);
    }
    for phase in ["authoring", "dependency-acquisition"] {
        let mut changed = original.clone();
        changed["phase"] = json!(phase);
        mutations.push(changed);
    }
    let mut changed = original.clone();
    changed["cache"]["approved"] = endpoint("/workspace/cargo-home");
    mutations.push(changed);
    let mut changed = original.clone();
    changed["network"]["mode"] = json!("package-sources");
    mutations.push(changed);
    let mut changed = original.clone();
    changed["cache"]["native"]["registry"]["identity"] = json!(digest("other-root"));
    mutations.push(changed);
    let mut changed = original.clone();
    changed["identities"]["toolchain"] = json!(digest("other-toolchain"));
    mutations.push(changed);
    for changed in mutations {
        assert!(
            validate(&changed).is_err(),
            "accepted metadata mutation: {changed}"
        );
    }
}

#[test]
fn native_cache_accepts_closed_verification_and_acquisition_with_optional_data() {
    for acquisition in [false, true] {
        for registry in [false, true] {
            for git in [false, true] {
                let value = request(acquisition, registry, git);
                validate(&value).unwrap_or_else(|error| panic!("{error}: {value}"));
                let mut nulls = value;
                if !registry {
                    nulls["cache"]["native"]["registry"] = Value::Null;
                }
                if !git {
                    nulls["cache"]["native"]["git"] = Value::Null;
                }
                validate(&nulls).unwrap();
            }
        }
    }
}

#[test]
fn native_cache_rejects_access_identity_path_and_mode_mutations() {
    for acquisition in [false, true] {
        let original = request(acquisition, true, true);
        let mut mutations = vec![];
        for index in [4, 5] {
            let mut value = original.clone();
            value["grants"][index]["access"] = json!(if acquisition {
                "read-only"
            } else {
                "read-write"
            });
            mutations.push(value);
        }
        for field in ["registry", "git"] {
            let mut value = original.clone();
            value["cache"]["native"][field]["identity"] = json!(digest("unapproved"));
            mutations.push(value);
            let mut value = original.clone();
            value["cache"]["native"][field]["destination"] = json!("/workspace/other");
            mutations.push(value);
            let mut value = original.clone();
            value["cache"][field] = json!("/workspace/scratch/cargo-home/registry");
            mutations.push(value);
        }
        for field in ["writable", "approved"] {
            let mut value = original.clone();
            value["cache"][field] = endpoint("/workspace/scratch/cargo-home");
            mutations.push(value);
        }
        let mut value = original.clone();
        value["cache"]["promotion"] =
            json!({"enabled": false, "source": "/workspace/scratch/cargo-home"});
        mutations.push(value);
        let mut value = original.clone();
        value["cache"]["native"]["promotion"] = json!({"enabled": true});
        mutations.push(value);
        let mut value = original.clone();
        value["cache"]["native"]["registry"] = Value::Null;
        mutations.push(value);
        let mut value = original.clone();
        value["phase"] = json!("authoring");
        mutations.push(value);
        for value in mutations {
            assert!(validate(&value).is_err(), "accepted mutation: {value}");
        }
    }
}

#[test]
fn native_cache_rejects_nonexact_home_scratch_runtime_and_cargo_argv() {
    for acquisition in [false, true] {
        let original = request(acquisition, false, false);
        let mut mutations = vec![];
        for (name, value) in [
            ("HOME", "/workspace/scratch/other"),
            ("CARGO_HOME", "/workspace/scratch/other"),
            ("CARGO_TARGET_DIR", "/workspace/scratch/other"),
            ("PATH", "/workspace/toolchain/bin"),
            ("PATH", "/workspace/bin:/usr/bin"),
            ("CARGO_PROFILE_DEV_DEBUG", "0"),
        ] {
            let mut changed = original.clone();
            changed["environment"][name] = json!(value);
            mutations.push(changed);
        }
        let mut value = original.clone();
        value["cache"]["cargo_home"] = json!("/workspace/scratch/other");
        value["environment"]["CARGO_HOME"] = json!("/workspace/scratch/other");
        mutations.push(value);
        let mut value = original.clone();
        value["argv"].as_array_mut().unwrap().pop();
        mutations.push(value);
        if !acquisition {
            let mut value = original.clone();
            value["argv"] = json!(["/workspace/toolchain/bin/cargo", "test", "--locked"]);
            mutations.push(value);
        }
        let mut value = original.clone();
        value["network"]["mode"] = json!(if acquisition {
            "deny"
        } else {
            "package-sources"
        });
        mutations.push(value);
        let mut value = original.clone();
        value["environment"][if acquisition {
            "CARGO_NET_GIT_FETCH_WITH_CLI"
        } else {
            "CARGO_NET_OFFLINE"
        }] = json!(if acquisition { "true" } else { "false" });
        mutations.push(value);
        let mut value = original.clone();
        value["working_directory"] = json!("/workspace/other");
        mutations.push(value);
        if acquisition {
            let mut value = original.clone();
            value["cache"]["lockfile"]["before_identity"] = json!(digest("other-lock"));
            mutations.push(value);
        }
        let mut value = original.clone();
        value["grants"].as_array_mut().unwrap().push(grant(
            "/workspace/extra",
            "toolchain",
            "read-only",
        ));
        mutations.push(value);
        for value in mutations {
            assert!(validate(&value).is_err(), "accepted mutation: {value}");
        }
    }
}

#[test]
fn native_cache_schema_declares_optional_closed_endpoints() {
    let schema: Value = serde_json::from_str(include_str!(
        "../schema/kvist-sandbox-request-v1.schema.json"
    ))
    .unwrap();
    assert!(schema.pointer("/$defs/cache/properties/native").is_some());
    assert_eq!(
        schema["$defs"]["nativeCache"]["additionalProperties"],
        json!(false)
    );
    assert!(
        schema["$defs"]["nativeCache"]
            .get("required")
            .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty))
    );
    for field in ["registry", "git"] {
        assert!(
            schema["$defs"]["nativeCache"]["properties"]
                .get(field)
                .is_some()
        );
    }
}

#[test]
#[ignore = "explicit native trial requires installed Rust and cached crates.io hex 0.4.3"]
fn native_runner_resolves_cached_registry_with_private_admin_and_no_network() {
    use std::os::unix::fs::symlink;
    use std::process::Command;

    let root = tempfile::Builder::new()
        .prefix(".native-cache-fixture-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let cargo_home = std::env::var_os("KVIST_NATIVE_CARGO_HOME")
        .or_else(|| std::env::var_os("CARGO_HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cargo")
        });
    let registry = cargo_home.join("registry").canonicalize().unwrap();
    let archive = registry.join("cache/index.crates.io-1949cf8c6b5b557f/hex-0.4.3.crate");
    let index = registry.join("index/index.crates.io-1949cf8c6b5b557f/.cache/3/h/hex");
    let source = registry.join("src/index.crates.io-1949cf8c6b5b557f/hex-0.4.3/src/lib.rs");
    let observed = [&archive, &index, &source].map(|path| std::fs::read(path).unwrap());
    let checksum = hex::encode(Sha256::digest(&observed[0]));
    let workspace = root.path().join("component");
    let scratch = root.path().join("scratch");
    let runtime = root.path().join("runtime");
    for directory in [&workspace, &scratch, &runtime] {
        std::fs::create_dir(directory).unwrap();
    }
    std::fs::create_dir(workspace.join("src")).unwrap();
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname='native-cache-fixture'\nversion='0.0.0'\nedition='2024'\n\
             [dependencies]\nhex='=0.4.3'\n",
    )
    .unwrap();
    std::fs::write(
            workspace.join("Cargo.lock"),
            format!(
                "version = 4\n[[package]]\nname='native-cache-fixture'\nversion='0.0.0'\n\
                 dependencies=['hex']\n[[package]]\nname='hex'\nversion='0.4.3'\n\
                 source='registry+https://github.com/rust-lang/crates.io-index'\nchecksum='{checksum}'\n"
            ),
        )
        .unwrap();
    std::fs::write(
        workspace.join("src/lib.rs"),
        r#"
    #[test]
    fn readonly_data_private_admin_and_denied_network() {
        assert_eq!(hex::encode([0xab, 0xcd]), "abcd");
        let home = std::path::PathBuf::from(std::env::var_os("CARGO_HOME").unwrap());
        assert_eq!(home, std::path::Path::new("/workspace/scratch/cargo-home"));
        assert_eq!(std::fs::read_link(home.join("registry")).unwrap(),
                   std::path::Path::new("/workspace/cargo-registry"));
        assert!(home.join("git").is_dir());
        assert!(std::fs::write(home.join("git/undeclared-data"), "bad").is_err());
        assert!(std::fs::create_dir(home.join("git/undeclared-root")).is_err());
        std::fs::write(home.join("private-admin"), "private").unwrap();
        assert!(std::fs::write("/workspace/cargo-registry/native-test-write", "bad").is_err());
        let address = "1.1.1.1:443".parse().unwrap();
        assert!(std::net::TcpStream::connect_timeout(
            &address, std::time::Duration::from_millis(100)).is_err());
    }
    "#,
    )
    .unwrap();
    let sysroot_output = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    assert!(sysroot_output.status.success());
    let sysroot =
        std::path::PathBuf::from(String::from_utf8(sysroot_output.stdout).unwrap().trim())
            .canonicalize()
            .unwrap();
    for name in ["cargo", "rustc", "rustdoc"] {
        symlink(
            format!("/workspace/toolchain/bin/{name}"),
            runtime.join(name),
        )
        .unwrap();
    }
    for name in ["cc", "ar", "as", "ld"] {
        symlink(format!("/usr/bin/{name}"), runtime.join(name)).unwrap();
    }
    let probe = Command::new(env!("CARGO_BIN_EXE_galla"))
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
    let mut value = request(false, true, false);
    value["resources"]["wall_time_ms"] = json!(120000);
    value["identities"]["backend"] = json!({
        "kind": "bubblewrap", "path": probe["backend"]["path"],
        "digest": probe["backend"]["digest"]
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
    validate(&value).unwrap();
    let mut wrong_node = value.clone();
    wrong_node["grants"][4]["source"] = json!(archive);
    validate(&wrong_node).unwrap();
    let rejected = execute_request(&wrong_node);
    assert_eq!(rejected.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("native Cargo data"));
    assert!(!scratch.join("cargo-home").exists());
    // Each conflict must fail before Cargo, without replacing caller state.
    for conflict in ["cargo-home", "home", "target"] {
        let path = scratch.join(conflict);
        symlink("/workspace/cargo-registry", &path).unwrap();
        let output = execute_request(&value);
        assert_eq!(output.status.code(), Some(3));
        assert!(String::from_utf8_lossy(&output.stderr).contains("conflicts"));
        assert_eq!(
            std::fs::read_link(&path).unwrap(),
            std::path::Path::new("/workspace/cargo-registry")
        );
        std::fs::remove_file(path).unwrap();
    }
    let output = execute_request(&value);
    for (path, before) in [&archive, &index, &source].into_iter().zip(&observed) {
        assert_eq!(
            &std::fs::read(path).unwrap(),
            before,
            "native cache changed"
        );
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    assert!(
        !scratch.join("cargo-home/private-admin").exists(),
        "admin is namespace-private"
    );
    assert!(!registry.join("native-test-write").exists());

    let metadata_scratch = root.path().join("metadata-scratch");
    std::fs::create_dir(&metadata_scratch).unwrap();
    std::fs::write(
        workspace.join("build.rs"),
        "fn main() { panic!(\"source-only metadata executed a build script\"); }\n",
    )
    .unwrap();
    let mut metadata = metadata_request(true, false);
    metadata["identities"] = value["identities"].clone();
    for (index, source) in [
        (0, &sysroot),
        (1, &runtime),
        (2, &metadata_scratch),
        (3, &workspace),
        (4, &registry),
    ] {
        metadata["grants"][index]["source"] = json!(source);
    }
    validate(&metadata).unwrap();
    let output = execute_request(&metadata);
    for (path, before) in [&archive, &index, &source].into_iter().zip(&observed) {
        assert_eq!(
            &std::fs::read(path).unwrap(),
            before,
            "metadata changed native cache data"
        );
    }
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["version"], json!(1));
    let packages = report["packages"].as_array().unwrap();
    let cached = packages
        .iter()
        .find(|package| package["name"] == "hex")
        .unwrap();
    assert_eq!(
        cached["source"],
        json!("registry+https://github.com/rust-lang/crates.io-index")
    );
    assert!(
        cached["manifest_path"]
            .as_str()
            .unwrap()
            .starts_with("/workspace/scratch/cargo-home/registry/src/"),
        "actual Cargo native manifest path: {}",
        cached["manifest_path"]
    );
    let local = packages
        .iter()
        .find(|package| package["name"] == "native-cache-fixture")
        .unwrap();
    assert!(local["targets"].as_array().unwrap().iter().any(|target| {
        target["kind"]
            .as_array()
            .unwrap()
            .contains(&json!("custom-build"))
    }));
    assert!(
        !metadata_scratch.join("target/debug").exists(),
        "metadata must not compile"
    );
    assert!(!metadata_scratch.join("cargo-home/.global-cache").exists());

    let missing_scratch = root.path().join("missing-scratch");
    std::fs::create_dir(&missing_scratch).unwrap();
    let mut missing = metadata_request(false, false);
    missing["identities"] = value["identities"].clone();
    for (index, source) in [
        (0, &sysroot),
        (1, &runtime),
        (2, &missing_scratch),
        (3, &workspace),
    ] {
        missing["grants"][index]["source"] = json!(source);
    }
    validate(&missing).unwrap();
    let output = execute_request(&missing);
    assert_eq!(
        output.status.code(),
        Some(101),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "failed metadata must not synthesize a report"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no matching package named `hex` found"),
        "{stderr}"
    );
    assert!(stderr.contains("offline mode"), "{stderr}");
}

#[test]
#[ignore = "explicit native trial requires installed Rust and Bubblewrap"]
fn native_runner_fetches_locked_local_graph_without_endpoints_or_promotion() {
    use std::os::unix::fs::symlink;
    use std::process::Command;

    let root = tempfile::Builder::new()
        .prefix(".native-local-graph-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let workspace = root.path().join("component");
    let scratch = root.path().join("scratch");
    let runtime = root.path().join("runtime");
    for directory in [&workspace, &scratch, &runtime] {
        std::fs::create_dir(directory).unwrap();
    }
    std::fs::create_dir(workspace.join("src")).unwrap();
    std::fs::write(workspace.join("src/lib.rs"), "").unwrap();
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname='native-local-graph'\nversion='0.0.0'\nedition='2024'\n",
    )
    .unwrap();
    let lock = "version=4\n[[package]]\nname='native-local-graph'\nversion='0.0.0'\n";
    std::fs::write(workspace.join("Cargo.lock"), lock).unwrap();
    let output = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let sysroot = std::path::PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
        .canonicalize()
        .unwrap();
    symlink("/workspace/toolchain/bin/cargo", runtime.join("cargo")).unwrap();
    symlink("/workspace/toolchain/bin/rustc", runtime.join("rustc")).unwrap();
    let probe = Command::new(env!("CARGO_BIN_EXE_galla"))
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
    let mut value = request(true, false, false);
    value["identities"]["backend"] = json!({
        "kind": "bubblewrap", "path": probe["backend"]["path"],
        "digest": probe["backend"]["digest"]
    });
    for (index, source) in [(0, &sysroot), (1, &runtime), (2, &scratch), (3, &workspace)] {
        value["grants"][index]["source"] = json!(source);
    }
    value["cache"]["lockfile"]["before_identity"] = json!(digest(lock));
    value["grants"][3]["identity"] = json!(digest(lock));
    validate(&value).unwrap();
    let mut changed = value.clone();
    changed["cache"]["lockfile"]["before_identity"] = json!(digest("unobserved-lock"));
    changed["grants"][3]["identity"] = json!(digest("unobserved-lock"));
    validate(&changed).unwrap();
    let rejected = execute_request(&changed);
    assert_eq!(
        rejected.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("before identity"));
    assert!(
        !scratch.join("cargo-home").exists(),
        "bad precondition must not mutate scratch"
    );
    let output = execute_request(&value);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("Cargo.lock")).unwrap(),
        lock
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("promotion"));
    assert!(!scratch.join("cargo-home/.package-cache").exists());
    assert!(!root.path().join("project-cache").exists());
}
