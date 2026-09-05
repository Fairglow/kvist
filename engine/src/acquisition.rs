//! Typed, fallible mediated Cargo acquisition and verification planning.
//!
//! This module deliberately models host paths separately from fixed sandbox
//! destinations. It creates no directories, invokes no Cargo process, and
//! never selects or mutates a project cache. The runner integration owns those
//! effects; this API provides the exact, validated plans it will consume.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    net::IpAddr,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::config::{AcquisitionCacheBounds, AcquisitionConfig, PackageSource};

/// Built-in Cargo source name.
pub const CANONICAL_CRATES_IO_NAME: &str = "crates-io";
/// Built-in sparse-index origin.
pub const CANONICAL_CRATES_IO_INDEX_ORIGIN: &str = "https://index.crates.io/";
/// Built-in crate-download origin.
pub const CANONICAL_CRATES_IO_DOWNLOAD_ORIGIN: &str = "https://static.crates.io/";

const MAX_EXTRA_SOURCES: usize = 63;
const MAX_SCALAR_BYTES: usize = 4096;

/// A lexically canonical, UTF-8, absolute host path. It is intentionally
/// distinct from [`SandboxPath`] so a plan cannot accidentally serialize host
/// paths into `CARGO_HOME`, argv, or a sandbox mount destination.
///
/// A `HostPath` is validated only *lexically*: it is absolute, UTF-8, and free
/// of `.`/`..`/duplicate-separator components. It does **not** assert any host
/// authority — it is not canonicalized against the filesystem, its inode/device
/// identity is not resolved, and symlinks are not followed here. Planning can
/// therefore reject obvious lexical overlap between roots, but it cannot prove
/// two roots are disjoint on the real filesystem. The runner integration that
/// materializes these paths MUST open each root with no-follow capabilities,
/// compare inode/canonical resolution, and reject symlink aliases before
/// trusting disjointness.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostPath(PathBuf);

impl HostPath {
    /// Creates a typed host path after lexical canonicality validation.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, PlanError> {
        let path = path.into();
        validate_path(&path, "host path")?;
        Ok(Self(path))
    }

    /// Returns this path for mount-source construction only.
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    fn join(&self, child: &str) -> Result<Self, PlanError> {
        Self::new(self.0.join(child))
    }

    /// Returns true when two host paths lexically overlap: they are equal, or
    /// one is a path-component prefix of the other. This is a conservative
    /// planning check only and does not prove filesystem disjointness.
    fn lexically_overlaps(&self, other: &HostPath) -> bool {
        self.0 == other.0 || self.0.starts_with(&other.0) || other.0.starts_with(&self.0)
    }
}

/// A canonical, UTF-8, absolute sandbox path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SandboxPath(String);

impl SandboxPath {
    /// Creates a typed sandbox destination after lexical canonicality checks.
    pub fn new(path: impl Into<String>) -> Result<Self, PlanError> {
        let path = path.into();
        validate_sandbox_path(&path, "sandbox path")?;
        Ok(Self(path))
    }

    /// Returns the exact protocol string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn child(&self, child: &str) -> Result<Self, PlanError> {
        Self::new(format!("{}/{child}", self.0))
    }
}

/// A content identity used for Cargo tools and lockfile state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentIdentity(String);

impl ContentIdentity {
    /// Parses a strict `sha256:` identity.
    pub fn new(identity: impl Into<String>) -> Result<Self, PlanError> {
        let identity = identity.into();
        validate_digest(&identity, "content identity")?;
        Ok(Self(identity))
    }

    /// Derives an identity from exact bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(digest_label(bytes))
    }

    /// Returns the canonical digest label.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A promotion-manifest entry bound into an acquisition result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionManifestEntry {
    path: String,
    size: u64,
    checksum: ContentIdentity,
}

impl PromotionManifestEntry {
    /// Creates an exact promotion-manifest entry.
    pub fn new(
        path: impl Into<String>,
        size: u64,
        checksum: ContentIdentity,
    ) -> Result<Self, PlanError> {
        let path = path.into();
        validate_relative_path(&path, "promotion manifest path")?;
        Ok(Self {
            path,
            size,
            checksum,
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn checksum(&self) -> &ContentIdentity {
        &self.checksum
    }
}

/// A complete acquisition result binding Cargo's real cache, both lockfile
/// identities, sources, and the exact cache manifest. The later runner
/// integration passes this data to immutable generation construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquisitionResult {
    cargo_home_source: HostPath,
    lockfile_before_identity: ContentIdentity,
    lockfile_after_identity: ContentIdentity,
    supported_source_identities: Vec<ContentIdentity>,
    manifest: Vec<PromotionManifestEntry>,
}

impl AcquisitionResult {
    pub fn cargo_home_source(&self) -> &HostPath {
        &self.cargo_home_source
    }

    pub fn lockfile_before_identity(&self) -> &ContentIdentity {
        &self.lockfile_before_identity
    }

    pub fn lockfile_after_identity(&self) -> &ContentIdentity {
        &self.lockfile_after_identity
    }

    pub fn supported_source_identities(&self) -> &[ContentIdentity] {
        &self.supported_source_identities
    }

    pub fn manifest(&self) -> &[PromotionManifestEntry] {
        &self.manifest
    }
}

/// An independently actionable planning failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanError {
    detail: String,
}

impl PlanError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for PlanError {}

/// A resolved package source with its independently derived identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedSource {
    CargoRegistry {
        name: String,
        index_origin: String,
        download_origin: String,
        identity: ContentIdentity,
    },
    CargoGit {
        repository: String,
        revision: String,
        identity: ContentIdentity,
    },
}

