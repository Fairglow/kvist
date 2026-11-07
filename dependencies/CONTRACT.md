<!-- kvist-contract-version: 1 -->

# Shared Dependencies Contract

`kvist-dependencies` is a Linux headless Rust library. It imports no maerg or
Skott types. Sav's process supervisor is an explicitly declared provider
(`sav/CONTRACT.md`); the caller owns execution authority. No command is exposed
as a model tool.

## Rust API

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

`rust::provision(project, options, cancellation)` performs explicitly selected
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
consumed. `force` explicitly rebuilds otherwise reusable material after damage.
