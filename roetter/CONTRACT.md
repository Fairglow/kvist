<!-- kvist-contract-version: 1 -->

# Shared Dependencies Contract

`kvist-roetter` is a Linux headless Rust library. It imports no maerg or
Skott types. Sav's process supervisor is an explicitly declared provider
(`sav/CONTRACT.md`); the caller owns execution authority. No command is exposed
as a model tool.

## Rust API

### Native offline cache (default preparation)

`rust::cache::home() -> Result<PathBuf>` selects explicit absolute `CARGO_HOME`,
otherwise absolute `HOME/.cargo`. Invalid explicit selection fails rather than
falling back. These paths are operator authority, never receipt-derived grants.

`rust::cache::view(project, home, cancellation) -> Result<CacheView>` performs
bounded, process/write/network-free native catalogue inspection.
`CacheView` exposes `cargo_home: PathBuf`, `registry: Option<PathBuf>`,
`git: Option<PathBuf>` and `report: VendoringReport`. Registry/Git paths are only
the selected home's native subdirectories, not a whole-home mount instruction.
The `registry` and `git` selectors may be symbolic links to shared native data.
Returned data paths are canonical directories suitable for no-follow mounts,
never the unresolved selectors. Filesystem root, the selected Cargo home and
its ancestors are refused as data targets. Inspection beneath a selected data
root still rejects links; native metadata may use either the exact home selector
prefix or its resolved data-root prefix, never an unrelated source location.
Absent data roots, or an absent home below a safe existing parent, are valid
catalogue observations: external entries are missing and local-only graphs need
no external cache. Inspection does not create those paths.
Catalogue presence is not content integrity, immutable readiness or source
provenance attestation. Cargo's sparse crates.io metadata, locked checksums and
extraction completion markers permit a cheap canonical catalogue. Sources whose
native identity cannot be established cheaply return `UnsupportedCacheSource`;
alternate registry/Git name/version matches never imply readiness.

`rust::cache::admit(project, home, &ProvisionOptions, cancellation)
-> Result<CacheView>` is an explicitly process-authorized current readiness check:
canonical catalogues remain process-free, while otherwise unsupported source
mapping uses bounded native `metadata --offline --locked --format-version 1
--all-features`. Exact locked source identities and manifest locations in the
selected native roots are validated. It does not acquire missing sources or
mint a receipt; a returned missing report is not readiness.

`rust::cache::sources(project, home, cancellation) -> Result<SourceView>` returns
only safe runtime data selection without processes or writes, even with no lock.
`SourceView` has `cargo_home`, optional `registry`/`git`, optional
`lockfile_digest`, and `admission: SourceAdmission`:
`Catalogue(VendoringReport)` carries a current canonical catalogue observation;
`CargoOfflineRequired { recorded_lock_admission: bool }` requires actual offline
Cargo resolution and never claims current presence; `NoLock` similarly delegates
the writable runtime lock to Cargo. `recorded_lock_admission` observes only a
matching minimal prior native receipt, not cache origin, paths or current readiness.
Malformed/unsafe existing locks and any consulted receipt remain explicit errors;
canonical catalogue selection does not consult irrelevant receipts.

`rust::cache::admit_with(project, home, cancellation, validate)
-> Result<CacheView>` provides configured admission without host fallback.
For unsupported cheap mappings the once-only callback receives the selected home
and returns actual bounded native Cargo metadata stdout from an independently
authorized private-admin invocation. Metadata paths must use fixed
`/workspace/cargo-registry/src` or `/workspace/cargo-git/checkouts`; these are
mapped only to the selected home's corresponding native data roots. No arbitrary
path or source mapping is accepted from metadata. The callback must run literal
offline/locked metadata argv with the project's policy budgets and redacted-safe
failure handling. This API grants no subprocess/sandbox authority itself.

`rust::cache::provision_with_validation(project, home, cancellation, force,
acquire, validate) -> Result<CacheView>` supplies the same configured validator
to preparation. Both callback roots exist before validation/acquisition.
`validate: FnMut(&Path) -> Result<Vec<u8>>` returns native sandbox metadata as
above; an explicitly classified `VendoringIncomplete` offline lookup failure
permits the once-only acquire callback, then metadata is checked again. Other
validator errors propagate without acquisition. Canonical warm reuse invokes
neither callback. This is the configured replacement for `provision_with`'s
fixed-host validation fallback, not a portable-export fallback.
Absent required native directories (`registry/index`, `registry/cache`,
`registry/src`, `git/db`, `git/checkouts`) establish missing external sources
without a speculative failed metadata call. Safe directory presence alone never
establishes source readiness.

