<!-- kvist-design-version: 1 -->

# Agent Runner — Design

## October run remediation

Resolve model budgets in one shared startup helper used by headless and each
TUI worker. Use bounded model-qualified serving metadata for llama-server,
and actual loaded Ollama context where advertised; configured capacity is the
explicit fallback. Preserve CLI precedence without converting absence to 8192.
Automatic output reserve is min(8192, window/4); explicit reserves must fit.

The coordinator retains outstanding human messages independently of summaries
until normal completion. Context preflight protects those messages, all system
instructions and the newest complete tool group. Compact toward 65% of the
window, retaining as many recent groups as possible; if protected context
alone exceeds that target, fit against the hard limit or fail unchanged.
Summary references retain paths, returned offsets and process status before
lossy content prefixes. Binary previews are explicit text, not raw executable
bytes, and escaped JSON costs bound previews before they enter context.

Model-only length recovery clones the pre-effect request, increases output
reserve within its context and attempt budgets, and never folds truncated
intents into conversation or dispatches them. Operational notices are fallible
recorder events. Native repeat classification is a fixed tool-name allowlist,
never a guess about shell scripts. Distinct successful outcomes reset only
consecutive stalls, not the effectful action history.

Native traversal applies explicit generated-directory and literal path filters,
accounts for exclusions/oversize without loading oversized files, and keeps
all existing recursion/entry/byte and encoded-page ceilings. File search uses
secure non-following descriptor access and explicit file-vs-directory errors.
Rust integration remains a separately bounded host-selection/read-only-mount
path; it must not import engine policy/types or relax its closed Cargo phases.

## Security-first hardening design

No new task-approval, credential or promotion authority is
introduced.

The coordinator performs full-request preflight before every request, including
the system prompt, complete schemas, and an enforced output reserve. Context
groups end only when all assistant calls have results; autonomous tool turns
are individually compactable while the latest user goal remains explicit.
Summaries carry a lossy/non-authoritative label. Failure to fit is an explicit
error, not a larger send or a silent discard of instructions.

Each prompt gets fresh answer and loop-detection state. A wall deadline spans
requests/backoff/effects. An owned deadline watcher cooperatively cancels the
same token used by providers and executors, and is joined on every exit.
Backoff polls cancellation with short bounded waits. Rejected/cancelled calls
get paired results before returning. Model terminal reasons are classified
before any effects; malformed, truncated, duplicate and filtered proposals
cannot execute.

Recording is fallible, pre-effect and synchronized. A local, versioned journal
separates dispatch from result, records argument shape/content hashes and
process flags, and represents mutation as unknown unless observed. The
readable transcript is private diagnostic text, not secret-free evidence.
Required headless recording cannot share an agent-writable directory.
Every exit attempts a terminal record; recording errors remain observable.
Unknown effects after interruption remain fenced by the absence of automatic
replay.

File tools are implemented in Rust using bounded input, closed typed arguments
and descriptor-relative no-follow traversal for writable paths. A separate
small helper runs inside the installed isolation boundary; it does not confer
process isolation when manually invoked. The broker mounts the helper and a
private payload read-only, and never stages files using provider IDs or writes
payloads into the workspace. Exact editing preserves raw unrelated bytes and
requires a SHA-256 preimage. Atomic replacement is per file, with stale-content
revalidation immediately before replacement; unrelated external writers are
not a transactional locking participant. Paginated reads/search/discovery are
deterministic and bounded, including recursion, scanned bytes, file size and
encoded output.

The headless CLI and TUI share transport/executor/session construction. NDJSON
events have a version and sequence; stdout is reserved for events or answer
text, with diagnostics on stderr. Headless mode has no host-execution escape
hatch and always records. The engine's separate protected broker remains the
only task-authoring integration.

This document explains how `agent-runner` realizes the requirements and contract.
It covers module layout, the agent loop, tool rendering, tool-chain advertisement
and gating, sandbox request construction, the terminal UI, cancellation, and the
edge cases that the tests cover.

## Design overview

