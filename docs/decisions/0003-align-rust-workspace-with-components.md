# ADR 0003: Align the Rust workspace with component ownership

## Status

Accepted

## Context

The current root component lives at `src/`, while its Cargo manifest, lockfile,
integration tests, and repository-level build configuration live above that
directory. A component-only execution mount therefore cannot both preserve
Kvist's context boundary and provide the files needed to author and verify a
root change. Treating the entire repository as the component would also expose
global intent and unrelated project state as ordinary implementation files.

Kvist is pre-release and has no layout-compatibility obligation. This is the
least costly point at which to make its own repository follow the component
model it expects from users.

## Decision

Move the Rust product workspace beneath one implementation root:

```text
engine/
  REQUIREMENTS.md
  CONTRACT.md
  DESIGN.md
  TODOS.yaml
  IMPL.md
  Cargo.toml
  Cargo.lock
  src/
  tests/
  agent_runtime/
  sandbox_runner/
```

Set `component_root = "engine"`. The `kvist.engine` root component owns the
workspace manifest, CLI and library source, root integration tests, and
component-local build configuration. `agent-runtime` and `sandbox-runner` are
complete child components with their own manifests, intent, queues, tests, and
implementation records. Project intent and governance remain at repository
root in `VISION.md`, `ARCHITECTURE.md`, `ROOT_CONTRACT.md`, ADRs, licensing,
and project-level documentation.

An authoring sandbox receives only the selected component's approved context
and write grants. A separate verification sandbox may receive read-only
provider source and workspace metadata needed by the compiler without adding
those files to the authoring agent's context or write authority.

The migration is one-way. Old component and workspace paths are not retained
as aliases.

## Alternatives considered

- **Keep `src/` as the root and grant discontiguous repository paths:** avoids
  a move, but permanently makes component ownership different from Rust
  workspace ownership and complicates every task policy.
- **Make the repository root the component:** makes Cargo convenient but
  collapses project intent, licensing, Git state, and implementation into one
  writable authority boundary.
- **Use multiple independent component roots:** represents peer packages well,
  but the current recursive model intentionally has one configured root and
  would need a broader product decision.

## Consequences

Cargo commands use `engine/Cargo.toml` or run from `engine/`. CI, documentation,
path dependencies, tests, and status fixtures must change together. The move
is substantial, but it makes root authoring, verification, child boundaries,
and future sandbox grants coherent instead of encoding permanent exceptions
for Kvist itself.
