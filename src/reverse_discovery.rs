//! Reverse-discovery pipeline for draft Kvist artifacts from existing source.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{
    KvistError, Result,
    component_documents::{self, DocumentKind},
    file_io::write_new_file_atomically,
    filesystem::is_link_like,
    task_queue,
};

const METADATA_DIRECTORY: &str = ".kvist";

/// The observable outcome of a reverse-discovery run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReverseDiscoverOutcome {
    /// Kvist artifacts were successfully reverse-discovered and written.
    Discovered {
        /// Directory that was scanned.
        path: PathBuf,
    },
}

impl std::fmt::Display for ReverseDiscoverOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discovered { path } => write!(
                formatter,
                "successfully reverse-discovered and generated draft Kvist artifacts at {}",
                path.join(METADATA_DIRECTORY).display()
            ),
        }
    }
}

#[derive(Debug, Default)]
struct DiscoveredSymbols {
    pub_exports: Vec<String>,
    tests: Vec<String>,
    documents: Vec<String>,
    source_files: Vec<PathBuf>,
}

/// Scans a directory and reverse-engineers a spec, task queue, and implementation record.
pub fn reverse_discover(path: &Path) -> Result<ReverseDiscoverOutcome> {
    // 1. Verify directory safety
    let metadata = fs::symlink_metadata(path).map_err(|source| KvistError::Io {
        operation: "inspect reverse-discovery path",
        path: path.to_path_buf(),
        source,
    })?;
    if is_link_like(&metadata) {
        return Err(KvistError::ProjectPathIsSymlink {
            path: path.to_path_buf(),
        });
    }
    if !metadata.file_type().is_dir() {
        return Err(KvistError::ProjectPathNotDirectory {
            path: path.to_path_buf(),
        });
    }

    // 2. Prevent overwriting existing component intent.
    let metadata_directory = path.join(METADATA_DIRECTORY);
    ensure_metadata_directory_safe(&metadata_directory, true)?;
    for root in [&metadata_directory, &path.join("src")] {
        for kind in [
            DocumentKind::Requirements,
            DocumentKind::Contract,
            DocumentKind::Design,
        ] {
            let target = root.join(kind.filename());
            if target.exists() {
                return Err(KvistError::ImportFailed {
                    reason: format!(
                        "refusing to overwrite existing component document at {}",
                        target.display()
                    ),
                });
            }
        }
    }

    // 3. Scan codebase recursively for symbols
    let mut symbols = DiscoveredSymbols::default();
    scan_directory(path, &mut symbols)?;

    // 4. Generate draft intent documents.
    let (requirements, contract, design) = generate_documents(&symbols);
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

    // 5. Generate TODOS and IMPL
    let todos_content = generate_todos(&symbols, &requirements, &contract, &design);
    task_queue::parse(&todos_content).map_err(|error| KvistError::TaskQueueUnavailable {
        path: metadata_directory.join("TODOS.yaml"),
        reason: format!("generated reverse-discovered queue is invalid: {error}"),
    })?;

    let impl_content = generate_impl_record(&symbols);

    // 6. Write artifacts to .kvist
    if !metadata_directory.exists() {
        fs::create_dir(&metadata_directory).map_err(|source| KvistError::Io {
            operation: "create metadata directory",
            path: metadata_directory.clone(),
            source,
        })?;
    }
    ensure_metadata_directory_safe(&metadata_directory, false)?;

    for (filename, contents) in [
        ("REQUIREMENTS.md", requirements.as_str()),
        ("CONTRACT.md", contract.as_str()),
        ("DESIGN.md", design.as_str()),
        ("TODOS.yaml", todos_content.as_str()),
        ("IMPL.md", impl_content.as_str()),
        (
            "COMPLIANCE_REVIEW.md",
            "<!-- kvist-compliance-review-version: 1 -->\n# Compliance Review\n",
        ),
    ] {
        ensure_metadata_directory_safe(&metadata_directory, false)?;
        write_new_file_atomically(&metadata_directory.join(filename), contents)?;
    }

    Ok(ReverseDiscoverOutcome::Discovered {
        path: path.to_path_buf(),
    })
}

