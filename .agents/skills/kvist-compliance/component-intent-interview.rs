//! Skill contract: component-intent-interview.
//!
//! This skill guides the architect through a terminal workflow to produce
//! bounded, reviewable requirements, contract, and design drafts without
//! silent product decisions.

/// Inputs for the component-intent-interview skill.
pub struct ComponentIntentInterviewInputs {
    /// The global root contract.
    pub root_contract: String,
    /// The approved project architecture.
    pub architecture: String,
    /// The immediate parent consumer contract, if this is a child component.
    pub parent_contract: Option<String>,
    /// The component directory path.
    pub component_path: String,
    /// Whether to use an external agent for drafting.
    pub use_agent: bool,
    /// The interview scope, such as outcomes, interfaces, or realization.
    pub scope: String,
    /// Existing requirements draft to extend.
    pub existing_requirements: Option<String>,
    /// Existing consumer contract draft to extend.
    pub existing_contract: Option<String>,
    /// Existing private design draft to extend.
    pub existing_design: Option<String>,
}

/// Outputs of the component-intent-interview skill.
pub struct ComponentIntentInterviewOutputs {
    /// Draft component outcomes, constraints, acceptance, and verification.
    pub draft_requirements: String,
    /// Draft consumer-facing interface and behavior.
    pub draft_contract: String,
    /// Draft private realization.
    pub draft_design: String,
    /// Whether the draft was rejected (empty string means accepted).
    pub rejection_reason: String,
    /// Traceability from interview questions to artifact sections.
    pub traceability: String,
}

/// The skill contract for component-intent interviews.
pub const CONTRACT: &'static str = r#"
# Skill: component-intent-interview
# Role: architect
#
# Contract:
#   Inputs: root contract, approved architecture, immediate parent CONTRACT.md
#           (optional), component path, agent usage flag, interview scope, and
#           existing REQUIREMENTS.md, CONTRACT.md, and DESIGN.md drafts.
#   Output: three draft intent documents with question-to-section traceability.
#   Approval: the human reviews all three before `component accept`.
#   Independence: the immediate parent CONTRACT.md is the only implicit
#               propagated component context. It is the nearest ancestor
#               component across transparent namespace directories. The skill
#               does not read parent requirements/design, peer artifacts, or
#               implementation.
#
# Constraints:
#   - Outcomes, constraints, acceptance, and verification go in REQUIREMENTS.md.
#   - Consumer-visible interfaces and behavior go in CONTRACT.md.
#   - Private structure, algorithms, state, and recovery go in DESIGN.md.
#   - Optional native schemas are referenced from CONTRACT.md by exact path and
#     dialect/version.
#   - All drafts must pass normal component-document validation.
#   - The skill refuses to overwrite existing intent documents.
#   - The skill refuses when the root contract is absent.
#
# Failure paths:
#   - Refusal: root contract is absent.
#   - Refusal: an existing intent set is already accepted.
#   - Ambiguity: the interview scope is underspecified.
#
# Version: 1
"#;

/// Component-intent interview skill version.
pub const SKILL_VERSION: u32 = 1;

/// The component-intent interview skill schema.
pub const SCHEMA: &'static str = r#"
{
  "skill_name": "component-intent-interview",
  "role": "architect",
  "version": 1,
  "inputs": {
    "root_contract": "string",
    "architecture": "string",
    "parent_contract": "string (optional)",
    "component_path": "string",
    "use_agent": "boolean",
    "scope": "string",
    "existing_requirements": "string (optional)",
    "existing_contract": "string (optional)",
    "existing_design": "string (optional)"
  },
  "outputs": {
    "draft_requirements": "string",
    "draft_contract": "string",
    "draft_design": "string",
    "rejection_reason": "string",
    "traceability": "string"
  },
  "approval_gate": "human",
  "independence_boundaries": {
    "implicit_parent_context": "nearest ancestor component CONTRACT.md across transparent namespace directories",
    "excluded": ["parent REQUIREMENTS.md", "parent DESIGN.md", "peer artifacts", "implementation files", "agent chat history"]
  },
  "constraints": {
    "artifact_authority": "requirements=outcomes/constraints/acceptance; contract=consumer semantics; design=private realization",
    "schema_references": "optional native schemas are referenced from CONTRACT.md with exact path and dialect/version",
    "validation": "all drafts pass component-document validation",
    "no_overwrite": "refuses to overwrite existing intent documents",
    "refusal_on_absent": "refuses when root contract is absent"
  },
  "failure_paths": [
    "refusal: root contract is absent",
    "refusal: existing intent set is already accepted",
    "ambiguity: interview scope is underspecified"
  ]
}
"#;
