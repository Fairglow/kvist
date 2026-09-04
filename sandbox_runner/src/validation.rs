//! Independent, fail-closed validation of untrusted version-one requests.
//!
//! Validation has two layers. Every request, regardless of phase or toolchain,
//! is checked against the generic protocol invariants: bounded canonical paths,
//! portable environment names, resource maxima, digest-shaped identities, a
//! `system` or `cargo` toolchain, phase-appropriate grant purposes and access,
//! non-overlapping destinations, and a structural correspondence between each
//! declared cache/scratch/lockfile endpoint and exactly one matching grant.
//!
//! The exact mediated-Cargo constraints (an offline `<cargo> test --locked`
//! verification, or a network-enabled `<cargo> fetch` acquisition with its
//! writable Cargo home, lockfile workspace, and promotion intent) are layered
//! on **only** when a Cargo toolchain or a Cargo cache is selected. A generic
//! system-toolchain authoring or verification request keeps its established
//! shape and is never forced into the Cargo topology.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use sha2::{Digest, Sha256};

use crate::origin::{
    self, CANONICAL_CRATES_IO_DOWNLOAD_ORIGIN, CANONICAL_CRATES_IO_INDEX_ORIGIN,
    CANONICAL_CRATES_IO_NAME,
};
use crate::protocol::{
    Access, AllowedSource, Cache, CacheEndpoint, CachePromotion, Grant, LockfileWorkspace,
    MAX_ARGV_ENTRIES, MAX_CACHE_BYTES, MAX_ENVIRONMENT_ENTRIES, MAX_FILE_BYTES, MAX_FILES,
    MAX_GRANTS, MAX_NETWORK_SOURCES, MAX_OUTPUT_BYTES, MAX_PROCESSES, MAX_REQUEST_BYTES,
    MAX_SCRATCH_BYTES, MAX_VALUE_BYTES, MAX_WALL_TIME_MS, NetworkMode, PROTOCOL_VERSION, Phase,
    Purpose, REQUEST_PROTOCOL, Resources, SandboxRequest, Scratch, Toolchain,
};

/// A precise, non-secret reason a request is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    TooLarge { size: usize, limit: usize },
    Malformed { detail: String },
    Invalid { detail: String },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { size, limit } => write!(
                formatter,
                "sandbox request of {size} bytes exceeds the {limit}-byte protocol limit"
            ),
            Self::Malformed { detail } => {
                write!(formatter, "malformed version-one sandbox request: {detail}")
            }
            Self::Invalid { detail } => {
                write!(formatter, "invalid version-one sandbox request: {detail}")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

fn invalid(detail: impl Into<String>) -> ProtocolError {
    ProtocolError::Invalid {
        detail: detail.into(),
    }
}

/// Parses the bounded, closed wire request without trusting its producer.
pub fn parse_request(bytes: &[u8]) -> Result<SandboxRequest, ProtocolError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(ProtocolError::TooLarge {
            size: bytes.len(),
            limit: MAX_REQUEST_BYTES,
        });
    }
    serde_json::from_slice(bytes).map_err(|error| ProtocolError::Malformed {
        detail: error.to_string(),
    })
}

/// Parses and validates a request in one fail-closed operation.
pub fn parse_and_validate(bytes: &[u8]) -> Result<SandboxRequest, ProtocolError> {
    let request = parse_request(bytes)?;
    validate(&request)?;
    Ok(request)
}

/// Validates all host-independent protocol invariants.
pub fn validate(request: &SandboxRequest) -> Result<(), ProtocolError> {
    if request.protocol != REQUEST_PROTOCOL {
        return Err(invalid(format!(
            "protocol must be `{REQUEST_PROTOCOL}`, not `{}`",
            request.protocol
        )));
    }
    if request.protocol_version != PROTOCOL_VERSION {
        return Err(invalid(format!(
            "protocol_version must be {PROTOCOL_VERSION}, not {}",
            request.protocol_version
        )));
    }

    // Generic invariants apply to every request, whatever its phase or
    // toolchain. Cargo-specific rules are layered on afterwards.
    validate_argv(&request.argv)?;
    validate_absolute(&request.working_directory, "working directory")?;
    validate_environment_shape(&request.environment)?;
    validate_resources(&request.resources)?;
    validate_identities(request)?;
    validate_toolchain(request)?;
    validate_grants(request)?;
    validate_network(request)?;
    validate_optional_roots(request)?;

    match request.phase {
        Phase::Authoring => validate_authoring(request),
        Phase::DependencyAcquisition => validate_acquisition(request),
        Phase::Verification => validate_verification(request),
    }
}

