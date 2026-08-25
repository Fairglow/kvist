//! Skill contract: reviewed-queue-generation.
//!
//! This skill drafts a component-local queue from an accepted specification,
//! generating task ordering, dependency graphs, and requirement traceability
//! without replacing human-authored work.

/// Inputs for the reviewed-queue-generation skill.
pub struct ReviewedQueueGenerationInputs {
    /// The accepted component specification.
    pub specification: String,
    /// The immediate parent's specification revision, if any.
    pub parent_specification: Option<String>,
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
    /// Traceability report linking tasks to specification requirements.
    pub traceability: String,
}

/// The skill contract for reviewed queue generation.
pub const CONTRACT: &'static str = r#"
# Skill: reviewed-queue-generation
# Role: architect
#
# Contract:
#   Inputs: accepted component specification, parent specification (optional),
#           root contract, revision flag, current queue (optional).
#   Output: a version-1 `TODOS.yaml` that passes schema validation and
#           requirement traceability checks.
#   Approval: the human must review the traceability report before accepting.
#   Independence: the skill does not read `SPEC.md` or prior `IMPL.md`.
#
# Constraints:
#   - The generated queue must respect the component's dependency graph.
#   - Each task must include `requirements` that map to specification
#     requirement locators.
#   - The task order must match the dependency graph: test tasks must
#     precede implementation tasks, and security audit tasks must precede
#     compliance review tasks.
#   - The skill refuses to overwrite an existing non-empty queue.
#   - The skill refuses to generate a queue for a stale specification.
#
# Failure paths:
#   - Refusal: the specification is stale or the queue is already complete.
#   - Ambiguity: the specification is underspecified for the requested scope.
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
    "specification": "string",
    "parent_specification": "string (optional)",
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
    "excluded": ["SPEC.md", "IMPL.md", "agent chat history"]
  },
  "constraints": {
    "dependency_graph": "must respect component dependency graph",
    "task_ordering": "test -> implementation -> security-audit -> compliance-review",
    "traceability": "each task must map to a specification requirement",
    "non_overwrite": "refuses to replace a non-empty existing queue",
    "stale_refusal": "refuses when the specification is stale"
  },
  "failure_paths": [
    "refusal: specification is stale",
    "refusal: queue is already complete",
    "refusal: specification is ambiguous or underspecified",
    "schema_error: generated queue fails validation"
  ]
}
"#;