impl ResolvedSource {
    pub fn identity(&self) -> &ContentIdentity {
        match self {
            Self::CargoRegistry { identity, .. } | Self::CargoGit { identity, .. } => identity,
        }
    }
}

/// The exact mediated acquisition plan. All fields are private so consumers
/// cannot manually splice host and sandbox paths after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquisitionPlan {
    cargo_tool_host: HostPath,
    cargo_tool_sandbox: SandboxPath,
    cargo_identity: ContentIdentity,
    cargo_home_host: HostPath,
    cargo_home_sandbox: SandboxPath,
    lockfile_workspace_host: HostPath,
    lockfile_workspace_sandbox: SandboxPath,
    scratch_host: HostPath,
    scratch_sandbox: SandboxPath,
    target_host: HostPath,
    target_sandbox: SandboxPath,
    sources: Vec<ResolvedSource>,
    environment: BTreeMap<String, String>,
    cache_bounds: AcquisitionCacheBounds,
    argv: Vec<String>,
    lockfile_before_identity: ContentIdentity,
}

impl AcquisitionPlan {
    pub fn cargo_tool_host(&self) -> &HostPath {
        &self.cargo_tool_host
    }

    pub fn cargo_tool_sandbox(&self) -> &SandboxPath {
        &self.cargo_tool_sandbox
    }

    pub fn cargo_identity(&self) -> &ContentIdentity {
        &self.cargo_identity
    }

    /// The real attempt-local `CARGO_HOME` and immutable-generation source.
    pub fn cargo_home_host(&self) -> &HostPath {
        &self.cargo_home_host
    }

    pub fn cargo_home_sandbox(&self) -> &SandboxPath {
        &self.cargo_home_sandbox
    }

    pub fn lockfile_workspace_host(&self) -> &HostPath {
        &self.lockfile_workspace_host
    }

    pub fn lockfile_workspace_sandbox(&self) -> &SandboxPath {
        &self.lockfile_workspace_sandbox
    }

    pub fn scratch_host(&self) -> &HostPath {
        &self.scratch_host
    }

    pub fn scratch_sandbox(&self) -> &SandboxPath {
        &self.scratch_sandbox
    }

    pub fn target_host(&self) -> &HostPath {
        &self.target_host
    }

    pub fn target_sandbox(&self) -> &SandboxPath {
        &self.target_sandbox
    }

    pub fn sources(&self) -> &[ResolvedSource] {
        &self.sources
    }

    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    pub fn cache_bounds(&self) -> AcquisitionCacheBounds {
        self.cache_bounds
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    pub fn lockfile_before_identity(&self) -> &ContentIdentity {
        &self.lockfile_before_identity
    }

    /// Binds the observed post-fetch lockfile and exact promoted Cargo-home
    /// manifest. A lockfile change is valid: acquisition deliberately does not
    /// use `--locked`.
    pub fn bind_result(
        &self,
        lockfile_after_identity: ContentIdentity,
        manifest: Vec<PromotionManifestEntry>,
    ) -> Result<AcquisitionResult, PlanError> {
        let mut paths = BTreeSet::new();
        for entry in &manifest {
            if !paths.insert(entry.path()) {
                return Err(PlanError::new(format!(
                    "promotion manifest repeats `{}`",
                    entry.path()
                )));
            }
        }
        Ok(AcquisitionResult {
            cargo_home_source: self.cargo_home_host.clone(),
            lockfile_before_identity: self.lockfile_before_identity.clone(),
            lockfile_after_identity,
            supported_source_identities: self
                .sources
                .iter()
                .map(|source| source.identity().clone())
                .collect(),
            manifest,
        })
    }
}

/// Exact offline-verification plan. It consumes the project-approved Cargo
/// home read-only and has an independent writable target scratch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationPlan {
    cargo_tool_host: HostPath,
    cargo_tool_sandbox: SandboxPath,
    cargo_identity: ContentIdentity,
    approved_cargo_home_host: HostPath,
    approved_cargo_home_sandbox: SandboxPath,
    scratch_host: HostPath,
    scratch_sandbox: SandboxPath,
    target_host: HostPath,
    target_sandbox: SandboxPath,
    environment: BTreeMap<String, String>,
    argv: Vec<String>,
}

