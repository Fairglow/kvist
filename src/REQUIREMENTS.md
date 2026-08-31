<!-- kvist-requirements-version: 1 -->
# Kvist Engine Requirements

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Purpose and scope

The root component provides the local headless Kvist engine and CLI. It owns
project initialization, component-document validation, bounded discovery,
project status, durable task queues, task lifecycle, Kvist-specific role and
policy selection, sandbox integration, context selection, import/conversion,
and durable execution evidence.

The reusable provider runtime belongs to the child `agent-runtime` component.
Kvist consumes that component's public contract and MUST NOT make its provider
or implementation types part of Kvist's durable formats.

This document defines both implemented requirements and newly approved target
requirements. The current CLI contract remains authoritative for implemented
commands: in particular, `component accept` currently performs structural
validation and revision recording only. The review, intent-proposal, and
contract-verification workflows below are planned and MUST NOT be reported as
implemented until their queues, code, tests, and independent review are
complete.

## Stakeholders and concerns

- Human architects need explicit approval, attribution, and arbitration.
- Component designers need stable requirement and contract identifiers.
- Implementers need bounded local context and deterministic task selection.
- Consumers need contracts without provider implementation details.
- Security reviewers need explicit trust, process, filesystem, network,
  credential, and resource boundaries.
- Compliance reviewers need independently derived observed behavior.
- CLI users and automation need deterministic output and actionable failures.

## Functional requirements

The following stable requirement identifiers define the mandatory behavior of
the root component.

### REQ-ARTIFACT-MODEL

A new project MUST contain versioned `VISION.md`, `ARCHITECTURE.md`,
`ROOT_CONTRACT.md`, configuration, and a root component with
`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, and `IMPL.md`.
Every discovered child component MUST use the same adjacent five-artifact set.

### REQ-COMPONENT-DOCUMENTS

Kvist MUST create deterministic requirements, contract, and design templates
without overwriting an existing document. It MUST validate each document's
version marker, required section order, uniqueness, and nonempty content and
return line-aware diagnostics without rewriting user content.

### REQ-DISCOVERY-STATUS

Kvist MUST discover the configured recursive component hierarchy
deterministically within configured hard bounds. Status MUST validate all
adjacent artifacts and report missing, invalid, unsupported, stale, blocked, or
current state without modifying project files.

### REQ-REVISION-PROVENANCE

Each queue MUST record SHA-256 revisions for local requirements, contract, and
design and, for a non-root component, its immediate parent contract. Status
MUST attribute each mismatch separately. Parent requirements or design changes
MUST NOT propagate merely because they are internal to the parent.

### REQ-TASK-QUEUE

`TODOS.yaml` MUST be a strict, versioned, deterministically serialized local
DAG. Tasks MUST have stable IDs, bounded purpose and outcome fields, durable
requirement references, legal lifecycle states, timestamps, and dependencies.
Lifecycle chains MUST order test, implementation, security audit, then
compliance review.

### REQ-TASK-LIFECYCLE

Task selection and transition MUST require a current, completely VCS-tracked
component. Writes MUST use exclusive user-owned locking, legal transitions,
atomic queue replacement, and append-only prepared/committed attempt evidence.
Stale locks and ambiguous prepared attempts MUST require explicit recovery.

### REQ-EXECUTION-BOUNDARY

Effectful task execution MUST require a complete explicit sandbox policy and an
independently installed runner outside the selected worktree. Kvist MUST
validate the exact runner, policy, component mount, network denial,
environment, time, and output bounds before invocation and MUST NOT fall back
to unconstrained host execution.

### REQ-AGENT-INTEGRATION

Kvist MAY invoke configured external agents for explicit prompt or task
operations. It MUST resolve commands without a shell, treat outputs as
untrusted and bounded, retain Kvist role and policy authority, and distinguish
acknowledged host execution from sandboxed task execution.

### REQ-COMPLIANCE

Implementation work MUST NOT certify itself. A clean-slate documenter derives
`IMPL.md` without intended requirements, contract, or design. A separate
source-blind reviewer compares that record with intended artifacts and retains
every discrepancy for human arbitration.

### REQ-ADVISORY-DOCUMENT-REVIEW

For the target component-acceptance workflow, Kvist MUST authorize acceptance
through one of three explicit paths:

1. when project review is required, a current review receipt covering the exact
   digests of
   `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, and a canonical projection of
   task definitions from `TODOS.yaml`, plus explicit human acknowledgement; or
2. an explicit exception bound to the same exact digests; or
3. a visible, deliberate project `[review] required = false` opt-out.