`rust::cache::classify_metadata_failure(project, failure: ProcessFailure,
exit_code: Option<i32>, stderr: &[u8], cancellation) -> Error` is the precise
configured callback failure adapter. It applies only to an independently observed
native `metadata --offline --locked --format-version 1 --all-features` invocation.
Only `ProcessFailure::Exit`, Cargo exit 101, and an anchored offline missing
registry package/archive or exact locked Git URL/revision chain yield
`VendoringIncomplete`; warning text, unrelated packages/sources, unknown exits,
time/output/I/O/enforcement failures remain `NativeCargo`. Diagnostic inspection
is bounded to 64 KiB; larger/invalid UTF-8 diagnostics cannot classify missing.
Native errors retain the reported `exit_code`, failure class and safe guidance,
not subprocess text. Cancellation remains `Error::Cancelled`.

`rust::cache::normalize_metadata_paths(project, cargo_home, stdout, cancellation)
-> Result<Vec<u8>>` normalizes only the exact private administrative Cargo home
from the approved request, mapping its `registry/src` and `git/checkouts` manifest
prefixes to the fixed native sandbox endpoints. The home argument is caller
authority, never extracted from JSON or a receipt. Unrelated aliases, traversal,
malformed/over-budget output and unsupported sources fail. Output remains native
package metadata for `admit_with`/`provision_with_validation` validation, not a
persisted inventory or standalone readiness claim.

`rust::cache::provision_with(project, home, cancellation, force, acquire)
-> Result<CacheView>` passes the live selected Cargo home to a once-only
`FnOnce(&Path) -> Result<()>` callback only when material is missing/incomplete.
Complete warm material bypasses it, including with stale legacy vendor state.
`force` refreshes the native receipt, not otherwise complete cached packages.
Native offline Cargo metadata validation establishes exact locked source and
manifest-path mapping when the cheap catalogue is unsupported; this bounded
fallback uses fixed host `/usr/bin/cargo`, with project policy budgets. A failed
offline source lookup may request acquisition once; unsafe/unmapped native
sources fail explicitly. The callback must coordinate Cargo's native writer/GC
locks (use Cargo itself), honor locked acquisition, and never erase useful
material on failure.

`rust::cache::populate(project, home, &ProvisionOptions, cancellation)
-> Result<()>` runs bounded literal native `cargo fetch --locked`, adding
`--offline` when selected and setting only operator-owned `CARGO_HOME`.
`rust::cache::provision(project, ProvisionOptions, cancellation)
-> Result<CacheView>` is the host-authorized native entry point for
`vendored_dir: None`; an explicit destination uses the directory export API below.
Its native-validation fallback uses the selected host Cargo executable.
Native process errors retain safe exit status and I/O cause where available,
alongside failure class and payload-free repair guidance; raw streams are excluded.

`rust::cache::acquisition_lease(home, cancellation) -> Result<AcquisitionLease>`
is an optional Linux host guard for acquisition using an isolated administrative
Cargo home with shared data roots. It acquires Cargo's retained native
`.package-cache-mutate` then `.package-cache` locks exclusively, in native order,
and fails explicitly if busy or locking is unsupported. Retain it only around
the isolated subprocess; do not call host Cargo with the same home while holding
it (that Cargo would wait for these locks). It coordinates cooperating native
writers/GC during that interval, not arbitrary host deletion or older readers.
`AcquisitionLease::check(cancellation)` rechecks the anchored home/lock identities.

Native provision holds `.kvist/cargo-cache-operation.lock` by process advisory
ownership, not file existence, and never deletes the retained lock file.
`.kvist/cargo-cache.json` contains only `schema_version` and `lockfile_digest`;
its presence alone is not cache readiness. Stale native receipts can be refreshed
after current native validation; malformed receipts fail explicitly.
Legacy export locks, stages and receipts are untouched. Host cache GC/mutation
is legitimate; private project locks do not freeze or pin it.

### Explicit directory export

`rust::check(project, cancellation)` returns a lightweight coherent Rust
readiness report or a typed failure, reading no application configuration and
starting no subprocess. `rust::check_catalogue(project, vendor, cancellation)`
checks package/native-metadata presence against Cargo.lock in a supplied view
without requiring an application manifest.

