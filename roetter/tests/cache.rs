use roetter::{
    Error, Result,
    rust::{ProvisionOptions, cache},
};
use sav::CancellationToken;
use std::{
    cell::Cell,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    time::Duration,
};

const SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";
const SUM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NAMESPACE: &str = "index.crates.io-1949cf8c6b5b557f";
mod support;

fn project() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix(".native-cache-test-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    fs::write(dir.path().join("Cargo.lock"), lock(SOURCE, SUM)).unwrap();
    fs::create_dir(dir.path().join("cargo-home")).unwrap();
    dir
}

fn lock(source: &str, checksum: &str) -> String {
    format!(
        "version=4\n[[package]]\nname='demo'\nversion='1.0.0'\nsource='{source}'\nchecksum='{checksum}'\n"
    )
}

fn fill(home: &Path) {
    let index = home.join("registry/index").join(NAMESPACE);
    let source = home.join("registry/src").join(NAMESPACE).join("demo-1.0.0");
    let archives = home.join("registry/cache").join(NAMESPACE);
    fs::create_dir_all(index.join(".cache/de/mo")).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&archives).unwrap();
    fs::write(
        index.join("config.json"),
        r#"{"dl":"https://static.crates.io/crates","api":"https://crates.io"}"#,
    )
    .unwrap();
    let record = format!(r#"{{"name":"demo","vers":"1.0.0","cksum":"{SUM}"}}"#);
    let mut index_bytes = b"\x03\x02\x00\x00\x00etag: \"fixture\"\x001.0.0\x00".to_vec();
    index_bytes.extend(record.as_bytes());
    index_bytes.push(0);
    fs::write(index.join(".cache/de/mo/demo"), index_bytes).unwrap();
    fs::write(archives.join("demo-1.0.0.crate"), b"native archive").unwrap();
    fs::write(
        source.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='1.0.0'\n",
    )
    .unwrap();
    fs::write(source.join(".cargo-ok"), r#"{"v":1}"#).unwrap();
}

fn token() -> CancellationToken {
    CancellationToken::new()
}

#[test]
fn warm_cache_bypasses_callback_and_preserves_legacy_interruption() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fill(&home);
    fs::create_dir_all(dir.path().join(".kvist/vendor-publication")).unwrap();
    fs::write(
        dir.path().join(".kvist/vendor-operation.lock"),
        "dead legacy owner",
    )
    .unwrap();
    fs::write(
        dir.path().join(".kvist/vendor.json"),
        "stale legacy receipt",
    )
    .unwrap();
    fs::write(dir.path().join(".kvist/vendor-publication/stage"), "retain").unwrap();
    let view = cache::provision_with(dir.path(), &home, &token(), false, |_| {
        panic!("complete native cache must bypass acquisition")
    })
    .unwrap();
    assert!(view.report.ready());
    assert_eq!(view.registry, Some(home.join("registry")));
    assert_eq!(
        fs::read_to_string(dir.path().join(".kvist/vendor-operation.lock")).unwrap(),
        "dead legacy owner"
    );
    assert!(dir.path().join(".kvist/vendor-publication/stage").is_file());
    assert!(!dir.path().join(".kvist/vendored").exists());
    assert!(
        cache::provision_with(dir.path(), &home, &token(), true, |_| {
            panic!("force is not redundant acquisition authority")
        })
        .unwrap()
        .report
        .ready()
    );
}

#[test]
fn view_is_read_only_and_reports_missing_material() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let view = cache::view(dir.path(), &home, &token()).unwrap();
    assert!(!view.report.ready());
    assert_eq!(view.report.missing_dependencies, ["demo-1.0.0"]);
    assert_eq!(view.registry, None);
    assert!(!dir.path().join(".kvist").exists());
    assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
}

#[test]
fn absent_home_is_a_read_only_missing_catalogue_and_local_only_is_ready() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fs::remove_dir(&home).unwrap();
    let external = cache::view(dir.path(), &home, &token()).unwrap();
    assert!(!external.report.ready());
    assert_eq!(external.registry, None);
    assert_eq!(external.git, None);
    fs::write(dir.path().join("Cargo.lock"), "version=4\n").unwrap();
    assert!(
        cache::view(dir.path(), &home, &token())
            .unwrap()
            .report
            .ready()
    );
    assert!(!home.exists());
    assert!(!dir.path().join(".kvist").exists());
}

