//! Skill contract: clean-slate-documentation.
//!
//! This skill generates `IMPL.md` from implementation source, tests, and
//! manifests without reading the specification, producing an observed contract
//! that reports uncertainty and never copies planned requirements.

/// Inputs for the clean-slate-documentation skill.
pub struct CleanSlateDocumentationInputs {
    /// The implementation source code.
    pub implementation: String,
    /// The generated test suite.
    pub tests: String,
    /// The manifest or Cargo.toml.
    pub manifest: String,
    /// Whether to include uncertainty markers.
    pub include_uncertainty: bool,
    /// The observed interfaces from the implementation.
    pub observed_interfaces: Vec<String>,
}

/// Outputs of the clean-slate-documentation skill.
pub struct CleanSlateDocumentationOutputs {
    /// The generated `IMPL.md`.
    pub impl_documentation: String,
    /// Whether the output was rejected (empty string means accepted).
    pub rejection_reason: String,
    /// The observed-contract record.
    pub observed_contract: String,
}

/// The skill contract for clean-slate documentation.
pub const CONTRACT: &'static str = r#"
# Skill: clean-slate-documentation
# Role: documenter
#
# Contract:
#   Inputs: implementation source, test suite, manifest, uncertainty flag,
#           observed interfaces.
#   Output: an observed-contract `IMPL.md` that describes what the
#           implementation actually does without referencing the specification.
#   Approval: the human must review the observed-contract record before
#           accepting.
#   Independence: the skill does not read `SPEC.md` or prior `IMPL.md`;
#               it only reads implementation artifacts.
#
# Constraints:
#   - The generated documentation must describe observed behavior, not
#     planned requirements.
#   - Uncertainty markers must be included for any requirement that lacks
#     corresponding implementation evidence.
#   - The skill refuses to copy planned requirements into implementation
#     evidence.
#   - The skill does not read `SPEC.md` or prior `IMPL.md`.
#
# Failure paths:
#   - Refusal: the implementation lacks sufficient evidence for a complete
#     observed-contract record.
#   - Schema error: the generated documentation fails validation.
#
# Version: 1
"#;

/// Clean-slate documentation skill version.
pub const SKILL_VERSION: u32 = 1;

/// The clean-slate documentation skill schema.
pub const SCHEMA: &'static str = r#"
{
  "skill_name": "clean-slate-documentation",
  "role": "documenter",
  "version": 1,
  "inputs": {
    "implementation": "string",
    "tests": "string",
    "manifest": "string",
    "include_uncertainty": "boolean",
    "observed_interfaces": "array of strings"
  },
  "outputs": {
    "impl_documentation": "string",
    "rejection_reason": "string",
    "observed_contract": "string"
  },
  "approval_gate": "human",
  "independence_boundaries": {
    "excluded": ["SPEC.md", "prior IMPL.md", "agent chat history"]
  },
  "constraints": {
    "observed_only": "describes actual implementation behavior",
    "no_planned_requirements": "never copies planned requirements into evidence",
    "uncertainty_markers": "includes markers for requirements without evidence",
    "no_spec_reading": "does not read SPEC.md or prior IMPL.md"
  },
  "failure_paths": [
    "refusal: implementation lacks sufficient evidence",
    "schema_error: generated documentation fails validation"
  ]
}
"#;