`rust::check_view(project, vendor, cancellation)` returns the lightweight
readiness report and optional safe source resolver for a caller-owned immutable
read-only vendor view. Managed receipts, operation/recovery fences and resolver
source binding are checked without reading package contents.
Valid crates.io catalogues without managed control artifacts
remain usable. This is the runtime consumer's coherence gate, not a
provisioning callback validator. Cargo's native checksum files are required but
their file inventory is not independently rehashed or attested.

`rust::read_lease(vendor, cancellation)` opens a non-link directory and holds a
shared advisory lock until its returned owner drops. Busy publication fails
explicitly; no operation lock is stolen. Shared-library publication takes an
exclusive lock on the current directory before replacement and retirement.
Callers retain the read owner through mount preparation and subprocess completion.

`rust::provision(project, options, cancellation)` performs explicit directory
host Cargo vendoring. `ProvisionOptions` contains an absolute host Cargo
executable, `offline`, optional vendored destination, timeout and explicit `force` rebuild.
`rust::provision_with(project, destination, cancellation, force, populate)` provides
the same transaction to caller-owned acquisition backends; `populate` receives
the private staged vendor path and returns Cargo's source-replacement TOML.
It is invoked at most once and not for matching complete metadata unless forced.
Backends must honor the lockfile and their independently selected authority.

`rust::populate(project, destination, options, cargo_home, cancellation)`
exposes the same bounded Cargo-vendor process for an explicit acquisition
adapter's offline repack. The optional cache is an absolute caller-owned Cargo
home, passed as literal environment selection via the fixed `/usr/bin/env`
executable, never through a shell.
Successful empty Cargo-vendor stdout is valid only for a locked graph without
external packages; the helper generates the standard empty-registry resolver.
An external graph still requires complete explicit source-replacement tables.

The common host paths are `.kvist/vendored`,
`.kvist/vendor.json`, `.kvist/sandbox-cargo/config.toml`, and a private
`.kvist/vendor-operation.lock`. The lock is exclusively held for a provisioning
operation. A process interruption can leave operation/publication state requiring
explicit reconciliation; a stale lock must never be stolen implicitly.
Custom destinations are explicit operator authority, not manifest-derived grants.
Checks derive default storage from the supplied project, not arbitrary
recorded host paths.

The receipt (`VendorManifest`) contains only `schema_version` and
`lockfile_digest`. It has no producer paths, timestamp, file inventory or
full-verification flag. Retired receipt fields/formats are not recognized.
`VendorLimits::load(project)` reads the optional `.kvist/vendor.toml` using
the native TOML 1.1 parser with a single `[limits]`
table matching `VendorLimits`: package/entry counts, total/file/path bytes,
source-table count, depth, lockfile/metadata/output bytes and timeout seconds. Omitted fields use
defaults; unknown/invalid values fail. Its parser has a separate 1 MiB input
bound, not a package-tree quota. Tree quotas apply to provisioning/cleanup;
readiness does not rescan package files to retest a lowered quota.

Published resolver content permits source-replacement tables only and supports
the locked sources emitted by Cargo. `rust::resolver_config(config, destination)`
retargets these safe tables to the caller's fixed sandbox-native vendor path.
No Cargo alias, build/environment setting, credential or host execution path
is taken from this generated resolver.

## Existing catalogue API

`vendoring` consolidates the former maerg Rust catalogue/report/manifest types,
constants and helpers. Catalogue presence observations preserve their existing
name/version semantics for maerg callers, but do not prove file integrity or
exact Git provenance. `Error` retains unavailable/incomplete/stale/I/O domains
for frontend adaptation. Newly provisioned material receives bounded nonfollowing filesystem inspection,
native package metadata presence checks and safe resolver/lock binding.

`VendorManifest::report` remains a cheap catalogue observation.
`VendorManifest::assert_ready` checks the caller's lock binding and full current
managed readiness through `rust::check`; it rejects active/interrupted
publication and incomplete receipt/resolver/package metadata.
Consumers must not substitute catalogue presence for this gate.
`vendoring::remove_stale_vendored_packages(vendor, names, limits)` takes explicit
quotas and exclusive directory ownership for bounded nonfollowing cleanup.
`rust::resolver_config_with_limits(config, destination, limits)` applies loaded
policy during retargeting; the context-free convenience API uses default limits.

## Failure and authority

