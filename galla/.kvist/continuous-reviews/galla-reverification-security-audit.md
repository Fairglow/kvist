# Independent review report

Task: `galla-reverification-security-audit`. Attempt: `attempt-0007`.
Source evidence digest: `sha256:d8b9d2771cec53fd24b4023ddc89dd7c8df44fedf05c0a992196d7e3a6617fda`.

This is agent-written advisory evidence, not human acceptance or verified facts. Inspect current source/evidence freshness before acting.

## Source-only observation

Not a separate observation phase for this audit.

## Comparison or audit

Aggregate of 6 evidence fragments; not acceptance.

Source-only synthesis of 6 fresh fragments. Evidence limitations: source-only; tests are observed but do not prove enforcement where implementation is missing; native Bubblewrap tests are ignored; fragment 2 is truncated at walk_dir. No prior reviews or acceptance are claimed.

Decision flags: 1=true, 2=true, 3=false, 4=true, 5=false, 6=true; combined decision_required=true.

Fragment 1, src/cache.rs. Verified: descriptor-relative no-follow root opens, symlink/hardlink/special rejection, manifest reconciliation, 0444/0555 normalization, RENAME_NOREPLACE. Suspected: fchmod immutability is not followed by fsync of files/directories; only staging root fsyncs before rename. Crash after rename may leave published entries writable (0600/0700), breaking immutability. reuse_or_collision returns Durability::Synced without parent-durability recheck. Decision needed: add post-fchmod fsyncs or document best-effort; need crash tests and later recovery code.

Fragment 2, src/enforcement.rs. Source observations: inspect_and_promote_cache verifies no manifest checksums or file listing; it only rejects files whose entire contents equal b"downloaded crate", reads whole files before size checks, and copies all walked files. Observed local gap: attacker-controlled source can promote arbitrary cache files. take_cache_snapshot is non-recursive and omits file type/inode; symlink or nested changes may evade before/after comparison. Network guard reads one 4096-byte chunk, never loops for full headers, forwards GET/HEAD as GET, and validates CONNECT hosts but not HTTP absolute hosts (userinfo/query allowed). Single-threaded accept and unbounded CONNECT tunnel are suspected DoS/path-bypass risks. count_processes_in_pgid parses /proc/*/stat by whitespace, breaking when comm contains spaces/parens; process limit may be bypassed. safe_prlimit ignores /usr/bin/prlimit absence/failure and is applied after a 15 ms sleep; rlimits may be absent or late. Fragment truncated at walk_dir; remaining observations unavailable. Decision flag preserved: true.

Fragment 3. Verified: strict URL parsing rejects credentials, query strings, fragments, percent-encoding, path traversal, and non-HTTP(S) schemes. IPv4 alias and IPv4-mapped IPv6 classification prevent private/loopback bypasses. Production policy rejects loopback and private origins; test loopback only admits loopback. Query strings are rejected for registry/crate downloads; only git-upload-pack on /info/refs is allowed for Cargo Git. Protocol types use deny_unknown_fields to reject unknown fields. Suspected: CONNECT tunnel authorization on authority alone may allow access to any path on the authorized host, relying on the application to enforce path restrictions. This is a design trade-off, not a verified vulnerability. Verified: probe verifies backend and runner digests, and namespace availability. Resource limits are defined and enforced. Decision flag preserved: false.

Fragment 4. Verified: fail-closed checks for bounds, canonical paths, digests, grant non-overlap, PATH grant containment, Cargo argv/env/topology, and origin-bound registry/Git identities (tests confirm). Suspected: argv[0] is only absolute-checked; for Toolchain::System it is not required under the toolchain root or outside administrative dirs. With a /usr toolchain grant, an administrative executable could be selected unless enforced elsewhere. Suspected: writable-scope symlink check inspects only immediate entries; nested escaping symlinks may pass unless client/runner recursively validates. Missing: origin.rs, backend/runner enforcement, client recursive symlink checks. Decision needed: confirm whether argv[0] containment and recursive symlink validation are enforced outside this fragment; otherwise tighten validation here.

Fragment 5. Verified test assertions: v1 request parser rejects legacy, unknown fields, relative/traversal/non-canonical paths, overlapping destinations, PATH entries outside read-only grants, administrative dirs, reserved /.galla-sandbox, resource limits over safe maxima, zero limits, oversized request/env name, deny+sources, scratch/cache endpoint identity mismatches, promotion source not cargo-home, toolchain substitution, environment allowlist violations, source identity golden vectors, loopback/numeric alias/overlap sources. Cache tests verify checksum mismatch, file count/aggregate size limits, RENAME_NOREPLACE collision with pre-existing tampered generation, reuse. Suspected gaps: no concurrent promotion race test, no staged-file reverification test (empty), no symlink/FIFO/device/traversal manifest entry tests, no IPv6/private origin rejection, no env var coverage for proxies/git fetch, no schema access/protocol checks (truncated). Missing implementation source; tests do not prove enforcement. Source-only cannot. Decision flag preserved: false.

Fragment 6. Verified: 121 tests pass, 2 ignored. Schema asserts required fields, promotion excludes post-fetch state, cache required. Runner tests use env_clear, piped stdin. Scaffold probe rejects nesting via /.galla-sandbox marker (exit 3) or succeeds with empty stderr. Suspected: toolchain_e2e writes to /tmp (go build -o /tmp/go-trial, CARGO_TARGET_DIR=/tmp/target) but request grants only /workspace/component and /workspace/scratch. If /tmp is not an intended writable mount, this is a grant/destination mismatch; if it is, the request should declare it. Suspected: nesting prevention relies on /.galla-sandbox; no evidence it is immutable or namespace-scoped. A writable/spoofable marker could bypass nesting detection. Missing evidence: Native Bubblewrap tests are #[ignore] and not executed, so sandbox isolation, read-only toolchain mounts, and no-host-mutation guarantees are unverified here. Need runner source for /tmp handling and marker creation. Decision flag preserved: true.

Raw machine evidence: galla-reverification-security-audit.attempt-0007.json

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
