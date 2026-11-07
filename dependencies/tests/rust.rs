use kvist_dependencies::{
    Error, ProcessFailure,
    rust::{self, ProvisionOptions},
};
use sav::CancellationToken;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::Duration,
};

const SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";
const SUM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CONFIG: &str = "[source.crates-io]\nreplace-with='vendored-sources'\n[source.vendored-sources]\ndirectory='discarded-host-path'\n";

fn project() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix(".dependencies-test-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    fs::write(dir.path().join("Cargo.lock"), lock("demo", SUM)).unwrap();
    dir
}
fn lock(name: &str, checksum: &str) -> String {
    format!(
        "version=4\n[[package]]\nname='{name}'\nversion='1.0.0'\nsource='{SOURCE}'\nchecksum='{checksum}'\n"
    )
}
fn populate(vendor: &Path) -> kvist_dependencies::Result<String> {
    fs::create_dir_all(vendor.join("demo")).unwrap();
    let text = "[package]\nname='demo'\nversion='1.0.0'\nedition='2024'\n";
    fs::write(vendor.join("demo/Cargo.toml"), text).unwrap();
    fs::create_dir(vendor.join("demo/src")).unwrap();
    fs::write(vendor.join("demo/src/lib.rs"), "pub fn demo() {}\n").unwrap();
    checksum(vendor);
    Ok(CONFIG.into())
}
fn checksum(vendor: &Path) {
    let mut files = serde_json::Map::new();
    for file in ["Cargo.toml", "src/lib.rs"] {
        files.insert(
            file.into(),
            hex::encode(Sha256::digest(
                fs::read(vendor.join("demo").join(file)).unwrap(),
            ))
            .into(),
        );
    }
    fs::write(
        vendor.join("demo/.cargo-checksum.json"),
        serde_json::to_vec(&serde_json::json!({"package":SUM,"files":files})).unwrap(),
    )
    .unwrap();
}
fn provision(dir: &Path) {
    assert!(
        rust::provision_with(dir, None, &CancellationToken::new(), false, populate)
            .unwrap()
            .ready()
    );
}
fn published(dir: &Path) -> Vec<Vec<u8>> {
    [
        "vendored/demo/src/lib.rs",
        "vendor.json",
        "sandbox-cargo/config.toml",
    ]
    .map(|file| fs::read(dir.join(".kvist").join(file)).unwrap())
    .into()
}

#[test]
fn shared_receipt_records_only_version_and_lock_binding() {
    let dir = project();
    provision(dir.path());
    let receipt = kvist_dependencies::vendoring::VendorManifest::load(dir.path())
        .unwrap()
        .unwrap();
    let value = serde_json::to_value(receipt).unwrap();
    let mut keys = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(keys, ["lockfile_digest", "schema_version"]);
}

#[test]
fn shared_reader_blocks_replacement_without_backend_work() {
    let dir = project();
    provision(dir.path());
    let reader = fs::File::open(dir.path().join(".kvist/vendored")).unwrap();
    reader.try_lock_shared().unwrap();
    let previous = published(dir.path());
    let mut changed = lock("demo", SUM);
    changed.push('\n');
    fs::write(dir.path().join("Cargo.lock"), changed).unwrap();
    let invoked = std::cell::Cell::new(false);
    assert!(
        rust::provision_with(
            dir.path(),
            None,
            &CancellationToken::new(),
            false,
            |vendor| {
                invoked.set(true);
                populate(vendor)
            }
        )
        .is_err(),
        "an active reader must prevent replacement"
    );
    assert!(
        !invoked.get(),
        "busy publication must not perform unnecessary acquisition"
    );
    assert_eq!(published(dir.path()), previous);
    drop(reader);
    assert!(
        rust::provision_with(dir.path(), None, &CancellationToken::new(), false, populate)
            .unwrap()
            .ready()
    );
}

#[test]
fn shared_readiness_does_not_hash_source_files_or_native_file_inventory() {
    let dir = project();
    provision(dir.path());
    fs::write(
        dir.path().join(".kvist/vendored/demo/src/lib.rs"),
        "changed source",
    )
    .unwrap();
    fs::write(
        dir.path().join(".kvist/vendored/demo/.cargo-checksum.json"),
        "Cargo checks this when used",
    )
    .unwrap();
    assert!(
        rust::check(dir.path(), &CancellationToken::new())
            .unwrap()
            .ready()
    );
    fs::remove_file(dir.path().join(".kvist/vendored/demo/.cargo-checksum.json")).unwrap();
    assert!(
        rust::check(dir.path(), &CancellationToken::new()).is_err(),
        "required native metadata must remain present"
    );
}