```
src/
  lib.rs        crate roots, public re-exports
  main.rs       process entry: logging, CLI dispatch, exit codes
  cli.rs        clap parser and top-level command
  error.rs      Error/Result, exit codes, actionable messages
  logging.rs    tracing subscriber (mirrors Kvist conventions)
  config.rs     Config, Model, SandboxPaths, loading and validation
  toolchain.rs  ToolProfile/ProfileSetting/ToolchainProbe, detection and gating
  tools.rs      ToolRegistry, ToolPolicy, tool -> sandbox command rendering
  file_tools.rs closed native requests, bounded traversal, exact file effects
  bin/file_tool.rs separately installed sandboxed native helper entry point
  executor.rs   private payload staging, helper grants and tool execution
  host.rs       explicit interactive unconfined opt-out, never fallback
  process.rs    shared bounded tool I/O, stdin and owned process-group cleanup
  context.rs    complete-request preflight and complete-group compaction
  session_log.rs private versioned fallible operational journal
  headless.rs   terminal-free same-loop execution and NDJSON
  sandbox.rs    request construction (Authoring phase) + executor
  session.rs    AgentSession (turn model) + AgentRunner (loop + events)
  run.rs        worker: drives AgentRunner on a thread, channel of events
  tui/
    mod.rs      ratatui run loop, event handling, wiring to run.rs
    app.rs      App state: transcript, selectors, input, status
    render.rs   pure transcript model + ratatui drawing
tests/
  component_tests.rs   configuration, registry and sandbox requests
  loop_integration.rs  injected transport/executor/recording lifecycle
  context_preflight.rs complete-request and group-boundary regressions
  native_file_tools.rs strict file semantics, bounds and staging
  headless_cli.rs      actual CLI and loopback provider fixtures
  live_llama.rs        opt-in actual provider and installed-boundary trials
```

Separation of concerns: `config`/`toolchain`/`tools`/`sandbox` build trusted
structures from untrusted inputs; `session` owns the model-agnostic conversation
and loop policy;
`run` owns process/thread plumbing; `tui` owns only presentation. Nothing in
`session` performs blocking subprocess I/O directly — it hands tool intents to a
`ToolExecutor` trait implemented over the sandbox. Tests inject a
fake transport and a recording executor.

## Internal structure

`config`/`toolchain`/`tools`/`sandbox` build trusted structures from untrusted
inputs; `session` owns the model-agnostic conversation and loop policy; `run`
owns process and thread plumbing; and `tui` owns only presentation. Nothing in
`session` performs blocking subprocess I/O directly — it hands tool intents to a
`ToolExecutor` trait implemented over the sandbox, which keeps the
conversation model unit-testable in isolation.

## Interactions and state

The loop is the classic stream-and-execute cycle, kept transport-agnostic:

1. `AgentSession::next_request` returns the next `ModelRequest` built from the
   accumulated `messages`, the tool definitions, `tool_choice = Auto`, and the
   selected `reasoning_effort`.
2. `AgentRunner` calls `transport.stream(request, token, on_event)`. The
   `on_event` closure forwards `TextDelta`/`ReasoningDelta` as `Event::Text`/
   `Event::Reasoning` to the UI, and buffers `ToolIntent` events.
3. After the turn, `ModelTurn.tool_intents` are the resolved proposals. For each
   intent in order, the runner: renders the tool to argv via the registry
   (rejecting policy violations), executes it in the sandbox (bounded,
   cancellable), and emits `Event::ToolResult`.
4. Each tool result is folded into the conversation as a `ToolResult` message
   (combined bounded preview with process status) and the loop repeats.
5. The loop accepts only normal `Stop`, no tools and nonblank text as a final
   answer. Invalid finishes/identities fail before effects. Every rejected or
   interrupted pending call gets a paired result.

Cancellation: a shared `CancellationToken` (from `agent_runtime`) is checked
between turns and is threaded through the sandbox executor, which terminates the
process group on interrupt. `agent_runtime::install_handler` routes Ctrl+C/SIGTERM
to a cooperative flag so the whole process stays safe.

Why a worker thread: the model transport and sandbox are blocking. The UI runs
on the main thread and would otherwise freeze for the whole turn. `run::start`
starts a fallibly spawned owned thread and pushes `Event`s over a bounded channel;
the UI reads events each frame and renders them, showing a status spinner while
a turn is in flight. Handle drop sets cancellation and a separate shutdown flag,
then joins. Event sends and prompt waits poll shutdown; retained prompt senders
and full event queues cannot prevent teardown. UI worker field order closes both
channels before joining on quit, errors and model switches.

