<!-- kvist-design-version: 1 -->
# Sandbox Runner Design

## Design overview

This revision is a protocol-capable, enforcement-unavailable boundary. A small
library exposes the closed version-one protocol types, an independent validator,
and a fail-closed probe; the executable dispatches the two protocol modes.
Bubblewrap production behavior remains excluded until its queued tests exist, so
a valid request is validated but never executed.

## Internal structure

| Path | Responsibility |
| --- | --- |
| `src/protocol.rs` | Closed, `deny_unknown_fields` version-one request and probe types and bounds |
| `src/validation.rs` | Independent size, structural, and lexical validation with actionable errors |
| `src/probe.rs` | Fail-closed capability probe that models the probe response but cannot confirm it |
| `src/lib.rs` | Module surface and honest status constants |
| `src/main.rs` | Fail-closed command boundary for the two protocol modes |
| `schema/*.json` | Non-normative JSON Schema 2020-12 aids mirroring `src/protocol.rs` |
| `tests/conformance.rs` | Accepted shape, canonical serialization, malformed/oversized/unknown, legacy rejection, fail-closed executable |
| `tests/scaffold.rs` | Structural and fail-closed evidence |

The later implementation will add private Bubblewrap, mount, resource, process,
and cleanup modules without importing engine types.

## Interactions and state

The runner receives one bounded JSON request on standard input, validates it
independently, and persists no state. It never trusts engine-side validation as
a substitute for its own. The production design will construct one explicit
Bubblewrap execution from a validated request, supervise it, enforce resources,
clean up the process tree, and return bounded results.

## Algorithms and decisions

Input is bounded before parsing. The strict serde parser rejects unknown
fields, missing fields, wrong types, unknown enumerants (including unknown
package-source and toolchain kinds), and every retired shape. Validation then
confirms the exact protocol identity and version, typed phase, bounded argv,
canonical absolute working directory and every path field (rejecting duplicate
`/` separators, a trailing `/` except root, and `.`/`..` components), portable
and length-bounded environment names, the network mode/source-emptiness and
phase coupling, `sha256:` digest formats, per-resource limits that are nonzero
and within explicit safe maxima (wall time, output, processes, files, file
bytes, scratch bytes, and cache bytes), per-phase grant purposes,
read-only/read-write consistency (a dependency cache is writable only during
mediated acquisition), typed toolchain (`system` root or `cargo` path), the
mediated dependency-cache structure and manifest bounds, the scratch and
mediated-cache endpoint-to-grant topology (each declared base scratch,
acquisition attempt/approved cache, and cache-promotion source/destination must
correspond exactly to a matching grant with the correct purpose, access, and
identity), and non-overlapping normalized destinations. Semantic source and
cache-promotion byte policy is deferred to the mediated-acquisition
integration. Because enforcement is unavailable, a valid request fails closed
rather than executing. ADR 0004 is authoritative for the Bubblewrap enforcement
shape.

## Failure and recovery

Every request either is rejected with a status-2 diagnostic or fails closed
with a status-3 enforcement-unavailable diagnostic; the probe likewise fails
closed. The runner creates no attempt, queue, approval, or evidence state and
therefore has no recovery transition. Interruption and process-tree recovery
behavior must be designed and tested with the production implementation.

## Security and resource design

No shell is used, no child process is spawned for a request, and no request
path is opened. Untrusted input is size-bounded before parsing and every value
is length-bounded. The later runner must verify its own installation and
Bubblewrap identity, validate every typed grant against the host filesystem
(resolving links and canonical roots), and enforce the namespace and resource
constraints accepted in ADR 0004.

## Verification strategy

Current tests assert the accepted strict shape, deterministic canonical
serialization, malformed, oversized, unknown, and legacy rejection, the
mediated cargo-acquisition shape (typed sources, dependency-cache grants, cargo
toolchain, and the full cache object), canonical-path and environment-name
rejection, resource limits at and beyond their explicit safe maxima, the
scratch and mediated-cache endpoint-to-grant topology (missing, mismatched, or
wrong-access correspondence rejected), schema-to-parser drift for enumerants
and required fields, and fail-closed executable behavior for both requests and
the probe. Queued production tests must precede the Bubblewrap implementation
and cover aliases and links, grant overlap against the real filesystem, hidden
workflow state, Bubblewrap identity, namespace setup, resource exhaustion,
process cleanup, and refusal to fall back to host execution.
