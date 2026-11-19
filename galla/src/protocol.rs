//! Strict, closed version-one wire protocol for the Kvist sandbox runner.
//!
//! These types are the runner's independent statement of the
//! `kvist-sandbox-request-v1` contract. Every type rejects unknown fields:
//! accepting an omitted or unrecognised capability is never compatible with
//! this pre-release protocol.

use std::collections::BTreeMap;

use hex::encode as hex_encode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The exact accepted request protocol identifier.
pub const REQUEST_PROTOCOL: &str = "kvist-sandbox-request-v1";
/// The exact accepted probe protocol identifier.
pub const PROBE_PROTOCOL: &str = "kvist-sandbox-probe-v1";
/// The single supported version of both protocol messages.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum accepted request size before parsing.
pub const MAX_REQUEST_BYTES: usize = 1 << 20;
/// Maximum grants in one request.
pub const MAX_GRANTS: usize = 256;
/// Maximum environment entries in one request.
pub const MAX_ENVIRONMENT_ENTRIES: usize = 256;
/// Maximum argv entries in one request.
pub const MAX_ARGV_ENTRIES: usize = 1024;
/// Maximum package sources in one request, including crates.io.
pub const MAX_NETWORK_SOURCES: usize = 64;
/// Maximum promotion-manifest entries in one request.
pub const MAX_CACHE_MANIFEST_ENTRIES: usize = 4096;
/// Maximum byte length of one scalar protocol value.
pub const MAX_VALUE_BYTES: usize = 4096;

/// Maximum accepted wall-clock time (24 hours).
pub const MAX_WALL_TIME_MS: u64 = 24 * 60 * 60 * 1000;
/// Maximum captured output (256 MiB).
pub const MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum concurrent processes.
pub const MAX_PROCESSES: u64 = 4096;
/// Maximum open files.
pub const MAX_FILES: u64 = 1 << 20;
/// Maximum individual file size (8 GiB).
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// Maximum scratch storage (64 GiB).
pub const MAX_SCRATCH_BYTES: u64 = 64 * 1024 * 1024 * 1024;
/// Maximum mediated Cargo-home storage (64 GiB).
pub const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

/// The disjoint authority phase of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// An authoring agent.
    Authoring,
    /// A network-denied build/test verification or native source-only admission.
    Verification,
    /// A non-compiling Cargo dependency acquisition.
    DependencyAcquisition,
}

/// Mount access mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    /// The destination is read-only.
    ReadOnly,
    /// The destination is read-write.
    ReadWrite,
}

/// The fixed purpose of a mount grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Purpose {
    /// Read-only task context.
    Context,
    /// Writable implementation or test root.
    Authoring,
    /// Read-only verification workspace.
    Verification,
    /// Read-only executable/toolchain material.
    Toolchain,
    /// Writable target and HOME scratch.
    Scratch,
    /// Cargo-home content.
    DependencyCache,
    /// Isolated writable workspace containing the acquisition lockfile.
    Lockfile,
    /// Registry data: read-only for verification, writable only for native
    /// dependency acquisition; also explicit read-only vendor exports.
    Registry,
    /// Read-only language offline configuration that directs the resolver at
    /// the vendored material (for example a `.cargo/config.toml`).
    CargoConfig,
    /// Read-only runtime bin directory exposing the toolchain binaries the
    /// build invokes by name (`rustc`, `rustdoc`) plus the system linker and
    /// archiver (`cc`, `ar`, `as`) so a locked build can compile and link
    /// without a host `PATH`.
    Runtime,
}

impl Purpose {
    /// The canonical mount-plan byte for this purpose. The byte values are
    /// the stable encoding the runner re-digests to bind an approved mount
    /// plan to the grants that actually mount.
    pub fn byte(self) -> u8 {
        match self {
            Purpose::Context => b'0',
            Purpose::Authoring => b'1',
            Purpose::Scratch => b'2',
            Purpose::Toolchain => b'3',
            Purpose::Verification => b'4',
            Purpose::DependencyCache => b'5',
            Purpose::Lockfile => b'6',
            Purpose::Registry => b'7',
            Purpose::CargoConfig => b'8',
            Purpose::Runtime => b'9',
        }
    }
}

/// Network authority requested by the phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkMode {
    /// No network authority.
    Deny,
    /// Typed package-source authority for Cargo acquisition only.
    PackageSources,
}

/// The only supported enforcement backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    /// A Bubblewrap Linux namespace backend.
    Bubblewrap,
}

