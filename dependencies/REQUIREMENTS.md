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
Publication across registry, resolver and receipt is not a multi-file atomic
transaction: intermediate states MUST fail readiness rather than look complete.
State MUST remain inspectable and compatible with the common `.kvist` layout.

## DEP-REQ-VALIDATION

Model/filesystem input MUST be treated as untrusted. Catalogue identities,
package names/versions, metadata paths and regular files MUST be
validated with explicit bounds; traversal, symbolic links and special files
MUST fail without following them during provisioning and metadata inspection.
Readiness MUST bind the locked graph to the receipt/resolver, reject
publication/operation fences and missing package/native metadata, and distinguish
missing, stale and malformed state. It MUST NOT traverse/hash package contents
or attest all file checksums. Cargo's required native metadata remains Cargo-owned.
`VendorManifest::assert_ready` MUST use this coherent lightweight gate.

## DEP-REQ-SHARED-AREA

Kvist and Skott MUST safely reuse one project `.kvist/vendored` tree without a
per-session package copy. Readers MUST prevent shared-library publication from
replacing/deleting the tree while used. Idle sessions MUST NOT hold reader locks.
Existing sandbox write exclusion MUST remain; lock/directory drift requires
explicit restart rather than replay. No persistent reader inventory is required.

## DEP-REQ-LIMITS

Vendor package/tree/byte/path/depth/metadata limits and acquisition output/time
budgets MUST be configurable through one optional shared project policy with
defaults. Invalid/unknown settings MUST fail explicitly. Ready material MUST be
reused without invoking Cargo or repeatedly scanning its files. Provisioning
MUST enforce selected quotas without producing a content hash inventory.
The receipt MUST contain only schema version and lock digest, not host paths,
timestamps, per-file checksums or a full-verification claim.

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
metadata failures, configurable bounds, reader/writer exclusion, lock drift,
process failures and native offline consumers.

This human-requested consolidation has an explicit advisory-review exception
while receipts are unavailable; independent evidence is not acceptance.
The 2026-11-07 user-directed simplification uses the same explicit local
advisory exception; earlier full-attestation/copy intent is deliberately replaced.
