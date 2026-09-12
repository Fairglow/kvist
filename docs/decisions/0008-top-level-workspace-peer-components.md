# 0008: Support Top-Level Peer Components for Workspace Dogfooding

## Status

Accepted

## Context

ADR 0003 originally aligned the Kvist engine by placing `agent_runtime` and
`sandbox_runner` as nested sub-components inside `engine/` with `component_root = "engine"`.
Subsequently, the repository was refactored (as asserted in `layout.rs` and documented
in `ARCHITECTURE.md`) to a standard Cargo workspace layout:
- Root workspace manifest: `/Cargo.toml` with members `engine`, `agent_runtime`, `sandbox_runner`.
- Independent top-level directories: `engine/`, `agent_runtime/`, `sandbox_runner/`.

However, `kvist.toml` remained set to `component_root = "engine"`, and Kvist's configuration
validator (`normalize_component_root`) strictly forbade relative `.` paths. As a consequence,
`kvist tree`, `kvist status`, `kvist doctor`, and `kvist task` were completely blind to
`agent_runtime/` and `sandbox_runner/`, preventing Kvist from managing or dogfooding its own
multi-component codebase.

Furthermore, `agent_runtime/TODOS.yaml` and `sandbox_runner/TODOS.yaml` still referenced
`../CONTRACT.md` as their parent contract from the superseded nested layout, which failed to
resolve at the top level where global intent is governed by `ROOT_CONTRACT.md`.

## Decision

1. **Permit `component_root = "."` in Configuration**:
   Update `normalize_component_root` in `engine/src/config.rs` to accept `.` as the component root.
   When `component_root` is `.`, the project root acts as a workspace namespace container.

2. **Namespace Discovery Semantics**:
   When `component_root` is `.`, the root directory itself is not forced into the discovered
   component list unless it contains the required component artifacts. Instead, its immediate
   children (`engine`, `agent_runtime`, `sandbox_runner`) are discovered as top-level peer components.

3. **Parent Contract Resolution for Top-Level Peers**:
   Top-level peer components have no parent component within the component tree; their architectural
   rules are governed directly by global `ROOT_CONTRACT.md`. Their `TODOS.yaml` queues set
   `parent_contract: null`.

4. **Update `kvist.toml` and Component Queues**:
   - Set `component_root = "."` in `kvist.toml`.
   - Update `agent_runtime/TODOS.yaml` and `sandbox_runner/TODOS.yaml` so `parent_contract` is `null`.
   - Update cross-component requirement references to point to `ROOT_CONTRACT.md` and local contracts.

## Consequences

- `kvist tree`, `kvist doctor .`, and `kvist status .` cleanly discover and report all three
  components (`engine`, `agent_runtime`, `sandbox_runner`) simultaneously.
- Task execution (`kvist task run`, `kvist task next`) can target any component in the workspace
  directly by its path (e.g. `kvist task next engine`, `kvist task next agent_runtime`, `kvist task next sandbox_runner`).
- Self-hosting and dogfooding are unlocked across the entire repository without altering the
  canonical Cargo workspace structure.