impl VerificationPlan {
    pub fn cargo_tool_host(&self) -> &HostPath {
        &self.cargo_tool_host
    }

    pub fn cargo_tool_sandbox(&self) -> &SandboxPath {
        &self.cargo_tool_sandbox
    }

    pub fn cargo_identity(&self) -> &ContentIdentity {
        &self.cargo_identity
    }

    pub fn approved_cargo_home_host(&self) -> &HostPath {
        &self.approved_cargo_home_host
    }

    pub fn approved_cargo_home_sandbox(&self) -> &SandboxPath {
        &self.approved_cargo_home_sandbox
    }

    pub fn scratch_host(&self) -> &HostPath {
        &self.scratch_host
    }

    pub fn scratch_sandbox(&self) -> &SandboxPath {
        &self.scratch_sandbox
    }

    pub fn target_host(&self) -> &HostPath {
        &self.target_host
    }

    pub fn target_sandbox(&self) -> &SandboxPath {
        &self.target_sandbox
    }

    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}

/// Creates the exact non-compiling `cargo fetch` acquisition plan.
pub fn build_acquisition_plan(
    attempt_root: HostPath,
    cargo_tool: HostPath,
    cargo_identity: ContentIdentity,
    lockfile_before_identity: ContentIdentity,
    path_value: String,
    config: &AcquisitionConfig,
) -> Result<AcquisitionPlan, PlanError> {
    tracing::debug!(
        sources_count = config.additional_sources.len() + 1,
        "building dependency acquisition plan"
    );
    validate_acquisition_config(config)?;
    validate_path_value(&path_value)?;
    // The supplied toolchain root must not lexically overlap the attempt root
    // (whose children supply the writable Cargo home, lockfile workspace, and
    // scratch). This is a conservative planning check; the runner must still
    // verify inode-level disjointness when it materializes the mounts.
    if cargo_tool.lexically_overlaps(&attempt_root) {
        return Err(PlanError::new(
            "the Cargo tool path must not lexically overlap the acquisition attempt root",
        ));
    }
    let cargo_home_host = attempt_root.join("cargo-home")?;
    let lockfile_workspace_host = attempt_root.join("lockfile-workspace")?;
    let scratch_host = attempt_root.join("acquisition-scratch")?;
    let target_host = scratch_host.join("target")?;
    let cargo_tool_sandbox = SandboxPath::new("/workspace/toolchain/cargo")?;
    let cargo_home_sandbox = SandboxPath::new("/workspace/cargo-home")?;
    let lockfile_workspace_sandbox = SandboxPath::new("/workspace/lockfile")?;
    let scratch_sandbox = SandboxPath::new("/workspace/scratch")?;
    let target_sandbox = scratch_sandbox.child("target")?;
    let mut environment = BTreeMap::new();
    environment.insert(
        "HOME".to_owned(),
        scratch_sandbox.child("home")?.as_str().to_owned(),
    );
    environment.insert("PATH".to_owned(), path_value);
    environment.insert(
        "CARGO_HOME".to_owned(),
        cargo_home_sandbox.as_str().to_owned(),
    );
    environment.insert(
        "CARGO_TARGET_DIR".to_owned(),
        target_sandbox.as_str().to_owned(),
    );
    environment.insert(
        "CARGO_NET_GIT_FETCH_WITH_CLI".to_owned(),
        "false".to_owned(),
    );
    Ok(AcquisitionPlan {
        cargo_tool_host: cargo_tool,
        cargo_tool_sandbox: cargo_tool_sandbox.clone(),
        cargo_identity,
        cargo_home_host,
        cargo_home_sandbox,
        lockfile_workspace_host,
        lockfile_workspace_sandbox,
        scratch_host,
        scratch_sandbox,
        target_host,
        target_sandbox,
        sources: resolved_sources(config)?,
        environment,
        cache_bounds: config.cache_bounds,
        argv: vec![cargo_tool_sandbox.as_str().to_owned(), "fetch".to_owned()],
        lockfile_before_identity,
    })
}

