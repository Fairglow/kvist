//! Skill contract: compliance-review.
//!
//! This skill compares local intent (requirements, contract, design) against
//! observed evidence (independently generated IMPL.md, bounded test evidence)
//! and produces a structured findings record. It is source-blind: it never
//! reads implementation source directly.

/// Inputs for the compliance-review skill.
pub struct ComplianceReviewInputs {
    /// The accepted component requirements.
    pub requirements: String,
    /// The accepted consumer contract.
    pub contract: String,
    /// The accepted private design.
    pub design: String,
    /// The independently generated implementation documentation.
    pub impl_documentation: String,
    /// The bounded test evidence (test results, coverage report).
    pub test_evidence: String,
    /// Whether to retain discrepancies for human arbitration.
    pub retain_discrepancies: bool,
}

/// Outputs of the compliance-review skill.
pub struct ComplianceReviewOutputs {
    /// The structured findings record.
    pub findings: String,
    /// The retained discrepancy details, if any.
    pub discrepancy: Option<String>,
    /// Whether the output was rejected (empty string means accepted).
    pub rejection_reason: String,
}

/// The skill contract for compliance review.
pub const CONTRACT: &'static str = r#"
# Skill: compliance-review
# Role: reviewer
#
# Contract:
#   Inputs: accepted REQUIREMENTS.md, CONTRACT.md, DESIGN.md; independently
#           generated IMPL.md; bounded test evidence; retain-discrepancies flag.
#   Output: a structured findings record comparing intent against observed
#           evidence, with retained discrepancy details when requested.
#   Approval: the human must review findings before accepting.
#   Independence: the skill reads only approved intent artifacts and observed
#               evidence. It excludes implementation source, agent chat
#               history, and Git history.
#
# Constraints:
#   - Findings must be traceable to specific requirement, contract, or design
#     locators and to specific observed evidence.
#   - The skill never reads implementation source directly.
#   - The skill never reads agent chat history or Git history.
#   - The skill refuses when intent artifacts are stale or inconsistent.
#   - Findings must distinguish between missing evidence and contradictory
#     evidence.
#   - The skill does not certify compliance; it only reports findings.
#
# Failure paths:
#   - Refusal: intent artifacts are stale, inconsistent, or incomplete.
#   - Ambiguity: observed evidence is insufficient to evaluate a requirement.
#   - Schema error: the findings record fails validation.
#
# Version: 1
"#;

/// Compliance-review skill version.
pub const SKILL_VERSION: u32 = 1;

/// The compliance-review skill schema.
pub const SCHEMA: &'static str = r#"
{
  "skill_name": "compliance-review",
  "role": "reviewer",
  "version": 1,
  "inputs": {
    "requirements": "string",
    "contract": "string",
    "design": "string",
    "impl_documentation": "string",
    "test_evidence": "string",
    "retain_discrepancies": "boolean"
  },
  "outputs": {
    "findings": "string",
    "discrepancy": "string (optional)",
    "rejection_reason": "string"
  },
  "approval_gate": "human",
  "independence_boundaries": {
    "allowed": ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md", "IMPL.md", "test evidence"],
    "excluded": ["implementation source", "agent chat history", "Git history", "prior IMPL.md", "TODOS.yaml", "VISION.md", "ARCHITECTURE.md", "ROOT_CONTRACT.md"]
  },
  "constraints": {
    "source_blind": "never reads implementation source directly",
    "traceability": "findings must be traceable to intent locators and observed evidence",
    "no_certification": "reports findings only; does not certify compliance",
    "evidence_distinction": "distinguishes missing evidence from contradictory evidence"
  },
  "failure_paths": [
    "refusal: intent artifacts are stale, inconsistent, or incomplete",
    "ambiguity: observed evidence is insufficient to evaluate a requirement",
    "schema_error: findings record fails validation"
  ]
}
"#;