#[test]
fn shared_vendor_policy_rejects_invalid_settings_and_enforces_selected_limits() {
    for settings in [
        "[limits]\nmax_total_bytes=1\n",
        "[limits]\nmax_total_bytes=0\n",
        "[limits]\nunknown=1\n",
        "[limits]\nmax_entries='wrong'\n",
    ] {
        let dir = project();
        fs::create_dir(dir.path().join(".kvist")).unwrap();
        fs::write(dir.path().join(".kvist/vendor.toml"), settings).unwrap();
        assert!(
            rust::provision_with(dir.path(), None, &CancellationToken::new(), false, populate)
                .is_err(),
            "{settings}"
        );
        assert!(
            !dir.path().join(".kvist/vendored").exists(),
            "invalid or exceeded limits must not publish"
        );
    }
}

#[test]
fn configured_vendor_tree_and_control_quotas_accept_exact_boundaries() {
    let sample = project();
    let vendor = sample.path().join("sample");
    populate(&vendor).unwrap();
    let files = [
        "demo/Cargo.toml",
        "demo/src/lib.rs",
        "demo/.cargo-checksum.json",
    ];
    let sizes: Vec<_> = files
        .iter()
        .map(|file| fs::metadata(vendor.join(file)).unwrap().len())
        .collect();
    let paths = [
        "demo",
        "demo/src",
        "demo/Cargo.toml",
        "demo/src/lib.rs",
        "demo/.cargo-checksum.json",
    ];
    for (key, boundary) in [
        ("max_total_bytes", sizes.iter().sum()),
        ("max_file_bytes", *sizes.iter().max().unwrap()),
        ("max_entries", paths.len() as u64),
        (
            "max_path_bytes",
            paths.iter().map(|path| path.len() as u64).sum(),
        ),
        ("max_depth", 2),
        (
            "max_lockfile_bytes",
            fs::metadata(sample.path().join("Cargo.lock"))
                .unwrap()
                .len(),
        ),
        ("max_metadata_bytes", *sizes.iter().max().unwrap()),
        ("max_sources", 2),
    ] {
        for (value, accepted) in [(boundary, true), (boundary - 1, false)] {
            let dir = project();
            fs::create_dir(dir.path().join(".kvist")).unwrap();
            fs::write(
                dir.path().join(".kvist/vendor.toml"),
                format!("[limits]\n{key}={value}\n"),
            )
            .unwrap();
            let result =
                rust::provision_with(dir.path(), None, &CancellationToken::new(), false, populate);
            assert_eq!(result.is_ok(), accepted, "{key}={value}: {result:?}");
            assert_eq!(dir.path().join(".kvist/vendor.json").exists(), accepted);
        }
    }
}

#[test]
fn configured_package_quota_is_applied_to_actual_lock_entries() {
    for bound in [1, 2] {
        let dir = project();
        fs::write(dir.path().join("Cargo.lock"), "version=4\n[[package]]\nname='one'\nversion='1.0.0'\n[[package]]\nname='two'\nversion='1.0.0'\n").unwrap();
        fs::create_dir(dir.path().join(".kvist")).unwrap();
        fs::write(
            dir.path().join(".kvist/vendor.toml"),
            format!("[limits]\nmax_packages={bound}\n"),
        )
        .unwrap();
        let result =
            rust::provision_with(dir.path(), None, &CancellationToken::new(), false, |_| {
                panic!("local graph needs no acquisition")
            });
        assert_eq!(result.is_ok(), bound == 2, "{result:?}");
    }
}

#[test]
fn configured_depth_can_exceed_the_old_default_without_recursive_stack_growth() {
    for depth in [64, 128] {
        let dir = project();
        fs::create_dir(dir.path().join(".kvist")).unwrap();
        fs::write(
            dir.path().join(".kvist/vendor.toml"),
            format!("[limits]\nmax_depth={depth}\n"),
        )
        .unwrap();
        let result = rust::provision_with(
            dir.path(),
            None,
            &CancellationToken::new(),
            false,
            |vendor| {
                let config = populate(vendor)?;
                let mut directory = vendor.join("demo");
                for _ in 0..96 {
                    directory.push("nested");
                    fs::create_dir(&directory).unwrap();
                }
                fs::write(
                    directory.join("extra"),
                    "bounded but not independently hashed",
                )
                .unwrap();
                Ok(config)
            },
        );
        assert_eq!(result.is_ok(), depth == 128, "{result:?}");
    }
}

#[test]
fn lowered_tree_quota_does_not_rescan_or_rebuild_ready_material() {
    let dir = project();
    provision(dir.path());
    let before = published(dir.path());
    fs::write(
        dir.path().join(".kvist/vendor.toml"),
        "[limits]\nmax_total_bytes=1\n",
    )
    .unwrap();
    assert!(
        rust::check(dir.path(), &CancellationToken::new())
            .unwrap()
            .ready()
    );
    rust::provision_with(dir.path(), None, &CancellationToken::new(), false, |_| {
        panic!("must reuse metadata-ready material")
    })
    .unwrap();
    assert_eq!(published(dir.path()), before);
    assert!(
        rust::provision_with(dir.path(), None, &CancellationToken::new(), true, populate).is_err()
    );
    assert_eq!(published(dir.path()), before);
}

