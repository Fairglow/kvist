#![forbid(unsafe_code)]
//! Convenience exec-mode request construction.
//!
//! The exec mode (`galla <command> <args...>`, or the explicit `galla exec
//! <command> <args...>`) runs a command line inside the sandbox with the
//! current working directory mounted read-write at `/workspace` and the
//! standard system tree mounted read-only at `/usr`. It constructs a closed
//! authoring-phase request and enforces it through the same Bubblewrap-backed
//! isolation as the one-shot request mode. No additional host mounts are
//! introduced; the working directory is the only writable grant.
//!
//! The request construction here is host-independent and pure: it takes the
//! already-resolved argv, working directory, and backend/runner identities,
//! and returns a [`SandboxRequest`] that the caller validates and enforces.
//! This keeps the enforcement boundary unchanged and lets the construction be
//! tested without a live sandbox.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::protocol::{
    command_identity, mount_plan_identity, Access, BackendIdentity, BackendKind, Grant, Identities,
    Network, NetworkMode, Phase, Purpose, REQUEST_PROTOCOL, Resources, SandboxRequest, Toolchain,
};

/// The canonical absolute sandbox destination for the working directory.
pub const WORKSPACE_DESTINATION: &str = "/workspace";
/// The canonical absolute sandbox destination for the system tree.
pub const SYSTEM_DESTINATION: &str = "/usr";
/// The sandbox `PATH` exposing the system bin directories.
pub const SYSTEM_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// Bounded execution limits for one exec command. These match the runner's
/// safe maxima and are deliberately generous: the exec mode is a convenience
/// boundary, not a tight task grant.
pub const fn default_resources() -> Resources {
    Resources {
        wall_time_ms: 120_000,
        max_output_bytes: 1 << 20,
        max_processes: 256,
        max_files: 1 << 12,
        max_file_bytes: None,
        max_scratch_bytes: 1 << 30,
        max_cache_bytes: None,
    }
}

/// Resolves the exec command line to an absolute argv.
///
/// A relative `argv[0]` is searched on the host `PATH` (the standard system
/// directories are always consulted), so `galla bash` works as well as
/// `galla /usr/bin/bash`. The resolved program path is canonicalized so the
/// request carries a canonical absolute path, as the protocol requires. The
/// remaining arguments are passed through unchanged.
pub fn resolve_argv(command_line: &[std::ffi::OsString]) -> Result<Vec<String>, String> {
    let Some(program) = command_line.first() else {
        return Err("exec command line is empty".to_owned());
    };
    let program_path = Path::new(program);
    let resolved = if program_path.is_absolute() {
        program_path.canonicalize().map_err(|error| {
            format!(
                "cannot resolve the exec command `{}`: {error}; the command must exist on the host",
                program.to_string_lossy()
            )
        })?
    } else {
        resolve_on_path(program_path)?
    };
    let mut argv = vec![resolved.to_string_lossy().into_owned()];
    for argument in &command_line[1..] {
        argv.push(
            argument.to_str().ok_or_else(|| {
                format!(
                    "exec argument `{}` is not valid UTF-8",
                    argument.to_string_lossy()
                )
            })?
            .to_owned(),
        );
    }
    Ok(argv)
}

/// Resolves a relative command name against the host `PATH`.
///
/// The standard system directories are always consulted, even when `PATH` is
/// unset or cleared, so the exec mode does not depend on an ambient
/// environment. The first regular executable file found is canonicalized and
/// returned.
pub fn resolve_on_path(name: &Path) -> Result<PathBuf, String> {
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .filter(|path| path.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    for directory in ["/usr/local/bin", "/usr/bin", "/bin"] {
        let path = PathBuf::from(directory);
        if !directories.contains(&path) {
            directories.push(path);
        }
    }
    for directory in directories {
        let candidate = directory.join(name);
        let metadata = match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "cannot inspect exec candidate `{}`: {error}",
                    candidate.display()
                ))
            }
        };
        if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
            return candidate.canonicalize().map_err(|error| {
                format!(
                    "cannot canonicalize exec candidate `{}`: {error}",
                    candidate.display()
                )
            });
        }
    }
    Err(format!(
        "exec command `{}` is not an absolute path and was not found on PATH",
        name.to_string_lossy()
    ))
}

