//! Strict, closed version-one wire protocol for the Kvist sandbox runner.
//!
//! These types are the runner's own independent statement of the
//! `kvist-sandbox-request-v1` and `kvist-sandbox-probe-v1` contracts. They do
//! not import Kvist engine types and are the authoritative parser for untrusted
//! request JSON. Every structure rejects unknown fields so a superseded
//! ("legacy") shape or a smuggled extra capability fails closed with an
//! actionable diagnostic instead of being silently ignored.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The exact accepted request protocol identifier.
pub const REQUEST_PROTOCOL: &str = "kvist-sandbox-request-v1";
/// The exact accepted probe protocol identifier.
pub const PROBE_PROTOCOL: &str = "kvist-sandbox-probe-v1";
/// The single supported protocol version for both messages.
pub const PROTOCOL_VERSION: u32 = 1;

/// Maximum accepted request size in bytes. Untrusted input beyond this bound is
/// rejected before parsing so a hostile producer cannot exhaust memory.
pub const MAX_REQUEST_BYTES: usize = 1 << 20;

/// Maximum number of grants accepted in one request.
pub const MAX_GRANTS: usize = 256;
/// Maximum number of environment variables accepted in one request.
pub const MAX_ENVIRONMENT_ENTRIES: usize = 256;
/// Maximum number of argument-vector entries accepted in one request.
pub const MAX_ARGV_ENTRIES: usize = 1024;
/// Maximum number of allowed network sources accepted in one request.
pub const MAX_NETWORK_SOURCES: usize = 64;
/// Maximum number of cache promotion manifest entries accepted in one request.
pub const MAX_CACHE_MANIFEST_ENTRIES: usize = 4096;
/// Maximum accepted byte length of any single path or scalar string value.
pub const MAX_VALUE_BYTES: usize = 4096;

/// Explicit safe maxima for each bounded [`Resources`] limit. Untrusted input
/// beyond any of these bounds is rejected so a hostile or buggy producer cannot
/// request an absurd limit that would overflow arithmetic, exhaust host
/// resources, or defeat the intent of a bounded execution. Each maximum is
/// deliberately generous enough for legitimate build/test/authoring workloads
/// while remaining finite; the engine producer's defaults and options must
/// never exceed these values.
///
/// Maximum accepted wall-clock time limit in milliseconds (24 hours).
pub const MAX_WALL_TIME_MS: u64 = 24 * 60 * 60 * 1000;
/// Maximum accepted combined captured output limit in bytes (256 MiB).
pub const MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum accepted concurrent-process limit.
pub const MAX_PROCESSES: u64 = 4096;
/// Maximum accepted open-file limit.
pub const MAX_FILES: u64 = 1 << 20;
/// Maximum accepted single-file size limit in bytes (8 GiB).
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// Maximum accepted total scratch storage limit in bytes (64 GiB).
pub const MAX_SCRATCH_BYTES: u64 = 64 * 1024 * 1024 * 1024;
/// Maximum accepted total mediated dependency-cache storage limit in bytes
/// (64 GiB).
pub const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024 * 1024;

/// The execution phase a request is authorized for.
///
/// Phases are disjoint filesystem and network authorities. The runner never
/// infers a phase; an unknown phase is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// An authoring agent that may write only explicit implementation/test roots.
    Authoring,
    /// A verification build/test run with network denied and read-only inputs.
    Verification,
    /// A distinct mediated dependency-acquisition phase (network-restricted).
    DependencyAcquisition,
}

/// Access mode granted for one mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    /// The destination is mounted read-only.
    ReadOnly,
    /// The destination is mounted read-write.
    ReadWrite,
}

/// The approved role a mount fills inside the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Purpose {
    /// Read-only local component context for the agent or build.
    Context,
    /// A writable implementation/test root for an authoring agent.
    Authoring,
    /// Read-only workspace or provider input required by verification.
    Verification,
    /// A read-only toolchain root.
    Toolchain,
    /// A writable, size-bounded scratch root.
    Scratch,
    /// Read-only mediated dependency cache content.
    DependencyCache,
}

