use super::{
    CacheView, ensure_root, inspect, native, optional, optional_root, receipt_matches, selected,
    view,
};
use crate::{
    Error, Result,
    fs::Dir,
    integrity::{LockedGraph, Origin, control},
    rust::ProvisionOptions,
    unavailable,
    vendoring::VendoringReport,
};
use sav::CancellationToken;
use std::path::{Path, PathBuf};

/// Source-only runtime admission, never a receipt-only readiness assertion.
#[derive(Debug, Clone)]
pub enum SourceAdmission {
    /// Current process-free catalogue evidence; its report may still be incomplete.
    Catalogue(VendoringReport),
    /// Actual offline Cargo consumption must resolve the source identities.
    CargoOfflineRequired {
        /// Historical matching lock admission only, not current package presence.
        recorded_lock_admission: bool,
    },
    /// Runtime Cargo owns resolving the writable/absent lock, still offline.
    NoLock,
}

/// Safe operator-selected runtime roots, distinct from current CLI readiness.
#[derive(Debug, Clone)]
pub struct SourceView {
    /// Operator-selected administrative home, not a whole-home mount grant.
    pub cargo_home: PathBuf,
    /// Native registry data root, if it exists.
    pub registry: Option<PathBuf>,
    /// Native Git data root, if it exists.
    pub git: Option<PathBuf>,
    /// Exact current lock identity when a project lock exists.
    pub lockfile_digest: Option<String>,
    /// Evidence and obligations for the actual offline runtime consumer.
    pub admission: SourceAdmission,
}

/// Select only native data roots with an explicit offline-resolution obligation.
/// Does not run Cargo, create paths, require a lock, or turn receipts into readiness.
pub fn sources(
    project: &Path,
    home: &Path,
    cancellation: &CancellationToken,
) -> Result<SourceView> {
    control(cancellation)?;
    let project = Dir::open(project)?;
    let home_path = selected(home)?;
    let parent = Dir::open(
        home_path
            .parent()
            .ok_or_else(|| unavailable(&home_path, "Cargo home has no parent"))?,
    )?;
    let name = home_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| unavailable(&home_path, "unsafe Cargo home"))?;
    let home = optional(&parent, name)?;
    let registry = home
        .as_ref()
        .map(|h| optional_root(h, "registry"))
        .transpose()?
        .flatten()
        .map(|d| d.display);
    let git = home
        .as_ref()
        .map(|h| optional_root(h, "git"))
        .transpose()?
        .flatten()
        .map(|d| d.display);
    let (lockfile_digest, admission) = if project.exists("Cargo.lock")? {
        let graph = LockedGraph::read_native(&project, cancellation)?;
        let admission = match &home {
            Some(home) => match inspect(&graph, home, cancellation) {
                Ok(view) => SourceAdmission::Catalogue(view.report),
                Err(Error::UnsupportedCacheSource { .. }) => {
                    let recorded_lock_admission = match optional(&project, ".kvist")? {
                        Some(state) => receipt_matches(&state, &graph)?,
                        None => false,
                    };
                    SourceAdmission::CargoOfflineRequired {
                        recorded_lock_admission,
                    }
                }
                Err(error) => return Err(error),
            },
            None => {
                let mut report = graph.report(&home_path);
                report.missing_dependencies = graph
                    .packages
                    .iter()
                    .filter(|p| !matches!(p.origin, Origin::Local))
                    .map(|p| format!("{}-{}", p.name, p.version))
                    .collect();
                SourceAdmission::Catalogue(report)
            }
        };
        graph.unchanged(&project, cancellation)?;
        (Some(graph.digest), admission)
    } else {
        (None, SourceAdmission::NoLock)
    };
    if let Some(home) = home {
        home.still_at_path()?;
    }
    parent.still_at_path()?;
    project.still_at_path()?;
    control(cancellation)?;
    Ok(SourceView {
        cargo_home: home_path,
        registry,
        git,
        lockfile_digest,
        admission,
    })
}

/// Explicit native CLI admission. Canonical warm catalogues start no process;
/// other supported sources use bounded actual offline/locked Cargo metadata.
/// Missing sources remain a missing report, never a receipt-only success.
pub fn admit(
    project: &Path,
    home: &Path,
    options: &ProvisionOptions,
    cancellation: &CancellationToken,
) -> Result<CacheView> {
    match view(project, home, cancellation) {
        Err(Error::UnsupportedCacheSource { .. }) => {
            let project = Dir::open(project)?;
            let graph = LockedGraph::read_native(&project, cancellation)?;
            let home = Dir::open(&selected(home)?)?;
            if let Some(missing) = native::directory_missing(&graph, &home)? {
                return Ok(missing);
            }
            native::validate(&project, &graph, &home, options, cancellation)
        }
        result => result,
    }
}

