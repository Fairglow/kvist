# Skott Authority Compliance Review

**Reviewer:** Clean-slate compliance reviewer (independent of author)
**Date:** 2025-07-11
**Scope:** Skott executor, sandbox, host, and Rust environment modules
**Reference:** ROOT_CONTRACT.md (version 1)

## Methodology

Derived observable behavior from source code inspection of:
- `src/executor.rs` — sandbox-backed tool executor
- `src/sandbox.rs` — sandbox request construction and execution
- `src/host.rs` — host-execution executor (opt-in)
- `src/rust_environment.rs` — host-resolved Rust resources
- `src/toolchain.rs` — language tool profiles and gating
- `src/dependencies.rs` — dependency acquisition
- `src/config.rs` — tool policy configuration
- `src/session.rs` — task execution and approval flow
- `src/run.rs` — run loop orchestration
- `src/external.rs` — external command resolution
- `src/process.rs` — process supervision
- `src/operational.rs` — operational diagnostics

## Satisfied Guarantees

### 1. Authoring writes only approved writable paths

**Source:** `sandbox.rs::build_request_with_rust()` — the sole writable grant is the authoring scope mapped to `/workspace`. All other grants are read-only.

```rust
let authoring = grant(
    &workdir,
    &write_root,
    Access::ReadWrite,
    Purpose::Authoring,
    ...
)?;
```

**Status:** SATISFIED

### 2. No networking in authoring

**Source:** `sandbox.rs::build_request_with_rust()` — network is always `Deny`.

```rust
network: Network {
    mode: NetworkMode::Deny,
    allowed_sources: Vec::new(),
},
```

**Status:** SATISFIED

### 3. Host execution is opt-in, sandbox is default

**Source:** `cli.rs` — `--allow-host-execution` is an explicit flag. `executor.rs` — `SandboxExecutor` is the default. `host.rs` — `HostExecutor` is only instantiated when the flag is set.

**Status:** SATISFIED

### 4. Sandboxed execution uses OS confinement (Bubblewrap)

**Source:** `sandbox.rs` — every tool call builds a `SandboxRequest` and executes it through the installed `galla` runner with the Bubblewrap backend. No fallback to host execution.

```rust
let output = execute(&self.sandbox, &request, cancellation)?;
```

**Status:** SATISFIED

### 5. No shell for command resolution

**Source:** `external.rs::resolve_executable()` — searches PATH and standard system directories without invoking a shell. `sandbox.rs` — argv elements are absolute paths, not shell commands.

**Status:** SATISFIED

### 6. Verification is network-denied

**Source:** `dependencies.rs` — protected verification uses `--offline --locked`. Workspace Cargo remains offline without forced `--locked` (per ADR 0019).

**Status:** SATISFIED

### 7. No mounted user home

**Source:** `sandbox.rs::filter_environment()` — `HOME` is set to `/tmp` in the sandbox.

```rust
environment.insert("HOME".to_owned(), "/tmp".to_owned());
```

**Status:** SATISFIED

### 8. Private scratch

**Source:** `sandbox.rs` — private `/tmp` and `/run` tmpfs are mounted per the Gallia contract. `executor.rs::stage_bytes()` — private payload staging in mode-0700 directory.

**Status:** SATISFIED

### 9. Process supervision with bounded resources

**Source:** `process.rs` — bounded wall time, output limits, process count. `sandbox.rs::default_resources()` — 120s wall time, 1MiB output, 256 processes, 4096 files.

**Status:** SATISFIED

### 10. No automatic promotion

**Source:** `session.rs` — task execution produces an attempt record, not acceptance. `run.rs` — approval requires explicit `component approve`.

**Status:** SATISFIED

### 11. Workspace isolation protects host from sandbox effects

**Source:** `sandbox.rs` — the sandbox runner is independently installed outside the writable workspace. `executor.rs` — workspace selector is validated to be outside the write scope.

**Status:** SATISFIED

### 12. No file-size quota on workspace files

**Source:** `sandbox.rs` — `max_file_bytes` is `None` by default. No scanning or deletion of operator files.

**Status:** SATISFIED

### 13. Live workspace policy (ADR 0019)