#[test]
fn manifest_assertion_rejects_publication_fences() {
    for failure in ["vendor-operation.lock", "vendor-publication"] {
        let dir = project();
        provision(dir.path());
        let manifest = kvist_dependencies::vendoring::VendorManifest::load(dir.path())
            .unwrap()
            .unwrap();
        assert!(manifest.assert_ready(dir.path()).unwrap().ready());
        let cheap_ready = manifest.report(dir.path()).unwrap().ready();
        let before = published(dir.path());
        match failure {
            "vendor-publication" => {
                fs::create_dir(dir.path().join(".kvist").join(failure)).unwrap()
            }
            "vendor-operation.lock" => {
                fs::write(dir.path().join(".kvist").join(failure), "").unwrap()
            }
            _ => fs::write(
                dir.path().join(".kvist/vendored/demo/src/lib.rs"),
                "corrupt",
            )
            .unwrap(),
        }
        assert_eq!(
            manifest.report(dir.path()).unwrap().ready(),
            cheap_ready,
            "cheap presence observation changed"
        );
        assert!(
            manifest.assert_ready(dir.path()).is_err(),
            "{failure} was reported ready"
        );
        if failure != "corrupt-content" {
            assert_eq!(published(dir.path()), before, "readiness must not write");
        }
    }
}

#[test]
fn publishes_full_state_and_reuses_without_backend_or_rewrite() {
    let dir = project();
    provision(dir.path());
    let before = published(dir.path());
    assert!(
        rust::check(dir.path(), &CancellationToken::new())
            .unwrap()
            .ready()
    );
    rust::provision_with(dir.path(), None, &CancellationToken::new(), false, |_| {
        panic!("must reuse")
    })
    .unwrap();
    assert_eq!(before, published(dir.path()));
    fs::write(
        dir.path().join(".kvist/vendored/demo/src/lib.rs"),
        "corrupt",
    )
    .unwrap();
    assert!(
        rust::check(dir.path(), &CancellationToken::new())
            .unwrap()
            .ready()
    );
    rust::provision_with(dir.path(), None, &CancellationToken::new(), true, populate).unwrap();
    assert_eq!(
        fs::read(dir.path().join(".kvist/vendored/demo/src/lib.rs")).unwrap(),
        b"pub fn demo() {}\n"
    );
}

#[test]
fn empty_graph_check_is_write_free_but_explicit_provision_publishes_without_backend() {
    let dir = project();
    fs::write(
        dir.path().join("Cargo.lock"),
        "version=4\n[[package]]\nname='local'\nversion='0.1.0'\n",
    )
    .unwrap();
    assert!(
        rust::check(dir.path(), &CancellationToken::new())
            .unwrap()
            .ready()
    );
    assert!(!dir.path().join(".kvist").exists());
    rust::provision_with(dir.path(), None, &CancellationToken::new(), false, |_| {
        panic!("empty graph")
    })
    .unwrap();
    assert!(dir.path().join(".kvist/vendored").is_dir());
    assert!(dir.path().join(".kvist/vendor.json").is_file());
    assert!(
        dir.path()
            .join(".kvist/sandbox-cargo/config.toml")
            .is_file()
    );
    let before = fs::read(dir.path().join(".kvist/vendor.json")).unwrap();
    rust::provision_with(dir.path(), None, &CancellationToken::new(), false, |_| {
        panic!("empty reuse")
    })
    .unwrap();
    assert_eq!(
        fs::read(dir.path().join(".kvist/vendor.json")).unwrap(),
        before
    );
    fs::write(
        dir.path().join("Cargo.lock"),
        "version=4\n[[package]]\nname='changed-local'\nversion='0.1.0'\n",
    )
    .unwrap();
    assert!(matches!(
        rust::check(dir.path(), &CancellationToken::new()),
        Err(Error::VendoringStale { .. })
    ));
}

#[test]
fn rejects_malformed_lock_utf8_and_traversal() {
    let dir = project();
    for text in [
        b"not toml[".as_slice(),
        b"version=4\n\xff",
        lock("../outside", SUM).as_bytes(),
        lock("demo", "short").as_bytes(),
    ] {
        fs::write(dir.path().join("Cargo.lock"), text).unwrap();
        assert!(rust::check(dir.path(), &CancellationToken::new()).is_err());
    }
    fs::remove_file(dir.path().join("Cargo.lock")).unwrap();
    symlink("Cargo.toml", dir.path().join("Cargo.lock")).unwrap();
    assert!(rust::check(dir.path(), &CancellationToken::new()).is_err());
}