/// Creates the exact offline `cargo test --locked` verification plan.
pub fn build_verification_plan(
    verification_root: HostPath,
    cargo_tool: HostPath,
    cargo_identity: ContentIdentity,
    approved_cargo_home: HostPath,
    path_value: String,
) -> Result<VerificationPlan, PlanError> {
    tracing::debug!("building offline verification plan");
    validate_path_value(&path_value)?;
    // The toolchain root and the approved Cargo-home generation must not
    // lexically overlap the verification root (whose children supply scratch and
    // the target directory), nor each other. Filesystem-level disjointness
    // remains the runner's responsibility.
    for (left, right) in [
        (&cargo_tool, &verification_root),
        (&approved_cargo_home, &verification_root),
        (&cargo_tool, &approved_cargo_home),
    ] {
        if left.lexically_overlaps(right) {
            return Err(PlanError::new(
                "verification toolchain, approved Cargo home, and root must not lexically overlap",
            ));
        }
    }
    let scratch_host = verification_root.join("verification-scratch")?;
    let target_host = scratch_host.join("target")?;
    let cargo_tool_sandbox = SandboxPath::new("/workspace/toolchain/cargo")?;
    let approved_cargo_home_sandbox = SandboxPath::new("/workspace/cargo-home")?;
    let scratch_sandbox = SandboxPath::new("/workspace/scratch")?;
    let target_sandbox = scratch_sandbox.child("target")?;
    let mut environment = BTreeMap::new();
    environment.insert(
        "HOME".to_owned(),
        scratch_sandbox.child("home")?.as_str().to_owned(),
    );
    environment.insert("PATH".to_owned(), path_value);
    environment.insert(
        "CARGO_HOME".to_owned(),
        approved_cargo_home_sandbox.as_str().to_owned(),
    );
    environment.insert(
        "CARGO_TARGET_DIR".to_owned(),
        target_sandbox.as_str().to_owned(),
    );
    environment.insert("CARGO_NET_OFFLINE".to_owned(), "true".to_owned());
    Ok(VerificationPlan {
        cargo_tool_host: cargo_tool,
        cargo_tool_sandbox: cargo_tool_sandbox.clone(),
        cargo_identity,
        approved_cargo_home_host: approved_cargo_home,
        approved_cargo_home_sandbox,
        scratch_host,
        scratch_sandbox,
        target_host,
        target_sandbox,
        environment,
        argv: vec![
            cargo_tool_sandbox.as_str().to_owned(),
            "test".to_owned(),
            "--locked".to_owned(),
        ],
    })
}

/// Derives the built-in crates.io identity.
pub fn crates_io_identity() -> ContentIdentity {
    registry_identity(
        CANONICAL_CRATES_IO_NAME,
        CANONICAL_CRATES_IO_INDEX_ORIGIN,
        CANONICAL_CRATES_IO_DOWNLOAD_ORIGIN,
    )
}

/// Derives a registry identity from its canonical fields.
pub fn registry_identity(name: &str, index_origin: &str, download_origin: &str) -> ContentIdentity {
    source_identity(
        b"kvist/cargo-registry/v1\0",
        &[
            name.as_bytes(),
            index_origin.as_bytes(),
            download_origin.as_bytes(),
        ],
    )
}

/// Derives an immutable-Git identity from canonical fields.
pub fn git_identity(repository: &str, revision: &str) -> ContentIdentity {
    source_identity(
        b"kvist/cargo-git/v1\0",
        &[repository.as_bytes(), revision.as_bytes()],
    )
}

fn source_identity(domain: &[u8], fields: &[&[u8]]) -> ContentIdentity {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for field in fields {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field);
    }
    ContentIdentity(format!("sha256:{}", hex::encode(hasher.finalize())))
}

/// Resolves the full built-in-plus-configured source set after enforcing
/// complete identity, registry-name, and origin-overlap parity.
pub fn resolved_sources(config: &AcquisitionConfig) -> Result<Vec<ResolvedSource>, PlanError> {
    validate_acquisition_config(config)?;
    let mut sources = vec![ResolvedSource::CargoRegistry {
        name: CANONICAL_CRATES_IO_NAME.to_owned(),
        index_origin: CANONICAL_CRATES_IO_INDEX_ORIGIN.to_owned(),
        download_origin: CANONICAL_CRATES_IO_DOWNLOAD_ORIGIN.to_owned(),
        identity: crates_io_identity(),
    }];
    for source in &config.additional_sources {
        sources.push(match source {
            PackageSource::CargoRegistry {
                name,
                index_origin,
                download_origin,
            } => ResolvedSource::CargoRegistry {
                name: name.clone(),
                index_origin: index_origin.clone(),
                download_origin: download_origin.clone(),
                identity: registry_identity(name, index_origin, download_origin),
            },
            PackageSource::CargoGit {
                repository,
                revision,
            } => ResolvedSource::CargoGit {
                repository: repository.clone(),
                revision: revision.clone(),
                identity: git_identity(repository, revision),
            },
        });
    }
    Ok(sources)
}

