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
    Access, BackendIdentity, BackendKind, Grant, Identities, Network, NetworkMode, Phase, Purpose,
    REQUEST_PROTOCOL, Resources, SandboxRequest, Toolchain, command_identity, mount_plan_identity,
};

/// The canonical absolute sandbox destination for the working directory.
pub const WORKSPACE_DESTINATION: &str = "/workspace";
/// The canonical absolute sandbox destination for the system tree.
pub const SYSTEM_DESTINATION: &str = "/usr";
/// The sandbox `PATH` exposing the system bin directories.
pub const SYSTEM_PATH: &str = "/usr/local/bin:/usr/bin:/bin";
/// The sandbox destination for the host Rust runtime (shims + initialize).
pub const RUST_RUNTIME_DEST: &str = "/rust/runtime";
/// The sandbox destination for the active Rust toolchain.
pub const RUST_TOOLCHAIN_DEST: &str = "/rust/toolchain";
/// The sandbox destination for the rustup home (settings + toolchain links).
pub const RUSTUP_HOME_DEST: &str = "/rust/rustup-home";
/// The sandbox destination for the rustup launcher.
pub const RUSTUP_DEST: &str = "/rust/rustup";
/// The sandbox destination for the offline Cargo registry.
pub const RUST_REGISTRY_DEST: &str = "/rust/cargo-registry";
/// The sandbox destination for the offline Cargo git dependencies.
pub const RUST_GIT_DEST: &str = "/rust/cargo-git";
/// The sandbox destination for user-installed cargo binaries.
pub const RUST_USER_CARGO_BIN_DEST: &str = "/rust/user-cargo-bin";
/// The sandbox destination prefix for each installed toolchain.
pub const RUST_TOOLCHAINS_DEST: &str = "/rust/toolchains";
/// The sandbox `PATH` when a Rust root is mounted.
pub const RUST_PATH: &str =
    "/rust/runtime/bin:/rust/toolchain/bin:/rust/user-cargo-bin:/usr/local/bin:/usr/bin:/bin";