Review MUST default to required when the `[review]` section or `required` field
is absent, and generated project templates MUST emit `required = true`
explicitly. The task projection MUST contain only `id`, `title`, `description`,
`context`, `purpose`, `expected_outcome`, `kind`, `depends_on`, and
`requirements`; it MUST exclude all `component` metadata plus task `status`,
`timestamps`, `blocked_reason`, and `recovery_state`. Generated intent drafts
MUST NOT be exempt. The review is advisory: no finding, score, or severity
threshold may require a change, block acceptance, or determine compliance. A
per-bundle exception MUST record actor, timestamp, reason, and exact digests. If
project review is required but no review agent is configured, Kvist MUST
require an explicit exception and MUST NOT infer a pass.

`component accept` MUST remain deterministic and local and MUST NOT launch an
agent or make a network call. A separate planned review operation MUST use the
existing shell-free, bounded, explicitly approved or acknowledged agent
execution path. Review context MUST be limited to the local intent set and
canonical task projection, `ROOT_CONTRACT.md`, and the immediate parent
`CONTRACT.md`; peer and parent internals MUST be excluded.

Kvist MUST mint receipts from bounded execution evidence rather than accepting
model-generated receipts. Versioned receipts and redacted reports MUST be
stored under component-local `.kvist/reviews/` and MUST bind target digests,
scope, reviewer and authoring context identity when known,
provider/profile/model/tool identity, Kvist version, timestamp, bounded report
digest and path, and acknowledgement or exception. Raw transcripts and secrets
MUST NOT be retained. These files MUST be VCS-trackable but MUST NOT join the
five-artifact set, trigger component candidacy or staleness, enter task
context, or count as compliance evidence.

Project-level review of `VISION.md`, `ARCHITECTURE.md`, `ROOT_CONTRACT.md`,
ADRs, and referenced native schemas MUST be addressed by a later project-level
acceptance surface rather than claimed as current component enforcement. The
gate MUST NOT be generalized to arbitrary Markdown files.

### REQ-OBSERVED-INTENT-PROPOSAL

Kvist MUST provide a planned `propose intent` or `derive draft` workflow that
reads an independently generated `IMPL.md` and writes no-clobber draft
`REQUIREMENTS.md` and `DESIGN.md` only. It MUST mark uncertainty, MUST NOT
claim to recover stakeholder intent, and MUST NOT generate a normative
`CONTRACT.md`.

Generated drafts MUST remain subject to human review, advisory AI review or an
explicit exception, acknowledgement, and normal acceptance. A separate
advisory comparison MAY report differences between `IMPL.md` and existing
intent without modifying either. That comparison MUST NOT use compliance
verdict vocabulary and MUST NOT count as compliance evidence.

The existing `reverse-discover` source-based onboarding pipeline remains
distinct. Any contract it generates is a non-normative draft.

### REQ-CONTRACT-VERIFICATION

Kvist MUST plan stable contract-clause locators, initially using existing
heading anchors. Explicit clause IDs MAY be introduced only through an
explicit format/version decision. Tests MUST be traceable to contract clauses,
and approved test execution evidence MUST feed a contract-clause traceability
report that identifies uncovered and failed clauses.

Tests are evidence, not proof. This report MUST NOT be described as code
coverage and MUST NOT replace compliance comparison of `CONTRACT.md`,
`IMPL.md`, and test evidence.

### REQ-CONVERSION-IMPORT

Conversion, reverse discovery, and repository import MUST preserve existing
implementation files, validate generated or imported artifacts before
activation, refuse ambiguous existing metadata, and make generated intent
explicitly draft rather than inferred truth.

## Quality requirements and constraints

- Rust stable edition 2024 is required; unsafe Rust is forbidden.
- Executable support is Linux-only until independently tested platform
  boundaries exist.
- Core inspection and lifecycle operations perform no hidden network or model
  invocation.
- Advisory document review, observed-intent proposal, and contract-clause
  traceability are target requirements, not claims about the current CLI.
- Configuration is limited to 64 KiB. Component Markdown and YAML artifacts
  read by the engine are limited to 1 MiB.
- Traversal depth, directory count, component count, entries per directory,
  path length, subprocess duration, and output are explicitly bounded.
- Writes use regular non-link paths and synchronized no-clobber or atomic
  replacement appropriate to the operation.
- Output, ordering, hashing, and canonical queue serialization are
  deterministic.
- Inputs and imported content are untrusted and invalid semantics fail
  explicitly rather than being ignored or repaired.
- The dependency graph remains small and justified for a local single-binary
  CLI.

## Acceptance and traceability

Each implemented `REQ-*` item requires unit or integration evidence appropriate
to its boundary. CLI and filesystem workflows require integration tests; pure
schema and transition behavior require unit tests; Linux sandbox and process
behavior require native boundary tests. Tests provide evidence rather than
proof. Security audit and independent compliance review are mandatory terminal
tasks for each deliverable chain.

The system decomposition and authority direction are defined in
[`../ARCHITECTURE.md`](../ARCHITECTURE.md). Consumer-visible behavior is
defined in [`CONTRACT.md`](CONTRACT.md), private realization in
[`DESIGN.md`](DESIGN.md), and standards alignment in
[`../docs/standards.md`](../docs/standards.md).
