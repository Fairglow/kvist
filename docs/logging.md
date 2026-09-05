# Kvist Logging and Observability Guide

## Intent and Overview

Kvist is a headless, architecture-driven tool where clarity, determinism, and inspectability are fundamental invariants. Logging in Kvist serves two critical audiences:

1. **Human Operators and Developers**: Need immediate, level-appropriate visibility into command execution, state transitions, sandbox isolation events, model provider requests, and test verification.
2. **Diagnostic Troubleshooting**: Allows pinpointing root causes during failures or test runs without polluting machine-readable stdout channels (such as `--json` outputs or CLI pipelines).

## Core Principles

- **Stream Isolation**: Standard output (`stdout`) is reserved strictly for command outputs (human-readable reports or deterministic JSON objects). All diagnostic and event logging (`tracing`) is emitted exclusively to standard error (`stderr`).
- **Level Appropriateness**: Higher log levels (e.g. `INFO`) answer *what* is happening at key lifecycle milestones. Lower log levels (`DEBUG`, `TRACE`) answer *why* decisions were made, with full parameter and configuration context.
- **Anti-Spamming & Deduplication**: Loops, polling intervals, stream chunk decoders, and recursive directory walks must never produce repetitive log entries at `INFO` or `WARN`. Aggregate summaries and thresholded logs are used instead.
- **Security & Privacy Boundary**: Secret tokens, API keys, private credentials, and sensitive prompt/response sentinels must never be emitted to global tracing subscribers.
- **Idempotent and Configurable**: Log initialization is safe across multithreaded test suites and configurable dynamically via standard environment variables.

---

## Log Level Disciplines

| Level | Purpose | When to Use | Information Included |
| :--- | :--- | :--- | :--- |
| **ERROR** (`tracing::error!`) | Hard failures, invariant violations, and aborted workflows. | Command execution failures, sandbox breach rejections, invalid states, verification errors, unrecoverable provider errors. | Failed operation, target resource (component path, task ID), specific error reason, actionable remediation advice. |
| **WARN** (`tracing::warn!`) | Recoverable anomalies, retries, fallbacks, or non-fatal discrepancies. | Process supervision restarts (e.g. timeout or loop detection restart), degraded fallback paths, recoverable validation warnings. | Reason for retry/fallback, attempt count vs. max attempts (`attempt N of M`), target resource, recovery action. |
| **INFO** (`tracing::info!`) | High-level milestones and workflow lifecycle events. | Starting/completing project initialization, discovering components (summary count), launching task in sandbox, test verification outcome, committing accepted changes. | High-level action name, primary resource identifier (task ID, component path), summary counts. |
| **DEBUG** (`tracing::debug!`) | Contextual diagnostics explaining decisions and parameters. | Resolved command arguments, sandbox mount lists, sandbox runner digests, VCS commit OIDs, HTTP status codes, profile resolution. | Parameter values, intermediate hashes/digests, timing durations, exit codes, resolved file paths. |
| **TRACE** (`tracing::trace!`) | Fine-grained internal mechanics for deep troubleshooting. | Step-by-step state machine ticks, single file hash computations, stream chunk sizes, directory walk visits. | Byte counts, individual file paths, regex matching details (excluding sensitive payload contents). |

---

## Environment Variables and Configuration

Kvist respects the following environment variables for filtering log output:

1. `KVIST_LOG`: Kvist-specific log filter (highest priority).
2. `RUST_LOG`: Standard Rust ecosystem filter (fallback if `KVIST_LOG` is unset).

If neither is set, Kvist defaults to `warn` to keep CLI execution quiet and clean during normal operation.

### Usage Examples

- **View high-level workflow events:**
  ```bash
  KVIST_LOG=info kvist task run my_task
  ```

- **Diagnose sandbox and runner execution details:**
  ```bash
  KVIST_LOG=debug kvist task run my_task
  ```

- **Scope debugging to specific modules:**
  ```bash
  KVIST_LOG=kvist::sandbox=debug,kvist::agent=trace kvist task run my_task
  ```

- **Debug tests with logging enabled:**
  ```bash
  KVIST_LOG=debug cargo test -- --nocapture
  ```

---

## Anti-Spamming Guidelines

To prevent overwhelming logs and retain high signal-to-noise ratio:

1. **Discovery Scans**:
   - `INFO`: Emit a single summary after completing discovery (e.g. `completed component discovery components_found=4 directories_scanned=28`).
   - `DEBUG`: Log each discovered component candidate.
   - `TRACE`: Log individual directories traversed.
2. **Supervision Retries**:
   - `WARN`: Log exactly once per retry attempt with clear context: `supervisor retrying after timeout (attempt 2/4)`.
   - Polling loops between ticks must not emit logs unless a state transition or threshold event occurs.
3. **Stream Decoding**:
   - Do not log each individual byte or chunk at `INFO` or `DEBUG`.
   - Stream start and finish are logged at `DEBUG`; chunk metrics belong at `TRACE`.