#[test]
fn local_only_graph_inspects_roots_not_unneeded_registry_metadata() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fill(&home);
    fs::write(dir.path().join("Cargo.lock"), "version=4\n").unwrap();
    fs::write(
        home.join("registry/index")
            .join(NAMESPACE)
            .join("config.json"),
        "unrelated broken index metadata",
    )
    .unwrap();
    let view = cache::view(dir.path(), &home, &token()).unwrap();
    assert!(view.report.ready());
    assert_eq!(view.registry, Some(home.join("registry")));
}

#[test]
fn operator_home_selection_rejects_invalid_explicit_values_without_fallback() {
    let dir = project();
    for (cargo, home, expected) in [
        (Some(""), dir.path().to_str().unwrap(), "invalid"),
        (Some("relative"), dir.path().to_str().unwrap(), "invalid"),
        (
            Some("/selected/native-cache"),
            "relative",
            "/selected/native-cache",
        ),
        (None, "relative", "invalid"),
        (
            None,
            dir.path().to_str().unwrap(),
            dir.path().join(".cargo").to_str().unwrap(),
        ),
    ] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "operator_home_selection_probe"])
            .env("HOME", home)
            .env("KVIST_NATIVE_HOME_EXPECTED", expected);
        if let Some(cargo) = cargo {
            command.env("CARGO_HOME", cargo);
        } else {
            command.env_remove("CARGO_HOME");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn operator_home_selection_probe() {
    let Some(expected) = std::env::var_os("KVIST_NATIVE_HOME_EXPECTED") else {
        return;
    };
    if expected == "invalid" {
        assert!(cache::home().is_err());
    } else {
        assert_eq!(cache::home().unwrap(), Path::new(&expected));
    }
}

#[test]
fn missing_sources_call_once_with_existing_native_data_roots() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let calls = Cell::new(0);
    let view = cache::provision_with(dir.path(), &home, &token(), false, |selected| {
        calls.set(calls.get() + 1);
        assert_eq!(selected, home);
        assert!(selected.join("registry").is_dir());
        assert!(selected.join("git").is_dir());
        fill(selected);
        Ok(())
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert!(view.report.ready());
}

#[test]
fn failed_acquisition_keeps_live_material_and_later_reuses_it() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let result = cache::provision_with(dir.path(), &home, &token(), false, |selected| {
        fill(selected);
        Err(Error::Cancelled)
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(
        home.join("registry/src")
            .join(NAMESPACE)
            .join("demo-1.0.0/.cargo-ok")
            .is_file()
    );
    assert!(!dir.path().join(".kvist/cargo-cache.json").exists());
    assert!(
        cache::provision_with(dir.path(), &home, &token(), false, |_| {
            panic!("retained acquired material must be useful")
        })
        .unwrap()
        .report
        .ready()
    );
}

#[test]
fn incomplete_acquisition_is_not_success_or_retried() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let calls = Cell::new(0);
    let result = cache::provision_with(dir.path(), &home, &token(), false, |_| {
        calls.set(calls.get() + 1);
        Ok(())
    });
    assert!(matches!(result, Err(Error::VendoringIncomplete { .. })));
    assert_eq!(calls.get(), 1);
    assert!(!dir.path().join(".kvist/cargo-cache.json").exists());
}

#[test]
fn lock_drift_preserves_acquired_sources_but_never_writes_receipt() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let result = cache::provision_with(dir.path(), &home, &token(), false, |selected| {
        fill(selected);
        let mut changed = lock(SOURCE, SUM);
        changed.push('\n');
        fs::write(dir.path().join("Cargo.lock"), changed).unwrap();
        Ok(())
    });
    assert!(matches!(result, Err(Error::VendoringStale { .. })));
    assert!(home.join("registry/src").exists());
    assert!(!dir.path().join(".kvist/cargo-cache.json").exists());
}

#[test]
fn linked_native_roots_return_canonical_mounts_and_reuse_warm_material() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let shared = dir.path().join("shared");
    fill(&shared);
    fs::create_dir(shared.join("git")).unwrap();
    for name in ["registry", "git"] {
        symlink(shared.join(name), home.join(name)).unwrap();
    }
    let view = cache::provision_with(dir.path(), &home, &token(), false, |_| {
        panic!("warm linked native roots must bypass acquisition")
    })
    .unwrap();
    assert!(view.report.ready());
    assert_eq!(view.registry, Some(shared.join("registry")));
    assert_eq!(view.git, Some(shared.join("git")));
    let sources = cache::sources(dir.path(), &home, &token()).unwrap();
    assert_eq!(sources.registry, view.registry);
    assert_eq!(sources.git, view.git);
}

