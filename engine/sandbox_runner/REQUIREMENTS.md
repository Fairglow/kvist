<!-- kvist-requirements-version: 1 -->
# Sandbox Runner Requirements

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Purpose and scope

`sandbox-runner` is the independently installed Linux enforcement component
for approved Kvist task grants. This component revision is a
*protocol-capable, enforcement-unavailable* intermediate state: it strictly
parses and validates the redefined version-one probe and request contracts and
rejects every superseded ("legacy") shape, but it does not yet perform
Bubblewrap-backed Linux isolation. It MUST NOT claim to enforce a request or
report a probe as confirmed before the queued Bubblewrap implementation task is
completed.

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
dependency-cache object, and scratch. Every submitted path field and `argv[0]`
MUST be canonical (no duplicate `/` separators, no trailing `/` except root,
and no `.`/`..` components), and environment names MUST be portable and
length-bounded. It MUST reject unknown fields, unknown enumerants, malformed
digests, oversized input, a resource limit that is zero or exceeds its explicit
safe maximum (wall time, output, processes, files, file bytes, scratch bytes,
and cache bytes), a read-write grant to a read-only purpose or phase, a declared
scratch or mediated-cache endpoint (base scratch, acquisition attempt/approved
cache, and cache-promotion source/destination) that does not correspond exactly
to a matching grant with the correct purpose, access, and identity, overlapping
or traversal destinations, and every retired request shape with an actionable,
non-secret diagnostic. Semantic package-source and cache-promotion byte policy
is deferred to the production runner. Because Bubblewrap enforcement is
not yet integrated, it MUST fail closed for an otherwise valid request and MUST
NOT fall back to unconstrained host execution or report a probe as confirmed.

### SR-REQ-PRODUCTION-RUNNER

The later implementation MUST follow
[`../../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md`](../../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md):
Bubblewrap-backed Linux isolation, verified backend and kernel-capability
probing, phase-specific mount construction, explicit resource enforcement,
process-tree cleanup, and fail-closed backend verification, built on the
version-one parsing and validation established by the protocol boundary. This
requirement is approved target intent, not a claim about the current revision.

## Quality requirements and constraints

- Rust stable edition 2024 is required and unsafe Rust is forbidden.
- Executable support is Linux-only.
- The component MUST NOT import Kvist engine implementation types.
- Repository input and protocol input are untrusted and bounded before use.
- No shell, network service, daemon, credential, ambient home, or worktree
  runner installation is introduced by this revision.

## Acceptance and traceability

The local queue orders protocol tests, implementation, security audit, and
independent compliance review. Protocol-boundary tests verify strict accepted
shape, canonical serialization, malformed, oversized, unknown, and legacy
rejection, and fail-closed unavailability. Production Bubblewrap and native
resource evidence belongs to those later tasks and MUST NOT be inferred from a
successful protocol-validation build.
