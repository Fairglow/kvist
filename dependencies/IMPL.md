<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Independent native root repair observations - 2026-11-08

Clean-slate `repair-source-observer` read current source and executable
assertions only, excluding intent, queues, prior records/reviews and history.
It executed no tests and made no compliance judgment.

Cargo home comes from explicit `CARGO_HOME`, otherwise `HOME/.cargo`; invalid
explicit selection fails without fallback. Only `registry`/`git` selectors are
canonicalized. Filesystem root, Cargo home and its ancestors are rejected as
targets; subsequent descriptor-relative traversal rejects links. Returned data
paths are canonical directories. Native admission accepts exact home-selector
or resolved-root manifest prefixes on the host; configured admission accepts
only fixed sandbox prefixes. It checks exact locked identities, duplicates,
all external locked packages and regular host manifests.

Observed assertions cover canonical linked roots, unsafe targets/deeper links,
configured endpoint mapping, real cached Git resolution through a linked root,
acquisition reuse, lock drift and receipts that cannot establish present
readiness. Paths are bounded to 32 KiB and segments to 255 bytes; graph policy
bounds package/output/depth inspection. Catalogue checks are not source/archive
integrity attestations; metadata admission checks manifest existence/type, not
contents. The observer found no high-confidence scoped defect.

Separate source-blind `repair-intent-comparator` found correspondence for
root-only canonicalization, unsafe/deeper-link refusal and exact locked-source
admission, with no concrete scoped discrepancy. It inspected intent and this
fresh record, not source. Neither review is acceptance or integrity attestation.
Implementer-attributed workspace nextest
`30302134-5ac4-45b4-8399-46f1963980d2` passed all 2727 executed tests with
20 skipped; affected strict all-target Clippy and workspace formatting passed.
Final all-feature workspace nextest
`92a39f59-3d6e-47bd-983e-566fcac6a64c` passed all 2748 executed tests with
20 skipped; final affected strict Clippy and formatting also passed.

## Final Rust readiness repairs - 2026-11-06

`final-rust-impl-observer` derived these observations from current source and
test assertions only, without intent, queues, prior records/reviews or history.
It executed no tests and made no changes. This supplement supersedes earlier
observations of the repaired mechanisms, not their historical evidence.

Full `rust::check` is process-free/write-free and rejects operation/publication
fences, partial managed state, stale lock binding, invalid resolver binding and
corrupt package files. It reobserves controls before returning.
`VendorManifest::assert_ready` now checks its expected lock digest and this full
gate. Cheap catalogue/report operations remain separate observations; canonical
unversioned package directory names do not replace integrity checks.

`populate` supervises literal host `cargo vendor --locked`, optionally offline,
with explicit cache selection, cancellation, bounded output/time and no retries.
Successful empty stdout is valid only for a graph without external packages,
where it generates the existing standard resolver. External graphs still
require complete source tables. Empty-graph publication independently avoids
acquisition. Writers preserve the exclusive operation lock, private candidate,
lock rechecks, package/file validation and retained recovery fence on incomplete
multi-file publication.

Observed assertions cover cheap/full readiness distinctions, fences, corrupt
files, legitimate empty output versus missing external resolver, cancellation,
lock drift, writer exclusion, recovery and real offline Cargo consumption.
Checksums bind supplied lock/metadata/content, not independent registry archive
or Git-origin attestation. Backend confinement and exhaustive adversarial
ancestor-race resistance are not established by this record.

Implementer-attributed execution: five initial blocker regressions failed in
`ebf1b5d9-c1ff-4a5e-b7b3-c9f365e3a7b2`; the empty-output regression also failed
before its repair. Final affected nextest
`2e9bc4ba-255b-4878-b85f-aafe0ae1e259` passed 389 tests. Strict workspace Clippy,
formatting and affected crate doctests passed. These are finite execution
results, not acceptance, security certification or review receipts.

The separate `rust-source-blind-comparator` read scoped local intent and these
observations without raw source or test implementations. It found substantial
correspondence for full readiness, empty local resolver handling and fenced
publication, with no concrete new shared-backend contradiction. It retained
attributed execution, origin-attestation and filesystem/confinement limits.
Review completion does not accept the stale component revisions.

Subsequent final production-runner nextest
`5c1a5560-129c-461a-b27b-4c29eac74ef1` passed all 389 affected cases after the final
code and queue changes. Installed Skott full readiness also passed against the
refreshed local vendor state; no downloads were used.

## Independent offline dependency observations - 2026-11-06

