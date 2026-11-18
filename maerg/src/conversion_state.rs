//! Read-only inspection of existing-project conversion state.
//!
//! When an existing Rust project is converted, draft Kvist artifacts are
//! created in a `.kvist` directory. This module provides typed inspection
//! of that conversion state without modifying user files.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    KvistError, Result,
    component_documents::{self, DocumentKind},
    file_io::{sync_directory, write_new_file_atomically},
    filesystem::is_link_like,
    task_queue,
};

/// The conversion metadata directory name.
pub const CONVERSION_METADATA_DIRECTORY: &str = ".kvist";

/// The observable state of an existing-project conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionState {
    /// No conversion metadata exists.
    NotConverted,
    /// Draft conversion artifacts exist but none have been accepted.
    Draft,
    /// Some conversion artifacts exist but the set is incomplete.
    Partial,
    /// Conversion artifacts exist but one or more are malformed.
    Invalid,
    /// Accepted conversion artifacts have drifted from their recorded revisions.
    Stale,
    /// Conversion artifacts are accepted and current.
    Accepted,
}

impl ConversionState {
    /// Stable lowercase name used by status and doctor reports.
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotConverted => "not-converted",
            Self::Draft => "draft",
            Self::Partial => "partial",
            Self::Invalid => "invalid",
            Self::Stale => "stale",
            Self::Accepted => "accepted",
        }
    }
}

impl std::fmt::Display for ConversionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name())
    }
}

/// Read-only inspection result for a project's conversion state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionInspection {
    /// The inspected project directory.
    pub project_dir: PathBuf,
    /// The detected conversion state.
    pub state: ConversionState,
    /// Path to the conversion metadata directory, if it exists.
    pub metadata_dir: Option<PathBuf>,
    /// Existing implementation root (src directory), if it exists.
    pub implementation_root: Option<PathBuf>,
    /// Per-artifact status for conversion artifacts.
    pub artifacts: Vec<ConversionArtifactStatus>,
    /// Human-readable diagnostic when inspection found issues.
    pub diagnostic: Option<String>,
    /// Guidance for the user on next steps.
    pub guidance: String,
}

/// Status of one conversion artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionArtifactStatus {
    /// Artifact filename.
    pub filename: String,
    /// Whether the artifact exists.
    pub exists: bool,
    /// Whether the artifact is valid (if it exists).
    pub valid: bool,
    /// Diagnostic message if invalid.
    pub diagnostic: Option<String>,
}

/// Inspects the conversion state of a project without modifying any files.
pub fn inspect(project_dir: &Path) -> Result<ConversionInspection> {
    let metadata_dir = project_dir.join(CONVERSION_METADATA_DIRECTORY);
    let implementation_root = project_dir.join("src");

    // Check if conversion metadata directory exists
    let metadata_exists = match fs::symlink_metadata(&metadata_dir) {
        Ok(metadata) => {
            if is_link_like(&metadata) {
                return Ok(ConversionInspection {
                    project_dir: project_dir.to_path_buf(),
                    state: ConversionState::Invalid,
                    metadata_dir: Some(metadata_dir.clone()),
                    implementation_root: if implementation_root.is_dir() {
                        Some(implementation_root)
                    } else {
                        None
                    },
                    artifacts: Vec::new(),
                    diagnostic: Some(format!(
                        "conversion metadata directory is a link: {}",
                        metadata_dir.display()
                    )),
                    guidance: "the conversion metadata directory must be a real directory"
                        .to_owned(),
                });
            }
            metadata.file_type().is_dir()
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect conversion metadata directory",
                path: metadata_dir,
                source,
            });
        }
    };

    if !metadata_exists {
        return Ok(ConversionInspection {
            project_dir: project_dir.to_path_buf(),
            state: ConversionState::NotConverted,
            metadata_dir: None,
            implementation_root: if implementation_root.is_dir() {
                Some(implementation_root)
            } else {
                None
            },
            artifacts: Vec::new(),
            diagnostic: None,
            guidance: "run `kvist init` to convert this existing project".to_owned(),
        });
    }

    // Inspect each conversion artifact
    let required_artifacts = [
        ("REQUIREMENTS.md", Some(DocumentKind::Requirements)),
        ("CONTRACT.md", Some(DocumentKind::Contract)),
        ("DESIGN.md", Some(DocumentKind::Design)),
        ("TODOS.yaml", None),
        ("IMPL.md", None),
        ("COMPLIANCE_REVIEW.md", None),
    ];

    let mut artifacts = Vec::new();
    let mut has_invalid = false;
    let mut has_missing = false;

    for (filename, kind) in &required_artifacts {
        let artifact_path = metadata_dir.join(filename);
        let status = inspect_conversion_artifact(&artifact_path, *kind);
        if !status.exists {
            has_missing = true;
        }
        if status.exists && !status.valid {
            has_invalid = true;
        }
        artifacts.push(status);
    }

    // Check if the conversion has been accepted (via ACCEPTED marker or
    // TODOS.yaml revalidation state)
    let accepted_marker = metadata_dir.join("ACCEPTED");
    let todos_path = metadata_dir.join("TODOS.yaml");
    let is_accepted = if accepted_marker.exists() {
        true
    } else if todos_path.exists() {
        match fs::read_to_string(&todos_path) {
            Ok(contents) => match task_queue::parse(&contents) {
                Ok(queue) => {
                    // Check revalidation state
                    !matches!(
                        queue.component.revalidation.state,
                        task_queue::RevalidationState::Stale
                    )
                }
                Err(_) => false,
            },
            Err(_) => false,
        }
    } else {
        false
    };

    // A `.kvist` directory that holds only local configuration (for example
    // `config.toml`) and no conversion artifact is not a conversion at all;
    // reporting it as partial would block task execution on a project that was
    // never converted. Only a directory that actually contains at least one
    // conversion artifact is a genuine (possibly incomplete) conversion.
    let has_any_artifact = artifacts.iter().any(|artifact| artifact.exists);
    let state = if !has_any_artifact {
        ConversionState::NotConverted
    } else if has_invalid {
        ConversionState::Invalid
    } else if has_missing {
        ConversionState::Partial
    } else if is_accepted {
        ConversionState::Accepted
    } else {
        ConversionState::Draft
    };

    // Build diagnostic and guidance
    let (diagnostic, guidance) = match state {
        ConversionState::Invalid => (
            Some("one or more conversion artifacts are malformed".to_owned()),
            "inspect the malformed artifacts and repair them, or re-run `kvist init`".to_owned(),
        ),
        ConversionState::Partial => (
            Some("some conversion artifacts are missing".to_owned()),
            "re-run `kvist init` to complete the conversion".to_owned(),
        ),
        ConversionState::Draft => (
            None,
            "review and accept the draft artifacts with `kvist component accept .`".to_owned(),
        ),
        ConversionState::Accepted => (None, "conversion is complete and accepted".to_owned()),
        _ => (None, String::new()),
    };

    Ok(ConversionInspection {
        project_dir: project_dir.to_path_buf(),
        state,
        metadata_dir: Some(metadata_dir),
        implementation_root: if implementation_root.is_dir() {
            Some(implementation_root)
        } else {
            None
        },
        artifacts,
        diagnostic,
        guidance,
    })
}