Startup is lazy so the terminal UI appears immediately instead of waiting on
the provider: `run` hands the UI loop a receiver from `spawn_bootstrap`, which
runs `SessionBuilder::start` (transport construction, provider connection,
model load) on a dedicated `agent-bootstrap` thread. The transcript shows a
`starting model…` status in the meantime; a held initial prompt (for example
one supplied positionally) is dispatched to the worker as soon as it is
installed. A failed bootstrap is reported as a failure notice, not fatal: the
UI stays usable and the next submission or new session starts a fresh
bootstrap. Submitting with a changed model or effort cancels the old worker,
discards a stale in-flight bootstrap, starts a new one, and holds the prompt
for it. The transport watchdogs are part of the turn budget: the
slot-allocation phase (provider acceptance, including model load/switch) and
the time-to-first-token phase (long-prompt prefill) are each granted the full
per-turn `deadline_secs`, so a slow model switch completes within the turn
instead of failing on a short fixed probe; the inter-token cadence watchdog
(default 30 s, configurable) still catches a stall after the first token.

## Independent review phase

The review phase is a second sequential execution of the existing headless
machinery, not a new loop. `headless::run` keeps its current flow; when the
implementation `RunSummary` reports `completed` and the resolved review policy
is enabled, it invokes a new `review::run_phase` with the implementation
summary, the original prompt, the working directory, the resolved policy, and
the shared output sink. Nothing from the implementation session is reused: the
phase builds its own `AgentSession`, `ContextManager`, transport, executor, and
`SessionLog`, so the reviewer context is empty by construction.

`review.rs` owns the phase: policy resolution, reviewer model selection,
prompt construction, registry shaping, report parsing, and event emission.
`headless.rs` only sequences the two phases and folds the phase outcome into
the process result. Prompt construction is a pure function of (write root,
mode, original prompt, implementation answer) and is unit-testable without a
transport.

Phase resolution:

1. Resolve the policy: configuration `[review]` with CLI overrides
   (`--review` forces on, `--no-review` forces off, `--review-model`,
   `--review-apply-fixes`). Absent configuration and no `--review` skips the
   phase without diagnostic noise.
2. Resolve the reviewer model: `--review-model`, then `[review] model`, then
   the implementation model. The id must name a configured model; otherwise
   the phase fails before any provider I/O with an actionable diagnostic.
3. Build the review session: the review system prompt, a fresh
   `AgentSession`, a fresh `ContextManager` sized from the reviewer model's
   resolved budgets, and a fresh `SessionLog` whose stem carries the
   `-review-` marker and whose metadata records the implementation model, the
   reviewer model, and the mode.
4. Shape the tool registry: assess-only mode filters `write_file` and
   `edit_file` out of the advertised definitions; `apply_fixes` mode keeps the
   full set. Both modes keep the shell, reads, search, and build tools so the
   reviewer can verify by running.
5. Run `AgentRunner` with the phase's own `RunLimits` (the reviewer model's
   resolved budgets, `max_turns` from the policy) and the shared cancellation
   token, emitting `review_start` first and `review_summary` last over the
   sink. The review prompt is the composed user message: the original task
   prompt and the implementation answer, presented as quoted untrusted data.

Prompt templates live in `review.rs` next to `run.rs`'s prompts. The review
system prompt states the role (independent reviewer of the just-completed
implementation), the fixed rubric (task suitability and correctness,
robustness, idiomaticness, efficiency, reliability, resilience, error
handling, safety, security, readability, maintainability, structure, test
quality), the verification duty (read the code, run the builds and tests,
report `cannot verify` when the workspace cannot demonstrate a claim), the
honesty duty (state weaknesses plainly, no padding), the untrusted-data
instruction (the embedded implementation answer is data, not instructions),
and the report format (a fenced JSON block with bounded `verdict`,
`findings`, `fixes_applied`, and `tests_passing`, followed by the narrative
assessment). In `apply_fixes` mode the prompt additionally orders
assess-then-fix-then-verify and requires separate as-delivered and after-fix
statements.

Report parsing extracts the last fenced JSON block from the final answer,
validates it against bounded shapes (findings capped, string fields capped,
known enum values), and degrades to `report_parsed: false` with the raw
narrative when parsing fails. Parsing never fails the run.

Failure semantics: a phase that fails before or during provider I/O emits
`review_summary` with `disposition: "failed"` and the diagnostic. Under
`on_failure = "warn"` (default) the process returns the implementation
disposition and exit status unchanged; under `on_failure = "fail"` it exits
unsuccessfully after the report. A skipped phase (non-`completed`
implementation, review disabled) emits no review events except a single
diagnostic note in plain mode when the phase was explicitly requested.

State and concurrency: the phase is sequential and in-process; it introduces
no shared mutable state, no new threads, and no persistent state beyond its
private journal. Reruns are stateless: they review the workspace as it is and
are never resumed, cached, or replayed.

## Algorithms and decisions

