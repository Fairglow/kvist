<!-- kvist-design-version: 1 -->

# Shared Dependencies Design

## Native cache preparation

The default backend is `rust::cache`, separate from directory export transactions
below. Reuse native registry index/cache/src and Git db/checkouts in the operator's
explicit Cargo home. Do not compute Cargo namespace hashes: recognize the native
canonical crates.io sparse namespace, inspect its bounded cached index records,
match locked checksums, and check the archive, expanded Cargo.toml and `.cargo-ok`
metadata only. Never walk package files. Missing catalogue material is distinct
from malformed metadata, unsafe paths and sources lacking a trustworthy cheap
mapping. The locked parser retains full source/checksum identity and moves the
directory-export-only name/version collision restriction out of native parsing.

When cheap mapping is unavailable, bounded native `cargo metadata --offline
--locked --format-version 1` establishes exact locked source identities and native
manifest locations. Validate each external metadata identity against the lock and
each path under the selected native registry/src or Git/checkouts tree; external
directory replacements never look like native readiness. Only recognized offline
missing-source diagnostics trigger the once-only acquisition callback. Revalidate
afterwards. Do not publish a catalogue-only success for unsupported sources.
Expose that process-backed path as explicit `admit` for CLI readiness. Keep
`view` strictly cheap and separate source-only runtime selection into a typed
`SourceView`: canonical catalogue, actual offline Cargo resolution still
required (with optional prior lock admission), or no current lock. A receipt
never fills in missing source identity; live runtime consumption still checks
the actual native sources. Local-only graphs require no registry metadata scan.

Configured admission/provisioning accepts actual bounded metadata stdout from a
caller-authorized private-admin callback, never starts host Cargo implicitly.
Factor metadata validation from process execution. Compare every returned full
name/version/source identity against the exact lock, require every external lock
identity observed, and map only the fixed native sandbox endpoint prefixes back
to the selected host roots for bounded nonfollowing manifest checks. Reject
source replacements outside those roots, malformed output, duplicates, lock
drift and over-budget metadata. Only an explicit missing-source failure permits
one acquisition; revalidate afterwards. Sandbox command authority remains the
caller/Galla responsibility; the callback surface does not authorize a new phase.
Before opaque-source resolution, inspect only required native directory roots.
Their absence proves incomplete data and can trigger one acquisition directly;
otherwise use metadata. Public failure adaptation requires observed Cargo exit
101 and anchored native offline error chains tied to locked package identities
or exact Git source URL/revision. Bound complete UTF-8 diagnostics to 64 KiB;
unknown/error-budget/I/O failures never become missing-source evidence. Reuse this
classification for host and configured metadata without reading arbitrary
subprocess text into errors. Preserve reported exit codes and I/O causes.

Cargo may emit manifest paths through its private administrative symlinks. A
separate bounded normalizer receives the exact approved request Cargo home and
maps only that prefix to fixed native endpoints, rejecting all unrelated paths.
The subsequent existing validator still checks full source identities, exact
lock stability and nonfollowing actual host manifest paths. Never infer an
administrative path from metadata, scan sources, or rewrite native JSON in Galla.

Anchor project/home and metadata with existing nonfollowing descriptor operations.
Resolve only existing native `registry`/`git` root selectors relative to the
anchored home, then reopen their canonical targets with ordinary no-follow
directory traversal. Reject root/home/ancestor targets. Return those canonical
paths for consumers, and use them for both cheap catalogue and native manifest
validation; retain nonfollowing inspection beneath each data root. Missing roots
are created as ordinary directories, never replaced. This is live root selection,
not a cache snapshot or authorization for links inside source metadata.
Open a retained bounded regular native operation lock file with no truncation or
links, and use process-held advisory locking. Recheck its path and the exact lock
identity across callback/validation and before a small atomic native receipt write.
Matching warm receipts need no writes; missing/stale receipts can be refreshed
without acquisition. Export interruption state is not native state and is untouched.
`force` refreshes receipt metadata only; it never authorizes redundant downloads.

Acquisition calls Cargo fetch with literal locked/offline argv and bounded Sav
supervision; Cargo owns native cache writer/GC coordination. The project lock
serializes only cooperating project operations, not the global cache. Host GC and
live mutation remain possible; no reader lease, immutable or pinning promise is
made. Callback implementers must use Cargo or equivalent native coordination and
retain live fetched material on errors. Failures expose safe diagnostic categories
and actionable repair guidance, not raw subprocess streams or credentials.
For isolated administrative homes, expose an explicit host acquisition lease
holding Cargo's `.package-cache-mutate` and `.package-cache` exclusive `flock`
locks, in that order (Cargo `MutateExclusive` convention). It is never implicitly
held across ordinary host Cargo calls, which acquire their own locks. All locking
failures remain explicit; no best-effort pinning claim.

## Explicit directory export

Separate cheap catalogue observations from coherent managed Rust readiness.
Keep shared errors frontend-independent. Parse TOML structurally; do not infer
package versions from arbitrary text lines for full validation.
Route receipt readiness through the coherent Rust check after verifying the
caller's expected lock digest; retain `report` as the explicitly cheap
catalogue-only operation. Never treat retained old metadata as permission to
bypass publication or operation fences.

Readiness checks locked external identities, required native-metadata presence
and receipt/resolver binding without visiting or hashing package source files.
Cargo owns its file-checksum inventory and validation. A provisioning-only
descriptor-relative metadata walk enforces configured byte/entry/path/depth
quotas, rejects links/special files and synchronizes staged material; do not
repeat that walk after publication. Retain deterministic ordering. Do not require new material
for source-less local packages or a dependency-free graph.

