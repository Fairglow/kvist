//! Skill contract: human-arbitration.
//!
//! This skill presents compliance mismatches as explicit human choices
//! with proposed redesign, contract change, or manual arbitration options.

/// Inputs for the human-arbitration skill.
pub struct HumanArbitrationInputs {
    /// The original specification.
    pub specification: String,
    /// The independently generated implementation documentation.
    pub impl_documentation: String,
    /// The compliance checker's structured findings.
    pub findings: String,
    /// The retained discrepancy details (optional).
    pub discrepancy: Option<String>,
    /// Whether to request redesign or contract change.
    pub request_redesign: bool,
}

/// Outputs of the human-arbitration skill.
pub struct HumanArbitrationOutputs {
    /// The selected action (redesign, contract change, manual arbitration).
    pub action: String,
    /// The proposed change or arbitration record.
    pub proposal: String,
    /// Whether the output was rejected (empty string means accepted).
    pub rejection_reason: String,
}

/// The skill contract for human arbitration.
pub const CONTRACT: &'static str = r#"
# Skill: human-arbitration
# Role: architect
#
# Contract:
#   Inputs: original specification, independently generated IMPL.md,
#           compliance checker findings, discrepancy details (optional),
#           redesign request flag.
#   Output: selected action (redesign, contract change, manual arbitration)
#            and proposal record.
#   Approval: the human must review and accept before revalidation.
#   Independence: the skill only reads approved artifacts; it does not
#               read agent chat history or implementation source.
#
# Constraints:
#   - Never automatically rewrite SPEC.md or IMPL.md.
#   - The proposal must be traceable to findings.
#   - The skill refuses when findings are ambiguous or underspecified.
#
# Failure paths:
#   - Refusal: findings are ambiguous or underspecified.
#   - Schema error: the proposal fails validation.
#
# Version: 1
"#;

/// Human-arbitration skill version.
pub const SKILL_VERSION: u32 = 1;

/// The human-arbitration skill schema.
pub const SCHEMA: &'static str = r#"
{
  "skill_name": "human-arbitration",
  "role": "architect",
  "version": 1,
  "inputs": {
    "specification": "string",
    "impl_documentation": "string",
    "findings": "string",
    "discrepancy": "string (optional)",
    "request_redesign": "boolean"
  },
  "outputs": {
    "action": "string",
    "proposal": "string",
    "rejection_reason": "string"
  },
  "approval_gate": "human",
  "independence_boundaries": {
    "excluded": ["agent chat history", "implementation source"]
  },
  "constraints": {
    "no_automated_rewrite": "never rewrites SPEC.md or IMPL.md",
    "traceability": "proposal must be traceable to findings",
    "refusal_on_ambiguity": "refuses when findings are ambiguous"
  },
  "failure_paths": [
    "refusal: findings are ambiguous or underspecified",
    "schema_error: proposal fails validation"
  ]
}
"#;
