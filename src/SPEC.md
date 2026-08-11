<!-- kvist-specification-version: 1 -->
# Kvist Root Component Specification

<details open>
<summary>Layer 1: Executive summary and public contract</summary>

## Purpose

Provide a local, headless Rust CLI that creates and inspects filesystem-native
Kvist projects. The root component makes durable project state, component
contracts, task queues, and compliance documentation visible to both humans and
automation.

## Public contract

The CLI provides `init`, `doctor`, `tree`, `spec new`, and `spec validate`.
`init` creates a validated root artifact set only in an uninitialized project;
it is a no-op for a current project and refuses every other existing state.
`doctor` reports project-state diagnostics without modifying files. `tree`
renders component layout, and specification commands create or validate
component contracts. `doctor` also reports whether every required root and
discovered component artifact is tracked by the selected Git or jj repository.
Commands use project-local configuration and produce deterministic,
non-interactive output.

</details>

<details>
<summary>Layer 2: Architectural guarantees</summary>

## Constraints and invariants

- Root configuration is limited to 64 KiB; specifications and inspected root
  text artifacts are limited to 1 MiB.
- Discovery uses configured, hard-bounded limits for depth, directories,
  components, directory entries, and relative-path bytes.
- Persistent writes use no-clobber same-directory temporary files. Root
  initialization is not a multi-file transaction and must remain recoverable
  through explicit user action.
- The root component performs no network or LLM invocation. It does not follow
  link-like paths and requires a trusted workspace before future task execution.
- VCS inspection never stages, commits, or snapshots a working copy. Git uses
  native tracking and ignore semantics; jj inspects only its saved snapshot.

</details>

<details>
<summary>Layer 3: Detailed strategy and algorithms</summary>

## Design and failure paths

Root inspection classifies a project as uninitialized, current, partial,
invalid, or unsupported-version. Initialization writes only uninitialized
projects, performs no writes for current projects, and refuses every other
existing state. `TODOS.yaml` ordering is a root-contract authoring rule; the
Phase 1 parser deliberately performs only structural validation. Discovery
reads a configured component root lexically, rejects resource-limit and
hierarchy violations, and reports missing or malformed adjacent artifacts.
Specification validation uses the versioned three-layer Markdown format and
returns line-aware diagnostics. Every operation returns contextual errors
rather than silently repairing, migrating, overwriting, or following links.
VCS auto-selection refuses a checkout containing both Git and jj until the
project owner selects one in `kvist.toml`.

</details>