/// The sandbox destination prefix for the user home. The home directory is
/// `/home/<username>`, where `<username>` is inherited from the host `USER`
/// environment variable. The home is mounted as a writable tmpfs, and the
/// toolchain caches are mounted read-only into their expected `$HOME`-relative
/// locations, so the tools find everything where a normal system would have
/// it.
pub const HOME_PREFIX: &str = "/home";
/// The default username when the host `USER` environment variable is unset.
pub const DEFAULT_USERNAME: &str = "user";

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
/// directories are always consulted, plus any `extra_dirs`), so `galla bash`
/// works as well as `galla /usr/bin/bash`. The resolved program path is
/// canonicalized so the request carries a canonical absolute path, as the
/// protocol requires. The remaining arguments are passed through unchanged.
pub fn resolve_argv(
    command_line: &[std::ffi::OsString],
    extra_dirs: &[PathBuf],
) -> Result<Vec<String>, String> {
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
        resolve_on_path(program_path, extra_dirs)?
    };
    let mut argv = vec![resolved.to_string_lossy().into_owned()];
    for argument in &command_line[1..] {
        argv.push(
            argument
                .to_str()
                .ok_or_else(|| {
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
/// The standard system directories and any `extra_dirs` are always consulted,
/// even when `PATH` is unset or cleared, so the exec mode does not depend on
/// an ambient environment. The first regular executable file found is
/// canonicalized and returned.
pub fn resolve_on_path(name: &Path, extra_dirs: &[PathBuf]) -> Result<PathBuf, String> {
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
    for directory in extra_dirs {
        if !directories.contains(directory) {
            directories.push(directory.clone());
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
                ));
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
        home: None,
    }
}

/// Discovers the host Rust resources under `root` and returns the
/// read-only grants that mount them at the same `/rust/*` destinations
/// Skott uses. Each entry is included only when the corresponding host
/// path exists, so a partial installation is mounted as far as it goes.
/// The root must be a directory; a missing or non-directory root is an
/// error because `--rust` was explicitly requested.
pub fn rust_grants(root: &Path) -> Result<Vec<(String, String)>, String> {
    let metadata = std::fs::symlink_metadata(root)
        .map_err(|error| format!("cannot inspect the Rust root `{}`: {error}", root.display()))?;
    if !metadata.is_dir() {
        return Err(format!(
            "the Rust root `{}` must be a directory",
            root.display()
        ));
    }
    let mut grants = Vec::new();
    // Fixed sub-resources, mounted at their canonical destinations.
    for (source, destination) in [
        (root.join("runtime"), RUST_RUNTIME_DEST),
        (root.join("toolchain"), RUST_TOOLCHAIN_DEST),
        (root.join("rustup-home"), RUSTUP_HOME_DEST),
        (root.join("cargo-registry"), RUST_REGISTRY_DEST),
        (root.join("cargo-git"), RUST_GIT_DEST),
        (root.join("user-cargo-bin"), RUST_USER_CARGO_BIN_DEST),
    ] {
        if source.is_dir() {
            grants.push((
                source.to_string_lossy().into_owned(),
                destination.to_owned(),
            ));
        }
    }
    // The rustup launcher is a file, not a directory.
    let rustup = root.join("rustup");
    if rustup.is_file() {
        grants.push((
            rustup.to_string_lossy().into_owned(),
            RUSTUP_DEST.to_owned(),
        ));
    }
    // Each installed toolchain, individually, at /rust/toolchains/<name>.
    if let Ok(entries) = std::fs::read_dir(root.join("toolchains")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                grants.push((
                    path.to_string_lossy().into_owned(),
                    format!("{RUST_TOOLCHAINS_DEST}/{name}"),
                ));
            }
        }
    }
    Ok(grants)
}

/// The host toolchain roots requested by the exec command line. Each field,
/// when `Some`, names a host directory that is mounted read-only at the
/// expected `$HOME`-relative location so the corresponding tool finds its
/// cache where a normal system would have it. The interpreters and compilers
/// for Go, Node, and Python live in the system tree, so only their caches
/// need mounting. Rust is different: its toolchain and registry live outside
/// the system tree, so `rust` names the host Rust root and the runtime,
/// toolchain, rustup home, and offline registry are mounted at the same
/// `/rust/*` destinations Skott uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolchainRoots {
    /// The host Rust root, mounted at the `/rust/*` destinations.
    pub rust: Option<PathBuf>,
    /// The host Go module cache, mounted at `$HOME/go`.
    pub go: Option<PathBuf>,
    /// The host npm cache, mounted at `$HOME/.npm`.
    pub node: Option<PathBuf>,
    /// The host uv cache, mounted at `$HOME/.cache/uv`.
    pub python: Option<PathBuf>,
}

impl ToolchainRoots {
    /// Returns `true` when at least one toolchain root is requested.
    pub fn any(&self) -> bool {
        self.rust.is_some() || self.go.is_some() || self.node.is_some() || self.python.is_some()
    }

    /// Returns the read-only grants for the requested toolchain roots, given
    /// the sandbox home directory. Each root is mounted at its expected
    /// `$HOME`-relative location (or, for Rust, at the `/rust/*`
    /// destinations). A missing or non-directory root is an error because the
    /// corresponding flag was explicitly requested.
    pub fn grants(&self, home: &str) -> Result<Vec<(String, String)>, String> {
        let mut grants = Vec::new();
        if let Some(root) = &self.rust {
            grants.extend(rust_grants(root)?);
        }
        if let Some(root) = &self.go {
            require_directory(root, "Go")?;
            grants.push((root.to_string_lossy().into_owned(), format!("{home}/go")));
        }
        if let Some(root) = &self.node {
            require_directory(root, "Node")?;
            grants.push((root.to_string_lossy().into_owned(), format!("{home}/.npm")));
        }
        if let Some(root) = &self.python {
            require_directory(root, "Python")?;
            grants.push((
                root.to_string_lossy().into_owned(),
                format!("{home}/.cache/uv"),
            ));
        }
        Ok(grants)
    }

