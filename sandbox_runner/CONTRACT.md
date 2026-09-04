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
Bubblewrap executable. This revision requires the Rust standard library, `nix`
for safe descriptor-relative Linux filesystem operations, `serde`/`serde_json`
for parsing, and `sha2`/`hex` for identities, and receives no engine authority.

## Data and schemas

The canonical request and probe shapes are version 1 and are defined by the
runner's own typed Rust parser in `src/protocol.rs`, which is authoritative.
The closed request models a disjoint execution phase (`authoring`,
`verification`, or `dependency-acquisition`), a bounded argument vector, a
canonical absolute working directory, portable environment entries, a network
profile (`deny`, or `package-sources` for mediated acquisition), bounded
resources (including an optional `max_cache_bytes`), approval-bound identities,
typed mount grants, a `system` or `cargo` toolchain, an optional mediated
dependency cache (`cargo_home`/`registry`/`git`, a `writable` or `approved`
endpoint, an acquisition lockfile workspace, and result-bound promotion
metadata), and scratch. Cargo phases have exact allowlisted environments, exact
argv, and exact toolchain/grant/cache topology. The runner independently
derives source identities using documented domain-separated encodings and
rejects a mismatch. Actual network transport, DNS/address pinning, redirect,
namespace, mount, and process enforcement remain deferred.

Source identity is the SHA-256 label of a domain followed by each field's
eight-byte big-endian byte length and bytes, with no implicit normalization:
`kvist/cargo-registry/v1\0` then `name`, `index_origin`, `download_origin`;
or `kvist/cargo-git/v1\0` then `repository`, `revision`. Golden vectors are
`sha256:4d3fc763437bea9217f14b2bc9b5201dbf78ea13ac0ef390497cc54896ab9486`
for registry `private`, `https://registry.example.invalid/index/`,
`https://registry.example.invalid/crates/`; and
`sha256:3405bc5e24e0f91ff11d2271a69f9c9b7a0547adb6acace6d10638fee39d2fe6`
for Git `https://git.example.invalid/dependency.git`,
`0123456789abcdef0123456789abcdef01234567`. Tests substitute every identity
field and require a mismatch rejection.

Non-normative JSON Schema aids for machine consumers are published alongside
the crate:

- [`schema/kvist-sandbox-request-v1.schema.json`](schema/kvist-sandbox-request-v1.schema.json)
- [`schema/kvist-sandbox-probe-v1.schema.json`](schema/kvist-sandbox-probe-v1.schema.json)

Both use the JSON Schema 2020-12 dialect
(`https://json-schema.org/draft/2020-12/schema`). The schemas structurally
mirror `src/protocol.rs` and encode the feasible semantic constraints (network
mode/source emptiness); the Rust parser in `src/validation.rs` remains
authoritative for all semantic invariants (per-phase purpose/access rules,
canonical paths, source policy, acquisition argv and environment rules, and the
cache endpoint topology). Where the schema and the Rust parser disagree, the
Rust parser governs. The retired version-one request shape is not recognized.

## Behavioral guarantees

The runner rejects unknown fields, unknown enumerants, wrong types, malformed
`sha256:` digests, oversized input, non-canonical paths (duplicate `/`
separators, a trailing `/` except root, or `.`/`..` components) in `argv[0]`
and every path field, oversized or malformed environment names, a resource
limit that is zero or exceeds its explicit safe maximum (wall time, output,
processes, files, file bytes, scratch bytes, and cache bytes), a read-write
grant to a read-only purpose or phase, traversal or overlapping destinations,
invalid Cargo cache, scratch, or lockfile endpoint topology, and every retired
request shape. Acquisition additionally rejects anything but `<cargo> fetch`,
a mismatched Cargo toolchain/grant, wrong Cargo home, lockfile workspace,
scratch target/HOME, path, source identity, or result-bound promotion metadata.
Verification rejects anything but `<cargo> test --locked`, non-denied network,
non-offline environment, mutable Cargo home, or wrong cache/scratch/workspace
topology. The runner exposes validated transport-policy value types and
descriptor-relative immutable generation construction; neither performs
network or mount enforcement. A fully valid request still fails closed because
OS enforcement is unavailable; the runner never reports it as enforced or
executes it on the host.

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
revision exercises no process, namespace, or network authority and opens no
request grant path. Its filesystem authority is limited to reading standard
input, writing its diagnostic, and — only when the later integration invokes
the cache primitive with provider-opened root capabilities — no-follow bounded
immutable-generation construction. The current command boundary never invokes
it; selecting or changing a project's current cache generation is not
implemented.

## Compatibility and verification

Kvist is pre-release and retains no legacy protocol compatibility. Current
tests cover strict accepted shape, canonical serialization, malformed,
oversized, unknown, and legacy rejection, fail-closed unavailability, the
mediated-acquisition source, argv, and environment policy, the origin matcher,
and the cache inspection/promotion primitive.
Native Bubblewrap, path, resource, cleanup, and substitution enforcement tests
are mandatory before the production contract can be reported as implemented.
