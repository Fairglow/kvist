<!-- kvist-contract-version: 1 -->

# Galla Contract

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Boundary and ownership

- **Component ID:** `galla`
- **Contract IDs:** `galla.protocol/v1`,
  `galla.cli/v1`
- **Owner:** standalone `galla` crate
- **Consumers:** the Kvist maerg and Linux operators

The component owns independent request validation and operating-system
enforcement. It does not authorize grants, transition tasks, promote output,
or mint Kvist evidence.

## Provided interfaces

The executable is named `galla`. It accepts exactly one protocol
mode argument:

- `--kvist-sandbox-request-v1` reads one bounded JSON request from standard
  input, strictly parses and independently validates it, and either rejects it
  with an actionable non-secret diagnostic or enforces it via Bubblewrap-backed
  Linux isolation. It never falls back to host execution and never reports an unconstrained
  request as enforced.
- `--kvist-sandbox-probe-v1` attempts to confirm production enforcement
  capabilities. When a verified Bubblewrap backend and namespace enforcement are
  available, it emits a confirmed probe response object; otherwise it fails closed
  with an actionable non-secret diagnostic.

The accepted target interface is the redefined
`kvist-sandbox-probe-v1` and `kvist-sandbox-request-v1` protocol described by
ADR 0004. Request parsing, validation, and Bubblewrap-backed isolation are
fully implemented and active.

## Enforcement status

ADR 0019 supersedes the previous administrative-directory exclusion policy:
standard system trees, including sbin paths, are live read-only views. Host
edits are legitimate inputs; executable names do not grant host privileges.
Namespace/capability and network/grant enforcement remain unchanged.
`Resources.max_file_bytes` is an optional positive integer; omission/null means
no payload file-size cap. Explicit caps restrict payload writes only.
The runner never scans/deletes operator files to enforce that cap.
Granted ordinary source nodes are opened through no-follow descriptor traversal
and bound from those descriptors; installed Bubblewrap must support `--bind-fd`
and `--ro-bind-fd`, qualified by the native probe. Descriptors are consumed before
the payload, not exposed as host directory capabilities. Mounted node contents
remain live; replacement nodes are selected on subsequent calls without a
content/revision check. This is origin authorization, not a frozen manifest.
Workspace vendor aliases are readable but read-only, not mode-000 input
exclusions. The private `/tmp` and `/run` filesystems each use `max_scratch_bytes`;
this is a bounded RAM budget, not a quota/poll on host-mounted scratch data.

System bin/sbin paths remain usable read-only. Authoring PATH may select
canonical sandbox directories, including writable workspace executables;
namespace enforcement, not PATH provenance, bounds their effects. Closed Cargo
phase-specific PATH rules are retained.
Explicit authoring environment values configure the isolated payload, including
runtime loaders and Git/language paths; host enforcers run with cleared
environments. Fixed Cargo-phase environment/source constraints remain separate.
`enforcement::source::pin(&Path) -> Result<OwnedFd, String>` opens an ordinary
absolute source through no-follow descriptor traversal. Returned descriptors
are close-on-exec by default; callers explicitly transfer only final bind
descriptors to a backend that consumes/closes them before payload execution.
Writable origins containing the running runner, its installed replacement,
host enforcers, their resolved transitive libraries/loaders, active executable
mappings or global loader inputs are rejected with scope/installation remediation.
Direct writable file grants are also rejected on pinned device/inode equality.
Known code-path ancestor selector slots are protected through their canonical
parent origins and encountered symlink targets, even when both the original
lookup name and final resolved file lie outside a writable grant.
Legitimate unlink/replacement of the running image does not require a restart
or surviving old pathname; kernel image protection remains active.
Approved directories are object grants: callers must avoid trusted-code/host
control-channel/inode aliases inside them; no whole-host alias/IPC inventory
or general attribution of concurrent external writes is claimed.

Bubblewrap-based namespace enforcement is fully implemented and active. The
runner constructs a complete Bubblewrap command line that establishes user,
IPC, PID, UTS, cgroup, and network namespace boundaries. Resource limits are
enforced via the `prlimit` system call. Network egress is proxied through a
source-aware guard that restricts connections to allowlisted registries and
Git sources. The separate portable-export generation primitive uses
`RENAME_NOREPLACE` for atomic publication; native acquisition writes directly
to approved data roots without generation construction.

## Required interfaces

The library query `validation::is_administrative_path(&Path) -> bool`
recognizes the three standard administrative prefixes after lexical component
normalization, without following filesystem links. Host executable selectors
check original and canonical paths separately. The maerg mirrors this contract
independently rather than depending on the runner as a Rust library.

The production runner will require Linux namespace support and a verified
Bubblewrap executable. This revision requires the Rust standard library, `nix`
for safe descriptor-relative Linux filesystem operations, `serde`/`serde_json`
for parsing, and `sha2`/`hex` for identities, and receives no maerg authority.