#[test]
fn unsafe_cache_roots_fail_before_acquisition_with_actual_io_cause() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    symlink(dir.path().join("missing"), home.join("registry")).unwrap();
    let result = cache::provision_with(dir.path(), &home, &token(), false, |_| {
        panic!("unsafe root must refuse callback")
    });
    assert!(matches!(result, Err(Error::Io { .. })));
    assert!(cache::view(dir.path(), Path::new("relative-home"), &token()).is_err());
}

#[test]
fn linked_native_roots_reject_broad_targets_and_deeper_metadata_links() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    for target in [Path::new("/"), &home, dir.path()] {
        symlink(target, home.join("registry")).unwrap();
        assert!(cache::sources(dir.path(), &home, &token()).is_err());
        fs::remove_file(home.join("registry")).unwrap();
    }
    let shared = dir.path().join("shared");
    fill(&shared);
    symlink(shared.join("registry"), home.join("registry")).unwrap();
    let index = shared.join("registry/index");
    fs::rename(&index, shared.join("index")).unwrap();
    symlink(shared.join("index"), &index).unwrap();
    assert!(cache::view(dir.path(), &home, &token()).is_err());
}

#[test]
fn configured_metadata_admission_uses_resolved_native_root() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let shared = dir.path().join("shared");
    fill(&shared);
    fs::create_dir(shared.join("git")).unwrap();
    for name in ["registry", "git"] {
        symlink(shared.join(name), home.join(name)).unwrap();
    }
    let source = "registry+sparse+https://other.example/index/";
    fs::write(dir.path().join("Cargo.lock"), lock(source, SUM)).unwrap();
    let view = cache::admit_with(dir.path(), &home, &token(), |_| {
        Ok(serde_json::to_vec(&serde_json::json!({"packages":[{
            "name":"demo", "version":"1.0.0", "source":source,
            "manifest_path":format!("/workspace/cargo-registry/src/{NAMESPACE}/demo-1.0.0/Cargo.toml")
        }]})).unwrap())
    })
    .unwrap();
    assert!(view.report.ready());
    assert_eq!(view.registry, Some(shared.join("registry")));
}

#[test]
fn receipt_contains_only_native_schema_and_exact_lock_digest() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fill(&home);
    let view = cache::provision_with(dir.path(), &home, &token(), false, |_| panic!()).unwrap();
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join(".kvist/cargo-cache.json")).unwrap())
            .unwrap();
    let mut keys = receipt
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(keys, ["lockfile_digest", "schema_version"]);
    assert_eq!(receipt["lockfile_digest"], view.report.lockfile_digest);
    assert_eq!(receipt["schema_version"], 1);
    let previous = fs::metadata(dir.path().join(".kvist/cargo-cache.json"))
        .unwrap()
        .modified()
        .unwrap();
    cache::provision_with(dir.path(), &home, &token(), false, |_| panic!()).unwrap();
    assert_eq!(
        previous,
        fs::metadata(dir.path().join(".kvist/cargo-cache.json"))
            .unwrap()
            .modified()
            .unwrap()
    );
}

#[test]
fn retained_native_lock_is_not_busy_but_live_owner_is() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fs::create_dir(dir.path().join(".kvist")).unwrap();
    fs::write(dir.path().join(".kvist/cargo-cache-operation.lock"), "").unwrap();
    cache::provision_with(dir.path(), &home, &token(), false, |selected| {
        let nested = cache::provision_with(dir.path(), selected, &token(), false, |_| panic!());
        assert!(matches!(nested, Err(Error::OperationLocked { .. })));
        fill(selected);
        Ok(())
    })
    .unwrap();
    assert!(
        dir.path()
            .join(".kvist/cargo-cache-operation.lock")
            .is_file()
    );
    cache::provision_with(dir.path(), &home, &token(), false, |_| panic!()).unwrap();
}

