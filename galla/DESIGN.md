<!-- kvist-design-version: 1 -->

# Galla Design

## Design overview

This runner enforces isolated Linux execution for approved Kvist task requests.
A core library strictly validates the version-one protocol types and bounds;
`src/enforcement.rs` executes requests within isolated Bubblewrap namespaces,
applies `prlimit` resource bounds, and guards network access. Missing
prerequisites or invalid requests fail closed.

## Internal structure

Before probe/enforcement work (and CLI request input), inspect the fixed
`/.galla-sandbox` marker with `symlink_metadata`. Any existing entry rejects
nesting; errors other than absence fail explicitly. Mount a fresh read-only
tmpfs marker after request grants in every phase. Reserve its path in grant
validation so marker setup cannot touch a writable host grant. Do not infer
confinement from environment, generic containers or namespace IDs. This
user-requested increment uses a deliberate advisory exception while review
receipts are unavailable; it claims neither acceptance nor certification.

`enforcement/system_layout.rs` constructs a namespace-only filtered `/usr`
view, binding its immediate children read-only except `sbin` and handling
`local` with the same bounded, sorted one-level enumeration. This avoids
following a host `/usr/sbin -> bin` link and accidentally masking `/usr/bin`.
Other child links retain their link text. Missing `/usr/local` uses only
namespace backing; present `/usr` and `/usr/local` must be non-link directories.
Each directory is bounded to 1024 children; inspection/read/link errors fail
explicitly. Mode-000 read-only tmpfs reserves the three administrative paths;
filtered parent mounts are remounted read-only and payload capabilities are
dropped. No host mountpoint or permissions are changed. Root and administrative
overmount grants are rejected; exact read-only Toolchain self-grants for the
filtered roots are not rebound. Native tests check access denial and successful
bin execution; unit fixtures include merged-bin aliases and absent local roots.

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
importing maerg types.

After binding grants, prepared Rust authoring with writable `/workspace` and
read-only toolchain `/rust/vendor` inspects the workspace source's `.kvist`
and `vendored` directories without following links. Existing registry content
is covered by mode-000 tmpfs, then remounted read-only; drop all child
capabilities so privileged execution cannot bypass its permissions. Missing
paths require no overlay and cause no host directory creation. Inspection
errors and non-directory/link paths fail enforcement explicitly. This fixed
mask neither hides sibling workflow files nor changes disjoint grant
validation. Tests check denial of listing, reads, writes and permission
changes, successful offline resolution, host non-mutation and unaffected
non-Rust requests.

## Interactions and state

The runner receives one bounded JSON request on standard input, validates it
independently, and persists no state. It never trusts maerg-side validation as
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

`tests/toolchain_e2e.rs` exercises the executable protocol with real
Bubblewrap, system PATH discovery and dependency-free execution. Its opt-in
Rust trial enumerates the host inventory without installation, creates
private settings and sandbox-native registration links, mounts every resolved
installation read-only, compares per-toolchain query output and builds and
runs fixtures. Streaming content hashes, link destinations and filesystem
metadata (excluding access time, which reads may update) are compared before
and after queries, execution and denied mutation attempts, even when the
execution assertions fail. External custom toolchain roots are also covered.
No consumer implementation is imported. CI explicitly enables these otherwise
ignored native trials; missing tools or enforcement cause failure.

Host rustup is resolved from executable PATH candidates with System fallbacks;
`RUSTUP_HOME` or `HOME/.rustup` locates the inventory. Host queries clear ambient
configuration and disable installation. The fixture mounts the resolved rustup
file at a private destination and generates a separate read-only bin directory
with sandbox-native rustup proxy links for Cargo, rustc and rustdoc. A linker-only
PATH fixture excludes System Rust proxies to reproduce the CI failure locally.
The repaired runtime must run the same locked offline Cargo unit/doctest and
build-script fixture there. Every installed version uses that private runtime.
System language selection searches the namespace's bin paths, records host PATH
candidates separately and rejects canonical executables or Go roots outside
the filtered System view. Diagnostics print only selected tool/path settings,
not the complete ambient environment, credentials or filesystem inventories.

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