## Data and schemas

The canonical request and probe shapes are version 1 and are defined by the
runner's own typed Rust parser in `src/protocol.rs`, which is authoritative.
The closed request models a disjoint execution phase (`authoring`,
`verification`, or `dependency-acquisition`), a bounded argument vector, a
canonical absolute working directory, portable environment entries, a network
profile (`deny`, or `package-sources` for mediated acquisition), bounded
resources (including an optional `max_cache_bytes`), approval-bound identities,
typed mount grants, a `system` or `cargo` toolchain, an optional mediated
dependency cache (`cargo_home`/`registry`/`git`, either explicit `native` data
endpoints or export-mode `writable`/`approved` whole-home endpoints, an
acquisition lockfile workspace, and export promotion intent), and scratch.
Cargo phases have exact allowlisted environments, exact
argv, and exact toolchain/grant/cache topology. The runner independently
derives source identities using documented domain-separated encodings and
rejects a mismatch. Network egress is strictly mediated via the source-aware proxy,
and namespace, mount, and process isolation are enforced via Bubblewrap.

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

### Native Cargo source mode

`Cache.native: Option<NativeCache>` explicitly selects native mode when present.
`NativeCache` is a closed object with optional `registry` and `git`
`CacheEndpoint` fields and optional `registries` descriptors (omission/null is
allowed). `Cache.registry` is exactly
`/workspace/cargo-registry`; `Cache.git` is exactly `/workspace/cargo-git`.
Present native registry/Git endpoints must name those destinations and bind
exact grant identities with `Purpose::Registry` / `Purpose::DependencyCache`.
Their grants are read-only during verification and read-write only during
dependency acquisition. The backend requires ordinary pinned data directories,
not file origins. Missing endpoints have no corresponding data grant.
`Cache.cargo_home` and `CARGO_HOME` are exactly
`scratch.destination + "/cargo-home"`; writable/approved whole-home endpoints
and promotion must be absent. There is no data promotion: acquisition writes
directly to the provider/caller-approved persistent data grants.

Test verification is exactly `<cargo> test --offline --locked`, with denied network
and exactly `HOME=scratch/home`, `PATH=/workspace/bin`,
`CARGO_HOME=scratch/cargo-home`, `CARGO_TARGET_DIR=scratch/target`,
`CARGO_NET_OFFLINE=true`. Runtime is one read-only grant at `/workspace/bin`.
Acquisition is exactly `<cargo> fetch --locked`, with approved package sources,
the same HOME/PATH/CARGO_HOME/target values, and
`CARGO_NET_GIT_FETCH_WITH_CLI=false` instead of `CARGO_NET_OFFLINE`.
Its exact writable Lockfile grant is bound to the before identity and equals
the working directory. Before payload launch the native backend reads
`Cargo.lock` through that pinned workspace without following links and verifies
its content matches the before identity; mismatch fails without scratch
initialization. This is a per-call precondition, not a frozen workspace.
Scratch and toolchain identities remain exact.
Native mode requires no whole-home cache-size bound or data manifest.
Native writer/GC coordination belongs to the shared provider; Galla's private
Cargo admin locks do not synchronize unrelated host cache actors.

After binds the backend creates a private Cargo administrative directory under
scratch and sandbox-only `registry`/`git` symlinks to declared data destinations.
Absent endpoints get no links and have empty read-only namespace-local data
directories, so acquisition cannot silently retain undeclared data in admin
scratch instead of an approved persistent root. Conflicting paths fail without
replacing them.
Native cache data is not copied or mounted below the private Cargo home.
`native` absent/null deliberately retains the explicitly approved export mode:
approved read-only whole Cargo home plus Registry `/workspace/vendored` and
CargoConfig `/workspace/.cargo` for genuine vendor/export verification.
Export acquisition retains its explicit whole-home endpoint and promotion
intent; it too uses `fetch --locked`. This is not a fallback or migration.

### Native named registry aliases

`NativeCache.registries: Option<Vec<NativeRegistryAlias>>` is optional
(omission/null/empty array generate no config). Each closed
`NativeRegistryAlias` has `name: String`, `index_origin: String`, and
`protocol: NativeRegistryProtocol`. The closed enum `NativeRegistryProtocol`
has `Git` / `Sparse` variants serialized as `"git"` / `"sparse"`. An omitted
`protocol` defaults to `Sparse`; explicit null/unknown values are rejected.
At most 64 aliases are accepted. Names are unique, 1–64 ASCII bytes from
letters/digits/`-`/`_`, excluding `crates-io` and Cargo-invalid dotted names.
Index origins are exact
canonical production HTTPS URLs; sparse origins must end in `/`, while Git
origins need not. Credentials, query, fragment,
percent/path aliases and private/loopback origins are rejected. The wire origin
does not include Cargo's `sparse+` prefix.
The caller can choose protocol from exact locked source prefixes matching the
configured origin: `registry+https` means Git; `sparse+https` or
`registry+sparse+https` means sparse. Conflicting matches require an explicit
ambiguity error; an unused alias can conservatively use sparse. Galla does not
infer protocol from host configuration or acquire additional authority.

