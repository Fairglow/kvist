<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Observation inputs and limits

This is source-only observation with supplied execution evidence, not acceptance,
implementation authority or compliance/security certification. It is derived
from current component Rust implementation/test source, shell fixture,
`agent_runner/Cargo.toml`, and these two files:

- `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/runtime-delivery-qualified.log`
- `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/live-delivery-qualified.log`

The previous record was not opened or reused. Intent, queues, reviews,
Git/history and peer implementation are excluded. Runtime and sandbox backend
internals were not inspected; their recorded hashes/results in allowed logs
do not expand that source boundary.

The observer computed current source hashes but **did not execute tests/builds,
live requests or executable hashing**. Those executions are supplied. Relevant
mechanisms and assertions were inspected, including the prefix-aware styled
wrapper and code continuation construction. Inventory matching is not a claim
that every source assertion was individually examined. Only this record was
written; source/tests were not changed.

There are **36 allowed files**: 26 implementation Rust, eight integration-test
Rust, one shell fixture and one manifest. Every file matches both supplied logs:
**36 matches per log, zero missing identities, zero mismatches**.
Current Markdown implementation/local tests hash to
`6b681962ef8ce917f245c94306419e2ee5123feef053f9cc4a5ff64191180d93`.
The complete inventory below has aggregate SHA-256
`b8467755254888b355f5904c1fdbc598b62d12e9b438de0fcd376f9dff117af8`.

## Supplied runs and source/caller identity

| Log | SHA-256 | Reported UTC start / end |
|---|---|---|
| `runtime-delivery-qualified.log` | `160031bfcf491dde59964ae6abfe91b85cbe3c0581c68b3777b7d42df192ece7` | `2026-10-02T02:21:47Z` / `2026-10-02T02:21:58Z` |
| `live-delivery-qualified.log` | `cfaf987d285794d8ccd6023f55e5388432949c6e083cc026aa43bb9d3826f5c0` | `2026-10-02T02:21:47Z` / `2026-10-02T02:21:52Z` |

Both report rustc `1.99.0 (b940084d7 2026-09-28)` and cargo
`1.99.0 (5f94df478 2026-08-27)`.
Runtime names `cargo test --locked -p agent-runner -p agent-runtime`,
with a 2.40-second build:

| Runner target | Passed | Failed | Ignored |
|---|---:|---:|---:|
| Library | 180 | 0 | 0 |
| Main binary | 0 | 0 | 0 |
| File-helper binary | 0 | 0 | 0 |
| `component_tests` | 35 | 0 | 0 |
| `context_preflight` | 18 | 0 | 0 |
| `headless_cli` | 4 | 0 | 0 |
| `input_boundaries` | 3 | 0 | 0 |
| `live_llama` | 0 | 0 | 3 |
| `loop_integration` | 33 | 0 | 0 |
| `native_file_tools` | 28 | 0 | 0 |
| `subprocess_supervision` | 26 | 0 | 0 |
| Runner doc tests | 0 | 0 | 0 |
| **Runner subtotal** | **327** | **0** | **3** |

The runtime dependency contributes 140 log-only passes: library 7, catalog 11,
CLI 25, command 6, GBNF 9, loop detection 11, model CLI 2, model transport 44,
profiles 4, setup 9, supervisor 9 and trajectory 3. Its main/doc targets have
zero cases; failures/ignored are zero. Dependency bodies are excluded.
The offline command therefore reports **467 passed, zero failed, three ignored**.

Live first names
`cargo test --locked -p agent-runner --test live_llama --no-run` (1.40-second
build), then records source/executable hashes and invokes:

```text
KVIST_LIVE_SANDBOX_RUNNER=/opt/target/release/kvist-sandbox-runner KVIST_LIVE_LLAMA_ENDPOINT=http://127.0.0.1:9931 KVIST_LIVE_LLAMA_MODEL=Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL cargo test --locked -p agent-runner --test live_llama -- --ignored --test-threads=1
```

Endpoint/model are those recorded values and effort is `none`. The second
build finishes in 1.48 seconds; the three named live tests pass in 2.80 seconds,
with zero failed/ignored. Across commands there are **330 runner passes**, or
**470 combined passes** including dependency results. The three offline
ignored cases execute only in the separate live command.