/// The network capability profile requested for the phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkMode {
    /// No network namespace connectivity is granted.
    Deny,
    /// Only explicitly listed typed package sources may be reached during a
    /// mediated dependency-acquisition phase.
    PackageSources,
}

/// The supported enforcement backend kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    /// A Bubblewrap-backed Linux namespace sandbox.
    Bubblewrap,
}

/// One typed, exact package source permitted during mediated acquisition.
///
/// The runner parses these shapes strictly now so a request that describes a
/// mediated acquisition can be represented, but it defers semantic source and
/// promotion enforcement (immutable Git pins, exact registry origins) to the
/// later Bubblewrap integration. Unknown source kinds and unknown fields are
/// rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AllowedSource {
    /// A Cargo registry source.
    CargoRegistry {
        /// The registry name.
        name: String,
        /// The exact registry index origin URL.
        index_origin: String,
        /// The exact crate download origin URL, when declared.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        download_origin: Option<String>,
        /// The `sha256:` approval-bound identity.
        identity: String,
    },
    /// A Cargo Git dependency source.
    CargoGit {
        /// The exact repository URL, when declared.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repository: Option<String>,
        /// The requested branch, when declared.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        /// The exact immutable revision, when declared.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<String>,
        /// The `sha256:` approval-bound identity.
        identity: String,
    },
}

/// The network capability block of a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    /// The requested network mode.
    pub mode: NetworkMode,
    /// Exact allowed sources; MUST be empty when `mode` is `deny` and MUST be
    /// nonempty when `mode` is `package-sources`.
    pub allowed_sources: Vec<AllowedSource>,
}

/// The bounded resource limits enforced for the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    /// Wall-clock time limit in milliseconds.
    pub wall_time_ms: u64,
    /// Maximum combined captured output in bytes.
    pub max_output_bytes: u64,
    /// Maximum number of concurrent processes.
    pub max_processes: u64,
    /// Maximum number of open files.
    pub max_files: u64,
    /// Maximum size of any single written file in bytes.
    pub max_file_bytes: u64,
    /// Maximum total scratch storage in bytes.
    pub max_scratch_bytes: u64,
    /// Maximum total mediated dependency-cache storage in bytes, when a cache
    /// is present. Absent for phases that declare no dependency cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cache_bytes: Option<u64>,
}

/// The identity of the enforcement backend covered by approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendIdentity {
    /// The backend kind.
    pub kind: BackendKind,
    /// The absolute path of the backend executable.
    pub path: String,
    /// The `sha256:` content digest of the backend executable.
    pub digest: String,
}

/// The approval-bound identities the runner must confirm before enforcement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identities {
    /// The `sha256:` digest of the trusted runner bytes.
    pub runner: String,
    /// The enforcement backend identity.
    pub backend: BackendIdentity,
    /// The `sha256:` digest binding the approved policy.
    pub policy: String,
    /// The `sha256:` digest binding the approved toolchain.
    pub toolchain: String,
    /// The `sha256:` digest binding the exact command.
    pub command: String,
    /// The `sha256:` digest binding the exact mount plan.
    pub mount_plan: String,
}

/// One typed filesystem mount grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// The canonical absolute host source path.
    pub source: String,
    /// The fixed absolute sandbox destination path.
    pub destination: String,
    /// The access mode.
    pub access: Access,
    /// The approved purpose.
    pub purpose: Purpose,
    /// The `sha256:` approval-bound identity of the grant.
    pub identity: String,
}

/// The toolchain block of a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Toolchain {
    /// A system-provided toolchain mounted read-only from an absolute root.
    System {
        /// The `sha256:` approval-bound identity.
        identity: String,
        /// The absolute toolchain root.
        root: String,
    },
    /// A Cargo toolchain reached through an approved absolute `cargo` path.
    Cargo {
        /// The `sha256:` approval-bound identity.
        identity: String,
        /// The absolute path of the approved `cargo` executable.
        cargo: String,
    },
}

/// One endpoint of the mediated dependency cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheEndpoint {
    /// The absolute sandbox destination of the cache endpoint.
    pub destination: String,
    /// The `sha256:` approval-bound identity.
    pub identity: String,
}

