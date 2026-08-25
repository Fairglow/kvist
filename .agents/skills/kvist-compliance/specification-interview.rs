//! Skill contract: specification-interview.
//!
//! This skill guides the architect through a terminal workflow to produce
//! a bounded, reviewable specification draft without silent product decisions.

/// Inputs for the specification-interview skill.
pub struct SpecificationInterviewInputs {
    /// The component root contract.
    pub root_contract: String,
    /// The component directory path.
    pub component_path: String,
    /// Whether to use an external agent for drafting.
    pub use_agent: bool,
    /// The interview scope (e.g., "purpose", "interfaces", "algorithms").
    pub scope: String,
    /// Existing specification to extend (optional).
    pub existing_specification: Option<String>,
}

/// Outputs of the specification-interview skill.
pub struct SpecificationInterviewOutputs {
    /// The draft specification.
    pub draft_specification: String,
    /// Whether the draft was rejected (empty string means accepted).
    pub rejection_reason: String,
    /// Traceability from interview questions to specification sections.
    pub traceability: String,
}

/// The skill contract for specification interviews.
pub const CONTRACT: &'static str = r#"
# Skill: specification-interview
# Role: architect
#
# Contract:
#   Inputs: root contract, component path, agent usage flag, interview scope,
#           existing specification (optional).
#   Output: a draft specification with traceability from questions to sections.
#   Approval: the human must review and accept before `spec accept`.
#   Independence: the skill only reads the root contract and existing
#               specification; it does not read peer implementations.
#
# Constraints:
#   - The draft must pass normal specification validation.
#   - The skill refuses to overwrite an existing specification without
#     explicit `spec accept`.
#   - The skill refuses when the root contract is absent.
#
# Failure paths:
#   - Refusal: root contract is absent.
#   - Refusal: existing specification is already accepted.
#   - Ambiguity: the interview scope is underspecified.
#
# Version: 1
"#;

/// Specification interview skill version.
pub const SKILL_VERSION: u32 = 1;

/// The specification interview skill schema.
pub const SCHEMA: &'static str = r#"
{
  "skill_name": "specification-interview",
  "role": "architect",
  "version": 1,
  "inputs": {
    "root_contract": "string",
    "component_path": "string",
    "use_agent": "boolean",
    "scope": "string",
    "existing_specification": "string (optional)"
  },
  "outputs": {
    "draft_specification": "string",
    "rejection_reason": "string",
    "traceability": "string"
  },
  "approval_gate": "human",
  "independence_boundaries": {
    "excluded": ["peer implementation files", "agent chat history"]
  },
  "constraints": {
    "validation": "draft must pass normal specification validation",
    "no_overwrite": "refuses to overwrite existing specification",
    "refusal_on_absent": "refuses when root contract is absent"
  },
  "failure_paths": [
    "refusal: root contract is absent",
    "refusal: existing specification is already accepted",
    "ambiguity: interview scope is underspecified"
  ]
}
"#;