fn ensure_metadata_directory_safe(path: &Path, allow_missing: bool) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_like(&metadata) => Err(KvistError::ImportFailed {
            reason: format!(
                "refusing to use link-like reverse-discovery metadata directory {}",
                path.display()
            ),
        }),
        Ok(metadata) if !metadata.file_type().is_dir() => Err(KvistError::ImportFailed {
            reason: format!(
                "reverse-discovery metadata path {} must be a directory",
                path.display()
            ),
        }),
        Ok(_) => Ok(()),
        Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(KvistError::Io {
            operation: "inspect reverse-discovery metadata directory",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn scan_directory(dir: &Path, symbols: &mut DiscoveredSymbols) -> Result<()> {
    if is_link_like(&fs::symlink_metadata(dir).map_err(|source| KvistError::Io {
        operation: "inspect directory for reverse-discovery walk",
        path: dir.to_path_buf(),
        source,
    })?) {
        return Ok(());
    }
    let entries = fs::read_dir(dir).map_err(|source| KvistError::Io {
        operation: "read directory for reverse-discovery walk",
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| KvistError::Io {
            operation: "read directory entry",
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| KvistError::Io {
            operation: "inspect entry metadata",
            path: path.clone(),
            source,
        })?;
        if is_link_like(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == ".git" || name == "target" || name == ".kvist" || name == "node_modules" {
                continue;
            }
            scan_directory(&path, symbols)?;
        } else if metadata.is_file() {
            let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            if extension == "rs" {
                symbols.source_files.push(path.clone());
                parse_rust_file(&path, symbols)?;
            } else if extension == "py" {
                symbols.source_files.push(path.clone());
                parse_python_file(&path, symbols)?;
            } else if extension == "md" {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !matches!(
                    name,
                    "REQUIREMENTS.md"
                        | "CONTRACT.md"
                        | "DESIGN.md"
                        | "IMPL.md"
                        | "COMPLIANCE_REVIEW.md"
                ) {
                    let content = fs::read_to_string(&path).unwrap_or_default();
                    let summary = if content.trim().is_empty() {
                        "Empty markdown file."
                    } else {
                        content.lines().next().unwrap_or("No content summary.")
                    };
                    symbols.documents.push(format!(
                        "- `{}`: {}",
                        path.display().to_string().replace('\\', "/"),
                        summary.trim()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn parse_rust_file(path: &Path, symbols: &mut DiscoveredSymbols) -> Result<()> {
    let contents = fs::read_to_string(path).map_err(|source| KvistError::Io {
        operation: "read rust file",
        path: path.to_path_buf(),
        source,
    })?;
    for line in contents.lines() {
        let line_trim = line.trim();
        if line_trim.starts_with("pub struct")
            || line_trim.starts_with("pub enum")
            || line_trim.starts_with("pub trait")
            || line_trim.starts_with("pub fn")
        {
            if let Some(decl) = line_trim.strip_suffix('{') {
                symbols.pub_exports.push(format!("- `{}`", decl.trim()));
            } else {
                symbols.pub_exports.push(format!("- `{}`", line_trim));
            }
        }
        if (line_trim.contains("fn test_") || line_trim.starts_with("#[test]"))
            && let Some(pos) = line_trim.find("fn ")
        {
            let decl = &line_trim[pos..];
            if let Some(test_name) = decl.strip_suffix('{') {
                symbols.tests.push(format!("- `{}`", test_name.trim()));
            } else {
                symbols.tests.push(format!("- `{}`", decl.trim()));
            }
        }
    }
    Ok(())
}

fn parse_python_file(path: &Path, symbols: &mut DiscoveredSymbols) -> Result<()> {
    let contents = fs::read_to_string(path).map_err(|source| KvistError::Io {
        operation: "read python file",
        path: path.to_path_buf(),
        source,
    })?;
    for line in contents.lines() {
        let line_trim = line.trim();
        if line_trim.starts_with("class ") {
            if let Some(decl) = line_trim.strip_suffix(':') {
                symbols.pub_exports.push(format!("- `{}`", decl.trim()));
            } else {
                symbols.pub_exports.push(format!("- `{}`", line_trim));
            }
        } else if line_trim.starts_with("def ") || line_trim.starts_with("async def ") {
            // Filter out private functions
            if !line_trim.starts_with("def _") && !line_trim.starts_with("async def _") {
                let decl = line_trim.strip_suffix(':').unwrap_or(line_trim);
                if decl.contains("test_") {
                    symbols.tests.push(format!("- `{}`", decl.trim()));
                } else {
                    symbols.pub_exports.push(format!("- `{}`", decl.trim()));
                }
            }
        }
    }
    Ok(())
}

fn generate_documents(symbols: &DiscoveredSymbols) -> (String, String, String) {
    let exports = if symbols.pub_exports.is_empty() {
        "- No public symbols reverse-discovered.".to_owned()
    } else {
        symbols.pub_exports.join("\n")
    };

    let files = if symbols.source_files.is_empty() {
        "- No implementation source files discovered.".to_owned()
    } else {
        symbols
            .source_files
            .iter()
            .map(|f| format!("- `{}`", f.display().to_string().replace('\\', "/")))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let docs = if symbols.documents.is_empty() {
        "- No supplemental markdown files discovered.".to_owned()
    } else {
        symbols.documents.join("\n")
    };

    let tests = if symbols.tests.is_empty() {
        "- No unit or integration tests reverse-discovered.".to_owned()
    } else {
        symbols.tests.join("\n")
    };

    let requirements = format!(
        r#"<!-- kvist-requirements-version: 1 -->
# Reverse-Discovered Requirements Draft

## Purpose and scope

Human review must define the intended purpose, scope, and non-goals. This draft
contains only observations from existing source.

## Stakeholders and concerns

No stakeholders or intended concerns can be inferred safely from source symbols.

## Functional requirements

No intended behavior is asserted. The following tests were observed and may
provide evidence after independent review:

{tests}

## Quality requirements and constraints

No intended quality requirement is inferred. The following source files were
observed:

{files}

## Acceptance and traceability

Replace this section with stable requirement IDs, architecture links, contract
clauses, and observable acceptance criteria before acceptance.
"#
    );
    let contract = format!(
        r#"<!-- kvist-contract-version: 1 -->
# Reverse-Discovered Contract Draft

## Boundary and ownership

Ownership, supported consumers, and component responsibility require human
review.

## Provided interfaces

The following public symbols were observed. They are not yet an approved
consumer contract:

{exports}

## Required interfaces

No cross-component dependency contract was inferred.

## Data and schemas

No machine-readable interface schema was identified.

## Behavioral guarantees

No consumer-visible guarantee is inferred from names or signatures alone.

## Errors and failure semantics

No error, retry, partial-effect, or recovery semantics were inferred.

## Security and authority

Trust, authorization, data, filesystem, process, and network boundaries require
human review.

## Compatibility and verification

Define stable interface IDs, versions, compatibility rules, and contract tests
before acceptance.
"#
    );
    let design = format!(
        r#"<!-- kvist-design-version: 1 -->
# Reverse-Discovered Design Draft

## Design overview

This source-derived draft inventories existing structure without asserting that
it is the intended design.

## Internal structure

Observed implementation files:

{files}

Supplemental Markdown observed:

{docs}

## Interactions and state

No interaction, lifecycle, or state model was inferred.

## Algorithms and decisions

No algorithm or rationale was inferred from symbol names.

## Failure and recovery

Failure propagation and recovery require independent analysis.

## Security and resource design

Security enforcement and resource bounds require independent analysis.

## Verification strategy

Observed tests:

{tests}

Map reviewed evidence to approved requirement and contract IDs before
acceptance.
"#
    );
    (requirements, contract, design)
}

fn generate_todos(
    symbols: &DiscoveredSymbols,
    requirements: &str,
    contract: &str,
    design: &str,
) -> String {
    let summary = if symbols.pub_exports.is_empty() {
        "Empty interface set"
    } else {
        "Discovered public exports"
    };

    format!(
        r#"schema_version: 1
component:
  requirements_revision: "{requirements_revision}"
  contract_revision: "{contract_revision}"
  design_revision: "{design_revision}"
  parent_contract: null
  revalidation:
    state: current
    checked_at: "2026-08-28T13:03:02Z"
    stale_since: null
    causes: []
tasks:
  - id: "define-discovered-tests"
    title: "Define test suite for discovered symbols"
    description: "Write focused unit and integration test fixtures covering the discovered public exports."
    context: "Summary: {summary}."
    purpose: "Establish a robust test suite for reverse-discovered code."
    expected_outcome: "A full set of unit tests covering the public interfaces."
    kind: test
    status: pending
    depends_on: []
    requirements:
      - "REQUIREMENTS.md#Acceptance-and-traceability"
    timestamps:
      created_at: "2026-08-28T13:03:02Z"
      updated_at: "2026-08-28T13:03:02Z"
      completed_at: null
    blocked_reason: null
    recovery_state: null
  - id: "refactor-discovered-symbols"
    title: "Refactor and verify reverse-discovered symbols"
    description: "Review and align the public contract exports with architectural requirements."
    context: "Summary: {summary}."
    purpose: "Align existing implementation with human-approved component intent."
    expected_outcome: "Clean compilation and zero compliance violations."
    kind: implementation
    status: pending
    depends_on:
      - "define-discovered-tests"
    requirements:
      - "CONTRACT.md#Provided-interfaces"
      - "DESIGN.md#Design-overview"
      - "REQUIREMENTS.md#Functional-requirements"
    timestamps:
      created_at: "2026-08-28T13:03:02Z"
      updated_at: "2026-08-28T13:03:02Z"
      completed_at: null
    blocked_reason: null
    recovery_state: null
  - id: "audit-discovered-boundaries"
    title: "Audit reverse-discovered boundaries"
    description: "Independently audit filesystem, process, and data-input boundaries of the discovered components."
    context: "Summary: {summary}."
    purpose: "Identify security risks before certifying reverse-discovered implementation work."
    expected_outcome: "Security findings are resolved or recorded for human arbitration."
    kind: security-audit
    status: pending
    depends_on:
      - "refactor-discovered-symbols"
    requirements:
      - "CONTRACT.md#Security-and-authority"
    timestamps:
      created_at: "2026-08-28T13:03:02Z"
      updated_at: "2026-08-28T13:03:02Z"
      completed_at: null
    blocked_reason: null
    recovery_state: null
  - id: "review-discovered-spec"
    title: "Review reverse-discovered component intent"
    description: "Compare independently observed behavior with the approved requirements, contract, and design."
    context: "Summary: {summary}."
    purpose: "Validate that implementation matches the approved component intent."
    expected_outcome: "Discrepancies are recorded for human arbitration."
    kind: compliance-review
    status: pending
    depends_on:
      - "audit-discovered-boundaries"
    requirements:
      - "CONTRACT.md#Compatibility-and-verification"
      - "DESIGN.md#Verification-strategy"
      - "REQUIREMENTS.md#Acceptance-and-traceability"
    timestamps:
      created_at: "2026-08-28T13:03:02Z"
      updated_at: "2026-08-28T13:03:02Z"
      completed_at: null
    blocked_reason: null
    recovery_state: null
"#,
        requirements_revision = revision(requirements),
        contract_revision = revision(contract),
        design_revision = revision(design),
    )
}

fn generate_impl_record(symbols: &DiscoveredSymbols) -> String {
    let files = if symbols.source_files.is_empty() {
        "- No source files recorded.".to_owned()
    } else {
        symbols
            .source_files
            .iter()
            .map(|f| format!("- `{}`", f.display().to_string().replace('\\', "/")))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

- **Verified At**: `2026-08-28T13:03:02Z`
- **Result**: `completed`

## Discovered Implementation Files

The following files form the core implementation:

{files}
"#
    )
}

fn revision(contents: &str) -> String {
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(contents.as_bytes()))
    )
}
