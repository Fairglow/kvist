//! Versioned templates for the artifacts at a Kvist project root.
//!
//! These templates contain no user-specific values, credentials, or license
//! terms. Filesystem creation belongs to the `init` command implementation.

use crate::component_documents::{
    COMPONENT_CONTRACT_TEMPLATE, COMPONENT_DESIGN_TEMPLATE, COMPONENT_REQUIREMENTS_TEMPLATE,
};

/// Current schema version for `kvist.toml`.
pub const CONFIGURATION_VERSION: u32 = 1;
/// Current document version for `VISION.md`.
pub const VISION_VERSION: u32 = 1;
/// Current document version for `ARCHITECTURE.md`.
pub const ARCHITECTURE_VERSION: u32 = 1;
/// Current document version for `ROOT_CONTRACT.md`.
pub const ROOT_CONTRACT_VERSION: u32 = 1;
/// Current schema version for `TODOS.yaml`.
pub const TODO_QUEUE_VERSION: u32 = 1;
/// Required filename for implementation records.
pub const IMPLEMENTATION_RECORD_FILENAME: &str = "IMPL.md";
/// Required root-relative path for the root implementation record.
pub const ROOT_IMPLEMENTATION_RECORD_PATH: &str = "src/IMPL.md";
/// Current document version for `IMPL.md`.
pub const IMPLEMENTATION_RECORD_VERSION: u32 = 1;
/// Version marker required on an implementation record.
pub const IMPLEMENTATION_RECORD_VERSION_MARKER: &str = "kvist-implementation-record-version";
/// Heading required in an implementation record.
pub const IMPLEMENTATION_RECORD_HEADING: &str = "# Component Implementation Record";

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

[discovery]
# Bounded, deterministic traversal limits. Each has a documented hard maximum.
max_depth = 64
max_directories = 10000
max_components = 10000
max_entries_per_directory = 10000
max_relative_path_bytes = 4096

[vcs]
# `auto` selects exactly one detected supported VCS. Set `git` or `jj` when
# both are present in a colocated checkout.
kind = "auto"

[llm]
# External LLM integration is opt-in. No provider is configured by default.
provider = "none"
"#;

const VISION: &str = r#"<!-- kvist-vision-version: 1 -->
# Project Vision

## Purpose

[Explain why the product should exist and the problem or opportunity it
addresses.]

## Outcomes and stakeholders

[Describe intended outcomes, principal stakeholders, and how success will be
recognized without prescribing architecture or implementation.]

## Scope and non-goals

[Define the product boundary, important exclusions, assumptions, and unresolved
product decisions.]

## Principles and priorities

[Record enduring product principles and the priority order used when goals
conflict.]
"#;

const ARCHITECTURE: &str = r#"<!-- kvist-architecture-version: 1 -->
# Project Architecture

This architecture description is inspired by ISO/IEC/IEEE 42010 and uses a
tailored arc42 structure with selective C4-compatible views. It identifies the
system decomposition and interfaces without duplicating component contracts or
private implementation designs.

## Scope, stakeholders, and concerns

[Identify the system of interest, actors, external systems, trust boundaries,
stakeholders, concerns, and explicit out-of-scope areas.]

## Architectural drivers and constraints

[Record the requirements, measurable quality goals, principles, and imposed
constraints that determine the architecture.]

## Component model

[List stable component IDs, paths, responsibilities, non-responsibilities,
owned state, and provided or required contract IDs. Explain the reason for the
decomposition.]

## Interactions and dependency rules

[Describe allowed dependency direction and the important runtime or workflow
interactions. Link to provider-owned CONTRACT.md files instead of repeating
their definitions.]

## Cross-cutting policies

[Describe system-wide security, authority, persistence, schema, compatibility,
observability, error, resource, and verification policies.]

## Views and diagrams

[Provide only useful context, container, component, runtime, deployment, data,
or security views. State each view's audience and concern. Diagrams must use
the same stable identifiers as component documents.]

## Decisions, risks, and traceability

[Link significant choices to ADRs. Record known risks and map architectural
drivers to components, contracts, requirements, and verification evidence.]

## Standards and interoperability

[Link the project's standards profile and identify any architecture or
interface formats that are normative, imported, exported, or deliberately
deferred.]
"#;

const ROOT_CONTRACT: &str = r#"<!-- kvist-root-contract-version: 1 -->
# Kvist Root Contract

This contract applies to every component in this project. It is the global
constraint set injected into component work.

## Non-negotiable architecture

- Approve `VISION.md` and `ARCHITECTURE.md` before detailed component work.
- Define and validate each component's requirements, consumer contract, design,
  constraints, acceptance criteria, and verification strategy before
  implementation.
- Keep each component's `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`,
  `TODOS.yaml`, `IMPL.md`, and implementation adjacent in its directory.
- Persist architecture and workflow state in version-controlled project files.
- Keep component context limited to local artifacts, explicitly required
  provider contracts, the immediate parent `CONTRACT.md`, and this root
  contract. Never expose peer or parent implementation or design by default.

## Change and compliance rules

