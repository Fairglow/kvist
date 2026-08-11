//! Safe initialization of a Kvist project root.

use std::{
    collections::BTreeSet,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use tempfile::NamedTempFile;

use crate::{
    KvistError, Result,
    artifacts::{ArtifactTemplate, root_artifacts},
};

/// The observable result of an initialization attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitOutcome {
    /// The complete root artifact set was written.
    Initialized {
        /// Directory containing the generated artifacts.
        project_dir: PathBuf,
    },
    /// The complete root artifact set already existed and was not changed.
    AlreadyInitialized {
        /// Directory containing the existing artifacts.
        project_dir: PathBuf,
    },
}

impl fmt::Display for InitOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialized { project_dir } => {
                write!(
                    formatter,
                    "initialized Kvist project at {}",
                    project_dir.display()
                )
            }
            Self::AlreadyInitialized { project_dir } => {
                write!(
                    formatter,
                    "Kvist project already initialized at {}",
                    project_dir.display()
                )
            }
        }
    }
}

/// Initializes `project_dir` with Kvist's complete root artifact set.
///
/// Existing complete projects are reported without modification. Any partial
/// Kvist artifact set is rejected to prevent implicit merging or overwrites.
pub fn initialize(project_dir: &Path) -> Result<InitOutcome> {
    ensure_project_directory(project_dir)?;
    validate_existing_artifact_parents(project_dir, root_artifacts())?;

    let existing_artifacts = existing_artifacts(project_dir, root_artifacts())?;
    if existing_artifacts.len() == root_artifacts().len() {
        return Ok(InitOutcome::AlreadyInitialized {
            project_dir: project_dir.to_path_buf(),
        });
    }
    if !existing_artifacts.is_empty() {
        return Err(KvistError::ExistingArtifacts {
            project_dir: project_dir.to_path_buf(),
            artifacts: existing_artifacts,
        });
    }

    create_artifact_parents(project_dir, root_artifacts())?;
    for artifact in root_artifacts() {
        write_artifact_atomically(project_dir, artifact)?;
    }

    Ok(InitOutcome::Initialized {
        project_dir: project_dir.to_path_buf(),
    })
}

fn ensure_project_directory(project_dir: &Path) -> Result<()> {
    match fs::symlink_metadata(project_dir) {
        Ok(_) => validate_project_directory(project_dir),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(project_dir).map_err(|source| KvistError::Io {
                operation: "create project directory",
                path: project_dir.to_path_buf(),
                source,
            })?;
            validate_project_directory(project_dir)
        }
        Err(source) => Err(KvistError::Io {
            operation: "inspect project directory",
            path: project_dir.to_path_buf(),
            source,
        }),
    }
}

fn validate_project_directory(project_dir: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(project_dir).map_err(|source| KvistError::Io {
        operation: "inspect project directory",
        path: project_dir.to_path_buf(),
        source,
    })?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Err(KvistError::ProjectPathIsSymlink {
            path: project_dir.to_path_buf(),
        });
    }
    if !file_type.is_dir() {
        return Err(KvistError::ProjectPathNotDirectory {
            path: project_dir.to_path_buf(),
        });
    }

    Ok(())
}

fn validate_existing_artifact_parents(
    project_dir: &Path,
    artifacts: &[ArtifactTemplate],
) -> Result<()> {
    for parent in artifact_parent_paths(project_dir, artifacts) {
        match fs::symlink_metadata(&parent) {
            Ok(metadata) => validate_artifact_parent(&parent, &metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(KvistError::Io {
                operation: "inspect artifact parent",
                path: parent,
                source,
            }),
        }?;
    }

    Ok(())
}

fn existing_artifacts(project_dir: &Path, artifacts: &[ArtifactTemplate]) -> Result<Vec<PathBuf>> {
    let mut existing = Vec::new();

    for artifact in artifacts {
        let path = project_dir.join(artifact.relative_path);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => existing.push(path),
            Ok(_) => return Err(KvistError::ArtifactPathNotFile { path }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect generated artifact",
                    path,
                    source,
                });
            }
        }
    }

    Ok(existing)
}

fn create_artifact_parents(project_dir: &Path, artifacts: &[ArtifactTemplate]) -> Result<()> {
    for parent in artifact_parent_paths(project_dir, artifacts) {
        fs::create_dir_all(&parent).map_err(|source| KvistError::Io {
            operation: "create artifact parent",
            path: parent.clone(),
            source,
        })?;
        let metadata = fs::symlink_metadata(&parent).map_err(|source| KvistError::Io {
            operation: "inspect artifact parent",
            path: parent.clone(),
            source,
        })?;
        validate_artifact_parent(&parent, &metadata)?;
    }

    Ok(())
}

fn artifact_parent_paths(project_dir: &Path, artifacts: &[ArtifactTemplate]) -> BTreeSet<PathBuf> {
    artifacts
        .iter()
        .filter_map(|artifact| Path::new(artifact.relative_path).parent())
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| project_dir.join(parent))
        .collect()
}

fn validate_artifact_parent(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(KvistError::ArtifactParentIsSymlink {
            path: path.to_path_buf(),
        });
    }
    if !file_type.is_dir() {
        return Err(KvistError::ArtifactParentNotDirectory {
            path: path.to_path_buf(),
        });
    }

    Ok(())
}

fn write_artifact_atomically(project_dir: &Path, artifact: &ArtifactTemplate) -> Result<()> {
    let destination = project_dir.join(artifact.relative_path);
    let Some(parent) = destination.parent() else {
        return Err(KvistError::Io {
            operation: "determine artifact parent",
            path: destination,
            source: io::Error::other("artifact destination has no parent"),
        });
    };
    let mut temporary_file = NamedTempFile::new_in(parent).map_err(|source| KvistError::Io {
        operation: "create temporary artifact",
        path: parent.to_path_buf(),
        source,
    })?;

    temporary_file
        .write_all(artifact.contents.as_bytes())
        .map_err(|source| KvistError::Io {
            operation: "write temporary artifact",
            path: destination.clone(),
            source,
        })?;
    temporary_file
        .as_file()
        .sync_all()
        .map_err(|source| KvistError::Io {
            operation: "sync temporary artifact",
            path: destination.clone(),
            source,
        })?;

    // `persist_noclobber` retains the no-overwrite guarantee after preflight.
    temporary_file
        .persist_noclobber(&destination)
        .map(|_| ())
        .map_err(|error| KvistError::Io {
            operation: "persist generated artifact without overwriting",
            path: destination,
            source: error.error,
        })
}
