<!-- kvist-design-version: 1 -->

# Shared Dependencies Design

Separate the extracted catalogue API from the fully checked Rust backend.
Keep shared errors frontend-independent. Parse TOML structurally; do not infer
package versions from arbitrary text lines for full validation.
Route manifest readiness through the full Rust check after verifying the
caller's expected lock digest; retain `report` as the explicitly cheap
catalogue-only operation. Never treat retained old metadata as permission to
bypass publication or operation fences.

Full validation checks locked external names/versions/origins, Cargo checksum
metadata and bounded nonfollowing regular file content. Retain deterministic
ordering and finite byte/entry/path/depth quotas. Do not require new material
for source-less local packages or a dependency-free graph.

Acquire a no-clobber operation lock under the validated project state directory.
Use same-filesystem private staging; populate once, validate the staged catalogue
and source config, then compare the lock again before publication. Retain the
old registry in an owned recovery location across rename publication. Replace
resolver and manifest atomically per file; readiness binds the original lock
and must refuse an interrupted multi-file commit. Reuse matching published
material without invoking Cargo or rewriting metadata. RAII cleanup may remove
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

Skott consumes shared validation/resolver content against its private immutable
Rust snapshot. It must not mount mutable workspace vendor aliases or import
manifest host paths. Future languages can supply separate acquisition and
offline-environment strategies when implemented; this phase adds no fake backend.
