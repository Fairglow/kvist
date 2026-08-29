//! Converts an existing Rust project into a Kvist-managed component.

use std::{
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use toml::Value;

use crate::{
    KvistError, Result,
    file_io::{sync_directory, write_new_file_atomically},
    filesystem::is_link_like,
    specification, task_queue,
};

const METADATA_DIRECTORY: &str = ".kvist";
const GENERATED_AT: &str = "2026-08-28T13:03:02Z";

/// The observable result of an existing-project conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertOutcome {
    /// Draft Kvist artifacts were created below `.kvist`.
    Converted {
        /// Converted project directory.
        project_dir: PathBuf,
    },
    /// The project already contains its conversion metadata directory.
    AlreadyConverted {
        /// Converted project directory.
        project_dir: PathBuf,
    },
}

impl std::fmt::Display for ConvertOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Converted { project_dir } => write!(
                formatter,
                "created draft Kvist artifacts for existing project at {}; review and accept \
                 .kvist/SPEC.md and .kvist/TODOS.yaml before task execution",
                project_dir.display()
            ),
            Self::AlreadyConverted { project_dir } => write!(
                formatter,
                "existing-project conversion metadata already exists at {}",
                project_dir.join(METADATA_DIRECTORY).display()
            ),
        }
    }
}

#[derive(Debug)]
struct Manifest {
    name: String,
    version: String,
    description: String,
    authors: Vec<String>,
    dependencies: Vec<String>,
    features: Vec<String>,
}

/// Converts a Rust project without changing its manifest or implementation.
///
/// The generated artifacts are drafts. They are deliberately placed in
/// `.kvist` so the existing implementation root stays untouched.
pub fn convert(project_dir: &Path) -> Result<ConvertOutcome> {
    validate_project_directory(project_dir)?;
    let manifest = read_manifest(project_dir)?;
    validate_implementation_root(project_dir)?;

    let metadata_directory = project_dir.join(METADATA_DIRECTORY);
    match fs::symlink_metadata(&metadata_directory) {
        Ok(metadata) if is_link_like(&metadata) => {
            return Err(KvistError::ComponentDirectoryIsSymlink {
                path: metadata_directory,
            });
        }
        Ok(metadata) if metadata.file_type().is_dir() => {
            let expected_artifacts = ["SPEC.md", "TODOS.yaml", "IMPL.md", "COMPLIANCE_REVIEW.md"];
            let mut existing_artifacts = expected_artifacts
                .iter()
                .map(|name| metadata_directory.join(name))
                .filter(|path| path.exists())
                .collect::<Vec<_>>();
            if existing_artifacts.len() == expected_artifacts.len()
                && existing_artifacts.iter().all(|path| {
                    fs::symlink_metadata(path).is_ok_and(|metadata| {
                        !is_link_like(&metadata) && metadata.file_type().is_file()
                    })
                })
            {
                return Ok(ConvertOutcome::AlreadyConverted {
                    project_dir: project_dir.to_path_buf(),
                });
            }
            if existing_artifacts.is_empty() {
                existing_artifacts.push(metadata_directory.clone());
            }
            return Err(KvistError::ExistingArtifacts {
                project_dir: project_dir.to_path_buf(),
                artifacts: existing_artifacts,
            });
        }
        Ok(_) => {
            return Err(KvistError::ArtifactParentNotDirectory {
                path: metadata_directory,
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect conversion metadata directory",
                path: metadata_directory,
                source,
            });
        }
    }

    let specification = draft_specification(&manifest);
    if !specification::validate(&specification).is_valid() {
        return Err(KvistError::GeneratedSpecificationInvalid {
            diagnostics: "existing-project conversion template failed validation".to_owned(),
        });
    }
    let todos = draft_todos(&manifest, &specification)?;
    task_queue::parse(&todos).map_err(|error| KvistError::TaskQueueUnavailable {
        path: metadata_directory.join("TODOS.yaml"),
        reason: format!("generated conversion queue is invalid: {error}"),
    })?;
    let implementation_record = draft_implementation_record(project_dir)?;

    fs::create_dir(&metadata_directory).map_err(|source| KvistError::Io {
        operation: "create conversion metadata directory",
        path: metadata_directory.clone(),
        source,
    })?;

    for (name, contents) in [
        ("SPEC.md", specification),
        ("TODOS.yaml", todos),
        ("IMPL.md", implementation_record),
        ("COMPLIANCE_REVIEW.md", draft_compliance_review()),
    ] {
        write_new_file_atomically(&metadata_directory.join(name), &contents)?;
    }
    sync_directory(&metadata_directory)?;

    Ok(ConvertOutcome::Converted {
        project_dir: project_dir.to_path_buf(),
    })
}

