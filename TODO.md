# Kvist Implementation Tracker

**Authority:** [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
**Reviewed:** Phase 1 & Phase 2 implementation audits

## Status conventions

- `TODO` — scoped and ready once its dependencies are done.
- `IN PROGRESS` — actively being implemented.
- `BLOCKED` — needs an explicit product or security decision.
- `DONE` / `COMPLETE` — acceptance criteria and listed verification are complete.

---

# Completed Milestones

Below is the record of completed Phase 1, Phase 2, and UX milestones.

<details>
<summary><b>Click to expand completed milestones (15 items)</b></summary>

- **P1-Core** — Core CLI engine features (`init`, `tree`, `spec new`, `spec validate`, bounded directory traversal, direct symlink safety checks, and read-only VCS tracking diagnostics).
- **P2-01 — Specify independent TODO queue and dependency graph schemas** (Version-2 parsing, semantic validation, deterministic serialization, root-inspection integration, and contract tests are complete. Compliance review documented in `COMPLIANCE_REVIEW.md`).
- **P2-02 — Implement project inspection and machine-readable status** (`kvist status` renders deterministic version-1 text and JSON reports from shared root/component model).
- **P2-03 — Implement safe task selection and execution state updates** (Task selection and transition contract, lock and attempt-record recovery rules, and integration tests complete).
- **P2-04 — Implement User-Provided Agent Invocation Mechanics** (Implemented under `src/config.rs` and `src/agent.rs` with configuration precedence, shell-free spawning, log capture, and optional token-record parsing.)
- **P2-04b — Implement Basic CLI-Wrapper Templates** (The two configured profiles support `{prompt}`, `{context_files}`, and `{target_directory}` through whitespace-delimited, shell-free arguments. This is not a general shell or quoting language.)
- **P2-05 — Implement test-command verification as an explicit trust boundary** (Configured test-policy verification with bounded execution and durable result persistence; later P2-05b through P2-05d complete its isolation, approval, and resource controls.)
- **P2-06 — Implement the atomic task execution loop** (`kvist task run <COMPONENT_DIR> [TASK_ID]` driver, concurrent locks, atomic progress/blocked state transitions).
- **UX-01 — Implement a Revalidation / Accept CLI Interface** (`kvist spec accept <COMPONENT_DIR>` resolves staleness programmatically, computing SHA-256 and updating revisions).
- **UX-04 — Agent Output Redirection, Logging, and Streamlining** (Redirected agent logs to local untracked logs, implemented `kvist task log`, and added real-time stdout/stderr redirection).
- **P2-07 — Perform Phase 2 security and compliance review** (Independent security, clean-slate documentation, and source-blind compliance passes completed. Retained review items remain for the external-execution boundary; this record does not approve production execution.)
- **P2-05b — Sandbox all external execution** (Agents and verifiers require a component-only, deny-network external sandbox runner; unavailable isolation fails before task mutation.)
- **P2-05c — Bind cryptographic approval to execution configuration** (Authenticated user-state approval binds effective agents, sandbox runner, test policy, and versions; changed or forged inputs fail before execution.)
- **P2-05d — Bound agent subprocess resources** (Per-profile timeouts, combined-output limits, cancellation, and redacted bounded evidence block unsafe agent runs.)
- **P2-08 — Complete execution-boundary compliance reconciliation** (Fresh clean-slate and source-blind reviews, explicit documentation arbitration, and legal durable queue transitions close the final Phase 2 lifecycle work.)

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

### TODO UX-08 — Add Lock Management and Manual Unlock Commands (Operational Gap)

- **Context:** If a task execution is forcefully aborted, crashed, or canceled, the user-owned lifecycle lock may be orphaned. Because the lock resides in a sandbox-inaccessible location (remediated in P2-08), it cannot be cleared by external agents, blocking subsequent execution runs.
- **Acceptance criteria:**
  - Implement `kvist task unlock <COMPONENT_DIR>` to allow manual unlocking of stuck directories.
  - Prompt the user for explicit confirmation before removing an active lock, unless `--force` is supplied.
  - Print clear, actionable instructions when lock acquisition fails (e.g., "Component is locked. If this is a stale lock from a previous crash, run `kvist task unlock <COMPONENT_DIR>` to clear it").
  - Validate with integration tests that `unlock` successfully resolves locked contention states.

### TODO UX-09 — Task Attempt Recovery and Reconciliation Tooling (Operational Gap)

- **Context:** As highlighted in `COMPLIANCE_REVIEW.md` (under residual limitations of Phase 2 reconciliation), a prepared task attempt that is aborted, timed out, or interrupted leaves a "prepared" record in the attempt history. This fences off the queue and blocks future transitions for that task. Currently, there is no CLI way to reconcile this, requiring manual file editing.
- **Acceptance criteria:**
  - Implement a `kvist task recover <COMPONENT_DIR> [TASK_ID]` command.
  - Provide interactive, safe recovery choices to the user:
    1. **Rollback:** Delete the trailing prepared attempt record and restore the task to its previous state (e.g., `todo` or `in-progress`).
    2. **Force-Commit:** Manually supply the final outcome state (e.g., `complete` or `blocked`) along with the required verification log path.
  - Ensure the command validates the queue structure and integrity before writing.
  - Write integration tests reproducing crashed runs and verifying success of both rollback and commit recovery paths.

### TODO UX-10 — Robust Process-Tree Cancellation & Cleanup (Robustness Gap)

- **Context:** The resource limits implemented in P2-05d terminate timed-out or canceled external processes. However, if an agent or test-policy runner spawns children (e.g., compile daemons, child subprocesses), terminating only the immediate child can leave orphan background processes running on the host.
- **Acceptance criteria:**
  - Refactor subprocess execution in `src/sandbox.rs` to launch processes inside a new Process Group (PGID).
  - Upon timeout or cancellation, send the termination signal to the entire process group (e.g., `kill -- -PGID` on POSIX hosts) to ensure complete cleanup.
  - Provide a safe fallback for unsupported platforms and document the platform-specific limitations.
  - Add integration tests verifying that child processes of timed-out runs are fully terminated.

---

## Phase 3 — AI Skill Definitions, Prompt Engineering & Engine Orchestration

Phase 3 transitions Kvist from a manual task-tracking CLI into an automated agent execution platform. It requires defining standardized prompts ("Skills") and implementing the Rust orchestration pipelines that execute the independent Triple-Blind compliance lifecycle.

### AI Skill & Prompt Definitions

#### TODO P3-01 — Component Hierarchy & Feasibility Skills

- **Architect Agent Skill:** Prompting guidelines to turn a human project vision into an iteratively reviewed hierarchy of self-contained components and layered specifications.
- **Specification Generation & Review Skill:** Prompting guidelines for the interactive "Interview" mode to define purpose, constraints, and algorithms without writing code.
- **Feasibility Analysis Skill:** A skill for reviewing a draft `SPEC.md` for logical gaps, contradictions, or missing edge cases before tasks are generated.

#### TODO P3-02 — Task Generation & TODO Queue Skills

- **Designer Agent Skill:** Prompting guidelines to convert a human-approved `SPEC.md` into an iteratively reviewed specialized queue strictly following the required lifecycle ordering (Test -> Implementation -> Security -> Review).

#### TODO P3-03 — Execution Skills (Testing & Implementation)

- **Unit Test Generation Skill:** Directives for writing tests that explicitly verify Layer 1 and Layer 2 invariants from `SPEC.md`.
- **Implementation Skill:** Guidelines for fulfilling the tests.
- **Source Code Documentation Skill:** Instructions for writing language-native docstrings (e.g., `///` in Rust) that cleanly map implementation details to spec requirements, enabling easier reverse-engineering.

#### TODO P3-04 — Clean-Slate Documenter Skill

- **Reverse-Engineering Skill:** Define the prompt for the clean-slate agent that extracts `IMPL.md` from raw source code and docstrings _without_ seeing the original `SPEC.md`. Must capture contracts, constraints, and error handling accurately.

#### TODO P3-05 — Compliance & Review Skills (Triple-Blind Loop)

- **Code Review Skill:** General structural, stylistic, and idiomatic code review.
- **Security Review Skill:** Focuses explicitly on memory safety, thread-safety, boundaries, and input validation invariants defined in Layer 2.
- **Test Coverage Review Skill:** Validates that tests comprehensively cover edge cases and failure paths defined in Layer 3.
- **Error Handling & Logging Review Skill:** Ensures error states are safely propagated and observability requirements are met.
- **Specification Drift / Contract Fulfillment Skill:** The final compliance prompt that compares the original `SPEC.md` against the generated `IMPL.md` to flag hallucinations or missed requirements.

### Rust Engine & Orchestration Implementation (Engine Gaps)

#### TODO P3-06 — Implement Clean-Slate Documenter & Source-Blind Review Engine Pipelines

- **Context:** The core Rust CLI engine must automate the execution of the clean-slate and source-blind reviewer agent pipelines defined in `REVIEW_RUNBOOK.md` and the Architectural Specification.
- **Acceptance criteria:**
  - Implement a pipeline that invokes the "Clean-Slate Documenter" agent using the configured `architect` profile.
  - Ensure the pipeline's sandbox strictly isolates the agent, mounting _only_ implementation source code, tests, and manifests. The component's `SPEC.md` and prior `IMPL.md` must be completely excluded from the agent's workspace.
  - Save the agent's output as the new `IMPL.md`.
  - Implement a second pipeline that invokes the "Compliance Checker" agent, mounting _only_ `SPEC.md`, the newly generated `IMPL.md`, the parent spec, and `ROOT_CONTRACT.md` (no source files or tests).
  - Parse the checker's output. If compliance is certified, record it and update task status to `complete`. If mismatches are found, raise a compliance mismatch and set the task state to `blocked`.

#### TODO P3-07 — Implement Interactive CLI Arbitration Workflow

- **Context:** When the compliance checker agent detects a mismatch, the engine must halt automated queue execution and trigger the conflict arbitration workflow defined in Section 4 of the architectural specification.
- **Acceptance criteria:**
  - Implement an interactive terminal prompt presented when a compliance mismatch occurs.
  - Display the specific mismatch details (e.g., "Spec requires non-blocking I/O, but frame.rs uses blocking connect").
  - Provide four actionable choices:
    1. **Trigger Agent Redesign:** Feed the mismatch details back to the implementation agent and reset the implementation/testing tasks.
    2. **Accept Implementation Changes:** Automatically update `SPEC.md` with the new observed behavior and recompute revisions.
    3. **Manually Arbitrate:** Launch the user's `$EDITOR` to let them manually reconcile the files.
    4. **AI Trade-off Analysis:** Query an AI assistant to weigh the pros/cons of the discrepancy before choosing an action.
  - Persist the selected arbitration decision as a signed, durable record under the component's VCS metadata.

#### TODO P3-08 — Implement CLI Interactive "Interview" Mode for Specification Drafting

- **Context:** The "Interview" mode (Section 3, Stage 1 of the specification) is essential to reduce specification friction, helping human architects draft valid three-layered specifications through interactive guided dialogue.
- **Acceptance criteria:**
  - Implement `kvist spec interview <COMPONENT_DIR>`.
  - Load the `architect` profile and run a specialized interactive shell session.
  - The agent asks structured questions regarding component purpose, external boundaries, concurrency requirements, algorithms, and error handling.
  - Once the user is satisfied, the agent writes the formal three-layered `SPEC.md` and exits.
  - Integrate command options to resume an interrupted interview.

#### TODO P3-09 — Implement Automated TODO Queue Generation Engine

- **Context:** Once a specification is approved, a designer agent must draft the initial `TODOS.yaml` from it. This process needs to be orchestrated by the Rust engine.
- **Acceptance criteria:**
  - Implement `kvist spec plan <COMPONENT_DIR>` or integrate queue generation.
  - Call the `architect` profile, passing the newly validated `SPEC.md` and root/parent contracts.
  - The designer agent generates a complete, valid `TODOS.yaml` that enforces all schema invariants (version, tasks, dependency graphs, kind-based order).
  - The engine validates the generated YAML. If valid, writes it to disk; otherwise, reports details and retry/re-prompt options.

---

## Phase 4 — Deferred Visual Web UI & Graphical Ecosystem

Phase 4 prioritizes an interactive graphical interface and editor integration. All UI features are deferred until the terminal and CLI core features are completely finalized, secure, and audited.

### TODO P4-01 — Embedded Web Server & API

- **Context:** An interactive UI needs a local service to query and mutate component states.
- **Acceptance criteria:**
  - Add `kvist serve` command that spawns a lightweight, local-only `axum` web server.
  - Implement REST/WebSocket API routes for reading component states, specs, attempt logs, and triggering transitions.
  - Strictly bind the server to `127.0.0.1` and randomize ports (or use a secure local token) to prevent unauthorized access.

### TODO P4-02 — Interactive Tree & Monaco Editor UI

- **Acceptance criteria:**
  - Embed a SPA (e.g., React or similar) into the Rust binary using `rust-embed`.
  - Integrate Monaco Editor to display and edit `SPEC.md`, `IMPL.md`, and source files.
  - Provide a clean visual layout showcasing the recursive component tree with live visual state indicators (current, stale, blocked).

### TODO P4-03 — Conflict Arbitration UI

- **Context:** Mismatch resolution is highly visual. Users need side-by-side diff views to compare intended specification logic against reverse-engineered implementation records.
- **Acceptance criteria:**
  - Implement a visual conflict arbitration screen in the web UI.
  - Display side-by-side rich diffs comparing the mismatched sections of `SPEC.md` and `IMPL.md`.
  - Provide interactive buttons mapped to the four arbitration options (Redesign, Accept, Manual, Trade-off Analysis) with smooth modal windows.

### TODO P4-04 — Lightweight Editor LSP Sidecar & Watch Daemon (Editor Gap)

- **Context:** Developers want real-time feedback on specification staleness or validation errors directly in their local IDE (VS Code, Neovim, Zed, etc.) without requiring full UI launches.
- **Acceptance criteria:**
  - Implement `kvist watch [DIR]`, spawning a lightweight file watcher that monitors changes to component artifacts and revalidates specs/queues on the fly.
  - Build a lightweight Language Server Protocol (LSP) sidecar mode within the CLI (`kvist lsp`).
  - Support standard LSP diagnostics (`publishDiagnostics`) to flag spec-mismatches, missing artifacts, or broken dependency cycles directly inside standard IDE editors.

---

_KVIST — Structured design for autonomous agents._
