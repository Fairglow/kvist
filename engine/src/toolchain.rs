//! Pinned Rust toolchain selection, host provisioning, and enforcement
//! (ADR-0012).
//!
//! The toolchain an offline Cargo build uses is a pinned, host-provisioned
//! artifact: the project pin (`rust-toolchain.toml` or `rust-toolchain`) is
//! the version-controlled catalogue, the host step [`ensure_toolchain`]
//! provisions and records it under `.kvist/`, and verification
//! ([`resolve_pinned_toolchain`]) consumes it, failing closed on absence or
//! drift. Builds never install, upgrade, or modify a toolchain.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde::{Deserialize, Serialize};

use crate::sandbox::ResolvedCargoToolchain;
use crate::{KvistError, Result};

/// Toolchain manifest schema version.
pub const TOOLCHAIN_SCHEMA_VERSION: u32 = 1;
/// Manifest file name under `<project>/.kvist/`.
pub const TOOLCHAIN_MANIFEST_FILENAME: &str = "rust-toolchain.json";
/// The rustup-native TOML pin file.
pub const PIN_TOML_NAME: &str = "rust-toolchain.toml";
/// The rustup-native plain-channel pin file.
pub const PIN_FILE_NAME: &str = "rust-toolchain";
/// Bound for a pin file.
const MAX_PIN_BYTES: u64 = 4096;
/// Bound for captured rustup diagnostics on failure.
const MAX_RUSTUP_ERROR_BYTES: usize = 4096;

/// The effective toolchain selection for a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainPin {
    /// The rustup channel, or `"default"` when no pin file exists.
    pub channel: String,
    /// Whether the channel came from a version-controlled pin file.
    pub pinned: bool,
}

/// A recorded, provisioned toolchain (the manifest content).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolchainManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// The pinned channel (`"default"` when no pin file exists).
    pub channel: String,
    /// Canonical toolchain root.
    pub toolchain_root: String,
    /// Canonical cargo executable beneath the root.
    pub cargo_path: String,
    /// SHA-256 digest of the cargo executable (the request toolchain identity).
    pub cargo_digest: String,
    /// Unix seconds when the manifest was last recorded.
    pub provisioned_at_unix_secs: u64,
}

fn toolchain_error(path: &str, reason: String) -> KvistError {
    KvistError::ToolchainUnavailable {
        path: path.to_owned(),
        reason,
    }
}

/// Validate a rustup channel to the supported forms: exact versions
/// (`1.95`, `1.95.0`), `stable`, `beta`, `nightly`, and their dated variants
/// (`nightly-2026-01-01`, `stable-2026-01-01`, `beta-2026-01-01`).
///
/// `source` names the offending pin for actionable diagnostics.
pub fn validate_channel(channel: &str, source: &str) -> Result<()> {
    if channel.is_empty() {
        return Err(toolchain_error(
            source,
            "toolchain channel is empty".to_owned(),
        ));
    }
    let is_exact_version = {
        let parts: Vec<&str> = channel.split('.').collect();
        (parts.len() == 2 || parts.len() == 3)
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    };
    let is_dated = |prefix: &str| {
        let Some(date) = channel.strip_prefix(prefix) else {
            return false;
        };
        let bytes = date.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || !(0..4).all(|i| bytes[i].is_ascii_digit())
            || !(5..7).all(|i| bytes[i].is_ascii_digit())
            || !(8..10).all(|i| bytes[i].is_ascii_digit())
        {
            return false;
        }
        let month = (bytes[5] - b'0') * 10 + (bytes[6] - b'0');
        let day = (bytes[8] - b'0') * 10 + (bytes[9] - b'0');
        (1..=12).contains(&month) && (1..=31).contains(&day)
    };
    if is_exact_version
        || channel == "stable"
        || channel == "beta"
        || channel == "nightly"
        || is_dated("stable-")
        || is_dated("beta-")
        || is_dated("nightly-")
    {
        return Ok(());
    }
    Err(toolchain_error(
        source,
        format!(
            "toolchain channel `{channel}` is not a supported rustup channel form; \
             expected an exact version such as `1.95.0`, one of `stable`, `beta`, \
             `nightly`, or a dated variant such as `nightly-2026-01-01`"
        ),
    ))
}

