# Kvist Implementation Tracker

**Authority:** [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
**Reviewed:** Phase 1 & Phase 2 implementation audits
**Reviewed by:** Stefan Kvist | 2026-08-24

## Status conventions

- `TODO` — scoped and ready once its dependencies are done.
- `IN PROGRESS` — actively being implemented.
- `BLOCKED` — needs an explicit product or security decision.
- `DONE` / `COMPLETE` — acceptance criteria and listed verification are complete.

---

# Completed Milestones

Below is the record of completed Phase 1, Phase 2, and UX milestones.

<details>
<summary><b>Click to expand completed milestones (16 items)</b></summary>

- **P1-Core** — Core CLI engine features (`init`, `tree`, `spec new`, `spec validate`, bounded directory traversal, direct symlink safety checks, and read-only VCS tracking diagnostics).
- **P2-01 — Specify independent TODO queue and dependency graph schemas** (Version-2 parsing, semantic validation, deterministic serialization, root-inspection integration, and contract tests are complete. Compliance review documented in `COMPLIANCE_REVIEW.md`).
- **P2-02 — Implement project inspection and machine-readable status** (`kvist status` renders deterministic version-1 text and JSON reports from shared root/component model).
- **P2-03 — Implement safe task selection and execution state updates** (Task selection and transition contract, lock and attempt-record recovery rules, and integration tests complete).
- **P2-04 — Implement User-Provided Agent Invocation Mechanics** (Implemented under `src/config.rs` and `src/agent.rs` with configuration precedence, shell-free spawning, log capture, and optional token-record parsing.)
- **P2-04b — Implement Basic CLI-Wrapper Templates** (The two configured profiles support `{prompt}`, `{context_files}`, and `{target_directory}` through whitespace-delimited, shell-free arguments. This is not a general shell or quoting language.)
- **P2-05 — Implement test-command verification as an explicit trust boundary** (Configured test-policy verification with bounded execution and durable result persistence; later P2-05b through P2-05d complete its isolation, approval, and resource controls.)
- **P2-05b — Sandbox all external execution** (Agents and verifiers require a component-only, deny-network external sandbox runner; unavailable isolation fails before task mutation.)
- **P2-05c — Bind cryptographic approval to execution configuration** (Authenticated user-state approval binds effective agents, sandbox runner, test policy, and versions; changed or forged inputs fail before execution.)
- **P2-05d — Bound agent subprocess resources** (Per-profile timeouts, combined-output limits, cancellation, and redacted bounded evidence block unsafe agent runs.)
- **P2-06 — Implement the atomic task execution loop** (`kvist task run <COMPONENT_DIR> [TASK_ID]` driver, concurrent locks, atomic progress/blocked state transitions).
- **P2-07 — Perform Phase 2 security and compliance review** (Independent security, clean-slate documentation, and source-blind compliance passes completed. Retained review items remain for the external-execution boundary; this record does not approve production execution.)
- **P2-08 — Complete execution-boundary compliance reconciliation** (Fresh clean-slate and source-blind reviews, explicit documentation arbitration, and legal durable queue transitions close the final Phase 2 lifecycle work.)
- **P2-09a — Model-resolution coverage** (The agent profiles now resolve a named model per role. `model` overrides `default_model`; the `default` and `default-model` aliases select the first listed model. A normal selected model uses the existing shell-free argument interpolation and may prefix `system_prompt`. An entry named `none` bypasses both interpolation and prompt prefixing, but still runs as a sandboxed external program under the approved execution policy. The selected source and complete profiles are approval-bound, so changing model selection or its command requires `kvist task approve-policy` again before `kvist task run`. Context: the resolver now selects a named model before command execution, but its behavior is security-sensitive: an incorrect fallback could invoke a different external program than the human approved. Acceptance criteria: cover explicit selection, `default_model` fallback, first-entry aliases, unknown-name diagnostics, raw `none`, and system-prompt prefixing without requiring an installed provider. Verify that a selection failure happens before sandbox probing or task mutation and lists the available names deterministically.)
- **P2-09b — Template-contract validation** (The model command uses a deliberately narrow, shell-free argument template. Unsupported or unresolved placeholders must never be presented as a supported provider integration. Context: model commands use a deliberately narrow, shell-free argument template. Unsupported or unresolved placeholders must never be presented as a supported provider integration. Acceptance criteria: exercise repeated context-path expansion, prompt and target substitution, quoting limitations, and non-shell behavior. Decide whether unsupported placeholders are rejected at configuration load time or resolved by a documented mechanism before execution. In particular, resolve or remove the built-in Ollama template's literal `{model}` placeholder; until then, document it as unsupported.)
- **P2-09c — Execution portability decision** (The descriptor-bound sandbox-runner launch currently fails closed on platforms without that mechanism. The supported behavior must be explicit rather than inferred from the portable CLI surface. Context: the descriptor-bound sandbox-runner launch currently fails closed on platforms without that mechanism. The supported behavior must be explicit rather than inferred from the portable CLI surface. Acceptance criteria: define the supported non-Linux execution behavior and its security invariant, or retain fail-closed refusal with an actionable diagnostic. Add platform-gated tests for every supported execution path and document the result in the user-facing execution policy.)

</details>

---

# Remaining Prioritized Backlog

## UX and Developer Experience Improvements (Terminal Focus)

These items focus on polishing Kvist for daily terminal usage, wrapping, and developer experience.

### DONE UX-02 — Add Missing Status Filters

- **Context:** Currently `kvist status` outputs the entire project's component tree unconditionally. While useful for high-level overviews, automated scripts or terminal developers working on a specific component require filtered views to isolate components in specific states.
- **Acceptance criteria:**
  - Add `--only-specs` to print only the status of specifications (`SPEC.md` validity, digest state) without queue details.
  - Add `--only-impls` to list and focus on implementation statuses across components (`IMPL.md` validity).
  - Add `--unfinished` to show only components that are blocked, stale, or incomplete (omitting `current` components).
  - Ensure these status filters are fully compatible with both the default plain-text reporter and the machine-readable versioned JSON report.
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
  - Support a uniform `--json` or `--format json` flag across every single Kvist command (including `init`, `spec new`, `spec accept`, `task run`, and `task log`).
  - Define and serialize stable, versioned JSON schemas for all command outputs.
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

### IN PROGRESS UX-10 — Robust Process-Tree Cancellation & Cleanup (Robustness Gap)

- **Context:** The resource limits implemented in P2-05d terminate timed-out or canceled external processes. However, if an agent or test-policy runner spawns children (e.g., compile daemons, child subprocesses), terminating only the immediate child can leave orphan background processes running on the host.
- **Acceptance criteria:**
  - Refactor subprocess execution in `src/sandbox.rs` to launch processes inside a new process group or platform-equivalent containment boundary.
  - On timeout or cancellation, use platform APIs rather than a shell command to terminate the whole contained process tree.
  - Provide a fail-closed or explicitly documented fallback for unsupported platforms and test the platform-specific behavior.
  - Add integration tests verifying that child processes of timed-out runs are fully terminated.

---

## Phase 3 — Independent Compliance Automation

Phase 3 is deferred until P2-09 closes. It may automate the existing human-directed lifecycle, but must preserve durable artifacts, strict component context boundaries, required sandbox approval, and independent certification.

### DONE P3-01 — Define component-design and feasibility skills

- **Context:** Architects need repeatable, reviewable guidance for turning a project vision into bounded components and layered specifications without letting an agent silently choose product behavior.
- **Acceptance criteria:**
  - Define versioned architect, specification-interview, and feasibility-review skill contracts with required inputs, permitted context, outputs, and human approval gates.
  - Require feasibility output to identify unresolved decisions, contradictions, bounds, and failure paths before a queue is drafted.

### TODO P3-02 — Define queue-design skills

- **Context:** A generated plan is only useful if it remains a valid, component-local `TODOS.yaml` that traces back to an accepted specification.
- **Acceptance criteria:**
  - Define a designer skill that produces atomic tasks with requirement traceability and the required test, implementation, security, and compliance ordering.
  - Specify human review, schema validation, and refusal behavior for ambiguous or incomplete specifications.

### TODO P3-03 — Define implementation and test skills

- **Context:** Automated implementation must remain constrained by the component contract rather than by peer implementation details or chat state.
- **Acceptance criteria:**
  - Define test-generation, implementation, and native-documentation skills with only the component, immediate-parent contract, and root contract as required context.
  - Require tests for public behavior, boundaries, malformed input, and failure paths before implementation is certified.

### TODO P3-04 — Define the clean-slate documentation skill

- **Context:** `IMPL.md` is credible only when observed from implementation without access to the specification it will later be compared against.
- **Acceptance criteria:**
  - Define a source-only documenter skill that receives implementation source, tests, and manifests, but excludes `SPEC.md` and prior `IMPL.md`.
  - Require an observed-contract record that reports uncertainty and never copies planned requirements into implementation evidence.

### TODO P3-05 — Define independent review skills

- **Context:** No implementer may certify its own work; compliance needs separate structural, security, test-coverage, error-handling, and spec-to-implementation review evidence.
- **Acceptance criteria:**
  - Define reviewer inputs, independence boundaries, structured findings, and evidence retention for each review type.
  - Require the final compliance skill to compare `SPEC.md` with independently produced `IMPL.md`, not with source code or implementer claims.

### TODO P3-06 — Implement clean-slate and source-blind pipelines

- **Context:** The defined skills must be enforced by the Rust engine, not only by prompts, before automated compliance claims are allowed.
- **Acceptance criteria:**
  - Run the clean-slate documenter through the approved sandbox with only source, tests, and manifests mounted; write a candidate `IMPL.md` only through an explicit reviewed artifact update.
  - Run a separate compliance checker with only `SPEC.md`, candidate `IMPL.md`, the immediate-parent specification, and `ROOT_CONTRACT.md`; exclude source files and tests.
  - Persist review evidence, mark mismatches blocked, and permit completion only after the independent compliance record is present.

### TODO P3-07 — Implement durable human arbitration

- **Context:** A compliance mismatch must stop automated progress and retain enough evidence for a human to resolve it without losing the original specification or observed implementation record.
- **Acceptance criteria:**
  - Present redesign, proposed contract change, manual arbitration, and optional trade-off analysis as explicit human choices.
  - Preserve the discrepancy, rationale, selected action, and resulting task state in a version-controlled component artifact.
  - Never automatically rewrite `SPEC.md` or `IMPL.md`; proposed changes require review and explicit acceptance before revalidation or task reset.

### TODO P3-08 — Implement specification interview mode

- **Context:** A guided terminal workflow can reduce specification friction without weakening the architect's authority over externally visible behavior.
- **Acceptance criteria:**
  - Implement a resumable `kvist spec interview <COMPONENT_DIR>` workflow that asks about purpose, interfaces, constraints, algorithms, and failures.
  - If an agent is used, route it through the approved execution boundary; do not launch an interactive shell or overwrite an existing specification.
  - Produce a draft that passes normal specification validation and still requires explicit human acceptance.

### TODO P3-09 — Implement reviewed queue generation

- **Context:** Once a specification is accepted, a designer can draft a component-local queue, but the engine must not replace human-authored work implicitly.
- **Acceptance criteria:**
  - Implement a planning command that passes only the accepted component specification, immediate-parent contract, and root contract to the approved architect profile.
  - Validate the generated queue schema, dependency graph, task ordering, and requirement traceability before presenting a draft.
  - Refuse to overwrite an existing queue; require explicit human review and acceptance of any replacement.

---

## Phase 4 — Deferred Visual and Editor Ecosystem

Phase 4 begins only after the terminal execution boundary and Phase 3 review workflow are independently reviewed. Every integration remains optional: core commands must stay headless, portable, credential-free, and daemon-free.

### TODO P4-01 — Provide a local component-state API

- **Context:** A visual client needs a stable read and mutation boundary rather than direct access to internal files or opaque process state.
- **Acceptance criteria:**
  - Define a versioned, authenticated local API for component state, validated artifact views, attempt evidence, and approved transitions.
  - If `kvist serve` is introduced, bind it to loopback, make startup explicit, define shutdown and token/port handling, and add a justified dependency review before adopting a web framework.
  - Preserve the same validation, approval, and atomic-write rules used by the terminal commands.

### TODO P4-02 — Build an optional local visual client

- **Context:** A browser-based tree and editor can improve navigation, but it must not become a required runtime or alter the filesystem-native model.
- **Acceptance criteria:**
  - Render the recursive component tree and current, stale, blocked, and invalid states from the versioned API.
  - Make editing an explicit, validated artifact workflow; do not bypass specification, queue, approval, or review checks.
  - Assess embedded assets and editor dependencies for size, maintenance, licensing, offline operation, and security before inclusion.

### TODO P4-03 — Add visual arbitration support

- **Context:** Side-by-side comparison may help humans resolve a retained mismatch, but the UI must enforce the same explicit decision record as the terminal flow.
- **Acceptance criteria:**
  - Show `SPEC.md`, independently generated `IMPL.md`, findings, and the durable arbitration history without exposing excluded review context.
  - Require an explicit human confirmation for redesign, a proposed contract update, or any manual resolution; never write either artifact implicitly.

### TODO P4-04 — Add opt-in editor diagnostics

- **Context:** Editors can surface stale or invalid artifacts early, but continuous background work must not become a core requirement.
- **Acceptance criteria:**
  - Define `kvist lsp` and any optional watch mode as foreground, user-started processes with explicit lifecycle, resource, and cross-platform behavior.
  - Publish standard diagnostics for specification validity, queue validity, stale revisions, and dependency cycles without mutating project files.
  - Test shutdown, filesystem races, and unsupported-platform behavior; do not require telemetry, credentials, cloud services, or a persistent daemon.

---

_KVIST — Structured design for autonomous agents._
