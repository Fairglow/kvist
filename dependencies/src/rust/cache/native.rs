use super::{CacheView, roots, unsupported};
use crate::{
    Error, ProcessFailure, Result,
    fs::Dir,
    integrity::{LockedGraph, Origin, control},
    rust::{ProvisionOptions, host_cargo},
    unavailable,
};
use sav::{CommandSpec, OutputStream, SupervisionPolicy, run_supervised_observed};
use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf, time::Duration};

pub(super) fn diagnostic(bytes: &[u8], failure: ProcessFailure, offline: bool) -> &'static str {
    if failure != ProcessFailure::Exit {
        return match failure {
            ProcessFailure::Timeout => {
                "Cargo exceeded its time budget; inspect retained cache progress and retry explicitly with an appropriate budget"
            }
            ProcessFailure::OutputLimit => {
                "Cargo exceeded the bounded output budget; inspect operator source configuration or explicitly adjust project policy"
            }
            ProcessFailure::Io => {
                "Cargo could not start or read its streams; check the selected executable and host runtime"
            }
            ProcessFailure::RetainedStreams => {
                "Cargo descendants retained output streams; inspect interrupted acquisition before retrying"
            }
            _ => "native Cargo supervision failed; inspect the observed execution before retrying",
        };
    }
    let text = String::from_utf8_lossy(bytes);
    if text.contains("lock file")
        && (text.contains("needs to be updated") || text.contains("--locked"))
    {
        "Cargo requires a different lock; reconcile Cargo.toml/configuration with the approved existing Cargo.lock before retrying"
    } else if text.contains("failed to get successful HTTP response")
        || text.contains("failed to download")
        || text.contains("Could not resolve")
    {
        "source acquisition failed; inspect the operator-selected source/network configuration and retained native cache, then retry explicitly"
    } else if text.contains("checksum") {
        "Cargo rejected native source checksums; inspect and repair the affected operator-owned cache before retrying"
    } else {
        match failure {
            ProcessFailure::Timeout => {
                "Cargo exceeded its time budget; inspect retained cache progress and retry explicitly with an appropriate budget"
            }
            ProcessFailure::OutputLimit => {
                "Cargo exceeded the bounded output budget; inspect operator source configuration or explicitly adjust project policy"
            }
            ProcessFailure::Io => {
                "Cargo could not start or read its streams; check the selected executable and host runtime"
            }
            ProcessFailure::RetainedStreams => {
                "Cargo descendants retained output streams; inspect interrupted acquisition before retrying"
            }
            _ if offline => {
                "offline Cargo validation/acquisition failed; inspect the existing lock, native cache and operator source configuration before retrying"
            }
            _ => {
                "Cargo acquisition failed; inspect the existing lock, retained cache and operator source/network configuration before retrying"
            }
        }
    }
}

pub(super) fn run(
    project: &Dir,
    graph: &LockedGraph,
    home: &Dir,
    options: &ProvisionOptions,
    argv: Vec<String>,
    operation: &'static str,
    token: &sav::CancellationToken,
) -> Result<Vec<u8>> {
    control(token)?;
    let cargo = host_cargo(&project.display, options)?;
    let mut arguments = vec![
        format!("CARGO_HOME={}", home.display.display()),
        cargo.to_string_lossy().into_owned(),
    ];
    arguments.extend(argv);
    let command = CommandSpec::new("/usr/bin/env", arguments).in_directory(&project.display);
    let timeout = options
        .timeout
        .min(Duration::from_secs(graph.limits.timeout_secs));
    let policy = SupervisionPolicy {
        idle_timeout: timeout,
        attempt_timeout: Some(timeout),
        detect_loops: false,
        max_retries: 0,
        max_output_bytes: graph.limits.max_output_bytes,
    };
    let mut stderr = Vec::new();
    let result = run_supervised_observed(&policy, &command, token, &mut |stream, bytes| {
        if stream == OutputStream::Stderr {
            let remaining = graph.limits.max_output_bytes.saturating_sub(stderr.len());
            stderr.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        }
        Ok(())
    });
    let output = result.map_err(|error| {
        let status = match &error {
            sav::Error::ProcessFailed { status } => Some(*status),
            _ => None,
        };
        let (failure, source) = match error {
            sav::Error::Io { source, .. } => (ProcessFailure::Io, Some(source)),
            other => match super::super::process_error(other) {
                Error::Process { failure } => (failure, None),
                other => return other,
            },
        };
        if operation == "offline source validation" {
            let mut error = super::classify_metadata_failure(
                &project.display,
                failure,
                status.and_then(|s| s.code()),
                &stderr,
                token,
            );
            if let Error::NativeCargo {
                status: recorded_status,
                source: recorded_source,
                ..
            } = &mut error
            {
                *recorded_status = status;
                *recorded_source = source;
            }
            return error;
        }
        Error::NativeCargo {
            operation,
            failure,
            diagnostic: diagnostic(
                &stderr,
                failure,
                options.offline || operation == "offline source validation",
            ),
            status,
            exit_code: status.and_then(|s| s.code()),
            source,
        }
    })?;
    control(token)?;
    graph.unchanged(project, token)?;
    home.still_at_path()?;
    Ok(output.stdout)
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<MetadataPackage>,
}
#[derive(Deserialize)]
struct MetadataPackage {
    name: String,
    version: String,
    source: Option<String>,
    manifest_path: PathBuf,
}

