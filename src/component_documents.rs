//! Creation and validation for Kvist component intent documents.
//!
//! Requirements, contracts, and designs are separate durable artifacts because
//! they have different readers and change-propagation rules. Validation is
//! structural and preserves all user-authored Markdown.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{KvistError, Result, file_io::write_new_file_atomically, filesystem::is_link_like};

/// Current document version for `REQUIREMENTS.md`.
pub const REQUIREMENTS_VERSION: u32 = 1;
/// Current document version for `CONTRACT.md`.
pub const CONTRACT_VERSION: u32 = 1;
/// Current document version for `DESIGN.md`.
pub const DESIGN_VERSION: u32 = 1;
/// Maximum supported size of an intent document read from disk.
pub const MAX_COMPONENT_DOCUMENT_BYTES: u64 = 1024 * 1024;

/// Deterministic template for component requirements.
pub const COMPONENT_REQUIREMENTS_TEMPLATE: &str = r#"<!-- kvist-requirements-version: 1 -->
# Component Requirements

Requirements define what this component must accomplish and how completion is
judged. They must not prescribe internal implementation choices unless a choice
is itself an externally imposed constraint.

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Purpose and scope

[Describe why the component exists, its scope, explicit non-goals, owned state,
and responsibility within the parent architecture.]

## Stakeholders and concerns

[Identify affected users, operators, components, and reviewers, together with
the concerns this document must address.]

## Functional requirements

[Give each testable behavior a stable requirement ID. Describe required
outcomes and externally imposed rules without selecting private algorithms.]

## Quality requirements and constraints

[State measurable quality scenarios and imposed security, performance,
durability, portability, dependency, concurrency, and resource constraints.]

## Acceptance and traceability

[Define observable acceptance criteria. Link requirements to architecture
drivers, parent responsibilities, contract clauses, tests, and relevant ADRs.]
"#;

/// Deterministic template for a component contract.
pub const COMPONENT_CONTRACT_TEMPLATE: &str = r#"<!-- kvist-contract-version: 1 -->
# Component Contract

This contract is the normative boundary that consumers may rely on. Internal
requirements and design choices are intentionally excluded. Machine-readable
interface definitions are optional; when present, they are authoritative for
syntax and data shape while this document remains authoritative for semantics.

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Boundary and ownership

[Record stable component and contract IDs, owner, supported consumers, purpose,
provided boundary, explicit non-responsibilities, and owned state.]

## Provided interfaces

[For each provided interface, record a stable ID and version, interface kind,
operations or messages, callers, inputs, outputs, and observable side effects.]

## Required interfaces

[List only dependencies required across component boundaries. Record the
provider contract ID, required version range, purpose, and permitted use.
Write "None" when the component has no required interfaces.]

## Data and schemas

[Define encodings and validation rules. Reference optional OpenAPI, AsyncAPI,
JSON Schema, Protocol Buffers, WIT, or other native IDL files by path and exact
dialect/version. State which source is authoritative and write "None" when no
machine-readable schema is useful.]

## Behavioral guarantees

[Define preconditions, postconditions, invariants, ordering, concurrency,
atomicity, idempotency, timeouts, cancellation, state transitions, and other
consumer-visible semantics.]

## Errors and failure semantics

[Define error categories, exit/status behavior, retryability, partial effects,
recovery responsibilities, and behavior for malformed or unsupported input.]

## Security and authority

[Define caller identity, authorization, trust boundaries, confidentiality,
integrity, sensitive-data handling, resource scope, and audit obligations.]

## Compatibility and verification

[Define contract versioning, compatibility, deprecation and removal policy,
negotiation or migration behavior, contract tests, conformance evidence, and
links to requirements and ADRs.]
"#;

/// Deterministic template for a component design.
pub const COMPONENT_DESIGN_TEMPLATE: &str = r#"<!-- kvist-design-version: 1 -->
# Component Design

The design explains how this component will satisfy its approved requirements
and contract. It is maintainer context, not a consumer-facing promise.

## Design overview

[Summarize the chosen approach, governing requirements and contract, design
drivers, assumptions, and alternatives deliberately left open.]

## Internal structure

[Describe private modules or child components, their responsibilities, owned
state, allowed dependencies, and the rationale for the decomposition.]