fn validate_project_directory(project_dir: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(project_dir).map_err(|source| KvistError::Io {
        operation: "inspect existing project directory",
        path: project_dir.to_path_buf(),
        source,
    })?;
    if is_link_like(&metadata) {
        return Err(KvistError::ProjectPathIsSymlink {
            path: project_dir.to_path_buf(),
        });
    }
    if !metadata.file_type().is_dir() {
        return Err(KvistError::ProjectPathNotDirectory {
            path: project_dir.to_path_buf(),
        });
    }
    Ok(())
}

fn validate_implementation_root(project_dir: &Path) -> Result<()> {
    let source_directory = project_dir.join("src");
    let metadata = fs::symlink_metadata(&source_directory).map_err(|source| KvistError::Io {
        operation: "inspect existing Rust source directory",
        path: source_directory.clone(),
        source,
    })?;
    if is_link_like(&metadata) {
        return Err(KvistError::ComponentDirectoryIsSymlink {
            path: source_directory,
        });
    }
    if !metadata.file_type().is_dir() {
        return Err(KvistError::ComponentDirectoryNotDirectory {
            path: source_directory,
        });
    }
    Ok(())
}

fn read_manifest(project_dir: &Path) -> Result<Manifest> {
    let path = project_dir.join("Cargo.toml");
    let metadata = fs::symlink_metadata(&path).map_err(|source| KvistError::Io {
        operation: "inspect Cargo.toml",
        path: path.clone(),
        source,
    })?;
    if is_link_like(&metadata) || !metadata.file_type().is_file() {
        return Err(KvistError::ArtifactPathNotFile { path });
    }
    let contents = fs::read_to_string(&path).map_err(|source| KvistError::Io {
        operation: "read Cargo.toml",
        path: path.clone(),
        source,
    })?;
    let document: Value =
        toml::from_str(&contents).map_err(|error| KvistError::InvalidProjectConfiguration {
            path: path.clone(),
            reason: format!("existing Cargo.toml cannot be parsed: {error}"),
        })?;
    let package = document
        .get("package")
        .and_then(Value::as_table)
        .ok_or_else(|| KvistError::InvalidProjectConfiguration {
            path: path.clone(),
            reason: "existing Cargo.toml must contain a [package] table".to_owned(),
        })?;
    let required_string = |key: &str| {
        package
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| KvistError::InvalidProjectConfiguration {
                path: path.clone(),
                reason: format!("existing Cargo.toml [package].{key} must be a nonblank string"),
            })
    };
    let table_keys = |key: &str| {
        document
            .get(key)
            .and_then(Value::as_table)
            .map(|table| table.keys().cloned().collect())
            .unwrap_or_default()
    };

    Ok(Manifest {
        name: required_string("name")?,
        version: required_string("version")?,
        description: package
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("No package description was provided.")
            .to_owned(),
        authors: package
            .get("authors")
            .and_then(Value::as_array)
            .map(|authors| {
                authors
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        dependencies: table_keys("dependencies"),
        features: table_keys("features"),
    })
}

fn draft_specification(manifest: &Manifest) -> String {
    format!(
        r#"<!-- kvist-specification-version: 1 -->
# {name} Specification

<details open>
<summary>Layer 1: Executive summary and public contract</summary>

## Purpose

{description}

## Public contract

This draft describes the existing Rust package `{name}` version `{version}`. Review and
replace its inferred contract before accepting it for task execution.

</details>

<details>
<summary>Layer 2: Architectural guarantees</summary>

## Constraints and invariants

The conversion preserves `Cargo.toml`, `src/`, `tests/`, and `benches/` without
modification. Dependencies: {dependencies}. Features: {features}. Authors: {authors}.

</details>

<details>
<summary>Layer 3: Detailed strategy and algorithms</summary>

## Design and failure paths

This is a generated draft based on manifest metadata. It is not an assertion that the
existing implementation meets these requirements; acceptance requires human review.

</details>
"#,
        name = manifest.name,
        version = manifest.version,
        description = manifest.description,
        authors = list_or_none(&manifest.authors),
        dependencies = list_or_none(&manifest.dependencies),
        features = list_or_none(&manifest.features),
    )
}

fn draft_todos(manifest: &Manifest, specification: &str) -> Result<String> {
    let specification_revision = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(specification.as_bytes()))
    );
    let context = yaml_string(&format!(
        "Converted package {} {} with authors [{}], dependencies [{}], and features [{}].",
        manifest.name,
        manifest.version,
        list_or_none(&manifest.authors),
        list_or_none(&manifest.dependencies),
        list_or_none(&manifest.features),
    ))?;
    Ok(format!(
        r#"schema_version: 1
component:
  specification_revision: {specification_revision}
  parent_specification: null
  revalidation:
    state: current
    checked_at: {GENERATED_AT}
    stale_since: null
    causes: []
tasks:
  - id: review-existing-tests
    title: Review existing test coverage
    description: Review the existing project tests and add coverage for the accepted component contract.
    context: {context}
    purpose: Establish test evidence before implementation changes.
    expected_outcome: Accepted contract behavior is covered by executable tests.
    kind: test
    status: pending
    depends_on: []
    requirements:
      - TODO.md#ONB-01
    timestamps:
      created_at: {GENERATED_AT}
      updated_at: {GENERATED_AT}
      completed_at: null
    blocked_reason: null
    recovery_state: null
  - id: align-existing-implementation
    title: Align implementation with accepted specification
    description: Make only the implementation changes needed to meet the accepted specification.
    context: {context}
    purpose: Preserve existing work while making the component contract explicit.
    expected_outcome: The implementation satisfies the accepted specification and tests.
    kind: implementation
    status: pending
    depends_on:
      - review-existing-tests
    requirements:
      - TODO.md#ONB-01
    timestamps:
      created_at: {GENERATED_AT}
      updated_at: {GENERATED_AT}
      completed_at: null
    blocked_reason: null
    recovery_state: null
  - id: audit-existing-boundaries
    title: Audit existing implementation boundaries
    description: Independently audit filesystem, process, and data-input boundaries of the converted component.
    context: {context}
    purpose: Identify security risks before certifying imported implementation work.
    expected_outcome: Security findings are resolved or recorded for human arbitration.
    kind: security-audit
    status: pending
    depends_on:
      - align-existing-implementation
    requirements:
      - TODO.md#ONB-01
    timestamps:
      created_at: {GENERATED_AT}
      updated_at: {GENERATED_AT}
      completed_at: null
    blocked_reason: null
    recovery_state: null
  - id: review-converted-component
    title: Independently review converted component
    description: Create a clean-slate implementation record and compare it against the accepted specification.
    context: {context}
    purpose: Ensure an implementer does not certify its own imported work.
    expected_outcome: Compliance evidence records the independent review result.
    kind: compliance-review
    status: pending
    depends_on:
      - audit-existing-boundaries
    requirements:
      - TODO.md#ONB-01
    timestamps:
      created_at: {GENERATED_AT}
      updated_at: {GENERATED_AT}
      completed_at: null
    blocked_reason: null
    recovery_state: null
"#
    ))
}

