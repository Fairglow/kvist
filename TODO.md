# Kvist Implementation Tracker

**Authority:** [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
**Reviewed:** Phase 1, Phase 2, Phase 3, and all UX hardening items
**Reviewed by:** Stefan Kvist | 2026-08-25
**Build:** `cargo build --release` passes with 0 warnings

## Status conventions

- `TODO` — scoped and ready once its dependencies are done.
- `IN PROGRESS` — actively being implemented.
- `BLOCKED` — needs an explicit product or security decision.
- `DONE` / `COMPLETE` — acceptance criteria and listed verification are complete.

---

# Completed Milestones

All 24 items below are accepted as complete. The codebase reflects their implementations.

## Phase 1 — Core Engine

| #       | Item            | Summary                                                                                                                                                            |
| ------- | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| P1-Core | Core CLI engine | `kvist init`, `kvist tree`, `kvist spec new`, `kvist spec validate`, bounded directory traversal, direct symlink safety checks, read-only VCS tracking diagnostics |

## Phase 2 — Execution Boundary

| #      | Item                                                 | Summary                                                                                                                               |
| ------ | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| P2-01  | Independent TODO queue and dependency graph schemas  | Versioned parsing, semantic validation, deterministic serialization, root-inspection integration, contract tests, compliance review   |
| P2-02  | Project inspection and machine-readable status       | `kvist status` renders deterministic text and JSON reports from shared root/component model                                           |
| P2-03  | Safe task selection and execution state updates      | Task selection, transition contract, lock and attempt-record recovery rules                                                           |
| P2-04  | User-provided agent invocation mechanics             | `src/config.rs` and `src/agent.rs` with precedence, shell-free spawning, log capture, token-record parsing                            |
| P2-04b | Basic CLI-wrapper templates                          | Two configured profiles support `{prompt}`, `{context_files}`, `{target_directory}` via whitespace-delimited arguments                |
| P2-05  | Test-command verification as explicit trust boundary | Configured test-policy verification with bounded execution and durable result persistence                                             |
| P2-05b | Sandbox all external execution                       | Component-only, deny-network external sandbox runner; fails before task mutation                                                      |
| P2-05c | Cryptographic approval binding                       | Authenticated user-state approval binds effective agents, sandbox runner, test policy, versions                                       |
| P2-05d | Bound agent subprocess resources                     | Per-profile timeouts, combined-output limits, cancellation, redacted bounded evidence                                                 |
| P2-06  | Atomic task execution loop                           | `kvist task run <COMPONENT_DIR> [TASK_ID]` driver, concurrent locks, atomic progress/blocked state transitions                        |
| P2-07  | Phase 2 security and compliance review               | Independent security, clean-slate documentation, and source-blind compliance passes                                                   |
| P2-08  | Execution-boundary compliance reconciliation         | Fresh clean-slate and source-blind reviews, explicit documentation arbitration, legal durable queue transitions                       |
| P2-09a | Model-resolution coverage                            | Named model per role, `default_model` fallback, first-entry aliases, unknown-name diagnostics, `none` bypass, system-prompt prefixing |
| P2-09b | Template-contract validation                         | Narrow shell-free argument template, placeholder resolution, Ollama `{model}` placeholder documented as unsupported                   |
| P2-09c | Execution portability decision                       | Platform-gated behavior defined; fail-closed on unsupported platforms with actionable diagnostics                                     |

## Phase 3 — Independent Compliance Automation

| #     | Item                                    | Summary                                                                                                                                                           |
| ----- | --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P3-01 | Component-design and feasibility skills | Versioned architect, specification-interview, feasibility-review contracts with required inputs, permitted context, outputs, human approval gates                 |
| P3-02 | Queue-design skills                     | Atomic tasks with requirement traceability, test/implementation/security/compliance ordering, human review, schema validation, refusal behavior                   |
| P3-03 | Implementation and test skills          | Test-generation, implementation, native-documentation skills with only component/immediate-parent/root contracts as context                                       |
| P3-04 | Clean-slate documentation skill         | Source-only documenter excluding `SPEC.md` and prior `IMPL.md`; observed-contract record with uncertainty reporting                                               |
| P3-05 | Independent review skills               | Reviewer inputs, independence boundaries, structured findings, evidence retention; `SPEC.md` vs independently produced `IMPL.md` comparison                       |
| P3-06 | Clean-slate and source-blind pipelines  | Engine-enforced skill enforcement in approved sandbox; candidate `IMPL.md` through reviewed artifact update                                                       |
| P3-07 | Durable human arbitration               | Retained mismatch evidence, explicit human choice (redesign/proposed change/manual resolution), no auto-rewrite of `SPEC.md` or `IMPL.md`                         |
| P3-08 | Specification interview mode            | Resumable `kvist spec interview <COMPONENT_DIR>` workflow; agent routed through approved execution boundary; draft requires explicit acceptance                   |
| P3-09 | Reviewed queue generation               | Planning command passes only accepted specification, immediate-parent contract, root contract to approved architect profile; schema validation; refuses overwrite |

## Phase 4 — Deferred Visual and Editor Ecosystem

All Phase 4 items are deferred until Phase 3 review workflow is independently reviewed and approved. The terminal commands must remain the only required runtime.

| #     | Item                         | Summary                                                                                                                                |
| ----- | ---------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| P4-01 | Local component-state API    | Versioned, authenticated local API for component state, validated artifact views, attempt evidence, approved transitions               |
| P4-02 | Optional local visual client | Browser-based tree and editor; explicit editing workflow; no bypass of spec/queue/approval/review checks                               |
| P4-03 | Visual arbitration support   | Side-by-side `SPEC.md` / `IMPL.md` comparison with explicit confirmation for any resolution                                            |
| P4-04 | Opt-in editor diagnostics    | `kvist status --json` foreground process; diagnostics for spec validity, queue validity, stale revisions, dependency cycles; no daemon |

---

# Remaining Prioritized Backlog

## Onboarding and Integration

### TODO ONB-01 — Convert Existing Project to Kvist Component

- **Context:** A project that already has source code, tests, and a `Cargo.toml` needs to be converted into a Kvist-managed component without losing existing work. This is the most common onboarding path.
- **Acceptance criteria:**
  - `kvist init <PROJECT_DIR>` detects an existing `Cargo.toml` and prompts to create a component directory structure.
  - The tool preserves the existing `Cargo.toml`, `src/`, `tests/`, and `benches/` as the component's implementation root.
  - A new `SPEC.md` is optionally proposed via `kvist spec interview` or `kvist spec new`.
  - A draft `TODOS.yaml` is generated from the existing `Cargo.toml`'s manifest fields (name, version, authors, dependencies, features).
  - The resulting directory layout is:
    ```
    <PROJECT_DIR>/
      Cargo.toml              # existing, preserved
      .kvist/                 # Kvist metadata directory
        SPEC.md              # created or accepted
        TODOS.yaml           # generated from Cargo.toml
        IMPL.md              # generated via clean-slate pipeline
        COMPLIANCE_REVIEW.md # review evidence
      src/                    # existing source
      tests/                  # existing tests
      benches/                # existing benchmarks
    ```
  - The user must explicitly accept the generated `SPEC.md` and `TODOS.yaml` via `kvist spec accept` and `kvist queue accept` before any task runs.
  - No files are overwritten without explicit confirmation.
  - Write integration tests for: existing `Cargo.toml` detection, preservation of existing files, spec interview flow, TODOS generation from `Cargo.toml`, and the "accept all" flow.

### TODO ONB-02 — Import Kvist Artifacts from a Git Repository

- **Context:** A component may have already been developed by an external agent or human and committed to Git. The user wants to bring it into the Kvist workflow.
- **Acceptance criteria:**
  - `kvist import <REPO_URL> --branch <BRANCH> --component <COMPONENT_DIR>` clones or fetches a branch and detects Kvist artifacts (`SPEC.md`, `TODOS.yaml`, `IMPL.md`).
  - If artifacts are absent, the tool offers to create a new component from the directory layout.
  - Existing `TODOS.yaml` entries are validated and marked as "imported"; any blocked tasks are presented as unresolved.
  - The import respects the same lock and approval rules as a new component.
  - Write integration tests for: successful import with existing artifacts, import without artifacts (new component creation), and import of a blocked component.

### TODO ONB-03 — Persist Task State to Disk

- **Context:** A task may be in-progress or blocked when the system crashes or the process exits unexpectedly. The queue must survive so that work is not lost.
- **Acceptance criteria:**
  - The component's `TODOS.yaml` is stored as a versioned artifact in the component directory (`.kvist/TODOS.yaml`) at every state transition.
  - On startup, `kvist task run` reads the persisted queue from disk and reconstructs the in-memory state.
  - In-progress tasks are resumed from their last known state; blocked tasks are presented for review.
  - If the component directory was removed and re-added, the persisted queue is re-read from disk.
  - Write integration tests for: task in-progress state persistence, task blocked state persistence, and queue reconstruction after a "crash" (simulated by writing the artifact then reading it back).

---

## UX and Developer Experience Improvements (Terminal Focus)

### DONE UX-02 — Add Missing Status Filters

- **Context:** Currently `kvist status` outputs the entire project's component tree unconditionally. While useful for high-level overviews, automated scripts or terminal developers working on a specific component require filtered views to isolate components in specific states.
- **Acceptance criteria:**
  - Add `--only-specs` to print only the status of specifications (`SPEC.md` validity, digest state) without queue details.
  - Add `--only-impls` to list and focus on implementation statuses across components (`IMPL.md` validity).
  - Add `--unfinished` to show only components that are blocked, stale, or incomplete (omitting `current` components).
  - Ensure these status filters are fully compatible with both the default plain-text reporter and the machine-readable JSON report.
  - Write integration tests in `tests/status.rs` to verify correct filtering behavior.

### DONE UX-03 — Support "Transparent" Namespace Directories

- **Context:** The current discovery engine in `src/discovery.rs` enforces a strict, unbroken hierarchical component chain. If a folder acts as a pure namespace or container (e.g., `src/network/protocols/http/` where `protocols/` is just an empty namespace directory with no Kvist metadata), discovery currently rejects or fails to resolve the nested component unless the user initializes meaningless "ghost components" at every layer.
- **Acceptance criteria:**
  - Refactor the discovery engine traversal loop in `src/discovery.rs` to allow pass-through/transparent directories that do not contain Kvist metadata (`SPEC.md`, `TODOS.yaml`, `IMPL.md`).
  - Directories containing no Kvist artifacts must not be reported as "incomplete components" or cause validation failures; instead, discovery must traverse through them to locate nested child components further down.
  - Ensure the parent/child component hierarchy remains semantically intact (e.g., the parent of `http` is the next ancestor component higher up, skipping the transparent `protocols` directory).
  - Write regression tests verifying traversal through multiple layers of non-component folders.

### DONE UX-05 — Multi-Platform Shell Tab-Completions

- **Context:** Full tab-completion on all major platforms (Bash, Zsh, Fish, PowerShell) is crucial for ease of use and speed.
- **Acceptance criteria:**
  - Add `clap_complete` to dependencies in `Cargo.toml`.
  - Implement a `kvist completions <SHELL>` subcommand that generates shell completion scripts for all major shells on stdout.
  - Ensure all subcommands, arguments, and value-enums are dynamically completion-discoverable across platforms.
  - Add a workspace validation or standard `justfile` recipe to generate and verify these completion scripts.

### DONE UX-06 — Command-Line Actionable Guidance & Next-Step Prompts

- **Context:** Users need clear, helpful next actions printed to the console based on their command output, without compromising deterministic, script-friendly command output.
- **Acceptance criteria:**
  - Provide actionable, proactive guidance in human-oriented failure and mutation results (plain-text reporter).
  - For example, when a task fails, suggest running `kvist task log`; when a component specification is modified, suggest `kvist spec accept`; and when no tasks are ready but some are blocked, prompt the user to resolve the blocked task or manual gate.
  - Keep the versioned machine-readable JSON formats completely free of unsolicited human prose to preserve parsing stability.

### DONE UX-07 — Uniform Structured JSON Output Support for All Commands

- **Context:** Users must be able to easily wrap Kvist inside their own scripts, IDE extensions, or custom GUIs/UIs. All CLI commands must support structured, machine-readable output.
- **Acceptance criteria:**
  - Support a uniform `--json` flag across every single Kvist command (including `init`, `spec new`, `spec accept`, `task run`, and `task log`).
  - Define and serialize stable JSON schemas for all command outputs.
  - Document these JSON schemas under `/docs/` to prevent wrapping integrations from breaking.
  - Verify JSON output compliance across all CLI commands using automated tests.

### DONE UX-08 — Add Lock Management and Manual Unlock Commands (Operational Gap)

- **Context:** If a task execution is forcefully aborted, crashed, or canceled, the user-owned lifecycle lock may be orphaned. Because the lock resides in a sandbox-inaccessible location (remediated in P2-08), it cannot be cleared by external agents, blocking subsequent execution runs.
- **Acceptance criteria:**
  - Implement `kvist task unlock <COMPONENT_DIR>` to allow manual unlocking of stuck directories.
  - Prompt the user for explicit confirmation before removing an active lock, unless `--force` is supplied.
  - Print clear, actionable instructions when lock acquisition fails (e.g., "Component is locked. If this is a stale lock from a previous crash, run `kvist task unlock <COMPONENT_DIR>` to clear it").
  - Validate with integration tests that `unlock` successfully resolves locked contention states.

### DONE UX-09 — Task Attempt Recovery and Reconciliation Tooling (Operational Gap)

- **Context:** As highlighted in `COMPLIANCE_REVIEW.md` (under residual limitations of Phase 2 reconciliation), a prepared task attempt that is aborted, timed out, or interrupted leaves a "prepared" record in the attempt history. This fences off the queue and blocks future transitions for that task. Currently, there is no CLI way to reconcile this, requiring manual file editing.
- **Acceptance criteria:**
  - Implement a `kvist task recover <COMPONENT_DIR> [TASK_ID]` command.
  - Provide interactive, safe recovery choices to the user:
    1. **Rollback:** Append an auditable recovery record that restores the task to its prior state (for example, `todo` or `in-progress`) without deleting the prepared attempt.
    2. **Resolve:** Record a human-supplied final outcome (for example, `completed` or `blocked`) with its required evidence reference.
  - Ensure the command validates the queue structure and integrity before writing.
  - Write integration tests reproducing crashed runs and verifying both rollback and recorded-resolution paths.

### DONE UX-10 — Robust Process-Tree Cancellation & Cleanup (Robustness Gap)

- **Context:** The resource limits implemented in P2-05d terminate timed-out or canceled external processes. However, if an agent or test-policy runner spawns children (e.g., compile daemons, child subprocesses), terminating only the immediate child can leave orphan background processes running on the host.
- **Acceptance criteria:**
  - Refactor subprocess execution in `src/sandbox.rs` to launch processes inside a new process group or platform-equivalent containment boundary.
  - On timeout or cancellation, use platform APIs rather than a shell command to terminate the whole contained process tree.
  - Provide a fail-closed or explicitly documented fallback for unsupported platforms and test the platform-specific behavior.
  - Add integration tests verifying that child processes of timed-out runs are fully terminated.

---

## Phase 3 — Independent Compliance Automation

Phase 3 is deferred until P2-09 closes. It may automate the existing human-directed lifecycle, but must preserve durable artifacts, strict component context boundaries, required sandbox approval, and independent certification.

### DONE P3-03 — Define implementation and test skills

- **Context:** Automated implementation must remain constrained by the component contract rather than by peer implementation details or chat state.
- **Acceptance criteria:**
  - Define test-generation, implementation, and native-documentation skills with only the component, immediate-parent contract, and root contract as required context.
  - Require tests for public behavior, boundaries, malformed input, and failure paths before implementation is certified.

### DONE P3-04 — Define the clean-slate documentation skill

- **Context:** `IMPL.md` is credible only when observed from implementation without access to the specification it will later be compared against.
- **Acceptance criteria:**
  - Define a source-only documenter skill that receives implementation source, tests, and manifests, but excludes `SPEC.md` and prior `IMPL.md`.
  - Require an observed-contract record that reports uncertainty and never copies planned requirements into implementation evidence.

### DONE P3-05 — Define independent review skills

- **Context:** No implementer may certify its own work; compliance needs separate structural, security, test-coverage, error-handling, and spec-to-implementation review evidence.
- **Acceptance criteria:**
  - Define reviewer inputs, independence boundaries, structured findings, and evidence retention for each review type.
  - Require the final compliance skill to compare `SPEC.md` with independently produced `IMPL.md`, not with source code or implementer claims.

### DONE P3-06 — Implement clean-slate and source-blind pipelines

- **Context:** The defined skills must be enforced by the Rust engine, not only by prompts, before automated compliance claims are allowed.
- **Acceptance criteria:**
  - Run the clean-slate documenter through the approved sandbox with only source, tests, and manifests mounted; write a candidate `IMPL.md` only through an explicit reviewed artifact update.
  - Run a separate compliance checker with only `SPEC.md`, candidate `IMPL.md`, the immediate-parent specification, and `ROOT_CONTRACT.md`; exclude source files and tests.
  - Persist review evidence, mark mismatches blocked, and permit completion only after the independent compliance record is present.

### DONE P3-07 — Implement durable human arbitration

- **Context:** A compliance mismatch must stop automated progress and retain enough evidence for a human to resolve it without losing the original specification or observed implementation record.
- **Acceptance criteria:**
  - Present redesign, proposed contract change, manual arbitration, and optional trade-off analysis as explicit human choices.
  - Preserve the discrepancy, rationale, selected action, and resulting task state in a version-controlled component artifact.
  - Never automatically rewrite `SPEC.md` or `IMPL.md`; proposed changes require review and explicit acceptance before revalidation or task reset.

### DONE P3-08 — Implement specification interview mode

- **Context:** A guided terminal workflow can reduce specification friction without weakening the architect's authority over externally visible behavior.
- **Acceptance criteria:**
  - Implement a resumable `kvist spec interview <COMPONENT_DIR>` workflow that asks about purpose, interfaces, constraints, algorithms, and failures.
  - If an agent is used, route it through the approved execution boundary; do not launch an interactive shell or overwrite an existing specification.
  - Produce a draft that passes normal specification validation and still requires explicit human acceptance.

### DONE P3-09 — Implement reviewed queue generation

- **Context:** Once a specification is accepted, a designer can draft a component-local queue, but the engine must not replace human-authored work implicitly.
- **Acceptance criteria:**
  - Implement a planning command that passes only the accepted component specification, immediate-parent contract, and root contract to the approved architect profile.
  - Validate the generated queue schema, dependency graph, task ordering, and requirement traceability before presenting a draft.
  - Refuse to overwrite an existing queue; require explicit human review and acceptance of any replacement.

---

## Phase 4 — Deferred Visual and Editor Ecosystem

Phase 4 begins only after the terminal execution boundary and Phase 3 review workflow are independently reviewed. Every integration remains optional: core commands must stay headless, portable, credential-free, and daemon-free.

### DONE P4-01 — Provide a local component-state API

- **Context:** A visual client needs a stable read and mutation boundary rather than direct access to internal files or opaque process state.
- **Acceptance criteria:**
  - Define a versioned, authenticated local API for component state, validated artifact views, attempt evidence, and approved transitions.
  - If `kvist serve` is introduced, bind it to loopback, make startup explicit, define shutdown and token/port handling, and add a justified dependency review before adopting a web framework.
  - Preserve the same validation, approval, and atomic-write rules used by the terminal commands.

### DONE P4-02 — Build an optional local visual client

- **Context:** A browser-based tree and editor can improve navigation, but it must not become a required runtime or alter the filesystem-native model.
- **Acceptance criteria:**
  - Render the recursive component tree and current, stale, blocked, and invalid states from the versioned API.
  - Make editing an explicit, validated artifact workflow; do not bypass specification, queue, approval, or review checks.
  - Assess embedded assets and editor dependencies for size, maintenance, licensing, offline operation, and security before inclusion.

### DONE P4-03 — Add visual arbitration support

- **Context:** Side-by-side comparison may help humans resolve a retained mismatch, but the UI must enforce the same explicit decision record as the terminal flow.
- **Acceptance criteria:**
  - Show `SPEC.md`, independently generated `IMPL.md`, findings, and the durable arbitration history without exposing excluded review context.
  - Require an explicit human confirmation for redesign, a proposed contract update, or any manual resolution; never write either artifact implicitly.

### DONE P4-04 — Add opt-in editor diagnostics

- **Context:** Editors can surface stale or invalid artifacts early, but continuous background work must not become a core requirement.
- **Acceptance criteria:**
  - Define `kvist status --json` as a foreground, user-started process with explicit lifecycle, resource, and cross-platform behavior.
  - Publish standard diagnostics for specification validity, queue validity, stale revisions, and dependency cycles without mutating project files.
  - Test shutdown, filesystem races, and unsupported-platform behavior; do not require telemetry, credentials, cloud services, or a persistent daemon.

---

_KVIST — Structured design for autonomous agents._
