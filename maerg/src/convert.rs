//! Converts an existing Rust project into a Kvist-managed component.

use std::{
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use toml::Value;

use crate::{
    KvistError, Result,
    component_documents::{self, DocumentKind},
    file_io::{sync_directory, write_new_file_atomically},
    filesystem::is_link_like,
    task_queue,
};

const METADATA_DIRECTORY: &str = ".kvist";

/// Maximum length for manifest fields interpolated into generated documents.
const MAX_FIELD_LENGTH: usize = 256;

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
                 .kvist intent documents and TODOS.yaml before task execution",
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
    tracing::info!(
        project_dir = %project_dir.display(),
        package = %manifest.name,
        "converting existing Rust project"
    );

    let metadata_directory = project_dir.join(METADATA_DIRECTORY);
    match fs::symlink_metadata(&metadata_directory) {
        Ok(metadata) if is_link_like(&metadata) => {
            return Err(KvistError::ComponentDirectoryIsSymlink {
                path: metadata_directory,
            });
        }
        Ok(metadata) if metadata.file_type().is_dir() => {
            let expected_artifacts = [
                "REQUIREMENTS.md",
                "CONTRACT.md",
                "DESIGN.md",
                "TODOS.yaml",
                "IMPL.md",
                "COMPLIANCE_REVIEW.md",
            ];
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

    let (requirements, contract, design) = draft_documents(&manifest);
    for (kind, contents) in [
        (DocumentKind::Requirements, requirements.as_str()),
        (DocumentKind::Contract, contract.as_str()),
        (DocumentKind::Design, design.as_str()),
    ] {
        let validation = component_documents::validate(kind, contents);
        if !validation.is_valid() {
            return Err(KvistError::GeneratedComponentDocumentInvalid {
                document: kind.filename(),
                diagnostics: component_documents::format_diagnostics(&validation.diagnostics),
            });
        }
    }
    let todos = draft_todos(&manifest, &requirements, &contract, &design)?;
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
        ("REQUIREMENTS.md", requirements),
        ("CONTRACT.md", contract),
        ("DESIGN.md", design),
        ("TODOS.yaml", todos),
        ("IMPL.md", implementation_record),
        ("COMPLIANCE_REVIEW.md", draft_compliance_review()),
    ] {
        write_new_file_atomically(&metadata_directory.join(name), &contents)?;
    }
    sync_directory(&metadata_directory)?;

    tracing::info!(
        project_dir = %project_dir.display(),
        "successfully converted project to Kvist component"
    );

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

/// Derive a deterministic UTC timestamp from manifest content for reproducible
/// draft generation. Identical inputs produce identical timestamps.
fn deterministic_timestamp(manifest: &Manifest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(
        format!(
            "{}:{}:{}",
            manifest.name, manifest.version, manifest.description
        )
        .as_bytes(),
    );
    let digest = hasher.finalize();
    // Use first 8 bytes to derive a Unix timestamp in a reasonable range
    let bytes = &digest[..8];
    let mut value: u64 = 0;
    for (i, byte) in bytes.iter().enumerate() {
        value |= (*byte as u64) << (i * 8);
    }
    // Map to a timestamp between 2020-01-01 and 2030-01-01
    let base: u64 = 1577836800; // 2020-01-01T00:00:00Z
    let range: u64 = 315360000; // 10 years in seconds
    let timestamp = base + (value % range);

    // Convert to ISO 8601 format
    let days = timestamp / 86400;
    let rem = timestamp % 86400;
    let hours = rem / 3600;
    let mins = (rem % 3600) / 60;
    let secs = rem % 60;

    // Days since epoch to date
    let mut year = 1970;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    let mut month = 1;
    let month_days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let days_in_feb = if is_leap(year) { 29 } else { 28 };
    let mut md = month_days;
    md[1] = days_in_feb;
    for (i, &d) in md.iter().enumerate() {
        if remaining_days < d as i64 {
            month = i + 1;
            break;
        }
        remaining_days -= d as i64;
    }
    let day = remaining_days + 1;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, mins, secs
    )
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Truncate and escape a manifest field for safe interpolation.
fn bounded_field(value: &str) -> String {
    let truncated = if value.len() > MAX_FIELD_LENGTH {
        format!("{}...", &value[..MAX_FIELD_LENGTH])
    } else {
        value.to_owned()
    };
    // Escape backslashes and double quotes for YAML/JSON contexts
    truncated.replace('\\', "\\\\").replace('"', "\\\"")
}

fn draft_documents(manifest: &Manifest) -> (String, String, String) {
    let name = bounded_field(&manifest.name);
    let version = bounded_field(&manifest.version);
    let description = bounded_field(&manifest.description);
    let authors = {
        let bounded: Vec<String> = manifest
            .authors
            .iter()
            .map(|s| bounded_field(s.as_str()))
            .collect();
        list_or_none(&bounded)
    };
    let dependencies = {
        let bounded: Vec<String> = manifest
            .dependencies
            .iter()
            .map(|s| bounded_field(s.as_str()))
            .collect();
        list_or_none(&bounded)
    };
    let features = {
        let bounded: Vec<String> = manifest
            .features
            .iter()
            .map(|s| bounded_field(s.as_str()))
            .collect();
        list_or_none(&bounded)
    };

    let requirements = format!(
        r#"<!-- kvist-requirements-version: 1 -->
# {name} Requirements

## Purpose and scope

{description}

This draft covers the existing Rust package `{name}` version `{version}`.
Its product scope and non-goals require human review.

## Stakeholders and concerns

Package authors recorded by Cargo metadata: {authors}. Identify actual users,
operators, integrators, and review concerns before acceptance.

## Functional requirements

No functional requirements can be inferred safely from manifest metadata.
Define stable requirement IDs from intended behavior before task execution.

## Quality requirements and constraints

Conversion MUST preserve the existing manifest, source, tests, and benchmarks.
Current dependencies: {dependencies}. Current features: {features}.

## Acceptance and traceability

Acceptance requires human-authored requirements, links to the parent
architecture and contract clauses, and observable verification criteria.
"#,
        name = name,
        version = version,
        description = description,
        authors = authors,
        dependencies = dependencies,
        features = features
    );
    let contract = format!(
        r#"<!-- kvist-contract-version: 1 -->
# {name} Contract

## Boundary and ownership

Draft boundary for existing package `{name}` version `{version}`. Ownership and
supported consumers require human confirmation.

## Provided interfaces

No consumer-visible interface is inferred from manifest metadata. Inspect
public source behavior independently and define stable interface IDs.

## Required interfaces

Cargo dependencies observed during conversion: {dependencies}. This list is
not yet an approved cross-component dependency contract.

## Data and schemas

None identified. Add exact paths and dialect versions only when a
machine-readable schema is useful.

## Behavioral guarantees

No behavioral guarantee is inferred from package metadata.

## Errors and failure semantics

No failure contract is inferred from package metadata.

## Security and authority

Trust, authorization, data, filesystem, process, and network boundaries require
human review.

## Compatibility and verification

Package version `{version}` was observed. Define the actual public compatibility
policy and contract-test evidence before acceptance.
"#,
        name = name,
        version = version,
        dependencies = dependencies
    );
    let design = format!(
        r#"<!-- kvist-design-version: 1 -->
# {name} Design

## Design overview

Generated conversion draft for package `{name}`. It records only manifest-level
facts and does not certify the existing implementation.

## Internal structure

The Rust package has features {features} and dependencies {dependencies}.
Source-level decomposition requires a separate design pass.

## Interactions and state

No runtime interaction or state model is inferred from Cargo metadata.

## Algorithms and decisions

No algorithm or architecture decision is inferred from Cargo metadata.

## Failure and recovery

Failure propagation and recovery behavior require implementation-independent
design and review.

## Security and resource design

Security enforcement and resource bounds require explicit design.

## Verification strategy

Review existing tests without treating them as intended requirements, then map
approved requirement and contract IDs to independent verification.
"#,
        name = name,
        dependencies = dependencies,
        features = features
    );
    (requirements, contract, design)
}

fn draft_todos(
    manifest: &Manifest,
    requirements: &str,
    contract: &str,
    design: &str,
) -> Result<String> {
    let requirements_revision = revision(requirements);
    let contract_revision = revision(contract);
    let design_revision = revision(design);
    let generated_at = deterministic_timestamp(manifest);
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
  requirements_revision: {requirements_revision}
  contract_revision: {contract_revision}
  design_revision: {design_revision}
  parent_contract: null
  revalidation:
    state: current
    checked_at: {generated_at}
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
      created_at: {generated_at}
      updated_at: {generated_at}
      completed_at: null
    blocked_reason: null
    recovery_state: null
  - id: align-existing-implementation
    title: Align implementation with approved component intent
    description: Make only the implementation changes needed to meet the approved requirements, contract, and design.
    context: {context}
    purpose: Preserve existing work while making the component contract explicit.
    expected_outcome: The implementation satisfies the approved intent documents and tests.
    kind: implementation
    status: pending
    depends_on:
      - review-existing-tests
    requirements:
      - TODO.md#ONB-01
    timestamps:
      created_at: {generated_at}
      updated_at: {generated_at}
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
      created_at: {generated_at}
      updated_at: {generated_at}
      completed_at: null
    blocked_reason: null
    recovery_state: null
  - id: review-converted-component
    title: Independently review converted component
    description: Create a clean-slate implementation record and compare it against the approved intent documents.
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
      created_at: {generated_at}
      updated_at: {generated_at}
      completed_at: null
    blocked_reason: null
    recovery_state: null
"#
    ))
}