| Executable identity supplied in live build/hash/run sequence | SHA-256 |
|---|---|
| `/opt/target/debug/deps/live_llama-91e0887ff0465c78` | `08b82e8fd163951e364509aebe6904fd745bfc628fe5aec483e01cb7f912e381` |
| `/opt/target/debug/agent-runner-file-tool` | `3a7264d97aadd70a89fcf08d7fa97ea57f8e09d3d6fc64ec6a2ab6c3378f9f97` |
| `/opt/target/release/kvist-sandbox-runner` | `db631ad7a514a31c0b57f74815ef612f224b25ff8b662affb0a64d9af7fda1b3` |
| `/usr/bin/bwrap` | `d9498f8b15b1c69e09791badee317d56f33abd359a306d6e96534f136381ad74` |

The caller path executed is the path hashed in that sequence. Matching current
local source plus supplied build/hash/run is the linkage observed; it is not
independent binary attestation. Live server revision/executable and model weights
identity are absent. No formatter, Clippy, real-terminal, cross-platform or
wider-workspace execution is supplied here.

## Markdown prefix-aware wrapping

### Styled-cell primitive

`render_document` is a root-exported pure function returning styled
RenderedLine values and code flags. Its private implementation enables
tables, task lists and strikethrough and handles headings, inline formatting,
lists/quotes, code and related parser events. Requested width and
margin-adjusted content width floor at one.

Generic `wrap_styled_line` delegates to `wrap_styled_line_with_prefix` with an
empty prefix. `split_cell_chunks` uses the generic path. The prefix-aware
function returns a fitting Line unchanged. Otherwise it measures styled
graphemes by terminal cells and, before adding a nonfitting grapheme to a
nonempty row, emits that row and starts another with cloned prefix spans and
their measured cell width. It retains grapheme/style, line base style and
coalesces adjacent equal styles.

Prefix is inserted on continuation boundaries, not prepended to the original
input by this primitive. It is not clipped or independently required to leave
space for the next grapheme. Consequently a prefix plus indivisible grapheme
can exceed this intermediate width; a grapheme itself wider than budget is
also retained whole. These are finite iteration mechanisms, not a universal
fit guarantee or physical font qualification.

### Code line construction

For each collected code source line, `finish_code` finds its leading substring
using `trim_start_matches(char::is_whitespace)` and constructs a continuation
prefix containing that exact whitespace with code-background style. It then
keeps **all** highlighted ranges, adds a gutter/space to the original line and
invokes the prefix-aware wrapper at current content width before emission.
Continuation prefix is source indentation, not a repeated gutter.

Highlight failure uses plain ranges. Optional language label is a separate
code row; one final empty split after trailing newline is removed. Parser/
bundled syntax/theme invariant `expect` calls remain. Collected parser code
is not raw Markdown byte restoration.

All emitted rows, including these prewrapped code rows and any decorations,
then undergo final **empty-prefix** wrapping at requested document width.
Thus fitting indentation can repeat on code continuations, but excessive
indentation, indivisible wide graphemes or further repartitioning do not imply
the same indentation on every final row. Tabs/physical terminal tab stops are
not separately resolved by this code. Prefix cloning can amplify intermediate
text; rendering has no aggregate allocation/work quota. These observations
are from source, not executed stress cases.

`code_continuations_preserve_fitting_source_indentation` is recorded passing.
Its inspected source renders a fenced line with two leading spaces and
`abcdefghijklmnop` at width eight, asserts multiple measured rows through
eight, all rows after the first begin with two spaces, and strips spaces/
gutter before asserting complete body. It is not a test of arbitrary indent,
tabs, all nested decorations or physical fonts.

### Other content reachability and fidelity

Styled-word wrapping merges equal-style runs, whitespace-splits words and
can introduce a separator at inline style boundaries. Long tokens chunk
without tail clipping. This is not exact whitespace/source reconstruction.

Table rows chunk complete collected cells by whole graphemes, emit continuation
fragments through the tallest cell, align/pad fitting fragments and keep
overwide indivisible chunks without clipping. Columns shrink with a one-cell
floor; decorated rows get final wrapping. Collected cell tails are available,
not necessarily original syntax/spacing or intact mixed-column copy layout.
Private alignment still clips a grapheme-safe prefix, but table production
chunks before alignment and bypasses it on overwide chunks.

The supplied log names these inspected tests as passing:

- `highlighted_code_keeps_the_complete_tail_when_wrapped`: width-five complete
  ASCII/CJK body in concatenated rows and measured width bounds.
- `narrow_tables_keep_complete_cell_tails`: width-five one-column ASCII/CJK
  values after removal of rendering spaces, not exact whitespace.
- `styled_markdown_wraps_whole_graphemes_by_terminal_cells`: combining/CJK/ZWJ
  string at width four with concatenated content and no split combining start.
- `decorated_markdown_stays_within_narrow_cell_widths`: six selected paragraph/
  heading/quote/list/code/table sources at widths 3/4/7/10.

These finite assertions do not establish every parser construct, oversized
grapheme, prefix/decorated combination or terminal/font behavior.

## TUI rows, hidden reasoning and retention

Markdown provenance is Start(raw source) followed by Continuation. Resize
rerenders only that Start's consecutive Continuations, leaving following plain
notices/reasoning/another Start distinct. Other rows rewrap displayed text with
first-span style, not original paragraph/multi-span provenance.

Reasoning collapse stores existing consecutive runs. Initial label construction
already wraps at current content width and marks one Placeholder head plus
PlaceholderContinuations. Resize preserves that distinction. Reveal consumes
saved runs only at heads, discards label continuations and rewraps saved rows at
current width, then applies retention/clamp/follow. Incoming reasoning pushes
still create reasoning rows; collapse scans existing rows and repeating the
same setting is a no-op.

The 5000 retained-row cap drops oldest rows and drains hidden runs for evicted
heads, not continuation fragments. Evicting a head may leave displayed orphan
label fragments until reveal removes them. Markdown Starts can similarly be
evicted while Continuations remain; subsequent plain fallback cannot reconstruct
raw block/style. Retention is not block-atomic or a peak allocation cap:
render/reflow vectors are constructed before truncation.

Current app assertions named passing include initial width-eight labels/
two-run round trip, repeated resize preserving two distinct hidden runs,
width-eight reasoning reveal retaining 20 characters, old-head eviction
discarding one run but keeping another across 5000 fillers, Markdown resize
notice/reasoning sentinels and 4000 long-row expansion bounded after reflow.
They do not enumerate all eviction/resize combinations.

Plain wrapping carries leading indentation, normalizes inter-word whitespace,
omits whitespace-only paragraph content and can overflow when indent exceeds
width. Saved reasoning/plain rows are already wrapped; widening does not
restore all original boundaries. Pending streams/raw source/hidden data,
editor/history/queued prompts and replay have separate allocations beyond
the visible-row cap.

Streaming fences safely hold no-newline openers, match backtick/tilde characters,
minimum three and sufficient closing run with no content tail, and count leading
newlines in consumed bytes. Final flush may render unclosed text; pending
buffers lack independent aggregate caps. Replay/history navigation uses `usize`
physical wrapped rows/saturating increments and visible windows, including
70000-row assertions; ordinary transcript/help offsets remain `u16`.
OSC52 copies trimmed rendered text via staged base64/duplicate suppression,
not exact original bytes. Physical clipboard/terminal execution is unsupplied.

## Package/API and startup

The manifest declares `agent-runner` 0.2.0, edition 2024, Rust minimum 1.95,
`agent_runner` library and `agent-runner`/`agent-runner-file-tool` binaries.
Library unsafe is forbidden. It directly uses Unix descriptors/process groups
and Linux `/proc/self/fd`; portability equivalence is not observed.

Public modules are config/context/error/executor/file_tools/headless/history/
retry/sandbox/session/session_log/toolchain/tools/tui. Root exports include
Cli/discovery/import helpers; Config/Model/ModelProvider/ToolPolicy/SandboxPaths;
AgentSession/AgentRunner/RunLimits/RunSummary/Event/EventSink/ToolExecutor/
Recorder; SandboxExecutor and unconfined HostExecutor/ToolOutcome; context/
retry/profile/probe types; SessionLog; ToolRegistry/RenderedTool/ExecContext/
descriptions; and selected private-module Markdown/system-prompt exports.
Internal App/process/worker items are not public merely by internal spelling.