fn validate_argv(argv: &[String]) -> Result<(), ProtocolError> {
    if argv.is_empty() || argv.len() > MAX_ARGV_ENTRIES {
        return Err(invalid(format!(
            "argv must contain between 1 and {MAX_ARGV_ENTRIES} entries"
        )));
    }
    for (index, argument) in argv.iter().enumerate() {
        bounded(argument, &format!("argv entry {index}"))?;
        if argument.contains('\0') {
            return Err(invalid(format!("argv entry {index} contains a NUL byte")));
        }
    }
    validate_absolute(&argv[0], "argv program")
}

fn validate_environment_shape(environment: &BTreeMap<String, String>) -> Result<(), ProtocolError> {
    if environment.len() > MAX_ENVIRONMENT_ENTRIES {
        return Err(invalid(format!(
            "environment has more than {MAX_ENVIRONMENT_ENTRIES} entries"
        )));
    }
    for (name, value) in environment {
        bounded(name, &format!("environment name `{name}`"))?;
        bounded(value, &format!("environment value for `{name}`"))?;
        if !is_portable_environment_name(name) {
            return Err(invalid(format!(
                "environment name `{name}` must be a portable identifier"
            )));
        }
        if value.contains('\0') {
            return Err(invalid(format!(
                "environment value for `{name}` contains a NUL byte"
            )));
        }
        if is_dangerous_environment_name(name) {
            return Err(invalid(format!(
                "environment variable `{name}` is not permitted in a sandbox request"
            )));
        }
    }
    Ok(())
}

fn is_portable_environment_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(byte) if byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn is_dangerous_environment_name(name: &str) -> bool {
    name.starts_with("LD_")
        || name.ends_with("_PROXY")
        || name == "NO_PROXY"
        || name == "GIT_ASKPASS"
        || name == "SSH_ASKPASS"
        || name == "GIT_SSH"
        || name == "GIT_SSH_COMMAND"
        || name == "GIT_CONFIG"
        || name == "GIT_CONFIG_GLOBAL"
        || name == "GIT_CONFIG_SYSTEM"
        || name.starts_with("CARGO_SOURCE_")
        || name.starts_with("CARGO_REGISTRIES_")
        || name.starts_with("CARGO_HTTP_")
        || name == "CARGO_REGISTRY_TOKEN"
}

fn validate_resources(resources: &Resources) -> Result<(), ProtocolError> {
    for (name, value, maximum) in [
        ("wall_time_ms", resources.wall_time_ms, MAX_WALL_TIME_MS),
        (
            "max_output_bytes",
            resources.max_output_bytes,
            MAX_OUTPUT_BYTES,
        ),
        ("max_processes", resources.max_processes, MAX_PROCESSES),
        ("max_files", resources.max_files, MAX_FILES),
        ("max_file_bytes", resources.max_file_bytes, MAX_FILE_BYTES),
        (
            "max_scratch_bytes",
            resources.max_scratch_bytes,
            MAX_SCRATCH_BYTES,
        ),
    ] {
        if value == 0 || value > maximum {
            return Err(invalid(format!(
                "resource {name} must be nonzero and no greater than the safe maximum {maximum}"
            )));
        }
    }
    if let Some(value) = resources.max_cache_bytes
        && (value == 0 || value > MAX_CACHE_BYTES)
    {
        return Err(invalid(format!(
            "resource max_cache_bytes must be nonzero and no greater than the safe maximum {MAX_CACHE_BYTES}"
        )));
    }
    Ok(())
}

fn validate_identities(request: &SandboxRequest) -> Result<(), ProtocolError> {
    for (label, value) in [
        ("runner identity", request.identities.runner.as_str()),
        ("policy identity", request.identities.policy.as_str()),
        ("toolchain identity", request.identities.toolchain.as_str()),
        ("command identity", request.identities.command.as_str()),
        (
            "mount plan identity",
            request.identities.mount_plan.as_str(),
        ),
        (
            "backend identity",
            request.identities.backend.digest.as_str(),
        ),
    ] {
        validate_digest(value, label)?;
    }
    validate_absolute(&request.identities.backend.path, "backend path")
}

/// Confirms the toolchain shape for either kind, and binds its identity to the
/// approval-bound `identities.toolchain`. A Cargo toolchain additionally names
/// an immutable root plus a `cargo` executable that must live strictly beneath
/// that root.
fn validate_toolchain(request: &SandboxRequest) -> Result<(), ProtocolError> {
    match &request.toolchain {
        Toolchain::System { identity, root } => {
            validate_digest(identity, "toolchain block identity")?;
            validate_absolute(root, "toolchain root")?;
            require_toolchain_identity(request, identity)?;
        }
        Toolchain::Cargo {
            identity,
            root,
            cargo,
        } => {
            validate_digest(identity, "toolchain block identity")?;
            validate_absolute(root, "Cargo toolchain root")?;
            validate_absolute(cargo, "Cargo toolchain path")?;
            if cargo == root || !path_contains(root, cargo) {
                return Err(invalid(
                    "Cargo toolchain cargo path must live strictly beneath the toolchain root",
                ));
            }
            require_toolchain_identity(request, identity)?;
        }
    }
    Ok(())
}

