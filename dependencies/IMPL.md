<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

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
