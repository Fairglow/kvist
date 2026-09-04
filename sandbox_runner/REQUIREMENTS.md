<!-- kvist-requirements-version: 1 -->
# Sandbox Runner Requirements

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Purpose and scope

`sandbox-runner` is the independently installed Linux enforcement component
for approved Kvist task grants. This component revision is a
*protocol-and-acquisition-capable, OS-enforcement-unavailable* intermediate
state: it strictly parses and validates the redefined version-one probe and
request contracts, rejects every superseded ("legacy") shape, and implements
the mediated Cargo validation and immutable-generation primitives (exact Cargo
phase argv, environment, toolchain/grant binding, independently derived source
identities, transport-policy value types, and bounded descriptor-relative cache
generation construction). It does not yet perform
Bubblewrap-backed Linux isolation, network-namespace source transport, or
mount resolution. It MUST NOT claim to enforce a request or report a probe as
confirmed before the queued Bubblewrap implementation task is completed, and
every otherwise valid request MUST still fail closed.

## Stakeholders and concerns

- Kvist engine integrators need a separately packaged executable boundary that
  agrees with the engine on one bounded version-one wire contract.
- Security reviewers need fail-closed behavior until native enforcement exists.
- Operators need an honest distinction between a protocol-validating,
  unenforced intermediate runner and a production runner.
- Future implementers need the accepted ADR 0004 boundary retained without
  prematurely claiming enforcement behavior.

## Functional requirements

The protocol-validation boundary and the later production enforcement have
separate, explicit requirements so protocol capability cannot be confused with
enforcement readiness.

### SR-REQ-PROTOCOL-BOUNDARY

The component MUST build as a Rust 2024 package, expose one
`kvist-sandbox-runner` executable, and keep its source, schemas, and tests
adjacent to local intent. It MUST strictly parse and independently validate the
redefined bounded version-one `kvist-sandbox-request-v1` request: exact
protocol identifier and version, typed phase (`authoring`, `verification`, or
`dependency-acquisition`), bounded argument vector, canonical absolute working
directory, typed environment, network capability (`deny`, or `package-sources`
with typed `cargo-registry`/`cargo-git` sources during mediated acquisition),
resource limits (including an optional `max_cache_bytes`), approval-bound
identities, typed path grants, a `system` or `cargo` toolchain, the mediated
dependency-cache object with a distinct `lockfile` purpose, and scratch. Every
submitted path field and `argv[0]`
MUST be canonical (no duplicate `/` separators, no trailing `/` except root,
and no `.`/`..` components), and environment names MUST be portable and
length-bounded. It MUST reject unknown fields, unknown enumerants, malformed
digests, oversized input, a resource limit that is zero or exceeds its explicit
safe maximum (wall time, output, processes, files, file bytes, scratch bytes,
and cache bytes), a read-write grant to a read-only purpose or phase,
overlapping or traversal destinations, and every retired request shape with an
actionable, non-secret diagnostic. Cargo acquisition and verification MUST use
closed, phase-specific environment allowlists, exact Cargo argv, and exact
cache/scratch/lockfile grant topology. In either Cargo phase the runner MUST
require `Toolchain::Cargo`, `toolchain.cargo == argv[0]`,
`toolchain.identity == identities.toolchain`, and one matching read-only
toolchain grant. Because Bubblewrap enforcement is not yet integrated, it MUST
fail closed for an otherwise valid request and MUST NOT fall back to
unconstrained host execution or report a probe as confirmed.

### SR-REQ-ACQUISITION-POLICY

The component MUST implement host-independent mediated Cargo semantics so the
later Bubblewrap integration only wires enforcement. It MUST require exact
canonical crates.io origins, canonical HTTPS additional registries and
immutable HTTPS Git pins, reject impersonation, and independently recompute
each domain-separated source identity from its canonical fields. Configured
production source origins forbid credentials, query, fragment, percent aliases,
and loopback/private/link-local addresses; loopback HTTP exists only in an
explicit test-policy matcher and is never accepted in a production request.
It MUST reject duplicate source identities, registry names, and origin
overlap.

Acquisition MUST be exactly `<cargo> fetch`, without `--locked`, using a real
writable attempt-local `CARGO_HOME`, a distinct writable lockfile workspace,
separate target scratch, `HOME` beneath that scratch, an explicit single
canonical `PATH`, and `CARGO_NET_GIT_FETCH_WITH_CLI=false`. Its promotion data
MUST bind `Cargo.lock` before/after identities, the real Cargo-home source, and
the exact derived supported-source identities. Verification MUST be exactly
`<cargo> test --locked`, with denied network, `CARGO_NET_OFFLINE=true`, an
approved Cargo-home generation mounted read-only at `CARGO_HOME`, and separate
writable target scratch. Exact allowlists reject loader variables, proxies,
Cargo/Git configuration, credentials, and source rewrites.

The cache primitive MUST require opened trusted-generation-parent and isolated
attempt-root capabilities, reject symlink roots and equal/containing roots,
validate nonzero bounded limits, traverse and copy descriptor-relatively with
no-follow, reject links and special entries, stream/hash/recheck staged bytes,
normalize modes, fsync, and atomically publish one complete immutable
generation with no replacement. It MUST not claim generic mutable-destination
promotion. Actual transport, DNS/address pinning, redirects, namespace,
mount, and process enforcement, as well as final project-cache-generation
selection, remain deferred; an otherwise valid request still fails closed.

### SR-REQ-PRODUCTION-RUNNER

The later implementation MUST follow
[`../../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md`](../../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md):
Bubblewrap-backed Linux isolation, verified backend and kernel-capability
probing, phase-specific mount construction, explicit resource enforcement,
process-tree cleanup, and fail-closed backend verification, built on the
version-one parsing, validation, and acquisition semantics established by the
protocol and acquisition boundaries. It wires the source origin matcher and
cache promotion primitives to real network namespaces and resolved mounts
rather than reinventing them. This requirement is approved target intent, not a
claim about the current revision.

## Quality requirements and constraints

- Rust stable edition 2024 is required and unsafe Rust is forbidden.
- Executable support is Linux-only.
- The component MUST NOT import Kvist engine implementation types.
- Repository input and protocol input are untrusted and bounded before use.
- The dependency graph remains small: the runtime crates are the Rust standard
  library, `nix` for safe Linux descriptor-relative filesystem operations,
  `serde`/`serde_json` for parsing, and `sha2`/`hex` for identities. No shell,
  network service, daemon, credential, ambient home, or worktree runner
  installation is introduced by this revision.

## Acceptance and traceability

The local queue orders protocol tests, implementation, security audit, and
independent compliance review. Protocol-boundary tests verify strict accepted
shape, canonical serialization, malformed, oversized, unknown, and legacy
rejection, and fail-closed unavailability. Acquisition-boundary tests verify
the canonical crates.io, additional-registry, and immutable Git source policy,
duplicate-identity and overlapping-origin rejection, the acquisition argv and
phase-environment rules, the origin matcher, and the cache
immutable-generation primitive (root/file replacement, symlink, hardlink,
special-file, count, size, aggregate, checksum, extra-entry, overlap, zero
bounds, collision, tamper/recheck, cleanup, and bounded generation identity
cases). Production Bubblewrap and native network, namespace, mount, redirect,
and process evidence belongs to later tasks and MUST NOT be inferred from a
successful protocol-and-acquisition-validation build.