/// One entry of a cache promotion manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheManifestEntry {
    /// The cache-relative path of the promoted file.
    pub path: String,
    /// The exact byte length of the promoted file.
    pub size: u64,
    /// The `sha256:` content checksum of the promoted file.
    pub checksum: String,
}

/// The mediated attempt-to-project cache promotion plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachePromotion {
    /// Whether promotion into the project cache is requested.
    pub enabled: bool,
    /// The absolute attempt-local promotion source.
    pub source: String,
    /// The absolute project-cache promotion destination.
    pub destination: String,
    /// The `sha256:` expected identity of the destination before promotion.
    pub expected_destination_identity: String,
    /// The exact files eligible for promotion.
    pub manifest: Vec<CacheManifestEntry>,
}

/// The mediated dependency-cache block of a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cache {
    /// The absolute `CARGO_HOME` root inside the sandbox.
    pub cargo_home: String,
    /// The absolute registry cache root inside the sandbox.
    pub registry: String,
    /// The absolute Git cache root inside the sandbox.
    pub git: String,
    /// The writable attempt-local cache endpoint, for acquisition phases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<CacheEndpoint>,
    /// The read-only approved cache endpoint, for verification phases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<CacheEndpoint>,
    /// The optional attempt-to-project promotion plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promotion: Option<CachePromotion>,
}

/// A writable, size-bounded scratch block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scratch {
    /// The absolute sandbox destination of the scratch root.
    pub destination: String,
    /// The `sha256:` approval-bound identity.
    pub identity: String,
}

/// The complete closed version-one execution request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxRequest {
    /// MUST equal [`REQUEST_PROTOCOL`].
    pub protocol: String,
    /// MUST equal [`PROTOCOL_VERSION`].
    pub protocol_version: u32,
    /// The authorized execution phase.
    pub phase: Phase,
    /// The exact program and argument vector.
    pub argv: Vec<String>,
    /// The absolute normalized sandbox working directory.
    pub working_directory: String,
    /// The exact host-supplied environment.
    pub environment: BTreeMap<String, String>,
    /// The network capability profile.
    pub network: Network,
    /// The bounded resource limits.
    pub resources: Resources,
    /// The approval-bound identities.
    pub identities: Identities,
    /// The typed mount grants.
    pub grants: Vec<Grant>,
    /// The toolchain block.
    pub toolchain: Toolchain,
    /// The optional mediated dependency cache.
    pub cache: Option<Cache>,
    /// The optional writable scratch root.
    pub scratch: Option<Scratch>,
}

/// A reference to an installed executable and its digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutableReference {
    /// The absolute path of the executable.
    pub path: String,
    /// The `sha256:` content digest of the executable.
    pub digest: String,
}

/// The kernel namespace capabilities verified by a probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Namespaces {
    /// Mount namespace availability.
    pub mount: bool,
    /// Network namespace availability.
    pub network: bool,
    /// PID namespace availability.
    pub pid: bool,
    /// IPC namespace availability.
    pub ipc: bool,
    /// UTS namespace availability.
    pub uts: bool,
    /// User namespace availability.
    pub user: bool,
}

/// The full set of verified enforcement capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// The verified namespace capabilities.
    pub namespaces: Namespaces,
    /// Whether the runner can start a new session.
    pub new_session: bool,
    /// Whether the runner can request a parent-death signal.
    pub parent_death_signal: bool,
}

/// The complete closed version-one probe response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxProbe {
    /// MUST equal [`PROBE_PROTOCOL`].
    pub protocol: String,
    /// MUST equal [`PROTOCOL_VERSION`].
    pub protocol_version: u32,
    /// The installed runner identity.
    pub runner: ExecutableReference,
    /// The enforcement backend identity.
    pub backend: BackendReference,
    /// The verified enforcement capabilities.
    pub capabilities: Capabilities,
}

/// A probe reference to the backend executable and its kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendReference {
    /// The backend kind.
    pub kind: BackendKind,
    /// The absolute path of the backend executable.
    pub path: String,
    /// The `sha256:` content digest of the backend executable.
    pub digest: String,
}