    /// Returns the bin directories to search when resolving a relative
    /// command name. For Rust, the runtime, toolchain, and user cargo bin
    /// directories are searched. For Go, Node, and Python, the interpreters
    /// are in the system tree, so no extra directories are needed.
    pub fn extra_dirs(&self) -> Vec<PathBuf> {
        match &self.rust {
            Some(root) => vec![
                root.join("runtime/bin"),
                root.join("toolchain/bin"),
                root.join("user-cargo-bin"),
            ],
            None => Vec::new(),
        }
    }
}

/// Requires that `root` is an existing directory, naming the toolchain in
/// the diagnostic. A missing or non-directory root is an error because the
/// corresponding flag was explicitly requested.
fn require_directory(root: &Path, toolchain: &str) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(root).map_err(|error| {
        format!(
            "cannot inspect the {toolchain} root `{}`: {error}",
            root.display()
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "the {toolchain} root `{}` must be a directory",
            root.display()
        ));
    }
    Ok(())
}

/// Wraps the payload argv so the Rust environment is set up under `$HOME`
/// before the payload runs.
///
/// When a Rust root is mounted, the read-only `/rust/*` mounts must be
/// bridged into the writable home locations that rustup and cargo expect,
/// namely `$HOME/.rustup` (settings and toolchain links) and `$HOME/.cargo`
/// (registry and git). The wrapped argv is a `bash -c` that creates those
/// directories, copies the rustup settings, symlinks the toolchains, and
/// links the offline registry and git dependencies, then `exec`s the original
/// command so the payload replaces the shell and the process group is
/// preserved. The setup uses `$HOME` (not the hardcoded `/tmp` paths from the
/// host `initialize` script) so the environment resembles a normal system,
/// with rustup and cargo finding their state where they would on a host and
/// no `RUSTUP_HOME` or `CARGO_HOME` overrides.
pub fn wrap_argv_with_rust_init(argv: &[String]) -> Vec<String> {
    let setup = format!(
        "mkdir -p \"$HOME/.rustup\" \"$HOME/.cargo\" \
         && cp {RUSTUP_HOME_DEST}/settings.toml \"$HOME/.rustup/settings.toml\" \
         && ln -sfn {RUSTUP_HOME_DEST}/toolchains \"$HOME/.rustup/toolchains\" \
         && {RUST_RUNTIME_DEST}/initialize \
         && ln -sfn {RUST_REGISTRY_DEST} \"$HOME/.cargo/registry\" \
         && ln -sfn {RUST_GIT_DEST} \"$HOME/.cargo/git\" \
         && ln -sfn {RUST_TOOLCHAIN_DEST} {RUST_TOOLCHAINS_DEST}/default"
    );
    let script = format!(
        "{setup} && exec {}",
        argv.iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ")
    );
    vec!["/usr/bin/bash".to_owned(), "-c".to_owned(), script]
}

/// Quotes a single argument for safe interpolation into a `bash -c` script.
///
/// The argument is wrapped in single quotes, with any embedded single quote
/// escaped as `'\''` (close quote, literal quote, open quote). This is the
/// standard POSIX shell quoting for arbitrary strings.
fn shell_quote(arg: &str) -> String {
    let mut quoted = String::with_capacity(arg.len() + 2);
    quoted.push('\'');
    for byte in arg.bytes() {
        if byte == b'\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(byte as char);
        }
    }
    quoted.push('\'');
    quoted
}