Acquire a no-clobber operation lock under the validated project state directory.
Use same-filesystem private staging; populate once, validate the staged catalogue
and source config, then compare the lock again before publication. Retain the
old registry in an owned recovery location across rename publication. Replace
resolver and minimal receipt atomically per file; readiness binds the original lock
and must refuse an interrupted multi-file commit. Reuse matching published metadata without invoking Cargo or rewriting files;
an explicit force option rebuilds damaged source when requested. RAII cleanup may remove
only owned staging/locks, not unrelated files; failures after publication must
retain actionable recovery evidence.

Host Cargo execution uses Sav's observed supervisor with retries/loop detection
disabled and bounded time/output. Captured stdout is parsed only as source TOML.
Cargo legitimately emits empty stdout for a local-only graph; generate the
same safe resolver used by empty-graph publication only in that successful case.
Never normalize malformed output or missing source tables for external graphs.
Do not forward raw diagnostic streams. Offline and online host acquisition
are explicit options, never fallbacks for one another. A callback lets the
maerg retain its installed Galla acquisition path without sharing task authority.

Readers take shared advisory locks on the opened vendor directory and recheck
its path identity. Publication takes an exclusive lock before renames and old
tree retirement; busy use is explicit, with no persistent reader records.
Skott uses the same live project vendor tree read-only, retaining a read owner
per tool call, not for an idle session. Its fixed runner mask removes the
writable workspace alias. Cache only directory/lock identity and small generated
runtime/source configuration, never a second package tree or file hash list.
Directory/lock replacement requires restart; direct host edits are not attested.
Load strict optional `.kvist/vendor.toml` once into each operation's locked graph
and pass its limits through all vendor-specific work. No application/model
configuration, implicit fallback or arbitrary receipt-derived paths participate. Future languages can supply separate acquisition and
offline-environment strategies when implemented; this phase adds no fake backend.


## Design overview

The design satisfies the native-cache, native-provision and native-admission
requirements plus the publication, validation, shared-area, limits, execution
and consumer requirements. The governing contract is the Rust API described
above. The design drivers are bounded deterministic inspection, explicit
failure semantics, and reuse of one shared Rust offline preparation and export
implementation across Kvist and Skott. The default backend is `rust::cache`,
separate from the directory export transaction. The alternatives deliberately
left open are future language-specific offline backends, which add no fake
backend in this phase.

## Internal structure

The private modules are `fs`, `integrity`, `limits`, `publication`, `resolver`,
`rust`, `vendoring` and `lib`. `rust` owns the native cache, provision and
admission paths plus the directory export transaction. `vendoring` consolidates
the catalogue/report/manifest types and helpers shared with maerg callers.
`limits` owns the configurable vendor policy. `publication` owns the staged
publish and recovery logic. `resolver` owns the safe source-replacement retarget.
`fs` and `integrity` own bounded nonfollowing descriptor operations and metadata
presence checks. The decomposition keeps cheap catalogue observation separate
from coherent managed readiness, and keeps host process execution separate from
the caller-authorized callback surface.

## Interactions and state

The native cache reads the operator-selected Cargo home and Cargo.lock and
returns a `CacheView` with canonical registry and Git data roots and a
`VendoringReport`. The directory export transaction stages new material in a
private candidate, validates it, rechecks the lock, and publishes atomically
per file while retaining the old registry in an owned recovery location. The
shared `.kvist/vendored` tree is reused by consumers with a shared advisory read
lock; publication takes an exclusive lock before replacement and retirement.
State is the small receipts and the optional shared policy; no persistent reader
inventory is kept.

## Algorithms and decisions

The catalogue inspects the native crates.io sparse namespace, matches locked
checksums, and checks the archive, expanded Cargo.toml and `.cargo-ok` metadata
only, never walking package files. When cheap mapping is unavailable, bounded
native `cargo metadata --offline --locked --format-version 1 --all-features`
establishes exact locked source identities and manifest locations. The
nonfollowing descriptor operations anchor project and home, resolve only the
existing native `registry`/`git` root selectors, and reject root/home/ancestor
targets. The once-only acquisition callback fires only on an observed Cargo
failure and a recognized offline missing-source diagnostic tied to the locked
graph. These choices link to the native-offline-cache ADR.

## Failure and recovery

Missing, malformed, unsafe and unsupported inputs are explicit errors, never a
successful empty registry. A process interruption can leave operation/publication
state requiring explicit reconciliation; a stale lock is never stolen
implicitly. Publication errors identify whether new material was published and
require inspection rather than replay. Retained old metadata is never permission
to bypass publication or operation fences. Recovery retains the last published
registry and reports state truthfully; no automatic package-manager retry or
authority fallback occurs.

## Security and resource design

Model and filesystem input is untrusted. Catalogue identities, package
names/versions, metadata paths and regular files are validated with explicit
bounds; traversal, symbolic links and special files fail without being followed.
Filesystem root, the selected Cargo home and its ancestors are refused as data
targets. Vendor package/tree/byte/path/depth/metadata limits and acquisition
output/time budgets are configurable through one optional shared project policy
with defaults; invalid or unknown settings fail explicitly. The receipt contains
only schema version and lock digest, not host paths, timestamps, per-file
checksums or a full-verification claim.

## Verification strategy

Requirements and contract clauses map to the component's Rust test suite. The
native cache, provision and admission tests cover warm reuse with legacy state,
missing-only acquisition, failure persistence, lock drift, unsafe roots, source
identity, minimal receipts, project lock ownership and an actual native offline
Cargo consumer. The publication and validation tests cover failed publication,
concurrent writers, malicious inputs, metadata failures, configurable bounds,
reader/writer exclusion and lock drift. Independent security and compliance
observations are recorded in `IMPL.md` and are not acceptance in themselves.