/// A source Cargo may contact in the acquisition phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AllowedSource {
    /// A Cargo registry with separate sparse-index and crate-download origins.
    CargoRegistry {
        /// Registry name.
        name: String,
        /// Exact canonical index origin.
        index_origin: String,
        /// Exact canonical download origin.
        download_origin: String,
        /// Derived source identity.
        identity: String,
    },
    /// An immutable Cargo Git dependency.
    CargoGit {
        /// Exact canonical HTTPS repository URL.
        repository: String,
        /// Immutable 40-digit lower-case revision.
        revision: String,
        /// Derived source identity.
        identity: String,
    },
}

/// The network capability block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    /// Requested network mode.
    pub mode: NetworkMode,
    /// Sources the transport may contact in package-source mode.
    pub allowed_sources: Vec<AllowedSource>,
}

/// Bounded resource limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub wall_time_ms: u64,
    pub max_output_bytes: u64,
    pub max_processes: u64,
    pub max_files: u64,
    /// Optional per-process file-write cap, not a quota on existing host files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_file_bytes: Option<u64>,
    pub max_scratch_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cache_bytes: Option<u64>,
}

/// Enforcement-backend identity covered by approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendIdentity {
    pub kind: BackendKind,
    pub path: String,
    pub digest: String,
}

/// Approval-bound request identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identities {
    pub runner: String,
    pub backend: BackendIdentity,
    pub policy: String,
    pub toolchain: String,
    pub command: String,
    pub mount_plan: String,
}

/// The canonical digest of an approval-bound command, recomputed from the
/// request argv. The encoding is `command` followed by a NUL, then each argv
/// element joined by NUL separators (the first element is not preceded by a
/// separator). This is the exact byte sequence the runner re-digests to bind
/// an approved command to the argv that actually runs.
pub fn command_identity(argv: &[String]) -> String {
    let mut input = b"command\0".to_vec();
    for (index, argument) in argv.iter().enumerate() {
        if index > 0 {
            input.push(b'\0');
        }
        input.extend_from_slice(argument.as_bytes());
    }
    format!("sha256:{}", hex_encode(Sha256::digest(&input)))
}

/// The canonical digest of an approval-bound mount plan, recomputed from the
/// request grants. Each grant contributes its destination, a NUL, an access
/// byte (`0` read-only, `1` read-write), a purpose byte (see
/// [`Purpose::byte`]), and a NUL. The runner re-digests the concatenation to
/// bind the approved mount plan to the grants that actually mount.
pub fn mount_plan_identity(grants: &[Grant]) -> String {
    let mut plan = Vec::new();
    for grant in grants {
        plan.extend_from_slice(grant.destination.as_bytes());
        plan.push(b'\0');
        plan.push(match grant.access {
            Access::ReadOnly => b'0',
            Access::ReadWrite => b'1',
        });
        plan.push(grant.purpose.byte());
        plan.push(b'\0');
    }
    format!("sha256:{}", hex_encode(Sha256::digest(&plan)))
}

/// One typed host-to-sandbox mount grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// Canonical absolute host source path.
    pub source: String,
    /// Fixed canonical absolute sandbox destination.
    pub destination: String,
    pub access: Access,
    pub purpose: Purpose,
    pub identity: String,
}

/// The approved toolchain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Toolchain {
    /// A generic system toolchain rooted at an approval-bound immutable root.
    System { identity: String, root: String },
    /// A usable Cargo toolchain: an approval-bound immutable root that contains
    /// `cargo`, `rustc`, `rustlib`, and the linker dependencies, plus the exact
    /// `cargo` executable path beneath that root. The single read-only
    /// toolchain grant is the root, not the executable file alone.
    Cargo {
        identity: String,
        root: String,
        cargo: String,
    },
}

/// A cache endpoint backed by one exact grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheEndpoint {
    pub destination: String,
    pub identity: String,
}

/// The pre-execution identity of the isolated writable lockfile workspace.
///
/// Only `before_identity` (the `Cargo.lock` content before `cargo fetch`) is a
/// legitimate authorization input. The post-fetch lockfile identity is an
/// *observation* the runner integration takes after the child exits; it is
/// deliberately absent from the request so no producer can assert an after
/// state the runner did not itself measure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockfileWorkspace {
    /// Writable workspace mount containing `Cargo.lock`.
    pub destination: String,
    /// The lockfile content identity before acquisition.
    pub before_identity: String,
}