`offline-dependency-source-observer` independently read current source and
executable assertions, excluding intent, queues, prior implementation records,
reviews, Git/history and chat intent. It ran no commands with effects or tests,
changed nothing and delegated nothing. Its initial read and two corrective
source-only follow-ups established no reportable scoped security vulnerability.
The observations below are its source-derived evidence, not certification,
acceptance or an advisory receipt.

### Current observed behavior

Carried forward from earlier source reads: the library separates cheap catalogue/manifest observations from full lock/content checks and transactional acquisition. `check_snapshot` validates a supplied private vendor copy together with managed fences, manifest/lock binding, resolver/source binding, and control re-observation; unmanaged implicit resolution is restricted to crates.io/local graphs. Checksums establish consistency against supplied metadata and lock labels, **not cryptographic origin attestation**. The corrected duplicate-key fixture now explicitly establishes its mutation before asserting rejection. Sources: `/opt/proj/kvist/dependencies/src/rust.rs:60–180,219–251`; `/opt/proj/kvist/dependencies/src/integrity.rs:404–504`; `/opt/proj/kvist/dependencies/tests/rust.rs:393–411`.

### Earlier detailed source observations, carried forward

- Full checks accept lock versions 3/4, validate package identities and registry
  checksums, require 40 lowercase hexadecimal Git revision syntax, and reject
  ambiguous external name/version identities. Catalogue validation hashes
  declared files and rejects duplicates, undeclared files, links, hardlinks
  and special files. It does not reconstruct registry archive checksums or
  cryptographically establish Git provenance.
- Descriptor-relative directory traversal uses `openat` with directory/no-follow
  flags. Regular reads require a single link, bounded size and stable identity.
  Tree limits are 100,000 entries, 256 MiB/file, 1 GiB total, depth 64 and
  32 MiB retained paths. Legacy stale deletion validates names/tree contents
  before removal; it is not a transaction against concurrent mutations.
- Matching published material is reused without callback invocation or
  metadata rewriting. Local-only unprovisioned graphs can pass `check` without
  creating state; explicit provisioning publishes empty registry/config/manifest
  state. Writers hold an exclusive operation lock that is never stolen.
  Publication fences recovery, retains previous material, renames the new
  directory, writes metadata and revalidates before clearing the fence.
  `Publication.published=true` means the directory rename occurred, not that
  every subsequent commit step succeeded. No automatic reconciliation exists;
  prepublication failures can leave newly created state directories.
- Acquisition selects an absolute host Cargo outside the project and literal
  `vendor --locked`, optionally `--offline`, arguments. An explicit cache is
  selected through fixed `/usr/bin/env`. It inherits host policy and is not
  OS-confined. Combined capture is bounded to 1 MiB; failures use payload-free
  classes and effects are not automatically replayed. Resolver parsing allows
  only closed source-replacement tables, rejects chains/cycles and unrelated
  configuration, and structurally retargets directory sources.

Detailed initial anchors: `src/vendoring.rs`, `src/rust.rs`,
`src/integrity.rs`, `src/fs.rs`, `src/publication.rs`, `src/resolver.rs`,
`src/lib.rs`, `tests/rust.rs` and `tests/catalogue.rs`. Source assertions cover
reuse, initial publication, corruption/links, cancellation, writer exclusion,
source binding, recovery and offline/locked native Cargo consumption. The
observer read these assertions but did not execute them.

### Corrective lineage and evidence limits

Earlier observation found consumer composition discrepancies: acquisition
scratch remained beside a vendor-only candidate, and Skott bypassed managed
state by checking only its catalogue. Later selective source reads observed
explicit scratch cleanup and shared snapshot/control checking; the final
read observed recovery-only and empty-resolver-directory gating, restored
selected-pin diagnostics, explicit lock guidance, and a field-order-independent
duplicate-key fixture. No persisting concrete discrepancy was identified
within those corrections. Comprehensive adversarial ancestor-race resistance,
backend confinement and source-origin attestation were not proven. Parent-run
test results are separate evidence; this observer executed no tests.

## Shared-area source observations - 2026-11-07

Independent `deps-source-audit` read current local source and test assertions,
excluding all intent, queues, previous records/reviews, peers and history.
It ran no tests, changed no files and reported no high-confidence exploitable
vulnerability within its scope. These observations supersede earlier descriptions
of checksum inventories, copied snapshots and fixed vendor quotas, not historical
execution evidence.

- Descriptor-relative traversal uses `O_NOFOLLOW`. Regular reads require a
  single link, enforce bounds and compare identities before/after reading.
  Directory identity is rechecked against the displayed path. These checks
  do not establish UID/GID ownership or reject writable permissions
  (`src/fs.rs:75-248`).
