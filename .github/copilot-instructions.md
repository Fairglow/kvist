# Kvist Copilot Instructions

## Project intent

Kvist is a production-quality, headless Rust CLI that enforces a recursive,
architecture-driven workflow for human-directed AI development. Product
direction is authoritative in [`VISION.md`](../VISION.md), system structure in
[`ARCHITECTURE.md`](../ARCHITECTURE.md), and the standards posture in
[`docs/standards.md`](../docs/standards.md). Read the relevant documents before
proposing or implementing a feature. Preserve these non-negotiable principles:

- **Structure before syntax:** define and validate a component's requirements,
  public contract, constraints, and test strategy before implementing it.
- **Filesystem-native, recursive components:** a component directory owns
  `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`,
  tests, and implementation.
- **Durable, inspectable state:** persist workflow state in version-controlled
  project files, never only in chat context or an opaque database.
- **Strict context boundaries:** the immediate parent `CONTRACT.md` is the only
  implicit propagated component context. Work from local artifacts, that
  parent contract, and global constraints; do not couple a component to peer
  implementations. General explicitly declared provider-contract
  materialization remains deferred.
- **Independent compliance review:** an implementer must not certify its own
  work. A clean-slate context derives `IMPL.md` without reading intent
  documents, and a separate source-blind context compares requirements,
  contract, and design with observed and test evidence.
- **Advisory intent review:** controlled intent should receive a
  bounded AI review opportunity before acceptance when reasonable, or a
  deliberate explicit exception. Findings are nonbinding, may be wrong or
  overly exacting, and never determine compliance.

## Change workflow

1. Inspect `VISION.md`, `ARCHITECTURE.md`, `ROOT_CONTRACT.md`, the local
   `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, immediate parent
   `CONTRACT.md` when present, and `TODOS.yaml` before changing an existing
   component. Treat each artifact's authority as binding.
2. If a component does not yet have these artifacts, define or update the
   requirements, consumer contract, design, and task breakdown before its
   implementation. Do not silently invent externally observable behavior:
   identify unresolved product decisions or use a clearly documented,
   conservative assumption.
3. Put outcomes, constraints, acceptance criteria, and verification obligations
   in `REQUIREMENTS.md`; consumer-visible interfaces and semantics in
   `CONTRACT.md`; and private structure, algorithms, state transitions, edge
   cases, and recovery in `DESIGN.md`. Reference optional native schemas from
   `CONTRACT.md` with exact paths and dialect/version.
4. Make `TODOS.yaml` atomic, ordered, and traceable to requirements. Each
   component queue must include `write_tests`, `implement_code`,
   `security_audit`, and `compliance_review`, in that order.
5. When the planned advisory-review mechanism exists, request and record review
   for the exact local `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, and
   canonical task-definition projection before acceptance, or record the
   explicit project opt-out or per-bundle exception. Acknowledge findings
   without treating them as binding. Do not include peer or parent internals,
   and do not treat review receipts or reports as compliance evidence.
   Generated intent drafts are included. When a later project-level acceptance
   surface exists, apply its review opportunity to controlled project intent
   such as vision, architecture, root contract, ADRs, and referenced native
   schemas.
6. Write failing tests from the approved intent before production code. Update
   tests, affected intent documents, and task state together only when a
   deliberate change is approved.
7. After implementation, derive `IMPL.md` from source and test evidence in a
   clean-slate context that excludes all intent documents, the queue, prior
   record, architecture/root intent, prior reviews, chat history, and Git
   history. A separate reviewer compares requirements, contract, and design
   with `IMPL.md` and test evidence. Report discrepancies for explicit human
   arbitration; never conceal them by automatically changing either side.

Current enforcement note: `component accept` only structurally validates and
records revisions. Advisory-review receipts, acknowledgement/exception
enforcement, project-level acceptance, IMPL-derived intent proposals, and
contract-clause traceability are not implemented. Do not fabricate receipts,
reports, command results, or current interfaces. The rubber-duck review in the
parent context for the documentation change that introduced this policy is
advisory input, not a Kvist receipt.

## Rust engineering standards

- Target stable Rust edition 2024. Prefer the standard library and a small,
  justified dependency graph; add dependencies only when their capability,
  maintenance, licensing, and security impact are appropriate for a local,
  single-binary CLI.
- Keep the engine headless and portable. Do not require cloud services,
  telemetry, credentials, or a runtime daemon for core commands. External LLM
  tools are optional subprocess integrations and must fail clearly when absent.
- Model invalid states out of existence with types. Use explicit domain errors
  (`Result` and meaningful error types); never use `unwrap`, `expect`, or
  panics for recoverable input, filesystem, parsing, subprocess, or network
  failures.
- Define clear ownership and concurrency boundaries. Do not introduce shared
  mutable state, blocking I/O in async paths, background processes, or unsafe
  code without a documented invariant and targeted tests.
- Treat filesystem data, YAML, Markdown, subprocess output, environment
  variables, and paths as untrusted input. Validate schemas and bounds, avoid
  shell interpolation, preserve atomic writes, and produce actionable,
  non-secret error messages.
- Expose small, documented module interfaces. Keep `main.rs` limited to CLI
  setup and command dispatch; place business logic in testable library modules.
  Use rustdoc for public APIs and non-obvious invariants.
- Prefer deterministic behavior: stable ordering, explicit configuration,
  reproducible output, and no hidden network or filesystem side effects.

## Quality gates

- The current artifact model is pre-release and intentionally has no backward
  compatibility or migration. Do not recognize retired component documents,
  command spellings, or queue fields. A local contract change affects declared
  consumers; only an immediate parent `CONTRACT.md` change propagates
  implicitly to a child.
- For sandboxed task execution, the component is writable at
  `/workspace/component`; `ROOT_CONTRACT.md` is read-only at
  `/workspace/context/ROOT_CONTRACT.md`; and a child's nearest ancestor
  component contract is read-only at
  `/workspace/context/PARENT_CONTRACT.md`. Transparent namespace directories
  may separate the child from that ancestor.
- Cover normal behavior, boundary cases, malformed input, error propagation,
  and platform-sensitive path behavior. Use unit tests for pure logic and
  integration tests for CLI and filesystem workflows.
- Run the smallest relevant existing formatter, lint, type-check, and test
  commands after a code change. Do not weaken tests, skip checks, or change
  production behavior merely to make validation pass.
- Make surgical changes. Do not reformat, rename, or alter unrelated files.
  Do not modify licensing terms without explicit authorization; Kvist uses BSL
  1.1 / dual licensing as defined by the project.
