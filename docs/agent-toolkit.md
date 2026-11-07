# Native agent toolkit

## Recommendation and current scope

Use a small, complementary toolkit with explicit schemas, bounded results and
actionable errors, not a catalog of overlapping wrappers. Model familiarity is
useful, but descriptions must describe the actual supported dialect. Tool
proposals are untrusted; Galla's filesystem/network grants remain the authority.
These tools belong to Skott's workspace runtime, not maerg's protected task
broker or an external coding agent's private tool loop.

| Need | Skott tool | Guidance |
| --- | --- | --- |
| Inspect files | `read_file` | Bounded UTF-8 byte pages and `page_sha256`; full raw-file SHA is opt-in. Omission continues per path within a prompt. Explicit `offset: 0` restarts. Stop at null `next_offset`. |
| Navigate | `list_dir`, `find_files` | Sorted scoped pages; discovery uses literal path substrings, not globs. |
| Find code/text | `search_files` | Scoped literal search, file filtering and explicit skipped-coverage counts; not regex or semantic search. |
| Create/replace | `write_file` | Whole content; use `expected_sha256` when replacing observed content. |
| Focused change | `edit_file` | One exact occurrence; optional preimage digest; rejects ambiguity. |
| Several changes in one file | `apply_patch` | Optional digest and strict unified hunks; no file headers, fuzzy matching or hidden targets. |
| Extend an existing file | `append_file` | Optional digest; adds bytes verbatim without an implicit newline. |
| Build, test, format, inspect Git | `shell` | Existing sandboxed shell, available installed language tools and bounded output; network remains denied. |

Read cursors use actual returned offsets, so UTF-8 and JSON-escaped output
shrinking cannot skip data. Paths have independent cursors; changing `limit`
does not restart. Cursors survive compaction and steering but reset for a new
prompt. Ordinary continuation does not freeze the file revision or synthesize
a guard. External edits are legitimate; page metadata records what was actually
read, not an atomic multi-page snapshot. `sha256` is null unless
`include_sha256:true` or `expected_sha256` requests a streamed full raw-file
digest. Never use `page_sha256` as a full-file mutation guard. After EOF it requests an
empty terminal page; identical terminal calls receive correction and eventually
the circuit breaker. Earlier explicit offsets remain deliberate observations.
Stateless helper/executor use outside the prompt loop still starts at zero.
The journal retains proposed arguments and actual result offsets.

## Recovering from inspection limits

### Restriction audit

| Area | Removed | Retained safeguard |
| --- | --- | --- |
| Files | General size caps, implicit whole-file hashes/guards | Bounded streaming/pages; explicit revisions and atomic local conflict checks |
| Host edits | Frozen manifests, reader leases, drift/restart gates | Live contents; no-follow grant origins and trusted outside selectors |
| Search | Discarded partial results and whole-file scan admission | Bounded work, explicit scan continuation and honest coverage |
| Commands | Default substring bans and administrative-directory exclusions | OS namespaces/capabilities, read-only systems, explicit operator filters |
| Environment | Read-only-only authoring PATH and blanket payload loader/Git bans | Cleared host enforcer environments; closed protected Cargo-phase controls |
| Rust | Mandatory runtime receipt/lock readiness and forced workspace locking | Offline Cargo, private credentials/cache, usable tools plus command-local errors |
| Publication | Inferred mutable sibling-cache writes | Explicit provider-owned descriptor authority; preserved attempt data |

These are finite object-grant guarantees, not a whole-host alias/socket
inventory. Process supervision is sampled; it is not kernel admission control.

Native discovery/search skip sockets, FIFOs and other nonregular descendants
without opening them and report `skipped_special`. Their presence is not a
security denial or a reason to abandon code inspection. Explicit special-file
scopes, unreadable ordinary files and detected substitutions still fail.

Search scans at most 8 MiB per call, including bounded partial-file work. It
keeps collected matches and returns `scan_limit_reached=true`, `complete=false`,
`next_scan` and continuation/narrowing guidance in `next_step`. `total` and
`next_offset` describe those collected matches only; null is not proof of full
coverage. An empty partial result cannot establish absence. Narrow `path` to a
relevant source directory or file, or add a literal `file_pattern`, such as
`.rs`. Result offsets paginate collected matches; `scan_offset` resumes scanning
the indicated file. Deep windows may have unknown absolute line numbers and
overlap may repeat matches. Depth, entry and matching-line
limits still fail explicitly with scope/filter guidance.

For example, a workspace-wide search for `answer>` in this repository should
be narrowed to `/workspace/maerg/src/bark` with `file_pattern: ".rs"` before
reading the relevant files. A native inspection failure does not imply source
changes occurred: generic `may_have_effects` failure metadata is conservative
process evidence, not a claim that a read mutated the workspace. Reconcile
state before repeating an effectful or uncertain command, not before every
already-authorized narrower search. Explain any actual blocker and continue
independent safe work rather than silently abandoning the goal.

