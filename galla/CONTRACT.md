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

The executable is named `galla-runner`. It accepts exactly one protocol
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
Git sources. Immutable cache promotion uses `RENAME_NOREPLACE` to guarantee
atomicity.

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
dependency cache (`cargo_home`/`registry`/`git`, a `writable` or `approved`
endpoint, an acquisition lockfile workspace, and result-bound promotion
metadata), and scratch. Cargo phases have exact allowlisted environments, exact
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
request shape. Acquisition additionally rejects anything but `<cargo> fetch`,
a mismatched Cargo toolchain/grant, wrong Cargo home, lockfile workspace,
scratch target/HOME, path, source identity, or result-bound promotion metadata.
Verification rejects anything but `<cargo> test --locked`, non-denied network,
non-offline environment, mutable Cargo home, or wrong cache/scratch/workspace
topology. The runner exposes validated transport-policy value types and
descriptor-relative immutable generation construction, and enforces namespace,
mount, resource, and network isolation via Bubblewrap and a local source-aware proxy;
if enforcement prerequisites are not met, a request fails closed and never falls
back to host execution.

## Errors and failure semantics

The request's promotion intent does not authorize a host destination. The CLI
must not infer a mutable sibling `project-cache`. Until its provider integration
supplies a trusted generation-parent descriptor, a successful fetch retains its
attempt-local files and reports promotion unavailable, without publishing or
replaying the payload. Providers can use the existing descriptor-relative
`cache::open_trusted_generation_parent`, `open_isolated_attempt_root` and
immutable-generation API after authorizing the destination.

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
the cache primitive with provider-opened root capabilities — no-follow bounded
immutable-generation construction. The current command boundary never invokes
it; selecting or changing a project's current cache generation is not
implemented.

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
