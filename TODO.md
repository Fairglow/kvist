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

## Baseline Completed

Phase 1 core CLI engine features, including `init`, `tree`, `spec new`, `spec validate`, bounded directory traversal, direct symlink safety checks, and read-only VCS tracking diagnostics are complete and fully covered by our integration test suite.

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

### COMPLETE P2-02 — Implement project inspection and machine-readable status

**Depends on:** P1-R1, P2-01
**Current evidence:** `kvist status` now renders deterministic version-1 text
and JSON reports from the shared root/component inspection model. It reports
missing, invalid, unsupported-version, stale, blocked, and current state
without writing files. Focused fixtures, independent security audit, and
clean-slate/source-blind review evidence are retained in
`COMPLIANCE_REVIEW.md`.
**Acceptance criteria:**

- Build one validated project/component state model shared by `init`, `tree`,
  TODO validation, and execution.
- Provide stable non-interactive output and documented exit codes for scripts
  and the future web/LSP layers; decide whether JSON is required.
- Surface missing, stale, invalid, unsupported, and blocked states without
  mutating the project.

**Verification:** golden tests for text and any machine-readable output, plus
state fixtures produced by P1-R1.

### COMPLETE P2-03 — Implement safe task selection and execution state updates

**Depends on:** P2-01, P2-02
**Current evidence:** The task selection and transition contract, lock and
attempt-record recovery rules, and focused failing CLI fixtures are retained
in `src/SPEC.md`, `src/TODOS.yaml`, and `tests/task_commands.rs`.
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

### COMPLETE P2-04 — Define User-Provided Agent Invocation Contract

**Depends on:** P2-02
**Current evidence:** Implemented under `src/config.rs` and `src/agent.rs` with full XDG/APPDATA persistent loading, multi-tier override chains, and safe program parameter tokenization.
**Acceptance criteria:**

- Define the command-line invocation protocol used by Kvist to launch the user-provided agent/runner.
- Support configuring agent paths, settings, and task-specific model routing (e.g., advanced model for specification breakdown, and a simple model for individual TODO implementation) in `kvist.toml`.
- Implement token-usage tracking and reporting interfaces, enabling Kvist to parse token consumption from agent outputs and enforce limits per component or task.
- Preserve strict context slicing: Kvist feeds only local target component directories, immediate parent interface contracts, and `ROOT_CONTRACT.md` to the agent.

**Verification:** integration tests for agent invocation, custom path handling, token output parsing, and context slicing bounds.

### COMPLETE P2-05 — Implement test-command verification as an explicit trust boundary

**Depends on:** P2-01, P2-03
**Current evidence:** Implemented under `src/config.rs` and `src/task_commands.rs` with explicit test-command policies, working directory isolation, timeout boundaries, capped stdout/stderr buffers, and canonical cryptographic SHA-256 policy approval.
**Decision gates (owner approval required before tests or code):**

1. Policy artifact: versioned location, schema, and component inheritance.
2. Authority: who approves commands and how a changed policy is detected.
3. Execution boundary: trusted-workspace/isolation requirement, command
   working directory, environment allowlist, timeout, cancellation, and output cap.
4. Verification semantics: required task states, durable redacted result
   record, failure/cancellation handling, and retry/recovery policy.
   **Acceptance criteria:**

- Define where test commands are configured and who may approve changes to
  them; do not execute arbitrary repository text implicitly.
- Execute tests with bounded output, timeout, cancellation, explicit working
  directory, and captured failure evidence.
- Record test results against the task attempt and require success before
  implementation tasks transition to complete.

**Verification:** command policy, timeout, cancellation, output limit,
nonzero exit, and result-persistence integration tests.

### COMPLETE P2-06 — Implement the atomic task execution loop

**Depends on:** P2-03, P2-04, P2-05
**Current evidence:** Integrated `kvist task run <COMPONENT_DIR> [TASK_ID]` driver under `src/task_commands.rs` and `src/cli.rs`. It resolves dependencies, triggers atomic InProgress transitions, launches interpolated commands, redirects logs asynchronously, and resolves output status Completed/Blocked.
**Acceptance criteria:**

