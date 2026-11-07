use super::native;
use crate::{
    Error, ProcessFailure,
    fs::Dir,
    integrity::{LockedGraph, Origin, control},
};
use sav::CancellationToken;
use std::{collections::BTreeSet, path::Path};

/// Classify only actual observed offline/locked native Cargo metadata failures.
///
/// Cargo exit 101 and a bounded anchored missing-source chain tied to the lock
/// permit acquisition. All other failures retain their class/code without raw
/// diagnostics; this helper grants no process, source or network authority.
pub fn classify_metadata_failure(
    project: &Path,
    failure: ProcessFailure,
    exit_code: Option<i32>,
    stderr: &[u8],
    cancellation: &CancellationToken,
) -> Error {
    let fallback = || Error::NativeCargo {
        operation: "offline source validation",
        failure,
        diagnostic: native::diagnostic(&stderr[..stderr.len().min(64 * 1024)], failure, true),
        status: None,
        exit_code,
        source: None,
    };
    if failure != ProcessFailure::Exit || exit_code != Some(101) || stderr.len() > 64 * 1024 {
        return fallback();
    }
    let Ok(text) = std::str::from_utf8(stderr) else {
        return fallback();
    };
    let mut errors = text.lines().filter(|line| line.starts_with("error: "));
    let Some(primary) = errors.next() else {
        return fallback();
    };
    if errors.next().is_some() {
        return fallback();
    }
    if let Err(error) = control(cancellation) {
        return error;
    }
    let graph = match Dir::open(project)
        .and_then(|project| LockedGraph::read_native(&project, cancellation))
    {
        Ok(graph) => graph,
        Err(Error::Cancelled) => return Error::Cancelled,
        Err(_) => return fallback(),
    };
    let missing = evidence(primary, text, &graph);
    if missing.is_empty() {
        return fallback();
    }
    Error::VendoringIncomplete {
        path: graph.project.to_string_lossy().into_owned(),
        count: missing.len(),
        missing: missing.into_iter().collect::<Vec<_>>().join(", "),
    }
}

fn evidence(primary: &str, text: &str, graph: &LockedGraph) -> BTreeSet<String> {
    let mut missing = BTreeSet::new();
    if let Some(name) = primary.strip_prefix("error: no matching package named `")
        .and_then(|s| s.strip_suffix("` found"))
        && text.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("As a reminder, you're using offline mode (--offline)")
                || line == "note: offline mode (via `--offline`) can sometimes cause surprising resolution failures"
        })
    {
        for package in &graph.packages {
            if package.name == name && matches!(package.origin, Origin::Registry { .. }) {
                missing.insert(format!("{}-{}", package.name, package.version));
            }
        }
    }
    if let Some(label) = primary
        .strip_prefix("error: failed to download `")
        .and_then(|s| s.strip_suffix('`'))
        && text.lines().any(|line| {
            line.trim() == "attempting to make an HTTP request, but --offline was specified"
        })
    {
        for package in &graph.packages {
            if label == format!("{} v{}", package.name, package.version)
                && matches!(package.origin, Origin::Registry { .. })
            {
                missing.insert(format!("{}-{}", package.name, package.version));
            }
        }
    }
    if !primary.starts_with("error: failed to get `") {
        return missing;
    }
    let update = text.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("unable to update ")
            .or_else(|| line.strip_prefix("Unable to update "))
    });
    let Some(mut target) = update.and_then(|url| url::Url::parse(url).ok()) else {
        return missing;
    };
    let revision = target.fragment().map(str::to_owned);
    let query: Vec<_> = target
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    target.set_query(None);
    target.set_fragment(None);
    let checkout = text.lines().any(|line| {
        let Some(rest) = line.trim().strip_prefix("can't checkout from '") else {
            return false;
        };
        let Some((address, reason)) = rest.split_once("': ") else {
            return false;
        };
        url::Url::parse(address).is_ok_and(|url| url == target)
            && reason == "you are in the offline mode (--offline)"
    });
    let lookup = text.lines().any(|line| line.trim() ==
        "failed to lookup reference in preexisting repository, and can't check for updates in offline mode (--offline)");
    if !checkout && !lookup {
        return missing;
    }
    let mut candidates = Vec::new();
    let mut revisions = BTreeSet::new();
    for package in &graph.packages {
        let Origin::Git { url, selector } = &package.origin else {
            continue;
        };
        let Some((_, locked)) = package
            .source
            .as_deref()
            .and_then(|source| source.rsplit_once('#'))
        else {
            continue;
        };
        if url != target.as_str() || !query.is_empty() && query.as_slice() != selector.as_slice() {
            continue;
        }
        let display_matches = revision
            .as_deref()
            .is_some_and(|short| short.len() >= 7 && locked.starts_with(short));
        let reference_missing = lookup
            && text.lines().any(|line| {
                line.trim()
                    .starts_with(&format!("revspec '{locked}' not found"))
            });
        if display_matches && (checkout || reference_missing) {
            revisions.insert(locked);
            candidates.push(package);
        }
    }
    if revisions.len() == 1 {
        for package in candidates {
            missing.insert(format!("{}-{}", package.name, package.version));
        }
    }
    missing
}
