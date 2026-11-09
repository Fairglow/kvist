use super::{execute_request, metadata_request, request, validate};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

#[path = "git_registry.rs"]
mod git_registry;

const INDEX: &str = "https://named-registry.example.invalid/index/";

fn alias() -> Value {
    json!({"name": "alt", "index_origin": INDEX})
}

fn source() -> Value {
    let download = "https://named-registry.example.invalid/crates/";
    json!({
        "kind": "cargo-registry", "name": "alt", "index_origin": INDEX,
        "download_origin": download,
        "identity": galla::validation::registry_identity("alt", INDEX, download)
    })
}

#[test]
fn native_registry_aliases_accept_declared_verification_and_exact_acquisition_sources() {
    let mut verification = metadata_request(true, false);
    verification["cache"]["native"]["registries"] = json!([alias()]);
    validate(&verification).unwrap();
    let mut acquisition = request(true, true, false);
    acquisition["network"]["allowed_sources"]
        .as_array_mut()
        .unwrap()
        .push(source());
    acquisition["cache"]["native"]["registries"] = json!([alias()]);
    validate(&acquisition).unwrap();
    for empty in [Value::Null, json!([])] {
        let mut value = metadata_request(false, false);
        value["cache"]["native"]["registries"] = empty;
        validate(&value).unwrap();
    }
}

#[test]
fn native_registry_aliases_preserve_git_index_origins_and_closed_protocols() {
    let git_index = "https://named-registry.example.invalid/git-index";
    let git_alias = json!({"name": "alt", "index_origin": git_index, "protocol": "git"});
    let mut verification = metadata_request(true, false);
    verification["cache"]["native"]["registries"] = json!([git_alias]);
    validate(&verification).unwrap();
    let mut acquisition = request(true, true, false);
    let mut approved = source();
    approved["index_origin"] = json!(git_index);
    approved["identity"] = json!(galla::validation::registry_identity(
        "alt",
        git_index,
        approved["download_origin"].as_str().unwrap()
    ));
    acquisition["network"]["allowed_sources"]
        .as_array_mut()
        .unwrap()
        .push(approved);
    acquisition["cache"]["native"]["registries"] =
        verification["cache"]["native"]["registries"].clone();
    validate(&acquisition).unwrap();
    for protocol in [Value::Null, json!("https"), json!("Git"), json!("sparse")] {
        let mut value = verification.clone();
        value["cache"]["native"]["registries"][0]["protocol"] = protocol;
        assert!(
            validate(&value).is_err(),
            "accepted invalid protocol/origin: {value}"
        );
    }
    for origin in [
        "http://named-registry.example.invalid/git-index",
        "https://user:secret@named-registry.example.invalid/git-index",
        "https://localhost/git-index",
        "https://named-registry.example.invalid/git-index?token=secret",
        "https://named-registry.example.invalid/git-index#fragment",
    ] {
        let mut value = verification.clone();
        value["cache"]["native"]["registries"][0]["index_origin"] = json!(origin);
        assert!(
            validate(&value).is_err(),
            "accepted unsafe Git origin: {value}"
        );
    }
    acquisition["cache"]["native"]["registries"][0]["index_origin"] =
        json!("https://forged.example.invalid/git-index");
    assert!(validate(&acquisition).is_err());
    verification["cache"]["native"]["registries"][0]["protocol"] = json!("sparse");
    verification["cache"]["native"]["registries"][0]["index_origin"] = json!(INDEX);
    validate(&verification).unwrap();
}

#[test]
fn native_registry_aliases_reject_credentials_injection_duplicates_and_unbound_sources() {
    let mut original = metadata_request(true, false);
    original["cache"]["native"]["registries"] = json!([alias()]);
    let mut mutations = vec![];
    for name in [
        "",
        "crates-io",
        "alt.release",
        "alt]\n[source.evil]",
        "with space",
        "/host/config",
    ] {
        let mut value = original.clone();
        value["cache"]["native"]["registries"][0]["name"] = json!(name);
        mutations.push(value);
    }
    let mut value = original.clone();
    value["cache"]["native"]["registries"][0]["name"] = json!("x".repeat(65));
    mutations.push(value);
    for origin in [
        "https://user:secret@named-registry.example.invalid/index/",
        "https://named-registry.example.invalid/index/?token=secret",
        "https://named-registry.example.invalid/index/#secret",
        "https://named-registry.example.invalid/%2e/index/",
        "https://named-registry.example.invalid/index",
        "sparse+https://named-registry.example.invalid/index/",
        "http://127.0.0.1:9000/index/",
        "https://127.0.0.1/index/",
        "https://10.0.0.1/index/",
        "https://localhost/index/",
    ] {
        let mut value = original.clone();
        value["cache"]["native"]["registries"][0]["index_origin"] = json!(origin);
        mutations.push(value);
    }
    for field in ["token", "source", "replace_with", "config"] {
        let mut value = original.clone();
        value["cache"]["native"]["registries"][0][field] = json!("unapproved");
        mutations.push(value);
    }
    let mut value = original.clone();
    value["cache"]["native"]["registries"] = json!([alias(), alias()]);
    mutations.push(value);
    let mut value = original.clone();
    value["cache"]["native"]["registries"] = json!(
        (0..65)
            .map(|index| json!({"name": format!("alt{index}"), "index_origin": INDEX}))
            .collect::<Vec<_>>()
    );
    mutations.push(value);
    let mut acquisition = request(true, true, false);
    acquisition["cache"]["native"]["registries"] = json!([alias()]);
    mutations.push(acquisition.clone());
    acquisition["network"]["allowed_sources"]
        .as_array_mut()
        .unwrap()
        .push(source());
    let mut value = acquisition.clone();
    value["cache"]["native"]["registries"][0]["index_origin"] =
        json!("https://forged.example.invalid/index/");
    mutations.push(value);
    let mut value = acquisition.clone();
    value["cache"]["native"]["registries"][0]["name"] = json!("forged");
    mutations.push(value);
    acquisition["cache"]["native"]["registries"] = Value::Null;
    mutations.push(acquisition);
    for value in mutations {
        assert!(
            validate(&value).is_err(),
            "accepted unapproved alias: {value}"
        );
    }
}