/// Parse the TOML pin file, requiring a `[toolchain] channel` string.
pub fn parse_toolchain_toml(contents: &str, source: &str) -> Result<String> {
    let value: toml::Value = toml::from_str(contents)
        .map_err(|error| toolchain_error(source, format!("pin file is not valid TOML: {error}")))?;
    let channel = value
        .get("toolchain")
        .and_then(|table| table.get("channel"))
        .and_then(|channel| channel.as_str())
        .ok_or_else(|| {
            toolchain_error(
                source,
                "pin file must declare a `[toolchain] channel` string".to_owned(),
            )
        })?
        .trim()
        .to_owned();
    validate_channel(&channel, source)?;
    Ok(channel)
}

/// Parse the plain pin file: a single channel line.
pub fn parse_toolchain_file(contents: &str, source: &str) -> Result<String> {
    let channel = contents.trim();
    if channel.is_empty() {
        return Err(toolchain_error(source, "pin file is empty".to_owned()));
    }
    if channel.contains(['\n', '\r']) {
        return Err(toolchain_error(
            source,
            "pin file must contain a single channel line".to_owned(),
        ));
    }
    validate_channel(channel, source)?;
    Ok(channel.to_owned())
}

/// Read a bounded, regular, non-link pin file, returning `None` when absent.
fn read_pin_file(project_root: &Path, path: &Path) -> Result<Option<String>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect toolchain pin",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(toolchain_error(
            &project_root.to_string_lossy(),
            format!(
                "toolchain pin `{}` must be a regular non-link file",
                path.display()
            ),
        ));
    }
    if !metadata.file_type().is_file() {
        return Err(toolchain_error(
            &project_root.to_string_lossy(),
            format!("toolchain pin `{}` must be a regular file", path.display()),
        ));
    }
    if metadata.len() > MAX_PIN_BYTES {
        return Err(toolchain_error(
            &project_root.to_string_lossy(),
            format!(
                "toolchain pin `{}` exceeds the {MAX_PIN_BYTES}-byte limit",
                path.display()
            ),
        ));
    }
    let contents = fs::read_to_string(path).map_err(|source| KvistError::Io {
        operation: "read toolchain pin",
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Some(contents))
}

/// Read the project's toolchain pin. The TOML pin takes priority over the
/// plain file; without either, the rustup default is the selection.
pub fn read_toolchain_pin(project_root: &Path) -> Result<ToolchainPin> {
    if let Some(contents) = read_pin_file(project_root, &project_root.join(PIN_TOML_NAME))? {
        let channel = parse_toolchain_toml(&contents, PIN_TOML_NAME)?;
        return Ok(ToolchainPin {
            channel,
            pinned: true,
        });
    }
    if let Some(contents) = read_pin_file(project_root, &project_root.join(PIN_FILE_NAME))? {
        let channel = parse_toolchain_file(&contents, PIN_FILE_NAME)?;
        return Ok(ToolchainPin {
            channel,
            pinned: true,
        });
    }
    Ok(ToolchainPin {
        channel: "default".to_owned(),
        pinned: false,
    })
}

/// Run a bounded rustup invocation, failing closed with a non-secret,
/// actionable message. `context` names the project in diagnostics.
fn rustup_run(context: &str, args: &[&str]) -> Result<Output> {
    std::process::Command::new("rustup")
        .args(args)
        .output()
        .map_err(|source| {
            toolchain_error(
                context,
                format!("cannot invoke rustup: {source}; install rustup to provision toolchains"),
            )
        })
}

