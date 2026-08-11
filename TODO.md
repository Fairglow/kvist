# Kvist Phase 1 Work Tracker

**Scope:** Phase 1 — Core CLI Engine (`kvist-cli`) from
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md).

**Current state:** Phase 1 is complete. The CLI contract, versioned root
artifacts, safe initialization, deterministic discovery and tree rendering,
and specification generation and validation are implemented and independently
reviewed.

## Tracking rules

- Use `TODO`, `IN PROGRESS`, `BLOCKED`, or `DONE` in each task heading. A task
  is `DONE` only after its acceptance criteria and listed verification have
  passed.
- Keep this file current in the same change as the implementation it tracks.
- Record unresolved product choices under **Decisions and blockers** rather
  than silently choosing externally visible behavior.
- Implement tasks in order unless their dependencies are explicitly satisfied.

## Exit criteria

Phase 1 is complete when a user can initialize a project safely with
`kvist init`, view its component tree deterministically with `kvist tree`, and
create and validate progressive-disclosure `SPEC.md` files from supported
templates. All commands must have actionable errors and automated coverage for
their filesystem behavior and malformed inputs.

## Tasks

### DONE P1-01 — Define core CLI contracts and module boundaries

**Depends on:** none  
**Acceptance criteria:**

- Define the public command surface and help text for `init`, `tree`, and the
  specification creation/validation workflow.
- Move command parsing and business logic out of `main.rs` into testable
  library modules; keep process exit handling at the CLI boundary.
- Define a domain error strategy that preserves error context without panics.
- Document supported platforms and the initial configuration-file location
  rules.

**Verification:** unit-test command parsing and error rendering; run formatter,
linter, and targeted tests.

### DONE P1-02 — Specify the root project artifact schemas and templates

**Depends on:** P1-01  
**Acceptance criteria:**

- Define versioned, human-editable initial contents for `kvist.toml` and
  `ROOT_CONTRACT.md`.
- Define the root component artifact set and safe initial templates, including
  `src/SPEC.md`, `src/TODOS.yaml`, and `src/DOCS.md`, consistent with the
  architectural specification.
- Document template invariants, required fields, defaults, and upgrade
  compatibility expectations.
- Keep license text and commercial terms out of generated files until they are
  explicitly approved for the repository.

**Verification:** test generated template content against the documented schema
and required invariants.

### DONE P1-03 — Implement safe, idempotent `kvist init`

**Depends on:** P1-01, P1-02  
**Acceptance criteria:**

- Create the root configuration, root contract, and root component artifacts in
  a user-selected project directory.
- Refuse to overwrite or merge existing Kvist artifacts without an explicit,
  documented user action; never destroy unrelated files.
- Validate target paths, propagate filesystem failures with actionable context,
  and write generated artifacts atomically.
- Make repeated initialization deterministic and clearly report whether the
  project was created, already initialized, or requires intervention.

**Verification:** integration tests for an empty directory, an already
initialized directory, conflicting files, unwritable paths, and nested target
paths.

### DONE P1-04 — Define the component discovery model

**Depends on:** P1-02  
**Acceptance criteria:**

- Model a component as a directory and its adjacent Kvist artifacts, including
  missing, malformed, and incomplete states.
- Establish traversal boundaries, symlink policy, ignored directories, maximum
  depth, and deterministic ordering.
- Detect invalid component layouts without treating arbitrary directories as
  valid components.
- Keep discovery independent from terminal rendering so it is directly
  testable.

**Verification:** unit tests using temporary directory fixtures for nested,
incomplete, malformed, cyclic/symlink (where supported), and permission-error
layouts.

### DONE P1-05 — Implement `kvist tree` and terminal rendering

**Depends on:** P1-01, P1-04  
**Acceptance criteria:**

- Render the discovered component hierarchy in stable lexical order with clear
  component status and useful diagnostics for invalid or incomplete artifacts.
- Support non-interactive output suitable for shells and CI; do not depend on
  terminal color, width, or Unicode support for correctness.
- Keep the renderer read-only and ensure it never mutates project state.
- Provide concise errors when the target is not a Kvist project or cannot be
  read.

**Verification:** snapshot or golden tests for deterministic output and
integration tests for valid, empty, malformed, and unreadable project layouts.

### DONE P1-06 — Define the layered `SPEC.md` format and validation model

**Depends on:** P1-02  
**Acceptance criteria:**

- Specify machine-checkable requirements for the three disclosure layers:
  executive summary/public contract, architectural guarantees, and detailed
  strategy/algorithms.
- Define how collapsible sections are represented in Markdown and which
  headings and content are mandatory.
- Return structured, location-aware validation diagnostics for missing layers,
  invalid ordering, empty required sections, and unsupported template versions.
- Preserve user-authored Markdown outside generated or validated structure.

**Verification:** parser tests covering valid specifications, each invalid
structure, UTF-8 input, and diagnostic locations.

### DONE P1-07 — Implement specification templates and generation workflow

**Depends on:** P1-01, P1-02, P1-06  
**Acceptance criteria:**

- Provide a CLI workflow that creates a new component `SPEC.md` from the
  approved template without overwriting existing specifications.
- Generate all mandatory progressive-disclosure sections with concise prompts
  for the required contract, guarantees, edge cases, and failure paths.
- Validate generated output before writing it atomically and report actionable
  remediation when validation fails.
- Ensure template output is deterministic and can be edited and revalidated by
  users.

**Verification:** integration tests for successful generation, existing-file
protection, invalid targets, and generated-document validation.

### DONE P1-08 — Perform Phase 1 integration, security, and compliance review

**Depends on:** P1-03, P1-05, P1-07  
**Acceptance criteria:**

- Exercise the documented end-to-end flow in a fresh temporary directory:
  initialize, create/validate a component specification, and render the tree.
- Review all filesystem writes, path handling, symlink handling, parsing
  limits, and error paths against the root contract and Phase 1 requirements.
- Ensure public CLI help, generated artifacts, and user-facing errors agree
  with implemented behavior.
- Produce the Phase 1 reverse-engineered documentation and use an independent
  review context to compare it with the specification; record discrepancies
  for explicit arbitration.

**Verification:** full project formatter, linter, test suite, and a clean
binary smoke test using only documented commands.

## Decisions and blockers

| Status | Item | Owner / resolution |
| --- | --- | --- |
| RESOLVED | Initial `kvist.toml` schema and configuration versioning policy | Use `schema_version = 1`, `component_root = "src"`, and opt-in `llm.provider = "none"`; incompatible changes need explicit migration. |
| RESOLVED | CLI argument parser and error-reporting crates | Use `clap` 4 with `derive` for typed parsing/help and `thiserror` 2 for domain errors; both are permissively licensed, mature, and widely maintained. |
| RESOLVED | Symlink policy for component traversal | Reject a symbolic-link component root and never follow symbolic links while discovering descendants; symbolic-link artifacts are invalid layouts. |
| RESOLVED | Name and syntax of the specification generation command | `kvist spec new <COMPONENT_DIR>` creates a specification and `kvist spec validate <SPEC_FILE>` validates one. |
