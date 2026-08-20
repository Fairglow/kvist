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
<summary><b>Click to expand completed milestones (10 items)</b></summary>

- **P1-Core** — Core CLI engine features (`init`, `tree`, `spec new`, `spec validate`, bounded directory traversal, direct symlink safety checks, and read-only VCS tracking diagnostics).
- **P2-01 — Specify independent TODO queue and dependency graph schemas** (Version-2 parsing, semantic validation, deterministic serialization, root-inspection integration, and contract tests are complete. Compliance review documented in `COMPLIANCE_REVIEW.md`).
- **P2-02 — Implement project inspection and machine-readable status** (`kvist status` renders deterministic version-1 text and JSON reports from shared root/component model).
- **P2-03 — Implement safe task selection and execution state updates** (Task selection and transition contract, lock and attempt-record recovery rules, and integration tests complete).
- **P2-04 — Define User-Provided Agent Invocation Contract** (Implemented under `src/config.rs` and `src/agent.rs` with persistent loading, override chains, and safe program parameter tokenization).
- **P2-04b — General CLI-Wrapper Template Engine for External Agents** (Interpolation templates, string substitution without shell injection risks, exit-code mapping, and JSON output parsing).
- **P2-05 — Implement test-command verification as an explicit trust boundary** (Configured, versioned, cryptographic SHA-256 policy approval with `kvist task approve-policy`, bounded execution, and result-persistence).
- **P2-06 — Implement the atomic task execution loop** (`kvist task run <COMPONENT_DIR> [TASK_ID]` driver, concurrent locks, atomic progress/blocked state transitions).
- **UX-01 — Implement a Revalidation / Accept CLI Interface** (`kvist spec accept <COMPONENT_DIR>` resolves staleness programmatically, computing SHA-256 and updating revisions).
- **UX-04 — Agent Output Redirection, Logging, and Streamlining** (Redirected agent logs to local untracked logs, implemented `kvist task log`, and added real-time stdout/stderr redirection).

</details>

---

# Remaining Prioritized Backlog

## Phase 2 — Post-Execution, Security & Remediation

This phase focuses on finalizing the security model and trust boundaries before Kvist is deployed to execute unverified, machine-generated repository files in production environments.

### TODO P2-07 — Perform Phase 2 security and compliance review

- **Acceptance criteria:**
  - Perform a clean-slate documentation pass and source-blind compliance comparison for the Phase 2 contract.
  - Review subprocess, environment, credentials, filesystem writes, locking, cancellation, and resource-boundary behavior.
  - Record arbitration items explicitly; do not silently rewrite requirements or observed behavior.
- **Verification:** independent review artifacts and an end-to-end Phase 2 fixture.

### TODO P2-05b — Implement Sandboxing for Test Executions

- **Context:** Running repository-defined test commands directly on the host machine is a severe security vulnerability against malicious LLM-generated code.
- **Acceptance criteria:**
  - Implement secure sandboxing boundaries for test command executions (e.g., via Docker, gVisor, or WASM).
  - Ensure only the local directory context is mounted and network access is restrictively controlled.

### TODO P2-05c — Expand Cryptographic Approval to Cover Agent Execution Command Templates (Security Gap)

- **Context:** While the test-command policy (`[test_policy]`) is cryptographically protected by `kvist task approve-policy`, the external agent command template (`[agent]`) is not. A malicious pull request could alter the agent's runner template to execute arbitrary shell payloads during `kvist task run` without triggering policy warnings.
- **Acceptance criteria:**
  - Expand the cryptographic verification and approval boundary to cover BOTH the `[test_policy]` and `[agent]` configuration blocks.
  - Reject task execution if any part of the execution-sensitive config has changed since the last `kvist task approve-policy`.

---

## UX and Developer Experience Improvements (Terminal Focus)

These items focus on polishing Kvist for daily terminal usage, wrapping, and developer experience.

### TODO UX-02 — Add Missing Status Filters

- **Context:** `TASKS.md` requires listing only specifications, implementations, and blocked/unfinished work, but `kvist status` currently outputs the entire tree state unconditionally.
- **Acceptance criteria:**
  - Add `--only-specs` to list components and validate their `SPEC.md` without queue details.
  - Add `--only-impls` to list implementation statuses across components.
  - Add `--unfinished` to show only blocked, stale, or incomplete components.

### TODO UX-03 — Support "Transparent" Namespace Directories

- **Context:** Discovery rejects components that live below ordinary directories, forcing developers to initialize meaningless "ghost components" just for namespacing (e.g., `src/network/protocols/http` forces `protocols` to be a Kvist component).
- **Acceptance criteria:**
  - Refactor the discovery engine in `src/discovery.rs` to allow pass-through / transparent directories.
  - Ensure that only directories containing Kvist artifacts are treated as components, without strict hierarchical unbroken chain requirements.

### TODO UX-05 — Multi-Platform Shell Tab-Completions

- **Context:** Full tab-completion on all major platforms (Bash, Zsh, Fish, PowerShell) is crucial for ease of use and speed.
- **Acceptance criteria:**
  - Add `clap_complete` to dependencies.
  - Implement a `kvist completions <SHELL>` subcommand that generates shell completion scripts for all major shells on stdout.
  - Ensure all subcommands, arguments, and value-enums are dynamically completion-discoverable across platforms.

