# Kvist Implementation Tracker

**Authority:** [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
**Reviewed:** Phase 1 implementation audit

## Current state

Phase 1 has working, tested implementations of `init`, `tree`, `spec new`, and
`spec validate`. The command paths, static templates, configuration parsing,
component discovery, and specification validation are implemented.

Phase 1 is **feature complete but not foundation complete**. The remediation
items below must be resolved before Phase 2 executes user tasks or invokes an
LLM. In particular, Kvist still lacks enforced CI quality gates, lifecycle
dogfooding, and VCS tracking inspection.

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
| DONE | Project initialization | Deterministic templates, no-clobber writes, direct symlink checks, and validated five-state initialization semantics. |
| DONE | Discovery and tree | Read-only lexical traversal, component layout statuses, plain ASCII output. |
| DONE | Specification workflow | Version-1 layered Markdown template, line-aware validation, safe creation and validation commands. |
| DONE | Initial review | End-to-end test, clean-slate `DOCS.md`, and source-blind `COMPLIANCE_REVIEW.md`. |

## Phase 1 remediation gate

### DONE P1-R1 — Define validated project state, recovery, and migration semantics

**Completed decision:** `project_state::inspect` validates all five root
artifacts and classifies `uninitialized`, `current`, `partial`, `invalid`, and
`unsupported-version`. The independent configuration, root-contract,
specification, TODO-queue, and documentation version domains are all `1`.
`init` writes only uninitialized projects, returns already initialized only for
validated current projects, and refuses every other state without overwriting.
`kvist doctor [PROJECT_DIR]` is the read-only diagnostic and recovery guidance
surface. Phase 1 has no automatic repair or migration; a future explicit
repair/migration command must define and opt into every rewrite. An interrupted
safe-write sequence is therefore deterministically inspectable as `partial`.

**Verification:** unit and integration tests cover every state, all independent
unsupported-version domains, malformed content and artifact types, doctor
output and read-only behavior, refusal/preservation for partial/invalid/
unsupported projects, and independent version constants.

### DONE P1-R2 — Bound discovery and document the filesystem threat model

**Completed decision:** Kvist supports malformed or untrusted *static*
workspaces for bounded read-only Phase 1 inspection, but is not a sandbox,
does not establish canonical containment, and cannot prevent concurrent
filesystem changes/TOCTOU. Phase 2 execution requires a separately authorized
trusted-workspace policy. Unix symbolic links and Windows reparse points
(including junctions) are link-like and rejected whenever discovery directly
inspects a component root or non-artifact descendant; link-like required
artifacts are invalid. Component paths require a component at every
intermediate directory. The configurable `[discovery]` limits have defaults
of depth 64, directories/components/entries 10,000 each, and relative path
4,096 encoded bytes; hard maxima are 256, 100,000, 100,000, 100,000, and
32,768 respectively. Invalid limits make the root state invalid. `.gitignore`
semantics remain deliberately unimplemented until P1-R5 because Git ignore
rules depend on tracked state.

**Verification:** deterministic direct and `tree` limit tests cover depth,
directories, components, entries, and encoded path length; configuration
default/range tests cover `tree`, `doctor`, and `init` propagation; hierarchy
and Unix link tests cover supported behavior. Permission-error testing is
documented for unprivileged supported-platform manual/release testing rather
than flaky root-bypassing automated tests.

### DONE P1-R3 — Make quality gates reproducible and enforced in CI

**Completed decision:** MSRV is Rust 1.85 (the edition-2024 baseline).
`Cargo.lock` is committed and every CI build/test uses `--locked`. GitHub
Actions runs format, strict all-target Clippy, tests, and release builds on
stable Linux/macOS/Windows, plus check/test on Rust 1.85. The default `just`
recipes use Cargo/Rustup only; optional external cargo tools are not required.

**Verification:** a clean checkout runs the documented Cargo gate and
`just all` without undeclared tools. CI enforces the same stable-platform gate
and MSRV check/test.

### DONE P1-R4 — Dogfood the lifecycle and publish a review runbook

**Completed decision:** Kvist dogfoods its generated lifecycle layout. The
repository tracks the validated root artifact set (`kvist.toml`,
`ROOT_CONTRACT.md`, `src/SPEC.md`, `src/TODOS.yaml`, and `src/DOCS.md`).
`REVIEW_RUNBOOK.md` defines the clean-slate and source-blind roles, their
permitted inputs, evidence locations, arbitration owner, retention policy, and
clean-checkout commands. The root component's documentation and compliance
record are intentionally distinct from the historical repository-level
`DOCS.md` and Phase 1 review record.

**Acceptance criteria:**

- Decide whether Kvist's repository dogfoods its own generated project layout.
  If yes, add the complete root artifact set and keep it valid; if no, document
  the exception and rationale.
- Define a repeatable clean-slate documenter and source-blind compliance-review
  procedure, including permitted inputs, output paths, discrepancy records,
  and arbitration ownership.
- Ensure generated `DOCS.md` is never confused with user-authored design
  documentation.

**Verification:** the root artifact set is validated by `doctor`; `tree` and
root `SPEC.md` validation succeed; the runbook has been executed with its
source-blind review and arbitration record retained in
`COMPLIANCE_REVIEW.md`.

### DONE P1-R5 — Inspect VCS tracking before Phase 2 execution

**Completed decision:** `kvist doctor` now performs a read-only durable-artifact
tracking inspection once root artifacts are current and component discovery
succeeds. It checks the root artifact set and each discovered component's
three required paths. `[vcs].kind` is `auto`, `git`, or `jj`; `auto` requires
exactly one detected VCS, so a Git/jj-colocated checkout requires an explicit
owner choice. Git uses `git ls-files` and native `git check-ignore` semantics
to distinguish tracked, ignored, and untracked files. jj uses an explicit
path-only `file list` template with `--ignore-working-copy`, never triggering
a snapshot. A jj path absent from its saved snapshot is reported rather than
hidden because it may be ignored, excluded by snapshot rules, or newer than
the saved snapshot. Neither implementation stages, commits, nor otherwise
mutates VCS state.

**Acceptance criteria:**

- Before Phase 2 task execution, verify that `kvist.toml`, `ROOT_CONTRACT.md`,
  and every component's `SPEC.md`, `TODOS.yaml`, and `DOCS.md` are tracked in
  a supported VCS.
- Support Git and jj without treating Git as the only VCS. Report a required
  artifact ignored by the selected VCS rather than hiding it.
- Never auto-stage or commit. Keep transient logs, locks, raw provider data,
  and credentials untracked.
- Define deterministic diagnostics and behavior for no VCS, unsupported VCS,
  ignored required artifacts, and mixed working trees.

**Verification:** Git fixtures cover tracked, ignored, nested-component,
no-repository, malformed-repository, and no-mutation diagnostics. The jj
fixture, when jj is installed, covers explicit selection, non-default
file-list templates, dash-prefixed paths, and saved-snapshot tracking. The
independent documentation and source-blind review records are retained in
`src/DOCS.md` and `COMPLIANCE_REVIEW.md`. CI installs jj 0.44.0 in its
dedicated VCS test job. Unit coverage proves that VCS query batching stays
within the 8 KiB argument budget and isolates individually unqueryable paths.

## Phase 2 — Task execution and LLM runner

Phase 2 begins only after the Phase 1 remediation gate is resolved or each
explicit exception is approved and recorded above.

### COMPLETE P2-01 — Specify independent TODO queue and dependency graph schemas

**Depends on:** P1-R1
**Current evidence:** Version-2 parsing, semantic validation, deterministic
serialization, root-inspection integration, and contract tests are complete.
Independent security audit and clean-slate/source-blind compliance review are
complete. `COMPLIANCE_REVIEW.md` records the review evidence and explicit
Phase 2 deferrals.
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
| Toolchain and dependencies | MSRV Rust 1.85; `Cargo.lock` is committed; intentional dependency updates must pass MSRV and stable CI. |
| Initial CLI | `init`, `tree`, `spec new`, and `spec validate`; project paths default to the current directory where applicable. |
| Project location | One project-local `kvist.toml`; no global configuration or parent-directory discovery. |
| Root artifact paths | `kvist.toml`, `ROOT_CONTRACT.md`, and `src/{SPEC.md,TODOS.yaml,DOCS.md}`. |
| Initial configuration | Version `1`, `component_root = "src"`, `llm.provider = "none"`. |
| Specification format | Version-1 Markdown with exact three ordered `<details>` layers and required headings. |
| Root-state and recovery | Five-state read-only inspection. Phase 1 never repairs or migrates automatically; `doctor` guides explicit user recovery. |
| Artifact version domains | Configuration, root contract, specification, TODO queue, and documentation have independent version domains. |
| VCS policy | Before Phase 2, durable artifacts must be tracked in a supported VCS (Git or jj); required ignored artifacts are reported. Kvist never auto-stages or commits; logs, locks, raw provider data, and credentials remain untracked. |
| Filesystem safety | No-clobber atomic file persistence; direct Unix-link/Windows-reparse rejection; lexical, bounded discovery; component-only intermediate hierarchy; ignored VCS/build directories. Static-workspace inspection is not a sandbox or TOCTOU defense; Phase 2 requires a trusted-workspace policy. |
| Input limits | `kvist.toml` at most 64 KiB; specifications and root contract/TODO/documentation inspection at most 1 MiB each. |
| Dependencies | `clap`, `thiserror`, `toml`, `tempfile`, and `serde_yaml`; add dependencies only with a documented need. |
| Review model | Clean-slate source documentation followed by source-blind specification comparison. |

## Open questions requiring an explicit decision

1. **Task execution trust:** Who authorizes repository-defined test commands
   and LLM execution, and what isolation or consent is required?
3. **Provider interface:** Which provider CLIs are first-class, what exact
   protocol do they implement, and where may credentials live?
4. **Context contract:** How are parent interfaces represented, and what is the
   normative algorithm for selecting local files while excluding peers?
5. **Staleness semantics:** Which parent/spec/config changes invalidate which
   child artifacts, and is invalidation based on hashes, explicit edges, or
   both?
6. **Automation interface:** Is stable JSON output and a documented exit-code
   taxonomy required before the web view, watcher, and CI integrations?
7. **Distribution and license:** What exact BSL/double-license text, Cargo
    metadata, release channel, MSRV, and binary distribution policy apply?
