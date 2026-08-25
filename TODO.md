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

## Summary of Completed Work

All items in the following sections have been completed and integrated into the codebase:

- **Phase 1 — Core Engine:** `kvist init`, `kvist tree`, `kvist spec new`, `kvist spec validate`, bounded directory traversal, symlink safety checks, read-only VCS tracking.
- **Phase 2 — Execution Boundary:** Independent TODO queue, project inspection, safe task selection, user-provided agent invocation, test-command verification, sandboxed execution, cryptographic approval binding, resource-bounded subprocesses, atomic task execution, security/compliance review, execution-boundary reconciliation, model-resolution coverage, template-contract validation, execution portability decisions.
- **Phase 3 — Independent Compliance Automation:** Component-design and feasibility skills, queue-design skills, implementation and test skills, clean-slate documentation skill, independent review skills, clean-slate and source-blind pipelines, durable human arbitration, specification interview mode, reviewed queue generation.
- **Phase 4 — Deferred Visual and Editor Ecosystem:** Deferred until Phase 3 review workflow is independently reviewed and approved.
- **UX Improvements:** All 10 UX items (UX-02 through UX-10) have been completed.

For detailed documentation of the completed work, refer to [`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md).

