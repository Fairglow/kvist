//! Read-only, deterministic discovery of Kvist component layouts.
//!
//! Content validation is deliberately outside this module: `SPEC.md`,
//! `TODOS.yaml`, and `DOCS.md` formats are validated by their owning phases.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{KvistError, Result};

/// Maximum number of directory levels below the component root to traverse.
pub const MAX_COMPONENT_DEPTH: usize = 64;

/// Directory names that are outside the component hierarchy.
pub const IGNORED_DIRECTORY_NAMES: [&str; 5] = [".git", ".hg", ".jj", "node_modules", "target"];

/// The required adjacent artifacts for every component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComponentArtifact {
    /// The component's progressive-disclosure specification.
    Specification,
    /// The component's ordered implementation task queue.
    TaskQueue,
    /// The component's reverse-engineered compliance documentation.
    Documentation,
}

impl ComponentArtifact {
    /// Returns this artifact's required filename.
    pub const fn filename(self) -> &'static str {
        match self {
            Self::Specification => "SPEC.md",
            Self::TaskQueue => "TODOS.yaml",
            Self::Documentation => "DOCS.md",
        }
    }
}

const REQUIRED_ARTIFACTS: [ComponentArtifact; 3] = [
    ComponentArtifact::Specification,
    ComponentArtifact::TaskQueue,
    ComponentArtifact::Documentation,
];

/// The observed layout state of one adjacent component artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactStatus {
    /// A regular file exists at the expected path.
    Present,
    /// No filesystem entry exists at the expected path.
    Missing,
    /// An entry exists but cannot be used as an artifact.
    Invalid(InvalidArtifactKind),
}

/// Why an artifact path is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidArtifactKind {
    /// The expected file path is a directory.
    Directory,
    /// The expected file path is a symbolic link.
    SymbolicLink,
    /// The expected file path is another unsupported filesystem object.
    Other,
}

/// Aggregated layout state of a component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentStatus {
    /// All three required adjacent artifacts are regular files.
    Complete,
    /// One or more required artifacts are absent.
    Incomplete {
        /// Artifacts that are absent from the component directory.
        missing: Vec<ComponentArtifact>,
    },
    /// One or more artifact paths are not regular files.
    Invalid {
        /// Artifacts that have invalid filesystem entries.
        invalid: Vec<ComponentArtifact>,
    },
}

/// A component directory and the observed state of its adjacent artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// Path relative to the discovered component root; `.` identifies the root.
    pub relative_path: PathBuf,
    artifact_statuses: [ArtifactStatus; 3],
}

impl Component {
    /// Returns the observed state for one required artifact.
    pub fn artifact_status(&self, artifact: ComponentArtifact) -> ArtifactStatus {
        self.artifact_statuses[artifact_index(artifact)]
    }

    /// Returns the component's aggregate layout status.
    pub fn status(&self) -> ComponentStatus {
        let invalid = REQUIRED_ARTIFACTS
            .iter()
            .copied()
            .filter(|artifact| {
                matches!(self.artifact_status(*artifact), ArtifactStatus::Invalid(_))
            })
            .collect::<Vec<_>>();
        if !invalid.is_empty() {
            return ComponentStatus::Invalid { invalid };
        }

        let missing = REQUIRED_ARTIFACTS
            .iter()
            .copied()
            .filter(|artifact| self.artifact_status(*artifact) == ArtifactStatus::Missing)
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return ComponentStatus::Incomplete { missing };
        }

        ComponentStatus::Complete
    }

    fn is_candidate(&self) -> bool {
        self.artifact_statuses
            .iter()
            .any(|status| *status != ArtifactStatus::Missing)
    }
}

/// Deterministically discovered components below a component root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// Components in lexical order by their path relative to the root.
    pub components: Vec<Component>,
}