/// Recursively inventory Rust implementation files under `src/`.
/// Returns paths relative to the project root, sorted for deterministic output.
fn inventory_implementation_files(project_dir: &Path) -> Result<Vec<String>> {
    let src_dir = project_dir.join("src");
    let mut files = Vec::new();
    walk_rust_files(&src_dir, &src_dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk_rust_files(base: &Path, current: &Path, out: &mut Vec<String>) -> Result<()> {
    let entries = fs::read_dir(current).map_err(|source| KvistError::Io {
        operation: "list directory",
        path: current.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| KvistError::Io {
            operation: "inspect directory entry",
            path: current.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| KvistError::Io {
            operation: "inspect file metadata",
            path: path.clone(),
            source,
        })?;
        if is_link_like(&metadata) {
            continue;
        }
        if metadata.file_type().is_dir() {
            walk_rust_files(base, &path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let relative = path
                .strip_prefix(base)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| {
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                });
            out.push(relative);
        }
    }
    Ok(())
}

fn draft_implementation_record(project_dir: &Path) -> Result<String> {
    let source_files = inventory_implementation_files(project_dir)?;

    Ok(format!(
        r#"<!-- kvist-implementation-record-version: 1 -->
# Existing Project Implementation Record

This draft records only the source files observed during conversion, without
using the generated requirements, contract, or design.

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

The generated requirements, contract, design, task queue, and implementation
record are drafts. An
independent clean-slate documentation and source-blind compliance review is required
after the intent documents and queue are accepted.
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

fn revision(contents: &str) -> String {
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(contents.as_bytes()))
    )
}

fn yaml_string(value: &str) -> Result<String> {
    serde_json::to_string(value).map_err(|error| KvistError::InvalidProjectConfiguration {
        path: PathBuf::from("Cargo.toml"),
        reason: format!("serialize generated conversion queue: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_manifest(name: &str, version: &str, description: &str) -> Manifest {
        Manifest {
            name: name.to_owned(),
            version: version.to_owned(),
            description: description.to_owned(),
            authors: vec![],
            dependencies: vec![],
            features: vec![],
        }
    }

    #[test]
    fn deterministic_timestamp_is_stable_for_identical_inputs() {
        let m = test_manifest("pkg", "1.0", "desc");
        let t1 = deterministic_timestamp(&m);
        let t2 = deterministic_timestamp(&m);
        assert_eq!(t1, t2, "identical inputs must produce identical timestamps");
    }

    #[test]
    fn deterministic_timestamp_differs_for_different_inputs() {
        let m1 = test_manifest("pkg-a", "1.0", "desc");
        let m2 = test_manifest("pkg-b", "1.0", "desc");
        let t1 = deterministic_timestamp(&m1);
        let t2 = deterministic_timestamp(&m2);
        assert_ne!(
            t1, t2,
            "different inputs should produce different timestamps"
        );
    }

    #[test]
    fn deterministic_timestamp_is_valid_iso8601() {
        let m = test_manifest("pkg", "1.0", "desc");
        let t = deterministic_timestamp(&m);
        // Basic format validation: YYYY-MM-DDTHH:MM:SSZ
        assert!(t.ends_with("Z"));
        assert_eq!(t.len(), 20);
        assert_eq!(t.chars().nth(4), Some('-'));
        assert_eq!(t.chars().nth(7), Some('-'));
        assert_eq!(t.chars().nth(10), Some('T'));
        assert_eq!(t.chars().nth(13), Some(':'));
        assert_eq!(t.chars().nth(16), Some(':'));
    }

    #[test]
    fn bounded_field_truncates_long_values() {
        let long = "x".repeat(500);
        let result = bounded_field(&long);
        assert!(result.len() <= MAX_FIELD_LENGTH + 4); // +4 for "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn bounded_field_escapes_special_characters() {
        let input = "hello \"world\" \\ path";
        let result = bounded_field(input);
        assert!(result.contains("\\\""));
        assert!(result.contains("\\\\"));
    }

    #[test]
    fn inventory_recurses_into_subdirectories() {
        let project = TempDir::new().unwrap();
        let src = project.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("main.rs"), "").unwrap();
        fs::write(src.join("sub/nested.rs"), "").unwrap();
        fs::write(src.join("sub/deep.rs"), "").unwrap();

        let files = inventory_implementation_files(project.path()).unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.contains(&"main.rs".to_owned()));
        assert!(files.contains(&"sub/nested.rs".to_owned()));
        assert!(files.contains(&"sub/deep.rs".to_owned()));
    }

    #[test]
    fn inventory_skips_non_rust_files() {
        let project = TempDir::new().unwrap();
        let src = project.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("main.rs"), "").unwrap();
        fs::write(src.join("README.md"), "").unwrap();
        fs::write(src.join("data.json"), "").unwrap();

        let files = inventory_implementation_files(project.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], "main.rs");
    }

    #[test]
    fn inventory_is_sorted_deterministically() {
        let project = TempDir::new().unwrap();
        let src = project.path().join("src");
        fs::create_dir_all(src.join("b")).unwrap();
        fs::create_dir_all(src.join("a")).unwrap();
        fs::write(src.join("z.rs"), "").unwrap();
        fs::write(src.join("a/first.rs"), "").unwrap();
        fs::write(src.join("b/second.rs"), "").unwrap();

        let files = inventory_implementation_files(project.path()).unwrap();
        assert_eq!(files, vec!["a/first.rs", "b/second.rs", "z.rs"]);
    }

    #[test]
    fn draft_documents_use_bounded_fields() {
        let manifest = Manifest {
            name: "test-pkg".to_owned(),
            version: "1.0.0".to_owned(),
            description: "A test package".to_owned(),
            authors: vec!["Author <author@example.com>".to_owned()],
            dependencies: vec!["serde".to_owned()],
            features: vec!["default".to_owned()],
        };
        let (requirements, contract, design) = draft_documents(&manifest);
        assert!(requirements.contains("test-pkg"));
        assert!(requirements.contains("1.0.0"));
        assert!(requirements.contains("serde"));
        assert!(contract.contains("test-pkg"));
        assert!(design.contains("test-pkg"));
    }

    #[test]
    fn draft_todos_contains_deterministic_timestamp() {
        let manifest = test_manifest("pkg", "1.0", "desc");
        let (req, con, des) = draft_documents(&manifest);
        let todos = draft_todos(&manifest, &req, &con, &des).unwrap();
        let expected_ts = deterministic_timestamp(&manifest);
        assert!(todos.contains(&expected_ts));
        assert!(!todos.contains("2026-08-28T13:03:02Z")); // old fixed timestamp
    }
}
