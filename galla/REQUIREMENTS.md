<!-- kvist-requirements-version: 1 -->

# Galla Requirements

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Purpose and scope

`galla` is the independently installed Linux enforcement component
for approved Kvist task grants. This component strictly parses and validates
the redefined version-one probe and request contracts, rejects every
superseded ("legacy") shape, implements native Cargo data-cache validation and
separate explicit export-generation primitives (exact Cargo phase argv,
environment,
toolchain/grant binding, independently derived source identities,
transport-policy value types, and bounded descriptor-relative cache
generation construction), and enforces approved requests through
Bubblewrap-backed Linux isolation: user, IPC, PID, UTS, cgroup, and network
namespace boundaries, `prlimit` resource caps, source-aware network egress
mediation, and process supervision. It never falls back to unconstrained host
execution and never reports an unconstrained request as enforced; whenever
verified enforcement prerequisites are unavailable, every otherwise valid
request MUST fail closed.

## Stakeholders and concerns

- Kvist maerg integrators need a separately packaged executable boundary that
  agrees with the maerg on one bounded version-one wire contract.
- Security reviewers need fail-closed behavior whenever enforcement
  prerequisites are unavailable.
- Operators need an honest distinction between a verified enforcement
  capability and a failed probe.
- Implementers need the accepted ADR 0004 boundary retained, with every
  enforcement claim backed by the verified probe.

## Functional requirements

### SR-REQ-NATIVE-CARGO-CACHE

Protected Cargo verification MUST resolve native registry index, archives,
expanded sources and Git data from caller-approved persistent read-only roots,
with denied networking and private writable Cargo administrative state.
Acquisition MUST use the same approved data roots read-write, persist acquired
data directly, and let Cargo acquire only missing locked dependencies. Neither
phase MUST mount a whole operator home, credentials or ambient Cargo/Git config,
copy native data, construct per-file manifests, fsync a data inventory, or invent
a publication destination. Native endpoints MAY be absent for a local graph or
an unavailable cache; this MUST NOT silently select export/vendor mode.
Verification MUST reject writable native endpoints; acquisition MUST reject
read-only native endpoints. Closed phase environments, argv, exact endpoint
identities, disjoint grant destinations, no-follow source pinning and fixed
purpose counts remain REQUIRED. Native Cargo MUST NOT be an authoring mode.
Present native data origins MUST be ordinary directories.
Private administrative directories and sandbox-only links MUST be created only
inside approved scratch after data binds, without replacing caller-owned
conflicting nodes. Conflicts MUST fail clearly before payload execution.
Tests MUST cover accepted native phases, absent endpoints, mutations of
authority/environment/argv, mutually exclusive source modes, no promotion,
and real offline Cargo resolution without data-cache writes.

This human-approved native-cache increment has an explicit local advisory
exception while receipt enforcement is unavailable. It records no acceptance,
receipt, implementer-derived IMPL, security certification or compliance result.
Independent security and compliance tasks remain pending.

### SR-REQ-NATIVE-CARGO-ADMISSION

Native source-only admission MUST be available as one exact verification
operation: `<cargo> metadata --offline --locked --format-version 1 --all-features`.
It MUST reuse native verification's read-only workspace/data grants, exact
toolchain and endpoint identities, private writable scratch administration,
closed environment and denied network. It MUST NOT execute build scripts,
mount operator home/config/credentials, create a source inventory, or acquire
dependencies. Omitted endpoints remain valid for local graphs, not a fallback.
This operation MUST NOT be accepted for portable exports, authoring or
acquisition, and MUST NOT authorize arbitrary Cargo verbs or additional flags.
Existing test and fetch behavior MUST remain unchanged.
Tests MUST reject command/flag/topology mutations and exercise actual offline
Cargo metadata with read-only cached data and a build script that cannot run.

This human-approved admission increment has an explicit local advisory
exception while receipt enforcement is unavailable. Independent security,
source observation and source-blind comparison remain parent-arranged; the
implementer records no IMPL, acceptance or compliance certification.

### SR-REQ-NATIVE-REGISTRY-ALIASES

