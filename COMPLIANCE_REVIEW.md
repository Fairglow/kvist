# Phase 1 Compliance and Security Review

**Scope:** Core CLI Engine (`init`, `doctor`, `tree`, `spec new`, and `spec validate`).

## Independent review process

1. A clean-slate documenter inspected only Rust source and tests, then produced
   [`IMPL.md`](IMPL.md) without access to the architecture specification.
2. A separate compliance reviewer compared only
   [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
   and `IMPL.md`, without access to source code.

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

**Scope:** `src/SPEC.md`, `src/IMPL.md`, and `ROOT_CONTRACT.md`.

The source-blind review was conducted under
[`REVIEW_RUNBOOK.md`](REVIEW_RUNBOOK.md). The reviewer had access only to the
three scope documents; implementation verification remains the separately
recorded responsibility of the clean-slate documentation pass.

| Classification | Finding | Arbitration |
| --- | --- | --- |
| Compliant | The observed documentation describes bounded I/O, link rejection, deterministic discovery, atomic per-file writes, and root-state classification required by the root specification. | None. |
| Mismatch resolved | `SPEC.md` previously implied that `init` refused a current project, while `IMPL.md` documented its idempotent no-op behavior. | The contract now explicitly permits a no-op only for `current`; all other existing states remain refused. |
| Mismatch resolved | `ROOT_CONTRACT.md` requires task ordering, while the observed YAML validator intentionally permits any non-empty task field values and order. | Ordering is an authoring and future execution rule, not a Phase 1 parser constraint. `SPEC.md` records that boundary; P2-01/P2-03 own schema and lifecycle enforcement. |
| Underspecified | The documentation does not independently establish all line-aware diagnostic or future task-execution trust-boundary details stated in the specification. | Retain the requirement in `SPEC.md`; the Phase 2 trusted-workspace policy and its implementation review remain deferred. |

## Root component VCS tracking review

**Scope:** Git/jj durable-artifact tracking diagnostics in `doctor`.

The clean-slate documenter reviewed source and tests without access to the
contract. The source-blind reviewer then compared only `src/SPEC.md`,
`src/IMPL.md`, and `ROOT_CONTRACT.md`.

| Classification | Finding | Arbitration |
| --- | --- | --- |
| Compliant | The observed command set, root-state handling, deterministic discovery, bounded parsing, link rejection, and no-clobber writes match the root contract. | None. |
| Compliant | VCS inspection is read-only; Git uses index and native ignore semantics, while jj uses an explicit selected saved snapshot. | None. |
| Compliant | `IMPL.md` records observed behavior rather than reproducing the specification. | None. |
| Deferred | This comparison establishes documentation consistency, not independent proof of source behavior. Task execution must enforce complete tracking before Phase 2 runs user tasks. | P2 execution and its trusted-workspace policy own that enforcement. |

## Final root component VCS review

**Scope:** Final root `SPEC.md`, source-derived `IMPL.md`, and
`ROOT_CONTRACT.md`.

| Classification | Finding | Arbitration |
| --- | --- | --- |
| Compliant | The command/state contract, initialization gating, read-only diagnostics, discovery, specification validation, link rejection, no-clobber writes, and VCS inspection agree across the scope documents. | None. |
| Compliant | The artifact layout, versioned three-layer specification, durable project-file state, and discrepancy process agree with the root contract. | None. |
| Deferred | The source-blind comparison cannot prove prior test/spec sequencing, independent review execution, or TODO ordering. | The retained runbook, review records, and P2 execution lifecycle own those process checks. |
| Deferred | The source-derived record does not establish the specification's future trusted-workspace prerequisite because task execution is not implemented. | P2 trusted-workspace and execution policy own this requirement. |

## Phase 2 P2-01 TODO queue contract review

**Scope:** Version-1 `TODOS.yaml` parsing, semantic validation, canonical
serialization, root-artifact inspection integration, and their public contract
documents.

**Independent roles and permitted inputs:**

| Role | Permitted inputs | Result |
| --- | --- | --- |
| Security reviewer | P2-01 implementation, direct integration, dependency manifest, and focused tests | No security vulnerabilities found in the reviewed boundary. |
| Clean-slate documenter | Rust source, tests, and manifests only | Rewrote `src/IMPL.md` from observed behavior without reading the queue specification, README, queue, root contract, or prior reviews. |
| Source-blind reviewer | `src/SPEC.md`, `src/IMPL.md`, and `ROOT_CONTRACT.md` only | Identified implementation-record contract ambiguities below; it did not inspect source or tests. |

### Security result

The security review found no exploitable vulnerability in the typed YAML
boundary. Its review covered unknown-field rejection, dependency traversal,
cycle detection, timestamp/revision validation, deterministic YAML escaping,
state metadata, and error handling. This is not a claim that future queue
loading, hashing, locking, subprocess execution, or persistence is safe:
those surfaces do not exist yet and remain separate Phase 2 trust boundaries.

### Source-blind findings and explicit arbitration

The source-blind comparison found that the observed behavior was more precise
than several parts of the intended contract. The project architect's
documentation arbitration is recorded here rather than silently changing the
meaning of either artifact.

| Classification | Finding | Arbitration decision |
| --- | --- | --- |
| Deferred, clarified | The observed queue parser accepts an in-memory string without a byte limit, while root artifact inspection bounds a filesystem queue to 1 MiB. | `src/SPEC.md` now assigns the 1 MiB bound to every filesystem loader before parsing and explicitly preserves the in-memory API boundary. Future loaders must apply that bound. |
| Deferred, clarified | The observed implementation validates recorded SHA-256-shaped revisions but does not hash `SPEC.md` files or derive stale causes. | `src/SPEC.md` now assigns comparison and stale derivation explicitly to P2-02. Version-1 parsing remains responsible only for representing and validating stale evidence. |
| Clarified | The observed parent path is exactly `../SPEC.md`; the prior contract described it only generally as a relative parent path. | The specification now makes the literal path explicit and ties it to the existing component-hierarchy rule. |
| Clarified | The observed requirement locator is `SOURCE#LOCATOR`, and stale evidence requires ordered timestamps plus distinct revisions; these constraints were not fully stated in the specification. | The specification now defines each syntax and invariant so consumers need not infer it from source or observed documentation. |
| Clarified | The observed lifecycle check is kind-based over a task's explicit transitive dependency chain. The first queue schema has no separate deliverable-group field. | The specification defines a deliverable as that explicit chain. Requirement references and dependency edges are the mandatory traceability proof of scope. Introducing a stronger grouping key would require a future versioned schema change and migration, not an undocumented extension. |
| Clarified | Human readers have no queue-content CLI yet. | The specification and observed documentation now state that durable YAML is the current human content surface and `doctor` reports root-artifact validity only. |
| Clarified | Discovery reports missing and filesystem-malformed adjacent artifacts, not their content validity. | The specification now reserves non-root content validation for a future component-inspection surface. |

### Final source-blind compliance result

After the documented arbitrations, a source-blind reviewer compared only
`ROOT_CONTRACT.md`, `src/SPEC.md`, `src/IMPL.md`, `TODO.md`, and this review
record. It did not inspect Rust source, tests, manifests, diffs, or the queue
artifact. The reviewer found the documentation contract **compliant**:

| Classification | Finding | Arbitration |
| --- | --- | --- |
| Compliant | The specification and source-derived implementation record agree on the version-1 structure, validation, lifecycle/dependency rules, timestamps, state metadata, deterministic serialization, and root inspection's sole 1 MiB pre-parse queue bound. | None. |
| Compliant | The documented absence of unique/canonical revalidation causes and cause-kind/path correspondence checks does not conflict with a stated version-1 requirement. | None. |
| Deferred | General filesystem queue loaders, revision hashing and stale-cause derivation, task selection/transitions/persistence, queue CLI commands, and migration remain unimplemented future work. | P2-02 and P2-03 own their implementation and separate review. |

The completed test suite and `kvist doctor .` independently verified that the
current root `src/TODOS.yaml` is a valid version-1 queue. No implementation
requirement was removed, no queue state was silently rewritten, and no future
execution behavior is claimed as implemented.

## Phase 2 P2-02 project status review

**Scope:** Read-only project/component inspection, SHA-256 revalidation
comparison, and version-1 `kvist status` text and JSON reports.

| Role | Permitted inputs | Result |
| --- | --- | --- |
| Security reviewer | P2-02 source, tests, manifest, root contract, and specification | Found and remediated text control-character injection; no blocking finding remained. |
| Clean-slate documenter | Source, tests, manifest, and generated configuration only | Derived the status command, component-state precedence, validation, hashing, escaping, and point-in-time limitations in `src/IMPL.md`. |
| Source-blind reviewer | `ROOT_CONTRACT.md`, `src/SPEC.md`, `src/IMPL.md`, `README.md`, roadmap, and runbook only | Found the final implementation-record status contract compliant; it correctly deferred execution integration to P2-03. |

### Security result and arbitration

The audit found that unescaped control characters in filesystem paths or stored
stale-cause paths could forge lines in the default text report. A regression
fixture now verifies deterministic escaping of such a component path; text
rendering escapes backslashes and ASCII control characters, while JSON uses
its own string escaping.

The audit also identified the existing path-based metadata/read TOCTOU
limitation. This is not a newly claimed security guarantee: the documented
static-workspace threat model already states that Kvist does not pin directory
or file descriptors and cannot prevent concurrent filesystem replacement.
P2-02 retains that explicit limitation; P2 execution must define and enforce
its separate trusted-workspace boundary before running user-controlled work.

### Final compliance result

| Classification | Finding | Arbitration |
| --- | --- | --- |
| Compliant | `status` reports root and lexical component state without writing durable workflow data; version-1 text and JSON report shapes, ordering, output escaping, and exit behavior are documented. | None. |
| Compliant | Valid component queues are compared only with their own and immediate parent's valid specification bytes. Missing, invalid, unsupported-version, stale, blocked, and current precedence is explicit. | None. |
| Compliant | The initial text/JSON report-parity gap was resolved by adding `component_root` to the version-1 JSON object and its fixture. | None. |
| Deferred at the review boundary | The source-blind pass could not establish test fixture execution, and task execution/persistence had not been implemented. | Retain test-gate evidence separately; P2-03 owned executor integration. |

## Phase 2 P2-03 task workflow review

The clean-slate documenter derived task selection, transition, locking, audit,
and recovery behavior from source and tests only. The source-blind reviewer
then compared `ROOT_CONTRACT.md`, `src/SPEC.md`, `src/IMPL.md`, README, roadmap,
and runbook only.

| Classification | Finding | Arbitration |
| --- | --- | --- |
| Compliant | Ready-task selection, legal transitions, VCS/component gates, locking, prepared/committed records, and atomic replacement are consistently documented. | None. |
| Compliant | The audit remediated text injection, attempt-directory durability on supported platforms, and recovery fencing for trailing prepared records. | None. |
| Deferred at the review boundary | Provider/test execution and explicit prepared-record recovery remained later Phase 2 work. | P2-04 through P2-06 owned those boundaries. |

## Phase 2 execution-policy discrepancy

**Scope:** Documentation reconciliation after `task run`, agent configuration,
test-policy approval, task logging, and `spec accept` were added.

This is an explicit discrepancy record, not a replacement for the required
clean-slate documentation pass and source-blind compliance comparison. The
root specification's execution-policy decision gates require an approved
trusted-workspace, subprocess, environment, credential, timeout, cancellation,
output-limit, and durable-result policy before provider or repository-defined
test execution. The current implementation launches configured agents and
approved test commands directly on the host before those requirements have
been satisfied.

| Classification | Finding | Arbitration required |
| --- | --- | --- |
| Mismatch | `task run` executes host agent programs and implementation test commands. Agent execution has no sandbox, timeout, output cap, or effective-agent-configuration approval. | Preserve the specification's gate. P2-05b through P2-05d must either enforce the gate before execution or the architect must explicitly revise the product policy. |
| Mismatch | The former README, GUIDE, and observed root implementation record described deferred or automated behavior inconsistently with the implementation. | The public documents now distinguish observed commands from planned automation and state the host-execution limitation. A clean-slate documenter must independently replace or confirm the observed `src/IMPL.md` account. |
| Deferred | The intended clean-slate documenter, source-blind reviewer, arbitration integration, and automated task-generation/interview flows are absent. | Phase 3 owns automation. Until then, follow `REVIEW_RUNBOOK.md` manually and retain review evidence. |
| Deferred | The Phase 2 end-to-end security and compliance review has not yet covered the full agent/test execution surface. | P2-07 is the release gate for this surface; do not represent task execution as production-safe beforehand. |
