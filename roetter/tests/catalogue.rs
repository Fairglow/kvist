use roetter::{Error, vendoring::*};
use std::{fs, path::Path};

// The extracted maerg fixtures intentionally contain a short checksum. Catalogue
// observation must remain cheap and must not be mistaken for full integrity.
const LOCK_WITH_MISSING: &str = "version = 4\n\n\
[[package]]\nname = \"vendortest\"\nversion = \"0.1.0\"\n\n\
[[package]]\nname = \"serde_json\"\nversion = \"1.0.151\"\n\
source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
checksum = \"c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14\"\n\n\
[[package]]\nname = \"itoa\"\nversion = \"1.0.11\"\n\
source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
checksum = \"41ed3c71d68f1f04ad7790f37911905f10320b73714c9c6f7e6f6b92\"\n\n\
[[package]]\nname = \"local_dep\"\nversion = \"0.1.0\"\n";

fn project() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".dependencies-catalogue-test-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap()
}
fn report_for(lock: &str, present: &[&str]) -> VendoringReport {
    let dir = project();
    fs::write(dir.path().join(CARGO_LOCK_FILENAME), lock).unwrap();
    let vendor = dir.path().join("vendored");
    for entry in present {
        fs::create_dir_all(vendor.join(entry)).unwrap();
        let version = match *entry {
            "serde_json" => Some("1.0.151"),
            "itoa" => Some("1.0.11"),
            _ => None,
        };
        if let Some(version) = version {
            fs::write(
                vendor.join(entry).join("Cargo.toml"),
                format!("[package]\nname = \"{entry}\"\nversion = \"{version}\""),
            )
            .unwrap();
        }
    }
    enforce_offline_readiness(dir.path(), &vendor).unwrap()
}

#[test]
fn classifies_sources_by_prefix() {
    let parsed = parse_cargo_lock(LOCK_WITH_MISSING).unwrap();
    let by_name: std::collections::BTreeMap<_, _> = parsed
        .iter()
        .map(|package| (package.name.as_str(), package.source))
        .collect();
    assert_eq!(by_name["serde_json"], LockSource::Registry);
    assert_eq!(by_name["itoa"], LockSource::Registry);
    assert_eq!(by_name["local_dep"], LockSource::Path);
}

#[test]
fn ready_only_when_all_durable_sources_are_present() {
    let report = report_for(LOCK_WITH_MISSING, &["serde_json", "itoa"]);
    assert!(report.ready());
    assert_eq!(report.registry_dependencies, 2);
    assert_eq!(report.path_dependencies, 2);
    report.enforce().unwrap();
    let incomplete = report_for(LOCK_WITH_MISSING, &["serde_json"]);
    assert!(!incomplete.ready());
    assert_eq!(incomplete.missing_dependencies, vec!["itoa-1.0.11"]);
    assert!(matches!(
        incomplete.enforce(),
        Err(Error::VendoringIncomplete { count: 1, .. })
    ));
}

#[test]
fn accepts_classic_name_version_layout_without_claiming_content_integrity() {
    assert!(report_for(LOCK_WITH_MISSING, &["serde_json-1.0.151", "itoa-1.0.11"]).ready());
}

#[test]
fn digest_is_deterministic_and_prefixed() {
    assert!(lockfile_digest(LOCK_WITH_MISSING.as_bytes()).starts_with("sha256:"));
    assert_eq!(
        lockfile_digest(LOCK_WITH_MISSING.as_bytes()),
        lockfile_digest(LOCK_WITH_MISSING.as_bytes())
    );
    assert_ne!(
        lockfile_digest(LOCK_WITH_MISSING.as_bytes()),
        lockfile_digest(b"version = 4\n\n[[package]]\nname = \"z\"\nversion = \"1\"\n")
    );
}

#[test]
fn offline_cargo_config_maps_crates_io_to_vendored() {
    let config = offline_cargo_config(VENDOR_SANDBOX_MOUNT);
    assert!(config.contains("replace-with = \"vendored-sources\""));
    assert!(config.contains(&format!("directory = \"{VENDOR_SANDBOX_MOUNT}\"")));
}

#[test]
fn manifest_round_trips_and_detects_staleness() {
    let dir = project();
    let root = dir.path();
    let lock = "version=4\n";
    fs::write(root.join(CARGO_LOCK_FILENAME), lock).unwrap();
    let vendor = root.join(".kvist/vendored");
    fs::create_dir_all(&vendor).unwrap();
    fs::create_dir(root.join(".kvist/sandbox-cargo")).unwrap();
    fs::write(
        root.join(".kvist/sandbox-cargo/config.toml"),
        offline_cargo_config(VENDOR_SANDBOX_MOUNT),
    )
    .unwrap();
    let manifest = VendorManifest {
        schema_version: VENDOR_SCHEMA_VERSION,
        lockfile_digest: lockfile_digest(lock.as_bytes()),
    };
    manifest.save(root).unwrap();
    assert_eq!(VendorManifest::load(root).unwrap(), Some(manifest.clone()));
    manifest.assert_ready(root).unwrap();
    let drifted = format!("{lock}\n\n[[package]]\nname = \"new_dep\"\nversion = \"0.1.0\"\n");
    fs::write(root.join(CARGO_LOCK_FILENAME), drifted).unwrap();
    assert!(matches!(
        VendorManifest::load(root)
            .unwrap()
            .unwrap()
            .assert_ready(root),
        Err(Error::VendoringStale { .. })
    ));
}

#[test]
fn stale_package_helpers_match_name_version_but_refuse_traversal() {
    let dir = project();
    fs::write(dir.path().join(CARGO_LOCK_FILENAME), LOCK_WITH_MISSING).unwrap();
    let vendor = dir.path().join("vendor");
    fs::create_dir_all(vendor.join("old-1.0.0")).unwrap();
    let stale = find_stale_vendored_packages(dir.path(), &vendor).unwrap();
    assert_eq!(stale, ["old-1.0.0"]);
    assert!(
        remove_stale_vendored_packages(
            &vendor,
            &["../outside".into()],
            &roetter::VendorLimits::default()
        )
        .is_err()
    );
    assert_eq!(
        remove_stale_vendored_packages(&vendor, &stale, &roetter::VendorLimits::default()).unwrap(),
        1
    );
}

#[test]
fn missing_vendor_is_incomplete_not_a_successful_empty_catalogue() {
    let dir = project();
    fs::write(dir.path().join(CARGO_LOCK_FILENAME), LOCK_WITH_MISSING).unwrap();
    let report = enforce_offline_readiness(dir.path(), &dir.path().join("missing")).unwrap();
    assert!(!report.ready());
    assert!(
        VendorManifest::load(Path::new(dir.path()))
            .unwrap()
            .is_none()
    );
}