/// Discovers components rooted at `component_root`.
///
/// Discovery never follows symbolic links. It recursively examines only normal
/// directories beneath `component_root`, skips `IGNORED_DIRECTORY_NAMES`, and
/// rejects layouts deeper than `MAX_COMPONENT_DEPTH` rather than truncating
/// them silently. The root is always represented; descendants are represented
/// only when at least one required adjacent artifact exists.
pub fn discover(component_root: &Path) -> Result<Discovery> {
    validate_component_root(component_root)?;

    let mut components = Vec::new();
    scan_directory(component_root, Path::new(""), 0, true, &mut components)?;
    components.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    Ok(Discovery { components })
}

fn validate_component_root(component_root: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(component_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(KvistError::ComponentRootNotFound {
                path: component_root.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect component root",
                path: component_root.to_path_buf(),
                source,
            });
        }
    };
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Err(KvistError::ComponentRootIsSymlink {
            path: component_root.to_path_buf(),
        });
    }
    if !file_type.is_dir() {
        return Err(KvistError::ComponentRootNotDirectory {
            path: component_root.to_path_buf(),
        });
    }

    Ok(())
}

fn scan_directory(
    directory: &Path,
    relative_path: &Path,
    depth: usize,
    is_root: bool,
    components: &mut Vec<Component>,
) -> Result<()> {
    let component = inspect_component(directory, relative_path)?;
    if is_root || component.is_candidate() {
        components.push(component);
    }

    let mut children = Vec::new();
    let entries = fs::read_dir(directory).map_err(|source| KvistError::Io {
        operation: "read component directory",
        path: directory.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| KvistError::Io {
            operation: "read component directory entry",
            path: directory.to_path_buf(),
            source,
        })?;
        let file_type = entry.file_type().map_err(|source| KvistError::Io {
            operation: "inspect component directory entry",
            path: entry.path(),
            source,
        })?;

        if !file_type.is_dir() || is_ignored_directory(&entry.file_name()) {
            continue;
        }

        children.push((entry.path(), relative_path.join(entry.file_name())));
    }
    children.sort_by(|left, right| left.1.cmp(&right.1));

    if depth == MAX_COMPONENT_DEPTH && !children.is_empty() {
        return Err(KvistError::ComponentDiscoveryDepthExceeded {
            path: directory.to_path_buf(),
            max_depth: MAX_COMPONENT_DEPTH,
        });
    }

    for (child_directory, child_relative_path) in children {
        scan_directory(
            &child_directory,
            &child_relative_path,
            depth + 1,
            false,
            components,
        )?;
    }

    Ok(())
}

fn inspect_component(directory: &Path, relative_path: &Path) -> Result<Component> {
    let artifact_statuses = [
        inspect_artifact(directory, ComponentArtifact::Specification)?,
        inspect_artifact(directory, ComponentArtifact::TaskQueue)?,
        inspect_artifact(directory, ComponentArtifact::Documentation)?,
    ];

    Ok(Component {
        relative_path: if relative_path.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            relative_path.to_path_buf()
        },
        artifact_statuses,
    })
}

fn inspect_artifact(directory: &Path, artifact: ComponentArtifact) -> Result<ArtifactStatus> {
    let path = directory.join(artifact.filename());
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(ArtifactStatus::Present),
        Ok(metadata) if metadata.file_type().is_dir() => {
            Ok(ArtifactStatus::Invalid(InvalidArtifactKind::Directory))
        }
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Ok(ArtifactStatus::Invalid(InvalidArtifactKind::SymbolicLink))
        }
        Ok(_) => Ok(ArtifactStatus::Invalid(InvalidArtifactKind::Other)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ArtifactStatus::Missing),
        Err(source) => Err(KvistError::Io {
            operation: "inspect component artifact",
            path,
            source,
        }),
    }
}

fn is_ignored_directory(name: &std::ffi::OsStr) -> bool {
    IGNORED_DIRECTORY_NAMES
        .iter()
        .any(|ignored_name| name == std::ffi::OsStr::new(ignored_name))
}

const fn artifact_index(artifact: ComponentArtifact) -> usize {
    match artifact {
        ComponentArtifact::Specification => 0,
        ComponentArtifact::TaskQueue => 1,
        ComponentArtifact::Documentation => 2,
    }
}
