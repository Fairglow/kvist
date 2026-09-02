<!-- kvist-contract-version: 1 -->
# Sandbox Runner Contract

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Boundary and ownership

- **Component ID:** `sandbox-runner`
- **Contract IDs:** `sandbox-runner.protocol/v1`,
  `sandbox-runner.cli/v1`
- **Owner:** standalone `sandbox-runner` crate
- **Consumers:** the Kvist engine and Linux operators

The component owns independent request validation and operating-system
enforcement. It does not authorize grants, transition tasks, promote output,
or mint Kvist evidence.

## Provided interfaces

The executable is named `kvist-sandbox-runner`. It accepts exactly one protocol
mode argument:

- `--kvist-sandbox-request-v1` reads one bounded JSON request from standard
  input, strictly parses and independently validates it, and either rejects it
  with an actionable non-secret diagnostic or — because Bubblewrap enforcement
  is not yet integrated — fails closed without executing it. It never falls
  back to host execution and never reports a request as enforced.
- `--kvist-sandbox-probe-v1` attempts to confirm production enforcement
  capabilities. In this revision it always fails closed with a diagnostic
  because no verified Bubblewrap backend and namespace enforcement are
  integrated; it emits no probe response object.

The accepted target interface is the redefined
`kvist-sandbox-probe-v1` and `kvist-sandbox-request-v1` protocol described by
ADR 0004. Request parsing and validation are implemented in this revision;
Bubblewrap-backed isolation and a confirmed probe response are not.

## Required interfaces

The production runner will require Linux namespace support and a verified
Bubblewrap executable. This revision requires only the Rust standard library
plus `serde`/`serde_json` for parsing and receives no engine authority.

## Data and schemas

The canonical request and probe shapes are version 1 and are defined by the
runner's own typed Rust parser in `src/protocol.rs`, which is authoritative.
The closed request models a disjoint execution phase (`authoring`,
`verification`, or `dependency-acquisition`), a bounded argument vector, a
canonical absolute working directory, portable environment entries, a network
profile (`deny`, or `package-sources` for mediated acquisition), bounded
resources (including an optional `max_cache_bytes`), approval-bound identities,
typed mount grants, a `system` or `cargo` toolchain, an optional mediated
dependency cache (`cargo_home`/`registry`/`git` with optional
`attempt`/`approved` endpoints and an optional `promotion` manifest), and an
optional scratch root. Typed package sources (`cargo-registry`, `cargo-git`)
are parsed strictly; semantic source and cache-promotion policy is enforced by
the later mediated-acquisition integration, not this revision.

Non-normative JSON Schema aids for machine consumers are published alongside
the crate:

- [`schema/kvist-sandbox-request-v1.schema.json`](schema/kvist-sandbox-request-v1.schema.json)
- [`schema/kvist-sandbox-probe-v1.schema.json`](schema/kvist-sandbox-probe-v1.schema.json)

Both use the JSON Schema 2020-12 dialect
(`https://json-schema.org/draft/2020-12/schema`). The schemas structurally
mirror `src/protocol.rs` and encode the feasible semantic constraints (network
mode/source emptiness); the Rust parser in `src/validation.rs` remains
authoritative for all semantic invariants (per-phase purpose/access rules,
canonical paths, and deferred source/promotion policy). Where the schema and
the Rust parser disagree, the Rust parser governs. The retired version-one
request shape is not recognized.

## Behavioral guarantees

The runner rejects unknown fields, unknown enumerants, wrong types, malformed
`sha256:` digests, oversized input, non-canonical paths (duplicate `/`
separators, a trailing `/` except root, or `.`/`..` components) in `argv[0]`
and every path field, oversized or malformed environment names, a resource
limit that is zero or exceeds its explicit safe maximum (wall time, output,
processes, files, file bytes, scratch bytes, and cache bytes), a read-write
grant to a read-only purpose or phase, traversal or overlapping destinations,
a declared scratch or mediated-cache endpoint (base scratch, acquisition
attempt/approved cache, and cache-promotion source/destination) that does not
correspond exactly to a matching grant with the correct purpose, access, and
identity, and every retired request shape. A fully valid request fails closed
because enforcement is unavailable; the runner never reports a request as
enforced and never executes it on the host.

## Errors and failure semantics

A rejected, unenforceable, or unknown invocation writes a concise non-secret
diagnostic to standard error, emits nothing on standard output, and returns a
nonzero exit status. A parse, validation, or usage failure returns status 2; an
otherwise valid but currently unenforceable request or probe returns status 3.
Unknown arguments do not activate fallback or host execution.

## Security and authority

The runner is a separate trust boundary. It validates all request input
independently, bounds untrusted input before parsing, and must be installed as
a regular non-link file outside the selected project and worktree. This
revision exercises no filesystem, process, namespace, network, or credential
authority beyond reading standard input and writing its diagnostic; it opens no
request path.

## Compatibility and verification

Kvist is pre-release and retains no legacy protocol compatibility. Current
tests cover strict accepted shape, canonical serialization, malformed,
oversized, unknown, and legacy rejection, and fail-closed unavailability.
Native Bubblewrap, path, resource, cleanup, and substitution enforcement tests
are mandatory before the production contract can be reported as implemented.
