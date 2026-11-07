<!-- kvist-design-version: 1 -->

# Shared Dependencies Design

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