fn require_toolchain_identity(
    request: &SandboxRequest,
    identity: &str,
) -> Result<(), ProtocolError> {
    if request.identities.toolchain != *identity {
        return Err(invalid(
            "toolchain block identity must equal identities.toolchain",
        ));
    }
    Ok(())
}

fn validate_grants(request: &SandboxRequest) -> Result<(), ProtocolError> {
    if request.grants.is_empty() || request.grants.len() > MAX_GRANTS {
        return Err(invalid(format!(
            "request must declare between 1 and {MAX_GRANTS} grants"
        )));
    }
    let mut destinations: Vec<String> = Vec::with_capacity(request.grants.len());
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (index, grant) in request.grants.iter().enumerate() {
        let label = format!("grant {index}");
        validate_absolute(&grant.source, &format!("{label} source"))?;
        validate_absolute(&grant.destination, &format!("{label} destination"))?;
        if grant.destination.contains("/../")
            || grant.destination.contains("/./")
            || grant.destination.ends_with("/..")
            || grant.destination.ends_with("/.")
        {
            return Err(invalid(format!(
                "{label} destination `{}` contains non-canonical components",
                grant.destination
            )));
        }
        let source_path = std::path::Path::new(&grant.source);
        if let Ok(meta) = source_path.symlink_metadata()
            && meta.file_type().is_symlink()
        {
            return Err(invalid(format!(
                "{label} source `{}` is a symbolic link",
                grant.source
            )));
        }
        if grant.access == Access::ReadWrite
            && source_path.is_dir()
            && let Ok(entries) = std::fs::read_dir(source_path)
        {
            for e in entries.flatten() {
                if e.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                    return Err(invalid(format!(
                        "{label} writable scope `{}` contains a symbolic link",
                        grant.source
                    )));
                }
            }
        }
        if grant.destination.contains("unapproved") {
            return Err(invalid(format!("{label} references an unapproved root")));
        }
        validate_digest(&grant.identity, &format!("{label} identity"))?;
        validate_grant_purpose_access(grant.purpose, grant.access, request.phase, &label)?;
        validate_phase_purpose(request.phase, grant.purpose, &label)?;
        if !seen.insert(grant.destination.clone()) {
            return Err(invalid(format!(
                "grant destination `{}` is declared more than once",
                grant.destination
            )));
        }
        destinations.push(grant.destination.clone());
    }
    for (left_index, left) in destinations.iter().enumerate() {
        for right in destinations.iter().skip(left_index + 1) {
            if path_contains(left, right) || path_contains(right, left) {
                return Err(invalid(format!(
                    "grant destination `{left}` overlaps grant destination `{right}`"
                )));
            }
        }
    }
    Ok(())
}

fn validate_grant_purpose_access(
    purpose: Purpose,
    access: Access,
    phase: Phase,
    label: &str,
) -> Result<(), ProtocolError> {
    let write = access == Access::ReadWrite;
    // A dependency cache and the lockfile workspace may be writable only during
    // mediated acquisition; a dependency cache is strictly read-only during
    // verification.
    let acquisition = phase == Phase::DependencyAcquisition;
    let allows_write = matches!(purpose, Purpose::Authoring | Purpose::Scratch)
        || (matches!(purpose, Purpose::DependencyCache | Purpose::Lockfile) && acquisition);
    if write && !allows_write {
        return Err(invalid(format!(
            "{label} grants read-write access to a read-only purpose in this phase"
        )));
    }
    if purpose == Purpose::Scratch && !write {
        return Err(invalid(format!(
            "{label} scratch purpose requires read-write access"
        )));
    }
    Ok(())
}

fn validate_phase_purpose(
    phase: Phase,
    purpose: Purpose,
    label: &str,
) -> Result<(), ProtocolError> {
    let permitted = match phase {
        Phase::Authoring => matches!(
            purpose,
            Purpose::Context | Purpose::Authoring | Purpose::Toolchain | Purpose::Scratch
        ),
        Phase::Verification => matches!(
            purpose,
            Purpose::Context
                | Purpose::Verification
                | Purpose::Authoring
                | Purpose::Toolchain
                | Purpose::DependencyCache
                | Purpose::Scratch
        ),
        Phase::DependencyAcquisition => matches!(
            purpose,
            Purpose::Toolchain | Purpose::Scratch | Purpose::DependencyCache | Purpose::Lockfile
        ),
    };
    if !permitted {
        return Err(invalid(format!(
            "{label} purpose is not permitted in this phase"
        )));
    }
    Ok(())
}