- Drive one approved task at a time through context preparation, user-provided agent invocation, change inspection, test verification, and state transition.
- Support parallel execution: Let Kvist manage concurrent/asynchronous execution of tasks across different, independent components in the tree, utilizing component-level locks (`.kvist-task.lock`) to prevent state collision.
- Record progress state durably as things progress, enabling the developer to jump around, expand on things that might have been partially completed, or update specifications at any level.

**Verification:** end-to-end fixtures using a mock user-provided agent and test command, including parallel component execution, failure handling, and state resumption.

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

| Decision                   | Current contract                                                                                                                                                                                                                                                                               |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Implementation language    | Stable Rust, edition 2024; headless local CLI.                                                                                                                                                                                                                                                 |
| Toolchain and dependencies | MSRV Rust 1.85; `Cargo.lock` is committed; intentional dependency updates must pass MSRV and stable CI.                                                                                                                                                                                        |
| Initial CLI                | `init`, `tree`, `spec new`, and `spec validate`; project paths default to the current directory where applicable.                                                                                                                                                                              |
| Project location           | One project-local `kvist.toml`; no global configuration or parent-directory discovery.                                                                                                                                                                                                         |
| Root artifact paths        | `kvist.toml`, `ROOT_CONTRACT.md`, and `src/{SPEC.md,TODOS.yaml,DOCS.md}` (where `DOCS.md` is logically `IMPL.md` / the component's internal Implementation Compliance Record).                                                                                                                 |
| Initial configuration      | Version `1`, `component_root = "src"`, `llm.provider = "none"`.                                                                                                                                                                                                                                |
| Specification format       | Version-1 Markdown with exact three ordered `<details>` layers and required headings.                                                                                                                                                                                                          |
| Root-state and recovery    | Five-state read-only inspection. Phase 1 never repairs or migrates automatically; `doctor` guides explicit user recovery.                                                                                                                                                                      |
| Artifact version domains   | Configuration, root contract, specification, TODO queue, and documentation have independent version domains.                                                                                                                                                                                   |
| VCS policy                 | Before Phase 2, durable artifacts must be tracked in a supported VCS (Git or jj); required ignored artifacts are reported. Kvist never auto-stages or commits; logs, locks, raw provider data, and credentials remain untracked.                                                               |
| Filesystem safety          | No-clobber atomic file persistence; direct Unix-link/Windows-reparse rejection; lexical, bounded discovery; component-only intermediate hierarchy; ignored VCS/build directories. Static-workspace inspection is not a sandbox or TOCTOU defense; Phase 2 requires a trusted-workspace policy. |
| Input limits               | `kvist.toml` at most 64 KiB; specifications and root contract/TODO/documentation inspection at most 1 MiB each.                                                                                                                                                                                |
| Dependencies               | `clap`, `thiserror`, `toml`, `tempfile`, and `serde_yaml`; add dependencies only with a documented need.                                                                                                                                                                                       |
| Review model               | Clean-slate source implementation documentation (logically `IMPL.md`) followed by source-blind specification comparison.                                                                                                                                                                       |

## Open questions requiring an explicit decision

### 1. **Task Execution Trust & Sandboxing Boundary**

- **The Decision:** Kvist acts purely as a process enforcer and context-slicing wrapper. The execution of the AI agent itself is delegated to a user-provided executable (e.g., a wrapper script, Claude Code, Cursor, or Gemini CLI) that operates under the user's host permissions.
- **The Impact:** Kvist carries **NO credentials or API keys**, completely eliminating credential leakage risks. Execution trust is fully owned by the user-provided runner. Kvist's role is restricted to preparing strict, minimal context boundaries (the target component directory, its parent interface, and the root contract) and invoking the user's agent with these scoped inputs.
- **How It Works:** In `kvist.toml`, the user configures the command path (e.g., `agent_runner = "claude-code --non-interactive"`). Kvist executes this command, mounting or supplying only the sliced directory, and monitors stdout/stderr or metadata outputs for completion status and token reporting.

### 2. **Provider and Agent Interface Protocol**

- **The Decision:** Define a simple, open-ended shell-invocation protocol that passes context inputs and gathers agent results without coupling Kvist to any specific LLM provider or SDK.
- **The Impact:** Since Kvist is provider-agnostic, users can configure any local, cloud, or enterprise agent.
- **How It Works:** Kvist launches the configured runner command, passing a structured environment or JSON input describing the target task (e.g., writing tests, code implementation) and the paths of the allowed context files. The runner performs the task and returns a standard JSON structure on stdout containing:
  - `status`: success, failure, or blocked.
  - `tokens_used`: input/output token counts for reporting.
  - `error_message`: if failure occurred.

### 3. **Agent and Model Tiering for Task Specialization**

- **The Decision:** Support configuring different models/agents and settings for different tasks based on complexity and context size.
- **The Impact:** Dramatically reduces API costs while maximizing reasoning quality where it matters most (the high-level design and compliance boundaries).
- **How It Works:** In `kvist.toml`, users can define agent profiles:
  - **Architect Profile (Advanced Model, e.g., Claude 3.5 Sonnet, Gemini 1.5 Pro):** Assigned to Stage 1 (creating/validating specs, designing sub-component hierarchies, breaking specs into `TODOS.yaml` tasks) and Stage 4 (triple-blind compliance verification, specification drift analysis, security audit).
  - **Developer Profile (Simple Model, e.g., Claude Haiku, Gemini 1.5 Flash):** Assigned to Stage 3 (writing unit tests, implementing individual TODO items, and generating inline source docstrings).

### 4. **Asynchronous & Parallel Multi-Component Execution**

- **The Decision:** Support concurrent task execution across separate, independent component directories.
- **The Impact:** Speeds up execution in large-scale multi-component codebases by allowing multiple agents to implement different branches of the component tree in parallel.
- **How It Works:** Since each component is self-contained with its own `SPEC.md` and `TODOS.yaml`, Kvist can spawn multiple asynchronous workers. Each component directory is protected by its own `.kvist-task.lock` lockfile, preventing simultaneous writes and ensuring absolute thread-safety during concurrent executions.

### 5. **Open-Ended Iterative Human-Agent Design Workflow**

- **The Decision:** Optimize Kvist for a fluid, jump-around developer experience rather than a linear, one-way progression.
- **The Impact:** Human engineers can continuously edit specifications at any layer of the hierarchy, and the engine automatically handles the dependency cascades.
- **How It Works:**
  - A user can jump into any component (even one previously marked "completed"), edit its `SPEC.md`, and add new requirements.
  - Kvist automatically detects this during the next status check, marks that component and its children `Stale`, and re-opens the task execution loop.
  - Work can be handed off asynchronously: the human user designs/refines the spec, and then hands off the implementation, test generation, and compliance review to the agent runner on demand.

### 6. **Licensing and Enterprise Distribution**

- **The Decision:** Determine what exact BSL/double-license terms, Cargo metadata, release channel, and binary distribution policy apply to ensure open non-commercial access while safeguarding enterprise commercial use.

## UX and Developer Experience Improvements

### DONE UX-01 — Implement a Revalidation / Accept CLI Interface

**Context:** Currently, when a component specification or parent contract changes, the component is marked `Stale`, but Kvist provides no CLI command to accept these changes. Developers must manually compute SHA-256 hashes and edit `TODOS.yaml`.
**Acceptance criteria:**

- Create `kvist spec accept <COMPONENT_DIR>` to resolve staleness programmatically.
- The command must compute the new SHA-256 digest of `SPEC.md`, update `specification_revision` in `TODOS.yaml`, and set `revalidation.state` to `Current` while clearing `causes`.

### TODO UX-02 — Add Missing Status Filters

**Context:** `TASKS.md` requires listing only specifications, implementations, and blocked/unfinished work, but `kvist status` currently outputs the entire tree state unconditionally.
**Acceptance criteria:**

- Add `--only-specs` to list components and validate their `SPEC.md` without queue details.
- Add `--only-impls` to list implementation statuses across components.
- Add `--unfinished` to show only blocked, stale, or incomplete components.

### TODO UX-03 — Support "Transparent" Namespace Directories

**Context:** Discovery rejects components that live below ordinary directories, forcing developers to initialize meaningless "ghost components" just for namespacing (e.g., `src/network/protocols/http` forces `protocols` to be a Kvist component).
**Acceptance criteria:**

- Refactor the discovery engine in `src/discovery.rs` to allow pass-through / transparent directories.
- Ensure that only directories containing Kvist artifacts are treated as components, without strict hierarchical unbroken chain requirements.

## Phase 2 Remediation & Security Improvements

### DONE P2-04b — General CLI-Wrapper Template Engine for External Agents

**Context:** To support as many different user-provided AI agents as possible (e.g., Claude Code, Gemini CLI, Aider, Cursor, or custom local wraps) without requiring Kvist to store credentials or carry out native API integrations. Kvist will launch these agents via configurable CLI commands.
**Acceptance criteria:**

- Define standard interpolation templates in `kvist.toml` for calling external agent commands (e.g., `agent_command = "claude-code --non-interactive --message '{prompt}' --file {context_files}"`).
- Implement robust string interpolation that safely substitutes `{prompt}`, `{context_files}`, and `{target_directory}` without shell injection risks.
- Support exit-code mapping and standard JSON output parsing for agent status reporting.

### TODO P2-05b — Implement Sandboxing for Test Executions

**Context:** Running repository-defined test commands directly on the host machine is a severe security vulnerability against malicious LLM-generated code.
**Acceptance criteria:**

- Implement secure sandboxing boundaries for test command executions (e.g., via Docker, gVisor, or WASM).
- Ensure only the local directory context is mounted and network access is restrictively controlled.

## UX and Developer Experience Improvements (Terminal Focus)

### DONE UX-04 — Agent Output Redirection, Logging, and Streamlining

**Context:** We want Kvist's standard CLI UX to remain exceptionally clean and streamlined (e.g., simple spinners or success indicators). However, the raw output (stdout, stderr, execution logs) of user-provided agents must be captured and made available to the user upon request.
**Acceptance criteria:**

- Redirect the raw output of launched agents to untracked local component log files (e.g., `<component_dir>/.kvist/logs/<task_id>_<timestamp>.log`).
- Implement the `kvist task log <COMPONENT_DIR> <TASK_ID>` command to display or stream the raw agent execution logs.
- Add a `--verbose` or `--stream` flag to the task execution loop to pipe raw agent output directly to the console in real-time.

### TODO UX-05 — Multi-Platform Shell Tab-Completions

**Context:** Full tab-completion on all major platforms (Bash, Zsh, Fish, PowerShell) is crucial for ease of use and speed.
**Acceptance criteria:**

- Add `clap_complete` to dependencies.
- Implement a `kvist completions <SHELL>` subcommand that generates shell completion scripts for all major shells on stdout.
- Ensure all subcommands, arguments, and value-enums are dynamically completion-discoverable across platforms.

### TODO UX-06 — Command-Line Actionable Guidance & Next-Step Prompts

**Context:** The user must always feel in control and understand the process. They must never be left in the dark about what state the system is in or what needs to be done next.
**Acceptance criteria:**

- Ensure all terminal output sequences end with a clear, actionable instruction for what command the user should run next (e.g., if status is stale, prompt "Run 'kvist spec accept <COMPONENT_DIR>' to revalidate").
- In `kvist task run`, output status markers that explain what the agent did, what files were edited, and suggest next verification runs.

### TODO UX-07 — Uniform Structured JSON Output Support for All Commands

**Context:** Users must be able to easily wrap Kvist inside their own scripts, IDE extensions, or custom GUIs/UIs. All CLI commands must support structured, machine-readable output.
**Acceptance criteria:**

- Support a uniform `--json` or `--format json` flag across every single Kvist command (including `init`, `spec new`, `spec accept`, `task run`, and `task log`).
- Document stable, versioned JSON output schemas for all commands to prevent wrapping integrations from breaking.

## Phase 3 — AI Skill Definitions & Standard Prompts

**Context:** The KVIST engine heavily relies on predictable, high-quality outputs from AI agents executing the lifecycle. Standardized "Skills" (system prompts, context rules, and structured output formats) must be rigorously defined for the terminal-UX agent.

### TODO P3-01 — Component Hierarchy & Feasibility Skills

**Acceptance criteria:**

- **Hierarchy Creation Skill:** Prompting guidelines to recursively break down a complex system into self-contained sub-components.
- **Specification Generation & Review Skill:** Prompting guidelines for the interactive "Interview" mode to define purpose, constraints, and algorithms without writing code.
- **Feasibility Analysis Skill:** A skill for reviewing a draft `SPEC.md` for logical gaps, contradictions, or missing edge cases before tasks are generated.

### TODO P3-02 — Task Generation & TODO Queue Skills

**Acceptance criteria:**

- **Task Breakdown Skill:** Prompting guidelines to convert a validated `SPEC.md` into atomic tasks strictly following the required lifecycle ordering (Test -> Implementation -> Security -> Review).

### TODO P3-03 — Execution Skills (Testing & Implementation)

**Acceptance criteria:**

- **Unit Test Generation Skill:** Directives for writing tests that explicitly verify Layer 1 and Layer 2 invariants from `SPEC.md`.
- **Implementation Skill:** Guidelines for fulfilling the tests.
- **Source Code Documentation Skill:** Instructions for writing language-native docstrings (e.g., `///` in Rust) that cleanly map implementation details to spec requirements, enabling easier reverse-engineering.

### TODO P3-04 — Clean-Slate Documenter Skill

**Acceptance criteria:**

- **Reverse-Engineering Skill:** Define the prompt for the clean-slate agent that extracts `DOCS.md` from raw source code and docstrings _without_ seeing the original `SPEC.md`. Must capture contracts, constraints, and error handling accurately.

### TODO P3-05 — Compliance & Review Skills (Triple-Blind Loop)

**Acceptance criteria:**

- **Code Review Skill:** General structural, stylistic, and idiomatic code review.
- **Security Review Skill:** Focuses explicitly on memory safety, thread-safety, boundaries, and input validation invariants defined in Layer 2.
- **Test Coverage Review Skill:** Validates that tests comprehensively cover edge cases and failure paths defined in Layer 3.
- **Error Handling & Logging Review Skill:** Ensures error states are safely propagated and observability requirements are met.
- **Specification Drift / Contract Fulfillment Skill:** The final compliance prompt that compares the original `SPEC.md` against the generated `DOCS.md` to flag hallucinations or missed requirements.

## Phase 4 — Deferred Visual Web UI & Graphical Ecosystem

**Context:** Deferring all graphical elements (the embedded Axum server, Monaco editor visualizer, and serving commands) to prioritize a robust, process-enforcing CLI core and perfect terminal UX.

### TODO P4-01 — Embedded Web Server & API

**Acceptance criteria:**

- Add `kvist serve` command that spawns a lightweight `axum` server.
- Implement API routes for reading component states, specs, and queues.

### TODO P4-02 — Interactive Tree & Monaco Editor UI

**Acceptance criteria:**

- Embed a SPA (e.g., React or similar) into the Rust binary.
- Integrate Monaco Editor to display and edit `SPEC.md` and source files.

### TODO P4-03 — Conflict Arbitration UI

**Acceptance criteria:**

- Implement Web-based interactive arbitration prompts (Redesign, Accept, Manual Edit, AI Trade-off Analysis).
