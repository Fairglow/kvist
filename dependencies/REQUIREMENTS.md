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

Explicit directory export MUST stage new material separately, validate it,
recheck the lock, and publish without exposing partial new material as ready. Concurrent library
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
Explicit export readiness MUST bind the locked graph to the receipt/resolver,
reject publication/operation fences and missing package/native metadata, and distinguish
missing, stale and malformed state. It MUST NOT traverse/hash package contents
or attest all file checksums. Cargo's required native metadata remains Cargo-owned.
`VendorManifest::assert_ready` MUST use this coherent lightweight gate.

## DEP-REQ-SHARED-AREA

Explicit export consumers MAY safely reuse one project `.kvist/vendored` tree
without a per-session package copy. Export readers MUST prevent shared-library
publication from replacing/deleting the tree while used. Idle sessions MUST NOT hold reader locks.
Existing sandbox write exclusion MUST remain; lock/directory drift requires
explicit restart rather than replay. No persistent reader inventory is required.

## DEP-REQ-LIMITS

Vendor package/tree/byte/path/depth/metadata limits and acquisition output/time
budgets MUST be configurable through one optional shared project policy with
defaults. Invalid/unknown settings MUST fail explicitly. Ready export material
and native material with trustworthy cheap source mapping MUST be reused without
invoking Cargo or repeatedly scanning source files. Provisioning of directory
exports MUST enforce selected tree quotas without producing a content hash inventory.
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

## DEP-REQ-NATIVE-CACHE

Default Rust offline preparation MUST reuse the operator-selected native Cargo
home's registry index, archives, expanded sources and Git state without
vendoring, copying, inventorying, hashing or synchronizing package contents.
An explicit absolute `CARGO_HOME` takes precedence over absolute `HOME/.cargo`;
neither receipts nor project configuration grants host cache paths.
Catalogue inspection MUST be process/write/network-free and bounded to locked
packages and native metadata. It MUST distinguish missing material from unsafe
paths and unsupported trustworthy source mapping. Alternate registry and Git
origins MUST NOT be certified from name/version or invented Cargo cache hashes.
Native mode MUST accept distinct sources with identical package name/version.

## DEP-REQ-NATIVE-PROVISION

Native provisioning MUST hold a process-owned project advisory lock; a retained
unowned lock file MUST remain reusable. Complete cache material MUST bypass
acquisition, even with stale legacy vendor receipts, interrupted locks or stages;
native operations MUST NOT remove or steal those legacy artifacts.
Acquisition MUST run at most once, only for absent/incomplete sources. Failures
MUST retain useful fetched/extracted cache material for recovery. Native Cargo
offline validation MAY establish exact source mapping when cheap inspection
cannot; unsupported mapping MUST fail explicitly, never return false readiness.
Recheck exact lock identity before reporting success. Persist only a small atomic
schema/version-and-lock-digest receipt at `.kvist/cargo-cache.json`; do not record
source inventories, producer paths, credentials or checksum attestations.
Cargo acquisition MUST coordinate its own writer/GC locks; the project lock
does not pin or freeze the shared host cache against legitimate host mutation.
Tests MUST cover warm reuse with legacy state, missing-only acquisition,
failure persistence, lock drift, unsafe roots, source identity, minimal receipts,
project lock ownership and an actual native offline Cargo consumer.

## DEP-REQ-NATIVE-ADMISSION

Native preparation and explicit readiness admission MUST support every valid
locked registry/Git source through actual bounded offline/locked Cargo resolution
when process-free catalogue mapping is unavailable. CLI readiness MUST have
current catalogue or actual resolver evidence, not merely a historical receipt.
Source-only runtime views MAY defer resolution to the actual offline Cargo
consumer, but MUST represent that obligation explicitly rather than inventing
package presence. A missing project lock MUST NOT prevent source-only runtime
access to safe operator-selected native roots. Matching minimal receipts record
only a prior lock admission, never present source provenance or immutable state.
Configured admission MUST use only the caller's approved native data endpoints
and private administrative configuration, with no host-config/executable fallback.
Cold native data-directory absence MAY establish missing sources directly.
Failed configured metadata MUST permit acquisition only on an observed Cargo
failure and a recognized offline missing-source diagnostic tied to the locked
graph; arbitrary nonzero exits, timeout, I/O, configuration and lock failures
MUST NOT be reclassified as missing. Request-bound private-home path aliases MAY
be normalized to fixed native endpoints, never inferred from subprocess output.