Native Cargo MUST support explicitly approved named Git-index and sparse registry aliases
without exposing operator configuration. Optional aliases MUST be bounded to
64 entries, have unique names of 1–64 ASCII letters/digits/`-`/`_` bytes
(Cargo rejects dotted registry names), and canonical
production HTTPS index origins, without credentials, queries,
fragments, percent aliases or private/loopback origins. The built-in
`crates-io` name MUST NOT be rewritten.
Each alias MUST declare `git` or `sparse` index protocol, defaulting to sparse
when omitted. Sparse origins MUST end in `/`; Git origins need not. Generated
Git index settings MUST retain the exact HTTPS origin, while sparse settings
add only Cargo's `sparse+` prefix. Protocol MUST NOT grant network authority or
change the exact name/origin acquisition comparison. Caller inference from
locked source prefixes MUST fail explicitly on ambiguity, never silently
reinterpret a Git source as sparse.
Native acquisition aliases MUST exactly match every additional approved
CargoRegistry source's name and index origin, with no extra or missing aliases.
Native verification remains network-denied and MUST NOT infer networking
authority from an alias. The backend MUST generate only deterministic bounded
`[registries.<name>].index` settings in private Cargo administration, with no
source replacement, credential, download, loader or ambient config settings.
Configuration MUST be namespace-local read-only input; no host config/data
copy or grant is added. Existing argv/environment/data topology stays fixed.
Tests MUST cover malformed/forged/credential-bearing and unbound acquisition
aliases, unchanged alias-free requests, and real offline named-registry
metadata that fails without the declared alias, and Git index
validation/materialization that preserves the original source URI. A focused
real metadata trial MUST resolve a named Git-index alternate registry using
Cargo-populated native index/archive/expanded data, generated private config,
read-only native mounts and denied network, without a fabricated checksum or
source inventory.

This human-approved increment has an explicit scoped advisory exception while
receipts are unavailable. Independent security/observation/comparison remain
parent-arranged; no implementer IMPL, acceptance or certification is recorded.

### SR-REQ-BOUNDED-TRANSPORT

The package-source proxy MUST relay authorized CONNECT streams concurrently in
both directions, including client-first TLS bytes and bytes coalesced with the
CONNECT header. An idle or stalled connection MUST NOT serialize other clients.
Connection admission, headers, buffering, upstream establishment, and transport
inactivity MUST be bounded. Slow partial headers MUST have an absolute deadline.
Half-close MUST preserve the opposite direction until its EOF or deadline.
Cancellation MUST stop admission, close owned sockets, and finish transport
workers within their establishment deadline rather than leave detached relays.
Malformed, unauthorized, oversized, timed-out, overloaded, and failed-upstream
requests MUST have explicit failure handling without terminating the listener.
Tests MUST exercise real loopback sockets without external service dependencies.

This human-approved repair has an explicit advisory exception for the scoped
local intent/task additions while review-receipt enforcement is unavailable.
It records neither component acceptance nor a fabricated receipt. Independent
security and compliance review remain required; the implementer does not certify
either. Cache-backed offline provisioning and missing-only acquisition are
approved separately; their request topology and publication authority are not
part of this transport increment.

### SR-REQ-ONE-WAY-ISOLATION

Portable-export promotion MUST NOT infer a host destination from a mutable
source pathname. Without a provider-owned publication capability, report
incomplete export promotion and retain actual fetched output; never follow a
sibling link or replay fetch. Native acquisition has no promotion stage and
writes directly to its approved persistent data roots.

ADR 0019 replaces categorical administrative-directory exclusion and mandatory
per-file caps for workspace execution. Standard system trees, including sbin,
MUST be read-only and usable without host modification. Namespace/capability,
network and explicit grant enforcement remain authoritative.
`Resources.max_file_bytes` MUST be optional; absence means no payload file-size
quota. An explicit cap MUST apply to payload writes, never host files merely
present in writable grants. Enforcement MUST NOT scan/delete operator files or
classify legitimate host edits as sandbox abuse. Host edits to read-only mounted
material MUST be visible. Tests MUST cover uncapped large files, survival of
pre-existing/operator-created files under explicit caps, live read-only changes
and denied writes outside grants.
This human-requested bundle has an explicit local advisory exception while
receipt enforcement is unavailable.