#[test]
fn isolated_acquisition_lease_coordinates_native_mutation_and_download_locks() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let lease = cache::acquisition_lease(&home, &token()).unwrap();
    for name in [".package-cache-mutate", ".package-cache"] {
        let file = fs::File::open(home.join(name)).unwrap();
        assert!(matches!(file.try_lock(), Err(fs::TryLockError::WouldBlock)));
    }
    assert!(cache::acquisition_lease(&home, &token()).is_err());
    drop(lease);
    cache::acquisition_lease(&home, &token()).unwrap();
}

#[test]
fn canonical_namespace_prefix_does_not_certify_alternate_registry() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fill(&home);
    fs::write(
        dir.path().join("Cargo.lock"),
        lock("registry+sparse+https://other.example/index/", SUM),
    )
    .unwrap();
    assert!(matches!(
        cache::view(dir.path(), &home, &token()),
        Err(Error::UnsupportedCacheSource { .. })
    ));
}

#[test]
fn native_graph_does_not_inherit_export_name_version_collision_restriction() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let mut bytes = lock(SOURCE, SUM);
    bytes.push_str(
        &lock("registry+sparse+https://other.example/index/", SUM).replace("version=4\n", ""),
    );
    fs::write(dir.path().join("Cargo.lock"), bytes).unwrap();
    assert!(matches!(
        cache::view(dir.path(), &home, &token()),
        Err(Error::UnsupportedCacheSource { .. })
    ));
}

#[test]
fn native_index_checksum_and_completion_marker_are_required() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fill(&home);
    let marker = home
        .join("registry/src")
        .join(NAMESPACE)
        .join("demo-1.0.0/.cargo-ok");
    fs::remove_file(&marker).unwrap();
    assert!(
        !cache::view(dir.path(), &home, &token())
            .unwrap()
            .report
            .ready()
    );
    fs::write(marker, r#"{"v":1}"#).unwrap();
    fs::write(dir.path().join("Cargo.lock"), lock(SOURCE, &"b".repeat(64))).unwrap();
    assert!(cache::view(dir.path(), &home, &token()).is_err());
}

#[test]
fn malformed_native_receipt_does_not_authorize_reacquisition_or_false_readiness() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fill(&home);
    fs::create_dir(dir.path().join(".kvist")).unwrap();
    fs::write(
        dir.path().join(".kvist/cargo-cache.json"),
        r#"{"schema_version":1,"lockfile_digest":"invalid","files":[]}"#,
    )
    .unwrap();
    assert!(cache::provision_with(dir.path(), &home, &token(), false, |_| panic!()).is_err());
}

#[test]
fn stale_native_receipt_refreshes_without_redundant_acquisition() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fill(&home);
    let old = cache::provision_with(dir.path(), &home, &token(), false, |_| panic!()).unwrap();
    fs::write(
        dir.path().join("Cargo.lock"),
        format!("{}\n", lock(SOURCE, SUM)),
    )
    .unwrap();
    let new = cache::provision_with(dir.path(), &home, &token(), false, |_| panic!()).unwrap();
    assert_ne!(old.report.lockfile_digest, new.report.lockfile_digest);
    assert!(new.report.ready());
}

