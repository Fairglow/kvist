<!-- kvist-design-version: 1 -->

# Galla Design

### Hermetic transport DNS tests

The private transport constructor accepts an address-lookup function so named
origin, bounded resolver-queue and pinned-address relay tests require no ambient
`/etc/hosts` or resolver configuration. Production startup always supplies the
native standard-library lookup; normalization, address bounds, policy checks
and deadlines remain shared with the fixture path. Testability does not grant
host configuration or networking to the workspace sandbox.

In portable-export mode, promotion intent alone supplies no trusted publication
destination. Do not derive or inspect a sibling project-cache. CLI execution
may retain successful export attempt output but must report incomplete
promotion until provider-owned descriptors are supplied to the existing
immutable generation primitive. Native mode has no promotion: acquisition
writes directly to approved persistent native data roots.

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

### Native Cargo data topology

Validate native structure before phase selection: exclusive source mode,
canonical fixed data destinations, exact endpoint/grant identities and access,
scratch-owned Cargo home, and optional endpoint absence. Exact phase validators
then compose toolchain, Runtime, scratch, workspace and each present data
purpose into one closed list. Native verification has no writable workspace or
data grant; acquisition has one writable lockfile workspace. No native
whole-home endpoint, data manifest, copying or publication exists.
Inspect present pinned data roots with `fstat` and reject non-directory nodes
before adding administrative mutations; never enumerate their contents.

Private `enforcement/native_cache.rs` checks scratch through its pinned
descriptor and refuses an existing `cargo-home` node. Bubblewrap operations
after data/scratch binds create `cargo-home`, `home`, `target` with private
permissions and only present native links. Namespace-local Cargo-home tmpfs
keeps links and admin locks/config private; no native cache bytes are copied.
The fixed links target the declared sandbox data mounts, never host origins.
For each absent endpoint, mount an empty read-only tmpfs at the corresponding
Cargo-home child. This preserves dependency-free operation but prevents
undeclared acquisition data from falling into writable private administration.
Before adding scratch operations, native acquisition opens `Cargo.lock`
read-only/no-follow relative to the pinned Lockfile workspace, requires an
ordinary file and streams its SHA-256 into fixed working memory. Compare only
the exact before identity; this has no file-size quota, tree walk, manifest or
snapshot/content-freeze claim.
Any required directory conflict is detected before mutation; the helper never
overwrites unrelated caller entries. Descriptor owners remain with the existing
bind handoff and are consumed before payload execution.

Native verification additionally accepts the exact seven-element metadata
argv defined in the contract. Compare the complete argument tail before phase
topology validation; only a present native cache permits metadata. Reuse the
same native verification validator/admin hook instead of adding a broader verb,
new phase, wrapper, environment, mount or host metadata subprocess. Cargo
metadata resolves the locked all-features source graph without build-script
execution. The runner forwards bounded Cargo stdout unchanged; the caller
validates the actual format-version-one package/source/manifest evidence.
Cargo preserves private-home registry/Git link prefixes in manifest paths.
Consumers normalize only the aliases implied by the exact accepted request's
Cargo home and native endpoint grants, never arbitrary reported path prefixes.
Admission never constructs or publishes a readiness report itself.

Validate native registry aliases before exact phase selection: bounded unique
Cargo-compatible ASCII letters/digits/`-`/`_` non-built-in names and canonical
production HTTPS index origins. Reject dots rather than accept an alias
that Cargo cannot reference from a manifest.
Deserialize the closed `NativeRegistryProtocol::{Git,Sparse}` field with
`Sparse` as its omission default. Require a trailing slash only for sparse.
Preserve the exact Git HTTPS origin when generating its index entry; add
`sparse+` only for sparse entries. Never infer protocol or rewrite source
identity inside the runner. The caller resolves exact locked source prefixes
and reports ambiguity before producing the request.
During acquisition compare the full alias set against additional registry
name/index tuples already independently source-identity validated. Sort aliases
by name and encode quoted table keys/values for a deterministic private
`config.toml` containing only registry index settings. Use a Linux close-on-exec
memory file for the bounded generated bytes, rewind it and hand off only that
descriptor to Bubblewrap `--ro-bind-data` after private Cargo-home tmpfs/link
operations. Retain the owner until backend spawn; Bubblewrap consumes/closes
the descriptor before the payload. No host file, source replacement, ambient
configuration or native-cache copy is involved. Return no config descriptor
for absent/empty aliases. Scratch conflict checks still precede mutation.

This explicit native-mode increment uses the human-approved local advisory
exception. Focused failing tests precede code; security/compliance and IMPL
derivation are left to independent contexts, with no acceptance claim.

### Bounded package transport increment

Extract transport and the existing request-line parser into private
`enforcement/transport.rs`, retaining parser semantics and the origin matcher.
A nonblocking listener has at most 16
joinable connection workers, reaps completed workers, and rejects overload without
spawning or queuing more clients. Guard RAII owns cancellation and the listener
thread; drop stops admission and joins it and its workers.

Each worker accumulates at most 16 KiB through the complete CRLF header terminator
under an absolute five-second admission deadline, retaining coalesced tunnel
bytes. It authorizes before DNS or upstream I/O. A single resolver helper
consumes a bounded queue and returns a bounded address list; workers wait at most
five seconds, cancellably. Standard DNS has no cancellable API; the helper holds
no transport sockets and may outlive guard cancellation during its one outstanding
lookup. TCP connects to the returned concrete address, not a second name lookup,
under one five-second deadline across addresses. An in-progress standard-library
connect can delay cancellation by at most that deadline. This does not claim to
repair existing acquisition namespace bypass or add TLS path inspection.