Canonical runtime request/message/turn/intent/stream types and transport trait
are consumed without provider-wire parsing here. Production config supports
llama-server/Ollama direct transport, configured deadline and 8 MiB response
bound passed to the dependency. Positive cadence sets a value; zero skips a
setter, leaving constructor behavior uninspected despite “disables” comments.
Injected library collaborators and directly constructible HostExecutor are
not constrained by production CLI sandbox selection. Prompts/descriptions
are not filesystem/network authority mechanisms.

CLI includes config/model/effort/cwd/profile, logs/context/run budgets,
headless/JSON, host opt-out/cap, prompt and listing/import dispatch.
Headless requires prompt and conflicts with host/no-log/list/import; JSON needs
headless. Host cap needs explicit host flag, defaults to one and validates
1–50; sandbox defaults 50, with invalid library caps rejected before model I/O.

Discovery is explicit/current-directory/user-XDG/system-XDG, not parent search.
Relative user XDG falls back; system entries lack the same restriction.
Closed schema-one TOML reads final no-follow regular UTF-8 through 64 KiB,
including growth checks. Validation covers absolute existing configured cwd,
model-list/default/duplicate IDs, nonblank provider model, effort/profile/
timings. Public fields/from_parts do not reproduce all checks; selectors
are not separately required nonblank.

Defaults/ranges: medium effort; deadline 300 seconds, 1–600; attempts three,
at least one without loader upper cap; delays 2/30 seconds, each 0–3600 and
base at most max; cadence 30, 0–600. Provider default endpoints are loopback
9931/11434; runner/backend default installed path and `/usr/bin/bwrap`.
Diagnostics escape controls and avoid raw parse-source echoes, not general
secret redaction.

Both execution modes choose cwd override and canonicalize before executor/
worker, rejecting missing/regular paths, resolving linked/relative directories.
Config loads before override, so invalid configured cwd can fail first.
Selected cwd feeds executor/metadata/default terminal logs/history/host prompt,
without global chdir; relative explicit logs use process cwd.
Startup advisory root enumeration has no quota/time and flattens entry errors.
Profile on/auto/off executable probing fails unavailable forced/on but can skip
auto; it does not qualify every described tool.

## Loop data and context authority

AgentSession owns ordered conversation/system/tools/model/effort and clears
stale answer on new prompts/runs. Requests prepend system text/use auto tools.
RunSummary separately reports answer, turns, executed tools, cancellation,
turn/budget exhaustion and failure; only answer without adverse flags succeeds.

Defaults are 1800 seconds, 1000000 estimated tokens, reserve 1024.
Limits require positive wall through 24 hours, tokens through 1000000000,
reserve 1–1048576. CLI window defaults 8192, exceeds reserve and caps at
1048576. Context warm-up must be below limit and recent retention positive.

Canonical JSON estimate is ceiling bytes/4 plus eight request, four/message,
eight/tool tokens, including escaping/schemas/generation fields; failure costs
`usize::MAX`. It is not an exact or universal model-token bound. Preparation
sets output reserve, validates complete result groups even on small requests,
preserves systems/latest genuine goal/newest safe group, compacts older groups
into labelled lossy User history through 4000 characters and actual spare
budget, and commits candidate/summary only on success. Legacy message
estimation/compaction lacks that complete request guarantee.

Joined watcher polls expiry/interrupts through 25 ms. Every attempt/retry
prepays estimate/reserve, deadlines grow linearly within policy/prompt remainder,
backoff is deterministic/capped with cooperative checks, and progress throttles
250 ms. Failed-attempt text stays provisional display rather than conversation;
sink failure cancels. Blocking injected callbacks/kernel I/O are not preempted.

Turns are recorded before validation; accepted Stop is nonblank/no calls,
ToolCalls has calls. Maximum 32, unique nonempty NUL-free IDs through 256 bytes,
names through 128, object args through 1 MiB encoded each. Invalid/truncated/
filtered turns never dispatch. Synchronized recorder dispatch precedes serial
effects. Repetition hashes name/args, not filesystem mutation. Policy/render
errors are feedback, other errors terminate; returned failed outcomes count
executed and can be model-handled. Remaining interrupted calls get rejected
pairs, but mid-fold record failure may leave incomplete history. No rollback.
Model feedback is combined status-bearing 8192 encoded bytes, rebounded after
lossy UTF-8 expansion.

## Filesystem tools and delegated sandbox policy