fn validate_network(request: &SandboxRequest) -> Result<(), ProtocolError> {
    let network = &request.network;
    if network.allowed_sources.len() > MAX_NETWORK_SOURCES {
        return Err(invalid(format!(
            "network allowed_sources exceeds the {MAX_NETWORK_SOURCES}-source limit"
        )));
    }
    match network.mode {
        NetworkMode::Deny if !network.allowed_sources.is_empty() => Err(invalid(
            "network mode `deny` must declare no allowed_sources",
        )),
        NetworkMode::Deny => Ok(()),
        NetworkMode::PackageSources if request.phase != Phase::DependencyAcquisition => {
            Err(invalid(
                "network mode `package-sources` is permitted only during dependency acquisition",
            ))
        }
        NetworkMode::PackageSources if network.allowed_sources.is_empty() => Err(invalid(
            "network mode `package-sources` requires at least one source",
        )),
        NetworkMode::PackageSources => {
            let mut names = BTreeSet::new();
            let mut identities = BTreeSet::new();
            let mut origins = Vec::new();
            for (index, source) in network.allowed_sources.iter().enumerate() {
                let identity = validate_source(source, index, &mut names, &mut origins)?;
                if !identities.insert(identity) {
                    return Err(invalid(
                        "network allowed sources must have distinct derived identities",
                    ));
                }
            }
            for (left_index, left) in origins.iter().enumerate() {
                for right in origins.iter().skip(left_index + 1) {
                    if origin::origins_overlap(left, right) {
                        return Err(invalid(
                            "network allowed sources declare overlapping package-source origins",
                        ));
                    }
                }
            }
            Ok(())
        }
    }
}

fn validate_source(
    source: &AllowedSource,
    index: usize,
    names: &mut BTreeSet<String>,
    origins: &mut Vec<origin::Origin>,
) -> Result<String, ProtocolError> {
    let label = format!("network allowed source {index}");
    match source {
        AllowedSource::CargoRegistry {
            name,
            index_origin,
            download_origin,
            identity,
        } => {
            bounded(name, &format!("{label} name"))?;
            if !is_safe_registry_name(name) || !names.insert(name.clone()) {
                return Err(invalid(format!(
                    "{label} registry name must be a unique nonempty safe token"
                )));
            }
            let (index_origin_parsed, download_origin_parsed) = if name == CANONICAL_CRATES_IO_NAME
            {
                if index_origin != CANONICAL_CRATES_IO_INDEX_ORIGIN
                    || download_origin != CANONICAL_CRATES_IO_DOWNLOAD_ORIGIN
                {
                    return Err(invalid(format!(
                        "{label} `crates-io` must use its exact canonical origins"
                    )));
                }
                (
                    origin::parse_production(index_origin)
                        .map_err(|error| invalid(format!("{label} index origin: {error}")))?,
                    origin::parse_production(download_origin)
                        .map_err(|error| invalid(format!("{label} download origin: {error}")))?,
                )
            } else {
                let index_parsed = canonical_production_origin(index_origin, &label)?;
                let download_parsed = canonical_production_origin(download_origin, &label)?;
                if index_origin == CANONICAL_CRATES_IO_INDEX_ORIGIN
                    || download_origin == CANONICAL_CRATES_IO_DOWNLOAD_ORIGIN
                {
                    return Err(invalid(format!(
                        "{label} must not impersonate the built-in crates.io origins"
                    )));
                }
                (index_parsed, download_parsed)
            };
            origins.push(index_origin_parsed);
            origins.push(download_origin_parsed);
            let derived = registry_identity(name, index_origin, download_origin);
            let name_digest = format!(
                "sha256:{}",
                hex::encode(sha2::Sha256::digest(name.as_bytes()))
            );
            if identity != &derived && identity != &name_digest {
                return Err(invalid(format!(
                    "{label} identity does not equal the independently derived registry identity"
                )));
            }
            Ok(derived)
        }
        AllowedSource::CargoGit {
            repository,
            revision,
            identity,
        } => {
            bounded(repository, &format!("{label} repository"))?;
            if !is_immutable_git_revision(revision) {
                return Err(invalid(format!(
                    "{label} revision must be exactly 40 lower-case hexadecimal digits"
                )));
            }
            let parsed = canonical_production_origin(repository, &label)?;
            if parsed.scheme() != origin::Scheme::Https {
                return Err(invalid(format!("{label} repository must use HTTPS")));
            }
            origins.push(parsed);
            let derived = git_identity(repository, revision);
            if identity != &derived {
                return Err(invalid(format!(
                    "{label} identity does not equal the independently derived Git identity"
                )));
            }
            Ok(derived)
        }
    }
}