pub(super) fn validate(
    project: &Dir,
    graph: &LockedGraph,
    home: &Dir,
    options: &ProvisionOptions,
    token: &sav::CancellationToken,
) -> Result<CacheView> {
    let output = run(
        project,
        graph,
        home,
        options,
        vec![
            "metadata".into(),
            "--offline".into(),
            "--locked".into(),
            "--format-version".into(),
            "1".into(),
            "--all-features".into(),
        ],
        "offline source validation",
        token,
    );
    let bytes = match output {
        Ok(bytes) => bytes,
        Err(Error::VendoringIncomplete { .. }) => {
            return missing(graph, home);
        }
        Err(error) => return Err(error),
    };
    validate_bytes(project, graph, home, &bytes, false, token)
}

pub(super) fn missing(graph: &LockedGraph, home: &Dir) -> Result<CacheView> {
    let mut view = roots(graph, home)?;
    view.report.missing_dependencies = graph
        .packages
        .iter()
        .filter(|p| !matches!(p.origin, Origin::Local))
        .map(|p| format!("{}-{}", p.name, p.version))
        .collect();
    Ok(view)
}

pub(super) fn directory_missing(graph: &LockedGraph, home: &Dir) -> Result<Option<CacheView>> {
    for (required, root, children) in [
        (
            graph
                .packages
                .iter()
                .any(|p| matches!(p.origin, Origin::Registry { .. })),
            "registry",
            &["index", "cache", "src"][..],
        ),
        (
            graph
                .packages
                .iter()
                .any(|p| matches!(p.origin, Origin::Git { .. })),
            "git",
            &["db", "checkouts"][..],
        ),
    ] {
        if !required {
            continue;
        }
        let Some(root) = super::optional_root(home, root)? else {
            return missing(graph, home).map(Some);
        };
        for child in children {
            if super::optional(&root, child)?.is_none() {
                return missing(graph, home).map(Some);
            }
        }
        root.still_at_path()?;
    }
    home.still_at_path()?;
    Ok(None)
}

pub(super) fn validate_bytes(
    project: &Dir,
    graph: &LockedGraph,
    home: &Dir,
    bytes: &[u8],
    sandbox: bool,
    token: &sav::CancellationToken,
) -> Result<CacheView> {
    control(token)?;
    graph.unchanged(project, token)?;
    if bytes.len() > graph.limits.max_output_bytes {
        return Err(unavailable(
            &project.display,
            "native Cargo metadata output limit exceeded",
        ));
    }
    let view = roots(graph, home)?;
    let metadata: Metadata = serde_json::from_slice(bytes).map_err(|_| {
        unavailable(
            &project.display,
            "native Cargo metadata is malformed or unsupported",
        )
    })?;
    if metadata.packages.len() > graph.limits.max_packages {
        return Err(unavailable(
            &project.display,
            "native Cargo metadata package limit exceeded",
        ));
    }
    let locked: BTreeSet<_> = graph
        .packages
        .iter()
        .map(|p| (p.name.as_str(), p.version.as_str(), p.source.as_deref()))
        .collect();
    let mut observed = BTreeSet::new();
    for package in &metadata.packages {
        control(token)?;
        let identity = (
            package.name.as_str(),
            package.version.as_str(),
            package.source.as_deref(),
        );
        if !locked.contains(&identity) {
            return Err(unsupported(
                &project.display,
                "native Cargo resolved an identity absent from the exact approved lock",
            ));
        }
        if !observed.insert(identity) {
            return Err(unavailable(
                &project.display,
                "native Cargo metadata duplicates a package identity",
            ));
        }
        let Some(source) = package.source.as_deref() else {
            continue;
        };
        let (native, sandbox_base, data_base) =
            if source.starts_with("registry+") || source.starts_with("sparse+") {
                (
                    "registry/src",
                    "/workspace/cargo-registry/src",
                    view.registry.as_ref().map(|p| p.join("src")),
                )
            } else if source.starts_with("git+") {
                (
                    "git/checkouts",
                    "/workspace/cargo-git/checkouts",
                    view.git.as_ref().map(|p| p.join("checkouts")),
                )
            } else {
                return Err(unsupported(
                    &home.display,
                    "native Cargo metadata source kind is unsupported",
                ));
            };
        let data_base = data_base.ok_or_else(|| {
            unsupported(
                &home.display,
                "native metadata requires an absent data root",
            )
        })?;
        let base = if sandbox {
            PathBuf::from(sandbox_base)
        } else {
            home.display.join(native)
        };
        let relative = package.manifest_path.strip_prefix(&base).or_else(|error| {
            if sandbox {
                Err(error)
            } else {
                package.manifest_path.strip_prefix(&data_base)
            }
        }).map_err(|_| {
            unsupported(&home.display, "native Cargo resolved external material outside the selected native data roots; directory exports/configuration are not native cache")
        })?;
        if relative.components().count() < 3
            || relative.components().count() > graph.limits.max_depth
            || package
                .manifest_path
                .file_name()
                .is_none_or(|name| name != "Cargo.toml")
        {
            return Err(unsupported(
                &home.display,
                "native Cargo manifest path does not identify a bounded native source location",
            ));
        }
        let actual = data_base.join(relative);
        let parent = actual
            .parent()
            .ok_or_else(|| unavailable(&home.display, "native package manifest has no parent"))?;
        Dir::open(parent)?.regular("Cargo.toml", graph.limits.max_metadata_bytes)?;
    }
    for package in &graph.packages {
        if !matches!(package.origin, Origin::Local)
            && !observed.contains(&(
                package.name.as_str(),
                package.version.as_str(),
                package.source.as_deref(),
            ))
        {
            return Err(unsupported(
                &project.display,
                "native metadata did not validate every external locked identity; full locked-source mapping is unavailable",
            ));
        }
    }
    graph.unchanged(project, token)?;
    home.still_at_path()?;
    Ok(view)
}