### TODO UX-06 — Command-Line Actionable Guidance & Next-Step Prompts

- **Context:** The user must always feel in control and understand the process. They must never be left in the dark about what state the system is in or what needs to be done next.
- **Acceptance criteria:**
  - Ensure all terminal output sequences end with a clear, actionable instruction for what command the user should run next (e.g., if status is stale, prompt "Run 'kvist spec accept <COMPONENT_DIR>' to revalidate").
  - In `kvist task run`, output status markers that explain what the agent did, what files were edited, and suggest next verification runs.

### TODO UX-07 — Uniform Structured JSON Output Support for All Commands

- **Context:** Users must be able to easily wrap Kvist inside their own scripts, IDE extensions, or custom GUIs/UIs. All CLI commands must support structured, machine-readable output.
- **Acceptance criteria:**
  - Support a uniform `--json` or `--format json` flag across every single Kvist command (including `init`, `spec new`, `spec accept`, `task run`, and `task log`).
  - Document stable, versioned JSON output schemas for all commands to prevent wrapping integrations from breaking.

### TODO UX-08 — Add Lock Management and Manual Unlock Commands (Operational Gap)

- **Context:** If a task execution is forcefully aborted, crashed, or canceled, the component-level `.kvist-task.lock` file may be orphaned, blocking subsequent `kvist task run` commands.
- **Acceptance criteria:**
  - Implement `kvist task unlock <COMPONENT_DIR>` to allow manual unlock of stuck directories.
  - Print clear, actionable instructions when lock acquisition fails (e.g., "Component is locked. If this is a stale lock from a previous crash, run `kvist task unlock <COMPONENT_DIR>` to clear it").

---

## Phase 3 — AI Skill Definitions & Standard Prompts

The KVIST engine heavily relies on predictable, high-quality outputs from AI agents executing the lifecycle. Standardized "Skills" (system prompts, context rules, and structured output formats) must be rigorously defined for the terminal-UX agent.

### TODO P3-01 — Component Hierarchy & Feasibility Skills

- **Hierarchy Creation Skill:** Prompting guidelines to recursively break down a complex system into self-contained sub-components.
- **Specification Generation & Review Skill:** Prompting guidelines for the interactive "Interview" mode to define purpose, constraints, and algorithms without writing code.
- **Feasibility Analysis Skill:** A skill for reviewing a draft `SPEC.md` for logical gaps, contradictions, or missing edge cases before tasks are generated.

### TODO P3-02 — Task Generation & TODO Queue Skills

- **Task Breakdown Skill:** Prompting guidelines to convert a validated `SPEC.md` into atomic tasks strictly following the required lifecycle ordering (Test -> Implementation -> Security -> Review).

### TODO P3-03 — Execution Skills (Testing & Implementation)

- **Unit Test Generation Skill:** Directives for writing tests that explicitly verify Layer 1 and Layer 2 invariants from `SPEC.md`.
- **Implementation Skill:** Guidelines for fulfilling the tests.
- **Source Code Documentation Skill:** Instructions for writing language-native docstrings (e.g., `///` in Rust) that cleanly map implementation details to spec requirements, enabling easier reverse-engineering.

### TODO P3-04 — Clean-Slate Documenter Skill

- **Reverse-Engineering Skill:** Define the prompt for the clean-slate agent that extracts `DOCS.md` from raw source code and docstrings _without_ seeing the original `SPEC.md`. Must capture contracts, constraints, and error handling accurately.

### TODO P3-05 — Compliance & Review Skills (Triple-Blind Loop)

- **Code Review Skill:** General structural, stylistic, and idiomatic code review.
- **Security Review Skill:** Focuses explicitly on memory safety, thread-safety, boundaries, and input validation invariants defined in Layer 2.
- **Test Coverage Review Skill:** Validates that tests comprehensively cover edge cases and failure paths defined in Layer 3.
- **Error Handling & Logging Review Skill:** Ensures error states are safely propagated and observability requirements are met.
- **Specification Drift / Contract Fulfillment Skill:** The final compliance prompt that compares the original `SPEC.md` against the generated `DOCS.md` to flag hallucinations or missed requirements.

---

## Phase 4 — Deferred Visual Web UI & Graphical Ecosystem

Phase 4 prioritizes an interactive graphical interface. All UI features are deferred until the terminal and CLI core features are completely finalized and secure.

### TODO P4-01 — Embedded Web Server & API

- **Acceptance criteria:**
  - Add `kvist serve` command that spawns a lightweight `axum` server.
  - Implement API routes for reading component states, specs, and queues.

### TODO P4-02 — Interactive Tree & Monaco Editor UI

- **Acceptance criteria:**
  - Embed a SPA (e.g., React or similar) into the Rust binary.
  - Integrate Monaco Editor to display and edit `SPEC.md` and source files.

### TODO P4-03 — Conflict Arbitration UI

- **Acceptance criteria:**
  - Implement Web-based interactive arbitration prompts (Redesign, Accept, Manual Edit, AI Trade-off Analysis).