For native acquisition, aliases equal the additional `AllowedSource::CargoRegistry`
name/index-origin set, excluding built-in `crates-io`. Every alias must match,
and every additional registry source must have its alias. Verification has no
network sources; its caller-approved aliases do not grant network authority.
No environment, argv, endpoint or source-replacement capability is added.

The backend writes only name-sorted `[registries."<name>"]` tables with
`index = "<index_origin>"` for Git or `index = "sparse+<index_origin>"` for
sparse into read-only
`CARGO_HOME/config.toml` within private namespace-local admin. Names/values are
escaped as TOML strings, not interpolated as syntax. Only these generated
settings are present: no host credentials/config, `[source]`, replacement,
tokens or downloader settings. The small generated buffer is passed through
an owned backend-consumed memory descriptor, never a host data/config
mount or a descriptor exposed to the payload. Native alias-free and portable
export behavior remain unchanged.

### Native Cargo metadata admission

`Phase::Verification` with `Toolchain::Cargo` and present `Cache.native`
additionally accepts exactly this seven-element argv:
`[cargo, "metadata", "--offline", "--locked", "--format-version", "1", "--all-features"]`.
The `cargo` element must still equal the exact approval-bound toolchain path.
All native test verification constraints are unchanged: read-only verification
workspace at the exact working directory; optional fixed read-only registry/Git
endpoints; one read-only Runtime `/workspace/bin`; exact scratch/toolchain
identities; no whole-home endpoint, lockfile workspace or promotion; and exactly
`HOME=scratch/home`, `PATH=/workspace/bin`, `CARGO_HOME=scratch/cargo-home`,
`CARGO_TARGET_DIR=scratch/target`, `CARGO_NET_OFFLINE=true`. Network is denied
with no allowed sources. Metadata does not execute build scripts or compile.
Portable export verification still accepts only the test operation; acquisition
still accepts only `fetch --locked`.

No request field, report envelope or readiness claim is added. On success,
stdout is actual Cargo metadata JSON in format version 1, bounded by the existing
resource/output policy. Cargo may spell native manifest paths through its
private-home links: `scratch/cargo-home/registry/src/...` and
`scratch/cargo-home/git/checkouts/...`, which alias the declared fixed data
mounts. The caller must map only these exact request-bound aliases to its
declared native roots before source-admission validation; Galla does not rewrite
JSON or authorize arbitrary path aliases. The caller supervises execution and
accepts stdout only on success
before handing it to its source-admission validator. Input/validation and
enforcement failures keep status 2/3 and non-secret stderr diagnostics;
Cargo failures retain existing payload exit/stream behavior. Metadata output
is evidence for caller validation, not a Galla cache readiness or content
attestation report.

### Bounded package-source transport

The package-source proxy admits at most 16 active clients per request; excess
clients are closed after a best-effort 503 response rather than entering an
unbounded queue. Proxy
headers are limited to 16 KiB and five seconds from admission. Upstream
resolution and TCP establishment each have a five-second deadline; an inactive
relay closes after 30 seconds without bytes making progress in either direction.
CONNECT preserves opaque bidirectional bytes and TCP half-close; encrypted paths
remain uninspectable and authorization remains exact scheme/host/port authority.
Plain HTTP retains the source path-prefix check. No protocol fields or consumer
APIs are added. Guard startup failure prevents payload launch; request teardown
stops admission and joins owned relay workers, with up to five seconds allowed
for an in-progress TCP establishment. Standard-library DNS cannot be interrupted:
one bounded resolver helper may finish its outstanding lookup after cancellation,
but it holds no client/upstream sockets and creates no extra lookup threads.
Authoring and verification retain their denied-network semantics.
Before establishing a tunnel, failures use 400 for malformed headers, 403 for
denied sources/addresses, 408 for header timeout, 431 for oversized headers,
502 for failed upstream connection/resolution and 504 for upstream deadlines.
These responses have empty bodies. After CONNECT success, transport failures
close the stream rather than inject plaintext into TLS. Diagnostics are bounded
and never contain submitted headers or tunneled bytes.

`max_processes` is sampled descendant/process-group task accounting, including
threads; it is not a kernel admission ceiling and may overshoot between samples.
Process names containing spaces or parentheses must not corrupt accounting.
UID-wide NPROC limits are not imposed on unrelated host users/jobs.