Stable generic tools are shell/read/write/list/find/search/edit. Rendering is
effect-free. Shell is one nonblank NUL-free command through 16384 bytes, Bash
argv and a literal case-sensitive denylist, not host confinement.

Native closed typed requests validate absolute lexical paths through 4096
bytes without dot/parent/NUL/duplicate separators. Mutations are strict
descendants on slash boundaries; reads may follow links in existing authority.
Bounds: 256 KiB helper payload, 1 MiB complete file, 64 KiB text argument,
1024 literal query, read default/max 4096/16384, collection 100/256, offsets/
visited/results through 4096, depth 32, scan 8 MiB, line preview 1024.
Adaptive complete JSON is below 7000 bytes (newline at most 7000), exposes
actual next offset and rejects oversized single entries. UTF-8 reads hash
complete content; descriptor walks/listing no-follow sort, report link/binary
skips and propagate ordinary errors. Literal search/find are not regex/glob;
pages are not frozen.

Mutation descriptor-walks non-link root/parents/target, checks preimage/digest,
edits exactly one overlapping-aware occurrence, preserves unrelated bytes/
CRLF/missing newline/mode, and exclusively stages sibling replacement with
64 collision attempts. Write/mode/sync and fresh bytes/device/inode/mode check
precede rename/directory sync. Not external-writer CAS, crash rollback or
ownership/xattr/time/hard-link preservation; post-rename errors can follow effect.

Helper validates bounded final no-follow payload. Production checks regular
non-link outside-workspace helper/ancestors, not ownership/execute mode, and
privately stages generated 0700/0600 requests in canonical workspace parent.
Writable parent is required, root workspace fails, ordinary-error RAII cleanup
does not imply crash cleanup, and no global staging fallback is used.

Version-one Authoring requests declare write-root workspace, outside regular
read-only contexts at `/context/N`, native payload/helper at `/context/0`/`1`,
network Deny, System `/usr`, environment/identities. Runner is outside workspace,
without equivalent backend builder separation. Workspace identity is path-based,
toolchain a fixed `/usr` marker, not content snapshots. HOME=/tmp is request
data, not independently observed backend mount behavior.

Production declares 120000 ms wall, 1 MiB capture, 256 processes, 4096 files,
64 MiB/file and 1 GiB scratch. Local supervision implements wall/capture;
remaining enforcement belongs to excluded backend.

Builder preflight checks actual token/shared 30 seconds, 1000000 entries,
depth 128 and 32 MiB path accounting (two OS copies, not all allocator memory).
Errors/escaping links fail; in-scope targets deduplicate/scan, unresolved
targets receive lexical containment. Identity reads are final no-follow/
nonblocking regular through 256 MiB each, growth checked and streaming-hashed
in 65536-byte chunks with cooperative checks. There is no aggregate identity
quota, locked tree/full ancestor descriptor identity walk/full spawn recheck.
Public builder creates a fresh token; production uses prompt token.

Execute rejects cancellation/encoded request over 1 MiB after allocation,
without repeating builder work or requiring caller-default resources.
Missing runner never becomes host fallback. Explicit host shell retains host
authority; native safeguards remain, wall 120 seconds, combined shell capture
8192/native 7001 bytes.

## Process, recording and headless/UI recovery

Shared owned-group pump uses nonblocking pipes, 8192-byte chunks and 5 ms idle
polling, exact combined capacity on all drains, distinct EOF/error and overflow
only beyond capacity. Cancel/time/overflow trigger termination; retained pipes
have a 250 ms drain window. WNOWAIT pins leader until original-group SIGKILL
and direct-child kill are independently attempted even on group success/ESRCH.
RAII/error/normal cleanup attempts one-second reap. Partial failed raw bytes/
status remain on successful cleanup; successful incomplete stdin is an error.
Escaped descendants and uninterruptible kernel work are not universally killed.
The shell fixture execs a sibling test script, not sandbox enforcement.

SessionLog has held descriptor no-link/private final directory, exclusive 0600
JSONL/transcript, sequence/schema one, operational scope/false canonical marker,
hashes/argument shapes/process flags and null mutation. Dispatch/terminal sync
is not every-event sync. Text caps at 64 KiB, not aggregate disk/rotation/
authentication. No unresolved-dispatch reconciliation or crash replay/rollback.

