//! Independent, fail-closed validation of untrusted version-one requests.
//!
//! Parsing enforces a hard size bound and the closed JSON shape. Structural
//! validation then confirms the exact protocol identity, typed phase, bounded
//! argument vector, absolute normalized working directory, digest formats, and
//! non-overlapping normalized destinations without trusting any producer.
//!
//! Filesystem-dependent enforcement (symlink resolution of host sources,
//! canonicalization, and Bubblewrap mount construction) is a separate,
//! later-integrated concern; this module performs only host-independent
//! structural and lexical checks so it can be exercised deterministically.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::protocol::{
    Access, AllowedSource, Cache, Grant, MAX_ARGV_ENTRIES, MAX_CACHE_BYTES,
    MAX_CACHE_MANIFEST_ENTRIES, MAX_ENVIRONMENT_ENTRIES, MAX_FILE_BYTES, MAX_FILES, MAX_GRANTS,
    MAX_NETWORK_SOURCES, MAX_OUTPUT_BYTES, MAX_PROCESSES, MAX_REQUEST_BYTES, MAX_SCRATCH_BYTES,
    MAX_VALUE_BYTES, MAX_WALL_TIME_MS, Network, NetworkMode, PROTOCOL_VERSION, Phase, Purpose,
    REQUEST_PROTOCOL, Resources, SandboxRequest, Scratch, Toolchain,
};

/// A precise, actionable reason an untrusted request was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// The raw request exceeded the accepted size bound before parsing.
    TooLarge {
        /// Observed size in bytes.
        size: usize,
        /// Maximum accepted size in bytes.
        limit: usize,
    },
    /// The request was not the closed version-one JSON shape.
    ///
    /// This includes an unknown field, a missing field, a wrong type, or a
    /// superseded ("legacy") request shape, each reported by the strict parser.
    Malformed {
        /// The underlying parser diagnostic.
        detail: String,
    },
    /// A structural or lexical constraint was violated.
    Invalid {
        /// A specific, non-secret explanation.
        detail: String,
    },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::TooLarge { size, limit } => write!(
                formatter,
                "sandbox request of {size} bytes exceeds the {limit}-byte protocol limit"
            ),
            ProtocolError::Malformed { detail } => {
                write!(formatter, "malformed version-one sandbox request: {detail}")
            }
            ProtocolError::Invalid { detail } => {
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

/// Parses untrusted request bytes into the closed version-one request.
///
/// The size bound is applied before parsing so a hostile producer cannot
/// exhaust memory. The strict parser rejects unknown fields, missing fields,
/// wrong types, and every superseded request shape.
pub fn parse_request(bytes: &[u8]) -> Result<SandboxRequest, ProtocolError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(ProtocolError::TooLarge {
            size: bytes.len(),
            limit: MAX_REQUEST_BYTES,
        });
    }
    serde_json::from_slice::<SandboxRequest>(bytes).map_err(|error| ProtocolError::Malformed {
        detail: error.to_string(),
    })
}

/// Parses and fully validates an untrusted request in one fail-closed step.
pub fn parse_and_validate(bytes: &[u8]) -> Result<SandboxRequest, ProtocolError> {
    let request = parse_request(bytes)?;
    validate(&request)?;
    Ok(request)
}

/// Confirms every host-independent structural and lexical invariant.
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

    validate_argv(&request.argv)?;
    validate_absolute_normalized(&request.working_directory, "working directory")?;
    validate_environment(request)?;
    validate_network(request)?;
    validate_resources(&request.resources)?;
    validate_identities(request)?;
    validate_toolchain(request)?;
    validate_grants(request)?;
    validate_optional_roots(request)?;

    Ok(())
}

fn validate_argv(argv: &[String]) -> Result<(), ProtocolError> {
    if argv.is_empty() {
        return Err(invalid("argv must contain at least the program path"));
    }
    if argv.len() > MAX_ARGV_ENTRIES {
        return Err(invalid(format!(
            "argv has {} entries, exceeding the {MAX_ARGV_ENTRIES}-entry limit",
            argv.len()
        )));
    }
    for (index, entry) in argv.iter().enumerate() {
        bounded_value(entry, &format!("argv entry {index}"))?;
        if entry.contains('\0') {
            return Err(invalid(format!(
                "argv entry {index} contains an interior NUL byte"
            )));
        }
    }
    let program = &argv[0];
    validate_canonical_absolute(program, "argv program")?;
    Ok(())
}

