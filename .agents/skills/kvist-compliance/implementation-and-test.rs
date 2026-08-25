//! Skill contract: implementation-and-test.
//!
//! This skill generates test cases and implementation code from a component
//! specification, respecting the component contract and excluding peer
//! implementation details or chat state.

/// Inputs for the implementation-and-test skill.
pub struct ImplementationAndTestInputs {
    /// The accepted component specification.
    pub specification: String,
    /// The immediate parent's specification revision, if any.
    pub parent_specification: Option<String>,
    /// The root contract.
    pub root_contract: String,
    /// The scope of this implementation attempt.
    pub scope: String,
    /// Whether to generate tests in addition to implementation.
    pub generate_tests: bool,
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
    /// Traceability report linking implementation to specification requirements.
    pub traceability: String,
}

/// The skill contract for implementation and test generation.
pub const CONTRACT: &'static str = r#"
# Skill: implementation-and-test
# Role: developer
#
# Contract:
#   Inputs: accepted component specification, parent specification (optional),
#           root contract, implementation scope, test generation flag,
#           native documentation flag, current implementation (optional).
#   Output: implementation code, test code (if requested), native documentation
#           (if requested), and a traceability report.
#   Approval: the human must review the traceability report before accepting.
#   Independence: the skill does not read peer implementation files or chat
#               history; it only reads the component specification and the
#               root contract.
#
# Constraints:
#   - The implementation must match the specified interfaces and outcomes.
#   - Generated tests must cover public behavior, boundaries, malformed input,
#     and failure paths.
#   - The skill refuses to generate code that violates the component contract.
#   - The skill does not read peer implementation details; it generates code
#     from the specification alone.
#   - The skill refuses to overwrite a committed implementation without a
#     revalidation record.
#
# Failure paths:
#   - Refusal: the specification is stale or the component is locked.
#   - Ambiguity: the specification is underspecified for the requested scope.
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
    "specification": "string",
    "parent_specification": "string (optional)",
    "root_contract": "string",
    "scope": "string",
    "generate_tests": "boolean",
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
    "excluded": ["peer implementation files", "agent chat history", "prior IMPL.md"]
  },
  "constraints": {
    "contract_compliance": "must match specified interfaces and outcomes",
    "test_coverage": "public behavior, boundaries, malformed input, failure paths",
    "no_peer_reading": "does not read peer implementation details",
    "no_overwrite": "refuses to overwrite a committed implementation without revalidation"
  },
  "failure_paths": [
    "refusal: specification is stale",
    "refusal: component is locked",
    "refusal: specification is ambiguous for the requested scope",
    "schema_error: generated code fails to compile or tests fail"
  ]
}
"#;