/// Bounded, trimmed stderr for diagnostics.
fn bounded_stderr(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let trimmed = stderr.trim();
    if trimmed.len() > MAX_RUSTUP_ERROR_BYTES {
        let mut end = MAX_RUSTUP_ERROR_BYTES;
        while !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        trimmed[..end].to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Resolve the exact cargo path for a channel, independent of the process
/// working directory and ambient rustup overrides.
pub fn rustup_which_cargo_for_channel(context: &str, channel: &str) -> Result<PathBuf> {
    let output = rustup_run(context, &["which", "cargo", "--toolchain", channel])?;
    if !output.status.success() {
        return Err(toolchain_error(
            context,
            format!(
                "`rustup which cargo --toolchain {channel}` failed: {}; \
                 run `kvist toolchain ensure` to install the pinned toolchain",
                bounded_stderr(&output)
            ),
        ));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if path.is_empty() {
        return Err(toolchain_error(
            context,
            "`rustup which cargo --toolchain` returned no toolchain path".to_owned(),
        ));
    }
    Ok(PathBuf::from(path))
}

/// The installed toolchain names from `rustup toolchain list`.
fn rustup_toolchain_list(context: &str) -> Result<Vec<String>> {
    let output = rustup_run(context, &["toolchain", "list"])?;
    if !output.status.success() {
        return Err(toolchain_error(
            context,
            format!(
                "`rustup toolchain list` failed: {}",
                bounded_stderr(&output)
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            line.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .filter(|name| !name.is_empty())
        .collect())
}

/// Install a toolchain channel on the host (the supported upgrade/downgrade
/// path). This is the only step that may obtain a toolchain from the network.
fn rustup_toolchain_install(context: &str, channel: &str) -> Result<()> {
    let output = rustup_run(context, &["toolchain", "install", channel])?;
    if !output.status.success() {
        return Err(toolchain_error(
            context,
            format!(
                "`rustup toolchain install {channel}` failed: {}",
                bounded_stderr(&output)
            ),
        ));
    }
    Ok(())
}

/// The manifest path for a project.
pub fn manifest_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".kvist")
        .join(TOOLCHAIN_MANIFEST_FILENAME)
}

/// Load the recorded toolchain manifest, or `Ok(None)` when absent.
pub fn load_manifest(project_root: &Path) -> Result<Option<ToolchainManifest>> {
    let path = manifest_path(project_root);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(KvistError::Io {
                operation: "read toolchain manifest",
                path: path.clone(),
                source,
            });
        }
    };
    let manifest: ToolchainManifest = serde_json::from_slice(&bytes).map_err(|source| {
        toolchain_error(
            &project_root.to_string_lossy(),
            format!("cannot parse toolchain manifest: {source}"),
        )
    })?;
    if manifest.schema_version != TOOLCHAIN_SCHEMA_VERSION {
        return Err(toolchain_error(
            &project_root.to_string_lossy(),
            format!(
                "toolchain manifest schema version {} is unsupported (expected {TOOLCHAIN_SCHEMA_VERSION}); \
                 re-run `kvist toolchain ensure`",
                manifest.schema_version
            ),
        ));
    }
    Ok(Some(manifest))
}

/// Persist the toolchain manifest atomically under `<project>/.kvist/`.
pub fn record_manifest(project_root: &Path, manifest: &ToolchainManifest) -> Result<()> {
    let path = manifest_path(project_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| KvistError::Io {
            operation: "create toolchain manifest directory",
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|source| {
        toolchain_error(
            &project_root.to_string_lossy(),
            format!("cannot serialize toolchain manifest: {source}"),
        )
    })?;
    crate::file_io::replace_file_atomically(&path, &String::from_utf8_lossy(&bytes))
}

/// The host-authorized toolchain provisioning step (ADR-0012): resolve the
/// project's pinned channel, install it via rustup when absent (the supported
/// upgrade/downgrade path), validate the toolchain layout, and record the
/// durable manifest. Runs on the host, outside any sandbox.
pub fn ensure_toolchain(project_root: &Path, context: &str) -> Result<ToolchainManifest> {
    let pin = read_toolchain_pin(project_root)?;
    if !pin.pinned {
        // The rustup default is the selection; it is always resolvable when
        // rustup works, so no install is required.
        let toolchain = crate::sandbox::resolve_cargo_toolchain(context)?;
        let cargo_bytes = fs::read(&toolchain.cargo).map_err(|source| KvistError::Io {
            operation: "read cargo executable for toolchain identity",
            path: toolchain.cargo.clone(),
            source,
        })?;
        let manifest = ToolchainManifest {
            schema_version: TOOLCHAIN_SCHEMA_VERSION,
            channel: "default".to_owned(),
            toolchain_root: toolchain.root.to_string_lossy().into_owned(),
            cargo_path: toolchain.cargo.to_string_lossy().into_owned(),
            cargo_digest: crate::vendoring::lockfile_digest(&cargo_bytes),
            provisioned_at_unix_secs: crate::language_vendoring::now_unix_secs(),
        };
        record_manifest(project_root, &manifest)?;
        return Ok(manifest);
    }

    let installed = rustup_toolchain_list(context)?;
    let channel = pin.channel.clone();
    if !installed
        .iter()
        .any(|name| name == &channel || name.starts_with(&format!("{channel}-")))
    {
        rustup_toolchain_install(context, &channel)?;
    }
    let cargo = rustup_which_cargo_for_channel(context, &channel)?;
    let toolchain = crate::sandbox::cargo_toolchain_from_path(&cargo.to_string_lossy(), context)?;
    let cargo_bytes = fs::read(&toolchain.cargo).map_err(|source| KvistError::Io {
        operation: "read cargo executable for toolchain identity",
        path: toolchain.cargo.clone(),
        source,
    })?;
    let manifest = ToolchainManifest {
        schema_version: TOOLCHAIN_SCHEMA_VERSION,
        channel,
        toolchain_root: toolchain.root.to_string_lossy().into_owned(),
        cargo_path: toolchain.cargo.to_string_lossy().into_owned(),
        cargo_digest: crate::vendoring::lockfile_digest(&cargo_bytes),
        provisioned_at_unix_secs: crate::language_vendoring::now_unix_secs(),
    };
    record_manifest(project_root, &manifest)?;
    Ok(manifest)
}

/// Resolve the project's pinned toolchain for use by a build, re-checking the
/// recorded manifest when present. Fails closed when the pinned toolchain is
/// absent, when a recorded manifest no longer matches the on-disk toolchain
/// (channel, root, or cargo digest drifted), and always independently of the
/// process working directory.
pub fn resolve_pinned_toolchain(
    project_root: &Path,
    context: &str,
) -> Result<ResolvedCargoToolchain> {
    let pin = read_toolchain_pin(project_root)?;
    let toolchain = if pin.pinned {
        let cargo = rustup_which_cargo_for_channel(context, &pin.channel)?;
        crate::sandbox::cargo_toolchain_from_path(&cargo.to_string_lossy(), context)?
    } else {
        crate::sandbox::resolve_cargo_toolchain(context)?
    };
    let expected_channel = if pin.pinned {
        pin.channel.clone()
    } else {
        "default".to_owned()
    };
    if let Some(manifest) = load_manifest(project_root)? {
        let cargo_bytes = fs::read(&toolchain.cargo).map_err(|source| KvistError::Io {
            operation: "read cargo executable for toolchain re-check",
            path: toolchain.cargo.clone(),
            source,
        })?;
        let current_digest = crate::vendoring::lockfile_digest(&cargo_bytes);
        let root = toolchain.root.to_string_lossy().into_owned();
        if manifest.channel != expected_channel {
            return Err(toolchain_error(
                &project_root.to_string_lossy(),
                format!(
                    "toolchain channel `{expected_channel}` no longer matches the recorded manifest channel `{}`; \
                     re-run `kvist toolchain ensure`",
                    manifest.channel
                ),
            ));
        }
        if manifest.toolchain_root != root || manifest.cargo_digest != current_digest {
            return Err(toolchain_error(
                &project_root.to_string_lossy(),
                "the on-disk Rust toolchain no longer matches the recorded toolchain manifest; \
                 re-run `kvist toolchain ensure`"
                    .to_owned(),
            ));
        }
    }
    Ok(toolchain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, contents).expect("write file");
    }

    #[test]
    fn validate_channel_accepts_supported_forms() {
        for channel in [
            "1.95",
            "1.95.0",
            "1.0.10",
            "19.99.99",
            "stable",
            "beta",
            "nightly",
            "nightly-2026-01-01",
            "stable-2026-12-31",
            "beta-2025-01-02",
        ] {
            assert!(
                validate_channel(channel, "test").is_ok(),
                "`{channel}` must be accepted"
            );
        }
    }

    #[test]
    fn validate_channel_rejects_invalid_forms() {
        for channel in [
            "",
            "1",
            "1.95.0.1",
            "stable-",
            "nightly-2026-13-01",
            "nightly-2026-00-10",
            "nightly-2026-01-32",
            "nightly-2026-1-1",
            "stable stable",
            "1,95,0",
            "nightly2026-01-01",
            "stable-nightly",
        ] {
            assert!(
                validate_channel(channel, "test").is_err(),
                "`{channel}` must be rejected"
            );
        }
    }

    #[test]
    fn parse_toolchain_toml_extracts_channel() {
        let contents = "[toolchain]\nchannel = \"1.95.0\"\ncomponents = [\"rustc\"]\n";
        assert_eq!(
            parse_toolchain_toml(contents, PIN_TOML_NAME).expect("parses"),
            "1.95.0"
        );
    }

    #[test]
    fn parse_toolchain_toml_rejects_missing_channel() {
        assert!(parse_toolchain_toml("[toolchain]\ncomponents = []\n", PIN_TOML_NAME).is_err());
        assert!(parse_toolchain_toml("channel = \"1.95.0\"\n", PIN_TOML_NAME).is_err());
    }

    #[test]
    fn parse_toolchain_toml_rejects_invalid_channel() {
        let contents = "[toolchain]\nchannel = \"1.95.0.0\"\n";
        assert!(parse_toolchain_toml(contents, PIN_TOML_NAME).is_err());
    }

    #[test]
    fn parse_toolchain_toml_rejects_malformed_toml() {
        assert!(parse_toolchain_toml("[toolchain", PIN_TOML_NAME).is_err());
    }

    #[test]
    fn parse_toolchain_file_accepts_a_single_line() {
        assert_eq!(
            parse_toolchain_file("1.95.0\n", PIN_FILE_NAME).expect("parses"),
            "1.95.0"
        );
        assert_eq!(
            parse_toolchain_file("stable", PIN_FILE_NAME).expect("parses"),
            "stable"
        );
    }

    #[test]
    fn parse_toolchain_file_rejects_multiple_lines() {
        assert!(parse_toolchain_file("stable\n1.95.0\n", PIN_FILE_NAME).is_err());
        assert!(parse_toolchain_file("\n", PIN_FILE_NAME).is_err());
    }

    #[test]
    fn read_toolchain_pin_prefers_the_toml_pin() {
        let project = tempfile::tempdir().expect("project");
        write(
            &project.path().join(PIN_TOML_NAME),
            "[toolchain]\nchannel = \"1.95.0\"\n",
        );
        write(project.path().join(PIN_FILE_NAME).as_path(), "stable\n");
        let pin = read_toolchain_pin(project.path()).expect("reads");
        assert_eq!(
            pin,
            ToolchainPin {
                channel: "1.95.0".to_owned(),
                pinned: true
            }
        );
    }

    #[test]
    fn read_toolchain_pin_falls_back_to_the_plain_pin() {
        let project = tempfile::tempdir().expect("project");
        write(
            project.path().join(PIN_FILE_NAME).as_path(),
            "nightly-2026-01-01\n",
        );
        let pin = read_toolchain_pin(project.path()).expect("reads");
        assert_eq!(
            pin,
            ToolchainPin {
                channel: "nightly-2026-01-01".to_owned(),
                pinned: true
            }
        );
    }

    #[test]
    fn read_toolchain_pin_defaults_without_pins() {
        let project = tempfile::tempdir().expect("project");
        let pin = read_toolchain_pin(project.path()).expect("reads");
        assert_eq!(
            pin,
            ToolchainPin {
                channel: "default".to_owned(),
                pinned: false
            }
        );
    }

    #[test]
    fn read_toolchain_pin_rejects_a_link_like_pin() {
        let project = tempfile::tempdir().expect("project");
        let target = project.path().join("target-channel");
        write(&target, "stable\n");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, project.path().join(PIN_FILE_NAME)).expect("symlink");
        assert!(read_toolchain_pin(project.path()).is_err());
    }

    #[test]
    fn read_toolchain_pin_rejects_an_oversized_pin() {
        let project = tempfile::tempdir().expect("project");
        let oversized = format!(
            "[toolchain]\nchannel = \"{}\"\n",
            "x".repeat(MAX_PIN_BYTES as usize)
        );
        write(project.path().join(PIN_TOML_NAME).as_path(), &oversized);
        assert!(read_toolchain_pin(project.path()).is_err());
    }

    #[test]
    fn manifest_round_trips() {
        let project = tempfile::tempdir().expect("project");
        let manifest = ToolchainManifest {
            schema_version: TOOLCHAIN_SCHEMA_VERSION,
            channel: "1.95.0".to_owned(),
            toolchain_root: "/home/u/.rustup/toolchains/1.95.0-x86_64".to_owned(),
            cargo_path: "/home/u/.rustup/toolchains/1.95.0-x86_64/bin/cargo".to_owned(),
            cargo_digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .to_owned(),
            provisioned_at_unix_secs: 1_700_000_000,
        };
        record_manifest(project.path(), &manifest).expect("records");
        let loaded = load_manifest(project.path())
            .expect("loads")
            .expect("present");
        assert_eq!(loaded, manifest);
    }

    #[test]
    fn manifest_rejects_unknown_fields() {
        let project = tempfile::tempdir().expect("project");
        let path = manifest_path(project.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(
            &path,
            r#"{"schema_version":1,"channel":"stable","toolchain_root":"/r","cargo_path":"/r/bin/cargo","cargo_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000","provisioned_at_unix_secs":1,"extra":true}"#,
        )
        .expect("write");
        assert!(load_manifest(project.path()).is_err());
    }

    #[test]
    fn manifest_rejects_an_unsupported_schema_version() {
        let project = tempfile::tempdir().expect("project");
        let path = manifest_path(project.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(
            &path,
            r#"{"schema_version":2,"channel":"stable","toolchain_root":"/r","cargo_path":"/r/bin/cargo","cargo_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000","provisioned_at_unix_secs":1}"#,
        )
        .expect("write");
        assert!(load_manifest(project.path()).is_err());
    }

    fn rustup_available() -> bool {
        std::process::Command::new("rustup")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn ensure_toolchain_records_the_default_toolchain() {
        if !rustup_available() {
            eprintln!("skip: rustup not on PATH");
            return;
        }
        let project = tempfile::tempdir().expect("project");
        let manifest = ensure_toolchain(project.path(), project.path().to_str().unwrap())
            .expect("ensures the default toolchain");
        assert_eq!(manifest.channel, "default");
        assert!(manifest.cargo_digest.starts_with("sha256:"));
        let loaded = load_manifest(project.path())
            .expect("loads")
            .expect("present");
        assert_eq!(loaded, manifest);
    }

    #[test]
    fn resolve_pinned_toolchain_fails_closed_on_manifest_drift() {
        if !rustup_available() {
            eprintln!("skip: rustup not on PATH");
            return;
        }
        let project = tempfile::tempdir().expect("project");
        write(
            project.path().join(PIN_FILE_NAME).as_path(),
            "this-channel-does-not-exist-9.99.99\n",
        );
        let error = resolve_pinned_toolchain(project.path(), project.path().to_str().unwrap())
            .expect_err("an absent pinned toolchain fails closed");
        assert!(
            matches!(error, KvistError::ToolchainUnavailable { .. }),
            "absent pinned toolchain is a ToolchainUnavailable: {error}"
        );
    }
}