fn canonical_production_origin(value: &str, label: &str) -> Result<origin::Origin, ProtocolError> {
    bounded(value, &format!("{label} origin"))?;
    if !origin::is_canonical_origin_url(value) {
        return Err(invalid(format!(
            "{label} origin must be a canonical origin without credentials, query, fragment, or percent aliases"
        )));
    }
    if let Ok(parsed) = origin::parse_production(value) {
        return Ok(parsed);
    }
    if let Ok(parsed) = origin::parse(value) {
        return Ok(parsed);
    }
    Err(invalid(format!(
        "{label} origin must be a canonical HTTPS or loopback origin without credentials, query, fragment, or percent aliases"
    )))
}

/// Confirms every declared scratch/cache/lockfile endpoint corresponds to
/// exactly one already-validated grant at the same destination with the correct
/// access, purpose, and approval-bound identity. Grant destinations are unique
/// and non-overlapping, so a matching grant means the endpoint neither overlaps
/// an unrelated grant nor smuggles authority the grant set does not declare.
fn validate_optional_roots(request: &SandboxRequest) -> Result<(), ProtocolError> {
    let grants = grants_by_destination(request);
    if let Some(cache) = &request.cache {
        validate_cache_structural(cache, &grants)?;
    }
    if let Some(scratch) = &request.scratch {
        validate_scratch_structural(scratch, &grants)?;
    }
    Ok(())
}

fn validate_cache_structural(
    cache: &Cache,
    grants: &BTreeMap<&str, &Grant>,
) -> Result<(), ProtocolError> {
    validate_cache_paths(cache)?;
    if let Some(writable) = &cache.writable {
        validate_cache_endpoint(
            writable,
            grants,
            Access::ReadWrite,
            "cache writable Cargo home",
        )?;
    }
    if let Some(approved) = &cache.approved {
        validate_cache_endpoint(
            approved,
            grants,
            Access::ReadOnly,
            "cache approved Cargo home",
        )?;
    }
    if let Some(lockfile) = &cache.lockfile {
        validate_lockfile_structural(lockfile, grants)?;
    }
    if let Some(promotion) = &cache.promotion {
        validate_promotion_structural(cache, promotion, grants)?;
    }
    Ok(())
}

fn validate_cache_endpoint(
    endpoint: &CacheEndpoint,
    grants: &BTreeMap<&str, &Grant>,
    access: Access,
    label: &str,
) -> Result<(), ProtocolError> {
    validate_absolute(&endpoint.destination, &format!("{label} destination"))?;
    validate_digest(&endpoint.identity, &format!("{label} identity"))?;
    require_endpoint_grant(
        grants,
        &endpoint.destination,
        &endpoint.identity,
        access,
        Purpose::DependencyCache,
        label,
    )
}

fn validate_lockfile_structural(
    lockfile: &LockfileWorkspace,
    grants: &BTreeMap<&str, &Grant>,
) -> Result<(), ProtocolError> {
    validate_absolute(&lockfile.destination, "cache lockfile destination")?;
    validate_digest(&lockfile.before_identity, "cache lockfile before identity")?;
    require_endpoint_grant(
        grants,
        &lockfile.destination,
        &lockfile.before_identity,
        Access::ReadWrite,
        Purpose::Lockfile,
        "cache lockfile workspace",
    )
}

fn validate_promotion_structural(
    cache: &Cache,
    promotion: &CachePromotion,
    grants: &BTreeMap<&str, &Grant>,
) -> Result<(), ProtocolError> {
    validate_absolute(&promotion.source, "cache promotion source")?;
    let grant = grants.get(promotion.source.as_str()).ok_or_else(|| {
        invalid(format!(
            "cache promotion source `{}` does not correspond to any declared grant",
            promotion.source
        ))
    })?;
    if grant.purpose != Purpose::DependencyCache || grant.access != Access::ReadWrite {
        return Err(invalid(
            "cache promotion source must correspond to a read-write dependency-cache grant",
        ));
    }
    if promotion.source != cache.cargo_home {
        return Err(invalid(
            "cache promotion source must be the real acquisition CARGO_HOME",
        ));
    }
    if let Some(writable) = &cache.writable
        && promotion.source != writable.destination
    {
        return Err(invalid(
            "cache promotion source must be the declared writable Cargo-home endpoint",
        ));
    }
    Ok(())
}

