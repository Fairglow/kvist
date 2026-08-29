//! Imports Kvist artifacts and codebases from a Git repository.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    KvistError, Result,
    init::{self, InitOutcome},
    specification, task_queue,
};

/// The observable result of a repository import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    /// Standard or converted artifacts existed and were successfully validated.
    ImportedWithExistingArtifacts {
        /// Local destination directory of the imported repository.
        dest_dir: PathBuf,
    },
    /// No Kvist artifacts existed; the project was automatically converted into a draft.
    ImportedAndConverted {
        /// Local destination directory of the imported repository.
        dest_dir: PathBuf,
    },
    /// No Kvist artifacts existed; a new Kvist component was initialized.
    ImportedAndInitialized {
        /// Local destination directory of the imported repository.
        dest_dir: PathBuf,
    },
}

impl std::fmt::Display for ImportOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ImportedWithExistingArtifacts { dest_dir } => write!(
                formatter,
                "successfully imported existing Kvist component to {}",
                dest_dir.display()
            ),
            Self::ImportedAndConverted { dest_dir } => write!(
                formatter,
                "imported project to {}; created draft Kvist conversion artifacts in .kvist",
                dest_dir.display()
            ),
            Self::ImportedAndInitialized { dest_dir } => write!(
                formatter,
                "imported project to {}; initialized new Kvist component artifacts",
                dest_dir.display()
            ),
        }
    }
}

/// Clones a remote repository, checks out the specific branch, and validates/initializes Kvist artifacts.
pub fn import(
    repo_url: &str,
    branch: &str,
    component: Option<&Path>,
    dest_dir: &Path,
) -> Result<ImportOutcome> {
    // 1. Ensure target destination directory is either empty or doesn't exist yet
    if dest_dir.exists() {
        if !dest_dir.is_dir() {
            return Err(KvistError::ProjectPathNotDirectory {
                path: dest_dir.to_path_buf(),
            });
        }
        let mut entries = fs::read_dir(dest_dir).map_err(|source| KvistError::Io {
            operation: "read destination directory",
            path: dest_dir.to_path_buf(),
            source,
        })?;
        if entries.next().is_some() {
            return Err(KvistError::ImportFailed {
                reason: format!(
                    "destination directory `{}` is not empty",
                    dest_dir.display()
                ),
            });
        }
    }

    // 2. Spawn git clone
    let output = std::process::Command::new("git")
        .args([
            "clone",
            "--branch",
            branch,
            "--depth",
            "1",
            repo_url,
            dest_dir.to_str().ok_or_else(|| KvistError::ImportFailed {
                reason: "destination directory is invalid UTF-8".to_owned(),
            })?,
        ])
        .output()
        .map_err(|source| KvistError::Io {
            operation: "execute git clone",
            path: dest_dir.to_path_buf(),
            source,
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(KvistError::ImportFailed {
            reason: format!("git clone failed: {}", stderr.trim()),
        });
    }

    // 3. Locate target directory
    let target_dir = match component {
        Some(path) => dest_dir.join(path),
        None => dest_dir.to_path_buf(),
    };

    if !target_dir.exists() || !target_dir.is_dir() {
        return Err(KvistError::ImportFailed {
            reason: format!(
                "component directory `{}` does not exist in the cloned repository",
                target_dir.display()
            ),
        });
    }

    // 4. Detect Kvist artifacts
    let std_spec = target_dir.join("SPEC.md");
    let std_todos = target_dir.join("TODOS.yaml");
    let std_impl = target_dir.join("IMPL.md");
    let has_std = std_spec.is_file() && std_todos.is_file() && std_impl.is_file();

    let conv_dir = target_dir.join(".kvist");
    let conv_spec = conv_dir.join("SPEC.md");
    let conv_todos = conv_dir.join("TODOS.yaml");
    let conv_impl = conv_dir.join("IMPL.md");
    let has_converted = conv_spec.is_file() && conv_todos.is_file() && conv_impl.is_file();

    if has_std || has_converted {
        // Validate existing artifacts
        let spec_path = if has_converted { conv_spec } else { std_spec };
        let todos_path = if has_converted { conv_todos } else { std_todos };

        // Validate specification
        let validation = specification::validate_file(&spec_path)?;
        if !validation.is_valid() {
            return Err(KvistError::SpecificationValidationFailed {
                path: spec_path,
                diagnostics: specification::format_diagnostics(&validation.diagnostics),
            });
        }

        // Validate task queue
        let queue_contents = fs::read_to_string(&todos_path).map_err(|source| KvistError::Io {
            operation: "read TODOS.yaml",
            path: todos_path.clone(),
            source,
        })?;
        task_queue::parse(&queue_contents).map_err(|error| KvistError::TaskQueueUnavailable {
            path: todos_path,
            reason: error.to_string(),
        })?;

        Ok(ImportOutcome::ImportedWithExistingArtifacts {
            dest_dir: dest_dir.to_path_buf(),
        })
    } else {
        // Artifacts are absent: initialize/convert using standard pipeline
        let init_outcome = init::initialize(&target_dir)?;
        match init_outcome {
            InitOutcome::ConvertedExistingRustProject { .. }
            | InitOutcome::AlreadyConvertedExistingRustProject { .. } => {
                Ok(ImportOutcome::ImportedAndConverted {
                    dest_dir: dest_dir.to_path_buf(),
                })
            }
            _ => Ok(ImportOutcome::ImportedAndInitialized {
                dest_dir: dest_dir.to_path_buf(),
            }),
        }
    }
}
