# Kvist Implementation Tracker

**Authority:** [`VISION.md`](VISION.md) -> [`ARCHITECTURE.md`](ARCHITECTURE.md)
-> component `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`
**Reviewed:** Implemented portions through 2026-08-25; partial and planned
review, onboarding, promotion, and compliance work is tracked below
**Reviewed by:** Stefan Kvist | 2026-08-25
**Build:** `cargo build --manifest-path engine/Cargo.toml --release` passes with
0 warnings

## Status conventions

- `TODO` — scoped and ready once its dependencies are done.
- `PARTIAL` — useful implementation exists, but listed acceptance criteria,
  review, promotion, or dependent queue chains remain incomplete.
- `IN PROGRESS` — actively being implemented.
- `BLOCKED` — needs an explicit product or security decision.

---

# Remaining Prioritized Backlog

## Dogfooding Execution Prerequisite

### TODO DOG-01 — Recoverable Supervised Bubblewrap Execution

**Context:** Kvist cannot currently use `task run` on its own root component.
No production runner implements the sandbox protocol, the migrated `engine/`
component now owns its Cargo manifest and integration tests, version-one
requests cannot express task-scoped workspace authority, and ambiguous
prepared attempts have no reconciliation command.

**Acceptance criteria:**

- Migrate the product workspace and root artifacts to `engine/`, with complete
  `agent_runtime/` and `sandbox_runner/` child components and no aliases for
  retired paths.
- Write the complete root and runner test plans before production changes.
- Add explicit digest-bound attempt recovery that never guesses about source
  effects or uses destructive VCS reset.
- Replace the unreleased protocol-version-one request and probe semantics with
  typed authoring, dependency, verification, context, toolchain, cache, and
  scratch grants; reject the legacy shape.
- Implement and independently install a single-file Bubblewrap runner outside
  the worktree. Bind both runner and Bubblewrap identity to approval.
- Give Cargo a distinct bounded acquisition phase with attempt-local writable
  dependency directories and source-aware network access. Support crates.io
  first; require exact policy for additional registries and Git sources.
- Keep authoring and verification network-denied. Use local agents initially;
  defer remote agents to host-owned model transport and typed tool brokering.
- Add supervised execution with an explicit task ID, no automatic retry, and
  separate human finalization.
- Give component, project, and task acceptance an explicit local-commit option
  that commits only the canonical accepted set. Preserve unrelated worktree and
  index state, disable hooks by default, honor signing policy, never push, and
  retain accepted-but-uncommitted recovery state.
- Implement exact Git commit automation first through an isolated index and
  expected-head update. Defer Jujutsu commit automation until its distinct
  working-copy and operation-log semantics are independently designed and
  promoted.
- Protect intent, queues, implementation records, approval material, canonical
  evidence, Git state, credentials, ambient home state, and child
  implementations from agent writes.
- Run native negative isolation, dependency, recovery, verification, and
  lifecycle tests against the real runner.
- Complete independent root and runner security audits and source-blind
  compliance reviews before resuming DOC-01.
- Keep unattended execution disabled until private workspaces,
  conflict-checked promotion, and crash-recoverable multi-file journaling are
  separately implemented and reviewed.

## Target Workflow: Review, Intent Proposals, and Contract Verification

The command names below are provisional design labels, not current CLI
interfaces. DOC-01 establishes shared evidence and acceptance machinery used
by DOC-02 and DOC-03.

### TODO DOC-01 — Advisory Review Evidence and Acceptance

**Context:** Controlled intent should receive bounded AI review before
acceptance when reasonable, without turning model findings into approval or
compliance decisions. Current `component accept` only structurally validates
and records revisions.

**Acceptance criteria:**

- Define a canonical review bundle containing exact
  `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md` digests plus a canonical
  projection containing task `id`, `title`, `description`, `context`, `purpose`,
  `expected_outcome`, `kind`, `depends_on`, and `requirements`, while excluding
  all `component` metadata and task lifecycle state.
- Add a separate review operation using the existing shell-free, bounded,
  approved or acknowledged agent execution path. Keep `component accept`
  deterministic, local, and free of agent or network calls.
- Limit review context to the local intent bundle, `ROOT_CONTRACT.md`, and the
  immediate parent `CONTRACT.md`; exclude peer and parent internals.
