# Kvist Implementation Tracker

**Authority:** [`VISION.md`](VISION.md) -> [`ARCHITECTURE.md`](ARCHITECTURE.md)
-> component `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`
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

### [COMPLETED] TODO ONB-01 — Convert Existing Project to Kvist Component

**Context:** An existing Rust project needs draft Kvist intent and queue
artifacts without losing source, tests, benchmarks, or manifest data.

**Acceptance criteria:**

- `kvist init <PROJECT_DIR>` detects the existing Rust project, while
  `kvist convert <PROJECT_DIR>` exposes conversion explicitly.
- Existing `Cargo.toml`, `src/`, `tests/`, and `benches/` remain unchanged.
- Source-derived `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md` are marked
  as drafts rather than inferred truth.
- A traceable draft `TODOS.yaml` uses `requirements_revision`,
  `contract_revision`, `design_revision`, and `parent_contract`; the parent
  path crosses transparent namespace directories to the nearest ancestor
  component contract.
- The metadata layout is:

  ```text
  <PROJECT_DIR>/
    Cargo.toml
    .kvist/
      REQUIREMENTS.md
      CONTRACT.md
      DESIGN.md
      TODOS.yaml
      IMPL.md
    src/
    tests/
    benches/
  ```
- `kvist component validate .kvist` validates all three intent documents;
  `kvist component accept .kvist` records reviewed intent and immediate-parent
  contract revisions.
- Conversion never recognizes or migrates retired component artifacts and
  never overwrites existing metadata.
- Tests cover detection, preservation, bounded deterministic generation,
  validation, acceptance, stale drafts, and task-execution refusal before
  review.

### [COMPLETED] TODO ONB-02 — Import Kvist Artifacts from a Git Repository

**Context:** A component developed elsewhere may be brought into a local Kvist
workflow as untrusted repository content.

**Acceptance criteria:**

- `kvist import <REPO_URL> --branch <BRANCH> --component <COMPONENT_DIR>`
  imports a selected repository and detects the current requirements, contract,
  design, queue, and implementation-record set.
- If artifacts are absent, onboarding creates explicit drafts rather than
  asserting inferred intent.
- Imported queues are validated and blocked tasks remain unresolved.
- The import respects the same lock and approval rules as a new component.
- Tests cover a current artifact set, absent artifacts, blocked state,
  malformed input, and no-clobber destination handling.

This repository operation is not a claim of ReqIF, architecture-model,
OpenAPI, AsyncAPI, or other general interchange support. Those adapters remain
deferred as documented in `docs/standards.md`.

### [COMPLETED] TODO ONB-03 — Persist Task State to Disk

**Context:** A task may be in-progress or blocked when the system crashes or the process exits unexpectedly. The queue must survive so that work is not lost.

**Acceptance criteria:**

- The component's `TODOS.yaml` is stored as a versioned artifact in the component directory (`.kvist/TODOS.yaml`) at every state transition.
- On startup, `kvist task run` reads the persisted queue from disk and reconstructs the in-memory state.
- In-progress tasks are resumed from their last known state; blocked tasks are presented for review.
- If the component directory was removed and re-added, the persisted queue is re-read from disk.
- Write integration tests for: task in-progress state persistence, task blocked state persistence, and queue reconstruction after a "crash" (simulated by writing the artifact then reading it back).

### [COMPLETED] TODO ONB-04 — Reverse-Discovery: Generate Draft Intent from Existing Implementation

**Context:** Existing source can provide evidence for a draft component model,
but implementation cannot prove intended product outcomes by itself.

**Requirements:**

1. **Input:** A directory containing an existing project with source code, tests, and documentation (e.g., a Rust crate with `Cargo.toml`, `src/`, `tests/`, `README.md`).
2. **Analysis Steps:**
   - Parse source code to identify modules, components, and interfaces (for Rust: modules, `pub` items, trait definitions, struct/enums).
   - Detect existing tests and infer test cases.
   - Detect existing documentation (README, doc comments, markdown files).
   - Infer component boundaries from directory structure and module exports.