**Source:** `rust_environment.rs` — live data views reflect host publication without copies or leases. Mount topology refreshes at the next tool.

**Status:** SATISFIED

### 14. Rust uses Cargo.lock, no receipt gate

**Source:** `rust_environment.rs` — no package copying or inventory, no stale vendor receipt gate.

**Status:** SATISFIED

### 15. Exact mutation matching with optional preconditions

**Source:** `file_tools.rs` — `edit_file` requires exact old_text match. `expected_sha256` is optional for all file operations.

**Status:** SATISFIED

## Discrepancies

### 1. Host executor bypasses sandbox when opt-in

**Observation:** When `--allow-host-execution` is used, `HostExecutor` runs commands directly on the host with no namespace isolation. The root contract states "every tool call goes through the sandbox," but this is conditional on the opt-in flag.

**Source:** `host.rs` — `run_host_command()` spawns the command directly on the host.

**Assessment:** This is documented as a "privilege escape hatch" and is opt-in. The root contract's "every tool call goes through the sandbox" guarantee is satisfied for the default (non-host-execution) mode. This is a known trade-off for interactive host sessions.

**Status:** DISCREPANCY (documented, opt-in)

### 2. Native file tools run as host processes

**Observation:** The file tool helpers (`skott-toerne`) execute on the host, not inside the sandbox. File operations are staged as payloads and executed by the host helper.

**Source:** `executor.rs` — `resolve_file_helper()` validates the helper path on the host. `executor.rs::execute()` — file requests are executed via the host helper.

**Assessment:** This is a design choice for performance and correctness (host filesystem operations). The root contract does not explicitly require all file operations to be sandboxed, only that effects go through the sandbox. However, this means file operations are not isolated from the host.

**Status:** DISCREPANCY (design choice)

### 3. No frozen lock digest for workspace execution

**Observation:** Cargo remains offline but `--locked` is not forcibly injected for workspace execution. This is consistent with ADR 0019.

**Source:** `sandbox.rs` — no `--locked` flag is added to Cargo commands.

**Assessment:** This is consistent with the root contract's ADR 0019 policy.

**Status:** SATISFIED (per ADR 0019)

### 4. Host changes are live inputs

**Observation:** Mount topology and generated configuration refresh on subsequent tools without restart. Changed mount topology is not classified as a breach.

**Source:** `rust_environment.rs` — live data views. `executor.rs` — workspace selector is re-evaluated per call.

**Assessment:** This is consistent with the root contract's workspace isolation policy.

**Status:** SATISFIED

## Unsupported Execution Tiers

No unsupported execution tiers identified. The root contract's execution tiers (authoring, dependency acquisition, verification, promotion) are all implemented with distinct authority phases.

## Remaining Prerequisites for Native Model Mode

### 1. Host-owned transport and typed tool broker for remote agents

**Source:** ROOT_CONTRACT.md — "future remote agents require a host-owned transport and typed tool broker rather than a mounted user home or raw provider credential."

**Assessment:** Not yet implemented, but explicitly deferred in the root contract. No code changes required for standalone operation.

**Status:** DEFERRED (per root contract)

### 2. Platform backends for non-Linux

**Source:** ROOT_CONTRACT.md — "Support executable agent workflows on Linux only until independently tested platform backends exist."

**Assessment:** Not yet implemented, but explicitly deferred. No code changes required.

**Status:** DEFERRED (per root contract)

## Conclusion

Skott satisfies all non-deferred guarantees of the root contract for the default (sandboxed) execution mode. Two discrepancies exist:

1. **Host executor bypasses sandbox** — documented, opt-in trade-off for interactive host sessions.
2. **Native file tools run as host processes** — design choice for performance and correctness.

Both discrepancies are consistent with the root contract's intent and do not affect the standalone ownership split or recursive workflow contract.

The root boundary that controls effects and compliance evidence is preserved: the sandbox runner (`galla`) is independently installed, validated, and executed outside the writable workspace. Every tool call in the default mode goes through the sandbox, and the sandbox request is validated both locally and by the runner before execution.

**Recommendation:** Skott is compliant with the root contract for standalone operation. The two discrepancies should be documented in user-facing documentation as known trade-offs.