Append and patch use the existing private payload, installed native helper and
atomic per-file replacement. They preserve modes and unrelated bytes, reject
linked mutation paths and stale digests, and validate before writing. There is
no automatic replay: re-read state after a failed or uncertain effect.

Example `apply_patch` arguments (substitute the actual digest):

```json
{
  "path": "/workspace/src/example.rs",
  "expected_sha256": "sha256:<64 lowercase hexadecimal digits>",
  "patch": "@@ -1,2 +1,2 @@\n unchanged\n-old\n+new\n"
}
```

Only hunk text is supplied, not `---`/`+++` headers or a patch-file path.
Coordinates, counts and context must match exactly. Transport lines end in LF;
CR before LF is preserved as file content, and missing final newlines use
`\ No newline at end of file`. Multi-file transactions, fuzzy merges and
provider-specific patch formats are not supported. Each tool's schema and the
[Skott contract](../skott/CONTRACT.md#native-file-tool-inputs-and-results)
specify bounds.

Currently, a patch whose result is unchanged is rejected. The independent
[source-blind comparison](reviews/tool-ergonomics-comparison.md) retains the
contract's underspecified no-op policy for human arbitration, alongside finite
evidence limits and separate existing-code findings. This implementation is
not a claim of formal acceptance or blanket compliance.

## Dependencies are a separate capability

There is deliberately **no model-facing install/update/vendor tool yet**.
Adding a shell wrapper around `cargo add`, `npm install` or `kvist vendor`
would cross the authoring network boundary and potentially execute package scripts.
A successful vendor
command also does not prove that the exact model-requested dependency/version
was fulfilled.

Today, report the package, exact requested version/features and missing-build
diagnostic. User-requested manifest authoring within the workspace need not be
blocked; offline Cargo may create/update the writable project lock.
Acquisition remains an explicit host operation. An operator reviews manifests,
provisions the registry through `skott --vendor --cwd DIR` (or `kvist vendor`),
optionally checks `skott --vendor --check --cwd DIR`, then retries only the
affected build. Live workspace resources refresh without restart, receipt,
reader lease or frozen lock digest. See
[toolchains and vendoring](sandbox-toolchains-and-vendoring.md).
Protected maerg authoring also explicitly refuses automatic dependency
fulfillment while its request-to-lock/vendor binding is absent.

A future bounded acquisition tool should:

1. Accept typed ecosystem/package/version/features and a reason, never arbitrary
   shell commands, URLs or package-manager flags.
2. Require preapproved registries, scopes, version policy, credentials and
   resource budgets; constrain downloads and transitive resolution. Treat
   manifests, hooks and package scripts as executable untrusted input.
3. Execute separately from authoring, with exact write targets and no ambient
   user home/credentials or unrestricted host process authority.
4. Bind the request, reviewed manifest/lock changes and vendor manifest,
   report the exact resolved versions, and fail without claiming fulfillment
   when binding is absent.
5. Re-prepare/pin resources before network-denied offline verification; preserve
   explicit failure/partial-effect evidence and human acceptance boundaries.

This needs a designed acquisition contract and enforcement, not another tool
name. It remains deferred rather than weakening network isolation here.

## What to add next, only with demonstrated need

Useful next candidates are bounded regex/glob search, range/line-number views,
read-only symbol/LSP queries, and structured build/test diagnostics. Existing
shell commands cover these imperfectly without introducing new authority.
Measure actual model success, calls/tokens, stale-edit retries and loop rate
before adding schemas. Native rename/delete/mkdir, multi-file edits, background
job controls, network fetch, MCP and arbitrary plugins need separately defined
failure/recovery and authority contracts; they are not implicitly enabled.
Human task planning can remain conversational; do not expose mutable maerg
queues, acceptance or compliance evidence as convenience tools.

## Research basis

Reviewed 2026-10-27:

- [Anthropic: writing effective tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents)
  recommends deliberate tool selection, meaningful bounded responses, clear
  descriptions and task-based evaluation rather than maximizing tool count.
- [Anthropic: text editor tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/text-editor-tool)
  demonstrates view/create/exact replacement/insertion as common editing needs.
- [OpenAI: apply patch](https://developers.openai.com/api/docs/guides/tools-apply-patch)
  demonstrates structured incremental file operations and explicit application
  outcomes. Skott's generic JSON tool is intentionally not that wire format or
  a claim of provider-native compatibility.

These are ergonomic precedents, not security guarantees. Deterministic
regressions exercise runtime/tool semantics; real-model qualification remains
an explicit opt-in, not an asserted performance improvement.