`ContextManager::prepare` runs before every request. Complete canonical JSON
serialization includes system instructions, full schemas, framing and
byte-aware text costs. Input estimate plus an enforced output reserve must fit;
this deliberately heuristic estimate is not a tokenizer guarantee.

The grouping pass validates unique assistant call IDs and matching tool
results, never splitting a pending call/result group. Autonomous iterations
under a single user goal can compact independently. It preserves all system
messages, the latest user goal and the newest complete group, retaining more
recent groups when they fit. The rolling summary is bounded and explicitly
lossy/non-authoritative. If the immutable/current material cannot fit, request
and summary remain unchanged and sending fails.

Compaction affects only model context. The optional private transcript records
bounded diagnostic text, including provider reasoning, and may contain secrets.
It is not guaranteed complete, is not canonical evidence and cannot restore
effects. The legacy `compact` helper is diagnostic only, not the send path.

When a compaction happens the loop emits `Event::Note`. The UI surfaces
context utilization and a compaction progress bar via `Event::Progress`.

### Tool rendering

The registry exposes seven tools, in stable order: `shell`, `read_file`,
`write_file`, `list_dir`, `find_files`, `search_files`, `edit_file`.
Each is rendered to an argv whose `[0]` is an absolute
canonical path, so the sandbox accepts it and no shell globbing or PATH lookup
happens on our argv.

- `shell { command }` → `["<bash>", "-c", "<command>", "agent-runner"]`. The
  command string is the agent's own script. Bash resolves inner tool names via
  the sandbox `PATH` we set (prepared `/rust/runtime/bin` first when available,
  then `/usr/bin:/bin:/usr/sbin:/sbin`). The `shell`
  command string is checked against the denylist before rendering.
  Commands longer than one 4096-byte protocol argv entry render instead as
  `["<bash>", "-c", "exec bash -c \"$(cat /context/0)\" agent-runner"]` with
  `shell_script` carrying the text; the executor stages it as a mode-0600 file
  in a mode-0700 private directory in the workspace's canonical parent (never
  inside the writable mount) and the request mounts it read-only at
  `/context/0`. The exec'd `bash` performs the quoted command substitution
  (substitution results inside double quotes are not re-expanded, split, or
  globbed, so the contents reach the interpreter verbatim) and then `exec`s
  itself into the real interpreter, so the process tree, script text, exit
  status and `$0` match the inline form exactly; trailing-newline stripping by
  `$(...)` is semantically inert for shell scripts. The wrapper is required:
  a bare `$(cat /context/0)` as the `-c` script would make the exec'd bash run
  `cat` and word-split the output into a command. The content identity rides
  on the context grant identity, while `identities.command` identifies the
  argv.
- Native file operations carry closed typed JSON, never shell snippets.
  The executor stages the payload mode 0600 in a mode-0700 host-owned temporary
  directory outside the workspace, with RAII cleanup on every exit. Provider
  IDs do not select host paths.
- The installed non-link executable helper and payload are exact read-only
  context-file grants at `/context/1` and `/context/0`. The helper receives only
  that payload path; absent/untrusted/workspace-contained helpers fail closed.
- Reads return UTF-8 text, digest and accurate byte-page metadata. List/find/
  literal-search results are stable bounded pages. Complete encoded results
  fit 7000 bytes, leaving room for the loop's 8-KiB process-status preview.
- Mutation uses descriptor-relative no-follow directory traversal, rejects
  linked targets and uses atomic per-file replacement, preserving permissions.
  Exact edits require a SHA-256 preimage and one literal occurrence, including
  rejecting overlapping matches. CRLF and unrelated/missing-newline bytes stay
  unchanged. External writers do not participate in a transactional lock:
  stale checks do not promise atomic compare-and-swap against arbitrary writers.

`bash` is resolved once to its canonical path at request construction. The
registry renders the sandbox-relative path the agent passes (typically under the
write root `/workspace`); the agent works in the sandbox view, so tool output
paths are consistent.

Write scope is enforced in two places. The registry rejects writes/edits when the
target is not inside the configured `write_root` on a slash boundary (so a sibling
such as `/workspace-evil` is rejected when the write root is `/workspace`, not
merely when it fails a bare `starts_with`), and the sandbox grants read-write
authority only at the write root, so any other write fails inside the sandbox
regardless.

### Tool-chain advertisement and gating

The `shell` tool advertises the tool-chains available inside the authoring
sandbox through the `toolkit()` strings of the enabled `ToolProfile`s. Advertisement
must be honest: the sandbox mounts the read-only System layout and, when
prepared, explicit read-only Rust resources, while clearing the environment.
A profile is advertised only when its interpreter genuinely reaches
the sandbox, never against the host `PATH`.