fn validate_environment(request: &SandboxRequest) -> Result<(), ProtocolError> {
    if request.environment.len() > MAX_ENVIRONMENT_ENTRIES {
        return Err(invalid(format!(
            "environment has {} entries, exceeding the {MAX_ENVIRONMENT_ENTRIES}-entry limit",
            request.environment.len()
        )));
    }
    for (name, value) in &request.environment {
        if name.is_empty() {
            return Err(invalid("environment names must not be empty"));
        }
        bounded_value(name, &format!("environment name `{name}`"))?;
        if name.contains('=') || name.contains('\0') {
            return Err(invalid(format!(
                "environment name `{name}` must not contain `=` or NUL"
            )));
        }
        if !name
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
            || name.as_bytes()[0].is_ascii_digit()
        {
            return Err(invalid(format!(
                "environment name `{name}` must be a portable identifier"
            )));
        }
        bounded_value(value, &format!("environment value for `{name}`"))?;
        if value.contains('\0') {
            return Err(invalid(format!(
                "environment value for `{name}` contains a NUL byte"
            )));
        }
    }
    Ok(())
}

fn validate_network(request: &SandboxRequest) -> Result<(), ProtocolError> {
    let network: &Network = &request.network;
    if network.allowed_sources.len() > MAX_NETWORK_SOURCES {
        return Err(invalid(format!(
            "network allowed_sources has {} entries, exceeding the {MAX_NETWORK_SOURCES}-entry limit",
            network.allowed_sources.len()
        )));
    }
    for (index, source) in network.allowed_sources.iter().enumerate() {
        validate_allowed_source(source, index)?;
    }
    match network.mode {
        NetworkMode::Deny => {
            if !network.allowed_sources.is_empty() {
                return Err(invalid(
                    "network mode `deny` must declare an empty allowed_sources list",
                ));
            }
        }
        NetworkMode::PackageSources => {
            if request.phase != Phase::DependencyAcquisition {
                return Err(invalid(
                    "network mode `package-sources` is permitted only in the dependency-acquisition phase",
                ));
            }
            if network.allowed_sources.is_empty() {
                return Err(invalid(
                    "network mode `package-sources` must declare at least one allowed source",
                ));
            }
        }
    }
    Ok(())
}

/// Confirms the structural shape and bounds of one typed package source.
///
/// Semantic source policy (immutable Git pins, exact registry origins) is
/// enforced by the later mediated-acquisition integration; this revision only
/// bounds fields, validates the identity digest, and rejects unknown shapes.
fn validate_allowed_source(source: &AllowedSource, index: usize) -> Result<(), ProtocolError> {
    let label = format!("network allowed source {index}");
    match source {
        AllowedSource::CargoRegistry {
            name,
            index_origin,
            download_origin,
            identity,
        } => {
            bounded_value(name, &format!("{label} name"))?;
            bounded_value(index_origin, &format!("{label} index_origin"))?;
            if let Some(download_origin) = download_origin {
                bounded_value(download_origin, &format!("{label} download_origin"))?;
            }
            validate_digest(identity, &format!("{label} identity"))?;
        }
        AllowedSource::CargoGit {
            repository,
            branch,
            revision,
            identity,
        } => {
            if let Some(repository) = repository {
                bounded_value(repository, &format!("{label} repository"))?;
            }
            if let Some(branch) = branch {
                bounded_value(branch, &format!("{label} branch"))?;
            }
            if let Some(revision) = revision {
                bounded_value(revision, &format!("{label} revision"))?;
            }
            validate_digest(identity, &format!("{label} identity"))?;
        }
    }
    Ok(())
}

