//! Integration check that language-aware vendoring enforcement works against a
//! real, vendored project root. It skips gracefully when the checkout is not a
//! Rust project root or has not been vendored yet (`.kvist` is gitignored).

use std::path::PathBuf;

use kvist::language_vendoring::enforce_vendoring;
use kvist::vendoring::VendorManifest;

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

#[test]
fn enforces_vendoring_for_the_real_project_when_vendored() {
    let manifest = manifest_dir();
    let project = manifest
        .parent()
        .expect("workspace root is the parent of the maerg crate");
    if !project.join("Cargo.lock").is_file() {
        return;
    }
    if !VendorManifest::load(project)
        .expect("load manifest")
        .is_some()
    {
        eprintln!("skipping: project not yet vendored (run `kvist vendor`)");
        return;
    }

    let enforcement = enforce_vendoring(project, false, None).expect("vendoring enforced");
    assert_eq!(enforcement.language, "rust");
    assert!(
        enforcement.lockfile_digest.starts_with("sha256:"),
        "mount identity is the lock-file digest"
    );
    // Rust mounts the vendored registry and the sandbox cargo config.
    assert!(
        enforcement.mounts.len() >= 2,
        "rust enforces at least the vendored registry and cargo config"
    );
    for mount in &enforcement.mounts {
        assert_eq!(mount.identity, enforcement.lockfile_digest);
        assert!(mount.destination.starts_with('/'));
        assert!(
            mount.source.is_dir(),
            "vendored mount source is an on-disk directory ({:?})",
            mount.source
        );
    }
}