Worker keeps session/context across prompts and resets cancellation, with
128-slot events/unbounded prompt queue. Backpressure checks shutdown every
10 ms, not prompt token; idle polls 25 ms. Drop cancels/shuts down/joins but
arbitrary blocking collaborators can delay it. Model/effort changes start fresh
worker context; display clear does not reset conversation.

Headless rejects host/no-log at public entry, requires nonblank prompt through
64 KiB/private outside-workspace logs and default absolute XDG/HOME state,
without terminal setup. JSON is flushed ordered schema-one start/events/summary;
plain prints only successful control-escaped answer without added newline,
private/JSON original remains. Main completion/cancel/failure exits 0/130/1;
startup fails diagnostically.

History limits 4096 entries, 5 MiB/no-follow regular transcript including growth,
skips unusable entries and may return empty on overflow/list failure.
Text markers are unauthenticated; replay trims/lossily decodes/drops empty lines,
not exact bytes or executable resume, with no aggregate I/O/time limit.
Terminal requires stdin terminal, selected-cwd worker before raw/alternate
screen, sends staged prompt before loop/clears editor, polls 150 ms and
tears down sequentially/fallibly without unconditional restoration guard.
Prompt limit is 16384 characters, not aggregate history/editor memory.

## Finite live assertions and remaining gaps

The three supplied-passing live cases, directly inspected, establish:

- `real_sandbox_confines_native_files_and_denies_host_network`: one native
  workspace write/read, outside sentinel denial without leakage, one denied
  host-loopback TCP/unconnected listener and no workspace staging.
- `live_llama_streams_a_bounded_answer_without_tools`: cap 64/no tools/effort
  none, trimmed `OK`, streamed equals final, Stop/no intents and usage bound
  only when usage exists.
- `live_llama_reads_edits_and_verifies_inside_the_real_sandbox`: production
  loop/transport/executor, eight turns/180 seconds/100000 estimated tokens/
  reserve 1024, success, exact `answer = 42\r\nunchanged = yes`, successful
  edit plus at least two reads, no shell/write result or staging. Prompt `DONE`
  is not an exact final-answer assertion.

Other permitted tests contain finite config/input/context/loop/native/headless/
process assertions, including fake-runner supervision. Their results are
tabulated, not universal authority proof.

Concrete limits are prefix/row reconstruction rather than all-indent fidelity,
overwide indivisible cells/excessive prefixes, intermediate prefix-copy/
render vectors without aggregate quotas, row-not-block eviction, separate
unbounded pending/raw/hidden/history/queue allocations, unbounded advisory
startup scan, cooperative callbacks/I/O, filesystem/writer races, non-transactional
crash recovery and escaped descendants. Prefix-aware code wrapping has a
recorded fitting-indent example; it does not justify a blanket “never repeats
indentation” or “preserves all indentation” claim.

Missing universal evidence is separate: all indent/tab/font/decorated cases,
all collapse/resize/eviction combinations, all backend limits/escapes,
runtime/backend internals, exact tokenization/zero-cadence defaults, server/
weights identity and physical terminal/clipboard/non-Linux behavior are not
qualified by supplied executions. No requirement satisfaction or certification
is inferred.

## Complete 36-file byte inventory

Aggregate hashes lexicographically repository-path-sorted UTF-8 lines of hash,
two spaces, repository-relative path and newline. All entries match both logs.
Byte identity does not include ownership/permissions or unlisted dependencies.