The runner rejects unknown fields, unknown enumerants, wrong types, malformed
`sha256:` digests, oversized input, non-canonical paths (duplicate `/`
separators, a trailing `/` except root, or `.`/`..` components) in `argv[0]`
and every path field, oversized or malformed environment names, a resource
limit that is zero or exceeds its explicit safe maximum (wall time, output,
processes, files, file bytes, scratch bytes, and cache bytes), a read-write
grant to a read-only purpose or phase, traversal or overlapping destinations,
invalid Cargo cache, scratch, or lockfile endpoint topology, and every retired
request shape. Acquisition additionally rejects anything but `<cargo> fetch --locked`,
a mismatched Cargo toolchain/grant, wrong Cargo home, lockfile workspace,
scratch target/HOME, path, source identity, or export promotion intent.
Cargo verification rejects anything but the exact test operation or, only in
native mode, the exact metadata admission operation. It rejects non-denied network,
non-offline environment, mutable cache data, or wrong cache/scratch/workspace
topology. The runner exposes validated transport-policy value types and
descriptor-relative immutable generation construction, and enforces namespace,
mount, resource, and network isolation via Bubblewrap and a local source-aware proxy;
if enforcement prerequisites are not met, a request fails closed and never falls
back to host execution.

## Errors and failure semantics

Portable-export promotion intent does not authorize a host destination. The CLI
must not infer a mutable sibling `project-cache`. Until its provider integration
supplies a trusted generation-parent descriptor, a successful fetch retains its
attempt-local files and reports promotion unavailable, without publishing or
replaying the payload. Providers can use the existing descriptor-relative
`cache::open_trusted_generation_parent`, `open_isolated_attempt_root` and
immutable-generation API after authorizing the destination. Native acquisition
has no promotion intent or generation-parent requirement: its declared
read-write data grants are the persistent destinations.

Galla reserves `/.galla-sandbox` as a read-only namespace-local marker in every
execution phase; grant destinations cannot cover it or its descendants.
Inside a marked sandbox, both protocol modes return status 3 without reading
stdin, probing Bubblewrap or starting another sandbox. Stderr explains:
already inside a Galla sandbox; nesting is unnecessary; run the intended
command directly. Clearing environment variables does not remove detection.
The marker is a diagnostic signal, not an authority or confinement certificate.

Workspace authoring overlays an existing ordinary `/workspace/.kvist/vendored`
file/directory read-only from the pinned writable workspace origin, independently
of Rust preparation. Its contents remain readable/live but cannot be changed
through that alias; host permissions and sibling files are unchanged. Missing,
linked or nontraversable nodes add no outside origin or host mountpoint. Other
request topologies are unaffected. This is not a general overlapping grant
capability or request field.

A rejected, unenforceable, or unknown invocation writes a concise non-secret
diagnostic to standard error, emits nothing on standard output, and returns a
nonzero exit status. A parse, validation, or usage failure returns status 2; an
otherwise valid but currently unenforceable request or probe returns status 3.
Unknown arguments do not activate fallback or host execution.

## Security and authority

The runner is a separate trust boundary. It validates all request input
independently, bounds untrusted input before parsing, and must be installed as
a regular non-link file outside the selected project and worktree. The runner
exercises process, namespace, and network authority exclusively via Bubblewrap
and the network guard proxy according to validated request constraints.
Portable-export generation construction requires provider-opened root
capabilities and no-follow bounded immutable-generation construction. The
current command boundary never invokes that primitive; selecting or changing
a project's export generation is not implemented. Native acquisition instead
uses the exact approved data-root grants and private Cargo administration.

## Compatibility and verification

Native inventory tests may supply multiple explicit read-only toolchain grants
and a private generated rustup home whose registrations refer to sandbox paths.
Galla does not discover host toolchains or synthesize that home for consumers.
Visibility is determined by the submitted grants and environment; a
single-toolchain request does not promise full host inventory visibility.

The native execution fixture resolves host rustup separately from its fixed
sandbox runtime destination. It supplies read-only Rust proxies and registrations,
so Cargo's compiler and documentation subprocesses do not depend on System
Rust proxies. Its diagnostic output includes detected host locations, explicit
grant mappings, sandbox path/selection settings and observed tool resolution.
This does not add runner-side automatic discovery, ambient PATH inheritance,
home-directory mounts or new consumer authority.

Kvist is pre-release and retains no legacy protocol compatibility. Current
tests cover strict accepted shape, canonical serialization, malformed,
oversized, unknown, and legacy rejection, fail-closed unavailability, the
mediated-acquisition source, argv, and environment policy, the origin matcher,
and the cache inspection/promotion primitive.
Native Bubblewrap, path, resource, cleanup, and substitution enforcement tests
are mandatory before the production contract can be reported as implemented.