The System gate lives in `ToolRegistry::resolve`. Sandboxed terminal/headless
startup uses `resolve_for_workspace`, adding validated installed Rust resources
and a bounded private vendor snapshot without changing the engine Cargo topology.
The fixed Cargo shim prepends `--offline --locked` and source overrides to every
normal PATH Cargo invocation; Cargo handles missing/stale locks before a build
can update them. Flags also apply to metadata and checks; version/help remain
usable without a project lock. Explicit alternate executable paths are not
rewritten. This is a locked-build default, not an artifact write-protection
mechanism for the standalone writable workspace. Concrete compiler,
rustdoc and native libraries remain under `/rust/toolchain`; the trusted shim
is under `/rust/runtime`. `HOME=/tmp`, `CARGO_HOME=/tmp/cargo-home` and
`CARGO_TARGET_DIR=/tmp/target` are private invocation scratch, not host caches.
Selection uses the standard host rustup layout, ignoring ambient overrides and
custom linked roots; engine `.kvist/rust-toolchain.json` is not authorization.
Preparation bounds are 30 seconds, 1 GiB vendor aggregate, 256 MiB/file,
100,000 entries, depth 64 and 32 MiB retained paths. Stage ownership is shared
with the registry/executor and removed when the final owner drops. Pin/root
substitution and tracked executable/library/shim drift fail before dispatch;
the identity is not a complete toolchain-tree digest.

The vendor snapshot is two-phase: a serial enumeration validates every entry,
enforces the bounds, creates destination directories and schedules files in a
deterministic order, then a bounded pool (minimum of 8 and host parallelism)
copies and digests files concurrently. Each worker re-checks its own source
fingerprint immediately after copying its files, so a concurrent host change
fails that file without trusting a global recheck; the directory fingerprints
are re-checked after all copies. The identity folds the per-file SHA-256
digests (length-prefixed relative path, copied byte count, digest) in the
deterministic enumeration order, so it is stable across runs and binds every
path and byte without a serial re-read.

Per-invocation validation compares each tracked executable/library/shim against
the dev/ino/size/mtime/ctime fingerprint captured at resolve time; any
modification, truncation or replacement updates at least one field, so strict
drift detection no longer re-hashes hundreds of megabytes per tool call. The
exact byte identities remain captured once at resolve time and bound into the
environment and grant identities.
Explicit host execution keeps the System-only registry. The System gate
combines three inputs:

- the per-profile `ProfileSetting` from `[tool_profiles]` (default `Auto` for every
  configurable profile, per `Config::from_parts`);
- a `ToolchainProbe` — `HostProbe` in production, which judges availability against
  the sandbox's read-only `MOUNTED_SYSTEM_DIRS` (`/usr`, `/lib`, `/lib64`, `/bin`,
  `/sbin`) rather than the host `PATH`; and
- an optional forced profile from `-p`/`--profile`.

`Generic` is always advertised. A configurable profile set to `On` (or forced by
the CLI) advertises only if its interpreter reaches the sandbox and otherwise fails
startup with `Error::ToolchainUnavailable`; `Auto` advertises only when available;
`Off` never does. `language_available` canonicalises each candidate and ignores any
interpreter whose path ends in `rustup`, so a `rustup` stub named `cargo`/`rustc`
is not advertised as a buildable tool-chain.

Project-language detection (`detect_languages`) is advisory only: it scans root
manifests (`Cargo.toml`, `pyproject.toml`, `package.json`, `go.mod`,
`CMakeLists.txt`, …) and is logged to inform the operator. It never gates what is
advertised — availability does. A setting of `on` for a profile the sandbox cannot
run is treated as an explicit, user-intended request and fails closed, so the model
never receives a tool list that promises a compiler it cannot invoke.

## Failure and recovery

Built-in executors share bounded subprocess supervision. Combined capture
accounts for both output streams before buffering, including fast exits and
final draining. Nonblocking pipes or finite queues keep intermediate storage
bounded. Stdin pumping shares cancellation and wall checks, so a non-reading
runner cannot strand the worker. Each spawned child owns a fresh process group;
error/cancellation/timeout/overflow cleanup signals the original owned group
and independently terminates the retained direct child before reaping it.
An absent or successfully signalled group does not establish direct-child
termination: an unconfined child can change its group membership. Cleanup
never follows that child into the supervisor's own group.
Request construction checks the same cancellation/deadline token while hashing
identity files in 64-KiB blocks and scanning the workspace. A held non-following
regular descriptor limits identities to 256 MiB, including concurrent growth.
Deduplicated directory scheduling charges two retained path representations,
caps their bytes at 32 MiB, and rejects depth above 128 or more than 1,000,000
entries. One 30-second cooperative preflight guard bounds both stages.
Enumeration/read-link failures are errors, not skipped entries or empty targets.
Post-exit retained streams have a finite cleanup window and explicit failure.
No drain reader is detached. Uninterruptible kernel work and genuinely escaped
host descendants are not claimed terminated merely because cleanup was attempted.