Nonblocking bidirectional pumps use one fixed 16 KiB buffer per direction, handle
partial writes/backpressure independently, and forward EOF as write-half shutdown
only after buffered bytes drain. Progress in either direction resets one shared
30-second inactivity deadline. Header/status writes also have deadlines. Poll
with a short bounded sleep when neither pump progresses; retry only Interrupted
and WouldBlock, explicitly classify all other failures. CONNECT status is sent
before waiting for upstream bytes. Plain HTTP preserves GET/HEAD and removes
proxy-only headers, then uses the same relay and bounds. No shell, new crates,
request validation changes, or cache publication changes are needed.
At most 16 transport diagnostics are emitted per guard, followed by a suppressed
count on teardown if needed; each failed client is still explicitly rejected or
closed. Client input is never included in diagnostic text.

Scoped intent and the four ordered transport task definitions use the explicit
human-approved advisory exception; no acceptance/review receipt is emitted.
Focused real-socket tests precede code. Independent contexts will observe and
review implementation; this implementer does not write IMPL or certify compliance.

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
| `src/cache.rs`         | Separate portable-export primitive for descriptor-relative bounded construction and atomic no-replacement publication of immutable Cargo-home generations             |
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
Both source modes use closed phase-specific environments and exact grants:
acquisition is only `<cargo> fetch --locked`; test verification is only
`<cargo> test --offline --locked`, with denied network and offline true. Native
verification may instead use exactly
`<cargo> metadata --offline --locked --format-version 1 --all-features`.
Native mode uses approved persistent registry/Git data grants, a private
writable scratch-owned Cargo home, and its exact verification or writable
lockfile workspace. Only deliberately selected portable-export mode uses
whole-home endpoints: acquisition has a separate writable attempt Cargo home
and export promotion intent, while verification has an approved read-only
Cargo home plus its fixed read-only vendor/config mounts. Endpoint absence
inside native mode never selects export mode. The runner independently derives
source identities from
length-delimited fields following a domain string; golden vectors cover each
field substitution.

`src/origin.rs` provides the source-aware origin policy used by the network
guard: canonical origins, production and test-loopback modes, and
reauthorization of an initial URL or redirect against supplied pinned
addresses. The separate portable-export primitive in `src/cache.rs` accepts
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
therefore has no recovery transition. Native acquired data remains directly in
its approved persistent roots; no export publication or tree inventory is
performed. Before separate portable-export publication, immutable-generation
construction can remove its exclusively named private staging tree only through
retained descriptors; a cleanup failure is returned explicitly rather than
concealed. Once `RENAME_NOREPLACE` succeeds, the complete generation is
published or the operation fails without replacing a prior generation.
Project cache-reference updates remain a separate later design.

## Security and resource design

No shell is used; the enforcement path spawns exactly one Bubblewrap child per
approved request. Untrusted input
is size-bounded before parsing and every value is length-bounded. The
portable-export generation primitive uses `nix` descriptor-relative no-follow
opens after its
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

`tests/native_cache.rs` has default pure closed-topology regressions and four
explicit ignored native trials. Run its `native_runner` selector with
`--ignored --test-threads=1` on a host with Rust, Bubblewrap and cached crates.io
`hex` 0.4.3 (optionally select data with `KVIST_NATIVE_CARGO_HOME`). It fails,
rather than self-skips, on missing prerequisites; compares selected native
index/archive/source bytes, proves writable namespace-only admin, denied
network/data writes and conflicting scratch links without clobbering. It adds
no host dependency acquisition or native data inventory. The second native
trial executes locked dependency-free acquisition without data endpoints or
promotion and verifies the actual lockfile-before precondition.
The cached-registry trial also executes exact native metadata, parses its
version-one source/manifest evidence, verifies unchanged cache bytes and no
compiled outputs, and discovers a panic-only build-script target without
running it. Its manifest-path assertion records Cargo's private-home symlink
alias rather than pretending Cargo emits canonical fixed mount paths.
The third trial in `tests/native_cache/registry_aliases.rs` reproduces missing
named-registry configuration, then succeeds with a declared alias to existing
crates.io sparse data. Offline fixture preparation uses owned alias-only admin
and disables auto-cleaning; Cargo authors its exact lock source identity rather
than the test guessing it. The protected metadata operation stays offline and
locked. This tests named aliases, not a complete private alternate registry.
The fourth trial publishes one owned package archive produced by `cargo package`
to an owned Git registry index with its actual archive checksum. Cargo bootstraps
native index/archive/expanded data using private fixture-only Git URL rewriting
to the owned repository and a bounded loopback archive server; external Git
protocols are disabled during preparation. None of that preparation config is
mounted into the protected run. The actual Galla metadata operation declares
the original HTTPS Git index alias, read-only native registry data and denied
network, preserving `registry+https` source identity through private admin.
The test observes Cargo's own index directory name rather than inventing it;
it constructs no per-file cache inventory. This is local Git-index source
resolution evidence, not production HTTPS acquisition or Maerg normalization
evidence.