## Interactions and state

[Describe internal data flow, runtime sequences, lifecycle and state-machine
transitions, consistency boundaries, and concurrency ownership.]

## Algorithms and decisions

[Describe non-obvious algorithms, validation order, bounded behavior, and
important trade-offs. Link architecturally significant choices to ADRs.]

## Failure and recovery

[Describe failure paths, propagation, cleanup, crash consistency, retries,
rollback or compensation, and behavior under dependency failure.]

## Security and resource design

[Describe enforcement mechanisms for trust boundaries, authorization, secrets,
input validation, filesystem/process/network access, memory, time, and output
bounds.]

## Verification strategy

[Map requirements and contract clauses to unit, integration, contract,
property, platform, security, and compliance checks. Identify required test
fixtures and independent evidence.]
"#;

/// One of the three normative component intent documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DocumentKind {
    /// Testable component outcomes and constraints.
    Requirements,
    /// Consumer-visible provided and required boundary.
    Contract,
    /// Internal strategy for satisfying requirements and contract.
    Design,
}

impl DocumentKind {
    /// Returns the required adjacent filename.
    pub const fn filename(self) -> &'static str {
        match self {
            Self::Requirements => "REQUIREMENTS.md",
            Self::Contract => "CONTRACT.md",
            Self::Design => "DESIGN.md",
        }
    }

    /// Returns the document's current schema version.
    pub const fn version(self) -> u32 {
        match self {
            Self::Requirements => REQUIREMENTS_VERSION,
            Self::Contract => CONTRACT_VERSION,
            Self::Design => DESIGN_VERSION,
        }
    }

    const fn marker(self) -> &'static str {
        match self {
            Self::Requirements => "kvist-requirements-version",
            Self::Contract => "kvist-contract-version",
            Self::Design => "kvist-design-version",
        }
    }

    const fn template(self) -> &'static str {
        match self {
            Self::Requirements => COMPONENT_REQUIREMENTS_TEMPLATE,
            Self::Contract => COMPONENT_CONTRACT_TEMPLATE,
            Self::Design => COMPONENT_DESIGN_TEMPLATE,
        }
    }

    const fn required_sections(self) -> &'static [&'static str] {
        match self {
            Self::Requirements => &[
                "## Purpose and scope",
                "## Stakeholders and concerns",
                "## Functional requirements",
                "## Quality requirements and constraints",
                "## Acceptance and traceability",
            ],
            Self::Contract => &[
                "## Boundary and ownership",
                "## Provided interfaces",
                "## Required interfaces",
                "## Data and schemas",
                "## Behavioral guarantees",
                "## Errors and failure semantics",
                "## Security and authority",
                "## Compatibility and verification",
            ],
            Self::Design => &[
                "## Design overview",
                "## Internal structure",
                "## Interactions and state",
                "## Algorithms and decisions",
                "## Failure and recovery",
                "## Security and resource design",
                "## Verification strategy",
            ],
        }
    }
}

/// The class of a component-document validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentDiagnosticKind {
    /// The version marker is absent from line one.
    MissingTemplateVersion,
    /// The version marker is not a positive integer.
    InvalidTemplateVersion,
    /// The marker declares a well-formed unsupported version.
    UnsupportedTemplateVersion {
        /// Version found in the document.
        found: u32,
        /// Version supported by this binary.
        supported: u32,
    },
    /// A required section is absent.
    MissingSection {
        /// Exact required Markdown heading.
        heading: &'static str,
    },
    /// A required section occurs more than once.
    DuplicateSection {
        /// Exact duplicated Markdown heading.
        heading: &'static str,
    },
    /// Required sections are not in their prescribed order.
    InvalidSectionOrder {
        /// Exact out-of-order Markdown heading.
        heading: &'static str,
    },
    /// A required section has no content before the next heading.
    EmptySection {
        /// Exact empty Markdown heading.
        heading: &'static str,
    },
}

/// A structured, one-based validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentDiagnostic {
    /// Classification of the failure.
    pub kind: DocumentDiagnosticKind,
    /// One-based source line.
    pub line: usize,
    /// One-based source column.
    pub column: usize,
}

