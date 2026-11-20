//! Security audit: independent confirmation that hostile project pins are
//! rejected fail-closed before any host rustup resolution or installation.
//!
//! These tests exercise the public `resolve_for_workspace` path with Rust
//! forced on. Each hostile pin must produce a diagnostic naming the specific
//! rejection reason (traversal, option injection, unknown field, ambiguity,
//! oversize), proving the fail-closed gate fires on the pin itself rather than
//! only on the absence of a host rustup launcher.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use skott::toolchain::HostProbe;
use skott::{ProfileSetting, ToolPolicy, ToolProfile, ToolRegistry};

/// The manifest directory of the crate under test for the current invocation.
fn manifest_dir() -> PathBuf {
    std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

fn fixture() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(".toolchain-pin-security-")
        .tempdir_in(manifest_dir())
        .unwrap()
}

/// Resolve the workspace with Rust forced on, returning the registry and its
/// diagnostics. A hostile pin must leave Rust unadvertised and name the reason.
fn resolve_with_rust_on(workdir: &std::path::Path) -> ToolRegistry {
    let settings = [(ToolProfile::Rust, ProfileSetting::On)]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    ToolRegistry::resolve_for_workspace(
        ToolPolicy::default(),
        &settings,
        &HostProbe,
        Some(ToolProfile::Rust),
        workdir,
    )
    .expect("a hostile pin must not abort independent tool resolution")
}

fn assert_rejected(registry: &ToolRegistry, needle: &str) {
    assert!(
        !registry.profiles().contains(&"rust"),
        "hostile pin must not advertise Rust"
    );
    let joined = registry.diagnostics().join("\n");
    assert!(
        joined.contains(needle),
        "expected the fail-closed reason `{needle}` in diagnostics, got: {joined}"
    );
}

#[test]
fn channel_path_traversal_is_rejected_fail_closed() {
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"../../outside\"\n",
    )
    .unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert_rejected(&registry, "bounded registration name");
}

#[test]
fn channel_absolute_path_is_rejected_fail_closed() {
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"/outside/toolchain\"\n",
    )
    .unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert_rejected(&registry, "bounded registration name");
}

#[test]
fn channel_option_injection_is_rejected_fail_closed() {
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"--help\"\n",
    )
    .unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert_rejected(&registry, "bounded registration name");
}

#[test]
fn unknown_pin_field_is_rejected_fail_closed() {
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"stable\"\npath = \"/outside\"\n",
    )
    .unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert_rejected(
        &registry,
        "only channel, components, targets and profile are supported",
    );
}

#[test]
fn target_traversal_is_rejected_fail_closed() {
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"stable\"\ntargets = [\"../../outside\"]\n",
    )
    .unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert_rejected(&registry, "invalid/bounded components, targets or profile");
}

#[test]
fn component_traversal_is_rejected_fail_closed() {
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"stable\"\ncomponents = [\"../../outside\"]\n",
    )
    .unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert_rejected(&registry, "invalid/bounded components, targets or profile");
}

#[test]
fn component_option_injection_is_rejected_fail_closed() {
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"stable\"\ncomponents = [\"--install\"]\n",
    )
    .unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert_rejected(&registry, "invalid/bounded components, targets or profile");
}

#[test]
fn both_pin_files_present_are_rejected_fail_closed() {
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"stable\"\n",
    )
    .unwrap();
    fs::write(directory.path().join("rust-toolchain"), "stable\n").unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert_rejected(&registry, "retain one authoritative project pin");
}

#[test]
fn oversized_pin_is_rejected_fail_closed() {
    let directory = fixture();
    fs::write(
        directory.path().join("rust-toolchain.toml"),
        " ".repeat(65537),
    )
    .unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert!(
        !registry.profiles().contains(&"rust"),
        "an oversized pin must not advertise Rust"
    );
    assert!(
        !registry.diagnostics().is_empty(),
        "an oversized pin must produce a diagnostic"
    );
}

#[test]
fn plain_pin_with_newlines_is_rejected_fail_closed() {
    let directory = fixture();
    fs::write(directory.path().join("rust-toolchain"), "stable\nnightly\n").unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert_rejected(&registry, "exactly one channel");
}

#[test]
fn plain_pin_traversal_is_rejected_fail_closed() {
    let directory = fixture();
    fs::write(directory.path().join("rust-toolchain"), "../outside\n").unwrap();
    let registry = resolve_with_rust_on(directory.path());
    assert_rejected(&registry, "bounded registration name");
}