/// Builds a closed authoring-phase request for the exec command line,
/// optionally mounting the requested toolchain roots and a user home. When
/// no toolchain root is requested, the request is identical to
/// [`build_request`]: only the system tree and the working directory are
/// mounted, and `HOME` is the working directory. When at least one root is
/// requested, the home directory `/home/<username>` is mounted as a writable
/// tmpfs (the username is inherited from the host `USER` environment
/// variable), the toolchain caches are mounted read-only into their expected
/// `$HOME`-relative locations, and the environment is extended with the
/// toolchain variables. For Rust, the payload argv is also wrapped so the
/// `initialize` script runs first. The sandbox resembles a normal system:
/// the tools find their caches where they would on a host.
pub fn build_request_with_toolchains(
    argv: &[String],
    workdir: &Path,
    backend_path: &Path,
    backend_digest: &str,
    runner_digest: &str,
    roots: &ToolchainRoots,
) -> Result<SandboxRequest, String> {
    // When a Rust root is mounted, wrap the payload so the initialize script
    // runs first, bridging the read-only /rust/* mounts into the writable
    // home locations that rustup and cargo expect.
    let effective_argv: Vec<String> = match roots.rust {
        Some(_) => wrap_argv_with_rust_init(argv),
        None => argv.to_vec(),
    };
    let mut grants = vec![
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
    let mut environment = BTreeMap::from([
        ("HOME".to_owned(), WORKSPACE_DESTINATION.to_owned()),
        ("PATH".to_owned(), SYSTEM_PATH.to_owned()),
    ]);
    let toolchain_identity = match &roots.rust {
        Some(root) => digest_input(b"toolchain", root.to_string_lossy().as_bytes()),
        None => digest_input(b"toolchain", SYSTEM_DESTINATION.as_bytes()),
    };
    // The home directory is /home/<username>, where <username> is inherited
    // from the host USER environment variable. It is mounted as a writable
    // tmpfs by the enforcement, and the toolchain caches are mounted
    // read-only into it at their expected locations. When no toolchain root
    // is requested, no home tmpfs is mounted and HOME stays the working
    // directory.
    let mut home: Option<String> = None;
    if roots.any() {
        let username = std::env::var("USER")
            .ok()
            .filter(|user| !user.is_empty())
            .unwrap_or_else(|| DEFAULT_USERNAME.to_owned());
        let home_dir = format!("{HOME_PREFIX}/{username}");
        for (source, destination) in roots.grants(&home_dir)? {
            grants.push(Grant {
                source,
                destination,
                access: Access::ReadOnly,
                purpose: Purpose::Toolchain,
                identity: digest_input(b"grant-toolchain", home_dir.as_bytes()),
            });
        }
        environment.insert("HOME".to_owned(), home_dir.clone());
        if roots.rust.is_some() {
            environment.insert("PATH".to_owned(), RUST_PATH.to_owned());
            environment.insert(
                "CARGO_TARGET_DIR".to_owned(),
                format!("{WORKSPACE_DESTINATION}/target"),
            );
            environment.insert("CARGO_NET_OFFLINE".to_owned(), "true".to_owned());
            environment.insert("RUSTC".to_owned(), format!("{RUST_RUNTIME_DEST}/bin/rustc"));
            environment.insert(
                "RUSTDOC".to_owned(),
                format!("{RUST_RUNTIME_DEST}/bin/rustdoc"),
            );
        }
        home = Some(home_dir);
    }

    let command_identity = command_identity(&effective_argv);
    let mount_plan = mount_plan_identity(&grants);
    let policy_identity = digest_input(b"policy", b"exec");

    Ok(SandboxRequest {
        protocol: REQUEST_PROTOCOL.to_owned(),
        protocol_version: 1,
        phase: Phase::Authoring,
        argv: effective_argv,
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
            toolchain: toolchain_identity.clone(),
            command: command_identity,
            mount_plan,
        },
        toolchain: Toolchain::System {
            identity: toolchain_identity,
            root: SYSTEM_DESTINATION.to_owned(),
        },
        grants,
        cache: None,
        scratch: None,
        home,
    })
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
        validation::validate(&request).expect("the exec request must validate");
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
        validation::validate(&request).expect("a shell exec request must validate");
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
        assert_eq!(
            request.grants.len(),
            2,
            "exactly two grants: system and workspace"
        );
        assert_eq!(
            request.grants[1].destination, WORKSPACE_DESTINATION,
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

    /// Builds a fixture Rust root with the expected sub-resources so the
    /// grant discovery and request construction can be tested without a
    /// live host installation.
    fn fixture_rust_root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for directory in [
            "runtime",
            "toolchain",
            "rustup-home",
            "cargo-registry",
            "cargo-git",
            "user-cargo-bin",
            "toolchains",
        ] {
            std::fs::create_dir(root.path().join(directory)).unwrap();
        }
        // The rustup launcher is a file.
        std::fs::write(root.path().join("rustup"), "fixture rustup").unwrap();
        // Two installed toolchains.
        std::fs::create_dir(root.path().join("toolchains/stable")).unwrap();
        std::fs::create_dir(root.path().join("toolchains/nightly")).unwrap();
        root
    }

    #[test]
    fn rust_grants_mounts_every_present_resource() {
        let root = fixture_rust_root();
        let grants = rust_grants(root.path()).unwrap();
        let destinations: Vec<&str> = grants.iter().map(|(_, d)| d.as_str()).collect();
        assert!(destinations.contains(&RUST_RUNTIME_DEST));
        assert!(destinations.contains(&RUST_TOOLCHAIN_DEST));
        assert!(destinations.contains(&RUSTUP_HOME_DEST));
        assert!(destinations.contains(&RUST_REGISTRY_DEST));
        assert!(destinations.contains(&RUST_GIT_DEST));
        assert!(destinations.contains(&RUST_USER_CARGO_BIN_DEST));
        assert!(destinations.contains(&RUSTUP_DEST));
        assert!(destinations.contains(&format!("{RUST_TOOLCHAINS_DEST}/stable").as_str()));
        assert!(destinations.contains(&format!("{RUST_TOOLCHAINS_DEST}/nightly").as_str()));
    }

    #[test]
    fn rust_grants_requires_a_directory_root() {
        let root = fixture_rust_root();
        // A file is not a valid Rust root.
        let file = root.path().join("rustup");
        assert!(rust_grants(&file).is_err());
        // A missing root is an error.
        assert!(rust_grants(Path::new("/nonexistent/rust")).is_err());
    }

    /// Computes the sandbox home directory the same way the builder does:
    /// `/home/<username>`, where `<username>` is the host `USER` environment
    /// variable or the default.
    fn expected_home() -> String {
        let username = std::env::var("USER")
            .ok()
            .filter(|user| !user.is_empty())
            .unwrap_or_else(|| DEFAULT_USERNAME.to_owned());
        format!("{HOME_PREFIX}/{username}")
    }

    /// Builds a fixture directory to serve as a host toolchain cache root.
    fn fixture_cache_root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("marker"), "fixture").unwrap();
        root
    }

    #[test]
    fn exec_request_with_rust_mounts_rust_and_sets_environment() {
        let workdir = tempfile::tempdir().unwrap();
        let root = fixture_rust_root();
        let argv = vec!["/usr/bin/true".to_owned()];
        let roots = ToolchainRoots {
            rust: Some(root.path().to_owned()),
            ..Default::default()
        };
        let request = build_request_with_toolchains(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
            &roots,
        )
        .unwrap();
        // The Rust resources are mounted read-only at the canonical destinations.
        let rust_destinations: Vec<&str> = request
            .grants
            .iter()
            .filter(|grant| grant.destination.starts_with("/rust/"))
            .map(|grant| grant.destination.as_str())
            .collect();
        assert!(rust_destinations.contains(&RUST_RUNTIME_DEST));
        assert!(rust_destinations.contains(&RUST_TOOLCHAIN_DEST));
        assert!(rust_destinations.contains(&RUSTUP_HOME_DEST));
        assert!(rust_destinations.contains(&RUST_REGISTRY_DEST));
        assert!(rust_destinations.contains(&RUST_GIT_DEST));
        assert!(rust_destinations.contains(&RUST_USER_CARGO_BIN_DEST));
        assert!(rust_destinations.contains(&RUSTUP_DEST));
        assert!(rust_destinations.contains(&format!("{RUST_TOOLCHAINS_DEST}/stable").as_str()));
        assert!(rust_destinations.contains(&format!("{RUST_TOOLCHAINS_DEST}/nightly").as_str()));
        // Every Rust grant is read-only.
        for grant in &request.grants {
            if grant.destination.starts_with("/rust/") {
                assert_eq!(grant.access, Access::ReadOnly);
                assert_eq!(grant.purpose, Purpose::Toolchain);
            }
        }
        // The home is a real /home/<username> directory, not /tmp.
        let home = expected_home();
        assert_eq!(
            request.environment.get("HOME").map(String::as_str),
            Some(home.as_str())
        );
        assert_eq!(request.home.as_deref(), Some(home.as_str()));
        // The Rust environment uses the standard locations under $HOME, with
        // no RUSTUP_HOME or CARGO_HOME overrides.
        assert_eq!(
            request.environment.get("PATH").map(String::as_str),
            Some(RUST_PATH)
        );
        assert_eq!(
            request
                .environment
                .get("CARGO_TARGET_DIR")
                .map(String::as_str),
            Some("/workspace/target")
        );
        assert_eq!(
            request
                .environment
                .get("CARGO_NET_OFFLINE")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            request.environment.get("RUSTC").map(String::as_str),
            Some("/rust/runtime/bin/rustc")
        );
        assert_eq!(
            request.environment.get("RUSTDOC").map(String::as_str),
            Some("/rust/runtime/bin/rustdoc")
        );
        assert!(
            !request.environment.contains_key("RUSTUP_HOME"),
            "no RUSTUP_HOME override; rustup uses $HOME/.rustup"
        );
        assert!(
            !request.environment.contains_key("CARGO_HOME"),
            "no CARGO_HOME override; cargo uses $HOME/.cargo"
        );
    }

    #[test]
    fn exec_request_with_rust_validates() {
        let workdir = tempfile::tempdir().unwrap();
        let root = fixture_rust_root();
        let argv = vec!["/usr/bin/true".to_owned()];
        let roots = ToolchainRoots {
            rust: Some(root.path().to_owned()),
            ..Default::default()
        };
        let request = build_request_with_toolchains(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
            &roots,
        )
        .unwrap();
        validation::validate(&request).expect("the Rust exec request must validate");
    }

    #[test]
    fn exec_request_with_go_mounts_module_cache_in_home() {
        let workdir = tempfile::tempdir().unwrap();
        let root = fixture_cache_root();
        let argv = vec!["/usr/bin/true".to_owned()];
        let roots = ToolchainRoots {
            go: Some(root.path().to_owned()),
            ..Default::default()
        };
        let request = build_request_with_toolchains(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
            &roots,
        )
        .unwrap();
        let home = expected_home();
        // The Go module cache is mounted read-only at $HOME/go.
        let go_grant = request
            .grants
            .iter()
            .find(|grant| grant.destination == format!("{home}/go"))
            .expect("the Go module cache must be mounted at $HOME/go");
        assert_eq!(go_grant.access, Access::ReadOnly);
        assert_eq!(go_grant.purpose, Purpose::Toolchain);
        // The home is a real /home/<username> directory.
        assert_eq!(request.home.as_deref(), Some(home.as_str()));
        assert_eq!(
            request.environment.get("HOME").map(String::as_str),
            Some(home.as_str())
        );
        // No Rust environment variables are set.
        assert!(!request.environment.contains_key("RUSTC"));
        assert!(!request.environment.contains_key("CARGO_HOME"));
    }

    #[test]
    fn exec_request_with_node_mounts_npm_cache_in_home() {
        let workdir = tempfile::tempdir().unwrap();
        let root = fixture_cache_root();
        let argv = vec!["/usr/bin/true".to_owned()];
        let roots = ToolchainRoots {
            node: Some(root.path().to_owned()),
            ..Default::default()
        };
        let request = build_request_with_toolchains(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
            &roots,
        )
        .unwrap();
        let home = expected_home();
        // The npm cache is mounted read-only at $HOME/.npm.
        let node_grant = request
            .grants
            .iter()
            .find(|grant| grant.destination == format!("{home}/.npm"))
            .expect("the npm cache must be mounted at $HOME/.npm");
        assert_eq!(node_grant.access, Access::ReadOnly);
        assert_eq!(node_grant.purpose, Purpose::Toolchain);
        assert_eq!(request.home.as_deref(), Some(home.as_str()));
    }

    #[test]
    fn exec_request_with_python_mounts_uv_cache_in_home() {
        let workdir = tempfile::tempdir().unwrap();
        let root = fixture_cache_root();
        let argv = vec!["/usr/bin/true".to_owned()];
        let roots = ToolchainRoots {
            python: Some(root.path().to_owned()),
            ..Default::default()
        };
        let request = build_request_with_toolchains(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
            &roots,
        )
        .unwrap();
        let home = expected_home();
        // The uv cache is mounted read-only at $HOME/.cache/uv.
        let python_grant = request
            .grants
            .iter()
            .find(|grant| grant.destination == format!("{home}/.cache/uv"))
            .expect("the uv cache must be mounted at $HOME/.cache/uv");
        assert_eq!(python_grant.access, Access::ReadOnly);
        assert_eq!(python_grant.purpose, Purpose::Toolchain);
        assert_eq!(request.home.as_deref(), Some(home.as_str()));
    }

    #[test]
    fn exec_request_with_multiple_toolchains_mounts_all() {
        let workdir = tempfile::tempdir().unwrap();
        let rust_root = fixture_rust_root();
        let go_root = fixture_cache_root();
        let node_root = fixture_cache_root();
        let python_root = fixture_cache_root();
        let argv = vec!["/usr/bin/true".to_owned()];
        let roots = ToolchainRoots {
            rust: Some(rust_root.path().to_owned()),
            go: Some(go_root.path().to_owned()),
            node: Some(node_root.path().to_owned()),
            python: Some(python_root.path().to_owned()),
        };
        let request = build_request_with_toolchains(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
            &roots,
        )
        .unwrap();
        let home = expected_home();
        // The Rust resources are mounted at /rust/*.
        assert!(
            request
                .grants
                .iter()
                .any(|grant| grant.destination == RUST_RUNTIME_DEST)
        );
        // The Go, Node, and Python caches are mounted under $HOME.
        assert!(
            request
                .grants
                .iter()
                .any(|grant| grant.destination == format!("{home}/go"))
        );
        assert!(
            request
                .grants
                .iter()
                .any(|grant| grant.destination == format!("{home}/.npm"))
        );
        assert!(
            request
                .grants
                .iter()
                .any(|grant| grant.destination == format!("{home}/.cache/uv"))
        );
        // The home is a real /home/<username> directory.
        assert_eq!(request.home.as_deref(), Some(home.as_str()));
    }

    #[test]
    fn exec_request_without_toolchains_is_unchanged() {
        let workdir = tempfile::tempdir().unwrap();
        let argv = vec!["/usr/bin/true".to_owned()];
        let plain = build_request(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
        );
        let with_none = build_request_with_toolchains(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
            &ToolchainRoots::default(),
        )
        .unwrap();
        // No toolchain roots produces the same request as the plain builder.
        assert_eq!(plain.grants.len(), with_none.grants.len());
        assert_eq!(plain.environment, with_none.environment);
        assert_eq!(plain.identities.toolchain, with_none.identities.toolchain);
        assert_eq!(plain.home, with_none.home);
        assert!(with_none.home.is_none(), "no home tmpfs without toolchains");
        assert_eq!(
            with_none.environment.get("HOME").map(String::as_str),
            Some(WORKSPACE_DESTINATION),
            "HOME is the working directory without toolchains"
        );
    }

    #[test]
    fn toolchain_rots_grants_requires_a_directory_root() {
        let root = fixture_cache_root();
        // A file is not a valid toolchain root.
        let file = root.path().join("marker");
        let roots = ToolchainRoots {
            go: Some(file.clone()),
            ..Default::default()
        };
        assert!(roots.grants("/home/user").is_err());
        // A missing root is an error.
        let roots = ToolchainRoots {
            node: Some(PathBuf::from("/nonexistent/node")),
            ..Default::default()
        };
        assert!(roots.grants("/home/user").is_err());
    }

    #[test]
    fn wrap_argv_with_rust_init_sets_up_home_then_execs_payload() {
        let argv = vec!["/usr/bin/cargo".to_owned(), "build".to_owned()];
        let wrapped = wrap_argv_with_rust_init(&argv);
        assert_eq!(wrapped.len(), 3);
        assert_eq!(wrapped[0], "/usr/bin/bash");
        assert_eq!(wrapped[1], "-c");
        // The script sets up the home locations and then execs the payload.
        let script = &wrapped[2];
        assert!(script.contains("mkdir -p \"$HOME/.rustup\" \"$HOME/.cargo\""));
        assert!(script.contains(&format!("cp {RUSTUP_HOME_DEST}/settings.toml")));
        assert!(script.contains(&format!("ln -sfn {RUSTUP_HOME_DEST}/toolchains")));
        assert!(script.contains(&format!("{RUST_RUNTIME_DEST}/initialize")));
        assert!(script.contains(&format!(
            "ln -sfn {RUST_REGISTRY_DEST} \"$HOME/.cargo/registry\""
        )));
        assert!(script.contains(&format!("ln -sfn {RUST_GIT_DEST} \"$HOME/.cargo/git\"")));
        assert!(script.contains(&format!(
            "ln -sfn {RUST_TOOLCHAIN_DEST} {RUST_TOOLCHAINS_DEST}/default"
        )));
        assert!(script.ends_with("&& exec '/usr/bin/cargo' 'build'"));
    }

    #[test]
    fn wrap_argv_with_rust_init_quotes_embedded_single_quotes() {
        let argv = vec![
            "/usr/bin/bash".to_owned(),
            "-c".to_owned(),
            "echo 'hi'".to_owned(),
        ];
        let wrapped = wrap_argv_with_rust_init(&argv);
        // The embedded single quotes are escaped as '\'' so the payload
        // argument survives the outer bash -c quoting.
        assert!(wrapped[2].contains(&"echo 'hi'".replace('\'', "'\\''")));
    }

    #[test]
    fn exec_request_with_rust_wraps_the_payload_argv() {
        let workdir = tempfile::tempdir().unwrap();
        let root = fixture_rust_root();
        let argv = vec!["/usr/bin/cargo".to_owned(), "build".to_owned()];
        let roots = ToolchainRoots {
            rust: Some(root.path().to_owned()),
            ..Default::default()
        };
        let request = build_request_with_toolchains(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
            &roots,
        )
        .unwrap();
        // The request argv is the wrapped form, not the raw payload.
        assert_eq!(request.argv[0], "/usr/bin/bash");
        assert_eq!(request.argv[1], "-c");
        assert!(request.argv[2].contains("mkdir -p \"$HOME/.rustup\" \"$HOME/.cargo\""));
        assert!(request.argv[2].ends_with("&& exec '/usr/bin/cargo' 'build'"));
        // The command identity binds the wrapped argv.
        assert_eq!(request.identities.command, command_identity(&request.argv));
    }

    #[test]
    fn exec_request_without_rust_does_not_wrap_the_payload_argv() {
        let workdir = tempfile::tempdir().unwrap();
        let root = fixture_cache_root();
        let argv = vec!["/usr/bin/go".to_owned(), "build".to_owned()];
        let roots = ToolchainRoots {
            go: Some(root.path().to_owned()),
            ..Default::default()
        };
        let request = build_request_with_toolchains(
            &argv,
            workdir.path(),
            Path::new("/usr/bin/bwrap"),
            &fixture_digest(b"backend"),
            &fixture_digest(b"runner"),
            &roots,
        )
        .unwrap();
        // Without a Rust root, the payload argv is not wrapped.
        assert_eq!(request.argv, argv);
    }
}
