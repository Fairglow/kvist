//! Read-only, deterministic discovery of Kvist component layouts.
//!
//! Content validation is deliberately outside this module: `SPEC.md`,
//! `TODOS.yaml`, and `DOCS.md` formats are validated by their owning phases.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{KvistError, Result};
use crate::{config::DiscoveryLimits, filesystem::is_link_like};

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
/// Discovery refuses link-like roots and non-artifact descendants rather than
/// following them. It recursively examines only normal directories beneath
/// `component_root`, skips `IGNORED_DIRECTORY_NAMES`, and rejects layouts
/// beyond its limits rather than truncating them silently. The root is always
/// represented; descendants are represented only when at least one required
/// adjacent artifact exists.
pub fn discover(component_root: &Path) -> Result<Discovery> {
    discover_with_limits(component_root, DiscoveryLimits::default())
}

/// Discovers components using explicit resource limits.
///
/// This is used by project commands after loading `kvist.toml`; the public
/// [`discover`] API retains its deterministic default limits for direct users.
pub fn discover_with_limits(component_root: &Path, limits: DiscoveryLimits) -> Result<Discovery> {
    validate_component_root(component_root)?;

    let mut context = ScanContext {
        limits,
        scanned_directories: 0,
        components: Vec::new(),
    };
    scan_directory(
        component_root,
        Path::new(""),
        ScanPosition {
            depth: 0,
            is_root: true,
            parent_is_component: true,
            ordinary_intermediate: None,
        },
        &mut context,
    )?;
    context
        .components
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    Ok(Discovery {
        components: context.components,
    })
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

    if is_link_like(&metadata) {
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

struct ScanContext {
    limits: DiscoveryLimits,
    scanned_directories: usize,
    components: Vec<Component>,
}

struct ScanPosition {
    depth: usize,
    is_root: bool,
    parent_is_component: bool,
    ordinary_intermediate: Option<PathBuf>,
}

fn scan_directory(
    directory: &Path,
    relative_path: &Path,
    position: ScanPosition,
    context: &mut ScanContext,
) -> Result<()> {
    context.scanned_directories += 1;
    if context.scanned_directories > context.limits.max_directories {
        return Err(KvistError::ComponentDiscoveryDirectoryLimitExceeded {
            path: directory.to_path_buf(),
            max_directories: context.limits.max_directories,
        });
    }

    let component = inspect_component(directory, relative_path)?;
    let is_component = position.is_root || component.is_candidate();
    if !position.is_root && is_component && !position.parent_is_component {
        return Err(KvistError::ComponentDiscoveryHierarchyViolation {
            path: relative_path.to_path_buf(),
            intermediate: position
                .ordinary_intermediate
                .expect("ordinary intermediate is known for non-component parent"),
        });
    }
    if is_component {
        if context.components.len() == context.limits.max_components {
            return Err(KvistError::ComponentDiscoveryComponentLimitExceeded {
                path: directory.to_path_buf(),
                max_components: context.limits.max_components,
            });
        }
        context.components.push(component);
    }

    let mut children = Vec::new();
    let read_entries = fs::read_dir(directory).map_err(|source| KvistError::Io {
        operation: "read component directory",
        path: directory.to_path_buf(),
        source,
    })?;
    let mut entries = Vec::new();
    for entry in read_entries {
        let entry = entry.map_err(|source| KvistError::Io {
            operation: "read component directory entry",
            path: directory.to_path_buf(),
            source,
        })?;
        if entries.len() == context.limits.max_entries_per_directory {
            return Err(KvistError::ComponentDiscoveryEntriesPerDirectoryExceeded {
                path: directory.to_path_buf(),
                max_entries: context.limits.max_entries_per_directory,
            });
        }
        entries.push(entry);
    }
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let name = entry.file_name();
        if is_artifact_name(&name) {
            continue;
        }
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path).map_err(|source| KvistError::Io {
            operation: "inspect component directory entry",
            path: entry_path.clone(),
            source,
        })?;
        let file_type = metadata.file_type();

        if is_link_like(&metadata) {
            return Err(KvistError::ComponentDiscoveryLinkLikePath { path: entry_path });
        }
        if !file_type.is_dir() || is_ignored_directory(&name) {
            continue;
        }

        let child_relative_path = relative_path.join(name);
        if child_relative_path.as_os_str().as_encoded_bytes().len()
            > context.limits.max_relative_path_bytes
        {
            return Err(KvistError::ComponentDiscoveryRelativePathTooLong {
                path: child_relative_path,
                max_bytes: context.limits.max_relative_path_bytes,
            });
        }
        children.push((entry_path, child_relative_path));
    }
    children.sort_by(|left, right| left.1.cmp(&right.1));

    if position.depth == context.limits.max_depth && !children.is_empty() {
        return Err(KvistError::ComponentDiscoveryDepthExceeded {
            path: directory.to_path_buf(),
            max_depth: context.limits.max_depth,
        });
    }

    for (child_directory, child_relative_path) in children {
        let child_ordinary_intermediate = if is_component {
            None
        } else {
            Some(
                position
                    .ordinary_intermediate
                    .as_deref()
                    .unwrap_or(relative_path)
                    .to_path_buf(),
            )
        };
        scan_directory(
            &child_directory,
            &child_relative_path,
            ScanPosition {
                depth: position.depth + 1,
                is_root: false,
                parent_is_component: is_component,
                ordinary_intermediate: child_ordinary_intermediate,
            },
            context,
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
        Ok(metadata) if is_link_like(&metadata) => {
            Ok(ArtifactStatus::Invalid(InvalidArtifactKind::SymbolicLink))
        }
        Ok(metadata) if metadata.file_type().is_file() => Ok(ArtifactStatus::Present),
        Ok(metadata) if metadata.file_type().is_dir() => {
            Ok(ArtifactStatus::Invalid(InvalidArtifactKind::Directory))
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

fn is_artifact_name(name: &std::ffi::OsStr) -> bool {
    REQUIRED_ARTIFACTS
        .iter()
        .any(|artifact| name == std::ffi::OsStr::new(artifact.filename()))
}

const fn artifact_index(artifact: ComponentArtifact) -> usize {
    match artifact {
        ComponentArtifact::Specification => 0,
        ComponentArtifact::TaskQueue => 1,
        ComponentArtifact::Documentation => 2,
    }
}