#[test]
fn actual_git_cache_is_validated_natively_and_reused_without_acquisition() {
    use std::process::Command;
    let dir = project();
    let repository = tempfile::Builder::new()
        .prefix(".native-git-fixture-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    fs::write(
        repository.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='1.0.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::create_dir(repository.path().join("src")).unwrap();
    fs::write(repository.path().join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "Cargo.toml", "src/lib.rs"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "Offline native fixture",
        ],
    ] {
        assert!(
            Command::new("/usr/bin/git")
                .args(args)
                .current_dir(repository.path())
                .status()
                .unwrap()
                .success()
        );
    }

    let revision = Command::new("/usr/bin/git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repository.path())
        .output()
        .unwrap();
    assert!(revision.status.success());
    let revision = String::from_utf8(revision.stdout).unwrap();
    let url = url::Url::from_file_path(repository.path()).unwrap();
    fs::write(dir.path().join("Cargo.toml"), format!("[workspace]\n[package]\nname='fixture'\nversion='1.0.0'\nedition='2024'\n[dependencies]\ndemo={{git='{url}'}}\n")).unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn value() { demo::demo(); }\n",
    )
    .unwrap();
    fs::write(dir.path().join("Cargo.lock"), format!("version=4\n[[package]]\nname='fixture'\nversion='1.0.0'\ndependencies=['demo']\n[[package]]\nname='demo'\nversion='1.0.0'\nsource='git+{url}#{}'\n", revision.trim())).unwrap();
    let home = dir.path().join("cargo-home");
    let options = ProvisionOptions {
        cargo: support::cargo_path().into(),
        offline: false,
        vendored_dir: None,
        timeout: Duration::from_secs(30),
        force: false,
    };
    let calls = Cell::new(0);
    support::assert_actual_missing_metadata(dir.path(), &home);
    let first = cache::provision_with(dir.path(), &home, &token(), false, |selected| {
        calls.set(calls.get() + 1);
        cache::populate(dir.path(), selected, &options, &token())
    })
    .unwrap();
    assert!(first.report.ready());
    assert_eq!(calls.get(), 1);
    let shared_git = dir.path().join("shared-git");
    fs::rename(home.join("git"), &shared_git).unwrap();
    symlink(&shared_git, home.join("git")).unwrap();
    assert!(matches!(
        cache::view(dir.path(), &home, &token()),
        Err(Error::UnsupportedCacheSource { .. })
    ));
    assert!(
        cache::provision_with(dir.path(), &home, &token(), false, |_| panic!(
            "warm Git cache must reuse native resolution"
        ))
        .unwrap()
        .report
        .ready()
    );
    assert!(home.join("git/checkouts").is_dir());
    assert!(!dir.path().join(".kvist/vendored").exists());
    let source_view = cache::sources(dir.path(), &home, &token()).unwrap();
    assert!(matches!(
        source_view.admission,
        cache::SourceAdmission::CargoOfflineRequired {
            recorded_lock_admission: true
        }
    ));
    assert!(
        cache::admit(dir.path(), &home, &options, &token())
            .unwrap()
            .report
            .ready()
    );
    assert_eq!(source_view.git, Some(shared_git));
    fs::remove_file(home.join("git")).unwrap();
    assert!(matches!(
        cache::sources(dir.path(), &home, &token())
            .unwrap()
            .admission,
        cache::SourceAdmission::CargoOfflineRequired {
            recorded_lock_admission: true
        }
    ));
    assert!(
        !cache::admit(dir.path(), &home, &options, &token())
            .unwrap()
            .report
            .ready()
    );
}