3. **Intent Generation:**
   - Generate draft `REQUIREMENTS.md` with observed candidate outcomes,
     constraints, acceptance gaps, and verification needs.
   - Generate draft `CONTRACT.md` with observed public interfaces and failure
     behavior. Reference optional native schemas only by exact path and
     dialect/version.
   - Generate draft `DESIGN.md` with observed internal structure, algorithms,
     state, failure recovery, and uncertainty.
   - Generate a `TODOS.yaml` with:
     - Tasks to refactor existing code into Kvist components
     - Tasks to add missing tests
     - Tasks to add documentation
     - Tasks to fix security issues (if detected)
4. **Output:** A `.kvist/` directory containing the three draft intent
   documents, `TODOS.yaml`, and independently observed `IMPL.md`.
5. **User Review:** The generated artifacts are presented to the user for review. The user can:
   - validate and accept the generated intent set;
   - modify requirements, contract, or design and regenerate the queue; or
   - Reject and start from scratch
6. **Independent Review:** Clean-slate `IMPL.md` derivation excludes all intent
   documents. A separate reviewer compares requirements, contract, and design
   with the record and test evidence before human arbitration.

**Constraints:**

- Generated intent is a suggestion, not a guarantee of correctness.
- The user must explicitly accept the generated artifacts before they are used.
- Generated artifacts are independently versioned and tracked.
- Existing accepted intent is never overwritten.
- Retired component formats are neither read as current intent nor migrated.

**Acceptance criteria:**

- `kvist reverse-discover <PATH>` scans a directory and produces a draft `.kvist/` directory.
- The generated intent documents must pass `kvist component validate`.
- The generated `TODOS.yaml` must be valid YAML and pass schema validation.
- The generated `IMPL.md` is derived from implementation and test evidence
  without access to requirements, contract, design, queue, or a prior record.
- Tests cover simple and multi-component projects and refusal to overwrite
  accepted intent.

**Context for future work:** This feature is part of the broader onboarding and integration effort. It enables Kvist to work with existing projects that were not designed with Kvist in mind. The reverse-discovery process is a one-time or infrequent operation, unlike the normal workflow which is driven by human-directed AI development.

---

## Agent & Model Capability Enhancements

### [COMPLETED] TODO AGN-01 — Dedicated Security Reviewer Role & Task Mapping

**Context:** Security auditing represents a highly specialized category of reviews. Running security audits under the same general `architect` model profile is sub-optimal. We need a dedicated `security_reviewer` role configured with special-purpose models and customized system prompts.

**Acceptance criteria:**

- Introduce `Role::SecurityReviewer` (command spelling: `security-reviewer`).
- Map `TaskKind::SecurityAudit` to `config.agent.security_reviewer` with fallback to `architect`.
- Ensure all signature validation, approval-policy checks, and serialization structures include the new profile's digests.
- Write unit/integration tests to verify correct role routing and approval-signature validation when the security reviewer is configured.

### [COMPLETED] TODO AGN-02 — Supervised Custom Prompt Execution with Loop Detection

**Context:** Local LLMs (including `llama-cli`) can hang or get stuck in infinite repetitive cycles. We need a supervised prompt execution command that monitors live output streams, implements configurable idle watchdogs, and analyzes streams for repetition loops.

**Acceptance criteria:**

- Implement `kvist prompt [PROMPT]` command supporting options: `--role`, `--idle-timeout`, `--detect-loops`, and `--max-restarts`.
- Accept bounded nonblank UTF-8 prompts from positional text, `--file`, redirected
  standard input, or an explicitly or interactively selected editor.
- Real-time streaming: Read stdout/stderr chunk-by-chunk and print to the console immediately.
- Idle watchdogs: Terminate and restart the subprocess if no new bytes are written within `idle_timeout` seconds (default 15 minutes).
- Loop detection: Analyze a rolling suffix buffer for consecutive matching cycle patterns (consecutive identical substrings of length 10-512 repeating >= 3 times, or consecutive line patterns). Terminate and restart the process upon detection.
- Max automatic restarts limits (default 3) to prevent infinite restart loops.

### [COMPLETED] TODO AGN-03 — Model Setup Wizard with Wrapper Script Support

**Context:** Users run local models with unique startup configurations, e.g. using helper scripts like `~/bin/llama-cli.sh`. The wizard must guide the user through setting up standard providers (llama-cli, llama-server, Ollama, Copilot, Gemini) and support custom shell wrappers.

