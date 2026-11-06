//! Full locked Rust readiness and explicitly authorized acquisition.
//!
//! No application configuration is read. Host Cargo inherits operator policy;
//! `--offline` constrains Cargo, not the operating system.

use crate::{
    Error, ProcessFailure, Result,
    fs::{Dir, Identity, METADATA_LIMIT, absolute, io_error},
    integrity::{self, LockedGraph, control},
    publication::{Publication, RECOVERY},
    resolver, unavailable,
    vendoring::{
        DEFAULT_VENDORED_DIRNAME, SANDBOX_CARGO_CONFIG_DIRNAME, VENDOR_MANIFEST_FILENAME,
        VENDOR_SANDBOX_MOUNT, VENDOR_SCHEMA_VERSION, VendorManifest, VendoringReport,
        now_unix_secs,
    },
};
use sav::{CancellationToken, CommandSpec, SupervisionPolicy, run_supervised_observed};
use std::{
    fs::File,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

const OPERATION_LOCK: &str = "vendor-operation.lock";
const OUTPUT_LIMIT: usize = 1024 * 1024;

/// Explicit host acquisition controls. There are deliberately no defaults.
#[derive(Debug, Clone)]
pub struct ProvisionOptions {
    /// Absolute executable outside the project, selected by the operator.
    pub cargo: PathBuf,
    /// Ask Cargo for offline acquisition, not OS network confinement.
    pub offline: bool,
    /// Operator-owned destination; None uses `.kvist/vendored`.
    pub vendored_dir: Option<PathBuf>,
    /// Wall-clock and idle bound, between 1 and 900 seconds.
    pub timeout: Duration,
}

fn state_clear(state: &Dir) -> Result<()> {
    if state.exists(RECOVERY)? {
        return Err(unavailable(
            &state.display,
            "interrupted vendor publication requires explicit inspection and reconciliation",
        ));
    }
    Ok(())
}

struct ControlObservation {
    state: Option<Dir>,
    resolver: Option<String>,
    manifest_identity: Option<Identity>,
    resolver_identity: Option<Identity>,
}

fn inspect_controls(
    project: &Dir,
    graph: &LockedGraph,
    refuse_writer: bool,
    allow_legacy: bool,
) -> Result<ControlObservation> {
    let unmanaged = |state| -> Result<ControlObservation> {
        if allow_legacy {
            // The built-in crates.io resolver is the only implicit source policy.
            resolver::bind(
                &resolver::parse(&crate::vendoring::offline_cargo_config(
                    VENDOR_SANDBOX_MOUNT,
                ))?,
                graph,
                true,
            )?;
        } else if graph.external() {
            return Err(unavailable(&project.display, "vendoring state absent"));
        }
        Ok(ControlObservation {
            state,
            resolver: None,
            manifest_identity: None,
            resolver_identity: None,
        })
    };
    if !project.exists(".kvist")? {
        return unmanaged(None);
    }
    let state = project.child(".kvist")?;
    state_clear(&state)?;
    if refuse_writer && state.exists(OPERATION_LOCK)? {
        return Err(Error::OperationLocked {
            path: state.display.join(OPERATION_LOCK),
        });
    }
    if !state.exists(VENDOR_MANIFEST_FILENAME)?
        && !state.exists(SANDBOX_CARGO_CONFIG_DIRNAME)?
        && (allow_legacy || !state.exists(DEFAULT_VENDORED_DIRNAME)?)
    {
        return unmanaged(Some(state));
    }
    let manifest_identity = validate_manifest(project, graph, &state)?;
    let cargo = state.child(SANDBOX_CARGO_CONFIG_DIRNAME)?;
    let (bytes, resolver_identity) = cargo.read("config.toml", resolver::CONFIG_LIMIT as u64)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| unavailable(&cargo.display, "resolver is not UTF-8"))?;
    let config = resolver::parse(text)?;
    resolver::bind(&config, graph, true)?;
    let config = toml::to_string(&config)
        .map_err(|_| unavailable(&cargo.display, "cannot serialize source configuration"))?;
    Ok(ControlObservation {
        state: Some(state),
        resolver: Some(config),
        manifest_identity: Some(manifest_identity),
        resolver_identity: Some(resolver_identity),
    })
}