Missing locks, malformed/unsupported metadata, unsafe paths, lock drift,
occupied operation locks and failed processes are explicit errors, never a
successful empty registry. Publication errors identify whether new material was
published and require inspection rather than replay. Host Cargo may read operator
configuration and contact configured sources unless offline is requested; the
library does not grant that authority to agent execution.

Readiness is bound to this locked graph, not a promise that arbitrary build
scripts, system libraries, tools, local path dependencies outside the workspace
or a different language can execute offline.
Checks establish lock/metadata coherence and package presence, not content
integrity or origin attestation. Cargo reports source/checksum failures when
consumed. Directory-export `force` explicitly rebuilds otherwise reusable
material. Native `force` refreshes the small receipt; it does not guarantee
reacquisition or repair of apparently complete damaged native material.


## Boundary and ownership

`kvist-roetter` is a Linux headless Rust library. It imports no maerg or
Skott types. Sav's process supervisor is an explicitly declared provider
(`sav/CONTRACT.md`); the caller owns execution authority. No command is exposed
as a model tool. The library owns dependency catalogue, provisioning and
offline-readiness mechanics; it owns no human approval, model tool, canonical
evidence or sandbox grant. It owns the `.kvist` vendored/cargo-cache state it
publishes and the small receipts it writes, and it reads the operator-selected
Cargo home and Cargo.lock.

## Provided interfaces

The Rust API is the provided interface. The native offline cache is the default
preparation path: `rust::cache::home`, `view`, `admit`, `sources`, `admit_with`,
`provision_with_validation`, `classify_metadata_failure`,
`normalize_metadata_paths`, `provision_with`, `populate`, `provision` and
`acquisition_lease`. The explicit directory export path provides `rust::check`,
`check_catalogue`, `check_view`, `read_lease`, `provision`, `provision_with`
and `populate`, plus the shared `vendoring` catalogue/report/manifest types.
These operations are consumed by Kvist and Skott to obtain a coherent offline
readiness gate and to publish a reusable vendor tree.

## Required interfaces

Sav (`sav/CONTRACT.md`) provides the process supervisor used for bounded host
Cargo execution; the caller owns execution authority and the library never
grants it to agent execution. No other cross-component interface is required.
The library requires no maerg or Skott types.

## Data and schemas

The published receipt (`VendorManifest`) contains only `schema_version` and
`lockfile_digest`; it has no producer paths, timestamp, file inventory or
full-verification flag. The native receipt at `.kvist/cargo-cache.json` contains
only `schema_version` and `lockfile_digest`. The optional shared policy at
`.kvist/vendor.toml` is a single `[limits]` table matching `VendorLimits`. The
published resolver content permits source-replacement tables only. No
machine-readable IDL file is authoritative; this document is authoritative for
semantics.

## Behavioral guarantees

Inspection is bounded, deterministic and process/write/network-free. Catalogue
presence is not content integrity, immutable readiness or source provenance
attestation. Missing locks, malformed or unsupported metadata, unsafe paths,
lock drift, occupied operation locks and failed processes are explicit errors,
never a successful empty registry. Publication errors identify whether new
material was published and require inspection rather than replay. Readiness is
bound to the locked graph, not a promise that arbitrary build scripts, system
libraries, tools or a different language can execute offline.

## Errors and failure semantics

The `Error` type retains unavailable/incomplete/stale/I/O domains for frontend
adaptation. `UnsupportedCacheSource` is returned for sources whose native
identity cannot be established cheaply. `VendoringIncomplete` is the classified
offline missing-source diagnostic that permits the once-only acquisition
callback. Cancellation remains `Error::Cancelled`. Unknown exits, timeout, I/O,
configuration and lock failures are never reclassified as missing source.

## Security and authority

Model and filesystem input is treated as untrusted. Catalogue identities,
package names/versions, metadata paths and regular files are validated with
explicit bounds; traversal, symbolic links and special files fail without being
followed during provisioning and metadata inspection. Filesystem root, the
selected Cargo home and its ancestors are refused as data targets. The library
does not grant host configuration or source authority to agent execution; the
caller/Galla owns sandbox command authority.

## Compatibility and verification

The library boundary supports future language-specific offline backends without
duplicating authority or introducing speculative provisioning support. Contract
versioning is carried by the `kvist-contract-version` marker; this is version 1.
Verification is the component's Rust test suite, which covers failed
publication, concurrent writers, malicious inputs, metadata failures,
configurable bounds, reader/writer exclusion, lock drift, process failures and
native offline consumption, plus the native cache root, admission and
provisioning tests described in `IMPL.md`.