**Acceptance criteria:**

- Add command `kvist agent setup` to launch an interactive CLI wizard.
- Wizard automatically queries Ollama's model tags or searches the shell `PATH` for standard binaries.
- Supports user-provided script wrappers (like `~/bin/llama-cli.sh`) by checking execute permissions and setting up custom templates.
- Runs a non-destructive verification test prompt and streams output to verify the connection.
- Prompts for the provider's model configuration name and exact command
  template before role assignment.
- Atomically creates or formatting-preservingly updates the generated profile
  in the user's preferred configuration file
  (`~/.config/kvist/config.toml` or `kvist.toml`) without discarding unrelated
  settings or models.

### [COMPLETED] TODO AGN-04 — Language-Specific BKM Prompt templates (Rust & Python)

**Context:** To enforce code quality and stylistic consistency, tasks must be generated and implemented using Best Known Methods (BKMs). Kvist should use modifiable prompt templates for Rust and Python unit tests, docstrings, and error patterns.

**Acceptance criteria:**

- Create a set of customizable templates under `.kvist/templates/` (or global user config directory) for developer tasks.
- For Rust: Enforce standard naming conventions, idiomatic `Result`/`Option` handling, explicit module visibilities, and doc comment tests.
- For Python: Enforce type annotations, PEP-8 formatting, Pydantic or standard data structures, and standard `unittest` / `pytest` suites.
- Read and inject the corresponding template during `task run` execution based on detected files or explicit configuration.

### TODO AGN-07 — Extract Linux-First Agent Runtime

**Context:** Prompt acquisition, provider command rendering, and process
supervision are useful outside Kvist, while Kvist-specific task state and
architectural context should not be embedded in a generic runtime. Nominal
Windows and macOS support consumes maintenance effort without native test
access and cannot currently support trustworthy execution guarantees.

**Acceptance criteria:**

- Create a standalone `agent-runtime` Rust library and `agent-run` CLI in its own
  component directory, consumed by Kvist as a path dependency.
- Move bounded prompt acquisition, shell-free command rendering, idle
  supervision, loop detection, and retry context into the reusable component.
- Require explicit acknowledgement for direct host execution and document that
  retry notices, backups, `fakeroot`, and source-control reset are not security
  boundaries.
- Keep Kvist role/model selection, task lifecycle, approval records, and
  component context construction in the root crate.
- Support Linux only; reject unsupported targets at build time, remove them
  from CI, and retain platform restoration as explicit future work.
- Specify and queue future snapshot/overlay workspaces, restricted identities,
  Bubblewrap/Landlock or stronger Linux isolation, brokered tools and provider
  networking, and independently tested macOS/Windows backends.

### TODO AGN-08 — Extract Reusable Provider Profile Setup

**Context:** The standalone runtime can execute an explicit command but provider
defaults, model verification, and formatting-preserving setup remain embedded
in Kvist. Those operations are provider concerns useful to other callers,
whereas Kvist roles and execution-policy approval are workflow concerns.

**Acceptance criteria:**

- Define a versioned, bounded standalone profile configuration containing
  generic names, provider kinds, and command templates without Kvist roles.
- Move provider-specific collection, custom-wrapper validation, endpoint
  probing, host-acknowledged verification, and profile persistence into
  reusable library APIs.
- Add `agent-run setup` and allow `agent-run run --profile NAME`
  to use the Linux user profile store.
- Make `kvist agent setup` reuse profile collection or load an existing
  standalone profile, then materialize the exact selected command into Kvist
  role configuration.
- Preserve existing Kvist configuration merge behavior and execution approval
  over exact command bytes; do not dynamically import mutable profile content
  during task execution.

### TODO AGN-09 — Layered Native Agent Runtime and Rig Transport Spike

**Context:** Inference backends such as llama-server and Ollama can propose
structured tool calls but do not provide a trusted coding-agent loop. Gemini,
Copilot, and similar CLIs contain useful but opaque loops. Kvist must preserve
its own policy, execution, and evidence boundaries while reusing provider
transport work where practical.

**Acceptance criteria:**

- Define standalone-owned canonical model, capability, tool-intent, result,
  runtime-event, and host-service contracts before selecting a framework.
