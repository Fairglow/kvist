# Phase 1 Compliance and Security Review

**Scope:** Core CLI Engine (`init`, `doctor`, `tree`, `spec new`, and `spec validate`).

## Independent review process

1. A clean-slate documenter inspected only Rust source and tests, then produced
   [`DOCS.md`](DOCS.md) without access to the architecture specification.
2. A separate compliance reviewer compared only
   [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
   and `DOCS.md`, without access to source code.

**Result:** The Phase 1 roadmap requirements are compliant: project
bootstrapping, deterministic component discovery/tree rendering, and layered
specification generation and validation are all present. Task execution, LLM
invocation, triple-blind pipelines, arbitration UI, web UI, and serving remain
explicitly deferred to Phases 2 and 3.

The architecture specification does not prescribe the exact CLI grammar, tree
status labels, overwrite policy, or detailed `SPEC.md` grammar. These
implemented contracts are documented in [`README.md`](README.md) and enforced
by tests.

## Security review

- No unsafe Rust, shared mutable state, network access, or background processes
  are used by Phase 1 commands.
- Generated files use same-directory temporary files, file synchronization, and
  no-clobber persistence. `init` writes only uninitialized projects; `doctor`
  classifies current, partial, invalid, and unsupported-version projects
  without modifying them.
- Project, configuration, component, and specification symlinks are rejected
  at direct checked paths; discovery does not traverse symlink entries.
- Configuration parsing is bounded to 64 KiB; specification and root
  contract/TODO/documentation inspection are each bounded to 1 MiB. Parsed
  component roots must be non-empty relative paths with normal path segments
  only.
- Filesystem, parsing, and validation failures are surfaced with contextual
  errors. CLI output is deterministic plain ASCII and does not emit secrets.

## Known operational limitation

Initialization writes each artifact atomically but is not a multi-file
transaction. If an I/O failure occurs after an earlier artifact is persisted,
the resulting partial Kvist artifact set is intentionally preserved and a
subsequent `init` refuses to merge or overwrite it. `doctor` provides
read-only diagnostics; Phase 1 has no automatic repair or migration, so
explicit user intervention is required.

## Root component dogfooding review

**Scope:** `src/SPEC.md`, `src/DOCS.md`, and `ROOT_CONTRACT.md`.

The source-blind review was conducted under
[`REVIEW_RUNBOOK.md`](REVIEW_RUNBOOK.md). The reviewer had access only to the
three scope documents; implementation verification remains the separately
recorded responsibility of the clean-slate documentation pass.

| Classification | Finding | Arbitration |
| --- | --- | --- |
| Compliant | The observed documentation describes bounded I/O, link rejection, deterministic discovery, atomic per-file writes, and root-state classification required by the root specification. | None. |
| Mismatch resolved | `SPEC.md` previously implied that `init` refused a current project, while `DOCS.md` documented its idempotent no-op behavior. | The contract now explicitly permits a no-op only for `current`; all other existing states remain refused. |
| Mismatch resolved | `ROOT_CONTRACT.md` requires task ordering, while the observed YAML validator intentionally permits any non-empty task field values and order. | Ordering is an authoring and future execution rule, not a Phase 1 parser constraint. `SPEC.md` records that boundary; P2-01/P2-03 own schema and lifecycle enforcement. |
| Underspecified | The documentation does not independently establish all line-aware diagnostic or future task-execution trust-boundary details stated in the specification. | Retain the requirement in `SPEC.md`; the Phase 2 trusted-workspace policy and its implementation review remain deferred. |

## Root component VCS tracking review

**Scope:** Git/jj durable-artifact tracking diagnostics in `doctor`.

The clean-slate documenter reviewed source and tests without access to the
contract. The source-blind reviewer then compared only `src/SPEC.md`,
`src/DOCS.md`, and `ROOT_CONTRACT.md`.

| Classification | Finding | Arbitration |
| --- | --- | --- |
| Compliant | The observed command set, root-state handling, deterministic discovery, bounded parsing, link rejection, and no-clobber writes match the root contract. | None. |
| Compliant | VCS inspection is read-only; Git uses index and native ignore semantics, while jj uses an explicit selected saved snapshot. | None. |
| Compliant | `DOCS.md` records observed behavior rather than reproducing the specification. | None. |
| Deferred | This comparison establishes documentation consistency, not independent proof of source behavior. Task execution must enforce complete tracking before Phase 2 runs user tasks. | P2 execution and its trusted-workspace policy own that enforcement. |

## Final root component VCS review

**Scope:** Final root `SPEC.md`, source-derived `DOCS.md`, and
`ROOT_CONTRACT.md`.

| Classification | Finding | Arbitration |
| --- | --- | --- |
| Compliant | The command/state contract, initialization gating, read-only diagnostics, discovery, specification validation, link rejection, no-clobber writes, and VCS inspection agree across the scope documents. | None. |
| Compliant | The artifact layout, versioned three-layer specification, durable project-file state, and discrepancy process agree with the root contract. | None. |
| Deferred | The source-blind comparison cannot prove prior test/spec sequencing, independent review execution, or TODO ordering. | The retained runbook, review records, and P2 execution lifecycle own those process checks. |
| Deferred | The source-derived record does not establish the specification's future trusted-workspace prerequisite because task execution is not implemented. | P2 trusted-workspace and execution policy own this requirement. |
