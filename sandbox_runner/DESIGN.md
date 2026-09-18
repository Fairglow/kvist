<!-- kvist-design-version: 1 -->

# Sandbox Runner Design

## Design overview

This runner enforces isolated Linux execution for approved Kvist task requests.
A core library strictly validates the version-one protocol types and bounds;
`src/enforcement.rs` executes requests within isolated Bubblewrap namespaces,
applies `prlimit` resource bounds, and guards network access. Missing
prerequisites or invalid requests fail closed.

## Internal structure

| Path                   | Responsibility                                                                                                                                                        |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/protocol.rs`      | Closed, `deny_unknown_fields` version-one request and probe types and bounds                                                                                          |
| `src/validation.rs`    | Independent size, structural, lexical, and mediated-acquisition semantic validation with actionable errors                                                            |
| `src/origin.rs`        | Strict URL parsing and a validated source-aware origin matcher for the network boundary                                                                               |
| `src/cache.rs`         | Descriptor-relative bounded construction and atomic no-replacement publication of immutable Cargo-home generations                                                    |
| `src/probe.rs`         | Verified capability probe testing kernel namespaces and Bubblewrap backend availability                                                                               |
| `src/enforcement.rs`   | Bubblewrap execution, namespace boundaries, `prlimit` resource caps, network guard proxy, and execution supervision                                                   |
| `src/lib.rs`           | Module surface and status constants                                                                                                                                   |
| `src/main.rs`          | Command boundary for probe and request protocol modes                                                                                                                 |
| `schema/*.json`        | Non-normative JSON Schema 2020-12 aids mirroring `src/protocol.rs`                                                                                                    |
| `tests/conformance.rs` | Accepted shape, canonical serialization, malformed/oversized/unknown, legacy rejection, acquisition source/argv/environment policy, probe, and executable enforcement |
| `tests/scaffold.rs`    | Structural and capability probe evidence                                                                                                                              |

The enforcement path composes the validated request into one explicit
Bubblewrap execution — resolved mounts, namespace boundaries, `prlimit`
resources, the source-aware network guard, and process supervision — without
importing engine types.

## Interactions and state

The runner receives one bounded JSON request on standard input, validates it
independently, and persists no state. It never trusts engine-side validation as
a substitute for its own. The enforcement path constructs one explicit
Bubblewrap execution from a validated request, supervises it, enforces
resources, cleans up the process tree, and returns bounded results.

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

`src/origin.rs` provides the source-aware origin policy used by the network
guard: canonical origins, production and test-loopback modes, and
reauthorization of an initial URL or redirect against supplied pinned
addresses. `src/cache.rs` accepts
retained descriptors for a trusted generation parent and isolated attempt
Cargo home, validates the exact manifest by no-follow traversal, copies and
rehashes to private mode-0700 staging, normalizes modes, fsyncs, and uses Linux
`RENAME_NOREPLACE` to publish one complete immutable generation. It never
offers mutable destination promotion. Network egress is strictly mediated by
the source-aware guard; namespace, mount, and process isolation are enforced
via Bubblewrap. ADR 0004 is authoritative for the Bubblewrap shape and
ADR 0005 for mediated acquisition.

## Failure and recovery

Every request either is rejected with a status-2 diagnostic or fails closed
with a status-3 enforcement-unavailable diagnostic; the probe likewise fails
closed. The runner creates no attempt, queue, approval, or evidence state and
therefore has no recovery transition. Before publication, immutable-generation
construction can remove its exclusively named private staging tree only through
retained descriptors; a cleanup failure is returned explicitly rather than
concealed. Once `RENAME_NOREPLACE` succeeds, the complete generation is
published or the operation fails without replacing a prior generation.
Project cache-reference updates remain a separate later design.

## Security and resource design

No shell is used; the enforcement path spawns exactly one Bubblewrap child per
approved request. Untrusted input
is size-bounded before parsing and every value is length-bounded. The
generation primitive uses `nix` descriptor-relative no-follow opens after its
child has exited, rejects symlink roots, root containment, symlinks, hardlinks,
special files, unknown manifest entries, and checked-arithmetic bound breaches.
It copies from opened source descriptors, rehashes staged bytes, and returns a
bounded immutable-generation identity and path. The enforcement path verifies
its installation and Bubblewrap identity, resolves mounts, constructs a
network-denied or source-limited namespace, pins trusted addresses and
reauthorizes redirects, and enforces process and resource limits under
ADR 0004.

## Verification strategy

Current tests assert strict shape and legacy rejection, exact Cargo argv,
environment, toolchain/grant, cache, scratch, and lockfile topology, source
identities and substitutions, production-origin rejection, schema shape, and
fail-closed behavior. Generation tests cover root replacement after open,
symlink/hardlink/special-file rejection, count/file/aggregate bounds,
extra-entry rejection, overlap, zero bounds, collision, staged recheck,
cleanup, and bounded generation identity. Conformance tests additionally cover
executable enforcement: aliases and links, real mount grants, hidden workflow
state, Bubblewrap identity, namespace setup, resource exhaustion, process
cleanup, redirects and address pinning, and host fallback refusal.
