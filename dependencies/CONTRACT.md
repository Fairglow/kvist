<!-- kvist-contract-version: 1 -->

# Shared Dependencies Contract

`kvist-dependencies` is a Linux headless Rust library. It imports no maerg or
Skott types. Sav's process supervisor is an explicitly declared provider
(`sav/CONTRACT.md`); the caller owns execution authority. No command is exposed
as a model tool.

## Rust API

`rust::check(project, cancellation)` returns a full integrity-checked Rust
readiness report or a typed failure, reading no application configuration and
starting no subprocess. `rust::check_catalogue(project, vendor, cancellation)`
validates the existing Cargo.lock against a caller-owned immutable vendor view
without requiring an application manifest.

`rust::check_snapshot(project, vendor, cancellation)` returns the full
readiness report and optional safe source resolver for a caller-owned immutable
vendor copy. Managed manifests, operation/recovery fences and resolver source
binding are checked against the current lock without rehashing the mutable
original. Valid legacy crates.io catalogues without managed control artifacts
remain usable. This is the runtime consumer's coherence gate, not a
provisioning callback validator.

`rust::provision(project, options, cancellation)` performs explicitly selected
host Cargo vendoring. `ProvisionOptions` contains an absolute host Cargo
executable, `offline`, optional vendored destination, and a bounded timeout.
`rust::provision_with(project, destination, cancellation, populate)` provides
the same transaction to caller-owned acquisition backends; `populate` receives
the private staged vendor path and returns Cargo's source-replacement TOML.
It is invoked at most once and never for already complete material.
Backends must honor the lockfile and their independently selected authority.

`rust::populate(project, destination, options, cargo_home, cancellation)`
exposes the same bounded Cargo-vendor process for an explicit acquisition
adapter's offline repack. The optional cache is an absolute caller-owned Cargo
home, passed as literal environment selection via the fixed `/usr/bin/env`
executable, never through a shell.
Successful empty Cargo-vendor stdout is valid only for a locked graph without
external packages; the helper generates the standard empty-registry resolver.
An external graph still requires complete explicit source-replacement tables.

The common host paths remain `.kvist/vendored`,
`.kvist/vendoring-v1.json`, `.kvist/sandbox-cargo/config.toml`, and a private
`.kvist/vendor-operation.lock`. The lock is exclusively held for a provisioning
operation. A process interruption can leave operation/publication state requiring
explicit reconciliation; a stale lock must never be stolen implicitly.
Custom destinations are explicit operator authority, not manifest-derived grants.
Full checks derive default storage from the supplied project, not arbitrary
recorded host paths.

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
for frontend adaptation. All newly provisioned material receives the stronger
Rust validation above.

`VendorManifest::report` remains a cheap catalogue observation.
`VendorManifest::assert_ready` checks the caller's lock binding and full current
managed readiness through `rust::check`; it rejects active/interrupted
publication, incomplete resolver/manifest state and corrupt package files.
Consumers must not substitute catalogue presence for this gate.

## Failure and authority

Missing locks, malformed/unsupported inputs, unsafe paths, corruption, lock drift,
occupied operation locks and failed processes are explicit errors, never a
successful empty registry. Publication errors identify whether new material was
published and require inspection rather than replay. Host Cargo may read operator
configuration and contact configured sources unless offline is requested; the
library does not grant that authority to agent execution.

Readiness is bound to this locked graph, not a promise that arbitrary build
scripts, system libraries, tools, local path dependencies outside the workspace
or a different language can execute offline.
File/checksum checks establish consistency with the locked catalogue and
supplied checksum metadata; they do not reconstruct downloaded archive
checksums or cryptographically attest a vendored Git tree's origin.
