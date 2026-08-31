//! Skill contract: clean-slate-documentation.
//!
//! This skill generates `IMPL.md` from implementation source, tests, and
//! manifests without reading any intent artifact, producing an observed record
//! that reports uncertainty and never copies planned behavior.

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
#   Output: an observed `IMPL.md` that describes what the implementation and
#           tests demonstrate without referencing intended behavior.
#   Approval: the human must review the observed-contract record before
#           accepting.
#   Independence: the skill reads only source, tests, manifests, and necessary
#               non-intent build configuration. It excludes REQUIREMENTS.md,
#               CONTRACT.md, DESIGN.md, TODOS.yaml, prior IMPL.md, VISION.md,
#               ARCHITECTURE.md, ROOT_CONTRACT.md, ADRs, prior reviews, chat,
#               and Git history.
#
# Constraints:
#   - The generated documentation must describe observed behavior, not
#     planned requirements.
#   - Uncertainty markers identify behavior not established by source or tests.
#   - The skill refuses to copy planned intent into implementation evidence.
#   - The implementer may review formatting and placement but cannot revise
#     the documenter's behavioral conclusions or certify compliance.
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
    "allowed": ["implementation source", "tests", "dependency manifests", "necessary non-intent build configuration"],
    "excluded": ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md", "TODOS.yaml", "prior IMPL.md", "VISION.md", "ARCHITECTURE.md", "ROOT_CONTRACT.md", "ADRs", "prior reviews", "agent chat history", "Git history"]
  },
  "constraints": {
    "observed_only": "describes actual implementation behavior",
    "no_planned_intent": "never copies intended behavior into evidence",
    "uncertainty_markers": "marks behavior not established by source or tests",
    "no_self_certification": "implementer cannot revise conclusions or certify compliance"
  },
  "failure_paths": [
    "refusal: implementation lacks sufficient evidence",
    "schema_error: generated documentation fails validation"
  ]
}
"#;