/// Validation result for one component intent document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentValidation {
    /// Kind validated by the caller.
    pub kind: DocumentKind,
    /// Version declared by the source when parseable.
    pub template_version: Option<u32>,
    /// All diagnostics in deterministic source order.
    pub diagnostics: Vec<DocumentDiagnostic>,
}

impl DocumentValidation {
    /// Returns true when no diagnostics were found.
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Paths created for a new component documentation set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedComponentDocuments {
    /// Generated paths in requirements, contract, design order.
    pub paths: Vec<PathBuf>,
}

/// Creates all three validated component intent documents without overwriting.
pub fn create(component_dir: &Path) -> Result<GeneratedComponentDocuments> {
    ensure_component_directory(component_dir)?;
    let kinds = [
        DocumentKind::Requirements,
        DocumentKind::Contract,
        DocumentKind::Design,
    ];
    let paths = kinds
        .iter()
        .map(|kind| component_dir.join(kind.filename()))
        .collect::<Vec<_>>();
    for path in &paths {
        match fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(KvistError::ComponentDocumentAlreadyExists { path: path.clone() });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(KvistError::Io {
                    operation: "inspect component document destination",
                    path: path.clone(),
                    source,
                });
            }
        }
    }

    for (kind, path) in kinds.into_iter().zip(&paths) {
        let validation = validate(kind, kind.template());
        if !validation.is_valid() {
            return Err(KvistError::GeneratedComponentDocumentInvalid {
                document: kind.filename(),
                diagnostics: format_diagnostics(&validation.diagnostics),
            });
        }
        write_new_file_atomically(path, kind.template())?;
    }

    Ok(GeneratedComponentDocuments { paths })
}

/// Validates one component document without rewriting it.
pub fn validate(kind: DocumentKind, contents: &str) -> DocumentValidation {
    let lines = contents.lines().collect::<Vec<_>>();
    let eof_line = lines.len() + 1;
    let mut diagnostics = Vec::new();
    let template_version = validate_template_version(kind, &lines, &mut diagnostics);
    let mut previous_line = 0;

    for heading in kind.required_sections() {
        let occurrences = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| **line == *heading)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        match occurrences.as_slice() {
            [] => diagnostics.push(DocumentDiagnostic {
                kind: DocumentDiagnosticKind::MissingSection { heading },
                line: eof_line,
                column: 1,
            }),
            [index] => {
                let line = index + 1;
                if line < previous_line {
                    diagnostics.push(DocumentDiagnostic {
                        kind: DocumentDiagnosticKind::InvalidSectionOrder { heading },
                        line,
                        column: 1,
                    });
                }
                previous_line = previous_line.max(line);
                if !section_has_content(&lines, *index) {
                    diagnostics.push(DocumentDiagnostic {
                        kind: DocumentDiagnosticKind::EmptySection { heading },
                        line,
                        column: 1,
                    });
                }
            }
            [first, rest @ ..] => {
                let duplicate = rest[0];
                diagnostics.push(DocumentDiagnostic {
                    kind: DocumentDiagnosticKind::DuplicateSection { heading },
                    line: duplicate + 1,
                    column: 1,
                });
                previous_line = previous_line.max(first + 1);
            }
        }
    }

    diagnostics.sort_by_key(|diagnostic| (diagnostic.line, diagnostic.column));
    DocumentValidation {
        kind,
        template_version,
        diagnostics,
    }
}

/// Reads and validates one bounded UTF-8 regular component document.
pub fn validate_file(kind: DocumentKind, path: &Path) -> Result<DocumentValidation> {
    let metadata = fs::symlink_metadata(path).map_err(|source| KvistError::Io {
        operation: "inspect component document",
        path: path.to_path_buf(),
        source,
    })?;
    if is_link_like(&metadata) {
        return Err(KvistError::ComponentDocumentIsSymlink {
            path: path.to_path_buf(),
        });
    }
    if !metadata.file_type().is_file() {
        return Err(KvistError::ComponentDocumentNotFile {
            path: path.to_path_buf(),
        });
    }
    if metadata.len() > MAX_COMPONENT_DOCUMENT_BYTES {
        return Err(KvistError::ComponentDocumentTooLarge {
            path: path.to_path_buf(),
            max_bytes: MAX_COMPONENT_DOCUMENT_BYTES,
        });
    }
    let contents = fs::read_to_string(path).map_err(|source| KvistError::Io {
        operation: "read component document",
        path: path.to_path_buf(),
        source,
    })?;
    Ok(validate(kind, &contents))
}