#[test]
fn actual_alternate_registry_uses_cargo_identity_not_namespace_guessing() {
    use sha2::{Digest, Sha256};
    use std::process::Command;
    let dir = project();
    let fixture = tempfile::Builder::new()
        .prefix(".native-registry-fixture-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    let package = fixture.path().join("demo-1.0.0");
    let index = fixture.path().join("index");
    fs::create_dir_all(package.join("src")).unwrap();
    fs::create_dir_all(index.join("de/mo")).unwrap();
    fs::write(
        package.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='1.0.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(package.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
    let archive = fixture.path().join("demo-1.0.0.crate");
    assert!(
        Command::new("/usr/bin/tar")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(fixture.path())
            .arg("demo-1.0.0")
            .status()
            .unwrap()
            .success()
    );
    let checksum = hex::encode(Sha256::digest(fs::read(&archive).unwrap()));
    let downloads = url::Url::from_directory_path(fixture.path()).unwrap();
    fs::write(
        index.join("config.json"),
        format!(r#"{{"dl":"{downloads}{{crate}}-{{version}}.crate"}}"#),
    )
    .unwrap();
    fs::write(index.join("de/mo/demo"), format!(r#"{{"name":"demo","vers":"1.0.0","deps":[],"cksum":"{checksum}","features":{{}},"yanked":false}}"#)).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "config.json", "de/mo/demo"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "Native registry fixture",
        ],
    ] {
        assert!(
            Command::new("/usr/bin/git")
                .args(args)
                .current_dir(&index)
                .status()
                .unwrap()
                .success()
        );
    }
    let index_url = url::Url::from_file_path(&index).unwrap();
    let source = format!("registry+{index_url}");
    fs::create_dir(dir.path().join(".cargo")).unwrap();
    fs::write(
        dir.path().join(".cargo/config.toml"),
        format!("[registries.fixture]\nindex='{index_url}'\n"),
    )
    .unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[workspace]\n[package]\nname='fixture'\nversion='1.0.0'\nedition='2024'\n[dependencies]\ndemo={version='=1.0.0',registry='fixture'}\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn value() { demo::demo(); }\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("Cargo.lock"),
        format!(
            "version=4\n[[package]]\nname='fixture'\nversion='1.0.0'\ndependencies=['demo']\n{}",
            lock(&source, &checksum).replace("version=4\n", "")
        ),
    )
    .unwrap();
    let home = dir.path().join("cargo-home");
    let options = ProvisionOptions {
        cargo: support::cargo_path().into(),
        offline: false,
        vendored_dir: None,
        timeout: Duration::from_secs(30),
        force: false,
    };
    let calls = Cell::new(0);
    support::assert_actual_missing_metadata(dir.path(), &home);
    let first = cache::provision_with(dir.path(), &home, &token(), false, |selected| {
        calls.set(calls.get() + 1);
        cache::populate(dir.path(), selected, &options, &token())
    })
    .unwrap();
    assert!(first.report.ready());
    assert_eq!(calls.get(), 1);
    assert!(matches!(
        cache::view(dir.path(), &home, &token()),
        Err(Error::UnsupportedCacheSource { .. })
    ));
    assert!(
        cache::provision_with(dir.path(), &home, &token(), false, |_| panic!(
            "native alternate cache must be reused"
        ))
        .unwrap()
        .report
        .ready()
    );
    assert!(
        cache::admit(dir.path(), &home, &options, &token())
            .unwrap()
            .report
            .ready()
    );
}

#[test]
fn source_only_runtime_needs_no_lock_and_never_claims_unknown_source_ready() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fs::create_dir(home.join("registry")).unwrap();
    fs::remove_file(dir.path().join("Cargo.lock")).unwrap();
    let no_lock = cache::sources(dir.path(), &home, &token()).unwrap();
    assert_eq!(no_lock.registry, Some(home.join("registry")));
    assert_eq!(no_lock.lockfile_digest, None);
    assert!(matches!(no_lock.admission, cache::SourceAdmission::NoLock));
    fs::write(
        dir.path().join("Cargo.lock"),
        lock("registry+sparse+https://other.example/index/", SUM),
    )
    .unwrap();
    let unknown = cache::sources(dir.path(), &home, &token()).unwrap();
    assert!(unknown.lockfile_digest.is_some());
    assert!(matches!(
        unknown.admission,
        cache::SourceAdmission::CargoOfflineRequired {
            recorded_lock_admission: false
        }
    ));
    assert!(!dir.path().join(".kvist").exists());
}

#[test]
fn canonical_admission_is_process_free_even_without_a_cargo_executable() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    fill(&home);
    let options = ProvisionOptions {
        cargo: "/unavailable/cargo".into(),
        offline: true,
        vendored_dir: None,
        timeout: Duration::from_secs(30),
        force: false,
    };
    assert!(
        cache::admit(dir.path(), &home, &options, &token())
            .unwrap()
            .report
            .ready()
    );
    assert!(
        cache::admit_with(dir.path(), &home, &token(), |_| panic!(
            "canonical warm admission must not run a validator"
        ))
        .unwrap()
        .report
        .ready()
    );
}

fn configured_metadata(home: &Path, source: &str) -> Vec<u8> {
    for root in ["registry/index", "registry/cache"] {
        fs::create_dir_all(home.join(root)).unwrap();
    }
    let manifest = home.join("registry/src/opaque-cargo-namespace/demo-1.0.0/Cargo.toml");
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(manifest, "[package]\nname='demo'\nversion='1.0.0'\n").unwrap();
    serde_json::to_vec(&serde_json::json!({"packages":[{"name":"demo","version":"1.0.0","source":source,
        "manifest_path":"/workspace/cargo-registry/src/opaque-cargo-namespace/demo-1.0.0/Cargo.toml"}]})).unwrap()
}