- Persist versioned Kvist-minted receipts and redacted bounded reports under
  component-local `.kvist/reviews/`, binding exact target digests and scope,
  reviewing and authoring context identity when known,
  provider/profile/model/tool identity, Kvist version, timestamp, report digest
  and path, and acknowledgement or exception. Do not retain raw transcripts or
  secrets.
- Keep review files VCS-trackable but outside the five-artifact set, component
  candidacy, staleness, task context, and compliance evidence.
- When project review is required, require either a current receipt plus
  explicit human acknowledgement or an exception with actor, timestamp, reason,
  and exact digests. Support a visible project `[review] required = false`
  opt-out that disables the per-bundle gate. Require an explicit exception when
  review is required but no agent is configured.
- Default review to required when configuration is absent; add the setting to
  the configuration schema, parser, generated template, and documentation, and
  emit `[review] required = true` in new projects.
- Ensure generated intent drafts are reviewed, generated evidence is exempt,
  arbitrary Markdown is not implicitly governed, and no finding or severity
  blocks acceptance or determines compliance.
- Add a separate project-level acceptance design and queue chain for
  `VISION.md`, `ARCHITECTURE.md`, `ROOT_CONTRACT.md`, ADRs, and referenced
  native schemas.
- Test exact-digest invalidation, canonical projection stability,
  acknowledgement, exception, opt-out, no-agent, redaction/bounds, strict
  context, model-output non-authority, and discovery/staleness exclusions.
- Complete independent security audit and compliance review before promotion.

### TODO DOC-02 — IMPL-Derived Intent Proposals and Advisory Comparison

**Context:** Independently observed behavior can suggest candidate intent, but
cannot recover stakeholder intent or create a normative consumer contract.

**Acceptance criteria:**

- Add a no-clobber `propose intent`/`derive draft` workflow that reads an
  independently generated `IMPL.md` and writes draft `REQUIREMENTS.md` and
  `DESIGN.md` only.
- Mark uncertainty and missing stakeholder decisions explicitly; do not
  generate normative `CONTRACT.md`.
- Keep generated drafts subject to human review, DOC-01 advisory review or
  exception, acknowledgement, and normal acceptance.
- Add a separate non-mutating advisory comparison between `IMPL.md` and
  existing intent. Do not use compliance verdict vocabulary or treat its
  output as compliance evidence.
- Preserve `reverse-discover` as a distinct source-based onboarding pipeline;
  any generated contract remains a non-normative draft.
- Test no-clobber behavior, uncertainty markers, contract non-generation,
  non-mutating comparison, strict input provenance, and review-gate handoff.
- Complete independent security audit and compliance review before promotion.

### TODO DOC-03 — Contract Clause Traceability Verification

**Depends on:** DOC-01 shared bounded evidence and receipt/report machinery.

**Context:** Contract implementation should be exercised by tests, while
recognizing that tests are evidence rather than proof.

**Acceptance criteria:**

- Define stable contract-clause locators using existing heading anchors
  initially. Introduce explicit IDs only through an explicit format/version
  decision.
- Define durable test-to-clause traceability and validate references without
  calling it code coverage.
- Use approved bounded test execution evidence to produce a contract-clause
  traceability report identifying clauses with passing evidence, failed
  evidence, or no linked tests.
- Keep the report advisory evidence and preserve independent compliance
  comparison of `CONTRACT.md`, `IMPL.md`, and test evidence.
- Test locator stability, malformed and duplicate references, approved
  execution binding, failed/uncovered reporting, deterministic output,
  redaction, and resource bounds.
- Complete independent security audit and compliance review before promotion.

## Onboarding and Integration

### [PARTIAL] TODO ONB-01 — Convert Existing Project to Kvist Component

**Context:** An existing Rust project needs draft Kvist intent and queue
artifacts without losing source, tests, benchmarks, or manifest data.

The conversion and preservation path exists. Detailed root queue chains and the
new advisory review, acknowledgement/exception, and associated bounds work
remain pending.

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
  current `kvist component accept .kvist` structurally validates and records
  current intent and immediate-parent contract revisions.
- Conversion never recognizes or migrates retired component artifacts and
  never overwrites existing metadata.
- Tests cover detection, preservation, bounded deterministic generation,
  validation, acceptance, stale drafts, and task-execution refusal before
  explicit revision acceptance.