#[test]
fn provisioning_rejects_source_links_hardlinks_and_special_files() {
    for kind in ["symlink", "hardlink", "fifo"] {
        let dir = project();
        let outside = dir.path().join("outside");
        fs::write(&outside, "unchanged").unwrap();
        let result = rust::provision_with(
            dir.path(),
            None,
            &CancellationToken::new(),
            false,
            |vendor| {
                let config = populate(vendor)?;
                let file = vendor.join("demo/src/lib.rs");
                fs::remove_file(&file).unwrap();
                match kind {
                    "symlink" => symlink(&outside, file).unwrap(),
                    "hardlink" => fs::hard_link(&outside, file).unwrap(),
                    _ => nix::unistd::mkfifo(&file, nix::sys::stat::Mode::S_IRUSR).unwrap(),
                }
                Ok(config)
            },
        );
        assert!(result.is_err(), "{kind}");
        assert!(!dir.path().join(".kvist/vendor.json").exists());
        assert_eq!(fs::read(&outside).unwrap(), b"unchanged");
    }
}

#[test]
fn source_config_is_closed_and_structural() {
    let clean = rust::resolver_config(CONFIG, "/rust/vendor").unwrap();
    assert!(clean.contains("/rust/vendor"));
    assert!(!clean.contains("discarded-host-path"));
    for config in [
        "[build]\nrustc='/host/evil'\n",
        "[source.crates-io]\nreplace-with='x'\n[source.x]\ndirectory='x'\nenv='bad'\n",
        "[source.crates-io]\nreplace-with='missing'\n",
        "[source.x]\nreplace-with='x'\n",
        "[source.x]\nregistry='https://secret:password@registry.invalid'\nreplace-with='vendor'\n[source.vendor]\ndirectory='x'\n",
    ] {
        assert!(rust::resolver_config(config, "/rust/vendor").is_err());
    }
    for destination in ["relative", "/rust/../vendor", "/rust/vendor\n", "/"] {
        assert!(rust::resolver_config(CONFIG, destination).is_err());
    }
    let git = "[source.'git+https://example.invalid/repo?branch=main']\ngit='https://example.invalid/repo'\nbranch='main'\nreplace-with='vendored-sources'\n[source.vendored-sources]\ndirectory='old'\n";
    let config = rust::resolver_config(git, "/workspace/vendored").unwrap();
    assert!(config.contains("branch = \"main\""));
    let alternate = "[source.'registry+https://registry.example.invalid/index']\nregistry='https://registry.example.invalid/index'\nreplace-with='vendor'\n[source.vendor]\ndirectory='host'\n";
    assert!(
        rust::resolver_config(alternate, "/rust/vendor")
            .unwrap()
            .contains("registry.example.invalid")
    );
}

#[test]
fn backend_failure_lock_drift_and_reentrant_writers_preserve_previous_bytes() {
    let dir = project();
    provision(dir.path());
    let before = published(dir.path());
    // Explicit force requests acquisition while retaining the published bytes.
    fs::write(
        dir.path().join(".kvist/vendored/demo/src/lib.rs"),
        "old corrupt",
    )
    .unwrap();
    let before_corrupt = published(dir.path());
    let err = rust::provision_with(dir.path(), None, &CancellationToken::new(), true, |_| {
        Err(Error::Cancelled)
    })
    .unwrap_err();
    assert!(matches!(err, Error::Cancelled));
    assert_eq!(before_corrupt, published(dir.path()));
    let err = rust::provision_with(dir.path(), None, &CancellationToken::new(), true, |stage| {
        let nested =
            rust::provision_with(dir.path(), None, &CancellationToken::new(), false, |_| {
                panic!("exclusive")
            });
        assert!(matches!(nested, Err(Error::OperationLocked { .. })));
        let config = populate(stage)?;
        fs::write(dir.path().join("Cargo.lock"), lock("demo", &"b".repeat(64))).unwrap();
        Ok(config)
    })
    .unwrap_err();
    assert!(matches!(err, Error::VendoringStale { .. }));
    assert_eq!(before_corrupt, published(dir.path()));
    assert_ne!(before, before_corrupt);
}

#[test]
fn occupied_lock_and_state_links_are_not_stolen_or_followed() {
    let dir = project();
    fs::create_dir(dir.path().join(".kvist")).unwrap();
    let lock = dir.path().join(".kvist/vendor-operation.lock");
    fs::write(&lock, "stale operator evidence").unwrap();
    assert!(matches!(
        rust::provision_with(dir.path(), None, &CancellationToken::new(), false, populate),
        Err(Error::OperationLocked { .. })
    ));
    assert_eq!(fs::read_to_string(lock).unwrap(), "stale operator evidence");
    let other = project();
    fs::remove_dir_all(dir.path().join(".kvist")).unwrap();
    symlink(other.path(), dir.path().join(".kvist")).unwrap();
    assert!(
        rust::provision_with(dir.path(), None, &CancellationToken::new(), false, populate).is_err()
    );
    assert!(!other.path().join("vendor-operation.lock").exists());
}

