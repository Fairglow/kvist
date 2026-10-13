# 0008: Support Top-Level Peer Components for Workspace Dogfooding

## Status

Accepted

## Context

ADR 0003 originally aligned the Kvist maerg by placing `sav` and
`galla` as nested sub-components inside `maerg/` with `component_root = "maerg"`.
Subsequently, the repository was refactored (as asserted in `layout.rs` and documented
in `ARCHITECTURE.md`) to a standard Cargo workspace layout:
- Root workspace manifest: `/Cargo.toml` with members `maerg`, `sav`, `galla`.
- Independent top-level directories: `maerg/`, `sav/`, `galla/`.

However, `kvist.toml` remained set to `component_root = "maerg"`, and Kvist's configuration
validator (`normalize_component_root`) strictly forbade relative `.` paths. As a consequence,
`kvist tree`, `kvist status`, `kvist doctor`, and `kvist task` were completely blind to
`sav/` and `galla/`, preventing Kvist from managing or dogfooding its own
multi-component codebase.

Furthermore, `sav/TODOS.yaml` and `galla/TODOS.yaml` still referenced
`../CONTRACT.md` as their parent contract from the superseded nested layout, which failed to
resolve at the top level where global intent is governed by `ROOT_CONTRACT.md`.

## Decision

1. **Permit `component_root = "."` in Configuration**:
   Update `normalize_component_root` in `maerg/src/config.rs` to accept `.` as the component root.
   When `component_root` is `.`, the project root acts as a workspace namespace container.

2. **Namespace Discovery Semantics**:
   When `component_root` is `.`, the root directory itself is not forced into the discovered
   component list unless it contains the required component artifacts. Instead, its immediate
   children (`maerg`, `sav`, `galla`) are discovered as top-level peer components.

3. **Parent Contract Resolution for Top-Level Peers**:
   Top-level peer components have no parent component within the component tree; their architectural
   rules are governed directly by global `ROOT_CONTRACT.md`. Their `TODOS.yaml` queues set
   `parent_contract: null`.

4. **Update `kvist.toml` and Component Queues**:
   - Set `component_root = "."` in `kvist.toml`.
   - Update `sav/TODOS.yaml` and `galla/TODOS.yaml` so `parent_contract` is `null`.
   - Update cross-component requirement references to point to `ROOT_CONTRACT.md` and local contracts.

## Consequences

- `kvist tree`, `kvist doctor .`, and `kvist status .` cleanly discover and report all three
  components (`maerg`, `sav`, `galla`) simultaneously.
- Task execution (`kvist task run`, `kvist task next`) can target any component in the workspace
  directly by its path (e.g. `kvist task next maerg`, `kvist task next sav`, `kvist task next galla`).
- Self-hosting and dogfooding are unlocked across the entire repository without altering the
  canonical Cargo workspace structure.
