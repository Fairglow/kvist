//! Strict, closed version-one wire protocol for the Kvist sandbox runner.
//!
//! These types are the runner's independent statement of the
//! `kvist-sandbox-request-v1` contract. Every type rejects unknown fields:
//! accepting an omitted or unrecognised capability is never compatible with
//! this pre-release protocol.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

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
    /// A network-denied build/test verification.
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
    pub max_file_bytes: u64,
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
    /// Must be `${cargo_home}/registry`.
    pub registry: String,
    /// Must be `${cargo_home}/git`.
    pub git: String,
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
