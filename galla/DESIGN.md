<!-- kvist-design-version: 1 -->

# Galla Design

Promotion intent alone supplies no trusted publication destination. Do not
derive or inspect a sibling project-cache. CLI execution may retain successful
attempt output but must report incomplete promotion until provider-owned
descriptors are supplied to the existing immutable generation primitive.

## Design overview

ADR 0019 replaces filtered administrative trees with live read-only System
binds and removes host-file polling/deletion. Optional file-write limits are
applied to the child process; absence requests no quota. Retain bounded protocol
parsing, namespace/capability/network/grant enforcement and owned-process cleanup.
Host edits never become file-size breach evidence.
Walk ordinary source ancestors with `openat(O_PATH|O_DIRECTORY|O_NOFOLLOW)` and
open the final ordinary node without following links. Bind final descriptors
using the backend's FD operations, retaining RAII owners through spawn; the
backend consumes/closes them before payload execution. Open any workspace vendor
overlay relative to the pinned workspace descriptor, not a second mutable host
pathname. No content hash, old inode, shared lease or inventory is compared.
This preserves origin authority through rename/link races without claiming an
atomic snapshot. Existing ordinary vendor nodes receive a readable read-only
overlay; nonmaterialized paths do not introduce new mounts or stop unrelated work.

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

Native nesting regression checks run probe and request in separate outer
sandbox invocations and aggregate failures, so a probe failure cannot hide a
request blocked on stdin. Each inner process has a cleared environment and an
open, empty stdin pipe, and must exit 3 promptly without stdout.

`enforcement/system_layout.rs` provides live read-only standard system trees,
preserving host alias text rather than enumerating/filtering administrative
children. Drop all payload capabilities. No host mountpoint or permission is
changed. System self-grants reuse this view. Native tests distinguish denied
writes/host networking from successful bin/sbin reads and execution.
Pin ordinary explicit origins using close-on-exec descriptors, then enable
inheritance only for backend bind handoff. Qualification exercises both FD-bind
flags and confirms the payload receives no raw source descriptors. Origin
pinning does not freeze bytes or promise an atomic multi-mount snapshot.
Before payload launch, `enforcement/host_code.rs` retains close-on-exec
`O_PATH` guards for the kernel `/proc/self/exe` image, any installed replacement,
resolved host enforcers and loader-control inputs. Inspect active executable
mapping paths and kernel device/inode metadata. Discover the actual backend
and prlimit transitive dependency/loader closure by invoking each trusted
installed ELF binary with a cleared environment, only
`LD_TRACE_LOADED_OBJECTS=1`, and `--version`. Supervise nonblocking bounded
stdout/stderr and a deadline with process-group cleanup. Bounded ELF program
headers distinguish static images from missing/unsupported loader traces;
dynamic images require a complete resolved trace including their interpreter.
Reject writable pathname containment and direct pinned regular-file
device/inode aliases. Missing old runner paths after legitimate replacement
are not prerequisites; unavailable installed launchers/dependencies are.
For every bounded known code pathname, inspect its ancestor selector slots:
retain the original slot name and the canonical parent's corresponding slot.
Follow encountered symlink-target ancestor paths within a deduplicated,
bounded selector closure; do not enumerate directories. This catches a
writable selector parent hidden between an outside lookup alias and outside
final code. The closure permits at most 16,384 distinct ancestor nodes and
16,384 selector slots; retain the ELF interpreter lookup independently of
its canonical trace identity. Missing deleted-path slots remain optional. Recompute this
per-call topology without comparing old content or rejecting operator updates.
Descriptors remain close-on-exec and are never handed to the payload.
This bounded code closure is not a recursive alias inventory, content freeze
or host-content drift comparison.

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

After binding grants, authoring with writable `/workspace` inspects `.kvist`
and `vendored` relative to the pinned workspace descriptor without following
links. Existing ordinary vendor files/directories receive a readable read-only
descriptor bind even without Rust preparation. Missing, linked or nontraversable
nodes require no overlay or host directory creation. All child capabilities
remain dropped. This overlay neither hides sibling workflow files nor changes
disjoint grant validation. Tests check readable/live contents, denied writes and
permission changes, offline resolution, host non-mutation and unaffected
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
System language selection and the request PATH use the same fixed
`/usr/local/bin:/usr/bin:/bin` directories. Ambient host PATH entries are not
added merely because they lie beneath a System prefix: other directories
require explicit read-only grants. Selection rejects canonical executables or
Go roots outside the filtered System view. Diagnostics print selected tool/path
settings, not the complete ambient environment, credentials or filesystem
inventories.

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