#[test]
fn cancellation_during_population_does_not_publish() {
    let dir = project();
    let token = CancellationToken::new();
    let result = rust::provision_with(dir.path(), None, &token, false, |stage| {
        let config = populate(stage)?;
        token.cancel();
        Ok(config)
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(!dir.path().join(".kvist/vendor.json").exists());
    assert!(!dir.path().join(".kvist/vendored").exists());
}

#[test]
fn source_mismatch_never_publishes() {
    let dir = project();
    for config in [
        CONFIG.replace("crates-io", "unknown"),
        CONFIG.replace(
            "[source.crates-io]",
            "[source.crates-io]\ngit='https://wrong-source.example.invalid/repo'",
        ),
    ] {
        let result = rust::provision_with(
            dir.path(),
            None,
            &CancellationToken::new(),
            false,
            |stage| {
                populate(stage)?;
                Ok(config)
            },
        );
        assert!(result.is_err());
        assert!(!dir.path().join(".kvist/vendored").exists());
    }
}

#[test]
fn publication_failure_is_not_ready_and_retains_recovery() {
    let dir = project();
    fs::create_dir_all(dir.path().join(".kvist/sandbox-cargo/config.toml")).unwrap();
    let result = rust::provision_with(dir.path(), None, &CancellationToken::new(), false, populate);
    assert!(result.is_err());
    assert!(rust::check(dir.path(), &CancellationToken::new()).is_err());
}

fn script(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("cargo-test-host");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn host_options(cargo: PathBuf, timeout: u64) -> ProvisionOptions {
    ProvisionOptions {
        force: false,
        cargo,
        offline: true,
        vendored_dir: None,
        timeout: Duration::from_secs(timeout),
    }
}

#[test]
fn empty_successful_cargo_resolver_is_valid_only_for_a_local_graph() {
    for local in [true, false] {
        let dir = project();
        if local {
            fs::write(dir.path().join("Cargo.lock"), "version=4\n").unwrap();
        }
        let host = tempfile::tempdir().unwrap();
        let vendor = dir.path().join("candidate");
        fs::create_dir(&vendor).unwrap();
        let result = rust::populate(
            dir.path(),
            &vendor,
            &host_options(script(host.path(), "exit 0"), 3),
            None,
            &CancellationToken::new(),
        );
        if local {
            let config = result.unwrap();
            assert!(rust::resolver_config(&config, "/rust/vendor").is_ok());
        } else {
            assert!(
                result.is_err(),
                "an empty resolver cannot satisfy external sources"
            );
        }
    }
}

#[test]
fn supervised_failures_are_typed_bounded_and_payload_free() {
    let dir = project();
    let host = project();
    for (body, class) in [
        (
            "echo 'SECRET stderr payload' >&2; exit 17",
            ProcessFailure::Exit,
        ),
        ("sleep 3", ProcessFailure::Timeout),
        ("head -c 1048577 /dev/zero", ProcessFailure::OutputLimit),
    ] {
        let cargo = script(host.path(), body);
        let error = rust::provision(
            dir.path(),
            host_options(cargo, 1),
            &CancellationToken::new(),
        )
        .unwrap_err();
        assert!(
            matches!(error, Error::Process { failure } if failure == class),
            "{error:?}"
        );
        assert!(!format!("{error:?}").contains("SECRET"));
        assert!(!dir.path().join(".kvist/vendor.json").exists());
    }
    let error = rust::provision(
        dir.path(),
        host_options(script(dir.path(), "exit 0"), 1),
        &CancellationToken::new(),
    )
    .unwrap_err();
    assert!(matches!(error, Error::VendoringUnavailable { .. }));
}

#[test]
fn native_cargo_consumes_fixture_offline_locked() {
    let dir = project();
    provision(dir.path());
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn local() { demo::demo(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[workspace]\n[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n[dependencies]\ndemo='=1.0.0'\n").unwrap();
    let mut lock = lock("demo", SUM);
    lock.push_str("\n[[package]]\nname='fixture'\nversion='0.1.0'\ndependencies=['demo']\n");
    fs::write(dir.path().join("Cargo.lock"), lock).unwrap();
    fs::create_dir(dir.path().join(".cargo")).unwrap();
    let config =
        rust::resolver_config(CONFIG, dir.path().join(".kvist/vendored").to_str().unwrap())
            .unwrap();
    fs::write(dir.path().join(".cargo/config.toml"), config).unwrap();
    let output = std::process::Command::new("cargo")
        .args(["check", "--offline", "--locked", "--quiet"])
        .env("CARGO_TARGET_DIR", dir.path().join("target"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn retired_receipt_path_fields_are_refused() {
    let dir = project();
    provision(dir.path());
    let path = dir.path().join(".kvist/vendor.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["vendored_dir"] = "../host-grant".into();
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(rust::check(dir.path(), &CancellationToken::new()).is_err());
}

#[test]
fn git_revision_and_metadata_are_checked_and_source_config_must_bind() {
    let dir = project();
    let git_lock = "version=4\n[[package]]\nname='demo'\nversion='1.0.0'\nsource='git+https://example.invalid/repo?branch=main#aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n";
    fs::write(dir.path().join("Cargo.lock"), git_lock).unwrap();
    let snapshot = dir.path().join("snapshot");
    populate(&snapshot).unwrap();
    let metadata = snapshot.join("demo/.cargo-checksum.json");
    let mut checksum: serde_json::Value =
        serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
    checksum["package"] = serde_json::Value::Null;
    fs::write(&metadata, serde_json::to_vec(&checksum).unwrap()).unwrap();
    assert!(
        rust::check_catalogue(dir.path(), &snapshot, &CancellationToken::new())
            .unwrap()
            .ready()
    );
    fs::remove_file(&metadata).unwrap();
    assert!(rust::check_catalogue(dir.path(), &snapshot, &CancellationToken::new()).is_err());
    fs::write(
        dir.path().join("Cargo.lock"),
        git_lock.replace("#aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", ""),
    )
    .unwrap();
    assert!(rust::check_catalogue(dir.path(), &snapshot, &CancellationToken::new()).is_err());
    fs::write(dir.path().join("Cargo.lock"), git_lock).unwrap();
    let error = rust::provision_with(
        dir.path(),
        None,
        &CancellationToken::new(),
        false,
        |stage| {
            populate(stage)?;
            let path = stage.join("demo/.cargo-checksum.json");
            let mut checksum: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            checksum["package"] = serde_json::Value::Null;
            fs::write(path, serde_json::to_vec(&checksum).unwrap()).unwrap();
            Ok(CONFIG.into())
        },
    )
    .unwrap_err();
    assert!(matches!(error, Error::VendoringUnavailable { .. }));
}

#[test]
fn once_only_populate_uses_explicit_cache_and_literal_locked_offline_arguments() {
    let dir = project();
    let host = project();
    let cache = host.path().join("cache");
    fs::create_dir(&cache).unwrap();
    let cargo = script(
        host.path(),
        "test \"$1\" = vendor && test \"$2\" = --locked && test \"$3\" = --offline && test \"$#\" = 4 && test -d \"$CARGO_HOME\" || exit 18\nprintf \"[source.crates-io]\\nreplace-with='vendored-sources'\\n[source.vendored-sources]\\ndirectory='discarded-host-path'\\n\"",
    );
    let options = host_options(cargo, 3);
    rust::provision_with(
        dir.path(),
        None,
        &CancellationToken::new(),
        false,
        |stage| {
            populate(stage)?;
            rust::populate(
                dir.path(),
                stage,
                &options,
                Some(&cache),
                &CancellationToken::new(),
            )
        },
    )
    .unwrap();
    assert!(
        rust::check(dir.path(), &CancellationToken::new())
            .unwrap()
            .ready()
    );
}

#[test]
fn native_cargo_vendor_uses_existing_offline_cache_and_preserves_lock() {
    let dir = project();
    // All selected material is already in this workspace's locked offline cache.
    let workspace_lock =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.lock")).unwrap();
    let parsed: toml::Value = toml::from_str(&workspace_lock).unwrap();
    let hex = parsed["package"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"].as_str() == Some("hex"))
        .unwrap();
    let lock = format!(
        "version=4\n[[package]]\nname='fixture'\nversion='0.1.0'\ndependencies=['hex']\n[[package]]\nname='hex'\nversion='{}'\nsource='{}'\nchecksum='{}'\n",
        hex["version"].as_str().unwrap(),
        hex["source"].as_str().unwrap(),
        hex["checksum"].as_str().unwrap()
    );
    fs::write(dir.path().join("Cargo.lock"), &lock).unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[workspace]\n[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nhex='=0.4.3'\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn local() { let _ = hex::encode(b\"ready\"); }\n",
    )
    .unwrap();
    let cargo = PathBuf::from(std::env::var_os("CARGO").unwrap_or_else(|| "/usr/bin/cargo".into()));
    rust::provision(
        dir.path(),
        host_options(cargo, 20),
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("Cargo.lock")).unwrap(),
        lock
    );
    assert!(
        rust::check(dir.path(), &CancellationToken::new())
            .unwrap()
            .ready()
    );
}

#[test]
fn concurrent_writer_is_excluded_while_callback_is_active() {
    let dir = project();
    let entered = std::sync::Barrier::new(2);
    let leave = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let writer = scope.spawn(|| {
            rust::provision_with(
                dir.path(),
                None,
                &CancellationToken::new(),
                false,
                |stage| {
                    entered.wait();
                    leave.wait();
                    populate(stage)
                },
            )
        });
        entered.wait();
        let result =
            rust::provision_with(dir.path(), None, &CancellationToken::new(), false, |_| {
                panic!("concurrent backend")
            });
        assert!(matches!(result, Err(Error::OperationLocked { .. })));
        assert!(matches!(
            rust::check(dir.path(), &CancellationToken::new()),
            Err(Error::OperationLocked { .. })
        ));
        leave.wait();
        writer.join().unwrap().unwrap();
    });
    assert!(
        rust::check(dir.path(), &CancellationToken::new())
            .unwrap()
            .ready()
    );
}

#[test]
fn populated_vendor_links_missing_files_and_wrong_sources_never_publish() {
    let dir = project();
    for mutation in 0..4 {
        let result = rust::provision_with(
            dir.path(),
            None,
            &CancellationToken::new(),
            false,
            |stage| {
                populate(stage)?;
                match mutation {
                    0 => fs::remove_file(stage.join("demo/.cargo-checksum.json")).unwrap(),
                    1 => {
                        fs::remove_file(stage.join("demo/src/lib.rs")).unwrap();
                        symlink("/dev/null", stage.join("demo/src/lib.rs")).unwrap();
                    }
                    2 => fs::write(
                        stage.join("demo/Cargo.toml"),
                        "[package]\nname='other'\nversion='1.0.0'\n",
                    )
                    .unwrap(),
                    _ => {
                        fs::write(stage.join("demo/Cargo.toml"), "invalid = [").unwrap();
                    }
                }
                Ok(CONFIG.into())
            },
        );
        assert!(result.is_err());
        assert!(!dir.path().join(".kvist/vendored").exists());
        assert!(!dir.path().join(".kvist/vendor-operation.lock").exists());
    }
}

#[test]
fn stale_lock_never_mutates_and_custom_manifest_paths_are_not_host_grants() {
    let dir = project();
    provision(dir.path());
    let before = published(dir.path());
    fs::write(dir.path().join("Cargo.lock"), lock("other", SUM)).unwrap();
    assert!(matches!(
        rust::check(dir.path(), &CancellationToken::new()),
        Err(Error::VendoringStale { .. })
    ));
    assert_eq!(before, published(dir.path()));
    fs::write(dir.path().join("Cargo.lock"), lock("demo", SUM)).unwrap();
    let host = project();
    let custom = host.path().join("custom-vendor");
    rust::provision_with(
        dir.path(),
        Some(&custom),
        &CancellationToken::new(),
        false,
        populate,
    )
    .unwrap();
    // Full check still uses this project's default view, not the recorded host path.
    assert!(
        rust::check(dir.path(), &CancellationToken::new())
            .unwrap()
            .ready()
    );
    fs::remove_dir_all(dir.path().join(".kvist/vendored")).unwrap();
    assert!(rust::check(dir.path(), &CancellationToken::new()).is_err());
    rust::provision_with(
        dir.path(),
        Some(&custom),
        &CancellationToken::new(),
        false,
        |_| panic!("complete custom view"),
    )
    .unwrap();
}

#[test]
fn pre_cancelled_and_invalid_timeout_provisioning_is_write_free() {
    let dir = project();
    let host = project();
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(
        rust::check(dir.path(), &token),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        rust::provision_with(dir.path(), None, &token, false, |_| panic!("cancelled")),
        Err(Error::Cancelled)
    ));
    let result = rust::provision(
        dir.path(),
        host_options(script(host.path(), "exit 0"), 0),
        &CancellationToken::new(),
    );
    assert!(matches!(result, Err(Error::VendoringUnavailable { .. })));
    assert!(!dir.path().join(".kvist").exists());
}

#[test]
fn shared_policy_controls_real_acquisition_output_and_time_and_can_raise_defaults() {
    for (policy, body, timeout, failure) in [
        (
            "[limits]\nmax_output_bytes=32\n",
            "printf '%080d' 0".to_owned(),
            3,
            Some(ProcessFailure::OutputLimit),
        ),
        (
            "[limits]\ntimeout_secs=1\n",
            "sleep 3".to_owned(),
            3,
            Some(ProcessFailure::Timeout),
        ),
        (
            "[limits]\ntimeout_secs=1200\n",
            format!("printf '%s' \"{CONFIG}\""),
            1200,
            None,
        ),
    ] {
        let dir = project();
        let host = project();
        fs::create_dir(dir.path().join(".kvist")).unwrap();
        fs::write(dir.path().join(".kvist/vendor.toml"), policy).unwrap();
        let result = rust::populate(
            dir.path(),
            &dir.path().join("candidate"),
            &host_options(script(host.path(), &body), timeout),
            None,
            &CancellationToken::new(),
        );
        if let Some(expected) = failure {
            assert!(
                matches!(result, Err(Error::Process { failure }) if failure == expected),
                "{result:?}"
            );
        } else {
            assert!(result.is_ok(), "{result:?}");
        }
        assert!(
            !dir.path().join(".kvist/vendor.json").exists(),
            "populate must not publish"
        );
    }
}

#[test]
fn supervised_process_honors_live_cancellation_and_never_leaks_payload() {
    let dir = project();
    let host = project();
    let cargo = script(host.path(), "echo SECRET >&2; sleep 5");
    let token = CancellationToken::new();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            token.cancel();
        });
        let result = rust::provision(dir.path(), host_options(cargo, 3), &token);
        assert!(matches!(result, Err(Error::Cancelled)));
    });
    assert!(!dir.path().join(".kvist/vendored").exists());
}

#[test]
fn managed_snapshot_binds_controls_but_never_rehashes_original_vendor() {
    let dir = project();
    provision(dir.path());
    let snapshot = dir.path().join("private-snapshot");
    populate(&snapshot).unwrap();
    fs::write(
        dir.path().join(".kvist/vendored/demo/src/lib.rs"),
        "original changed after copying",
    )
    .unwrap();
    let (report, config) =
        rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).unwrap();
    assert_eq!(report.vendored_dir, snapshot.to_string_lossy());
    assert!(config.unwrap().contains("/workspace/vendored"));
    assert!(
        rust::check(dir.path(), &CancellationToken::new())
            .unwrap()
            .ready()
    );
}

#[test]
fn snapshot_refuses_operation_and_recovery_fences_while_catalogue_remains_pure() {
    let dir = project();
    provision(dir.path());
    let snapshot = dir.path().join("private-snapshot");
    populate(&snapshot).unwrap();
    for fence in ["vendor-operation.lock", "vendor-publication"] {
        let path = dir.path().join(".kvist").join(fence);
        fs::write(&path, "explicit inspectable fence").unwrap();
        assert!(rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).is_err());
        assert!(
            rust::check_catalogue(dir.path(), &snapshot, &CancellationToken::new())
                .unwrap()
                .ready()
        );
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn snapshot_refuses_stale_unverified_malformed_or_partial_managed_state() {
    let dir = project();
    provision(dir.path());
    let snapshot = dir.path().join("private-snapshot");
    populate(&snapshot).unwrap();
    let path = dir.path().join(".kvist/vendor.json");
    let original = fs::read(&path).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
    for (key, value) in [
        ("verified", serde_json::Value::Bool(false)),
        ("schema_version", serde_json::json!(999)),
        (
            "lockfile_digest",
            serde_json::json!(format!("sha256:{}", "b".repeat(64))),
        ),
        ("lockfile_path", serde_json::json!("../Cargo.lock")),
    ] {
        let mut mutated = manifest.clone();
        mutated[key] = value;
        fs::write(&path, serde_json::to_vec(&mutated).unwrap()).unwrap();
        assert!(rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).is_err());
    }
    fs::write(&path, "malformed").unwrap();
    assert!(rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).is_err());
    fs::remove_file(&path).unwrap();
    assert!(rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).is_err());
    fs::write(&path, original).unwrap();
    fs::remove_file(dir.path().join(".kvist/sandbox-cargo/config.toml")).unwrap();
    assert!(rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).is_err());
    fs::remove_file(dir.path().join("Cargo.lock")).unwrap();
    assert!(rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).is_err());
}