/// Pre-execution authorization intent for constructing an immutable cache
/// generation from the fetched Cargo home.
///
/// This object carries only inputs that can be honestly authorized before the
/// child runs: whether promotion is intended and the writable Cargo home that
/// will supply the bytes. The post-fetch manifest, promoted-file checksums,
/// the observed lockfile-after identity, and the resulting generation identity
/// are all derived by the runner integration *after* the child exits and are
/// never accepted from the request. The destination is likewise absent: the
/// integration opens a provider-owned generation parent and passes that
/// capability directly to the cache primitive, so this wire object never
/// authorizes mutation of an arbitrary path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachePromotion {
    pub enabled: bool,
    /// Must be the real writable Cargo home used by `cargo fetch`.
    pub source: String,
}

/// The Cargo-home topology for a Cargo phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cache {
    /// The actual `CARGO_HOME`, not an unrelated staging cache.
    pub cargo_home: String,
    /// Native fixed data destination, or `${cargo_home}/registry` in export mode.
    pub registry: String,
    /// Native fixed data destination, or `${cargo_home}/git` in export mode.
    pub git: String,
    /// Explicit native data mode with private scratch-owned Cargo admin state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeCache>,
    /// Writable Cargo-home endpoint in acquisition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writable: Option<CacheEndpoint>,
    /// Read-only project-approved Cargo-home endpoint in verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<CacheEndpoint>,
    /// Acquisition lockfile workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lockfile: Option<LockfileWorkspace>,
    /// Bound acquisition result ready for immutable generation creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promotion: Option<CachePromotion>,
}

/// Native Cargo data grants, excluding whole-home administration/configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCache {
    /// Optional registry index, archives and expanded source data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<CacheEndpoint>,
    /// Optional Git databases and checkouts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<CacheEndpoint>,
    /// Approved named registry aliases, excluding ambient configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registries: Option<Vec<NativeRegistryAlias>>,
}

/// One explicitly approved native registry name, index origin and protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRegistryAlias {
    /// Unique safe non-built-in registry name.
    pub name: String,
    /// Canonical HTTPS index origin, without Cargo's `sparse+` prefix.
    pub index_origin: String,
    /// Index transport semantics; omission selects sparse.
    #[serde(default)]
    pub protocol: NativeRegistryProtocol,
}

/// Cargo index semantics, independent of source networking authority.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeRegistryProtocol {
    /// Preserve an HTTPS Git index URL without a sparse prefix.
    Git,
    /// Prefix an HTTPS index URL ending in `/` with `sparse+`.
    #[default]
    Sparse,
}

/// Writable target/HOME scratch endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scratch {
    pub destination: String,
    pub identity: String,
}

/// The complete closed execution request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxRequest {
    pub protocol: String,
    pub protocol_version: u32,
    pub phase: Phase,
    pub argv: Vec<String>,
    pub working_directory: String,
    pub environment: BTreeMap<String, String>,
    pub network: Network,
    pub resources: Resources,
    pub identities: Identities,
    pub grants: Vec<Grant>,
    pub toolchain: Toolchain,
    pub cache: Option<Cache>,
    pub scratch: Option<Scratch>,
    /// The canonical absolute sandbox destination for the user home, mounted
    /// as a writable tmpfs. When `Some`, the enforcement mounts a tmpfs at
    /// this path before the grants, so read-only toolchain caches can be
    /// mounted into it at their expected `$HOME`-relative locations. When
    /// `None`, no home tmpfs is mounted and the `HOME` environment variable
    /// is set to the working directory.
    #[serde(default)]
    pub home: Option<String>,
}

/// Reference to a verified executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutableReference {
    pub path: String,
    pub digest: String,
}

/// Kernel namespace capabilities verified by a future probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Namespaces {
    pub mount: bool,
    pub network: bool,
    pub pid: bool,
    pub ipc: bool,
    pub uts: bool,
    pub user: bool,
}

/// Verified backend capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub namespaces: Namespaces,
    pub new_session: bool,
    pub parent_death_signal: bool,
}

/// Closed probe response shape retained for the later Linux backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxProbe {
    pub protocol: String,
    pub protocol_version: u32,
    pub runner: ExecutableReference,
    pub backend: BackendReference,
    pub capabilities: Capabilities,
}

/// Probe reference to the enforcement backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendReference {
    pub kind: BackendKind,
    pub path: String,
    pub digest: String,
}
