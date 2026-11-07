# Native cache source observation

Source-only observer: `d38462f8-d2f9-4d78-a5bb-c512f6458890`, reported
2026-11-08. The following excerpts preserve the observer's current-source
findings. The observer performed only reads/searches and executed no checks.
It excluded intent documents, README/docs, ADRs, queues, previous IMPL records,
reviews, Git history/diffs and prior chat/session history. Source comments
referencing those artifacts were not followed. Tests below are assertions, not
passing executions. This is not compliance or acceptance certification.

## Dependencies

`rust::cache::home` selects explicit `CARGO_HOME`, otherwise `HOME/.cargo`;
relative paths and `/` are rejected. Project configuration and cache receipts
do not select the home. `CacheView` separates that administrative home from
optional `registry` and `git` data roots
(`dependencies/src/rust/cache.rs:25–103`).

`view` performs catalogue inspection without launching Cargo or creating project
state. An absent home produces missing external dependencies; a local-only graph
can be reported ready without one. `sources` additionally supports an absent
lock and returns `NoLock`, `Catalogue`, or `CargoOfflineRequired`. Matching
historical receipts in the latter case are recorded as lock admission, not
present-source readiness (`cache.rs:329–368`; `cache/admission.rs:13–126`).

Canonical inspection recognizes crates.io namespace/configuration, sparse-index
cache records, matching name/version/checksum, archive presence, extraction
marker version, and package manifest identity. Namespace suffixes are inspected
rather than computed. Archive contents and source-file contents are not hashed;
the archive check permits any regular-file size (`cache.rs:105–327`).

Native provisioning uses a project-local advisory operation lock and a small
`.kvist/cargo-cache.json` receipt containing only schema version and exact lock
digest. It checks existing material, invokes the acquisition callback at most
once if incomplete, and rechecks before publishing the receipt. Failures retain
acquired material. `force` refreshes the receipt but does not force reacquisition
of an apparently complete catalogue (`cache.rs:370–617`).

For sources outside the supported canonical catalogue, offline Cargo metadata
validates exact locked identities and manifest locations beneath selected native
registry/Git roots. Every external locked identity must appear; duplicates,
unexpected identities, unsupported sources and external directory replacements
are rejected. Configured callbacks use fixed sandbox endpoints, with
request-bound private-home alias normalization (`cache/native.rs:170–354`;
`cache/admission.rs:145–290`).

Missing-source classification requires Cargo exit 101 and narrowly matched,
bounded offline diagnostic chains connected to the lock; other failures remain
errors (`cache/failure.rs:14–174`). Native catalogue inspection does not
recursively inventory source trees or enforce directory-export aggregate-content
quotas (`dependencies/src/limits.rs:10–50`).

## Galla

Native validation requires fixed `/workspace/cargo-registry` and
`/workspace/cargo-git` destinations, exactly `scratch/cargo-home`, writable
scratch, and matching grant identities/access. Native mode forbids whole-home
approved/writable endpoints and promotion. Data is read-write only for dependency
acquisition, otherwise read-only
(`galla/src/validation/native_cache.rs:9–83`).

Allowed Cargo commands are exactly acquisition `fetch --locked`, verification
`test --offline --locked`, and native metadata
`metadata --offline --locked --format-version 1 --all-features`.
Verification requires denied network and read-only workspace; acquisition
requires its writable lockfile workspace. Environment and grant-purpose sets
are closed, including exact `/workspace/bin` PATH
(`galla/src/validation.rs:854–999`; `validation/native_cache.rs:140–218`).

Alias configuration is generated deterministically into a private descriptor,
never copied from host Cargo administration. Acquisition alias names/origins
must exactly match additional registry approvals; verification aliases still
undergo bounded-name and canonical-origin validation
(`native_cache.rs:144–183`; `validation/native_cache.rs:85–138`).

Native validation returns before the whole-home branch's mandatory
`max_cache_bytes` check. No aggregate quota over native data or ordinary writable
scratch directories was observed in these paths. Lockfile digest streaming in
`verify_lockfile` has no explicit byte or elapsed-time bound inside that helper.

The package proxy admits at most 16 workers, bounds headers to 16 KiB and DNS
answers to 32, defaults header/DNS/connect deadlines to five seconds and
inactivity to 30 seconds, and caps emitted diagnostics. It preserves buffered
handshake bytes, full-duplex traffic and half-closes
(`galla/src/enforcement/transport.rs:13–19,108–128,366–468,539–795`).