fn validate_resources(resources: &Resources) -> Result<(), ProtocolError> {
    let bounds = [
        (
            "resource wall_time_ms",
            resources.wall_time_ms,
            MAX_WALL_TIME_MS,
        ),
        (
            "resource max_output_bytes",
            resources.max_output_bytes,
            MAX_OUTPUT_BYTES,
        ),
        (
            "resource max_processes",
            resources.max_processes,
            MAX_PROCESSES,
        ),
        ("resource max_files", resources.max_files, MAX_FILES),
        (
            "resource max_file_bytes",
            resources.max_file_bytes,
            MAX_FILE_BYTES,
        ),
        (
            "resource max_scratch_bytes",
            resources.max_scratch_bytes,
            MAX_SCRATCH_BYTES,
        ),
    ];
    for (name, value, maximum) in bounds {
        if value == 0 {
            return Err(invalid(format!("{name} limit must be greater than zero")));
        }
        if value > maximum {
            return Err(invalid(format!(
                "{name} limit of {value} exceeds the safe maximum of {maximum}"
            )));
        }
    }
    if let Some(max_cache_bytes) = resources.max_cache_bytes {
        if max_cache_bytes == 0 {
            return Err(invalid(
                "resource max_cache_bytes limit must be greater than zero",
            ));
        }
        if max_cache_bytes > MAX_CACHE_BYTES {
            return Err(invalid(format!(
                "resource max_cache_bytes limit of {max_cache_bytes} exceeds the safe maximum of {MAX_CACHE_BYTES}"
            )));
        }
    }
    Ok(())
}

fn validate_identities(request: &SandboxRequest) -> Result<(), ProtocolError> {
    let identities = &request.identities;
    validate_digest(&identities.runner, "runner identity")?;
    validate_digest(&identities.policy, "policy identity")?;
    validate_digest(&identities.toolchain, "toolchain identity")?;
    validate_digest(&identities.command, "command identity")?;
    validate_digest(&identities.mount_plan, "mount_plan identity")?;
    validate_digest(&identities.backend.digest, "backend identity digest")?;
    validate_absolute_normalized(&identities.backend.path, "backend path")?;
    Ok(())
}

fn validate_toolchain(request: &SandboxRequest) -> Result<(), ProtocolError> {
    match &request.toolchain {
        Toolchain::System { identity, root } => {
            validate_digest(identity, "toolchain block identity")?;
            validate_absolute_normalized(root, "toolchain root")?;
        }
        Toolchain::Cargo { identity, cargo } => {
            validate_digest(identity, "toolchain block identity")?;
            validate_canonical_absolute(cargo, "toolchain cargo path")?;
        }
    }
    Ok(())
}