fn validate_manifest(project: &Dir, graph: &LockedGraph, state: &Dir) -> Result<Identity> {
    let (bytes, identity) = state.read(VENDOR_MANIFEST_FILENAME, METADATA_LIMIT)?;
    let manifest: VendorManifest = serde_json::from_slice(&bytes)
        .map_err(|_| unavailable(&project.display, "malformed vendoring manifest"))?;
    if manifest.schema_version != VENDOR_SCHEMA_VERSION
        || !manifest.verified
        || manifest.lockfile_path != "Cargo.lock"
        || manifest.sandbox_vendored_mount != VENDOR_SANDBOX_MOUNT
        || !manifest
            .lockfile_digest
            .strip_prefix("sha256:")
            .is_some_and(integrity::digest_shape)
    {
        return Err(unavailable(
            &project.display,
            "invalid or unverified vendoring manifest",
        ));
    }
    for recorded in [
        &manifest.project_root,
        &manifest.vendored_dir,
        &manifest.sandbox_cargo_dir,
    ] {
        if !Path::new(recorded).is_absolute() || absolute(Path::new(recorded)).is_err() {
            return Err(unavailable(
                &project.display,
                "malformed informational manifest path",
            ));
        }
    }
    if manifest.lockfile_digest != graph.digest {
        return Err(Error::VendoringStale {
            path: project.display.to_string_lossy().into_owned(),
            current: graph.digest.clone(),
            expected: manifest.lockfile_digest,
        });
    }
    Ok(identity)
}

fn confirm_controls(
    project: &Dir,
    graph: &LockedGraph,
    observed: &ControlObservation,
    refuse_writer: bool,
    allow_legacy: bool,
) -> Result<()> {
    if let Some(state) = &observed.state {
        state.still_at_path()?;
    }
    let current = inspect_controls(project, graph, refuse_writer, allow_legacy)?;
    if current.state.is_some() != observed.state.is_some()
        || current.manifest_identity != observed.manifest_identity
        || current.resolver_identity != observed.resolver_identity
    {
        return Err(unavailable(
            &project.display,
            "vendoring control state changed during validation",
        ));
    }
    Ok(())
}

fn check_at(
    project: &Dir,
    graph: &LockedGraph,
    vendor: &Path,
    token: &CancellationToken,
    refuse_writer: bool,
) -> Result<VendoringReport> {
    control(token)?;
    let observed = inspect_controls(project, graph, refuse_writer, false)?;
    if observed.resolver.is_none() {
        graph.unchanged(project, token)?;
        confirm_controls(project, graph, &observed, refuse_writer, false)?;
        return Ok(graph.report(vendor));
    }
    let report = integrity::catalogue(graph, &Dir::open(vendor)?, token)?;
    graph.unchanged(project, token)?;
    confirm_controls(project, graph, &observed, refuse_writer, false)?;
    Ok(report)
}

/// Process-free, write-free full readiness from project-local state.
/// Informational manifest host paths are never used to select storage.
pub fn check(project: &Path, cancellation: &CancellationToken) -> Result<VendoringReport> {
    control(cancellation)?;
    let project = Dir::open(project)?;
    let graph = LockedGraph::read(&project, cancellation)?;
    let vendor = project
        .display
        .join(".kvist")
        .join(DEFAULT_VENDORED_DIRNAME);
    check_at(&project, &graph, &vendor, cancellation, true)
}