Source-origin protection MUST remain independent of content stability. Open
ordinary grant nodes through no-follow, descriptor-relative traversal and keep
them alive until descriptor-backed binds are consumed. Never pass host directory
capabilities to the payload. This per-call origin pin prevents sandbox-controlled
symlink races from adding host mounts; it does not freeze contents or require
old fingerprints. Native probes/tests MUST qualify descriptor-backed binds and
descriptor closure. Existing workspace vendor nodes MUST be readable but
read-only through an internal overlay even when Rust integration is unavailable.
Unusable/nonmaterialized vendor paths MUST not globally block independent tools.
Writable grants MUST NOT include the running runner inode, its installed
replacement, backend, process-limit launcher or their resolved transitive
loader/dependency closure. Direct writable file grants MUST also be checked
by their pinned device/inode identity, not just pathname containment.
Writable origins MUST NOT contain selector slots along those bounded known
code paths, including slots reached through canonical parent directories
and symlink-target ancestors. An outside-named lookup and outside final
library MUST NOT hide an intervening writable selector parent.
A legitimately unlinked/replaced running image MUST remain usable through
its kernel executable reference, without requiring its old pathname to exist.
Dependency discovery MUST be bounded, clear host-launcher environments and
keep protection descriptors close-on-exec. Payload edits to host enforcers could
widen host authority. Report the conflicting origin and a narrower scope or
outside-scope installation, not a file-content freeze or host-edit violation.

The following requirements define mandatory behavior of the enforcement component.

### SR-REQ-NO-NESTING

Every sandbox MUST contain an immutable namespace-local marker. Request and
probe invocations inside that sandbox MUST fail promptly with status 3, empty
stdout and a diagnostic explaining that another sandbox is unnecessary.
Detection MUST survive environment clearing and precede request stdin reads.
Grants MUST NOT cover the reserved marker. This is a usability guard, not
proof of confinement or authorization to bypass a consumer's sandbox policy.

The following stable requirement identifiers define the mandatory behavior of
the enforcement component.

### SR-REQ-BIN-ONLY-SYSTEM

Standard System trees, including `/sbin`, `/usr/sbin` and `/usr/local/sbin`,
MUST remain readable/executable read-only, including merged-directory aliases.
Host content, permissions and links MUST remain unchanged. Executable names
MUST NOT substitute for namespace, capability, network or filesystem authority.
Authoring PATH MAY select canonical sandbox directories, including writable
workspace scripts; it does not authorize host paths or effects. Closed Cargo
verification/acquisition PATH rules remain separate.
Explicit authoring environment values such as `LD_PRELOAD`, language paths and
Git configuration paths MAY affect the isolated payload. They MUST NOT be
inherited by the host runner/process-limit/backend launchers. Portable names,
NUL/encoding bounds and fixed Cargo-phase environments remain enforced.

The protocol-validation boundary and production enforcement have separate,
explicit requirements so protocol capability cannot be confused with
enforcement readiness.

### SR-REQ-PROTOCOL-BOUNDARY

The component MUST build as a Rust 2024 package, expose one
`galla` executable, and keep its source, schemas, and tests
adjacent to local intent. It MUST strictly parse and independently validate the
redefined bounded version-one `kvist-sandbox-request-v1` request: exact
protocol identifier and version, typed phase (`authoring`, `verification`, or
`dependency-acquisition`), bounded argument vector, canonical absolute working
directory, typed environment, network capability (`deny`, or `package-sources`
with typed `cargo-registry`/`cargo-git` sources during mediated acquisition),
resource limits (including an optional `max_cache_bytes`), approval-bound
identities, typed path grants, a `system` or `cargo` toolchain, the explicit
native-data or portable-export dependency-cache object with a distinct
`lockfile` purpose, and scratch. Every
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
toolchain grant. It MUST NOT fall back to unconstrained host execution, MUST
NOT report an unconstrained request as enforced, and MUST fail closed whenever
verified enforcement prerequisites are unavailable.

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

In both Cargo source modes, acquisition MUST be exactly
`<cargo> fetch --locked` and test verification MUST be exactly
`<cargo> test --offline --locked`. Native source-only verification MAY instead
use the exact metadata operation in SR-REQ-NATIVE-CARGO-ADMISSION. Native mode follows
SR-REQ-NATIVE-CARGO-CACHE: data roots are read-only during verification,
while Cargo administration remains private and writable under scratch.

In the deliberately selected portable-export/whole-home mode, acquisition
MUST use a real writable attempt-local `CARGO_HOME`, a distinct writable
lockfile workspace,
separate target scratch, `HOME` beneath that scratch, an explicit single
canonical `PATH`, and `CARGO_NET_GIT_FETCH_WITH_CLI=false`. Its promotion data
MUST bind `Cargo.lock` before/after identities, the real Cargo-home source, and
the exact derived supported-source identities. Only export verification MUST
mount the approved export Cargo-home generation read-only at `CARGO_HOME`,
alongside its read-only vendor registry and resolver configuration, denied
network, `CARGO_NET_OFFLINE=true`, and separate writable target scratch.
Exact phase allowlists in both modes reject loader variables, proxies,
Cargo/Git configuration, credentials, and source rewrites.