```text
14e60a5926eaabff797996fd0cfa1a9e49859349b2cc95b21f356695f8cb2d4c  agent_runner/Cargo.toml
52cf1d709d6beb4cf5f9a55b889c4bf3cab60912c50934793bcdaf8700be09ca  agent_runner/src/bin/file_tool.rs
325c5bcd635425ee9c8d1b96a7597b1174d6e837abe3ac91298dcf9ab60b1f23  agent_runner/src/cli.rs
bab8ee23d2f1444454516dfd78e7970c073430857a5a4bcec7932ea2282b1002  agent_runner/src/config.rs
eee943aab3ea0874996e36df385e598394919fdc8b668e7ebc6bd38d6352fb76  agent_runner/src/context.rs
81eab7097f9a3807c6986dbb77712426cd848f09ca010326016ccee1d2615714  agent_runner/src/error.rs
58e71a462cbb2fe856427b4179997fadc380b8e7a2030662b49e87b2394e8e24  agent_runner/src/executor.rs
06d325dc6397b39a1af34519fbe4f30acf3f06062da6656b469d2ec0457879fa  agent_runner/src/file_tools.rs
9cd8c8a60bc3c6bb37afbdba41dc1d004763a4b4002671619b7700d483d95c91  agent_runner/src/headless.rs
380a7d47d5f38d706e3ebb16cd6cc75fb3eabb262c7e11e35559a3e9091863e7  agent_runner/src/history.rs
51ba9c3d0da6d7f1f4f3c756233ae95488ff07648ba0c5041160b02578825dfb  agent_runner/src/host.rs
187cc3a7670aad10ffaa73587dfab04fcbb4f770586328b8efd0c91a8c2dc342  agent_runner/src/kvist_import.rs
7fad87f4a0ece9ecbc83ebebd574c61f81f4a377f221ee76019c47dd64b517a1  agent_runner/src/lib.rs
e1fd8911c0f16ec0c4b7d27d792a988833bf20934e5280ad3e43f1b3bbaea2e2  agent_runner/src/logging.rs
9964b659c6c0aeb42c6e21a4e65c2fc5282e5fbd3e1e361864ebee1a47d0ff11  agent_runner/src/main.rs
6b681962ef8ce917f245c94306419e2ee5123feef053f9cc4a5ff64191180d93  agent_runner/src/markdown.rs
d0aacea446a4b2702c8a4ee84fc362e0e76d364c4d3a723b90f6f55e465bf97e  agent_runner/src/process.rs
91895900f1d169ee215e60f11ba8d779917abee6a7492d5326e1f12f8e1de4bd  agent_runner/src/retry.rs
6e0f21a026f04d19a5ec645b21fa204b250adacc4c062bba22bb483465ea512e  agent_runner/src/run.rs
f69d77fc89288397118e7324e46d0d6c2276494caf23a3b546606c908d5afbd4  agent_runner/src/sandbox.rs
98f40e74502d135297917f41bd3c2c9119f8265c2329226b279d18092d16a416  agent_runner/src/session.rs
eb8513df7c8ef9a23da976b63a0c9f6f1fdd7c0bca2373a435e907791ebfccc4  agent_runner/src/session_log.rs
69d92bf902b4a6285868fc236b928987e0e73d119e1234adda407d7c4f40436e  agent_runner/src/toolchain.rs
04e364b48b0894c56547424bb6bd5c19f150cc064e3976d748c6368efd9e5a0b  agent_runner/src/tools.rs
d50ece87784d06d272a5f7a9d4e3de6ec4d76aade92a34fc852694eb8165cdc7  agent_runner/src/tui/app.rs
b63e3be005726f71f668096227304a068fa3f3bdea37f6b96d2c22217bbf3863  agent_runner/src/tui/mod.rs
442235f5b1fec9e9de8afd37e9cf23abb106de1d37686a10b8b40b62360f89d7  agent_runner/src/tui/render.rs
874e7ab70fc9b8b4c4f5cd72e9005b21edf1ea96c9ef5dccbf6a11d88c361946  agent_runner/tests/component_tests.rs
b302849a1a50878813f9f133b7a26a4214e57319ad86e46610f9b51eb0ce906c  agent_runner/tests/context_preflight.rs
eb89cfaad9ce4f61ff85405197c281e106838120cf0a8cf788af66189a9abba5  agent_runner/tests/fixtures/subprocess_runner.sh
606512f914fe7a726c3ed78ce421c502801669c3198bfe849b4f828b9a40b8ee  agent_runner/tests/headless_cli.rs
f64d046ec8898cb67287cc961115942f486eea89c9272b5bb3d41b382bda0ec9  agent_runner/tests/input_boundaries.rs
74c26f530916e294710aa6be8ddfee0a10c97e21223d1c0b83106fc7c8b5bf6d  agent_runner/tests/live_llama.rs
97efc8a85353fe69524f19b19a0770fc2b44f9ee94cfa4aa50b647d98b716ce4  agent_runner/tests/loop_integration.rs
7d88376e10db1485fb8221c3f6b5469588322a7222dd8ea909f85c5813491419  agent_runner/tests/native_file_tools.rs
221784293daa1b53d3f5fb106fbce51510456e12dd9306a9bfd815d40dbb838a  agent_runner/tests/subprocess_supervision.rs
```