fn draft_implementation_record(project_dir: &Path) -> Result<String> {
    let mut source_files = fs::read_dir(project_dir.join("src"))
        .map_err(|source| KvistError::Io {
            operation: "list existing Rust source files",
            path: project_dir.join("src"),
            source,
        })?
        .map(|entry| {
            entry.map_err(|source| KvistError::Io {
                operation: "inspect existing Rust source entry",
                path: project_dir.join("src"),
                source,
            })
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().is_some_and(|extension| extension == "rs"))
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    source_files.sort();

    Ok(format!(
        r#"<!-- kvist-implementation-record-version: 1 -->
# Existing Project Implementation Record

This draft records only the source files observed during conversion, without reading
the generated specification.

## Observed public contract

Existing Rust source files: {}.

## Observed guarantees and constraints

The conversion did not modify the manifest, source, test, or benchmark files.

## Observed design and failure paths

An independent clean-slate documentation pass must replace this draft before compliance
review.
"#,
        list_or_none(&source_files)
    ))
}

fn draft_compliance_review() -> String {
    r#"<!-- kvist-compliance-review-version: 1 -->
# Converted Component Compliance Review

## Review Status

**Status:** Not performed

The generated specification, task queue, and implementation record are drafts. An
independent clean-slate documentation and source-blind compliance review is required
after the specification and queue are accepted.
"#
    .to_owned()
}

fn list_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_owned()
    } else {
        values.join(", ")
    }
}

fn yaml_string(value: &str) -> Result<String> {
    serde_json::to_string(value).map_err(|error| KvistError::InvalidProjectConfiguration {
        path: PathBuf::from("Cargo.toml"),
        reason: format!("serialize generated conversion queue: {error}"),
    })
}
