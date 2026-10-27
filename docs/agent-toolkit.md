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
| Inspect files | `read_file` | Bounded UTF-8 byte pages with full-file digest; omission continues per path within a prompt. Explicit `offset: 0` restarts. Stop at null `next_offset`. |
| Navigate | `list_dir`, `find_files` | Sorted scoped pages; discovery uses literal path substrings, not globs. |
| Find code/text | `search_files` | Scoped literal search, file filtering and explicit skipped-coverage counts; not regex or semantic search. |
| Create/replace | `write_file` | Whole content; use `expected_sha256` when replacing observed content. |
| Focused change | `edit_file` | One exact occurrence and required preimage digest; rejects ambiguity. |
| Several changes in one file | `apply_patch` | Required digest and strict unified hunks; no file headers, fuzzy matching or hidden targets. |
| Extend an existing file | `append_file` | Required digest; adds bytes verbatim without an implicit newline. |
| Build, test, format, inspect Git | `shell` | Existing sandboxed shell, available installed language tools and bounded output; network remains denied. |

Read cursors use actual returned offsets, so UTF-8 and JSON-escaped output
shrinking cannot skip data. Paths have independent cursors; changing `limit`
does not restart. Cursors survive compaction and steering but reset for a new
prompt. Automatic continuation guards the previous digest: if a file changes,
restart explicitly instead of combining revisions. After EOF it requests an
empty terminal page; identical terminal calls receive correction and eventually
the circuit breaker. Earlier explicit offsets remain deliberate observations.
Stateless helper/executor use outside the prompt loop still starts at zero.
The journal retains proposed arguments and actual result offsets.

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
would cross the authoring network boundary, potentially execute package scripts,
and leave the prepared read-only dependency snapshot stale. A successful vendor
command also does not prove that the exact model-requested dependency/version
was fulfilled.

Today, report the package, exact requested version/features and missing-build
diagnostic. An operator updates/reviews manifests and locks on the host,
provisions the registry through `kvist vendor`, then restarts Skott so it
prepares fresh read-only resources. See
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