/// Validates a manually constructed public acquisition config as thoroughly as
/// parsed TOML. Plan construction calls this independently.
pub fn validate_acquisition_config(config: &AcquisitionConfig) -> Result<(), PlanError> {
    validate_bounds(config.cache_bounds)?;
    if config.additional_sources.len() > MAX_EXTRA_SOURCES {
        return Err(PlanError::new(format!(
            "at most {MAX_EXTRA_SOURCES} additional sources are permitted"
        )));
    }
    let mut names = BTreeSet::from([CANONICAL_CRATES_IO_NAME.to_owned()]);
    let mut identities = BTreeSet::from([crates_io_identity().as_str().to_owned()]);
    let mut origins = vec![
        parse_production_origin(CANONICAL_CRATES_IO_INDEX_ORIGIN)?,
        parse_production_origin(CANONICAL_CRATES_IO_DOWNLOAD_ORIGIN)?,
    ];
    for source in &config.additional_sources {
        match source {
            PackageSource::CargoRegistry {
                name,
                index_origin,
                download_origin,
            } => {
                bounded_scalar(name, "registry name")?;
                if !safe_registry_name(name) || !names.insert(name.clone()) {
                    return Err(PlanError::new(
                        "registry names must be unique nonempty safe tokens",
                    ));
                }
                let index = parse_production_origin(index_origin)?;
                let download = parse_production_origin(download_origin)?;
                if index_origin == CANONICAL_CRATES_IO_INDEX_ORIGIN
                    || download_origin == CANONICAL_CRATES_IO_DOWNLOAD_ORIGIN
                {
                    return Err(PlanError::new(
                        "additional registries must not impersonate crates.io",
                    ));
                }
                let identity = registry_identity(name, index_origin, download_origin);
                if !identities.insert(identity.as_str().to_owned()) {
                    return Err(PlanError::new(
                        "configured sources must have distinct derived identities",
                    ));
                }
                origins.push(index);
                origins.push(download);
            }
            PackageSource::CargoGit {
                repository,
                revision,
            } => {
                let repository = parse_production_origin(repository)?;
                if repository.scheme != "https" || !immutable_revision(revision) {
                    return Err(PlanError::new(
                        "Cargo Git sources require canonical HTTPS URLs and immutable 40-hex revisions",
                    ));
                }
                let identity = git_identity(repository.original.as_str(), revision);
                if !identities.insert(identity.as_str().to_owned()) {
                    return Err(PlanError::new(
                        "configured sources must have distinct derived identities",
                    ));
                }
                origins.push(repository);
            }
        }
    }
    for (index, left) in origins.iter().enumerate() {
        if origins
            .iter()
            .skip(index + 1)
            .any(|right| origins_overlap(left, right))
        {
            return Err(PlanError::new(
                "configured package-source origins must not overlap",
            ));
        }
    }
    Ok(())
}

/// Validates one source independently for TOML diagnostics.
pub fn validate_package_source(source: &PackageSource) -> Result<(), String> {
    let config = AcquisitionConfig {
        cache_bounds: AcquisitionCacheBounds::default(),
        additional_sources: vec![source.clone()],
    };
    validate_acquisition_config(&config).map_err(|error| error.to_string())
}

fn validate_bounds(bounds: AcquisitionCacheBounds) -> Result<(), PlanError> {
    if bounds.max_files == 0 || bounds.max_file_bytes == 0 || bounds.max_cache_bytes == 0 {
        return Err(PlanError::new(
            "all acquisition cache bounds must be nonzero",
        ));
    }
    // `max_files` bounds the promoted file count, which can never exceed the
    // protocol promotion-manifest maximum of 4096 entries.
    if bounds.max_files > 4096
        || bounds.max_file_bytes > 8 * 1024 * 1024 * 1024
        || bounds.max_cache_bytes > 64 * 1024 * 1024 * 1024
    {
        return Err(PlanError::new(
            "an acquisition cache bound exceeds the runner maximum",
        ));
    }
    Ok(())
}

fn validate_path(path: &Path, label: &str) -> Result<(), PlanError> {
    let text = path
        .to_str()
        .ok_or_else(|| PlanError::new(format!("{label} must be valid UTF-8")))?;
    validate_sandbox_path(text, label)
}

fn validate_sandbox_path(path: &str, label: &str) -> Result<(), PlanError> {
    bounded_scalar(path, label)?;
    if !path.starts_with('/') || path.contains('\0') || (path != "/" && path.ends_with('/')) {
        return Err(PlanError::new(format!(
            "{label} must be an absolute canonical path"
        )));
    }
    for part in path.split('/').skip(1) {
        if part.is_empty() || part == "." || part == ".." {
            return Err(PlanError::new(format!(
                "{label} must not contain duplicate separators, `.` or `..`"
            )));
        }
    }
    Ok(())
}

