# Kvist Copilot Instructions

## Project intent

Kvist is a production-quality, headless Rust CLI that enforces a recursive,
spec-driven architecture workflow for human-directed AI development. The
authoritative product vision is
[`KVIST_Architectural_Specification_Full.md`](../KVIST_Architectural_Specification_Full.md).
Read the relevant sections before proposing or implementing a feature. Preserve
these non-negotiable principles:

- **Structure before syntax:** define and validate a component's requirements,
  public contract, constraints, and test strategy before implementing it.
- **Filesystem-native, recursive components:** a component directory owns its
  specification, task queue, compliance documentation, and implementation.
- **Durable, inspectable state:** persist workflow state in version-controlled
  project files, never only in chat context or an opaque database.
- **Strict context boundaries:** work from the target component, its immediate
  parent contract, and global constraints; do not couple a component to peer
  implementations without an explicit interface requirement.
- **Independent compliance review:** an implementer must not certify its own
  work. Compliance documentation is derived from code without the specification,
  then compared against the specification by a separate review context.

## Change workflow

1. Inspect the applicable `ROOT_CONTRACT.md`, local `SPEC.md`, parent interface,
   and `TODOS.yaml` before changing an existing component. Treat them as
   contracts, not optional documentation.
2. If a component does not yet have these artifacts, create or update its
   specification and task breakdown before its implementation. Do not silently
   invent externally observable behavior: identify unresolved product decisions
   or use a clearly documented, conservative assumption.
3. Keep `SPEC.md` progressively disclosed: an executive summary and public
   contract first, guarantees and constraints next, then algorithms, state
   transitions, edge cases, and failure paths.
4. Make `TODOS.yaml` atomic, ordered, and traceable to requirements. Each
   component queue must include `write_tests`, `implement_code`,
   `security_audit`, and `compliance_review`, in that order.
5. Write failing tests from the specification before production code. Update
   tests, specs, and task state together when a deliberate contract change is
   approved.
6. After implementation, produce or update `DOCS.md` from observed code
   behavior, not by copying `SPEC.md`. Report any spec-to-implementation
   discrepancy for explicit arbitration; never conceal it by changing either
   artifact automatically.

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

- Preserve compatibility unless a specification explicitly authorizes a
  breaking change. Update all affected contracts when parent requirements make
  child components stale.
- Cover normal behavior, boundary cases, malformed input, error propagation,
  and platform-sensitive path behavior. Use unit tests for pure logic and
  integration tests for CLI and filesystem workflows.
- Run the smallest relevant existing formatter, lint, type-check, and test
  commands after a code change. Do not weaken tests, skip checks, or change
  production behavior merely to make validation pass.
- Make surgical changes. Do not reformat, rename, or alter unrelated files.
  Do not modify licensing terms without explicit authorization; Kvist uses BSL
  1.1 / dual licensing as defined by the project.