/// Marks a conversion as accepted by writing an acceptance marker.
///
/// This is called after `component accept` successfully validates and
/// records the conversion artifacts. The marker enables task commands
/// to recognize the component as a valid converted component.
pub fn mark_accepted(project_dir: &Path) -> Result<()> {
    let metadata_dir = project_dir.join(CONVERSION_METADATA_DIRECTORY);
    let marker_path = metadata_dir.join("ACCEPTED");
    write_new_file_atomically(&marker_path, "")?;
    sync_directory(&metadata_dir)?;
    Ok(())
}

fn inspect_conversion_artifact(
    path: &Path,
    kind: Option<DocumentKind>,
) -> ConversionArtifactStatus {
    if !path.exists() {
        return ConversionArtifactStatus {
            filename: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            exists: false,
            valid: false,
            diagnostic: Some("artifact does not exist".to_owned()),
        };
    }

    match kind {
        Some(kind) => match component_documents::validate_file(kind, path) {
            Ok(validation) => ConversionArtifactStatus {
                filename: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                exists: true,
                valid: validation.is_valid(),
                diagnostic: if validation.is_valid() {
                    None
                } else {
                    Some(component_documents::format_diagnostics(
                        &validation.diagnostics,
                    ))
                },
            },
            Err(error) => ConversionArtifactStatus {
                filename: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                exists: true,
                valid: false,
                diagnostic: Some(format!("failed to validate: {error}")),
            },
        },
        None => {
            // For non-document artifacts, just check existence
            ConversionArtifactStatus {
                filename: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                exists: true,
                valid: true,
                diagnostic: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_project() -> TempDir {
        let project = TempDir::new().expect("create test project");
        fs::create_dir_all(project.path().join("src")).expect("create src directory");
        fs::write(
            project.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        fs::write(project.path().join("src/lib.rs"), "pub fn test() {}\n").expect("write lib.rs");
        project
    }

    #[test]
    fn inspect_not_converted_project() {
        let project = create_test_project();
        let inspection = inspect(project.path()).expect("inspect project");
        assert_eq!(inspection.state, ConversionState::NotConverted);
        assert!(inspection.metadata_dir.is_none());
    }

    #[test]
    fn a_kvist_directory_holding_only_local_config_is_not_a_conversion() {
        let project = create_test_project();
        // A local agent configuration lives in `.kvist/config.toml`; its mere
        // presence must not be mistaken for a (partial) existing-project
        // conversion, which would block task execution.
        fs::create_dir_all(project.path().join(CONVERSION_METADATA_DIRECTORY))
            .expect("create .kvist directory");
        fs::write(
            project
                .path()
                .join(CONVERSION_METADATA_DIRECTORY)
                .join("config.toml"),
            "[agent.profiles.developer]\ncommand_template = \"/usr/bin/echo agent\"\n",
        )
        .expect("write local config");
        let inspection = inspect(project.path()).expect("inspect project");
        assert_eq!(inspection.state, ConversionState::NotConverted);
    }

    #[test]
    fn conversion_state_names() {
        assert_eq!(ConversionState::NotConverted.name(), "not-converted");
        assert_eq!(ConversionState::Draft.name(), "draft");
        assert_eq!(ConversionState::Partial.name(), "partial");
        assert_eq!(ConversionState::Invalid.name(), "invalid");
        assert_eq!(ConversionState::Stale.name(), "stale");
        assert_eq!(ConversionState::Accepted.name(), "accepted");
    }
}
