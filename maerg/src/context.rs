//! Context resolution for command execution.
//!
//! Kvist commands resolve *where they are acting* before doing anything:
//!
//! - **Project root** — an explicit `PROJECT_DIR` argument, or the nearest
//!   directory at or above the current working directory that contains a
//!   regular `kvist.toml` (the same "walk upward" convention as Git).
//! - **Active component** — an explicit `COMPONENT_DIR` argument, or the
//!   nearest discovered Kvist component containing the current working
//!   directory.
//!
//! Resolution is pure filesystem derivation on every invocation: nothing is
//! cached, nothing is created, and an explicit argument always wins.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use crate::{KvistError, Result, config, project_state};

/// Name of the project marker file that identifies a Kvist project root.
pub const PROJECT_MARKER: &str = "kvist.toml";

/// Maximum directory levels searched upward for [`PROJECT_MARKER`].
pub const MAX_PROJECT_SEARCH_DEPTH: usize = 32;

/// The resolved Kvist project root for one command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectContext {
    /// Absolute project root containing `kvist.toml`.
    pub project_dir: PathBuf,
}

impl ProjectContext {
    /// Resolves the project root from an explicit directory argument, falling
    /// back to walking upward from the current working directory.
    pub fn resolve(explicit: Option<&Path>) -> Result<Self> {
        match explicit {
            Some(path) => Ok(Self {
                project_dir: current_dir()?.join(path),
            }),
            None => {
                let start = current_dir()?;
                let project_dir = Self::find_root(&start)?;
                Ok(Self { project_dir })
            }
        }
    }

    /// Walks upward from `start` for the nearest directory containing a
    /// regular (non-symlink) `kvist.toml`, bounded by [`MAX_PROJECT_SEARCH_DEPTH`].
    pub fn find_root(start: &Path) -> Result<PathBuf> {
        let mut dir = start.to_path_buf();
        for _ in 0..=MAX_PROJECT_SEARCH_DEPTH {
            let marker = dir.join(PROJECT_MARKER);
            match fs::symlink_metadata(&marker) {
                Ok(metadata) if metadata.file_type().is_file() => return Ok(dir),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(KvistError::Io {
                        operation: "inspect Kvist project marker",
                        path: marker,
                        source,
                    });
                }
            }
            let Some(parent) = dir.parent() else {
                break;
            };
            dir = parent.to_path_buf();
        }
        Err(KvistError::NotInProject {
            searched_from: start.to_path_buf(),
        })
    }

    /// Joins a component-root-relative component path against the project's
    /// component root, yielding an absolute path into the project.
    pub fn component_path(&self, component: &Path) -> Result<PathBuf> {
        let configuration = config::load(&self.project_dir)?;
        Ok(self
            .project_dir
            .join(configuration.component_root)
            .join(component))
    }

    /// Resolves the component path relative to the component root, from an
    /// explicit argument or from the current working directory.
    pub fn resolve_component(&self, explicit: Option<&Path>) -> Result<PathBuf> {
        let configuration = config::load(&self.project_dir)?;
        let component_root = configuration.component_root.clone();
        if let Some(explicit) = explicit {
            return normalize_component_argument(explicit, &self.project_dir, &component_root);
        }
        let cwd = current_dir()?;
        let rel = cwd.strip_prefix(&self.project_dir).map_err(|_| {
            KvistError::ComponentNotInsideProject {
                cwd: cwd.clone(),
                project_dir: self.project_dir.clone(),
            }
        })?;
        let rel_to_root = if component_root == Path::new(".") {
            rel.to_path_buf()
        } else {
            rel.strip_prefix(&component_root)
                .map(Path::to_path_buf)
                .ok()
                .filter(|path| !path.as_os_str().is_empty())
                .ok_or_else(|| KvistError::ComponentNotInsideProject {
                    cwd: cwd.clone(),
                    project_dir: self.project_dir.clone(),
                })?
        };
        let inspection = project_state::inspect(&self.project_dir)?;
        let mut best: Option<&Path> = None;
        for component in &inspection.components {
            let path = &component.path;
            if (path.as_path() == rel_to_root.as_path() || rel_to_root.starts_with(path.as_path()))
                && best
                    .as_ref()
                    .is_none_or(|current| path.as_os_str().len() > current.as_os_str().len())
            {
                best = Some(path);
            }
        }
        if best.is_none()
            && (rel_to_root.as_os_str().is_empty() || rel_to_root == Path::new("."))
            && inspection
                .components
                .iter()
                .any(|component| component.path == Path::new("."))
        {
            best = Some(Path::new("."));
        }
        match best {
            Some(path) => Ok(path.to_path_buf()),
            None => Err(KvistError::ComponentNotResolvable {
                cwd,
                project_dir: self.project_dir.clone(),
                known: join_component_list(
                    &inspection
                        .components
                        .iter()
                        .map(|component| component.path.to_string_lossy().into_owned())
                        .collect::<Vec<_>>(),
                ),
            }),
        }
    }
}

