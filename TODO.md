# Kvist Implementation Tracker

**Authority:** [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
**Reviewed:** Phase 1 implementation audit

## Current state

Phase 1 has working, tested implementations of `init`, `tree`, `spec new`, and
`spec validate`. The command paths, static templates, configuration parsing,
component discovery, and specification validation are implemented.

Phase 1 is **feature complete but not foundation complete**. The remediation
items below must be resolved before Phase 2 executes user tasks or invokes an
LLM. In particular, Kvist has no validated project-state model, no recovery
path for a partial initialization, no bounded component-count traversal, and
CI does not enforce all local quality gates.

## Status conventions

- `TODO` — scoped and ready once its dependencies are done.
- `IN PROGRESS` — actively being implemented.
- `BLOCKED` — needs an explicit product or security decision.
- `DONE` — acceptance criteria and listed verification are complete.

Keep this file current with the implementation it tracks. Do not silently
resolve an open decision through code.

## Phase 1 completed baseline

| Status | Deliverable | Implemented behavior |
| --- | --- | --- |
| DONE | CLI foundation | Typed Rust CLI, contextual errors, process boundary in `main.rs`. |
| DONE | Project initialization | Deterministic templates, no-clobber writes, direct symlink checks, idempotent complete-set detection. |
| DONE | Discovery and tree | Read-only lexical traversal, component layout statuses, plain ASCII output. |
| DONE | Specification workflow | Version-1 layered Markdown template, line-aware validation, safe creation and validation commands. |
| DONE | Initial review | End-to-end test, clean-slate `DOCS.md`, and source-blind `COMPLIANCE_REVIEW.md`. |

## Phase 1 remediation gate

### TODO P1-R1 — Define validated project state, recovery, and migration semantics

**Why:** `init` treats any complete set of five regular files as initialized,
does not validate their contents or versions, and deliberately leaves a partial
set after a mid-operation failure with no repair path.

**Acceptance criteria:**

- Define the project states `uninitialized`, `current`, `partial`, `invalid`,
  and `unsupported-version`, including the exact artifact and content checks
  that distinguish them.
- Split independent version domains for configuration, root contract,
  specification, TODO queue, and compliance documentation; do not use one
  template version as a proxy for every schema.
- Define safe behavior for `init` against every state. Specify whether a
  read-only `kvist doctor`, an explicit `init --repair`, and/or a migration
  command is the recovery surface. Never overwrite user content implicitly.
- Make root initialization transactional where practical, or make partial
  recovery deterministic, inspectable, and testable.
- Add migration fixtures for current, partial, invalid, and unsupported-version
  projects.

**Verification:** integration tests for every project state, an interrupted
write simulation, migration/repair refusal paths, and preservation of
user-authored files.

### TODO P1-R2 — Bound discovery and document the filesystem threat model

**Why:** discovery has a depth limit but no maximum component count, directory
entry count, path length, or total metadata-read budget. Direct symlink checks
do not establish canonical containment or eliminate time-of-check/time-of-use
races.

**Acceptance criteria:**

- Define supported filesystem and attacker assumptions: trusted local checkout,
  hostile workspace, or both. Specify Windows junction/reparse-point behavior.
- Add configurable, documented limits for discovered directories/components,
  directory entries, and path bytes. Report the exact exceeded limit.
- Decide whether component descendants beneath a non-component directory are
  legal. Enforce the chosen hierarchy invariant instead of merely indenting
  their relative path in tree output.
- Add supported-platform permission-error tests and tests for each resource
  limit, symlink/junction behavior, and malformed intermediate path.

**Verification:** deterministic limit tests, Unix and Windows platform coverage
where available, and no uncontrolled traversal of ignored or linked paths.

### TODO P1-R3 — Make quality gates reproducible and enforced in CI

**Why:** local review used `cargo fmt --check`, strict all-target Clippy, tests,
and a release build, but GitHub Actions currently runs only build and tests.
The new `justfile` additionally assumes optional tools (`unbuffer`,
`cargo-nextest`, and `cargo-outdated`) without provisioning or fallback.

**Acceptance criteria:**

- Define the supported Rust toolchain/MSRV and dependency update policy.
- Make CI run formatting, `cargo clippy --all-targets -- -D warnings`, tests,
  and release build, with lockfile enforcement.
- Either provision optional recipe tools in a documented developer bootstrap or
  make the default recipes use only the supported Rust toolchain.
- Add platform CI coverage for Linux, macOS, and Windows or explicitly narrow
  the support statement in `README.md`.

**Verification:** clean checkout executes the documented default quality gate
and required CI jobs run it without undeclared tools.

### TODO P1-R4 — Dogfood the lifecycle and publish a review runbook

**Why:** the engine generates lifecycle artifacts for user projects, but Kvist's
own repository is not yet a validated Kvist project. The independent review
was performed manually and has no repeatable command/runbook.

**Acceptance criteria:**

- Decide whether Kvist's repository dogfoods its own generated project layout.
  If yes, add the complete root artifact set and keep it valid; if no, document
  the exception and rationale.
- Define a repeatable clean-slate documenter and source-blind compliance-review
  procedure, including permitted inputs, output paths, discrepancy records,
  and arbitration ownership.
- Ensure generated `DOCS.md` is never confused with user-authored design
  documentation.

**Verification:** execute the runbook from a clean checkout and retain only
the intended review artifacts.

## Phase 2 — Task execution and LLM runner

Phase 2 begins only after the Phase 1 remediation gate is resolved or each
explicit exception is approved and recorded above.

### TODO P2-01 — Specify independent TODO queue and dependency graph schemas

**Depends on:** P1-R1
**Acceptance criteria:**

- Define a versioned `TODOS.yaml` schema using `serde`/`serde_yaml`, with
  stable task IDs, titles, descriptions, status, dependencies, requirement
  references, timestamps, and stale-revalidation state.
- Define legal task states and transitions, duplicate-ID handling, dependency
  cycles, ordering, and deterministic serialization.
- Specify how a parent specification change identifies and marks affected
  children stale.
- Replace the current illustrative task list with a schema-valid template and
  migration strategy.

**Verification:** parser/serializer, malformed YAML, cycle, state-transition,
and deterministic-output tests.

### TODO P2-02 — Implement project inspection and machine-readable status

**Depends on:** P1-R1, P2-01
**Acceptance criteria:**

- Build one validated project/component state model shared by `init`, `tree`,
  TODO validation, and execution.
- Provide stable non-interactive output and documented exit codes for scripts
  and the future web/LSP layers; decide whether JSON is required.
- Surface missing, stale, invalid, unsupported, and blocked states without
  mutating the project.

**Verification:** golden tests for text and any machine-readable output, plus
state fixtures produced by P1-R1.

### TODO P2-03 — Implement safe task selection and execution state updates

**Depends on:** P2-01, P2-02
**Acceptance criteria:**

- Select only ready tasks whose dependencies are complete and whose component
  specification is current.
- Acquire a documented project lock; prevent concurrent state corruption.
- Persist task transitions atomically with an audit record, recovery behavior,
  and no false success after a crash or cancellation.
- Enforce the mandatory lifecycle ordering: tests, implementation, security
  audit, compliance review.

**Verification:** dependency, lock contention, crash-recovery, cancellation,
and atomic-state-update integration tests.

### TODO P2-04 — Define the external LLM provider contract and credentials policy

**Depends on:** P2-02
**Status:** BLOCKED — requires product decisions below.
**Acceptance criteria:**

- Define a provider-neutral request/response contract for external CLIs,
  including executable discovery, argument construction, working directory,
  context files, stdin/stdout protocol, timeouts, cancellation, output-size
  limits, and exit-code mapping.
- Define credential sourcing, redaction, environment inheritance, telemetry
  policy, and whether network access requires explicit per-run consent.
- Preserve strict context slicing: target component, parent interface, root
  contract, and no peer implementation unless explicitly required.
- Never invoke a shell to construct provider commands.

**Verification:** fake-provider integration tests for success, malformed
output, timeout, cancellation, missing executable, nonzero exit, secret
redaction, and output limits.

### TODO P2-05 — Implement test-command verification as an explicit trust boundary

**Depends on:** P2-01, P2-03
**Status:** BLOCKED — requires test-command policy.
**Acceptance criteria:**

- Define where test commands are configured and who may approve changes to
  them; do not execute arbitrary repository text implicitly.
- Execute tests with bounded output, timeout, cancellation, explicit working
  directory, and captured failure evidence.
- Record test results against the task attempt and require success before
  implementation tasks transition to complete.

**Verification:** command policy, timeout, cancellation, output limit,
nonzero exit, and result-persistence integration tests.

### TODO P2-06 — Implement the atomic task execution loop

**Depends on:** P2-03, P2-04, P2-05
**Acceptance criteria:**

- Drive one approved task at a time through context preparation, provider
  invocation, change inspection, test verification, and state transition.
- Preserve an inspectable attempt record without persisting secrets or opaque
  chat-only state as project truth.
- Stop safely on failure and report the task, stage, evidence path, and
  recovery action.

**Verification:** end-to-end fixtures using a fake provider and test command,
including a failed implementation and resumed execution.

### TODO P2-07 — Perform Phase 2 security and compliance review

**Depends on:** P2-06
**Acceptance criteria:**

- Perform a clean-slate documentation pass and source-blind compliance
  comparison for the Phase 2 contract.
- Review subprocess, environment, credentials, filesystem writes, locking,
  cancellation, and resource-boundary behavior.
- Record arbitration items explicitly; do not silently rewrite requirements or
  observed behavior.

**Verification:** independent review artifacts and an end-to-end Phase 2
fixture.

## Decisions already adopted

| Decision | Current contract |
| --- | --- |
| Implementation language | Stable Rust, edition 2024; headless local CLI. |
| Initial CLI | `init`, `tree`, `spec new`, and `spec validate`; project paths default to the current directory where applicable. |
| Project location | One project-local `kvist.toml`; no global configuration or parent-directory discovery. |
| Root artifact paths | `kvist.toml`, `ROOT_CONTRACT.md`, and `src/{SPEC.md,TODOS.yaml,DOCS.md}`. |
| Initial configuration | Version `1`, `component_root = "src"`, `llm.provider = "none"`. |
| Specification format | Version-1 Markdown with exact three ordered `<details>` layers and required headings. |
| Filesystem safety | No-clobber atomic file persistence, direct symlink rejection, lexical discovery, ignored VCS/build directories, 64-level depth limit. |
| Input limits | `kvist.toml` at most 64 KiB; `SPEC.md` at most 1 MiB. |
| Dependencies | `clap`, `thiserror`, `toml`, and `tempfile`; add dependencies only with a documented need. |
| Review model | Clean-slate source documentation followed by source-blind specification comparison. |

## Open questions requiring an explicit decision

1. **Project recovery:** Which explicit command repairs a partial or
   unsupported-version project, and what may it rewrite?
2. **Hierarchy invariant:** Must every intermediate directory in a component
   path be a component, or may a component sit below an ordinary source
   directory?
3. **Schema versioning:** Are configuration, contract, specification, TODO,
   and documentation versions independently evolved, and what compatibility
   window must Kvist support?
4. **Task execution trust:** Who authorizes repository-defined test commands
   and LLM execution, and what isolation or consent is required?
5. **Provider interface:** Which provider CLIs are first-class, what exact
   protocol do they implement, and where may credentials live?
6. **Context contract:** How are parent interfaces represented, and what is the
   normative algorithm for selecting local files while excluding peers?
7. **Staleness semantics:** Which parent/spec/config changes invalidate which
   child artifacts, and is invalidation based on hashes, explicit edges, or
   both?
8. **Automation interface:** Is stable JSON output and a documented exit-code
   taxonomy required before the web view, watcher, and CI integrations?
9. **Platform/security scope:** Is Kvist supported on Windows junctions and
   hostile workspaces, or only trusted local checkouts?
10. **Distribution and license:** What exact BSL/double-license text, Cargo
    metadata, release channel, MSRV, and binary distribution policy apply?
