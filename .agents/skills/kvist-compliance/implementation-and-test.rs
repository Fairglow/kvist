//! Skill contract: implementation-and-test.
//!
//! This skill writes tests before implementation from accepted component
//! requirements, contract, and design while preserving strict context
//! boundaries.

/// Inputs for the implementation-and-test skill.
pub struct ImplementationAndTestInputs {
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
    /// The scope of this implementation attempt.
    pub scope: String,
    /// Whether failing tests have already been written for this task chain.
    pub tests_written: bool,
    /// Whether to generate native documentation.
    pub generate_native_documentation: bool,
    /// The current implementation, if any (for incremental development).
    pub current_implementation: Option<String>,
}

/// Outputs of the implementation-and-test skill.
pub struct ImplementationAndTestOutputs {
    /// The generated or revised implementation.
    pub implementation: String,
    /// Generated test code, if requested.
    pub tests: Option<String>,
    /// Generated or revised native documentation.
    pub native_documentation: Option<String>,
    /// Whether the output was rejected (empty string means accepted).
    pub rejection_reason: String,
    /// Traceability report linking tests and implementation to intent locators.
    pub traceability: String,
}

/// The skill contract for implementation and test generation.
pub const CONTRACT: &'static str = r#"
# Skill: implementation-and-test
# Role: developer
#
# Contract:
#   Inputs: accepted REQUIREMENTS.md, CONTRACT.md, DESIGN.md, immediate parent
#           CONTRACT.md (optional), root contract, implementation scope,
#           tests-written flag, native documentation flag, and current
#           implementation (optional).
#   Output: failing tests first, then implementation code, native documentation
#           if requested, and a traceability report.
#   Approval: the human must review the traceability report before accepting.
#   Independence: immediate parent CONTRACT.md is the only implicit propagated
#               component context. The skill excludes parent
#               requirements/design, peer artifacts, prior IMPL.md, and chat.
#   Runtime context: sandboxed work uses writable /workspace/component,
#               read-only /workspace/context/ROOT_CONTRACT.md, and for a child
#               read-only /workspace/context/PARENT_CONTRACT.md. General
#               provider-contract materialization remains deferred.
#
# Constraints:
#   - Failing tests derived from approved intent must exist before production
#     implementation is generated.
#   - Tests cover outcomes, consumer behavior, boundaries, malformed input,
#     acceptance criteria, and failure paths.
#   - Implementation must satisfy requirements, consumer contract, and design.
#   - The skill refuses to generate code that violates the component contract.
#   - The skill does not read peer implementation details.
#   - The skill refuses to overwrite a committed implementation without a
#     revalidation record.
#
# Failure paths:
#   - Refusal: intent or parent contract is stale, tests are absent, or the
#     component is locked.
#   - Ambiguity: component intent is underspecified for the requested scope.
#   - Schema error: the generated code fails to compile or tests fail.
#
# Version: 1
"#;

/// Implementation-and-test skill version.
pub const SKILL_VERSION: u32 = 1;

/// The implementation-and-test skill schema.
pub const SCHEMA: &'static str = r#"
{
  "skill_name": "implementation-and-test",
  "role": "developer",
  "version": 1,
  "inputs": {
    "requirements": "string",
    "contract": "string",
    "design": "string",
    "parent_contract": "string (optional)",
    "root_contract": "string",
    "scope": "string",
    "tests_written": "boolean",
    "generate_native_documentation": "boolean",
    "current_implementation": "string (optional)"
  },
  "outputs": {
    "implementation": "string",
    "tests": "string (optional)",
    "native_documentation": "string (optional)",
    "rejection_reason": "string",
    "traceability": "string"
  },
  "approval_gate": "human",
  "independence_boundaries": {
    "implicit_parent_context": "nearest ancestor component CONTRACT.md across transparent namespace directories",
    "sandbox_mounts": ["writable /workspace/component", "read-only /workspace/context/ROOT_CONTRACT.md", "read-only /workspace/context/PARENT_CONTRACT.md for children"],
    "deferred": ["general explicitly declared provider-contract materialization"],
    "excluded": ["parent REQUIREMENTS.md", "parent DESIGN.md", "peer artifacts", "agent chat history", "prior IMPL.md"]
  },
  "constraints": {
    "test_first": "failing tests precede production implementation",
    "intent_compliance": "must satisfy requirements, contract, and design",
    "test_coverage": "outcomes, public behavior, boundaries, malformed input, acceptance, failure paths",
    "no_peer_reading": "does not read peer implementation details",
    "no_overwrite": "refuses to overwrite a committed implementation without revalidation"
  },
  "failure_paths": [
    "refusal: local intent or immediate parent contract is stale",
    "refusal: failing tests do not exist",
    "refusal: component is locked",
    "refusal: component intent is ambiguous for the requested scope",
    "schema_error: generated code fails to compile or tests fail"
  ]
}
"#;