- Distinguish native model, one-shot model, opaque external-agent, and plan-only
  backends, with advertised, tested, and policy-enabled capabilities.
- Keep authorization, tool brokering, sandbox execution, transactional
  promotion, and durable evidence outside every provider library.
- Implement Kvist task-policy, grant, approved-binding, execution-tier,
  promotion, and compliance-evidence adapters in the root `agn-authority`
  lifecycle chain; child loop tests use deterministic fake host services.
- Keep the exactly pinned `rig-core` 0.42.0 adapter optional behind
  `rig-transport`. Rust 1.94 is the supported MSRV because it is Rig's
  upstream-tested release toolchain; Rust 1.85 remains recorded as the
  historical failure caused by Rust 1.88 let-chain syntax.
- The first private local Ollama/llama-server transport and text-only
  `agent-run model` command are implemented; complete their independent
  security audit and compliance review before the native loop depends on them.
  Keep the seam replaceable and do not adopt `rig-agent`, Rig tools, MCP
  conversion, or Rig persistence as Kvist authority. Complete the optional
  Rig adapter's independent security and compliance reviews before promotion.
- Gate every future immutable Rig upgrade on locked Rust 1.94 and current
  stable builds, local Ollama/llama-server conformance,
  structured tool-intent conversion, cancellation, malformed-stream handling,
  TRACE leakage tests, endpoint/credential policy, dependency features,
  advisories, licenses, TLS, and objective dependency/binary-size thresholds.
- Record an explicit promotion decision. A failed Rig experiment or upgrade
  must leave the canonical interface and direct provider adapter usable.
- Build the bounded native loop only after the transport, broker, transactional
  workspace, and Linux execution boundaries pass independent review.
- Harden Gemini, Copilot, and other external agents as whole sandboxed
  processes; provider permission flags are defense in depth, not authorization.

Detailed rationale and task chains are in
`docs/agent-runtime/architecture.md`,
`docs/agent-runtime/rig-evaluation.md`, and
`src/agent_runtime/TODOS.yaml`.

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

- **Phase 1 — Core Engine:** `kvist init`, `kvist tree`,
  `kvist component new`, `kvist component validate`, bounded directory
  traversal, symlink safety checks, and read-only VCS tracking.
- **Phase 2 — Execution Boundary:** Independent TODO queue, project inspection, safe task selection, user-provided agent invocation, test-command verification, sandboxed execution, cryptographic approval binding, resource-bounded subprocesses, atomic task execution, security/compliance review, execution-boundary reconciliation, model-resolution coverage, template-contract validation, execution portability decisions.
- **Phase 3 — Independent Compliance Automation:** Component-intent and
  feasibility skills, queue-design skills, test-before-implementation skills,
  clean-slate documentation, independent review, source-blind comparison,
  durable human arbitration, component-intent interview mode, and reviewed
  queue generation.
- **Phase 4 — Deferred Visual and Editor Ecosystem:** Deferred until Phase 3 review workflow is independently reviewed and approved.
- **Onboarding and Integration:** `kvist convert`, `kvist import`, and
  `kvist reverse-discover` provide bounded repository onboarding and draft
  generation for the current artifact set. They do not constitute completed
  standards interchange; ReqIF, architecture-model, native-schema adapter, and
  generalized import/export support remain deferred in `docs/standards.md`.
- **Agent and Model Capability Enhancements:** `security_reviewer` agent profile (AGN-01, dedicated security auditor role profile, task routing mapping, and cryptographic HMAC approval validation), supervised custom prompt execution (AGN-02, real-time chunked streaming, idle watchdog timeout-breaking, and consecutive cycle/line/oscillation loop detection with automated restarts), interactive model setup wizard (AGN-03, terminal prober, custom script/wrapper helper, custom template TOML generation, and connection verification), and language-specific BKM prompt templates (AGN-04, project-level and global customizable prompt templates for Rust and Python tasks, dynamic language detection, and automatic self-healing default generation) are fully implemented and verified.
- **UX Improvements:** All 10 UX items (UX-02 through UX-10) have been completed.

For detailed strategy and current contracts, refer to
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md),
[`ARCHITECTURE.md`](ARCHITECTURE.md), and the root component documents under
`src/`.