fn validate_path_value(value: &str) -> Result<(), PlanError> {
    if value.is_empty() || value.contains(':') {
        return Err(PlanError::new(
            "Cargo PATH must be one explicit canonical absolute directory",
        ));
    }
    validate_sandbox_path(value, "Cargo PATH")
}

fn validate_relative_path(path: &str, label: &str) -> Result<(), PlanError> {
    bounded_scalar(path, label)?;
    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return Err(PlanError::new(format!(
            "{label} must be a canonical relative path"
        )));
    }
    if path
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(PlanError::new(format!("{label} must be canonical")));
    }
    Ok(())
}

fn validate_digest(value: &str, label: &str) -> Result<(), PlanError> {
    if value.strip_prefix("sha256:").is_none_or(|hex| {
        hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }) {
        return Err(PlanError::new(format!(
            "{label} must be sha256: followed by 64 lower-case hexadecimal digits"
        )));
    }
    Ok(())
}

fn bounded_scalar(value: &str, label: &str) -> Result<(), PlanError> {
    if value.len() > MAX_SCALAR_BYTES {
        return Err(PlanError::new(format!(
            "{label} exceeds the {MAX_SCALAR_BYTES}-byte limit"
        )));
    }
    Ok(())
}

fn digest_label(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn safe_registry_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn immutable_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[derive(Debug)]
struct ParsedOrigin {
    original: String,
    scheme: &'static str,
    host: String,
    port: u16,
    path: String,
}

fn parse_production_origin(value: &str) -> Result<ParsedOrigin, PlanError> {
    bounded_scalar(value, "package-source origin")?;
    if value.contains(['\0', '%', '?', '#']) || value.chars().any(char::is_whitespace) {
        return Err(PlanError::new(
            "package-source origins must not contain credentials, query, fragment, percent aliases, or whitespace",
        ));
    }
    let rest = value.strip_prefix("https://").ok_or_else(|| {
        PlanError::new("production package-source origins must use canonical HTTPS")
    })?;
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => {
            return Err(PlanError::new(
                "package-source origins must include canonical `/` path",
            ));
        }
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(PlanError::new(
            "package-source origins must not contain credentials and must declare a host",
        ));
    }
    let (host, port) = split_authority(authority)?;
    if !legal_host(&host) || host == "localhost" {
        return Err(PlanError::new("package-source origin host is invalid"));
    }
    if let Ok(address) = host.parse::<IpAddr>()
        && !public_address(address)
    {
        return Err(PlanError::new(
            "production package-source origin address is private, link-local, or loopback",
        ));
    }
    let canonical_path = canonical_url_path(path)?;
    let parsed = ParsedOrigin {
        original: value.to_owned(),
        scheme: "https",
        host,
        port,
        path: canonical_path,
    };
    if serialize_origin(&parsed) != value {
        return Err(PlanError::new(
            "package-source origins must use exact canonical HTTPS serialization",
        ));
    }
    Ok(parsed)
}

fn split_authority(authority: &str) -> Result<(String, u16), PlanError> {
    if let Some(rest) = authority.strip_prefix('[') {
        let close = rest
            .find(']')
            .ok_or_else(|| PlanError::new("IPv6 package-source origin is missing `]`"))?;
        let host = rest[..close].to_ascii_lowercase();
        let suffix = &rest[close + 1..];
        let port = if suffix.is_empty() {
            443
        } else {
            suffix
                .strip_prefix(':')
                .ok_or_else(|| PlanError::new("invalid IPv6 package-source origin port"))?
                .parse::<u16>()
                .map_err(|_| PlanError::new("invalid package-source origin port"))?
        };
        return Ok((host, port));
    }
    match authority.rsplit_once(':') {
        Some((host, port))
            if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            Ok((
                host.to_ascii_lowercase(),
                port.parse::<u16>()
                    .map_err(|_| PlanError::new("invalid package-source origin port"))?,
            ))
        }
        Some(_) => Err(PlanError::new("invalid package-source origin port")),
        None => Ok((authority.to_ascii_lowercase(), 443)),
    }
}

fn legal_host(host: &str) -> bool {
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'))
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
    {
        return false;
    }
    if host.contains(':') {
        // Only a real IPv6 literal may contain a colon.
        return host.parse::<std::net::Ipv6Addr>().is_ok();
    }
    // Reject noncanonical numeric IPv4 aliases (single integer, shortened,
    // octal, or hexadecimal) that resolve to loopback/private addresses through
    // `inet_aton`-style parsing but bypass dotted-decimal classification.
    !is_numeric_ipv4_alias(host)
}

fn is_numeric_ipv4_alias(host: &str) -> bool {
    if is_canonical_dotted_ipv4(host) {
        return false;
    }
    let has_digit = host.bytes().any(|byte| byte.is_ascii_digit());
    has_digit
        && host
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || matches!(byte, b'.' | b'x' | b'X'))
}