#[test]
fn configured_native_admission_maps_only_exact_source_and_native_sandbox_endpoints() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let source = "registry+sparse+https://other.example/index/";
    fs::write(dir.path().join("Cargo.lock"), lock(source, SUM)).unwrap();
    let bytes = configured_metadata(&home, source);
    assert!(
        cache::admit_with(dir.path(), &home, &token(), |_| Ok(bytes.clone()))
            .unwrap()
            .report
            .ready()
    );
    let wrong_source = configured_metadata(&home, SOURCE);
    assert!(matches!(
        cache::admit_with(dir.path(), &home, &token(), |_| Ok(wrong_source)),
        Err(Error::UnsupportedCacheSource { .. })
    ));
    let unsafe_path = String::from_utf8(bytes).unwrap().replace(
        "/workspace/cargo-registry/src",
        "/workspace/component/vendor",
    );
    assert!(matches!(
        cache::admit_with(
            dir.path(),
            &home,
            &token(),
            |_| Ok(unsafe_path.into_bytes())
        ),
        Err(Error::UnsupportedCacheSource { .. })
    ));
}

#[test]
fn configured_provision_uses_validator_without_host_fallback_and_acquires_missing_once() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let source = "registry+sparse+https://other.example/index/";
    fs::write(dir.path().join("Cargo.lock"), lock(source, SUM)).unwrap();
    for root in ["registry/index", "registry/cache", "registry/src"] {
        fs::create_dir_all(home.join(root)).unwrap();
    }
    let validations = Cell::new(0);
    let acquisitions = Cell::new(0);
    let ready = cache::provision_with_validation(
        dir.path(),
        &home,
        &token(),
        false,
        |selected| {
            assert!(selected.join("registry").is_dir());
            assert!(selected.join("git").is_dir());
            acquisitions.set(acquisitions.get() + 1);
            configured_metadata(selected, source);
            Ok(())
        },
        |selected| {
            assert!(selected.join("registry").is_dir());
            assert!(selected.join("git").is_dir());
            validations.set(validations.get() + 1);
            if validations.get() == 1 {
                Err(Error::VendoringIncomplete {
                    path: selected.display().to_string(),
                    count: 1,
                    missing: "demo-1.0.0".into(),
                })
            } else {
                Ok(configured_metadata(selected, source))
            }
        },
    )
    .unwrap();
    assert!(ready.report.ready());
    assert_eq!(validations.get(), 2);
    assert_eq!(acquisitions.get(), 1);
    assert!(matches!(
        cache::sources(dir.path(), &home, &token())
            .unwrap()
            .admission,
        cache::SourceAdmission::CargoOfflineRequired {
            recorded_lock_admission: true
        }
    ));
}

#[test]
fn configured_validation_errors_and_lock_drift_do_not_acquire_or_mint_admission() {
    let dir = project();
    let home = dir.path().join("cargo-home");
    let source = "registry+sparse+https://other.example/index/";
    fs::write(dir.path().join("Cargo.lock"), lock(source, SUM)).unwrap();
    for root in ["registry/index", "registry/cache", "registry/src"] {
        fs::create_dir_all(home.join(root)).unwrap();
    }
    let failure = cache::provision_with_validation(
        dir.path(),
        &home,
        &token(),
        false,
        |_| panic!(),
        |_| Err(Error::Cancelled),
    );
    assert!(matches!(failure, Err(Error::Cancelled)));
    assert!(!dir.path().join(".kvist/cargo-cache.json").exists());
    let drift = cache::admit_with(dir.path(), &home, &token(), |selected| {
        fs::write(
            dir.path().join("Cargo.lock"),
            format!("{}\n", lock(source, SUM)),
        )
        .unwrap();
        Ok(configured_metadata(selected, source))
    });
    assert!(matches!(drift, Err(Error::VendoringStale { .. })));
}