/// Validate locked packages and every declared file in an immutable vendor view.
/// This catalogue-only check needs no application manifest or generated resolver.
/// Git locks must name immutable revisions; source replacement binding is checked
/// separately by project readiness and resolver validation.
pub fn check_catalogue(
    project: &Path,
    vendor: &Path,
    cancellation: &CancellationToken,
) -> Result<VendoringReport> {
    control(cancellation)?;
    let project = Dir::open(project)?;
    let graph = LockedGraph::read(&project, cancellation)?;
    let report = integrity::catalogue(&graph, &Dir::open(vendor)?, cancellation)?;
    graph.unchanged(&project, cancellation)?;
    Ok(report)
}

/// Full readiness against a caller-owned private vendor copy and project controls.
///
/// Managed fences, manifest/lock binding and source replacement are checked without
/// inspecting the original vendor tree. The optional resolver is structurally
/// sanitized and retains the managed `/workspace/vendored` destination.
/// Unmanaged legacy catalogues may omit it only for crates.io/local locked graphs.
/// Partial managed state and missing locks always fail; nothing is written or run.
pub fn check_snapshot(
    project: &Path,
    vendor: &Path,
    cancellation: &CancellationToken,
) -> Result<(VendoringReport, Option<String>)> {
    control(cancellation)?;
    let project = Dir::open(project)?;
    let graph = LockedGraph::read(&project, cancellation)?;
    let observed = inspect_controls(&project, &graph, true, true)?;
    let report = integrity::catalogue(&graph, &Dir::open(vendor)?, cancellation)?;
    graph.unchanged(&project, cancellation)?;
    confirm_controls(&project, &graph, &observed, true, true)?;
    control(cancellation)?;
    Ok((report, observed.resolver))
}

/// Parse closed Cargo-vendor source tables, retargeting directory sources only.
/// Build/env/alias/credential keys, replacement cycles, and unsafe destinations fail.
pub fn resolver_config(config: &str, destination: &str) -> Result<String> {
    resolver::retarget(config, destination)
}

fn host_cargo(project: &Path, options: &ProvisionOptions) -> Result<PathBuf> {
    if options.timeout < Duration::from_secs(1)
        || options.timeout > Duration::from_secs(900)
        || !options.cargo.is_absolute()
    {
        return Err(unavailable(
            project,
            "Cargo must be absolute; timeout must be between 1 and 900 seconds",
        ));
    }
    absolute(&options.cargo)?;
    // Executable symlinks are explicit host authority, unlike untrusted vendor data.
    let cargo = std::fs::canonicalize(&options.cargo)
        .map_err(|e| io_error("resolve host Cargo executable", &options.cargo, e))?;
    absolute(&cargo)?;
    let metadata = std::fs::metadata(&cargo)
        .map_err(|e| io_error("inspect host Cargo executable", &cargo, e))?;
    if cargo.starts_with(project)
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o111 == 0
    {
        return Err(unavailable(
            project,
            "Cargo must be an executable regular host file outside the project",
        ));
    }
    // Preserve the selected executable's argv[0] (notably rustup's Cargo proxy).
    Ok(options.cargo.clone())
}

fn process_error(error: sav::Error) -> Error {
    let failure = match error {
        sav::Error::Cancelled => return Error::Cancelled,
        sav::Error::ProcessFailed { .. } => ProcessFailure::Exit,
        sav::Error::AttemptTimedOut { .. } | sav::Error::SupervisionExhausted { .. } => {
            ProcessFailure::Timeout
        }
        sav::Error::OutputLimitExceeded { .. } => ProcessFailure::OutputLimit,
        sav::Error::OutputStreamsRetained => ProcessFailure::RetainedStreams,
        sav::Error::Io { .. } => ProcessFailure::Io,
        _ => ProcessFailure::Supervision,
    };
    Error::Process { failure }
}