Configuration opens use nonblocking no-follow descriptors, regular-file
validation and a bounded read that detects growth. Pure registries advertise
only Generic; language resolution requires executable candidates under mounted
system roots and does not run arbitrary probes or promise every companion tool.

Streaming Markdown buffers incomplete fence openers until a newline arrives.
Consumed block lengths include leading separators. Wrapping preserves grapheme
order and measures terminal cells, carrying indentation onto continuation rows.
History/replay offsets and physical row counts use `usize`; render only the
selected wrapped viewport rather than narrowing offsets to the widget's `u16`.
Replay Down/PageDown advances before clamping. CLI host-turn overrides require
the explicit host-execution flag, even when no prompt is supplied.

- A turn that hits a recoverable, temporal transport error (dropped connection,
  provider timeout, or transient server error) is retried: the worker waits a
  capped exponential `backoff_delay` and replays the turn with a fresh request and
  a larger per-turn deadline, reported to the user via `Event::Note`; a retry that
  exhausts `max_attempts` is surfaced as `Event::Failed`. Cancellation is never
  retried.
- A shared `CancellationToken` (from `agent_runtime`) is checked between turns and
  threaded through the sandbox executor, which terminates the process group on
  interrupt; `agent_runtime::install_handler` routes Ctrl+C/SIGTERM to a
  cooperative flag so the whole process stays safe.
- A missing or unverified sandbox runner or backend fails closed with an actionable
  diagnostic and never triggers host execution; oversized or non-UTF-8 output is
  truncated and reported rather than buffered unbounded.

## Security and resource design

`sandbox::build_request` assembles a version-one `SandboxRequest` in the
`Authoring` phase, reusing the shared `kvist_sandbox_runner::protocol` types so
the wire shape is identical to the runner's own statement of the contract.

Identities are honest `sha256:` digests rather than placeholders: `runner` is
the hash of the runner binary bytes, `backend` is the hash of the bwrap bytes,
`toolchain` identifies the system toolchain root, `command` identifies the argv,
`mount_plan` identifies the sorted grants, and `policy` identifies the tool
policy. The runner only re-verifies the backend at runtime, but computing all
of them keeps the request auditable and self-consistent, and `identities.toolchain`
is required to equal the toolchain block identity.

Grants: one read-write `authoring` grant (working directory → sandbox write
root) and one read-only `context` grant per exact regular non-link context file.
Missing/directory/overlapping context sources fail explicitly. No scratch
grant is declared: the runner already mounts a private `/tmp` tmpfs in every
sandbox, so that area serves as scratch and `HOME` points at `/tmp` (a dangling
`HOME` would break tools that write home-relative state). Destinations are
disjoint, so the runner's overlap check passes; the writable-scope check
rejects only symlinks whose resolved target escapes the working directory
(symlinks that stay inside the scope are allowed), so the build succeeds for
real projects that ship in-project links. If the working directory contains a
symlink that could write outside the scope the build fails closed with an
actionable message naming the link and its target.

Network is `Deny` and no Cargo cache is declared, which is what the `Authoring`
phase requires. Resources are bounded well below the runner's maxima.

## Terminal UI

Startup resolves the selected default or override directory through one
canonical existing-directory validator shared with headless execution, before
terminal setup or tool selection. Markdown rows distinguish the source-bearing
first row from explicit continuation rows; resize replaces only those rows,
never subsequent plain notices or reasoning. Styled wrapping and clipping use
terminal cells and whole graphemes. Final rendered rows are wrapped after
gutter/list/table decoration, and resized transcript retention remains bounded.
An indivisible glyph wider than the available viewport cannot be made to fit;
this physical limit is not a promise of universal terminal/font behavior.
Code keeps complete highlighted spans for final cell wrapping rather than
clipping tails. Each table cell is split into whole-grapheme chunks and emitted
on successive rows; decoration is wrapped afterward. Revealed reasoning is
reflowed at the current width and rejoins bounded visible retention. Evicted
placeholders discard their corresponding hidden runs so surviving placeholders
cannot restore the wrong reasoning.
Wrapped placeholder continuations are explicitly distinct from the first
placeholder row and never consume another hidden run during reveal.
Initial placeholder construction also wraps to the current width with the same
head/continuation distinction, before any resize occurs.
Highlighted code wrapping repeats source-leading whitespace on continuation
rows when it fits; the gutter remains on the first row. Overwide indentation
still requires the explicitly unresolved width/indentation policy decision.