/// Configured admission from actual bounded private-admin Cargo metadata stdout.
///
/// Callback execution authority and offline/locked supervision are caller-owned.
/// Only fixed native sandbox endpoints are mapped to selected host data roots;
/// no host Cargo, config fallback, or portable directory export runs here.
pub fn admit_with(
    project: &Path,
    home: &Path,
    cancellation: &CancellationToken,
    validate: impl FnOnce(&Path) -> Result<Vec<u8>>,
) -> Result<CacheView> {
    match view(project, home, cancellation) {
        Err(Error::UnsupportedCacheSource { .. }) => {
            let project = Dir::open(project)?;
            let graph = LockedGraph::read_native(&project, cancellation)?;
            let home = Dir::open(&selected(home)?)?;
            if let Some(missing) = native::directory_missing(&graph, &home)? {
                return Ok(missing);
            }
            ensure_root(&home, "registry")?;
            ensure_root(&home, "git")?;
            graph.unchanged(&project, cancellation)?;
            let bytes = validate(&home.display)?;
            native::validate_bytes(&project, &graph, &home, &bytes, true, cancellation)
        }
        result => result,
    }
}

/// Normalize only private-home aliases from the independently approved request.
/// Input is actual successful native metadata; no paths are inferred from JSON.
/// Subsequent admission still validates exact lock sources and actual host files.
pub fn normalize_metadata_paths(
    project: &Path,
    cargo_home: &Path,
    stdout: &[u8],
    cancellation: &CancellationToken,
) -> Result<Vec<u8>> {
    control(cancellation)?;
    let private = selected(cargo_home)?;
    let project = Dir::open(project)?;
    let graph = LockedGraph::read_native(&project, cancellation)?;
    if stdout.len() > graph.limits.max_output_bytes {
        return Err(unavailable(
            &project.display,
            "native Cargo metadata output limit exceeded",
        ));
    }
    let mut metadata: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|_| unavailable(&project.display, "native Cargo metadata is malformed"))?;
    let packages = metadata
        .get_mut("packages")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| {
            unavailable(
                &project.display,
                "native Cargo metadata package array absent",
            )
        })?;
    if packages.len() > graph.limits.max_packages {
        return Err(unavailable(
            &project.display,
            "native Cargo metadata package limit exceeded",
        ));
    }
    for package in packages {
        control(cancellation)?;
        let source = match package.get("source") {
            Some(serde_json::Value::Null) => continue,
            Some(serde_json::Value::String(source)) => source,
            _ => {
                return Err(unavailable(
                    &project.display,
                    "native Cargo metadata source malformed",
                ));
            }
        };
        let (suffix, fixed) = if source.starts_with("registry+") || source.starts_with("sparse+") {
            ("registry/src", Path::new("/workspace/cargo-registry/src"))
        } else if source.starts_with("git+") {
            ("git/checkouts", Path::new("/workspace/cargo-git/checkouts"))
        } else {
            return Err(super::unsupported(
                &project.display,
                "native Cargo metadata source kind is unsupported",
            ));
        };
        let path = package
            .get("manifest_path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                unavailable(
                    &project.display,
                    "native Cargo metadata manifest path absent",
                )
            })?;
        let path = Path::new(path);
        let alias = private.join(suffix);
        let relative = path.strip_prefix(&alias).or_else(|_| path.strip_prefix(fixed))
                    .map_err(|_| super::unsupported(&private, "native metadata path is neither the exact request home alias nor its declared native endpoint"))?;
        if relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
            || relative.components().count() < 3
            || relative.components().count() > graph.limits.max_depth
            || relative.file_name().is_none_or(|name| name != "Cargo.toml")
        {
            return Err(super::unsupported(
                &private,
                "native metadata alias path is unsafe or unbounded",
            ));
        }
        package["manifest_path"] =
            serde_json::Value::String(fixed.join(relative).to_string_lossy().into_owned());
    }
    graph.unchanged(&project, cancellation)?;
    serde_json::to_vec(&metadata)
        .map_err(|_| unavailable(&project.display, "cannot normalize native metadata"))
}