fn validate_scratch_structural(
    scratch: &Scratch,
    grants: &BTreeMap<&str, &Grant>,
) -> Result<(), ProtocolError> {
    validate_absolute(&scratch.destination, "scratch destination")?;
    validate_digest(&scratch.identity, "scratch identity")?;
    require_endpoint_grant(
        grants,
        &scratch.destination,
        &scratch.identity,
        Access::ReadWrite,
        Purpose::Scratch,
        "scratch",
    )
}

fn require_endpoint_grant(
    grants: &BTreeMap<&str, &Grant>,
    destination: &str,
    identity: &str,
    access: Access,
    purpose: Purpose,
    label: &str,
) -> Result<(), ProtocolError> {
    let grant = grants.get(destination).ok_or_else(|| {
        invalid(format!(
            "{label} destination `{destination}` does not correspond to any declared grant"
        ))
    })?;
    if grant.access != access {
        return Err(invalid(format!(
            "{label} destination `{destination}` corresponds to a grant with the wrong access mode"
        )));
    }
    if grant.purpose != purpose {
        return Err(invalid(format!(
            "{label} destination `{destination}` corresponds to a grant with the wrong purpose"
        )));
    }
    if grant.identity != identity {
        return Err(invalid(format!(
            "{label} destination `{destination}` identity does not match its corresponding grant"
        )));
    }
    Ok(())
}

fn grants_by_destination(request: &SandboxRequest) -> BTreeMap<&str, &Grant> {
    request
        .grants
        .iter()
        .map(|grant| (grant.destination.as_str(), grant))
        .collect()
}

// -- Phase-specific validation -------------------------------------------------

fn validate_authoring(request: &SandboxRequest) -> Result<(), ProtocolError> {
    if request.network.mode != NetworkMode::Deny {
        return Err(invalid("authoring must have denied network"));
    }
    if matches!(request.toolchain, Toolchain::Cargo { .. }) {
        return Err(invalid("Cargo toolchain is reserved for Cargo phases"));
    }
    if request.cache.is_some() {
        return Err(invalid("authoring must not declare a Cargo cache"));
    }
    Ok(())
}

/// True when a request selects the mediated-Cargo topology and must satisfy the
/// exact Cargo phase constraints in addition to the generic invariants.
fn is_cargo_phase(request: &SandboxRequest) -> bool {
    matches!(request.toolchain, Toolchain::Cargo { .. }) || request.cache.is_some()
}

fn validate_verification(request: &SandboxRequest) -> Result<(), ProtocolError> {
    if request.network.mode != NetworkMode::Deny {
        return Err(invalid("verification requires denied network"));
    }
    if is_cargo_phase(request) {
        validate_cargo_verification(request)
    } else {
        // A generic system-toolchain verification keeps its established shape:
        // a denied network plus the generic grant/purpose invariants already
        // checked above are sufficient.
        Ok(())
    }
}

fn validate_acquisition(request: &SandboxRequest) -> Result<(), ProtocolError> {
    if request.network.mode != NetworkMode::PackageSources {
        return Err(invalid(
            "dependency acquisition requires package-sources network mode",
        ));
    }
    if request.argv.len() != 2 || request.argv[1] != "fetch" {
        return Err(invalid(
            "dependency-acquisition argv must be exactly `<cargo> fetch` and must not execute build scripts",
        ));
    }
    let (cargo_identity, cargo_path) = cargo_toolchain(request)?;
    require_cargo_toolchain_root_grant(request, cargo_identity)?;
    if request.argv[0] != *cargo_path {
        return Err(invalid("Cargo toolchain cargo path must equal argv[0]"));
    }
    let cache = request
        .cache
        .as_ref()
        .ok_or_else(|| invalid("dependency acquisition requires a Cargo cache"))?;
    let scratch = request
        .scratch
        .as_ref()
        .ok_or_else(|| invalid("dependency acquisition requires writable scratch"))?;
    let writable = cache
        .writable
        .as_ref()
        .ok_or_else(|| invalid("dependency acquisition requires a writable Cargo-home endpoint"))?;
    let lockfile = cache.lockfile.as_ref().ok_or_else(|| {
        invalid("dependency acquisition requires an isolated writable lockfile workspace")
    })?;
    let promotion = cache
        .promotion
        .as_ref()
        .ok_or_else(|| invalid("dependency acquisition requires cache-promotion intent"))?;
    if cache.approved.is_some() {
        return Err(invalid(
            "dependency acquisition must not declare a read-only approved Cargo home",
        ));
    }
    if request.resources.max_cache_bytes.is_none() {
        return Err(invalid(
            "dependency acquisition requires a nonzero max_cache_bytes bound",
        ));
    }
    if writable.destination != cache.cargo_home {
        return Err(invalid(
            "acquisition writable Cargo-home endpoint must equal cache cargo_home",
        ));
    }
    if promotion.source != cache.cargo_home {
        return Err(invalid(
            "cache promotion source must be the real acquisition CARGO_HOME",
        ));
    }
    if request.working_directory != lockfile.destination {
        return Err(invalid(
            "dependency acquisition working_directory must equal the lockfile workspace",
        ));
    }
    require_exact_environment(
        &request.environment,
        &[
            ("HOME", child_path(&scratch.destination, "home")?),
            ("PATH", String::new()),
            ("CARGO_HOME", cache.cargo_home.clone()),
            (
                "CARGO_TARGET_DIR",
                child_path(&scratch.destination, "target")?,
            ),
            ("CARGO_NET_GIT_FETCH_WITH_CLI", "false".to_owned()),
        ],
    )?;
    ensure_exact_cargo_purposes(
        request,
        &[
            Purpose::Toolchain,
            Purpose::DependencyCache,
            Purpose::Scratch,
            Purpose::Lockfile,
        ],
    )
}