- Complete the detailed root queue chain and DOC-01 review/bounds integration,
  then independently review the finished onboarding workflow.

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

### [PARTIAL] TODO ONB-04 — Reverse-Discovery: Generate Draft Intent from Existing Implementation

**Context:** Existing source can provide evidence for a draft component model,
but implementation cannot prove intended product outcomes by itself.

The source-analysis and draft-generation path exists. Detailed root queue
chains and the new advisory review, acknowledgement/exception, and bounded
evidence work remain pending.

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
- Complete the detailed root queue chain and DOC-01 review/bounds integration,
  then independently review the finished reverse-discovery workflow.

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

### [PARTIAL] TODO AGN-02 — Supervised Custom Prompt Execution with Loop Detection

**Context:** Local LLMs (including `llama-cli`) can hang or get stuck in infinite repetitive cycles. We need a supervised prompt execution command that monitors live output streams, implements configurable idle watchdogs, and analyzes streams for repetition loops.

The runtime behavior exists, but its shared prompt-input and model-setup
security audit and compliance review remain pending.

**Acceptance criteria:**

- Implement `kvist prompt [PROMPT]` command supporting options: `--role`, `--idle-timeout`, `--detect-loops`, and `--max-restarts`.
- Accept bounded nonblank UTF-8 prompts from positional text, `--file`, redirected
  standard input, or an explicitly or interactively selected editor.
- Real-time streaming: Read stdout/stderr chunk-by-chunk and print to the console immediately.
- Idle watchdogs: Terminate and restart the subprocess if no new bytes are written within `idle_timeout` seconds (default 15 minutes).
- Loop detection: Analyze a rolling suffix buffer for consecutive matching cycle patterns (consecutive identical substrings of length 10-512 repeating >= 3 times, or consecutive line patterns). Terminate and restart the process upon detection.
- Max automatic restarts limits (default 3) to prevent infinite restart loops.

### [PARTIAL] TODO AGN-03 — Model Setup Wizard with Wrapper Script Support

**Context:** Users run local models with unique startup configurations, e.g. using helper scripts like `~/bin/llama-cli.sh`. The wizard must guide the user through setting up standard providers (llama-cli, llama-server, Ollama, Copilot, Gemini) and support custom shell wrappers.

The setup behavior exists, but its shared prompt-input and model-setup security
audit and compliance review remain pending.

**Acceptance criteria:**

- Add command `kvist agent setup` to launch an interactive CLI wizard.
- Wizard automatically queries Ollama's model tags or searches the shell `PATH` for standard binaries.
- For providers with machine-readable model catalogs, present bounded numbered
  model choices and put free-form model entry behind a final custom choice.
  Use Ollama and llama-server HTTP catalogs and Copilot/Gemini ACP session
  model lists; file-backed and custom wrappers remain explicit manual paths.
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

### [PARTIAL] TODO AGN-07 — Extract Linux-First Agent Runtime

**Context:** Prompt acquisition, provider command rendering, and process
supervision are useful outside Kvist, while Kvist-specific task state and
architectural context should not be embedded in a generic runtime. Nominal
Windows and macOS support consumes maintenance effort without native test
access and cannot currently support trustworthy execution guarantees.

The standalone runtime extraction and Linux-first boundaries exist. Review and
the deferred isolation, brokering, and platform-restoration queue work remain.

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

### [PARTIAL] TODO AGN-08 — Extract Reusable Provider Profile Setup

**Context:** The standalone runtime initially executed only an explicit command
while provider defaults, model verification, and formatting-preserving setup
were embedded in Kvist. Those operations are provider concerns useful to other
callers, whereas Kvist roles and execution-policy approval are workflow
concerns.

Reusable profile setup and Kvist integration exist. Remaining promotion work
includes independent review and closure of deferred provider/bounds cases.

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

### [PARTIAL] TODO AGN-09 — Layered Native Agent Runtime and Rig Transport Spike

**Context:** Inference backends such as llama-server and Ollama can propose
structured tool calls but do not provide a trusted coding-agent loop. Gemini,
Copilot, and similar CLIs contain useful but opaque loops. Kvist must preserve
its own policy, execution, and evidence boundaries while reusing provider
transport work where practical.

