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
| `src/validation.rs` | Independent size, structural, lexical, and mediated-acquisition semantic validation with actionable errors |
| `src/origin.rs` | Strict URL parsing and a validated source-aware origin matcher for the future network boundary |
| `src/cache.rs` | Descriptor-relative bounded construction and atomic no-replacement publication of immutable Cargo-home generations |
| `src/probe.rs` | Fail-closed capability probe that models the probe response but cannot confirm it |
| `src/lib.rs` | Module surface and honest status constants |
| `src/main.rs` | Fail-closed command boundary for the two protocol modes |
| `schema/*.json` | Non-normative JSON Schema 2020-12 aids mirroring `src/protocol.rs` |
| `tests/conformance.rs` | Accepted shape, canonical serialization, malformed/oversized/unknown, legacy rejection, acquisition source/argv/environment policy, and fail-closed executable |
| `tests/scaffold.rs` | Structural and fail-closed evidence |

The later implementation will add private Bubblewrap, mount, resource, process,
and cleanup modules — wiring the existing origin matcher and cache primitive to
real network namespaces and resolved mounts — without importing engine types.

## Interactions and state

The runner receives one bounded JSON request on standard input, validates it
independently, and persists no state. It never trusts engine-side validation as
a substitute for its own. The production design will construct one explicit
Bubblewrap execution from a validated request, supervise it, enforce resources,
clean up the process tree, and return bounded results.

## Algorithms and decisions

Input is bounded before parsing. The strict serde parser rejects unknown
fields, missing fields, wrong types, unknown enumerants, and every retired
shape. It validates canonical paths, bounded resources, and non-overlapping
destinations. A Cargo phase additionally requires `Toolchain::Cargo` to bind
the exact `argv[0]`, `identities.toolchain`, and one read-only toolchain grant.
It uses a closed phase-specific environment and exact grants: acquisition is
only `<cargo> fetch`, with real writable Cargo home, lockfile workspace, and
scratch; verification is only `<cargo> test --locked`, with denied network,
offline true, approved read-only Cargo home, scratch, and verification
workspace. The runner independently derives source identities from
length-delimited fields following a domain string; golden vectors cover each
field substitution.

`src/origin.rs` provides policy values for a future application-aware
transport: canonical origins, production and test-loopback modes, and
reauthorization of an initial URL or redirect against supplied pinned
addresses. It performs no resolution or transport. `src/cache.rs` accepts
retained descriptors for a trusted generation parent and isolated attempt
Cargo home, validates the exact manifest by no-follow traversal, copies and
rehashes to private mode-0700 staging, normalizes modes, fsyncs, and uses Linux
`RENAME_NOREPLACE` to publish one complete immutable generation. It never
offers mutable destination promotion. OS transport, DNS/address pinning,
redirect, namespace, mount, process, and final project-generation selection
remain deferred, so valid requests fail closed. ADR 0004 is authoritative for
the Bubblewrap shape and ADR 0005 for mediated acquisition.

## Failure and recovery

Every request either is rejected with a status-2 diagnostic or fails closed
with a status-3 enforcement-unavailable diagnostic; the probe likewise fails
closed. The runner creates no attempt, queue, approval, or evidence state and
therefore has no recovery transition. Before publication, immutable-generation
construction can remove its exclusively named private staging tree only through
retained descriptors; a cleanup failure is returned explicitly rather than
concealed. Once `RENAME_NOREPLACE` succeeds, the complete generation is
published or the operation fails without replacing a prior generation. Project
cache-reference updates and OS process recovery are separate later designs.

## Security and resource design

No shell is used and no child process is spawned for a request. Untrusted input
is size-bounded before parsing and every value is length-bounded. The
generation primitive uses `nix` descriptor-relative no-follow opens after its
child has exited, rejects symlink roots, root containment, symlinks, hardlinks,
special files, unknown manifest entries, and checked-arithmetic bound breaches.
It copies from opened source descriptors, rehashes staged bytes, and returns a
bounded immutable-generation identity and path. The later runner must verify
its installation and Bubblewrap identity, resolve mounts, construct a
network-denied or source-limited namespace, pin trusted addresses and
reauthorize redirects, and enforce process/resources under ADR 0004.

## Verification strategy

Current tests assert strict shape and legacy rejection, exact Cargo argv,
environment, toolchain/grant, cache, scratch, and lockfile topology, source
identities and substitutions, production-origin rejection, schema shape, and
fail-closed behavior. Generation tests cover root replacement after open,
symlink/hardlink/special-file rejection, count/file/aggregate bounds,
extra-entry rejection, overlap, zero bounds, collision, staged recheck,
cleanup, and bounded generation identity. Queued production tests must precede
Bubblewrap implementation and cover aliases and links, real mount grants,
hidden workflow state, Bubblewrap identity, namespace setup, resource
exhaustion, process cleanup, redirects/address pinning, and host fallback
refusal.