/// Perform one bounded, literal host Cargo-vendor acquisition into caller-owned staging.
///
/// `cargo_home` is an explicit cache capability, never read from an application
/// manifest. A fixed `/usr/bin/env` applies that one assignment without a shell.
/// This helper does not publish. Use inside [`provision_with`] for transactions.
pub fn populate(
    project: &Path,
    destination: &Path,
    options: &ProvisionOptions,
    cargo_home: Option<&Path>,
    cancellation: &CancellationToken,
) -> Result<String> {
    control(cancellation)?;
    let project = Dir::open(project)?;
    let graph = LockedGraph::read(&project, cancellation)?;
    let cargo = host_cargo(&project.display, options)?;
    let destination = absolute(destination)?;
    let parent = destination
        .parent()
        .ok_or_else(|| unavailable(&destination, "vendor destination has no parent"))?;
    let parent = Dir::open(parent)?;
    let name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| unavailable(&destination, "unsafe vendor destination"))?;
    if parent.exists(name)? {
        parent.child(name)?;
    }
    let mut arguments = vec!["vendor".into(), "--locked".into()];
    if options.offline {
        arguments.push("--offline".into());
    }
    arguments.push(destination.to_string_lossy().into_owned());
    let command = if let Some(home) = cargo_home {
        let home = Dir::open(home)?.display;
        let mut with_env = vec![
            format!("CARGO_HOME={}", home.display()),
            cargo.to_string_lossy().into_owned(),
        ];
        with_env.extend(arguments);
        CommandSpec::new("/usr/bin/env", with_env)
    } else {
        CommandSpec::new(cargo.to_string_lossy(), arguments)
    }
    .in_directory(&project.display);
    let policy = SupervisionPolicy {
        idle_timeout: options.timeout,
        attempt_timeout: Some(options.timeout),
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: OUTPUT_LIMIT,
    };
    let output = run_supervised_observed(&policy, &command, cancellation, &mut |_, _| Ok(()))
        .map_err(process_error)?;
    control(cancellation)?;
    graph.unchanged(&project, cancellation)?;
    let text = String::from_utf8(output.stdout)
        .map_err(|_| unavailable(&project.display, "Cargo-vendor stdout is not UTF-8"))?;
    // Cargo emits no source tables when the locked graph has no external packages.
    let text = if !graph.external() && text.trim().is_empty() {
        crate::vendoring::offline_cargo_config(&destination.to_string_lossy())
    } else {
        text
    };
    resolver::bind(&resolver::parse(&text)?, &graph, false)?;
    Ok(text)
}

/// Explicit host Cargo provisioning, with no retry or offline/online fallback.
pub fn provision(
    project: &Path,
    options: ProvisionOptions,
    cancellation: &CancellationToken,
) -> Result<VendoringReport> {
    control(cancellation)?;
    let project = absolute(project)?;
    host_cargo(&project, &options)?;
    provision_with(
        &project,
        options.vendored_dir.as_deref(),
        cancellation,
        |stage| populate(&project, stage, &options, None, cancellation),
    )
}

struct Operation<'a> {
    state: &'a Dir,
    file: File,
    identity: Identity,
}
impl<'a> Operation<'a> {
    fn acquire(state: &'a Dir) -> Result<Self> {
        let mut file = state.exclusive(OPERATION_LOCK)?;
        file.write_all(b"exclusive vendor operation; never steal a stale lock\n")
            .map_err(|e| {
                io_error(
                    "write operation lock",
                    &state.display.join(OPERATION_LOCK),
                    e,
                )
            })?;
        file.sync_all().map_err(|e| {
            io_error(
                "synchronize operation lock",
                &state.display.join(OPERATION_LOCK),
                e,
            )
        })?;
        state.sync()?;
        let identity = Identity::of(&file, &state.display.join(OPERATION_LOCK))?;
        Ok(Self {
            state,
            file,
            identity,
        })
    }
    fn intact(&self) -> Result<()> {
        self.state.still_at_path()?;
        let file = self.state.regular(OPERATION_LOCK, METADATA_LIMIT)?;
        if Identity::of(&file, &self.state.display.join(OPERATION_LOCK))? != self.identity
            || Identity::of(&self.file, &self.state.display.join(OPERATION_LOCK))? != self.identity
        {
            return Err(unavailable(
                &self.state.display,
                "operation lock changed during acquisition",
            ));
        }
        Ok(())
    }
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        if self.intact().is_ok() {
            let _ = std::fs::remove_file(self.state.path().join(OPERATION_LOCK));
            let _ = self.state.sync();
        }
    }
}

