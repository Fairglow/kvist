//! Skill contract: reviewed-queue-generation.
//!
//! This skill drafts a component-local queue from accepted requirements,
//! contract, and design documents without replacing human-authored work.

/// Inputs for the reviewed-queue-generation skill.
pub struct ReviewedQueueGenerationInputs {
    /// The accepted component requirements.
    pub requirements: String,
    /// The accepted consumer contract.
    pub contract: String,
    /// The accepted private design.
    pub design: String,
    /// The nearest ancestor component contract across transparent namespaces.
    pub parent_contract: Option<String>,
    /// The root contract.
    pub root_contract: String,
    /// Whether the user is requesting a queue revision.
    pub revision_requested: bool,
    /// The current queue contents, if any (for incremental planning).
    pub current_queue: Option<String>,
}

/// Outputs of the reviewed-queue-generation skill.
pub struct ReviewedQueueGenerationOutputs {
    /// The generated or revised queue.
    pub queue: String,
    /// Whether the queue was rejected (empty string means accepted).
    pub rejection_reason: String,
    /// Traceability report linking tasks to durable intent locators.
    pub traceability: String,
}

/// The skill contract for reviewed queue generation.
pub const CONTRACT: &'static str = r#"
# Skill: reviewed-queue-generation
# Role: architect
#
# Contract:
#   Inputs: accepted REQUIREMENTS.md, CONTRACT.md, DESIGN.md, immediate parent
#           CONTRACT.md (optional), root contract, revision flag, and current
#           queue (optional).
#   Output: a version-1 `TODOS.yaml` that passes schema validation and
#           requirement traceability checks.
#   Approval: the human must review the traceability report before accepting.
#   Independence: immediate parent CONTRACT.md is the only implicit propagated
#               component context. The skill excludes parent
#               requirements/design, peers, implementation, and prior IMPL.md.
#
# Constraints:
#   - The generated queue must respect the component's dependency graph.
#   - Queue provenance uses requirements_revision, contract_revision,
#     design_revision, and parent_contract.
#   - parent_contract.path uses one or more `..` segments followed by
#     CONTRACT.md to reach the actual nearest ancestor component across
#     transparent namespace directories.
#   - Each task maps to durable requirement or contract locators.
#   - The task order must match the dependency graph: test tasks must
#     precede implementation tasks, and security audit tasks must precede
#     compliance review tasks.
#   - The skill refuses to overwrite an existing non-empty queue.
#   - The skill refuses stale local intent or a stale parent contract.
#
# Failure paths:
#   - Refusal: intent or parent contract is stale, or the queue is complete.
#   - Ambiguity: component intent is underspecified for the requested scope.
#   - Schema error: the generated queue fails validation.
#
# Version: 1
"#;

/// Reviewed queue generation skill version.
pub const SKILL_VERSION: u32 = 1;

/// The reviewed queue generation skill schema.
pub const SCHEMA: &'static str = r#"
{
  "skill_name": "reviewed-queue-generation",
  "role": "architect",
  "version": 1,
  "inputs": {
    "requirements": "string",
    "contract": "string",
    "design": "string",
    "parent_contract": "string (optional)",
    "root_contract": "string",
    "revision_requested": "boolean",
    "current_queue": "string (optional)"
  },
  "outputs": {
    "queue": "string",
    "rejection_reason": "string",
    "traceability": "string"
  },
  "approval_gate": "human",
  "independence_boundaries": {
    "implicit_parent_context": "immediate parent CONTRACT.md only",
    "excluded": ["parent REQUIREMENTS.md", "parent DESIGN.md", "peer artifacts", "implementation files", "prior IMPL.md", "agent chat history"]
  },
  "constraints": {
    "dependency_graph": "must respect component dependency graph",
    "task_ordering": "test -> implementation -> security-audit -> compliance-review",
    "provenance_fields": "requirements_revision, contract_revision, design_revision, parent_contract",
    "parent_contract_path": "one or more .. segments followed by CONTRACT.md, computed to the nearest ancestor component across transparent namespaces",
    "traceability": "each task maps to durable requirement or contract locators",
    "non_overwrite": "refuses to replace a non-empty existing queue",
    "stale_refusal": "refuses when local intent or the immediate parent contract is stale"
  },
  "failure_paths": [
    "refusal: local intent or immediate parent contract is stale",
    "refusal: queue is already complete",
    "refusal: component intent is ambiguous or underspecified",
    "schema_error: generated queue fails validation"
  ]
}
"#;