Plain HTTP GET/HEAD authorization includes the visible path. HTTPS CONNECT
authorizes an authority and then relays opaque bytes: it cannot inspect HTTPS
paths, downloaded contents, or Git revisions. Resolved addresses are checked
before connecting. The single DNS helper uses uninterruptible standard-library
resolution and is deliberately not joined during shutdown; caller waits remain
bounded (`transport.rs:204–288,629–690`).

Actual runner trials for native registry consumption, local fetch, named aliases
and Git-index consumption are explicitly ignored and require installed
prerequisites (`native_cache.rs:436,688`; `native_cache/registry_aliases.rs:171`;
`native_cache/git_registry.rs:165`).

## Maerg

Rust defaults to native preparation. Missing material requires `populate`;
configured acquisition/admission uses the sandbox without host fallback. An
explicit `vendored_dir` selects separate directory-export preparation. With a
sandbox, native acquisition is followed by explicitly selected offline host
Cargo repacking into the export destination
(`maerg/src/vendor_command.rs:294–453`).

`vendor_project_protected` always refuses an unbound model dependency request,
including when unrelated locked material is ready, without filesystem
inspection. Its separate readiness helper validates policy format and approved
runner identity, permits Rust only, and enforces existing-source admission.
Noncanonical readiness can execute offline metadata and stage private resolver
inputs, rather than being universally process-free (`vendor_command.rs:123–216`).

Configured preparation resolves installed tooling, re-identifies/probes runner
and backend, stages a private workspace, and checks both staged and original
lock digests afterwards. Metadata execution is capped at 30 seconds and
available remaining budget; fetch uses the project vendor timeout. Cleanup
removes owned administrative staging, not native downloads
(`vendor_command.rs:598–786,856–884`).

Native snapshots follow bounded local/workspace manifest relationships and
select metadata plus target files. Manifests/locks retain bytes; declared and
automatic target files become empty placeholders. `.cargo` configuration,
unrelated deep source trees and unrelated sockets are excluded. Selected
links/special files, escaping paths and changed fingerprints fail. Discovery
bounds include 128 crates, 100,000 tracked entries, 32 MiB path bytes and depth
128; the root lock uses the configured lock-byte bound
(`maerg/src/sandbox/cargo_context.rs:474–641,740–1162`). Acquisition applies Git
approval checks; admission does not convert cached Git consumption into
acquisition permission.

Go selects explicit `GOMODCACHE` or queries `go env GOMODCACHE`; rejects
traversal, non-directory leaf nodes and broad ancestors. Literal
`go mod download` uses local tooling, readonly module flags and disabled
workspaces. `go.mod` and `go.sum` jointly determine identity; acquisition drift
fails while retaining downloads (`maerg/src/vendor_command/go_cache.rs:21–205,318–441`).

npm runs `ci`; Yarn and pnpm run frozen installs into `.kvist/vendored-js`.
pnpm does not copy the global store. Offline verification runs `node --test`,
relying on installed project modules rather than reinstalling from the store
(`vendor_command.rs:979–1020,1315–1332`; `language_verification.rs:225–237`).

uv-only projects export locked requirements; pip downloads distributions, then
uv or stdlib venv creation precedes offline installation. Missing optional uv
differs from an installed but failing uv executable
(`vendor_command.rs:1023–1154,1280–1312`).

Generic host tools have project time/output limits, no automatic retries,
retained partial material and sanitized/redacted diagnostics capped at 4096
bytes (`vendor_command.rs:810–854,888–969`).

Offline language verification calls enforcement with automatic acquisition
disabled, mounts dependency resources read-only and creates disjoint writable
scratch. Go disables proxy/checksum networking and automatic toolchain
acquisition; its compiler and standard library must reside under bound system
prefixes (`language_verification.rs:49–127,166–224`).

Go inspection covers manifest-declared requirements and metadata presence, not
every archive/source byte or proven transitive completeness. Other non-Rust
readiness is coarser: JavaScript checks nonempty store, Python
distribution-directory/interpreter presence, and C/C++ package-directory
presence. JavaScript readiness does not itself check `node_modules`; current
lock digests identify mounts without persisted package-by-package correspondence
(`maerg/src/language_vendoring.rs:473–689,694–876`).

Python/npm/Conan generated configuration uses guarded atomic replacement and
avoids rewriting identical bytes. vcpkg configuration instead directly
recreates/writes its file; the shared helper's unchanged-content and special/link
guards are not used (`language_vendoring/config.rs:47–128`;
`language_vendoring.rs:879–899`).