fn is_canonical_dotted_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 3
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && !(part.len() > 1 && part.starts_with('0'))
                && part.parse::<u8>().is_ok()
        })
}

fn canonical_url_path(path: &str) -> Result<String, PlanError> {
    if !path.starts_with('/') || path.contains("//") {
        return Err(PlanError::new(
            "package-source origin path must be canonical",
        ));
    }
    if path.split('/').any(|part| part == "." || part == "..") {
        return Err(PlanError::new(
            "package-source origin path must not contain traversal",
        ));
    }
    Ok(path.to_owned())
}

fn serialize_origin(origin: &ParsedOrigin) -> String {
    let host = if origin.host.contains(':') {
        format!("[{}]", origin.host)
    } else {
        origin.host.clone()
    };
    if origin.port == 443 {
        format!("{}://{host}{}", origin.scheme, origin.path)
    } else {
        format!("{}://{host}:{}{}", origin.scheme, origin.port, origin.path)
    }
}

fn origins_overlap(left: &ParsedOrigin, right: &ParsedOrigin) -> bool {
    left.scheme == right.scheme
        && left.host == right.host
        && left.port == right.port
        && (path_within(&left.path, &right.path) || path_within(&right.path, &left.path))
}

fn path_within(prefix: &str, candidate: &str) -> bool {
    let prefix = prefix.strip_suffix('/').unwrap_or(prefix);
    prefix.is_empty()
        || candidate == prefix
        || candidate
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            !(address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || octets[0] >= 224)
        }
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return public_address(IpAddr::V4(mapped));
            }
            let first = address.segments()[0];
            !(address.is_loopback()
                || address.is_unspecified()
                || address.is_multicast()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTRY_GOLDEN: &str =
        "sha256:4d3fc763437bea9217f14b2bc9b5201dbf78ea13ac0ef390497cc54896ab9486";
    const GIT_GOLDEN: &str =
        "sha256:3405bc5e24e0f91ff11d2271a69f9c9b7a0547adb6acace6d10638fee39d2fe6";

    fn source_config() -> AcquisitionConfig {
        AcquisitionConfig {
            cache_bounds: AcquisitionCacheBounds::default(),
            additional_sources: vec![PackageSource::CargoRegistry {
                name: "private".to_owned(),
                index_origin: "https://registry.example.invalid/index/".to_owned(),
                download_origin: "https://registry.example.invalid/crates/".to_owned(),
            }],
        }
    }

    #[test]
    fn source_identity_golden_vectors_and_field_substitutions() {
        let registry = registry_identity(
            "private",
            "https://registry.example.invalid/index/",
            "https://registry.example.invalid/crates/",
        );
        let git = git_identity(
            "https://git.example.invalid/dependency.git",
            "0123456789abcdef0123456789abcdef01234567",
        );
        assert_eq!(registry.as_str(), REGISTRY_GOLDEN);
        assert_eq!(git.as_str(), GIT_GOLDEN);
        assert_ne!(
            registry,
            registry_identity(
                "other",
                "https://registry.example.invalid/index/",
                "https://registry.example.invalid/crates/"
            )
        );
        assert_ne!(
            registry,
            registry_identity(
                "private",
                "https://other.example.invalid/index/",
                "https://registry.example.invalid/crates/"
            )
        );
        assert_ne!(
            registry,
            registry_identity(
                "private",
                "https://registry.example.invalid/index/",
                "https://other.example.invalid/crates/"
            )
        );
        assert_ne!(
            git,
            git_identity(
                "https://git.example.invalid/other.git",
                "0123456789abcdef0123456789abcdef01234567"
            )
        );
        assert_ne!(
            git,
            git_identity(
                "https://git.example.invalid/dependency.git",
                "1123456789abcdef0123456789abcdef01234567"
            )
        );
    }

    #[test]
    fn plans_use_real_cargo_home_and_distinct_typed_paths() {
        let plan = build_acquisition_plan(
            HostPath::new("/workspace/attempt").expect("attempt"),
            HostPath::new("/workspace/toolchain/cargo").expect("cargo"),
            ContentIdentity::from_bytes(b"cargo"),
            ContentIdentity::from_bytes(b"old-lock"),
            "/usr/bin".to_owned(),
            &source_config(),
        )
        .expect("plan");
        assert_eq!(plan.argv(), ["/workspace/toolchain/cargo", "fetch"]);
        assert_eq!(
            plan.cargo_home_host().as_path(),
            Path::new("/workspace/attempt/cargo-home")
        );
        assert_eq!(plan.cargo_home_sandbox().as_str(), "/workspace/cargo-home");
        assert_eq!(
            plan.lockfile_workspace_sandbox().as_str(),
            "/workspace/lockfile"
        );
        assert_eq!(
            plan.environment()
                .get("CARGO_TARGET_DIR")
                .map(String::as_str),
            Some("/workspace/scratch/target")
        );
        assert!(!plan.environment().contains_key("CARGO_NET_OFFLINE"));
    }

    #[test]
    fn verification_is_offline_locked_with_independent_scratch() {
        let plan = build_verification_plan(
            HostPath::new("/workspace/verify").expect("root"),
            HostPath::new("/workspace/toolchain/cargo").expect("cargo"),
            ContentIdentity::from_bytes(b"cargo"),
            HostPath::new("/workspace/project-cache/generation").expect("cache"),
            "/usr/bin".to_owned(),
        )
        .expect("plan");
        assert_eq!(
            plan.argv(),
            ["/workspace/toolchain/cargo", "test", "--locked"]
        );
        assert_eq!(
            plan.environment().get("CARGO_HOME").map(String::as_str),
            Some("/workspace/cargo-home")
        );
        assert_eq!(
            plan.environment()
                .get("CARGO_NET_OFFLINE")
                .map(String::as_str),
            Some("true")
        );
        assert_ne!(plan.scratch_host(), plan.approved_cargo_home_host());
    }

    #[test]
    fn validates_manual_config_count_overlap_and_production_origins() {
        let mut config = source_config();
        config
            .additional_sources
            .push(PackageSource::CargoRegistry {
                name: "nested".to_owned(),
                index_origin: "https://registry.example.invalid/index/nested/".to_owned(),
                download_origin: "https://other.example.invalid/crates/".to_owned(),
            });
        assert!(validate_acquisition_config(&config).is_err());
        config = source_config();
        config.additional_sources = (0..64)
            .map(|index| PackageSource::CargoGit {
                repository: format!("https://git{index}.example.invalid/dependency.git"),
                revision: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            })
            .collect();
        assert!(validate_acquisition_config(&config).is_err());
        assert!(
            validate_package_source(&PackageSource::CargoRegistry {
                name: "local".to_owned(),
                index_origin: "http://127.0.0.1:8080/index/".to_owned(),
                download_origin: "http://127.0.0.1:8080/crates/".to_owned(),
            })
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn typed_paths_reject_relative_noncanonical_and_non_utf8_inputs() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        assert!(HostPath::new("relative").is_err());
        assert!(HostPath::new("/workspace/../outside").is_err());
        assert!(SandboxPath::new("/workspace//cache").is_err());
        assert!(HostPath::new(PathBuf::from(OsString::from_vec(vec![b'/', 0xff,]))).is_err());
    }

    #[test]
    fn plans_reject_lexically_overlapping_host_roots() {
        // The Cargo tool path may not lexically overlap the attempt root.
        assert!(
            build_acquisition_plan(
                HostPath::new("/workspace/attempt").expect("attempt"),
                HostPath::new("/workspace/attempt/cargo").expect("cargo"),
                ContentIdentity::from_bytes(b"cargo"),
                ContentIdentity::from_bytes(b"lock"),
                "/usr/bin".to_owned(),
                &source_config(),
            )
            .is_err()
        );
        // The approved Cargo home may not lexically overlap the verification
        // root, and the toolchain may not overlap either.
        assert!(
            build_verification_plan(
                HostPath::new("/workspace/verify").expect("root"),
                HostPath::new("/workspace/toolchain/cargo").expect("cargo"),
                ContentIdentity::from_bytes(b"cargo"),
                HostPath::new("/workspace/verify/cache").expect("cache"),
                "/usr/bin".to_owned(),
            )
            .is_err()
        );
        // Lexical non-overlap is not proof of filesystem disjointness; a valid
        // plan still relies on the runner opening no-follow capabilities.
        assert!(
            build_verification_plan(
                HostPath::new("/workspace/verify").expect("root"),
                HostPath::new("/opt/toolchain/cargo").expect("cargo"),
                ContentIdentity::from_bytes(b"cargo"),
                HostPath::new("/var/cache/generation").expect("cache"),
                "/usr/bin".to_owned(),
            )
            .is_ok()
        );
    }

    #[test]
    fn production_origins_reject_numeric_ipv4_aliases_and_mapped_ipv6() {
        for origin in [
            "https://2130706433/index/",
            "https://127.1/index/",
            "https://0x7f.0.0.1/index/",
            "https://0177.0.0.1/index/",
            "https://[::ffff:127.0.0.1]/index/",
            "https://[::ffff:10.0.0.1]/index/",
        ] {
            assert!(
                validate_package_source(&PackageSource::CargoRegistry {
                    name: "aliased".to_owned(),
                    index_origin: origin.to_owned(),
                    download_origin: "https://registry.example.invalid/crates/".to_owned(),
                })
                .is_err(),
                "origin `{origin}` must be rejected by the engine parser"
            );
        }
    }
}