- `TODOS.yaml` orders work as tests, implementation, security audit, then
  compliance review.
- Requirements state what must be achieved, contracts state what consumers may
  rely on, and designs state how the component intends to satisfy them. Do not
  duplicate normative facts across these artifacts.
- `IMPL.md` describes independently observed implementation behavior and is not
  copied from intended requirements, contracts, or designs.
- A clean-slate documenter and a separate compliance reviewer must verify
  implemented behavior before it is declared compliant.
- Record intent-to-implementation discrepancies for explicit
  arbitration; do not silently alter either artifact.
"#;

const ROOT_TODOS: &str = r#"schema_version: 1
component:
  requirements_revision: sha256:bd53663c2dc76fdcbe58b111c0174a7550a3e3fe773a1c3e4a14196c1089dfa0
  contract_revision: sha256:54b07fd8cbfb911f7e8546854b49944eb499429ea14cf302c2cba0f64238b98c
  design_revision: sha256:6d6579ce1b018dce3b34e87afd72b494d27692ada8d54003b0a10ecd74abed17
  parent_contract: null
  revalidation:
    state: current
    checked_at: 2026-01-01T00:00:00Z
    stale_since: null
    causes: []
tasks: []
"#;

const ROOT_IMPLEMENTATION_RECORD: &str = r#"<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

This document is produced by reverse-engineering implemented behavior without
using the component requirements, contract, or design. It must describe only
behavior observable from the implementation.

## Observed public contract

[Document after independent code inspection.]

## Observed requirements and constraints

[Document after independent code inspection.]

## Observed internal design and failure paths

[Document after independent code inspection.]
"#;

const ROOT_ARTIFACTS: [ArtifactTemplate; 9] = [
    ArtifactTemplate {
        relative_path: "kvist.toml",
        contents: KVIST_TOML,
    },
    ArtifactTemplate {
        relative_path: "VISION.md",
        contents: VISION,
    },
    ArtifactTemplate {
        relative_path: "ARCHITECTURE.md",
        contents: ARCHITECTURE,
    },
    ArtifactTemplate {
        relative_path: "ROOT_CONTRACT.md",
        contents: ROOT_CONTRACT,
    },
    ArtifactTemplate {
        relative_path: "src/REQUIREMENTS.md",
        contents: COMPONENT_REQUIREMENTS_TEMPLATE,
    },
    ArtifactTemplate {
        relative_path: "src/CONTRACT.md",
        contents: COMPONENT_CONTRACT_TEMPLATE,
    },
    ArtifactTemplate {
        relative_path: "src/DESIGN.md",
        contents: COMPONENT_DESIGN_TEMPLATE,
    },
    ArtifactTemplate {
        relative_path: "src/TODOS.yaml",
        contents: ROOT_TODOS,
    },
    ArtifactTemplate {
        relative_path: ROOT_IMPLEMENTATION_RECORD_PATH,
        contents: ROOT_IMPLEMENTATION_RECORD,
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
                "ARCHITECTURE.md",
                "VISION.md",
                "kvist.toml",
                ROOT_IMPLEMENTATION_RECORD_PATH,
                "src/CONTRACT.md",
                "src/DESIGN.md",
                "src/REQUIREMENTS.md",
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
        assert_eq!(config["vcs"]["kind"].as_str(), Some("auto"));
        assert_eq!(config["llm"]["provider"].as_str(), Some("none"));
    }

    #[test]
    fn markdown_templates_are_versioned_and_follow_the_lifecycle() {
        assert!(VISION.starts_with("<!-- kvist-vision-version: 1 -->"));
        assert!(ARCHITECTURE.starts_with("<!-- kvist-architecture-version: 1 -->"));
        assert!(ROOT_CONTRACT.starts_with("<!-- kvist-root-contract-version: 1 -->"));
        assert!(
            COMPONENT_REQUIREMENTS_TEMPLATE.starts_with("<!-- kvist-requirements-version: 1 -->")
        );
        assert!(COMPONENT_CONTRACT_TEMPLATE.starts_with("<!-- kvist-contract-version: 1 -->"));
        assert!(COMPONENT_DESIGN_TEMPLATE.starts_with("<!-- kvist-design-version: 1 -->"));
        assert!(
            ROOT_IMPLEMENTATION_RECORD
                .starts_with("<!-- kvist-implementation-record-version: 1 -->")
        );

        assert!(ARCHITECTURE.contains("ISO/IEC/IEEE 42010"));
        assert!(COMPONENT_CONTRACT_TEMPLATE.contains("## Data and schemas"));
        assert!(COMPONENT_DESIGN_TEMPLATE.contains("## Verification strategy"));
        assert!(
            ROOT_IMPLEMENTATION_RECORD
                .contains("without\nusing the component requirements, contract, or design")
        );
    }

    #[test]
    fn todo_template_uses_the_current_schema_and_is_valid() {
        assert!(ROOT_TODOS.starts_with("schema_version: 1\n"));
        crate::task_queue::parse(ROOT_TODOS).expect("valid TODO queue template");
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
