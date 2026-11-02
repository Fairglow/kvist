# Independent review report

Task: `galla-reverification-security-audit`. Attempt: `attempt-0006`.
Source evidence digest: `sha256:d2eee41cd97691034dce728c27cffa67c0fb0e8881d3c76065a954e7612043c2`.

This is agent-written advisory evidence, not human acceptance or verified facts. Inspect current source/evidence freshness before acting.

## Source-only observation

Not a separate observation phase for this audit.

## Comparison or audit

Aggregate of 5 evidence fragments; not acceptance.

## Synthesis of fresh source-only security observations

**Decision flags:** fragment 1 decision_required=true; fragment 2 decision_required=true; fragment 3 decision_required=true; fragment 4 decision_required=true; fragment 5 decision_required=false. Overall decision_required=true.

### Verified
- Fragment 1: roots are opened via / plus openat2/componentwise O_NOFOLLOW, and root dev/ino is rechecked, reducing symlink/TOCTOU escape. Source walk rejects symlinks, nlink != 1, non-regular files, and re-hashes via retained FDs.
- Fragment 2: run unshares net only for Deny. safe_prlimit ignores errors, omits nproc, and sleeps 15 ms. File/scratch size checks inspect immediate files only. inspect_and_promote_cache skips manifest checksums, uses non-atomic fs::copy, and top-level snapshot misses nested concurrent cache changes.
- Fragment 3: origin parser rejects credentials, percent-encoding, query/fragment, traversal, duplicate slashes, non-http(s), and non-loopback plain HTTP. Production parser rejects private/loopback/localhost and non-HTTPS. Policy matches exact authority and component-boundary path prefixes; typed query only for Cargo Git info/refs?service=git-upload-pack.
- Fragment 4: registry sources accept identity equal to sha256 of name alone. Validator returns derived identity. PATH is only checked as one absolute directory.
- Fragment 5: legacy requests fail closed with actionable diagnostics and no stdout. Administrative PATH and covering grants are rejected. Loopback, numeric aliases, duplicate/overlapping sources, and forged identities are rejected. Schema enumerants and required fields mirror parser. Probe succeeds and emits JSON. Host permissions remain unchanged after namespace exclusions. Read-only mutations fail with Read-only file system. Host rustup/toolchain state is unchanged.

### Suspected
- Fragment 1: trusted generation parent permissions/ownership are not checked. If parent is local-writable, staging names .kvist-cache-staging-<pid>-<seq> can be pre-created, exhausted, or swapped, enabling DoS/tamper. Reuse path returns Durability::Synced without parent fsync, possibly overstating durability for ParentSyncUnknown generation.
- Fragment 2: PackageSources leaves host net shared, so proxy env vars are bypassable. Guard reads only first 4096 bytes and uses raw starts_with, allowing prefix/subdomain bypass and smuggling. It handles only http:// absolute-form, not CONNECT, so HTTPS may fail. pgid process counting can be evaded if child changes pgid/session.
- Fragment 3: is_public_address treats IPv4-compatible IPv6 ::a.b.c.d as public because only ::ffff:a.b.c.d is mapped; could allow SSRF to embedded loopback/private. probe locate_backend trusts PATH and fixed paths without signature/immutable digest, and sets new_session/parent_death_signal true unconditionally. Validation allows read-only /usr or /usr/local toolchain grants, exposing /usr/sbin. Writable-scope symlink escape check is immediate-only. Dangerous env list may miss RUSTC_WRAPPER/GIT_CONFIG_COUNT. PATH admin check is lexical only.
- Fragment 4: canonical_production_origin falls back to parse and registry paths lack explicit HTTPS scheme check; non-canonical/non-HTTPS origins may pass if is_canonical_origin_url/parse allow them. Tests accept /usr/bin even when no /usr grant and Runtime mount exists at /workspace/bin, allowing PATH to point to writable scratch or unmounted dirs.
- Fragment 5: scaffold status string claims fully implemented Bubblewrap enforcement, but source-only evidence cannot verify the actual enforcement implementation.

### Discrepancies
- Fragment 2: tests expect atomic immutable promotion with checksum/collision checks, but inspect_and_promote_cache skips manifest checksums and uses non-atomic fs::copy.
- Fragment 4: explicit decision is needed: pin PATH to read-only Runtime/Toolchain destination or validate it is a granted read-only path.
- Fragment 5: claimed Bubblewrap enforcement is not present in source-only evidence.