The UI is built with `ratatui` + `crossterm`, following the same line-editor and
theme-detection spirit as the Kvist shell but as a full-screen transcript rather
than a line REPL. `App` holds the transcript (`Vec<TranscriptRow>`), the selected
model, the selected effort, the running flag, the input buffer, a scroll
offset, and a help-overlay flag. Rendering is a pure function of `App` state, so
the transcript model is unit-tested without a terminal. The header persistently
labels actual sandboxed or HOST UNCONFINED scope; model instructions use the
same selected scope. Reasoning collapse retains other rows and restores hidden
reasoning in order.

The transcript and prompt areas are panels with a single top edge (no left,
right, or bottom borders) so each gains a row and a column of content over a
fully boxed panel. Every transcript row is filled to the panel's full inner
width: ratatui's `set_line` only styles cells up to the last span, so the
renderer appends a trailing space span carrying the row's background. All
output defaults to the standard panel background — black in the default `dark`
theme, white in `light` — so the transcript reads as one quiet surface. Two
cues set content apart from it: model reasoning carries a left edge (▌ first
row, │ continuations) plus a muted tint, and highlighted code keeps its
gutter/indentation plus the theme's code patch. The prompt echo is distinct by
its bold prompt foreground alone. A scrollbar rides the transcript's right
inner column at all times, sized to the panel's inner height, with a thumb that
encodes the current window (content length, viewport, offset) into the full
transcript, so it adapts to both scrolling and terminal resizes. Resized rows
keep their kind and background.

Theming lives in `tui::theme`: one `Theme` table per built-in theme supplies
every colour the render layer and each transcript row reads (panels, edges,
reasoning, prompt, notes, stats, scrollbar, menu, and the full
`MarkdownStyles` table, including the code patch). The configuration's
`theme` key selects `dark` (default) or `light` — unknown names fail at load —
the CLI `--theme` flag overrides it per run, and Ctrl+S cycles the live theme:
existing rows are restyled in place (prompt rows take the prompt style,
reasoning rows are rebuilt with the edge and tint, everything else gains the
panel background) while text, Markdown spans, and code highlights are left
untouched.

Transcript text is pre-wrapped to the panel's inner width (the full terminal
width minus the top edge's column) so no line extends past the visible
area, and tool-call events carry a short description of what applied where
(file, directory, or command). The `wrap` helper splits each source line at
word boundaries and prefixes every continuation line with that line's leading
whitespace, so text blocks keep their indentation in the transcript; an
unbreakable word is hard-split at the width. The help, menu, session-history,
and replay panels render through ratatui `Paragraph` widgets with soft wrapping
(`Wrap { trim: false }`) so long lines (configuration paths, session
summaries, replayed content) wrap inside the panel border instead of being
truncated, and their scroll clamps use the paragraph's rendered line count for
the panel width so all wrapped content stays reachable. Events from `run::start`
are folded into `App` each frame. Ctrl+Enter submits; Enter submits on a blank
line and otherwise inserts a newline. Ctrl+C cancels (or quits when idle),
Esc opens the action menu, Ctrl+H opens help, PageUp/PageDown scroll,
Ctrl+P steps backward through prompt history, and Ctrl+N starts a new session
from anywhere. The ESC menu closes on selection: a history item transitions to
the replay overlay, and "New session" (Enter or `n`) starts a fresh session.
Terminal prompt submissions are bounded at 1,048,576 characters so long briefs
run without interruption.
Session records are named `session-{UTC date-time}-{pid}-{n}` in the log
directory; `Recorder::on_prompt` renames both the journal and the transcript
(via `renameat` against the held directory descriptor, so path substitution
cannot redirect the record) to append a bounded slug of the first prompt's
first words, e.g. `session-2026-10-03T14-22-05Z-4242-1-fix-the-bug.log`, making
transcripts recognisable by their content.
The status bar shows `model | effort | status` and a cancel hint; the prompt line
shows the input with autocomplete-free editing to keep the dependency surface
small.