Canonical transport seams, direct local transports, the text-only model
command, and optional Rig spike exist. Independent review, promotion decisions,
native-loop prerequisites, and deferred hardening remain incomplete.

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
`agent_runtime/TODOS.yaml`.

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

## Logging, Diagnostics & Observability Policy

### [PARTIAL] TODO OBS-01 — Structured Diagnostic Logging & Observability Standard Across All Subsystems

**Context:** Kvist needs a uniform logging discipline where events are classified consistently across all subsystems (engine, agent_runtime, sandbox_runner) to provide enough actionable context without log spamming. Diagnostic logs must never corrupt stdout (reserved for commands/JSON).

**Acceptance criteria:**

- Integrate `tracing` and `tracing-subscriber` into `engine` and `agent-runtime`.
- Emit all diagnostic logging to `stderr` with level filtering (`KVIST_LOG`, `RUST_LOG`).
- Enforce log level semantics: `ERROR` for invariant violations, `WARN` for retries/degraded states, `INFO` for operator milestones, `DEBUG` for contextual parameters, and `TRACE` for fine-grained internal steps.
- Enforce anti-spamming: summarize directory discovery, debounce streaming chunks, rate-limit retries.
- Ensure test executions default to `DEBUG` log level (`init_test_logging`) so diagnostic insights are available during test failures.
- Document logging rules and standards in `docs/logging.md` and link from `docs/standards.md`.

### TODO OBS-02 — Sandbox Runner Wire-Protocol Structured Logging & Diagnostics

**Context:** The standalone Linux `kvist-sandbox-runner` executes in a distinct process boundary and must surface diagnostic insights consistently over `stderr` during Bubblewrap setup, mount validation, resource cap enforcement, and isolation breaches.

**Acceptance criteria:**

- Ensure `kvist-sandbox-runner` uses structured diagnostics adhering to the log level standards.
- Provide actionable diagnostic context on exit without leaking confidential data or breaking fail-closed invariants.
- Cover runner diagnostic outputs with integration tests.

---

## Implemented Work and Remaining Qualifications

The following summarizes implemented capabilities without claiming that every
phase, queue chain, review, or promotion gate is complete:

- **Phase 1 — Core Engine:** `kvist init`, `kvist tree`,
  `kvist component new`, `kvist component validate`, bounded directory
  traversal, symlink safety checks, and read-only VCS tracking.
- **Phase 2 — Execution Boundary:** Implemented work includes the TODO queue,
  project inspection, safe task selection, user-provided agent invocation,
  test-command verification, sandboxed execution, cryptographic approval
  binding, resource-bounded subprocesses, atomic task execution,
  execution-boundary reconciliation, model-resolution coverage,
  template-contract validation, and Linux portability decisions. Remaining
  child-runtime review, promotion, and deferred isolation work is captured by
  AGN-07 through AGN-09; the newly approved document-review evidence is DOC-01.
- **Phase 3 — Independent Compliance Workflow:** Manual procedures and
  implemented supporting primitives preserve clean-slate documentation,
  source-blind comparison, and durable human arbitration. Full workflow
  automation and review/promotion remain incomplete and must not be inferred
  from the existing compliance evidence.
- **Phase 4 — Deferred Visual and Editor Ecosystem:** Deferred until Phase 3 review workflow is independently reviewed and approved.
- **Onboarding and Integration:** `kvist convert`, `kvist import`, and
  `kvist reverse-discover` provide repository onboarding and draft generation
  for the current artifact set. ONB-01 and ONB-04 remain partial because their
  detailed root queue chains and review/bounds integration are pending. These
  commands do not constitute completed standards interchange; ReqIF,
  architecture-model, native-schema adapter, and generalized import/export
  support remain deferred in `docs/standards.md`.
- **Agent and Model Capability Enhancements:** The `security_reviewer` profile
  (AGN-01) and language-specific BKM prompt templates (AGN-04) are implemented
  and verified. Supervised custom prompt execution (AGN-02) and the interactive
  model setup wizard (AGN-03) are implemented but remain partial until their
  shared security audit and compliance review finish. AGN-07 through AGN-09
  describe the remaining runtime extraction, provider, and native-loop work.
- **UX Improvements:** All 10 UX items (UX-02 through UX-10) have been completed.

For detailed strategy and current contracts, refer to
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md),
[`ARCHITECTURE.md`](ARCHITECTURE.md), and the root component documents under
`engine/`.