#[test]
fn snapshot_gates_resolver_keys_and_locked_source_binding() {
    let dir = project();
    provision(dir.path());
    let snapshot = dir.path().join("private-snapshot");
    populate(&snapshot).unwrap();
    let path = dir.path().join(".kvist/sandbox-cargo/config.toml");
    for config in [
        "[build]\nrustc='/host/evil'\n".to_owned(),
        CONFIG.replace("crates-io", "unknown"),
        CONFIG.replace(
            "[source.crates-io]",
            "[source.crates-io]\ngit='https://wrong.example.invalid/repo'",
        ),
        CONFIG.replace("discarded-host-path", "/host/arbitrary"),
    ] {
        fs::write(&path, config).unwrap();
        assert!(rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).is_err());
    }
    fs::remove_file(&path).unwrap();
    symlink("/dev/null", &path).unwrap();
    assert!(rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).is_err());
}

#[test]
fn snapshot_allows_unmanaged_crates_io_but_not_unbound_alternate_or_git_sources() {
    let dir = project();
    populate(&dir.path().join(".kvist/vendored")).unwrap();
    let snapshot = dir.path().join("private-snapshot");
    populate(&snapshot).unwrap();
    let (report, config) =
        rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).unwrap();
    assert!(report.ready());
    assert!(config.is_none());
    assert!(!dir.path().join(".kvist/vendor.json").exists());
    assert!(!dir.path().join(".kvist/sandbox-cargo").exists());
    fs::write(
        dir.path().join("Cargo.lock"),
        lock("demo", SUM).replace(SOURCE, "registry+https://alternate.example.invalid/index"),
    )
    .unwrap();
    assert!(rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).is_err());
    let git = "version=4\n[[package]]\nname='demo'\nversion='1.0.0'\nsource='git+https://example.invalid/repo#aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n";
    fs::write(dir.path().join("Cargo.lock"), git).unwrap();
    let metadata = snapshot.join("demo/.cargo-checksum.json");
    let mut checksum: serde_json::Value =
        serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
    checksum["package"] = serde_json::Value::Null;
    fs::write(metadata, serde_json::to_vec(&checksum).unwrap()).unwrap();
    assert!(rust::check_view(dir.path(), &snapshot, &CancellationToken::new()).is_err());
}