/// Transaction around a caller-owned once-only acquisition backend.
///
/// Complete matching material is reused without writes or callback invocation.
/// Empty external graphs publish standard empty state without acquisition.
/// Partial publication retains
/// an explicit `.kvist/vendor-publication` fence and owned old/new material.
pub fn provision_with(
    project: &Path,
    destination: Option<&Path>,
    cancellation: &CancellationToken,
    populate: impl FnOnce(&Path) -> Result<String>,
) -> Result<VendoringReport> {
    control(cancellation)?;
    let project = Dir::open(project)?;
    let graph = LockedGraph::read(&project, cancellation)?;
    let default = project
        .display
        .join(".kvist")
        .join(DEFAULT_VENDORED_DIRNAME);
    let vendor = match destination {
        Some(path) => absolute(path)?,
        None => default.clone(),
    };
    if vendor == project.display
        || project.display.starts_with(&vendor)
        || vendor == project.display.join(".kvist")
        || (vendor.starts_with(project.display.join(".kvist")) && vendor != default)
    {
        return Err(unavailable(
            &vendor,
            "vendor destination must not replace the project or state directory",
        ));
    }
    let mut has_manifest = false;
    if project.exists(".kvist")? {
        let state = project.child(".kvist")?;
        state_clear(&state)?;
        if state.exists(OPERATION_LOCK)? {
            return Err(Error::OperationLocked {
                path: state.display.join(OPERATION_LOCK),
            });
        }
        has_manifest = state.exists(VENDOR_MANIFEST_FILENAME)?;
    }
    if has_manifest {
        match check_at(&project, &graph, &vendor, cancellation, true) {
            Ok(report) => return Ok(report),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(Error::OperationLocked { path }) => return Err(Error::OperationLocked { path }),
            Err(Error::Io {
                operation,
                path,
                source,
            }) if source.kind() != std::io::ErrorKind::NotFound => {
                return Err(Error::Io {
                    operation,
                    path,
                    source,
                });
            }
            Err(_) => (),
        }
    }
    let state = project.ensure_child(".kvist")?;
    state_clear(&state)?;
    let operation = Operation::acquire(&state)?;
    graph.unchanged(&project, cancellation)?;
    if state.exists(VENDOR_MANIFEST_FILENAME)?
        && let Ok(report) = check_at(&project, &graph, &vendor, cancellation, false)
    {
        return Ok(report);
    }
    let parent_path = vendor
        .parent()
        .ok_or_else(|| unavailable(&vendor, "vendor destination has no parent"))?;
    let parent = Dir::open(parent_path)?;
    let name = vendor
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| unavailable(&vendor, "invalid vendor name"))?;
    if parent.exists(name)? {
        parent.child(name)?;
    }
    let staging = tempfile::Builder::new()
        .prefix(".vendor-stage-")
        .tempdir_in(parent.path())
        .map_err(|e| io_error("create private vendoring stage", &parent.display, e))?;
    let stage_name = staging
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| unavailable(&parent.display, "invalid private stage name"))?;
    let stage = parent.child(stage_name)?;
    let target = stage.display.join("vendor");
    let source_config = if graph.external() {
        populate(&target)?
    } else {
        std::fs::create_dir(stage.path().join("vendor"))
            .map_err(|e| io_error("create empty vendor stage", &target, e))?;
        crate::vendoring::offline_cargo_config(VENDOR_SANDBOX_MOUNT)
    };
    control(cancellation)?;
    if stage.entries()? != ["vendor"] {
        return Err(unavailable(
            &stage.display,
            "acquisition wrote outside its vendor target",
        ));
    }
    let config = resolver_config(&source_config, VENDOR_SANDBOX_MOUNT)?;
    resolver::bind(&resolver::parse(&config)?, &graph, true)?;
    integrity::catalogue(&graph, &stage.child("vendor")?, cancellation)?;
    graph.unchanged(&project, cancellation)?;
    operation.intact()?;
    // Preflight safe destinations before the first published byte changes.
    let cargo = state.ensure_child(SANDBOX_CARGO_CONFIG_DIRNAME)?;
    if cargo.exists("config.toml")? {
        cargo.regular("config.toml", METADATA_LIMIT)?;
    }
    if state.exists(VENDOR_MANIFEST_FILENAME)? {
        state.regular(VENDOR_MANIFEST_FILENAME, METADATA_LIMIT)?;
    }
    let manifest = VendorManifest {
        schema_version: VENDOR_SCHEMA_VERSION,
        generated_at_unix_secs: now_unix_secs(),
        project_root: project.display.to_string_lossy().into_owned(),
        lockfile_path: "Cargo.lock".into(),
        lockfile_digest: graph.digest.clone(),
        vendored_dir: vendor.to_string_lossy().into_owned(),
        sandbox_cargo_dir: cargo.display.to_string_lossy().into_owned(),
        sandbox_vendored_mount: VENDOR_SANDBOX_MOUNT.into(),
        verified: true,
    };
    let manifest = serde_json::to_vec_pretty(&manifest)
        .map_err(|_| unavailable(&project.display, "cannot serialize vendoring manifest"))?;
    // Synchronize every staged file and directory before publishing renames.
    synchronize_tree(&stage.child("vendor")?, cancellation, 0, &mut 0)?;
    control(cancellation)?;
    graph.unchanged(&project, cancellation)?;
    let result = Publication {
        stage: &stage,
        parent: &parent,
        name,
        state: &state,
        cancellation,
    }
    .commit(
        || {
            graph.unchanged(&project, cancellation)?;
            operation.intact()
        },
        || {
            graph.unchanged(&project, cancellation)?;
            cargo.atomic("config.toml", config.as_bytes())?;
            control(cancellation)?;
            graph.unchanged(&project, cancellation)?;
            state.atomic(VENDOR_MANIFEST_FILENAME, &manifest)?;
            control(cancellation)?;
            integrity::catalogue(&graph, &parent.child(name)?, cancellation)?;
            graph.unchanged(&project, cancellation)?;
            operation.intact()?;
            Ok(())
        },
    );
    if let Err(error) = result {
        if matches!(error, Error::Publication { .. }) {
            let _ = staging.keep();
        }
        return Err(error);
    }
    let mut report = graph.report(&vendor);
    report.vendored_dir = vendor.to_string_lossy().into_owned();
    Ok(report)
}

fn synchronize_tree(
    directory: &Dir,
    token: &CancellationToken,
    depth: usize,
    entries: &mut usize,
) -> Result<()> {
    if depth > 64 {
        return Err(unavailable(
            &directory.display,
            "staged tree depth exceeded",
        ));
    }
    for name in directory.entries()? {
        control(token)?;
        *entries += 1;
        if *entries > 100_000 {
            return Err(unavailable(
                &directory.display,
                "staged tree entry bound exceeded",
            ));
        }
        let m = std::fs::symlink_metadata(directory.path().join(&name))
            .map_err(|e| io_error("inspect staged file", &directory.display.join(&name), e))?;
        if m.is_dir() {
            synchronize_tree(&directory.child(&name)?, token, depth + 1, entries)?;
        } else {
            directory
                .regular(&name, 256 * 1024 * 1024)?
                .sync_all()
                .map_err(|e| {
                    io_error("synchronize staged file", &directory.display.join(&name), e)
                })?;
        }
    }
    directory.sync()
}