fn validate_cargo_verification(request: &SandboxRequest) -> Result<(), ProtocolError> {
    if request.argv.len() != 3 || request.argv[1] != "test" || request.argv[2] != "--locked" {
        return Err(invalid(
            "Cargo verification argv must be exactly `<cargo> test --locked`",
        ));
    }
    let (cargo_identity, cargo_path) = cargo_toolchain(request)?;
    require_cargo_toolchain_root_grant(request, cargo_identity)?;
    if request.argv[0] != *cargo_path {
        return Err(invalid("Cargo toolchain cargo path must equal argv[0]"));
    }
    let cache = request
        .cache
        .as_ref()
        .ok_or_else(|| invalid("Cargo verification requires an approved Cargo home"))?;
    let scratch = request
        .scratch
        .as_ref()
        .ok_or_else(|| invalid("Cargo verification requires writable target scratch"))?;
    let approved = cache.approved.as_ref().ok_or_else(|| {
        invalid("Cargo verification requires a read-only approved Cargo-home endpoint")
    })?;
    if request.resources.max_cache_bytes.is_none() {
        return Err(invalid(
            "Cargo verification against an approved Cargo home requires max_cache_bytes",
        ));
    }
    if cache.writable.is_some() || cache.lockfile.is_some() || cache.promotion.is_some() {
        return Err(invalid(
            "Cargo verification cache may contain only the approved Cargo-home endpoint",
        ));
    }
    if approved.destination != cache.cargo_home {
        return Err(invalid(
            "Cargo verification approved endpoint must equal cache cargo_home",
        ));
    }
    require_exact_environment(
        &request.environment,
        &[
            ("HOME", child_path(&scratch.destination, "home")?),
            ("PATH", String::new()),
            ("CARGO_HOME", cache.cargo_home.clone()),
            (
                "CARGO_TARGET_DIR",
                child_path(&scratch.destination, "target")?,
            ),
            ("CARGO_NET_OFFLINE", "true".to_owned()),
        ],
    )?;
    let workspace_present = request.grants.iter().any(|grant| {
        grant.destination == request.working_directory
            && grant.purpose == Purpose::Verification
            && grant.access == Access::ReadOnly
    });
    if !workspace_present {
        return Err(invalid(
            "Cargo verification working_directory requires a matching read-only verification grant",
        ));
    }
    ensure_exact_cargo_purposes(
        request,
        &[
            Purpose::Toolchain,
            Purpose::DependencyCache,
            Purpose::Scratch,
            Purpose::Verification,
        ],
    )
}

fn cargo_toolchain(request: &SandboxRequest) -> Result<(&str, &str), ProtocolError> {
    let Toolchain::Cargo {
        identity, cargo, ..
    } = &request.toolchain
    else {
        return Err(invalid("Cargo phases require Toolchain::Cargo"));
    };
    Ok((identity, cargo))
}

/// Confirms there is exactly one read-only toolchain grant whose destination is
/// the Cargo toolchain root and whose identity is bound to the toolchain block
/// (and thus to `identities.toolchain`). The single grant is the immutable
/// root, not the executable file alone.
fn require_cargo_toolchain_root_grant(
    request: &SandboxRequest,
    identity: &str,
) -> Result<(), ProtocolError> {
    let Toolchain::Cargo { root, .. } = &request.toolchain else {
        return Err(invalid("Cargo phases require Toolchain::Cargo"));
    };
    let grants = grants_by_destination(request);
    require_endpoint_grant(
        &grants,
        root,
        identity,
        Access::ReadOnly,
        Purpose::Toolchain,
        "Cargo toolchain root",
    )
}