/// Builds a closed authoring-phase request for the exec command line.
///
/// The working directory is mounted read-write at [`WORKSPACE_DESTINATION`],
/// and the system tree is mounted read-only at [`SYSTEM_DESTINATION`]. The
/// working directory is the write root; the system grant provides the
/// executables and libraries the command needs. No other host path is
/// mounted. The approval-bound identities are computed from the supplied
/// backend and runner digests and the request's own argv and grants.
pub fn build_request(
    argv: &[String],
    workdir: &Path,
    backend_path: &Path,
    backend_digest: &str,
    runner_digest: &str,
) -> SandboxRequest {
    let toolchain_identity = digest_input(b"toolchain", SYSTEM_DESTINATION.as_bytes());
    let policy_identity = digest_input(b"policy", b"exec");
    let command_identity = command_identity(argv);
    let grants = vec![
        Grant {
            source: SYSTEM_DESTINATION.to_owned(),
            destination: SYSTEM_DESTINATION.to_owned(),
            access: Access::ReadOnly,
            purpose: Purpose::Toolchain,
            identity: digest_input(b"grant", SYSTEM_DESTINATION.as_bytes()),
        },
        Grant {
            source: workdir.to_string_lossy().into_owned(),
            destination: WORKSPACE_DESTINATION.to_owned(),
            access: Access::ReadWrite,
            purpose: Purpose::Authoring,
            identity: digest_input(b"grant", workdir.to_string_lossy().as_bytes()),
        },
    ];
    let mount_plan = mount_plan_identity(&grants);

    let environment = BTreeMap::from([
        ("HOME".to_owned(), WORKSPACE_DESTINATION.to_owned()),
        ("PATH".to_owned(), SYSTEM_PATH.to_owned()),
    ]);

    SandboxRequest {
        protocol: REQUEST_PROTOCOL.to_owned(),
        protocol_version: 1,
        phase: Phase::Authoring,
        argv: argv.to_vec(),
        working_directory: WORKSPACE_DESTINATION.to_owned(),
        environment,
        network: Network {
            mode: NetworkMode::Deny,
            allowed_sources: Vec::new(),
        },
        resources: default_resources(),
        identities: Identities {
            runner: runner_digest.to_owned(),
            backend: BackendIdentity {
                kind: BackendKind::Bubblewrap,
                path: backend_path.to_string_lossy().into_owned(),
                digest: backend_digest.to_owned(),
            },
            policy: policy_identity,
            toolchain: toolchain_identity,
            command: command_identity,
            mount_plan,
        },
        toolchain: Toolchain::System {
            identity: digest_input(b"toolchain", SYSTEM_DESTINATION.as_bytes()),
            root: SYSTEM_DESTINATION.to_owned(),
        },
        grants,
        cache: None,
        scratch: None,
    }
}

/// Computes a `sha256:` digest of a domain-separated input.
fn digest_input(domain: &[u8], input: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(b"\0");
    hasher.update(input);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation;

    fn fixture_digest(bytes: &[u8]) -> String {
        format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
    }

    #[test]
    fn exec_request_validates_with_a_real_workdir() {
        let workdir = tempfile::tempdir().unwrap();
        let argv = vec!["/usr/bin/true".to_owned()];
        let request = build_request(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
        );
        validation::validate(&request)
            .expect("the exec request must validate");
    }

    #[test]
    fn exec_request_validates_with_a_shell_command() {
        let workdir = tempfile::tempdir().unwrap();
        let argv = vec![
            "/usr/bin/bash".to_owned(),
            "-c".to_owned(),
            "echo hello".to_owned(),
        ];
        let request = build_request(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
        );
        validation::validate(&request)
            .expect("a shell exec request must validate");
    }

    #[test]
    fn exec_request_binds_command_and_mount_plan_identities() {
        let workdir = tempfile::tempdir().unwrap();
        let argv = vec!["/usr/bin/true".to_owned()];
        let request = build_request(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
        );
        assert_eq!(
            request.identities.command,
            command_identity(&request.argv),
            "the command identity must bind the actual argv"
        );
        assert_eq!(
            request.identities.mount_plan,
            mount_plan_identity(&request.grants),
            "the mount plan identity must bind the actual grants"
        );
    }

    #[test]
    fn exec_request_mounts_only_workspace_and_system() {
        let workdir = tempfile::tempdir().unwrap();
        let argv = vec!["/usr/bin/true".to_owned()];
        let request = build_request(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
        );
        assert_eq!(request.grants.len(), 2, "exactly two grants: system and workspace");
        assert_eq!(
            request.grants[1].destination,
            WORKSPACE_DESTINATION,
            "the working directory must mount at /workspace"
        );
        assert_eq!(request.grants[1].access, Access::ReadWrite);
        assert_eq!(request.grants[0].destination, SYSTEM_DESTINATION);
        assert_eq!(request.grants[0].access, Access::ReadOnly);
    }

    #[test]
    fn exec_request_denies_network_and_uses_system_toolchain() {
        let workdir = tempfile::tempdir().unwrap();
        let argv = vec!["/usr/bin/true".to_owned()];
        let request = build_request(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
        );
        assert_eq!(request.network.mode, NetworkMode::Deny);
        assert!(request.network.allowed_sources.is_empty());
        assert!(matches!(request.toolchain, Toolchain::System { .. }));
        assert!(request.cache.is_none());
        assert!(request.scratch.is_none());
    }
}
