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
<summary><b>Click to expand completed milestones (11 items)</b></summary>

- **P1-Core** — Core CLI engine features (`init`, `tree`, `spec new`, `spec validate`, bounded directory traversal, direct symlink safety checks, and read-only VCS tracking diagnostics).
- **P2-01 — Specify independent TODO queue and dependency graph schemas** (Version-2 parsing, semantic validation, deterministic serialization, root-inspection integration, and contract tests are complete. Compliance review documented in `COMPLIANCE_REVIEW.md`).
- **P2-02 — Implement project inspection and machine-readable status** (`kvist status` renders deterministic version-1 text and JSON reports from shared root/component model).
- **P2-03 — Implement safe task selection and execution state updates** (Task selection and transition contract, lock and attempt-record recovery rules, and integration tests complete).
- **P2-04 — Implement User-Provided Agent Invocation Mechanics** (Implemented under `src/config.rs` and `src/agent.rs` with configuration precedence, shell-free spawning, log capture, and optional token-record parsing. The safety policy remains incomplete.)
- **P2-04b — Implement Basic CLI-Wrapper Templates** (The two configured profiles support `{prompt}`, `{context_files}`, and `{target_directory}` through whitespace-delimited, shell-free arguments. This is not a general shell or quoting language.)
- **P2-05 — Implement test-command verification as an explicit trust boundary** (Configured, versioned, cryptographic SHA-256 policy approval with `kvist task approve-policy`, bounded execution, and result-persistence).
- **P2-06 — Implement the atomic task execution loop** (`kvist task run <COMPONENT_DIR> [TASK_ID]` driver, concurrent locks, atomic progress/blocked state transitions).
- **UX-01 — Implement a Revalidation / Accept CLI Interface** (`kvist spec accept <COMPONENT_DIR>` resolves staleness programmatically, computing SHA-256 and updating revisions).
- **UX-04 — Agent Output Redirection, Logging, and Streamlining** (Redirected agent logs to local untracked logs, implemented `kvist task log`, and added real-time stdout/stderr redirection).
- **P2-07 — Perform Phase 2 security and compliance review** (Independent security, clean-slate documentation, and source-blind compliance passes completed. The retained review records release-blocking host-execution, locking, logging, and documentation discrepancies for P2-05b through P2-05d; it does not approve production execution.)

</details>

---

# Remaining Prioritized Backlog

## Phase 2 — Post-Execution, Security & Remediation

This phase focuses on finalizing the security model and trust boundaries before Kvist is deployed to execute unverified, machine-generated repository files in production environments.

### COMPLETE P2-07 — Perform Phase 2 security and compliance review

- **Acceptance criteria:**
  - Perform a clean-slate documentation pass and source-blind compliance comparison for the Phase 2 contract.
  - Review subprocess, environment, credentials, filesystem writes, locking, cancellation, and resource-boundary behavior.
  - Record arbitration items explicitly; do not silently rewrite requirements or observed behavior.
- **Verification:** retained in `COMPLIANCE_REVIEW.md`; `cargo test --locked`
  passed the Phase 2 end-to-end workflow fixture on 2026-08-21. The fixture
  establishes current behavior but does not resolve the explicitly retained
  P2-05b through P2-05d security blockers.

### TODO P2-05b — Sandbox all external execution

- **Context:** `task run` launches both configured agents and repository-defined
  test commands directly on the host. This violates the intended controlled
  execution boundary for untrusted or machine-generated repository content.
- **Acceptance criteria:**
  - Define and implement an opt-in, portable sandbox boundary for both agent
    and test subprocesses, with explicit mount, network, environment, and
    credential policy.
  - Refuse `task run` when the required isolation is unavailable; do not
    silently fall back to host execution.
  - Ensure only the declared component context is available by default.

### TODO P2-05c — Expand Cryptographic Approval to Cover Agent Execution Command Templates (Security Gap)

- **Context:** While the test-command policy (`[test_policy]`) is
  cryptographically protected by `kvist task approve-policy`, the resolved
  external-agent configuration is not. A malicious project, local override, or
  changed global configuration can alter what `kvist task run` executes
  without triggering policy warnings.
- **Acceptance criteria:**
  - Expand the cryptographic verification and approval boundary to cover the
    effective agent configuration, its source path, and `[test_policy]`.
  - Reject task execution if any execution-sensitive configuration has changed
    since explicit approval.

### TODO P2-05d — Bound agent subprocess resources

- **Context:** Test execution has a configured timeout and output cap, but
  agent execution can run indefinitely and write unbounded logs.
- **Acceptance criteria:**
  - Define per-profile timeout, cancellation, and combined output limits.
  - Persist bounded, redacted execution evidence and block a task on a limit
    breach without losing the durable transition record.

---

## UX and Developer Experience Improvements (Terminal Focus)

These items focus on polishing Kvist for daily terminal usage, wrapping, and developer experience.

### TODO UX-02 — Add Missing Status Filters

- **Context:** `TODO.md` calls for focused inspection views, but `kvist status`
  currently outputs the entire tree state unconditionally.
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

- **Context:** Users need clear next actions without compromising deterministic,
  script-friendly command output.
- **Acceptance criteria:**
  - Provide actionable guidance in human-oriented failure and mutation results;
    keep versioned machine-readable formats free of unsolicited prose.
  - In `kvist task run`, report the durable task outcome, execution-log path,
    and verification result without inferring what files an agent edited.

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

- **Architect Agent Skill:** Prompting guidelines to turn a human project vision
  into an iteratively reviewed hierarchy of self-contained components and
  layered specifications.
- **Specification Generation & Review Skill:** Prompting guidelines for the interactive "Interview" mode to define purpose, constraints, and algorithms without writing code.
- **Feasibility Analysis Skill:** A skill for reviewing a draft `SPEC.md` for logical gaps, contradictions, or missing edge cases before tasks are generated.

### TODO P3-02 — Task Generation & TODO Queue Skills

- **Designer Agent Skill:** Prompting guidelines to convert a human-approved
  `SPEC.md` into an iteratively reviewed specialized queue strictly following
  the required lifecycle ordering (Test -> Implementation -> Security ->
  Review).

### TODO P3-03 — Execution Skills (Testing & Implementation)

- **Unit Test Generation Skill:** Directives for writing tests that explicitly verify Layer 1 and Layer 2 invariants from `SPEC.md`.
- **Implementation Skill:** Guidelines for fulfilling the tests.
- **Source Code Documentation Skill:** Instructions for writing language-native docstrings (e.g., `///` in Rust) that cleanly map implementation details to spec requirements, enabling easier reverse-engineering.

### TODO P3-04 — Clean-Slate Documenter Skill

- **Reverse-Engineering Skill:** Define the prompt for the clean-slate agent that extracts `IMPL.md` from raw source code and docstrings _without_ seeing the original `SPEC.md`. Must capture contracts, constraints, and error handling accurately.

### TODO P3-05 — Compliance & Review Skills (Triple-Blind Loop)

- **Code Review Skill:** General structural, stylistic, and idiomatic code review.
- **Security Review Skill:** Focuses explicitly on memory safety, thread-safety, boundaries, and input validation invariants defined in Layer 2.
- **Test Coverage Review Skill:** Validates that tests comprehensively cover edge cases and failure paths defined in Layer 3.
- **Error Handling & Logging Review Skill:** Ensures error states are safely propagated and observability requirements are met.
- **Specification Drift / Contract Fulfillment Skill:** The final compliance prompt that compares the original `SPEC.md` against the generated `IMPL.md` to flag hallucinations or missed requirements.

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
