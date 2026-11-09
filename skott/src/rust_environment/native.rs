//! Native cache origins, with no source copying or project vendor state.

use super::*;

pub(super) const REGISTRY_DEST: &str = "/rust/cargo-registry";
pub(super) const GIT_DEST: &str = "/rust/cargo-git";

pub(super) fn roots(
    workspace: &Path,
    cancellation: &CancellationToken,
    issues: &mut Vec<String>,
) -> Result<(Option<PathBuf>, Option<PathBuf>)> {
    let home = match roetter::rust::cache::home() {
        Ok(home) => home,
        Err(error) => {
            issues.push(format!("native Cargo cache selection unavailable: {error}; installed/local tools remain usable"));
            return Ok((None, None));
        }
    };
    match host::operator_selector(&home, workspace) {
        Ok(true) => {}
        Ok(false) => {
            issues.push("native Cargo cache selector is inside the writable scope; no extra host origin is mounted. Select an operator-controlled CARGO_HOME outside the workspace".into());
            return Ok((None, None));
        }
        Err(error) => {
            issues.push(format!("native Cargo cache selector cannot be authorized: {error}; no extra origin is mounted. Installed/local tools remain usable"));
            return Ok((None, None));
        }
    }
    let candidates = match roetter::rust::cache::view(workspace, &home, cancellation) {
        Ok(view) => (view.registry, view.git),
        Err(roetter::Error::Cancelled) => return Err(Error::Cancelled),
        Err(error) => {
            issues.push(format!("native Cargo source catalogue unavailable: {error}; missing sources are diagnosed by offline Cargo, not acquired automatically. Run `skott --vendor --cwd DIR` explicitly on the host when needed"));
            // A lock/catalogue error is not a reason to hide installed tools or
            // existing data from a workspace whose lock is writable.
            (Some(home.join("registry")), Some(home.join("git")))
        }
    };
    crate::executor::check_cancelled(cancellation)?;
    Ok((
        approved_root(candidates.0, workspace, issues)?,
        approved_root(candidates.1, workspace, issues)?,
    ))
}

fn approved_root(
    candidate: Option<PathBuf>,
    workspace: &Path,
    issues: &mut Vec<String>,
) -> Result<Option<PathBuf>> {
    let Some(path) = candidate else {
        return Ok(None);
    };
    match inspect_path(&path) {
        Ok(metadata) if metadata.is_dir() => {
            if path.starts_with(workspace) || workspace.starts_with(&path) {
                issues.push(
                    "native Cargo data overlaps the writable workspace; no extra origin is mounted"
                        .into(),
                );
                Ok(None)
            } else {
                Ok(Some(path))
            }
        }
        Ok(_) => {
            issues.push(
                "native Cargo data is not an ordinary directory; no extra origin is mounted".into(),
            );
            Ok(None)
        }
        Err(error) => {
            if path.exists() {
                issues.push(format!("native Cargo data cannot be safely mounted: {error}; no extra origin is mounted"));
            }
            Ok(None)
        }
    }
}