A live stats bar reports progress without cluttering the transcript. It shows
the output-only generation speed (`⚡ N t/s`, prompt processing and tool time
excluded — the figure comparable to `llama-server`'s per-prompt tokens/sec),
the session-wide average throughput (`avg N t/s`, cumulative provider tokens
over wall clock), context utilization (`ctx ▇▇▇▇▇▇▇░ +P% cur/limit`), cumulative
processed tokens in short units (`123.4k tok`), and elapsed time (`m:ss` or
`h:mm:ss`). The compaction field (`compaction ▁▃▅▇ P%`) appears only while the
live context is past the warm-up threshold, i.e. only when compaction is in
play. Every field is padded to a fixed width so values hold their columns while
magnitudes change. Each stat is derived from `Event::Progress`, and the stats
line is unit-tested without a terminal so the presentation model stays
verifiable.

The compaction bar also carries an ETA (`compaction ...% (in 2m12s)`), forecast
from the context growth rate observed between consecutive progress samples. The
ETA is computed in the presentation layer (`App`), not the transport-facing
`Event`, so the event contract stays stable and the estimate is derived from the
observed event stream. It is shown only while the live context steadily climbs
toward the hard limit (`compaction_progress > 0`), and suppressed when the
context is flat, was just rolled back by a compaction, or grows too slowly to
trust. No speculative or misleading number is ever shown.

## Verification strategy

- Empty prompt is ignored, not submitted.
- A normal nonblank Stop with no tools returns an answer; other zero-tool
  finishes fail.
- A tool that renders outside the write root is rejected before the sandbox; a
  sibling prefix such as `/workspace-evil` is rejected when the write root is
  `/workspace`.
- A shell command matching the denylist is rejected before the sandbox.
- A missing runner/backend path fails the session, not the host.
- A request whose working directory contains a symlink that escapes the writable
  scope fails the sandbox build (in-project links that stay inside are allowed).
- Cancellation during a turn reports `cancelled` and stops the loop.
- Oversized output is truncated and reported, not buffered unbounded.
- Config with wrong `schema_version` or unknown model selector fails to load.
- A profile set to `on` or forced via `-p` that is not available inside the
  sandbox fails startup with `ToolchainUnavailable`; `auto` advertises only the
  profiles whose interpreter reaches the read-only System/prepared resources, and `rustup`
  stubs are not treated as usable tool-chains.
- `detect_languages` reports the project's detected language without gating
  advertisement.
- Deterministic ordering of tool definitions, grants, and identities.
- Preflight fits complete request plus reserve or fails before I/O; compaction
  keeps systems/current goal/newest complete group and never orphans results.
- The rolling summary contains each rolled turn line exactly once, even when a
  tight window forces multiple compaction passes.
- Progress accounting is emitted after every turn with cumulative input/output
  tokens, so the live stats and the compaction ETA update during the session.
- The stats line is empty until progress is reported, then reports speed,
  context, compaction, cumulative tokens, and elapsed time.
- A completed headless run with review enabled starts exactly one fresh review
  session (new transcript, journal, and context) and emits `review_start` and a
  bounded `review_summary`; a non-`completed` run skips the phase with a
  diagnostic note.
- Review policy resolution honors `--review`/`--no-review`/`--review-model`
  over configuration, rejects unknown reviewer model ids before provider I/O,
  and keeps `write_file`/`edit_file` out of the assess-only tool definitions.
- A review failure under `on_failure = "warn"` preserves the implementation
  disposition and exit status; under `"fail"` the process exits unsuccessfully
  after the report.
- An unparseable reviewer report degrades to `report_parsed: false` with the
  raw narrative and never fails the run.

## Security posture

- No per-action prompts: authority is established once in configuration.
- Fail closed: a missing or unverified sandbox runner/backend never triggers host
  execution.
- Untrusted inputs (config, tool arguments, subprocess output) are validated for
  schema and bounds; argv entries are NUL-free and bounded; the request size is
  bounded before parsing.
- Environment values passed to the sandbox are a small allowlist; dangerous
  names (`LD_*`, `GIT_*`, `CARGO_*`, proxies) are not forwarded.
- Arguments and output values are hash-only in the operational journal.
  Private transcripts may contain sensitive text; logging can be explicitly
  disabled in interactive mode, never headless. Neither file is engine evidence.
- The denylist and write-root enforcement are tested so a regression cannot
  silently widen authority.
- The review phase is advisory and reuses the closed sandbox request; assess-only
  mode withholds the native write tools, and review reports are never canonical
  evidence, compliance decisions, or acceptance receipts.