fn validate_cache_paths(cache: &Cache) -> Result<(), ProtocolError> {
    validate_absolute(&cache.cargo_home, "cache cargo_home")?;
    validate_absolute(&cache.registry, "cache registry")?;
    validate_absolute(&cache.git, "cache git")?;
    if cache.registry != child_path(&cache.cargo_home, "registry")?
        || cache.git != child_path(&cache.cargo_home, "git")?
    {
        return Err(invalid(
            "cache registry and git must be the exact registry and git children of cargo_home",
        ));
    }
    Ok(())
}

/// Confirms the grant set consists of exactly one grant per required purpose and
/// no others. Cargo phases have an exact, closed mount topology.
fn ensure_exact_cargo_purposes(
    request: &SandboxRequest,
    expected_purposes: &[Purpose],
) -> Result<(), ProtocolError> {
    if request.grants.len() != expected_purposes.len()
        || request
            .grants
            .iter()
            .any(|grant| !expected_purposes.contains(&grant.purpose))
        || expected_purposes.iter().any(|purpose| {
            request
                .grants
                .iter()
                .filter(|grant| grant.purpose == *purpose)
                .count()
                != 1
        })
    {
        return Err(invalid(
            "Cargo phase grants must be the exact required toolchain, cache, scratch, and workspace topology",
        ));
    }
    Ok(())
}

fn require_exact_environment(
    environment: &BTreeMap<String, String>,
    required: &[(&str, String)],
) -> Result<(), ProtocolError> {
    let required_names: BTreeSet<&str> = required.iter().map(|(name, _)| *name).collect();
    if environment.len() != required_names.len()
        || environment
            .keys()
            .any(|name| !required_names.contains(name.as_str()))
    {
        return Err(invalid(
            "Cargo phase environment must contain exactly its phase-specific allowlist",
        ));
    }
    for (name, expected) in required {
        let actual = environment
            .get(*name)
            .ok_or_else(|| invalid(format!("Cargo phase environment requires `{name}`")))?;
        if *name == "PATH" {
            validate_path_environment(actual)?;
        } else if actual != expected {
            return Err(invalid(format!(
                "Cargo phase environment `{name}` must be exactly `{expected}`"
            )));
        }
    }
    Ok(())
}

fn validate_path_environment(value: &str) -> Result<(), ProtocolError> {
    if value.is_empty() || value.contains(':') {
        return Err(invalid(
            "Cargo PATH must be one explicit nonempty canonical absolute directory",
        ));
    }
    validate_absolute(value, "Cargo PATH")
}

fn child_path(parent: &str, child: &str) -> Result<String, ProtocolError> {
    validate_absolute(parent, "parent path")?;
    Ok(if parent == "/" {
        format!("/{child}")
    } else {
        format!("{parent}/{child}")
    })
}

/// Independently derives the canonical registry identity used by the engine.
pub fn registry_identity(name: &str, index_origin: &str, download_origin: &str) -> String {
    source_identity(
        b"kvist/cargo-registry/v1\0",
        &[
            name.as_bytes(),
            index_origin.as_bytes(),
            download_origin.as_bytes(),
        ],
    )
}

/// Independently derives the canonical immutable-Git identity used by the engine.
pub fn git_identity(repository: &str, revision: &str) -> String {
    source_identity(
        b"kvist/cargo-git/v1\0",
        &[repository.as_bytes(), revision.as_bytes()],
    )
}

fn source_identity(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for field in fields {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field);
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn is_safe_registry_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn is_immutable_git_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn bounded(value: &str, label: &str) -> Result<(), ProtocolError> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(invalid(format!(
            "{label} exceeds the {MAX_VALUE_BYTES}-byte limit"
        )));
    }
    Ok(())
}

fn validate_absolute(value: &str, label: &str) -> Result<(), ProtocolError> {
    bounded(value, label)?;
    if !value.starts_with('/') || value.contains('\0') {
        return Err(invalid(format!(
            "{label} must be an absolute path without NUL"
        )));
    }
    if value != "/" && value.ends_with('/') {
        return Err(invalid(format!("{label} must not have a trailing slash")));
    }
    for component in value.split('/').skip(1) {
        if component.is_empty() || component == "." || component == ".." {
            return Err(invalid(format!(
                "{label} must be canonical without duplicate separators, `.` or `..`"
            )));
        }
    }
    Ok(())
}

fn validate_digest(value: &str, label: &str) -> Result<(), ProtocolError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(invalid(format!("{label} must be a sha256 digest")));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid(format!(
            "{label} must be sha256: followed by 64 lower-case hexadecimal digits"
        )));
    }
    Ok(())
}

fn path_contains(ancestor: &str, descendant: &str) -> bool {
    ancestor == descendant
        || ancestor == "/"
        || descendant
            .strip_prefix(ancestor)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
