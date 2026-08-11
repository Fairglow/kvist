//! Versioned templates for the artifacts at a Kvist project root.
//!
//! These templates contain no user-specific values, credentials, or license
//! terms. Filesystem creation belongs to the `init` command implementation.

/// Current schema version for `kvist.toml`.
pub const CONFIGURATION_VERSION: u32 = 1;
/// Current document version for `ROOT_CONTRACT.md`.
pub const ROOT_CONTRACT_VERSION: u32 = 1;
/// Current document version for `SPEC.md`.
pub const SPECIFICATION_VERSION: u32 = 1;
/// Current schema version for `TODOS.yaml`.
pub const TODO_QUEUE_VERSION: u32 = 1;
/// Current document version for `DOCS.md`.
pub const DOCUMENTATION_VERSION: u32 = 1;

/// A file generated when initializing a Kvist project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactTemplate {
    /// Path relative to the project root.
    pub relative_path: &'static str,
    /// Complete UTF-8 content to write to the artifact.
    pub contents: &'static str,
}

const KVIST_TOML: &str = r#"# Kvist project configuration.
# This schema is versioned independently of the Kvist binary.
schema_version = 1

# Directory containing the root component and its descendants.
component_root = "src"

[llm]
# External LLM integration is opt-in. No provider is configured by default.
provider = "none"
"#;

const ROOT_CONTRACT: &str = r#"<!-- kvist-root-contract-version: 1 -->
# Kvist Root Contract

This contract applies to every component in this project. It is the global
constraint set injected into component work.

## Non-negotiable architecture

- Define and validate a component's specification, public contract,
  constraints, and test strategy before implementation.
- Keep each component's `SPEC.md`, `TODOS.yaml`, `DOCS.md`, and implementation
  adjacent in its directory.
- Persist architecture and workflow state in version-controlled project files.
- Keep component context limited to the component, its immediate parent
  contract, and this root contract.

## Change and compliance rules

- `TODOS.yaml` orders work as tests, implementation, security audit, then
  compliance review.
- `DOCS.md` describes observed implementation behavior and is not copied from
  `SPEC.md`.
- A clean-slate documenter and a separate compliance reviewer must verify
  implemented behavior before it is declared compliant.
- Record specification-to-implementation discrepancies for explicit
  arbitration; do not silently alter either artifact.
"#;

const ROOT_SPEC: &str = r#"<!-- kvist-specification-version: 1 -->
# Root Component Specification

<details open>
<summary>Layer 1: Executive summary and public contract</summary>

## Purpose

[Describe the root component's purpose and why it exists.]

## Public contract

[Describe users, inputs, outputs, and observable behavior.]

</details>

<details>
<summary>Layer 2: Architectural guarantees</summary>

## Constraints and invariants

[Describe performance bounds, concurrency invariants, memory limits, dependency
policy, and compatibility commitments.]

</details>

<details>
<summary>Layer 3: Detailed strategy and algorithms</summary>

## Design and failure paths

[Describe algorithms, state transitions, validation, error handling, and edge
cases.]

</details>
"#;

const ROOT_TODOS: &str = r#"schema_version: 1
tasks:
  - id: write_tests
    status: pending
    description: Define failing tests for the component specification.
  - id: implement_code
    status: pending
    description: Implement the specified behavior until tests pass.
  - id: security_audit
    status: pending
    description: Review memory safety, input boundaries, and concurrency invariants.
  - id: compliance_review
    status: pending
    description: Run the independent documentation and compliance review.
"#;

const ROOT_DOCS: &str = r#"<!-- kvist-documentation-version: 1 -->
# Root Component Compliance Documentation

This document is produced by reverse-engineering implemented behavior without
using the component specification. It must describe only behavior observable
from the implementation.

## Observed public contract

[Document after independent code inspection.]

## Observed guarantees and constraints

[Document after independent code inspection.]

## Observed design and failure paths

[Document after independent code inspection.]
"#;

const ROOT_ARTIFACTS: [ArtifactTemplate; 5] = [
    ArtifactTemplate {
        relative_path: "kvist.toml",
        contents: KVIST_TOML,
    },
    ArtifactTemplate {
        relative_path: "ROOT_CONTRACT.md",
        contents: ROOT_CONTRACT,
    },
    ArtifactTemplate {
        relative_path: "src/SPEC.md",
        contents: ROOT_SPEC,
    },
    ArtifactTemplate {
        relative_path: "src/TODOS.yaml",
        contents: ROOT_TODOS,
    },
    ArtifactTemplate {
        relative_path: "src/DOCS.md",
        contents: ROOT_DOCS,
    },
];

/// Returns the complete, deterministic root artifact set for a new project.
pub fn root_artifacts() -> &'static [ArtifactTemplate] {
    &ROOT_ARTIFACTS
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn root_artifact_set_is_complete_unique_and_nonempty() {
        let paths = root_artifacts()
            .iter()
            .map(|artifact| artifact.relative_path)
            .collect::<BTreeSet<_>>();

        assert_eq!(
            paths,
            BTreeSet::from([
                "ROOT_CONTRACT.md",
                "kvist.toml",
                "src/DOCS.md",
                "src/SPEC.md",
                "src/TODOS.yaml",
            ])
        );
        assert!(
            root_artifacts()
                .iter()
                .all(|artifact| artifact.contents.ends_with('\n'))
        );
    }

    #[test]
    fn configuration_template_is_valid_and_has_safe_defaults() {
        let config: toml::Value = toml::from_str(KVIST_TOML).expect("valid configuration template");

        assert_eq!(
            config["schema_version"].as_integer(),
            Some(i64::from(CONFIGURATION_VERSION))
        );
        assert_eq!(config["component_root"].as_str(), Some("src"));
        assert_eq!(config["llm"]["provider"].as_str(), Some("none"));
    }

    #[test]
    fn markdown_templates_are_versioned_and_follow_the_lifecycle() {
        assert!(ROOT_CONTRACT.starts_with("<!-- kvist-root-contract-version: 1 -->"));
        assert!(ROOT_SPEC.starts_with("<!-- kvist-specification-version: 1 -->"));
        assert!(ROOT_DOCS.starts_with("<!-- kvist-documentation-version: 1 -->"));

        assert!(ROOT_SPEC.contains("Layer 1: Executive summary and public contract"));
        assert!(ROOT_SPEC.contains("Layer 2: Architectural guarantees"));
        assert!(ROOT_SPEC.contains("Layer 3: Detailed strategy and algorithms"));
        assert!(ROOT_DOCS.contains("without\nusing the component specification"));
    }

    #[test]
    fn todo_template_has_the_required_ordered_lifecycle_stages() {
        let task_ids = ROOT_TODOS
            .lines()
            .filter_map(|line| line.strip_prefix("  - id: "))
            .collect::<Vec<_>>();

        assert_eq!(
            task_ids,
            [
                "write_tests",
                "implement_code",
                "security_audit",
                "compliance_review"
            ]
        );
        assert!(ROOT_TODOS.starts_with("schema_version: 1\n"));
    }

    #[test]
    fn generated_artifacts_do_not_embed_license_terms_or_secrets() {
        let generated_contents = root_artifacts()
            .iter()
            .map(|artifact| artifact.contents)
            .collect::<String>();

        for prohibited_text in [
            "Business Source License",
            "MIT License",
            "Apache License",
            "api_key",
        ] {
            assert!(!generated_contents.contains(prohibited_text));
        }
    }
}