## Sav

`offline_rust` provides fixed initialization and wrapper scripts; callers own
validation, mounting and execution authority. Initialization copies only rustup
settings into private temporary administration and links read-only
installations/native registry/Git roots (`sav/src/offline_rust.rs:1–24`).

Protected wrappers inject offline/locked Cargo resolution; workspace wrappers
inject offline only. An optional fixed runtime vendor-config file remains
consumable, but wrappers do not generate source replacement or choose host cache
paths. The scripts themselves provide no subprocess supervision or installation
authority (`offline_rust.rs:58–132`).

## Skott

Explicit `--vendor` preparation is independent of model configuration and uses
host operator authority. `--check` is acquisition-free: canonical catalogue
checks are process-free; Git/alternate checks disclose offline host metadata,
including possible administrative writes or cached extraction. Preparation
requires an existing bounded lock; `--offline` selects Cargo offline mode, not
OS isolation (`skott/src/dependencies.rs:15–94`; `skott/src/cli.rs:28–38`).

Workspace Rust preparation instead exposes installed tooling and live native
data read-only. Cache-selector and data overlap with writable workspace authority
is rejected. Catalogue errors become diagnostics and can still expose safely
inspected existing roots, allowing offline Cargo to diagnose missing sources
without disabling installed/local tools
(`skott/src/rust_environment/native.rs:8–87`).

Resources share ownership of private wrapper/settings staging. Sandbox
environment uses private HOME/Cargo/rustup administration, disables
installation/network resolution, and persists targets under `/workspace/target`.
Workspace wrappers do not force locked resolution. Native data is granted
through authoring Toolchain resources, not Galla's native acquisition mode
(`skott/src/rust_environment.rs:460–653,735–862`).

Live refresh compares selected topology fingerprints, reuses unchanged
preparation, and rebuilds after relevant observed changes. Failure preserves
independent tools and does not replay prior operations. Observations cover
root/selector/tool topology, not recursive cached-package contents; these are
live views without source leases or immutable content assertions
(`skott/src/rust_environment/live.rs:25–184`).

## Hermetic resolver source supplement

Fresh source-only observer `721f6eeb-1581-42b9-afd0-6eeadbe74b50` inspected only
current transport source/tests, excluding all intent, previous records/reviews
and history. It executed no checks. Its supplement follows.

In `galla/src/enforcement/transport.rs:294–308`, `start_with_limits` selects
`native_address_lookup`; `start_with_lookup` accepts a function-pointer
substitute. Native lookup uses `ToSocketAddrs`, collecting at most 33 results.
Shared normalization rejects empty or greater-than-32 lists before sorting and
deduplicating (`260–275`). Request host spelling is passed unchanged; IP
literals bypass the lookup queue (`239–256`).

`Resolver` creates one helper thread and a bounded synchronous queue sized to
the client limit. Admission is nonblocking; unsuccessful admission becomes an
upstream failure. Expired queued requests are skipped, while callers poll with
deadline/cancellation checks. Native lookup itself is not interrupted, and the
helper is not joined (`209–292`, `362–366`).

`handle_client` authorizes the requested target, resolves it, requires every
returned IP to satisfy address authorization, and connects using the resulting
numeric addresses, without another hostname lookup. CONNECT sends 200 before
bidirectional relay; relay preserves already-read payload, uses bounded buffers,
propagates half-closes, and checks cancellation/inactivity (`connect_upstream`,
`handle_client`, `Direction::pump`, `relay`).

In `transport_tests.rs:110–138`,
`registered_loopback_name_resolves_then_relays_to_pinned_address` injects a
lookup asserting `"localhost"` and returning loopback with the supplied port.
It asserts a 200 prefix and exact server-record bytes; its joined helper asserts
exact client-record bytes (`78–86`). These are synthetic bytes, not a validated
TLS handshake. The test substitutes native resolution and does not assert
lookup count, rebinding resistance, or normalization boundaries.

No checks were executed; this observation neither reports tests passing nor
certifies intent or security.

The same source-only observer subsequently re-observed `resolve_addresses`:
lookup errors map to `Failure::Io("DNS lookup", error)`, retaining the original
`io::Error`; display includes its message and HTTP status remains 502.
`resolver_failure_preserves_native_cause` injects `PermissionDenied` and asserts
the supplied message survives formatting, without exercising native DNS,
asserting error-kind preservation or inspecting client responses.
The observer executed no checks and did not establish the attributed earlier
failure or later test success.