The separate explicit export-generation cache primitive MUST require opened
trusted-generation-parent and isolated attempt-root capabilities, reject
symlink roots and equal/containing roots,
validate nonzero bounded limits, traverse and copy descriptor-relatively with
no-follow, reject links and special entries, stream/hash/recheck staged bytes,
normalize modes, fsync, and atomically publish one complete immutable
generation with no replacement. It MUST not claim generic mutable-destination
promotion. The export primitive performs no transport or resolution and does
not authorize a caller's project-cache selection; network, namespace, mount,
and process enforcement belong to the production-runner requirement below.

### SR-REQ-PRODUCTION-RUNNER

The component MUST follow
[`../../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md`](../../docs/decisions/0004-bubblewrap-runner-and-sandbox-v1.md):
Bubblewrap-backed Linux isolation, verified backend and kernel-capability
probing, phase-specific mount construction, explicit resource enforcement,
process-tree cleanup, and fail-closed backend verification, built on the
version-one parsing, validation, and acquisition semantics established by the
protocol and acquisition boundaries. It wires the source origin matcher and
native data/private-admin topology to resolved mounts. Portable export
generation remains a separate provider-capability primitive, not native
acquisition publication.

### SR-REQ-TOOLCHAIN-E2E

Explicit native toolchain trials MUST execute the real runner and Bubblewrap
with denied network and fail, not self-skip, on missing prerequisites.
System-toolchain trials MUST discover Python, Node, Go, C and C++ through
sandbox PATH and execute real dependency-free programs. Rust inventory trials
MUST explicitly grant every host-installed toolchain read-only through a
private, sandbox-native rustup registration, compare installed toolchain names,
compiler/Cargo/rustdoc versions, target catalogues and installed targets and
components with host query evidence, compile for every installed target, and
execute native Rust and locked offline Cargo tests for every toolchain.
Rustup mutation attempts MUST fail, and before/after content and metadata
snapshots MUST detect changes to the host rustup installation, including
externally linked toolchain roots. Tests MUST NOT install or update host tools.
These trials establish enforcement of explicit grants, not automatic
full-inventory discovery by a consumer's selected-toolchain profile.

The Rust inventory fixture MUST include at least two installed toolchains
and at least one installed non-host target, so its multi-version and
cross-target obligations cannot pass vacuously.

Native trials MUST resolve the host rustup executable and installation roots
from the execution environment rather than assume a distribution-specific
proxy location. Their explicit read-only runtime MUST supply the Rust proxies
needed by Cargo subprocesses independently of host System proxies. A native
regression MUST exercise Cargo with no System Rust proxies on PATH, including
unit tests, doctests and a build-script compiler query. Diagnostics MUST report
host executable candidates and resolved paths, source-to-destination grants,
the selected non-secret sandbox path environment, inside-sandbox executable
and sysroot resolution, and command results on success and failure. System
language trials MUST distinguish host-only tools from tools available in the
filtered System mounts and verify the latter by actual execution.

### SR-REQ-WORKSPACE-VENDOR-MASK

For authoring requests with a writable `/workspace` grant, the runner MUST make
an existing ordinary workspace `.kvist/vendored` node readable read-only in the
sandbox, independently of Rust integration. The resolver mount, when supplied,
MUST remain readable. Overlaying MUST NOT change host registry content or
permissions, hide sibling `.kvist` files, relax grant-overlap validation, or
create host directories when the registry is absent. Unsupported or absent
paths MUST NOT disable independent tools or follow new host origins.

## Quality requirements and constraints

- Rust stable edition 2024 is required. Prefer safe Rust; necessary, minimally
  scoped unsafe exceptions must satisfy the root contract's justification,
  safety-invariant, targeted-verification, and independent-review rules.
  Retain current compile-time unsafe guards unless deliberately approving
  such an exception; this policy does not authorize their broad removal.
- Executable support is Linux-only.
- The component MUST NOT import Kvist maerg implementation types.
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
and process evidence comes from the executable-enforcement tests and the
queued security audit and compliance review, and MUST NOT be inferred from a
successful protocol-and-acquisition-validation build.