#[test]
fn native_metadata_never_accepts_source_paths_outside_selected_cache() {
    let dir = project();
    let executable = tempfile::Builder::new()
        .prefix(".native-cargo-validation-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    let source = "registry+sparse+https://other.example/index/";
    fs::write(dir.path().join("Cargo.lock"), lock(source, SUM)).unwrap();
    for root in ["registry/index", "registry/cache", "registry/src"] {
        fs::create_dir_all(dir.path().join("cargo-home").join(root)).unwrap();
    }
    let cargo = executable.path().join("cargo");
    fs::write(&cargo, format!("#!/bin/sh\nprintf '%s' '{{\"packages\":[{{\"name\":\"demo\",\"version\":\"1.0.0\",\"source\":\"{source}\",\"manifest_path\":\"/unapproved/demo/Cargo.toml\"}}]}}'\n")).unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o700)).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "native_metadata_validation_probe", "--nocapture"])
        .env("KVIST_NATIVE_VALIDATION_PROJECT", dir.path())
        .env("KVIST_NATIVE_VALIDATION_CARGO", &cargo)
        .env("CARGO_HOME", dir.path().join("cargo-home"))
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stdout)
    );
}

#[test]
fn native_metadata_validation_probe() {
    let Some(project) = std::env::var_os("KVIST_NATIVE_VALIDATION_PROJECT") else {
        return;
    };
    let options = ProvisionOptions {
        cargo: std::env::var_os("KVIST_NATIVE_VALIDATION_CARGO")
            .unwrap()
            .into(),
        offline: true,
        vendored_dir: None,
        timeout: Duration::from_secs(10),
        force: false,
    };
    assert!(matches!(
        cache::provision(Path::new(&project), options, &token()),
        Err(Error::UnsupportedCacheSource { .. })
    ));
}

#[test]
fn native_fetch_is_literal_locked_and_reports_safe_actionable_failure() {
    let dir = project();
    let executable = tempfile::Builder::new()
        .prefix(".native-cargo-executable-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    let cargo = executable.path().join("cargo");
    fs::write(&cargo, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CARGO_HOME/argv\"\necho 'failed to download package; token=DO_NOT_RETAIN' >&2\nexit 7\n").unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o700)).unwrap();
    let options = ProvisionOptions {
        cargo,
        offline: true,
        vendored_dir: None,
        timeout: Duration::from_secs(10),
        force: false,
    };
    let error = cache::populate(
        dir.path(),
        &dir.path().join("cargo-home"),
        &options,
        &token(),
    )
    .unwrap_err();
    assert!(
        matches!(&error, Error::NativeCargo { status: Some(status), .. } if status.code() == Some(7))
    );
    let message = error.to_string();
    assert!(!message.contains("DO_NOT_RETAIN"));
    assert!(message.contains("offline") || message.contains("acquisition"));
    assert_eq!(
        fs::read_to_string(dir.path().join("cargo-home/argv")).unwrap(),
        "fetch\n--locked\n--offline\n"
    );
}

#[test]
fn actual_native_cargo_fetch_consumes_host_cache_offline_without_export() -> Result<()> {
    let dir = project();
    let home = cache::home()?;
    let locked = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Cargo.lock"),
    )
    .unwrap();
    let parsed: toml::Value = toml::from_str(&locked).unwrap();
    let itoa = parsed["package"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"].as_str() == Some("itoa"))
        .unwrap();
    let version = itoa["version"].as_str().unwrap();
    let sum = itoa["checksum"].as_str().unwrap();
    fs::write(dir.path().join("Cargo.toml"), format!("[workspace]\n[package]\nname='fixture'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nitoa='={version}'\n")).unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn value() -> String { itoa::Buffer::new().format(1).to_owned() }\n",
    )
    .unwrap();
    fs::write(dir.path().join("Cargo.lock"), format!("version=4\n[[package]]\nname='fixture'\nversion='1.0.0'\ndependencies=['itoa']\n[[package]]\nname='itoa'\nversion='{version}'\nsource='{SOURCE}'\nchecksum='{sum}'\n")).unwrap();
    let options = ProvisionOptions {
        cargo: support::cargo_path().into(),
        offline: true,
        vendored_dir: None,
        timeout: Duration::from_secs(30),
        force: false,
    };
    cache::populate(dir.path(), &home, &options, &token())?;
    assert!(cache::view(dir.path(), &home, &token())?.report.ready());
    assert!(!dir.path().join(".kvist/vendored").exists());
    Ok(())
}