#[test]
#[ignore = "explicit native trial requires installed Rust, Bubblewrap and cached crates.io hex"]
fn native_named_registry_metadata_uses_only_declared_private_alias_config() {
    use std::os::unix::fs::symlink;

    let root = tempfile::Builder::new()
        .prefix(".native-registry-alias-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap();
    let host_home = std::env::var_os("KVIST_NATIVE_CARGO_HOME")
        .or_else(|| std::env::var_os("CARGO_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cargo"));
    let host_registry = host_home.join("registry");
    let archive =
        std::fs::read(host_registry.join("cache/index.crates.io-1949cf8c6b5b557f/hex-0.4.3.crate"))
            .unwrap();
    let workspace = root.path().join("component");
    let runtime = root.path().join("runtime");
    for directory in [&workspace, &runtime] {
        std::fs::create_dir(directory).unwrap();
    }
    std::fs::create_dir(workspace.join("src")).unwrap();
    std::fs::write(workspace.join("src/lib.rs"), "").unwrap();
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\n[package]\nname='named-native-fixture'\nversion='0.0.0'\nedition='2024'\n\
         [dependencies]\nhex={version='=0.4.3',registry='alt'}\n",
    )
    .unwrap();
    let sysroot_query = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    assert!(sysroot_query.status.success());
    let sysroot = PathBuf::from(String::from_utf8(sysroot_query.stdout).unwrap().trim())
        .canonicalize()
        .unwrap();
    let registry = host_registry.canonicalize().unwrap();
    // Offline fixture preparation lets Cargo author its exact named-source
    // lock identity. Only native data and an owned alias-only admin are used.
    let fixture_home = root.path().join("fixture-cargo-home");
    std::fs::create_dir(&fixture_home).unwrap();
    symlink(&registry, fixture_home.join("registry")).unwrap();
    std::fs::write(
        fixture_home.join("config.toml"),
        "[registries.alt]\nindex='sparse+https://index.crates.io/'\n",
    )
    .unwrap();
    let prepared = Command::new(sysroot.join("bin/cargo"))
        .args([
            "metadata",
            "--offline",
            "--format-version",
            "1",
            "--all-features",
        ])
        .current_dir(&workspace)
        .env_clear()
        .env("HOME", &fixture_home)
        .env("CARGO_HOME", &fixture_home)
        .env("CARGO_TARGET_DIR", fixture_home.join("target"))
        .env("CARGO_CACHE_AUTO_CLEAN_FREQUENCY", "never")
        .env(
            "PATH",
            format!("{}:/usr/bin", sysroot.join("bin").display()),
        )
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    let expected_source = prepared["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "hex")
        .unwrap()["source"]
        .clone();
    for name in ["cargo", "rustc", "rustdoc"] {
        symlink(
            format!("/workspace/toolchain/bin/{name}"),
            runtime.join(name),
        )
        .unwrap();
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
    let mut value = metadata_request(true, false);
    value["identities"]["backend"] = json!({
        "kind": "bubblewrap", "path": probe["backend"]["path"],
        "digest": probe["backend"]["digest"]
    });
    let scratch = root.path().join("without-alias");
    std::fs::create_dir(&scratch).unwrap();
    for (index, source) in [
        (0, &sysroot),
        (1, &runtime),
        (2, &scratch),
        (3, &workspace),
        (4, &registry),
    ] {
        value["grants"][index]["source"] = json!(source);
    }
    let without = execute_request(&value);
    assert_eq!(
        without.status.code(),
        Some(101),
        "{}",
        String::from_utf8_lossy(&without.stderr)
    );
    assert!(
        String::from_utf8_lossy(&without.stderr).contains("registry index was not found"),
        "{}",
        String::from_utf8_lossy(&without.stderr)
    );
    let scratch = root.path().join("declared-alias");
    std::fs::create_dir(&scratch).unwrap();
    value["grants"][2]["source"] = json!(scratch);
    value["cache"]["native"]["registries"] =
        json!([{"name": "alt", "index_origin": "https://index.crates.io/"}]);
    validate(&value).unwrap();
    let with = execute_request(&value);
    assert!(
        with.status.success(),
        "{}",
        String::from_utf8_lossy(&with.stderr)
    );
    let metadata: Value = serde_json::from_slice(&with.stdout).unwrap();
    let package = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == "hex")
        .unwrap();
    assert_eq!(package["source"], expected_source);
    assert!(
        package["manifest_path"]
            .as_str()
            .unwrap()
            .starts_with("/workspace/scratch/cargo-home/registry/src/")
    );
    assert!(
        !scratch.join("cargo-home/config.toml").exists(),
        "config must be namespace-local"
    );
    assert!(!scratch.join("target/debug").exists());
    assert_eq!(
        std::fs::read(registry.join("cache/index.crates.io-1949cf8c6b5b557f/hex-0.4.3.crate"))
            .unwrap(),
        archive
    );
}