/// Renders structured diagnostics in stable human-readable form.
pub fn format_diagnostics(diagnostics: &[DocumentDiagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{}:{}: {}",
                diagnostic.line,
                diagnostic.column,
                diagnostic_message(&diagnostic.kind)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn validate_template_version(
    kind: DocumentKind,
    lines: &[&str],
    diagnostics: &mut Vec<DocumentDiagnostic>,
) -> Option<u32> {
    let expected_prefix = format!("<!-- {}: ", kind.marker());
    let Some(first_line) = lines.first() else {
        diagnostics.push(DocumentDiagnostic {
            kind: DocumentDiagnosticKind::MissingTemplateVersion,
            line: 1,
            column: 1,
        });
        return None;
    };
    let Some(version_text) = first_line
        .strip_prefix(&expected_prefix)
        .and_then(|value| value.strip_suffix(" -->"))
    else {
        diagnostics.push(DocumentDiagnostic {
            kind: DocumentDiagnosticKind::MissingTemplateVersion,
            line: 1,
            column: 1,
        });
        return None;
    };
    let Ok(version) = version_text.parse::<u32>() else {
        diagnostics.push(DocumentDiagnostic {
            kind: DocumentDiagnosticKind::InvalidTemplateVersion,
            line: 1,
            column: expected_prefix.len() + 1,
        });
        return None;
    };
    if version == 0 {
        diagnostics.push(DocumentDiagnostic {
            kind: DocumentDiagnosticKind::InvalidTemplateVersion,
            line: 1,
            column: expected_prefix.len() + 1,
        });
    } else if version != kind.version() {
        diagnostics.push(DocumentDiagnostic {
            kind: DocumentDiagnosticKind::UnsupportedTemplateVersion {
                found: version,
                supported: kind.version(),
            },
            line: 1,
            column: expected_prefix.len() + 1,
        });
    }
    Some(version)
}

fn section_has_content(lines: &[&str], heading_index: usize) -> bool {
    lines
        .iter()
        .skip(heading_index + 1)
        .take_while(|line| !line.starts_with('#'))
        .any(|line| !line.trim().is_empty())
}

fn diagnostic_message(kind: &DocumentDiagnosticKind) -> String {
    match kind {
        DocumentDiagnosticKind::MissingTemplateVersion => {
            "expected the document version marker on line 1".to_owned()
        }
        DocumentDiagnosticKind::InvalidTemplateVersion => {
            "document version must be a positive integer".to_owned()
        }
        DocumentDiagnosticKind::UnsupportedTemplateVersion { found, supported } => {
            format!("unsupported document version {found}; supported version is {supported}")
        }
        DocumentDiagnosticKind::MissingSection { heading } => {
            format!("missing required section `{heading}`")
        }
        DocumentDiagnosticKind::DuplicateSection { heading } => {
            format!("required section `{heading}` appears more than once")
        }
        DocumentDiagnosticKind::InvalidSectionOrder { heading } => {
            format!("required section `{heading}` is out of order")
        }
        DocumentDiagnosticKind::EmptySection { heading } => {
            format!("required section `{heading}` must contain content")
        }
    }
}

fn ensure_component_directory(component_dir: &Path) -> Result<()> {
    match fs::symlink_metadata(component_dir) {
        Ok(metadata) if is_link_like(&metadata) => {
            return Err(KvistError::ComponentDirectoryIsSymlink {
                path: component_dir.to_path_buf(),
            });
        }
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(KvistError::ComponentDirectoryNotDirectory {
                path: component_dir.to_path_buf(),
            });
        }
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(KvistError::Io {
                operation: "inspect component directory",
                path: component_dir.to_path_buf(),
                source,
            });
        }
    }
    fs::create_dir_all(component_dir).map_err(|source| KvistError::Io {
        operation: "create component directory",
        path: component_dir.to_path_buf(),
        source,
    })
}
