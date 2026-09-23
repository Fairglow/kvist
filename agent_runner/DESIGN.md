<!-- kvist-design-version: 1 -->

# Agent Runner — Design

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
  sandbox.rs    request construction (Authoring phase) + executor
  session.rs    AgentSession (turn model) + AgentRunner (loop + events)
  run.rs        worker: drives AgentRunner on a thread, channel of events
  tui/
    mod.rs      ratatui run loop, event handling, wiring to run.rs
    app.rs      App state: transcript, selectors, input, status
    render.rs   pure transcript model + ratatui drawing
tests/
  config.rs, tools.rs, sandbox.rs, session.rs, run.rs, cli.rs, tui.rs, integration.rs
```

Separation of concerns: `config`/`toolchain`/`tools`/`sandbox` build trusted
structures from untrusted inputs; `session` owns the model-agnostic conversation
and loop policy;
`run` owns process/thread plumbing; `tui` owns only presentation. Nothing in
`session` performs blocking subprocess I/O directly — it hands argv to an
`Executor` trait, which the worker implements over the sandbox. Tests inject a
fake transport and a recording executor.

## Internal structure

`config`/`toolchain`/`tools`/`sandbox` build trusted structures from untrusted
inputs; `session` owns the model-agnostic conversation and loop policy; `run`
owns process and thread plumbing; and `tui` owns only presentation. Nothing in
`session` performs blocking subprocess I/O directly — it hands argv to an
`Executor` trait that the worker implements over the sandbox, which keeps the
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
   (redacted, bounded) and the loop repeats.
5. The loop stops when the turn ends with `FinishReason::Stop` or proposes no
   tool intents; the assistant text of the final turn is delivered as the
   session answer.

Cancellation: a shared `CancellationToken` (from `agent_runtime`) is checked
between turns and is threaded through the sandbox executor, which terminates the
process group on interrupt. `agent_runtime::install_handler` routes Ctrl+C/SIGTERM
to a cooperative flag so the whole process stays safe.

Why a worker thread: the model transport and sandbox are blocking. The UI runs
on the main thread and would otherwise freeze for the whole turn. `run::spawn`
starts the loop on a detached thread and pushes `Event`s over a bounded channel;
the UI reads events each frame and renders them, showing a status spinner while
a turn is in flight.

## Algorithms and decisions

Long-running sessions grow one turn per step, so `ContextManager` keeps the
model's live context bounded. It estimates request size with the ~4-chars-per-token
heuristic (`estimate_messages`), which accounts for each message plus the
always-present tool-definition framing, and starts compacting at 75% of the
window.

`compact` first keeps the configured `keep_full_turns` most recent turns in full.
If that still exceeds the hard `limit_tokens`, it keeps rolling older turns into
the rolling summary while reducing how many recent turns stay full, until the
estimated context is bounded. The single most recent turn is always kept in full
as the floor: that is the only case where keeping it cannot bring the context
under the limit, and it preserves the model's immediate context and any pending
tool continuity. The rolling summary is trimmed to `MAX_SUMMARY_CHARS` (keeping
the newest content), so it never grows unbounded.

Compaction only ever removes material from the _model_ context; the full
transcript, including every reasoning trace, is preserved separately in the
durable session log (`crate::session_log`). A compacted turn is never lost — it
is simply moved from the live context into the record. This keeps the model's
context bounded without weakening the audit trail Kvist relies on.

The loop calls `maybe_compact` after each turn; when a compaction happens it
emits `Event::Note` describing how many turns were rolled up. The UI surfaces
context utilization and a compaction progress bar via `Event::Progress`.

### Tool rendering

The registry exposes four tools, in stable order: `shell`, `read_file`,
`write_file`, `list_dir`. Each is rendered to an argv whose `[0]` is an absolute
canonical path, so the sandbox accepts it and no shell globbing or PATH lookup
happens on our argv.

- `shell { command }` → `["<bash>", "-c", "<command>", "agent-runner"]`. The
  command string is the agent's own script. Bash resolves inner tool names via
  the sandbox `PATH` we set (`/usr/bin:/bin:/usr/sbin:/sbin`). The `shell`
  command string is checked against the denylist before rendering.
- `read_file { path }` → `["<bash>", "-c", "exec cat -- \"$1\"", "agent-runner",
"<path>"]`. The path arrives as a bash argument, so it is never re-parsed as
  shell.
- `write_file { path, content }` → a staged write. The content is written to a
  host path inside the working directory first (see `StagedWrite`), then the
  rendered argv `["<bash>", "-c", "exec mv -f -- \"$1\" \"$2\", "agent-runner",
"<sandbox-staging>", "<path>"]` moves it into place inside the sandbox. Staging
  lets arbitrarily large files be written without exceeding the sandbox argv byte
  limit; the staged file already lives under the read-write write root, so the
  move stays inside the writable scope. Both staging paths share the same
  `.agent-writes/<call_id>` tail: the host staging path joins under the working
  directory (`PathBuf::join` keeps it relative), and the sandbox staging path
  joins under the configured `write_root`, so the in-sandbox `mv` finds the file
  regardless of the configured root (`write_root` is not assumed to be `/`).
- `list_dir { path }` → `["<bash>", "-c", "exec ls -la -- \"$1\"",
"agent-runner", "<path>"]`.

`bash` is resolved once to its canonical path at request construction. The
registry renders the sandbox-relative path the agent passes (typically under the
write root `/workspace`); the agent works in the sandbox view, so tool output
paths are consistent.

Write scope is enforced in two places. The registry rejects `write_file` when the
target is not inside the configured `write_root` on a slash boundary (so a sibling
such as `/workspace-evil` is rejected when the write root is `/workspace`, not
merely when it fails a bare `starts_with`), and the sandbox grants read-write
authority only at the write root, so any other write fails inside the sandbox
regardless.

### Tool-chain advertisement and gating

The `shell` tool advertises the tool-chains available inside the authoring
sandbox through the `toolkit()` strings of the enabled `ToolProfile`s. Advertisement
must be honest: the sandbox mounts only the read-only `/usr` layout and clears the
environment, so a profile is advertised only when its interpreter genuinely reaches
the sandbox, never against the host `PATH`.

The gate lives in `ToolRegistry::resolve` (wired in `tui::run` and the CLI). It
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
root) and one read-only `context` grant per declared read root. No scratch
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

The UI is built with `ratatui` + `crossterm`, following the same line-editor and
theme-detection spirit as the Kvist shell but as a full-screen transcript rather
than a line REPL. `App` holds the transcript (`Vec<TranscriptRow>`), the selected
model, the selected effort, the running flag, the input buffer, a scroll
offset, and a help-overlay flag. Rendering is a pure function of `App` state, so
the transcript model is unit-tested without a terminal.

Transcript text is wrapped to the box's inner width (the full terminal width
minus the two vertical borders) so no line extends past the visible area, and
tool-call events carry a short description of what applied where (file,
directory, or command). Events from `run::spawn` are folded into `App` each frame. Key handling: Enter
submits the input as a prompt, Ctrl+C cancels the current turn (and quits when
idle), Esc toggles help, and Ctrl+Up/Down, PageUp/PageDown scroll the transcript.
The status bar shows `model | effort | status` and a cancel hint; the prompt line
shows the input with autocomplete-free editing to keep the dependency surface
small.

A live stats bar reports progress without cluttering the transcript: working
speed (`⚡ N tok/s`), context utilization (`▁▃▅▇ +P% cur/limit`), a compaction
progress bar (`compaction ▁▃▅▇ P%`), and cumulative `total_tokens` with elapsed
time (`m:ss` or `h:mm:ss`). Each stat is derived from `Event::Progress`, and the
stats line is unit-tested without a terminal so the presentation model stays
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
- A model turn with zero tool intents ends the loop and returns the answer.
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
  profiles whose interpreter reaches the read-only `/usr` layout, and `rustup`
  stubs are not treated as usable tool-chains.
- `detect_languages` reports the project's detected language without gating
  advertisement.
- Deterministic ordering of tool definitions, grants, and identities.
- Compaction keeps recent turns in full and fits under the limit; when the
  window is tight it reduces the number of full turns (flooring at one) while
  always keeping the most recent turn.
- The rolling summary contains each rolled turn line exactly once, even when a
  tight window forces multiple compaction passes.
- Progress accounting is emitted after every turn with cumulative input/output
  tokens, so the live stats and the compaction ETA update during the session.
- The stats line is empty until progress is reported, then reports speed,
  context, compaction, cumulative tokens, and elapsed time.

## Security posture

- No per-action prompts: authority is established once in configuration.
- Fail closed: a missing or unverified sandbox runner/backend never triggers host
  execution.
- Untrusted inputs (config, tool arguments, subprocess output) are validated for
  schema and bounds; argv entries are NUL-free and bounded; the request size is
  bounded before parsing.
- Environment values passed to the sandbox are a small allowlist; dangerous
  names (`LD_*`, `GIT_*`, `CARGO_*`, proxies) are not forwarded.
- Secrets are never logged; only redacted, bounded summaries reach tracing.
- The denylist and write-root enforcement are tested so a regression cannot
  silently widen authority.
