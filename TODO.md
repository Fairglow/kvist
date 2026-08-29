# Kvist Implementation Tracker

**Authority:** [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
**Reviewed:** Phase 1, Phase 2, Phase 3, and all UX hardening items
**Reviewed by:** Stefan Kvist | 2026-08-25
**Build:** `cargo build --release` passes with 0 warnings

## Status conventions

- `TODO` — scoped and ready once its dependencies are done.
- `IN PROGRESS` — actively being implemented.
- `BLOCKED` — needs an explicit product or security decision.

---

# Remaining Prioritized Backlog

## Onboarding and Integration

### TODO ONB-01 — Convert Existing Project to Kvist Component

**Context:** A project that already has source code, tests, and a `Cargo.toml` needs to be converted into a Kvist-managed component without losing existing work. This is the most common onboarding path.

**Acceptance criteria:**

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

**Remediation plan (2026-08-28):**

- **ONB-01a — Define conversion lifecycle and tests:** Specify a distinct
  `draft`, `spec-accepted`, and `queue-accepted` conversion state, its durable
  versioned evidence, and backward-compatible CLI/JSON behavior. Write
  failing integration tests for every legal and illegal lifecycle transition,
  including a modified draft invalidating prior acceptance.
- **ONB-01b — Integrate converted-project inspection:** Extend the shared
  project-state and component-discovery model to inspect `.kvist` conversion
  artifacts as a component whose implementation root is the project directory.
  Preserve current-project inspection behavior and report actionable draft,
  partial, stale, invalid, and accepted states without writes.
- **ONB-01c — Implement explicit acceptance:** Add `kvist queue accept` and
  conversion-aware `kvist spec accept`, requiring a valid reviewed
  specification before queue acceptance. Persist acceptance evidence atomically
  and reject link-like, partial, malformed, or changed artifacts.
- **ONB-01d — Connect execution safely:** Make `task next`, `transition`,
  `run`, `log`, and VCS/approval checks resolve a converted component's
  metadata and implementation root consistently. Prove no task can start
  before both acceptances and that logs, locks, and recovery evidence remain
  component-scoped.
- **ONB-01e — Harden draft generation:** Replace fixed timestamps, include
  recursively discovered source/test/benchmark evidence in the source-blind
  implementation draft, bound all manifest and filesystem reads, and make
  manifest rendering deterministic and safely escaped.
- **ONB-01f — Independently audit and certify:** Run a security review of
  acceptance/metadata trust boundaries and a clean-slate, source-blind
  compliance comparison before marking ONB-01 complete.

### TODO ONB-02 — Import Kvist Artifacts from a Git Repository

**Context:** A component may have already been developed by an external agent or human and committed to Git. The user wants to bring it into the Kvist workflow.

**Acceptance criteria:**

- `kvist import <REPO_URL> --branch <BRANCH> --component <COMPONENT_DIR>` clones or fetches a branch and detects Kvist artifacts (`SPEC.md`, `TODOS.yaml`, `IMPL.md`).
- If artifacts are absent, the tool offers to create a new component from the directory layout.
- Existing `TODOS.yaml` entries are validated and marked as "imported"; any blocked tasks are presented as unresolved.
- The import respects the same lock and approval rules as a new component.
- Write integration tests for: successful import with existing artifacts, import without artifacts (new component creation), and import of a blocked component.

### TODO ONB-03 — Persist Task State to Disk

**Context:** A task may be in-progress or blocked when the system crashes or the process exits unexpectedly. The queue must survive so that work is not lost.

**Acceptance criteria:**

- The component's `TODOS.yaml` is stored as a versioned artifact in the component directory (`.kvist/TODOS.yaml`) at every state transition.
- On startup, `kvist task run` reads the persisted queue from disk and reconstructs the in-memory state.
- In-progress tasks are resumed from their last known state; blocked tasks are presented for review.
- If the component directory was removed and re-added, the persisted queue is re-read from disk.
- Write integration tests for: task in-progress state persistence, task blocked state persistence, and queue reconstruction after a "crash" (simulated by writing the artifact then reading it back).

### TODO ONB-04 — Reverse-Discovery: Generate Specification from Existing Implementation

**Context:** The Kvist system must be capable of reverse-engineering a specification from an existing implementation. This allows existing codebases to be imported and turned into properly specified, versioned components without starting from scratch.

**Requirements:**

1. **Input:** A directory containing an existing project with source code, tests, and documentation (e.g., a Rust crate with `Cargo.toml`, `src/`, `tests/`, `README.md`).
2. **Analysis Steps:**
   - Parse source code to identify modules, components, and interfaces (for Rust: modules, `pub` items, trait definitions, struct/enums).
   - Detect existing tests and infer test cases.
   - Detect existing documentation (README, doc comments, markdown files).
   - Infer component boundaries from directory structure and module exports.
3. **Spec Generation:**
   - Generate a `SPEC.md` for each detected component with:
     - Purpose and scope
     - Public API (types, functions, methods)
     - Invariants and constraints
     - Dependencies on other components
   - Generate a `TODOS.yaml` with:
     - Tasks to refactor existing code into Kvist components
     - Tasks to add missing tests
     - Tasks to add documentation
     - Tasks to fix security issues (if detected)
4. **Output:** A `.kvist/` directory with generated artifacts (`SPEC.md`, `TODOS.yaml`, `IMPL.md`).
5. **User Review:** The generated artifacts are presented to the user for review. The user can:
   - Accept the generated specification and queue
   - Modify the specification and regenerate
   - Reject and start from scratch
6. **Integration with Phase 3:** The generated specification is treated as a draft and requires independent review via Phase 3 workflows (clean-slate documentation skill, source-blind pipeline, human arbitration).

**Constraints:**

- The generated specification is a suggestion, not a guarantee of correctness.
- The user must explicitly accept the generated artifacts before they are used.
- The generated specification is versioned and tracked in the repository.
- The generated specification must not overwrite an existing, accepted `SPEC.md` without explicit confirmation.

**Acceptance criteria:**

- `kvist reverse-discover <PATH>` scans a directory and produces a draft `.kvist/` directory.
- The generated `SPEC.md` must be valid Markdown and pass `kvist spec validate`.
- The generated `TODOS.yaml` must be valid YAML and pass schema validation.
- The generated `IMPL.md` must be derived from the implementation alone (not from `SPEC.md`).
- Write integration tests for: reverse-discovery on a simple crate, reverse-discovery on a multi-component project, and rejection of overwriting an existing accepted spec.

**Context for future work:** This feature is part of the broader onboarding and integration effort. It enables Kvist to work with existing projects that were not designed with Kvist in mind. The reverse-discovery process is a one-time or infrequent operation, unlike the normal workflow which is driven by human-directed AI development.

---

## Agent & Model Capability Enhancements

### TODO AGN-01 — Dedicated Security Reviewer Role & Task Mapping

**Context:** Security auditing represents a highly specialized category of reviews. Running security audits under the same general `architect` model profile is sub-optimal. We need a dedicated `security_reviewer` role configured with special-purpose models and customized system prompts.

**Acceptance criteria:**

- Introduce `Role::SecurityReviewer` (command spelling: `security-reviewer`).
- Map `TaskKind::SecurityAudit` to `config.agent.security_reviewer` with fallback to `architect`.
- Ensure all signature validation, approval-policy checks, and serialization structures include the new profile's digests.
- Write unit/integration tests to verify correct role routing and approval-signature validation when the security reviewer is configured.

### TODO AGN-02 — Supervised Custom Prompt Execution with Loop Detection

**Context:** Local LLMs (including `llama-cli`) can hang or get stuck in infinite repetitive cycles. We need a supervised prompt execution command that monitors live output streams, implements configurable idle watchdogs, and analyzes streams for repetition loops.

**Acceptance criteria:**

- Implement `kvist prompt [PROMPT]` command supporting options: `--role`, `--idle-timeout`, `--detect-loops`, and `--max-restarts`.
- Real-time streaming: Read stdout/stderr chunk-by-chunk and print to the console immediately.
- Idle watchdogs: Terminate and restart the subprocess if no new bytes are written within `idle_timeout` seconds (default 15 minutes).
- Loop detection: Analyze a rolling suffix buffer for consecutive matching cycle patterns (consecutive identical substrings of length 10-512 repeating >= 3 times, or consecutive line patterns). Terminate and restart the process upon detection.
- Max automatic restarts limits (default 3) to prevent infinite restart loops.

### TODO AGN-03 — Model Setup Wizard with Wrapper Script Support

**Context:** Users run local models with unique startup configurations, e.g. using helper scripts like `~/bin/llama-cli.sh`. The wizard must guide the user through setting up standard providers (llama-cli, llama-server, Ollama, Copilot, Gemini) and support custom shell wrappers.

**Acceptance criteria:**

- Add command `kvist agent setup` to launch an interactive CLI wizard.
- Wizard automatically queries Ollama's model tags or searches the shell `PATH` for standard binaries.
- Supports user-provided script wrappers (like `~/bin/llama-cli.sh`) by checking execute permissions and setting up custom templates.
- Runs a non-destructive verification test prompt and streams output to verify the connection.
- Programmatically writes the generated profile to the user's preferred configuration file (`~/.config/kvist/config.toml` or `kvist.toml`).

### TODO AGN-04 — Language-Specific BKM Prompt templates (Rust & Python)

**Context:** To enforce code quality and stylistic consistency, tasks must be generated and implemented using Best Known Methods (BKMs). Kvist should use modifiable prompt templates for Rust and Python unit tests, docstrings, and error patterns.

**Acceptance criteria:**

- Create a set of customizable templates under `.kvist/templates/` (or global user config directory) for developer tasks.
- For Rust: Enforce standard naming conventions, idiomatic `Result`/`Option` handling, explicit module visibilities, and doc comment tests.
- For Python: Enforce type annotations, PEP-8 formatting, Pydantic or standard data structures, and standard `unittest` / `pytest` suites.
- Read and inject the corresponding template during `task run` execution based on detected files or explicit configuration.

### TODO AGN-05 — Configurable Log Retention & Monotonic Naming

**Context:** Agent execution outputs accumulate quickly. Kvist should support log file cleanup according to a retention policy and use descriptive, monotonic file names to simplify tracing.

**Acceptance criteria:**

- Log files must follow a trace-friendly naming convention: `<component_dir>/.kvist/logs/<task_id>_<attempt_seq>_<timestamp>.log`.
- Add a configuration field `agent.logs.retention_days` (defaulting to 7 days).
- Upon task run, automatically scan the logs folder and remove log files older than the retention threshold.

### TODO AGN-06 — Multi-Task Parallel Execution Support

**Context:** In large systems with independent components, running audits, reviews, or implementations sequentially is slow. Because Kvist locks are component-scoped, tasks on separate components can run concurrently.

**Acceptance criteria:**

- Add option `kvist task run-all --parallel` to discover ready tasks across all components and execute them in parallel up to a CPU-concurrency limit.
- Ensure that task queue files and task locks are safely isolated, avoiding collision on state updates.

---

## Summary of Completed Work

All items in the following sections have been completed and integrated into the codebase:

- **Phase 1 — Core Engine:** `kvist init`, `kvist tree`, `kvist spec new`, `kvist spec validate`, bounded directory traversal, symlink safety checks, read-only VCS tracking.
- **Phase 2 — Execution Boundary:** Independent TODO queue, project inspection, safe task selection, user-provided agent invocation, test-command verification, sandboxed execution, cryptographic approval binding, resource-bounded subprocesses, atomic task execution, security/compliance review, execution-boundary reconciliation, model-resolution coverage, template-contract validation, execution portability decisions.
- **Phase 3 — Independent Compliance Automation:** Component-design and feasibility skills, queue-design skills, implementation and test skills, clean-slate documentation skill, independent review skills, clean-slate and source-blind pipelines, durable human arbitration, specification interview mode, reviewed queue generation.
- **Phase 4 — Deferred Visual and Editor Ecosystem:** Deferred until Phase 3 review workflow is independently reviewed and approved.
- **Onboarding and Integration:** `kvist convert` (ONB-01, existing project conversion), `kvist import` (ONB-02, remote/local Git repository import with validation and automatic conversion/init fallback), and `kvist reverse-discover` (ONB-04, automatic generation of specifications and TDD task queues from existing implementation files) are fully implemented and verified.
- **Agent and Model Capability Enhancements:** `security_reviewer` agent profile (AGN-01, dedicated security auditor role profile, task routing mapping, and cryptographic HMAC approval validation) and supervised custom prompt execution (AGN-02, real-time chunked streaming, idle watchdog timeout-breaking, and consecutive cycle/line/oscillation loop detection with automated restarts) are fully implemented and verified.
- **UX Improvements:** All 10 UX items (UX-02 through UX-10) have been completed.

For detailed documentation of the completed work, refer to [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md).