/// Joins a component list for error hints, with a stable fallback.
fn join_component_list(components: &[String]) -> String {
    if components.is_empty() {
        "(none discovered; run `kvist component new <DIR>` to create one)".to_owned()
    } else {
        components.join(", ")
    }
}

/// Builds the concrete next step for a component in the given inspected state.
pub fn component_state_hint(state: project_state::ComponentState, component: &Path) -> String {
    let display = component.display().to_string();
    match state {
        project_state::ComponentState::Current => {
            format!("the component is current; run `kvist task next {display}`")
        }
        project_state::ComponentState::Stale => format!(
            "review the changed documents listed by `kvist status` (for example `git diff HEAD -- {display}` under Git), then run `kvist component accept {display}`"
        ),
        project_state::ComponentState::Blocked => format!(
            "resolve the blocked tasks listed by `kvist status`, for example `kvist task transition {display} <TASK_ID> pending --reason <why>` or `kvist task finalize {display} <TASK_ID> <ATTEMPT_ID> accept`"
        ),
        project_state::ComponentState::Invalid => {
            format!("run `kvist component validate {display}` to see which document is invalid")
        }
        project_state::ComponentState::Missing => {
            format!("run `kvist component new {display}` to create the missing document templates")
        }
        project_state::ComponentState::UnsupportedVersion => {
            format!("upgrade the artifacts in {display} to a supported version")
        }
    }
}

/// Builds the hint for a component path that was not discovered.
pub fn component_not_found_hint(component: &Path) -> String {
    format!(
        "`{}` is not a discovered component; run `kvist tree` to list the components",
        component.display()
    )
}

/// Resolves the current working directory with a descriptive failure.
fn current_dir() -> Result<PathBuf> {
    std::env::current_dir().map_err(|source| KvistError::Io {
        operation: "determine current working directory",
        path: PathBuf::from("."),
        source,
    })
}

/// Normalizes an explicit component argument to a component-root-relative
/// path. Relative arguments are taken as-is; absolute arguments must be
/// inside the project. The result is `.` for the root component.
pub fn normalize_component_argument(
    path: &Path,
    project_dir: &Path,
    component_root: &Path,
) -> Result<PathBuf> {
    let project_relative = if path.is_absolute() {
        path.strip_prefix(project_dir)
            .map_err(|_| KvistError::TaskComponentPathInvalid {
                path: path.to_path_buf(),
            })?
    } else {
        path
    };
    let component_relative = project_relative
        .strip_prefix(component_root)
        .unwrap_or(project_relative);
    if component_relative.as_os_str().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        normalize_component_path(component_relative)
    }
}

/// Validates a component-root-relative path: it must be `.` or contain only
/// normal path segments.
pub fn normalize_component_path(path: &Path) -> Result<PathBuf> {
    if path == Path::new(".") {
        return Ok(PathBuf::from("."));
    }
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(KvistError::TaskComponentPathInvalid {
            path: path.to_path_buf(),
        });
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    fn project_at(dir: &Path) {
        fs::write(dir.join("kvist.toml"), "schema_version = 1\n").unwrap();
    }

    #[test]
    fn find_root_finds_marker_in_ancestor() {
        let outer = tempdir().unwrap();
        let project = outer.path().join("project");
        fs::create_dir_all(project.join("a/b")).unwrap();
        project_at(&project);
        let found = ProjectContext::find_root(&project.join("a/b")).unwrap();
        assert_eq!(found, project);
    }

    #[test]
    fn find_root_finds_marker_in_start() {
        let dir = tempdir().unwrap();
        project_at(dir.path());
        let found = ProjectContext::find_root(dir.path()).unwrap();
        assert_eq!(found, dir.path());
    }

    #[test]
    fn find_root_reports_not_in_project() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a")).unwrap();
        match ProjectContext::find_root(dir.path().join("a").as_path()) {
            Err(KvistError::NotInProject { searched_from }) => {
                assert_eq!(searched_from, dir.path().join("a"))
            }
            other => panic!("expected NotInProject, got {other:?}"),
        }
    }

    #[test]
    fn find_root_ignores_symlink_marker() {
        let outer = tempdir().unwrap();
        fs::create_dir_all(outer.path().join("project")).unwrap();
        fs::write(outer.path().join("real.toml"), "schema_version = 1\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            outer.path().join("real.toml"),
            outer.path().join("project/kvist.toml"),
        )
        .unwrap();
        match ProjectContext::find_root(outer.path().join("project").as_path()) {
            Err(KvistError::NotInProject { .. }) => {}
            other => panic!("expected NotInProject, got {other:?}"),
        }
    }

    #[test]
    fn resolve_explicit_path_joins_current_directory() {
        let dir = tempdir().unwrap();
        let before = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = ProjectContext::resolve(Some(Path::new("sub")));
        let _ = std::env::set_current_dir(before);
        assert_eq!(result.unwrap().project_dir, dir.path().join("sub"));
    }
}