fn validate_grants(request: &SandboxRequest) -> Result<(), ProtocolError> {
    if request.grants.is_empty() {
        return Err(invalid("at least one grant is required"));
    }
    if request.grants.len() > MAX_GRANTS {
        return Err(invalid(format!(
            "request declares {} grants, exceeding the {MAX_GRANTS}-grant limit",
            request.grants.len()
        )));
    }

    let mut destinations: Vec<String> = Vec::with_capacity(request.grants.len());
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (index, grant) in request.grants.iter().enumerate() {
        let label = format!("grant {index}");
        bounded_value(&grant.source, &format!("{label} source"))?;
        validate_absolute_normalized(&grant.source, &format!("{label} source"))?;
        validate_absolute_normalized(&grant.destination, &format!("{label} destination"))?;
        validate_digest(&grant.identity, &format!("{label} identity"))?;
        validate_grant_purpose_access(grant.purpose, grant.access, request.phase, &label)?;
        validate_phase_purpose(request.phase, grant.purpose, &label)?;

        let normalized = normalize_absolute(&grant.destination)
            .ok_or_else(|| invalid(format!("{label} destination is not a normalized path")))?;
        if !seen.insert(normalized.clone()) {
            return Err(invalid(format!(
                "grant destination `{}` is declared more than once",
                grant.destination
            )));
        }
        destinations.push(normalized);
    }

    for (left_index, left) in destinations.iter().enumerate() {
        for (right_index, right) in destinations.iter().enumerate() {
            if left_index == right_index {
                continue;
            }
            if is_prefix_path(left, right) {
                return Err(invalid(format!(
                    "grant destination `{}` overlaps grant destination `{}`",
                    request.grants[left_index].destination, request.grants[right_index].destination
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
    let write = matches!(access, Access::ReadWrite);
    // A dependency cache may be writable only during mediated acquisition; it is
    // strictly read-only during verification.
    let dependency_cache_writable =
        matches!(purpose, Purpose::DependencyCache) && phase == Phase::DependencyAcquisition;
    let allows_write =
        matches!(purpose, Purpose::Authoring | Purpose::Scratch) || dependency_cache_writable;
    if write && !allows_write {
        return Err(invalid(format!(
            "{label} grants read-write access to a read-only purpose in this phase"
        )));
    }
    let requires_write = matches!(purpose, Purpose::Scratch);
    if requires_write && !write {
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
                | Purpose::Toolchain
                | Purpose::Scratch
                | Purpose::DependencyCache
        ),
        Phase::DependencyAcquisition => matches!(
            purpose,
            Purpose::Toolchain | Purpose::Scratch | Purpose::DependencyCache | Purpose::Context
        ),
    };
    if !permitted {
        return Err(invalid(format!(
            "{label} purpose is not permitted in this phase"
        )));
    }
    Ok(())
}

fn validate_optional_roots(request: &SandboxRequest) -> Result<(), ProtocolError> {
    // Every declared scratch/cache endpoint must correspond exactly to one
    // already-validated grant at the same normalized destination. Grant
    // destinations are unique and non-overlapping (enforced by
    // `validate_grants`), so a matching grant means the endpoint neither
    // overlaps an unrelated grant nor smuggles authority the grant set does not
    // already declare.
    let mut grants_by_destination: BTreeMap<String, &Grant> = BTreeMap::new();
    for grant in &request.grants {
        if let Some(normalized) = normalize_absolute(&grant.destination) {
            grants_by_destination.insert(normalized, grant);
        }
    }
    if let Some(cache) = &request.cache {
        validate_cache(cache, &grants_by_destination)?;
    }
    if let Some(scratch) = &request.scratch {
        validate_scratch(scratch, &grants_by_destination)?;
    }
    Ok(())
}

/// Confirms one declared endpoint corresponds exactly to a matching grant.
///
/// The grant at the endpoint's normalized destination must exist and declare
/// exactly the expected access mode, purpose, and the same approval-bound
/// identity. Anything else is a topology inconsistency the runner refuses.
fn require_endpoint_grant(
    grants: &BTreeMap<String, &Grant>,
    destination: &str,
    identity: &str,
    expected_access: Access,
    expected_purpose: Purpose,
    label: &str,
) -> Result<(), ProtocolError> {
    let normalized = normalize_absolute(destination)
        .ok_or_else(|| invalid(format!("{label} destination is not a normalized path")))?;
    let grant = grants.get(&normalized).ok_or_else(|| {
        invalid(format!(
            "{label} destination `{destination}` does not correspond to any declared grant"
        ))
    })?;
    if grant.access != expected_access {
        return Err(invalid(format!(
            "{label} destination `{destination}` corresponds to a grant with the wrong access mode"
        )));
    }
    if grant.purpose != expected_purpose {
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

/// Confirms a writable scratch endpoint corresponds to its read-write scratch
/// grant.
fn validate_scratch(
    scratch: &Scratch,
    grants: &BTreeMap<String, &Grant>,
) -> Result<(), ProtocolError> {
    validate_absolute_normalized(&scratch.destination, "scratch destination")?;
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

/// Confirms the structural shape of the mediated dependency-cache block and
/// that every endpoint and promotion terminus corresponds exactly to a matching
/// grant with the correct purpose and access.
///
/// Semantic cache promotion enforcement (safe-copy checks, checksum
/// verification of promoted bytes) belongs to the later mediated-acquisition
/// integration; this revision validates paths, digests, bounds, and the
/// endpoint/grant topology correspondence.
fn validate_cache(cache: &Cache, grants: &BTreeMap<String, &Grant>) -> Result<(), ProtocolError> {
    validate_absolute_normalized(&cache.cargo_home, "cache cargo_home")?;
    validate_absolute_normalized(&cache.registry, "cache registry")?;
    validate_absolute_normalized(&cache.git, "cache git")?;
    if let Some(attempt) = &cache.attempt {
        validate_absolute_normalized(&attempt.destination, "cache attempt destination")?;
        validate_digest(&attempt.identity, "cache attempt identity")?;
        // The attempt-local cache is writable during mediated acquisition, so it
        // must correspond to a read-write dependency-cache grant.
        require_endpoint_grant(
            grants,
            &attempt.destination,
            &attempt.identity,
            Access::ReadWrite,
            Purpose::DependencyCache,
            "cache attempt",
        )?;
    }
    if let Some(approved) = &cache.approved {
        validate_absolute_normalized(&approved.destination, "cache approved destination")?;
        validate_digest(&approved.identity, "cache approved identity")?;
        // The approved cache is consumed read-only, so it must correspond to a
        // read-only dependency-cache grant.
        require_endpoint_grant(
            grants,
            &approved.destination,
            &approved.identity,
            Access::ReadOnly,
            Purpose::DependencyCache,
            "cache approved",
        )?;
    }
    if let Some(promotion) = &cache.promotion {
        validate_absolute_normalized(&promotion.source, "cache promotion source")?;
        validate_absolute_normalized(&promotion.destination, "cache promotion destination")?;
        validate_digest(
            &promotion.expected_destination_identity,
            "cache promotion expected_destination_identity",
        )?;
        validate_promotion_topology(cache, promotion, grants)?;
        if promotion.manifest.len() > MAX_CACHE_MANIFEST_ENTRIES {
            return Err(invalid(format!(
                "cache promotion manifest has {} entries, exceeding the {MAX_CACHE_MANIFEST_ENTRIES}-entry limit",
                promotion.manifest.len()
            )));
        }
        for (index, entry) in promotion.manifest.iter().enumerate() {
            validate_relative_path(
                &entry.path,
                &format!("cache promotion manifest {index} path"),
            )?;
            validate_digest(
                &entry.checksum,
                &format!("cache promotion manifest {index} checksum"),
            )?;
        }
    }
    Ok(())
}

/// Confirms the promotion source and destination correspond to declared
/// endpoints and dependency-cache grants.
///
/// The promotion source is the attempt-local writable cache and must correspond
/// to a read-write dependency-cache grant; when an attempt endpoint is declared
/// it must be exactly that endpoint. The promotion destination is the project
/// cache and must correspond to a read-only dependency-cache grant whose
/// identity matches the expected destination identity; when an approved
/// endpoint is declared it must be exactly that endpoint.
fn validate_promotion_topology(
    cache: &Cache,
    promotion: &crate::protocol::CachePromotion,
    grants: &BTreeMap<String, &Grant>,
) -> Result<(), ProtocolError> {
    let source_norm = normalize_absolute(&promotion.source)
        .ok_or_else(|| invalid("cache promotion source is not a normalized path"))?;
    let source_grant = grants.get(&source_norm).ok_or_else(|| {
        invalid(format!(
            "cache promotion source `{}` does not correspond to any declared grant",
            promotion.source
        ))
    })?;
    if source_grant.purpose != Purpose::DependencyCache || source_grant.access != Access::ReadWrite
    {
        return Err(invalid(
            "cache promotion source must correspond to a read-write dependency-cache grant",
        ));
    }
    if let Some(attempt) = &cache.attempt {
        let attempt_norm = normalize_absolute(&attempt.destination)
            .ok_or_else(|| invalid("cache attempt destination is not a normalized path"))?;
        if source_norm != attempt_norm {
            return Err(invalid(
                "cache promotion source must be the declared attempt cache endpoint",
            ));
        }
    }

    let destination_norm = normalize_absolute(&promotion.destination)
        .ok_or_else(|| invalid("cache promotion destination is not a normalized path"))?;
    let destination_grant = grants.get(&destination_norm).ok_or_else(|| {
        invalid(format!(
            "cache promotion destination `{}` does not correspond to any declared grant",
            promotion.destination
        ))
    })?;
    if destination_grant.purpose != Purpose::DependencyCache
        || destination_grant.access != Access::ReadOnly
    {
        return Err(invalid(
            "cache promotion destination must correspond to a read-only dependency-cache grant",
        ));
    }
    if destination_grant.identity != promotion.expected_destination_identity {
        return Err(invalid(
            "cache promotion expected_destination_identity does not match its corresponding grant",
        ));
    }
    if let Some(approved) = &cache.approved {
        let approved_norm = normalize_absolute(&approved.destination)
            .ok_or_else(|| invalid("cache approved destination is not a normalized path"))?;
        if destination_norm != approved_norm {
            return Err(invalid(
                "cache promotion destination must be the declared approved cache endpoint",
            ));
        }
    }
    Ok(())
}

fn bounded_value(value: &str, label: &str) -> Result<(), ProtocolError> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(invalid(format!(
            "{label} is {} bytes, exceeding the {MAX_VALUE_BYTES}-byte limit",
            value.len()
        )));
    }
    Ok(())
}

/// Confirms a value is an absolute, lexically canonical path.
///
/// A canonical path is absolute, contains no interior empty component
/// (duplicate `/` separators), has no trailing slash except the root `/`, and
/// contains no `.` or `..` component. This is applied uniformly to every
/// submitted path field so a non-canonical alias cannot smuggle a different
/// effective location past the strict parser.
fn validate_absolute_normalized(value: &str, label: &str) -> Result<(), ProtocolError> {
    validate_canonical_absolute(value, label)
}

fn validate_canonical_absolute(value: &str, label: &str) -> Result<(), ProtocolError> {
    bounded_value(value, label)?;
    if value.contains('\0') {
        return Err(invalid(format!("{label} contains a NUL byte")));
    }
    if !is_absolute(value) {
        return Err(invalid(format!(
            "{label} `{value}` must be an absolute path"
        )));
    }
    if value == "/" {
        return Ok(());
    }
    for (index, component) in value.split('/').enumerate() {
        if component.is_empty() {
            if index == 0 {
                continue;
            }
            return Err(invalid(format!(
                "{label} `{value}` must be canonical without duplicate `/` separators or a trailing `/`"
            )));
        }
        if component == "." || component == ".." {
            return Err(invalid(format!(
                "{label} `{value}` must be canonical without `.` or `..` components"
            )));
        }
    }
    Ok(())
}

/// Confirms a value is a safe, lexically canonical relative path.
///
/// Used for cache promotion manifest entries, which are declared relative to a
/// cache root and must not be absolute, empty, or contain `.`/`..` or duplicate
/// separators.
fn validate_relative_path(value: &str, label: &str) -> Result<(), ProtocolError> {
    bounded_value(value, label)?;
    if value.contains('\0') {
        return Err(invalid(format!("{label} contains a NUL byte")));
    }
    if value.is_empty() {
        return Err(invalid(format!("{label} must not be empty")));
    }
    if is_absolute(value) {
        return Err(invalid(format!(
            "{label} `{value}` must be a relative path"
        )));
    }
    for component in value.split('/') {
        if component.is_empty() {
            return Err(invalid(format!(
                "{label} `{value}` must be canonical without duplicate `/` separators or a trailing `/`"
            )));
        }
        if component == "." || component == ".." {
            return Err(invalid(format!(
                "{label} `{value}` must be canonical without `.` or `..` components"
            )));
        }
    }
    Ok(())
}

/// Confirms a value is a `sha256:` digest.
fn validate_digest(value: &str, label: &str) -> Result<(), ProtocolError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(invalid(format!(
            "{label} must be a `sha256:` digest, found `{value}`"
        )));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid(format!(
            "{label} must be `sha256:` followed by 64 lowercase hexadecimal digits"
        )));
    }
    Ok(())
}

fn is_absolute(value: &str) -> bool {
    value.starts_with('/')
}

/// Lexically normalizes an absolute path, rejecting `.`/`..` and empty inputs.
fn normalize_absolute(value: &str) -> Option<String> {
    if !is_absolute(value) {
        return None;
    }
    let mut components = Vec::new();
    for component in value.split('/') {
        match component {
            "" | "." => {}
            ".." => return None,
            other => components.push(other),
        }
    }
    Some(format!("/{}", components.join("/")))
}

/// Returns true when `ancestor` is `descendant` or a path prefix of it.
fn is_prefix_path(ancestor: &str, descendant: &str) -> bool {
    if ancestor == descendant {
        return true;
    }
    let prefix = if ancestor == "/" {
        "/".to_owned()
    } else {
        format!("{ancestor}/")
    };
    descendant.starts_with(&prefix)
}