- Lock versions 3/4, package identities, registry checksum shape and immutable
  Git revision shape are validated. Exact lock bytes receive a SHA-256 digest
  and are rechecked with identity. Package origin/content is not independently
  attested (`src/integrity.rs:79-220`).
- Managed readiness gates operation/recovery fences, receipt/lock binding,
  resolver binding and required package metadata. Catalogue validation reads
  package name/version and checks regular native checksum metadata presence,
  without parsing the file inventory or hashing package source. Legacy catalogue
  helpers additionally accept numbered directories without inspecting contents
  (`src/rust.rs:83-262`, `src/integrity.rs:361-407`,
  `src/vendoring.rs:153-173`).
- Matching material is reused without acquisition. Otherwise, an exclusive
  operation file and advisory vendor-directory locks coordinate cooperating
  operations. Staged trees receive structural/quota validation. Publication
  fences directory renames and atomic resolver/receipt writes; interrupted
  publication retains recovery material rather than automatically rolling back
  (`src/rust.rs:420-656`, `src/publication.rs:24-129`).
- Closed resolver parsing rejects unrelated Cargo configuration and requires
  one-hop directory replacements, retargeted through TOML serialization.
  Managed replacements bind locked sources to `/workspace/vendored`; accepted
  URL schemes include HTTP and local files (`src/resolver.rs:37-248`).
- Optional policy rejects unknown fields and zero limits but allows increases
  beyond defaults. Reuse does not fully rescan source-tree quotas. Acquisition
  passes literal `vendor --locked [--offline]` arguments, timeout/output bounds
  and zero retries to Sav. Executable validation checks the canonical target
  outside the project but executes the selected original path. This interface
  configures no OS sandbox; supervisor internals were not inspected
  (`src/limits.rs:62-102`, `src/rust.rs:280-393,510-539`).

Implementer-attributed evidence: initial targeted regressions reproduced the
retired receipt, checksum inventory, ignored policy and shared-reader publication
behavior before repairs. Production-runner nextest
`21afbe63-ae2a-482d-a3f4-36c6ab3d20b2` passed all 399 affected tests across three
components, including all dependency tests and exact quota/process boundaries.
Strict workspace Clippy, formatting, affected doctests and offline release builds
subsequently passed. The audit did not execute or independently verify those
runs. Queue timestamps record later bookkeeping, not test execution chronology.
No acceptance, review receipt, exhaustive race proof or certification is claimed.

Separate `deps-blind-comparison` compared only current local requirements,
contract and design with the new independent observations, without raw source,
tests, queues or peers. It found scoped correspondence for lightweight readiness,
reuse/locking, configurable policy and fenced publication, with no concrete active
contradiction. Earlier inventory/copy/fixed-quota descriptions were explicitly
superseded history. It retained missing itemized proof of the exact two-field
receipt/retired-format rejection, TOML dialect/parser bound/default propagation,
and consumer-side ownership/masking/restart behavior. Attributed multi-component
tests support but do not independently certify those details; the component-only
observer did not inspect consumer implementations. Advisory locks and uninspected
supervisor internals do not establish exhaustive race/confinement proof.

Final implementer-run production nextest
`7eb7fff4-82b4-4b8d-83a5-9c8e576971ab` passed all 399 affected tests after the lint
correction and operational queue updates. Both freshly built release frontends
reported the same ready root tree: 365 registry, zero Git and six local packages.
Kvist reused it with host offline policy, without acquisition. These results
remain attributed execution, not accepted intent revisions or certification.

## Native cache observation (2026-11-07 UTC)

Independent source-only observer `d38462f8-d2f9-4d78-a5bb-c512f6458890`
derived the Dependencies section of
[`native-cache-observation.md`](../docs/reviews/native-cache-observation.md).
Its excluded inputs and source/test references are recorded there; no checks
were executed by that observer.

Observed native preparation publishes only schema version and lock digest,
reuses registry/Git data and acquires at most once when incomplete. Canonical
inspection hashes neither archives nor source-file contents; `force` refreshes
the receipt rather than reacquiring apparently complete material. Source
admission validates exact locked metadata beneath selected data roots. Native
catalogues do not enforce directory-export aggregate-content quotas or attest
source integrity. Historical export observations above are not the native
backend's storage model. No acceptance or certification is inferred.

Source-blind comparer `a6add327-2a55-4006-b3f6-b2750041ff26` independently
compared scoped intent and these observations. Its correspondence, original
documentation discrepancies, re-observed closure and execution limits are
persisted in
[`native-cache-comparison.md`](../docs/reviews/native-cache-comparison.md).
No source was read by that comparer; broader failures remain unresolved.
