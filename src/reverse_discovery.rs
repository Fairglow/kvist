//! Reverse-discovery pipeline to generate Kvist specifications and task queues from existing source code.

use std::{
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{
    KvistError, Result, file_io::write_new_file_atomically, filesystem::is_link_like,
    specification, task_queue,
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

    // 2. Prevent overwriting accepted or existing specification
    let metadata_directory = path.join(METADATA_DIRECTORY);
    let target_spec = metadata_directory.join("SPEC.md");
    if target_spec.exists() {
        return Err(KvistError::ImportFailed {
            reason: format!(
                "refusing to overwrite existing specification at {}",
                target_spec.display()
            ),
        });
    }

    // Also check adjacent src/SPEC.md
    let src_spec = path.join("src/SPEC.md");
    if src_spec.exists() {
        return Err(KvistError::ImportFailed {
            reason: format!(
                "refusing to overwrite existing specification at {}",
                src_spec.display()
            ),
        });
    }

    // 3. Scan codebase recursively for symbols
    let mut symbols = DiscoveredSymbols::default();
    scan_directory(path, &mut symbols)?;

    // 4. Generate specification markdown
    let spec_content = generate_spec(&symbols);
    if !specification::validate(&spec_content).is_valid() {
        return Err(KvistError::GeneratedSpecificationInvalid {
            diagnostics: "reverse-discovered specification template failed validation".to_owned(),
        });
    }

    // Calculate spec hash for TODOS/IMPL
    let mut hasher = Sha256::new();
    hasher.update(spec_content.as_bytes());
    let spec_hash = hex::encode(hasher.finalize());

    // 5. Generate TODOS and IMPL
    let todos_content = generate_todos(&symbols, &spec_hash);
    task_queue::parse(&todos_content).map_err(|error| KvistError::TaskQueueUnavailable {
        path: metadata_directory.join("TODOS.yaml"),
        reason: format!("generated reverse-discovered queue is invalid: {error}"),
    })?;

    let impl_content = generate_impl_record(&symbols, &spec_hash);

    // 6. Write artifacts to .kvist
    if !metadata_directory.exists() {
        fs::create_dir(&metadata_directory).map_err(|source| KvistError::Io {
            operation: "create metadata directory",
            path: metadata_directory.clone(),
            source,
        })?;
    }

    write_new_file_atomically(&target_spec, &spec_content)?;
    write_new_file_atomically(&metadata_directory.join("TODOS.yaml"), &todos_content)?;
    write_new_file_atomically(&metadata_directory.join("IMPL.md"), &impl_content)?;
    write_new_file_atomically(
        &metadata_directory.join("COMPLIANCE_REVIEW.md"),
        "<!-- kvist-compliance-review-version: 1 -->\n# Compliance Review\n",
    )?;

    Ok(ReverseDiscoverOutcome::Discovered {
        path: path.to_path_buf(),
    })
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
                if name != "SPEC.md" && name != "IMPL.md" && name != "COMPLIANCE_REVIEW.md" {
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

fn generate_spec(symbols: &DiscoveredSymbols) -> String {
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

    format!(
        r#"<!-- kvist-specification-version: 1 -->
# Reverse-Discovered Specification

<details open>
<summary>Layer 1: Executive summary and public contract</summary>

## Purpose

Automatically reverse-discovered and generated specification for the components in this directory.

## Public contract

The following public interfaces and symbols were extracted from implementation files:

{exports}

</details>

<details>
<summary>Layer 2: Architectural guarantees</summary>

## Constraints and invariants

The existing codebase consists of the following identified files:

{files}

The following supplemental documentation files were discovered:

{docs}

</details>

<details>
<summary>Layer 3: Detailed strategy and algorithms</summary>

## Design and failure paths

The existing codebase contains the following reverse-discovered unit/integration tests:

{tests}

</details>
"#
    )
}

fn generate_todos(symbols: &DiscoveredSymbols, spec_hash: &str) -> String {
    let summary = if symbols.pub_exports.is_empty() {
        "Empty interface set"
    } else {
        "Discovered public exports"
    };

    format!(
        r#"schema_version: 1
component:
  specification_revision: "sha256:{spec_hash}"
  parent_specification: null
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
      - "SPEC.md#Public-contract"
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
    purpose: "Align existing implementation with a human-approved specification."
    expected_outcome: "Clean compilation and zero compliance violations."
    kind: implementation
    status: pending
    depends_on:
      - "define-discovered-tests"
    requirements:
      - "SPEC.md#Public-contract"
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
      - "SPEC.md#Public-contract"
    timestamps:
      created_at: "2026-08-28T13:03:02Z"
      updated_at: "2026-08-28T13:03:02Z"
      completed_at: null
    blocked_reason: null
    recovery_state: null
  - id: "review-discovered-spec"
    title: "Review reverse-discovered specification"
    description: "Align the generated SPEC.md file with human-defined architectural constraints."
    context: "Summary: {summary}."
    purpose: "Validate that the reverse-discovered specification matches the expected component contract."
    expected_outcome: "Spec matches architecture and SPEC.md is human-reviewed."
    kind: compliance-review
    status: pending
    depends_on:
      - "audit-discovered-boundaries"
    requirements:
      - "SPEC.md#Public-contract"
    timestamps:
      created_at: "2026-08-28T13:03:02Z"
      updated_at: "2026-08-28T13:03:02Z"
      completed_at: null
    blocked_reason: null
    recovery_state: null
"#
    )
}

fn generate_impl_record(symbols: &DiscoveredSymbols, spec_hash: &str) -> String {
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
# Root Component Implementation Record

- **Component Specification Revision**: `sha256:{spec_hash}`
- **Verified At**: `2026-08-28T13:03:02Z`
- **Result**: `completed`

## Discovered Implementation Files

The following files form the core implementation:

{files}
"#
    )
}