### Missing evidence
- Parent isolation model, PID-reuse/cleanup guarantees, cross-fragment promotion integration.
- AllowedSource validation and cache source.
- Cross-fragment enforcement context.
- Resource limit validation, grant uniqueness/overlap enforcement, origin parser definitions.
- Bubblewrap enforcement code.

### Arbitration
Situation: source-only evidence shows multiple suspected weaknesses and one explicit PATH decision, while also claiming Bubblewrap enforcement that cannot be verified. Design implications: writable parent can undermine staging integrity and durability; shared host net can bypass proxy controls; URL guard may allow smuggling or fail HTTPS; IPv4-mapped IPv6 may enable SSRF; weak registry identity and non-HTTPS fallback may allow origin confusion; non-atomic promotion may expose inconsistent cache state; PATH may point to writable or unmounted directories; missing enforcement code may mean namespace isolation is not actually implemented. Alternatives: enforce parent ownership/permissions and parent fsync before claiming Synced; parse full URLs and handle CONNECT; use cgroup or robust process accounting; require canonical HTTPS-only origins and bind identity to index/download origins; make promotion atomic with checksums and collision checks; pin PATH to a granted read-only directory; add and test Bubblewrap enforcement. Tradeoffs: stricter parent checks may reject legitimate writable parents; full URL parsing may reject unusual but valid URLs; cgroup accounting adds complexity; atomic promotion may require filesystem support; pinning PATH reduces flexibility; adding enforcement requires more code and tests. Advisory recommendation: do not accept the source-only state until parent durability, network guard, promotion atomicity, origin/identity, PATH grant, and Bubblewrap enforcement are clarified or implemented, because current evidence cannot establish intended secure design. Evidence needed: source of Bubblewrap enforcement, parent isolation model, PID cleanup guarantees, AllowedSource validation, origin parser definitions, resource limit validation, grant overlap enforcement, tests for CONNECT/HTTPS, IPv4-mapped IPv6, PATH writable scratch, and promotion checksums.

Raw machine evidence: galla-reverification-security-audit.attempt-0006.json

## Choose or approve

List existing numbered responses with `kvist decision 'galla' actions`; use `kvist decision 'galla' do NUMBER` to invoke the displayed exact command. Tab shows response descriptions; no approval hash copying is needed.

Get structured advisory alternatives:

kvist decision 'galla' options --task 'galla-reverification-security-audit'

Stage your chosen option and any further guidance:

kvist decision 'galla' plan --task 'galla-reverification-security-audit' --option OPTION_ID --guidance 'Additional direction'

Inspect the generated Markdown for all intent and task effects and its exact `apply PLAN_ID --approve SHA256 [--continue]` command. Choosing prepares a proposal; only exact approval applies it. A recommendation is not human acceptance. Intent approval never approves implementation or a reviewer's claims.

## Ask for clarification

Keep work paused and discuss the evidence without edits:

kvist decision 'galla' discuss 'Explain the evidence and alternatives' --task 'galla-reverification-security-audit'

Follow-up questions share durable advisory discussion. Answers do not change approved intent and are not compliance evidence.

## Reject or redirect

Stage a new direction, or explicitly request withdrawal with reasons:

kvist decision 'galla' plan --task 'galla-reverification-security-audit' --guidance 'Reject the proposal; instead ...'

Withdrawal must include the entire dependent closure; completed/live/fenced work cannot be withdrawn. It preserves records, not a fictitious completion.

## Manual or offline planning

kvist decision 'galla' plan --task 'galla-reverification-security-audit' --import draft.json --review-exception 'Explicit review exception'

Run `kvist decision 'galla' plan --help` for the typed draft format; no hand-edited queue or agent-minted lifecycle metadata is needed. Apply the resulting exact approved plan by the same command as an agent-prepared plan.

## Resume after resolution

Run these commands from the project root only after reviewing retained effects and resolving the issue. Unresolved attempts/fences still require explicit recovery/disposition; do not delete evidence or replay unknown effects.
An applied resolution makes approved follow-up work schedulable. Prefer `apply ... --continue`, or `kvist task run 'galla'`. For a manually resolved pause, explicitly retry the task or checkpoint without rerunning completed tasks:

kvist task run 'galla' 'galla-reverification-security-audit'

Changed execution policy requires `kvist task approve-policy` first. This brief changes no task state, approval or recovery fence.
