<!-- kvist-requirements-version: 1 -->

# Shared Dependencies Requirements

The uppercase normative terms follow BCP 14. This library owns reusable
dependency catalogue, provisioning and offline-readiness mechanics, not human
approval, model tools, canonical evidence or sandbox grants.

## DEP-REQ-RUST

Rust provisioning MUST consume an existing bounded regular non-link Cargo.lock,
retain its exact identity, and invoke Cargo with `--locked`. It MUST NOT resolve
or update a new lockfile. Checks MUST be read-only and network/process-free.
Unsupported language inputs MUST fail explicitly; no all-language runtime
support follows from this first Rust backend.

## DEP-REQ-PUBLICATION

Provisioning MUST stage new material separately, validate it, recheck the lock,
and publish without exposing partial new material as ready. Concurrent library
writers MUST be excluded by a project-local operation lock. Interruption or
failure MUST retain the last published registry and report recovery state
truthfully. No automatic package-manager retry or authority fallback is allowed.
Publication across registry, resolver and manifest is not a multi-file atomic
transaction: intermediate states MUST fail readiness rather than look complete.
State MUST remain inspectable and compatible with the common `.kvist` layout.

## DEP-REQ-VALIDATION

Model/filesystem input MUST be treated as untrusted. Catalogue identities,
package names/versions, manifest paths, regular files and checksums MUST be
validated with explicit bounds; traversal, symbolic links and special files
MUST fail without following them. Full readiness MUST bind vendored packages to
locked registry checksums, validate content digests and distinguish missing,
stale and malformed material. Cheap legacy catalogue observations MUST NOT be
represented as full integrity validation.

`VendorManifest::assert_ready` MUST use full managed readiness, including
publication/operation fences, current manifest/resolver coherence and package
file integrity. The explicitly cheap `report` API remains an observation only.
Tests MUST distinguish those APIs on interrupted publication and corruption.

## DEP-REQ-EXECUTION

Host provisioning is an explicitly authorized acquisition capability, not a
sandbox. It MUST use literal argv, bounded supervised output/time/cancellation,
and no replay. Offline provisioning MUST ask Cargo for offline resolution;
this flag is not OS network isolation. Host configuration/source policy remains
the caller/operator's authority. Raw subprocess output MUST NOT leak into
diagnostics without caller-controlled redaction.

## DEP-REQ-CONSUMERS

Kvist and Skott MUST share one Rust provisioning/readiness implementation.
Existing protected-task refusal and enforcement remain caller-owned.
The library boundary SHOULD support future language-specific offline backends
without duplicating authority or introducing speculative provisioning support.
Tests MUST cover failed publication, concurrent writers, malicious inputs,
integrity failures, lock drift, process failures and native offline consumers.

This human-requested consolidation has an explicit advisory-review exception
while receipts are unavailable; independent evidence is not acceptance.
