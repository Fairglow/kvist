# Markdown workspace: scoped implementation observation

## Evidence boundary

This is a fresh source/test-only observation, not an intent comparison, compliance
certification, or vulnerability audit. Behavior below is derived from code, not
from requirements or inferred product goals.

Inspected source was limited to selected relevant bodies and unit tests in:

- `maerg/src/presentation.rs`, `status_markdown.rs`, `cli.rs`, `error.rs`,
  `file_io.rs`, and `bark.rs`;
- `maerg/src/bark/{completion,prefill,tree,theme}.rs`;
- `maerg/src/bark/tui.rs` and all seven Rust files below `bark/tui/`;
- `maerg/src/agent/redaction.rs`.

Inspected integration-test source comprised `maerg/tests/markdown_output.rs`,
`tui_workspace.rs`, and relevant sections of `prompt.rs`, `status.rs`,
`shared_registry.rs`, `vcs.rs`, `tree.rs`, `task_commands.rs`,
`project_state.rs`, and `discovery_limits.rs`. Necessary prompt-acquisition API
references were followed through `maerg/src/prompt_input.rs` to
`sav/src/prompt.rs`. Narrow error-path references were also followed into
`maerg/src/repair.rs` (`RepairReport` completeness/Display) and
`maerg/src/task_commands.rs` (accept/finalize commit-failure branches).
Blad internals were not inspected.

No intent artifacts, README/documentation/guide inputs, adjacent reviews, prior
records, Git diffs/history, or session history were read. No agents were invoked.
Implementation and tests were not edited.

## Observed presentation and command routing

`cli::execute` leaves global JSON output, completion scripts, internal authoring
application output, and explicit status text/JSON formats outside its Markdown
wrapper. Empty output and output already starting with `# ` also bypass that
wrapper. Other human command results receive a command-specific heading; tree
text is fenced, while other legacy reports pass through `legacy_report`.
This is a transformation of returned `CommandOutput`, not interception of every
write made by a command or delegated process.

`presentation::sanitize` retains newline/tab, removes carriage return, and
represents other Rust control characters as printable `\u{....}` text.
`literal` additionally escapes selected Markdown punctuation and replaces
newlines with `<br>`. `fence` chooses a backtick delimiter longer than every
backtick run in the body. `document` escapes its title and projects its body.

`report` parses top-level JSON objects/arrays. Objects become headings and
labelled facts; recognized prose fields such as `report`, `message`, `output`,
and `log_content` are recursively projected as report text. Uniform arrays of
nonempty objects with matching keys and single-line scalar values become
tables; other arrays receive numbered item headings. Empty arrays become
`_None._`, null becomes an em dash, and sufficiently deep structural rendering
falls back to fenced JSON. Non-JSON prose remains authored Markdown rather than
being escaped as a literal. Unknown object fields remain represented.

`legacy_report` recognizes box borders, bullets, label/value lines and fenced
blocks. Within detected fences it preserves line contents apart from control
sanitization; outside fences it applies heuristic formatting. It is not a
general lossless plain-text conversion.

Default status and doctor output use `status_markdown::render`. The report
includes project state/artifact tables, diagnostics, VCS facts, optional
toolchain facts, component artifact states, changed-document digests and Git
diff hints. Each visible component's queue is separately read through a
nonfollowing regular descriptor with a 1-MiB inclusive bound, then parsed.
Successful reads produce completed/total counts, active/blocked/decision
counts, next-ready guidance, and fenced recorded reasons. Read/parse failures
produce a “Task progress unavailable” fact instead of suppressing the component.
The document/implementation flags filter component artifact rows; unfinished
omits current components. These flags do not remove the project document table
or all queue facts.

Bare invocation with terminal stdin and stdout enters `bark::run_shell`, including
from an uninitialized current directory when no project is found. Redirected
invocation is finite: project Markdown overview/help pointer, JSON status
envelope, or help/not-in-project output. Explicit `shell` resolves a project.
Parser help/errors are fenced Markdown with the parser's stdout/stderr choice;
version output retains clap's printing. `JsonCommandFailure` prints its payload
verbatim regardless of whether global JSON was requested; ordinary incomplete
repair is a concrete exception to the human Markdown wrapper (defect 4).
Other domain errors become an Error document.

## Observed workspace, editor, and dispatch

The full-screen host is selected only when both streams are terminals,
`NO_COLOR` is absent, `CLICOLOR` is not `0`, `TERM` exists and is not `dumb`,
and terminal size is at least 40 by 10. Otherwise `run_shell` uses its separate
Reedline host, which refuses nonterminal stdin. Color forcing does not override
the full-screen host's eligibility checks.

The workspace canonicalizes its project path, reopens saved transcript/history,
loads state/theme/status, and appends a startup heading. Its guard enters raw
mode, alternate screen, mouse capture and bracketed paste; failed entry attempts
restoration, and normal/error unwinding runs restoration through the guard.
The event loop polls every 50 ms; dirty Markdown re-rendering is throttled to
100-ms intervals and reacts to width/resize changes.

The view has focus/state/progress and branch/sandbox/model/lock lines, a
scrollable transcript, scrollbar, command footer, completion popup, and help,
settings and confirmation overlays. Navigation spans persisted segments;
Ctrl+Home/End reaches oldest/latest, mouse wheel scrolls, and Alt+Left/Right
changes horizontal position. Settings expose component, developer model,
developer reasoning effort, theme, a selector for the latest 100 segments, and
follow-latest. The selector's bound does not bound PageUp access.

Input insertion is bounded to 8192 UTF-8 bytes. Paste maps newline/carriage
return/tab to spaces and removes other controls; it does not submit multiple
commands. Movement/deletion uses grapheme segmentation, word movement uses
whitespace, and undo retains up to 100 snapshots. Clearing with `set` clears
undo. Completion replaces a checked byte span, quotes whitespace/quote/backslash
values, and preserves text outside the span.

Completion uses the clap-derived command tree plus injected builtins and
re-exposed task argument slots. Dynamic candidates are snapshot-based; explicit
component arguments scope task/attempt values, and current focus influences
ordering. Prefill inserts the selected component into the editable argument
slot, does not overwrite explicit/partial values, and remembers deliberate
deletion until submission reset. Dash-leading or command-named focus values
receive a `./` prefix; control-containing focus values are not inserted.

Submission records a redacted command/history entry before parsing. Builtins
cover focus, tasks, overview, run, help, history, journal, recent runs, locks and
themes. Other commands are clap-parsed and normalized, then run as a child of
the current executable with explicit arguments and project working directory.
Optional component arguments on selected operations can receive focus at
dispatch; required run arguments rely on the visible buffer. Task execution
gets `--stream`. Nested shell/internal authoring application are not dispatched
as ordinary workspace commands.

Confirmation overlays cover forced unlock, removal of all models,
accept-with-commit, and component commit, rather than every mutating command.
Prompt editing restores the terminal, invokes the external editor integration,
re-enters/clears the terminal, retains the authored prompt file, displays the
prompt, and asks for confirmation. Direct embedded host-execution prompts are
rejected. Model/effort menu actions invoke developer-role configuration commands;
they do not call policy approval.

While a child runs, Enter supplies an answer rather than a new command.
Answers are masked, not added to command history, and replaced in the
transcript by an “answer not retained” notice. A child can nevertheless echo
an answer in its own output; the host does not categorically suppress that.
Ctrl+C requests cancellation or clears idle input. Empty-input Ctrl+D requests
workspace exit and cancels a running child; cancellation does not roll back
task effects.

## Capture, rendering, persistence, and bounds

The child has piped stdin/stdout/stderr, its own Unix process group, disabled
color/pager environment, and separate output-reader threads. Readers use
4096-byte buffers, reassemble incomplete UTF-8, report invalid UTF-8, and feed
a bounded 32-event channel. An input-writer thread has a two-answer channel;
additional unconsumed answers fail without blocking the UI. Drain takes at most
64 events per call.

Cancellation sends process-group SIGINT, followed by SIGKILL after five
seconds. An exited child whose pipes have not reached both EOFs is also subject
to group SIGKILL after two seconds. Completion waits for child status and both
EOFs. Dropping the owner kills the group, closes channels, waits, and joins its
threads.

Each stdout/stderr `Stream` buffers report JSON across reads, projects complete
records, and has a 16-MiB pending-record limit. Incomplete JSON is held across
nonfinal flushes. Ordinary complete lines take a different projection path;
see defect 1 below. Stream capture failures request cancellation and mark the
command failed. Oversized indivisible transcript output starts a new document
for the error; ordinary persistence errors propagate out of the workspace.

Configured role redaction values and allowed sandbox-environment values are
collected on refresh. `StreamRedactor` ignores empty patterns, uses longest
matching prefixes at UTF-8 boundaries, and holds possible partial matches
across deltas. JSON string values can be decoded before redaction; object keys
and nonstring JSON values are not handled by that string-value traversal.
Workspace appends also redact and sanitize. This is not a general
secret-discovery mechanism; defect 1 gives a concrete boundary failure.

Rendering calls Blad with an explicit positive width, derived light/dark mode
and selected host palette overrides. Its emitted bytes go through a VTE parser
into Ratatui text: printable characters, line breaks, expanded tabs and
supported SGR styles are retained, rather than forwarding arbitrary terminal
actions. The code creates a renderer and renders the current source segment
again when dirty; no incremental-rendering or performance claim was measured.

Transcript storage is `.kvist/transcript` within the project or resolved
component scope. Twenty-digit numeric Markdown filenames form segments, and
`.writer` is locked against another workspace. Newly created private directories
use mode 0700. Opened existing state files must be regular, single-link,
nonfollowing files without group/other permission bits. Existing directory
permissions are not tightened by `private_directory`.

Segments normally target 1 MiB but may grow to 16 MiB for indivisible Markdown
blocks. Rotation uses pulldown-cmark top-level block boundaries with completed
blank-line separation. It writes/synchronizes new pages before atomically
replacing the old page; interrupted rotation can duplicate diagnostics.
There is no total-history retention limit. Only the displayed segment is loaded
as workspace source, although opening scans directory entries for the largest
sequence. Navigation assumes intervening numeric segments exist; a missing
selected segment is an I/O error, not repaired automatically.

Command history is JSON, scoped with the transcript, capped at 500 entries and
1 MiB; reopening additionally checks each line against 8192 bytes. Scope changes
open/read the destination before replacing active transcript/history state.
Theme changes validate/load before saving the project preference and updating
the active theme. Atomic replacement uses a synchronized same-directory file
and synchronizes the parent directory.

Custom theme loading uses an existing loader with a metadata-size precheck,
not the bounded regular-descriptor helper used for queues. Neither theme spec
loading nor preference reading provides the same regular-file/read-bound
guarantees as transcript reads; see defect 5.

## Test-source assertions, not executed results

The inspected tests assert the following; this observation did not run them:

- `presentation` unit tests: nested JSON report readability, metadata retention,
  collision-free fences/cells, inert controls, and legacy fenced indentation.
- TUI unit tests: grapheme editing, paste/prefill, quoted replacement preserving
  suffixes, cleared-answer undo, split/pretty JSON and selected split-secret
  cases, owned child output/exit/cancellation, Blad-rendered tables/code/math/
  diagrams, inert controls, custom palette propagation, layout, history seek,
  scope restoration, theme failure preservation, competing-writer/link refusal,
  segment reopening, block-preserving rotation, and oversized-block refusal
  without altering previously saved text.
- `markdown_output.rs`: human init/status/doctor/tree/help/task-next output starts
  with a heading and has no ESC in selected routes; status has artifact/task
  facts; explicit text status, JSON status/global JSON, and Bash completion
  remain native.
- `tui_workspace.rs`: one Unix PTY scenario enters bare Kvist, focuses the root
  component, completes `task next`, consumes stub agent output and an empty
  answer, changes theme/model/effort through menus, switches histories, browses,
  exits, and checks alternate-screen enter/leave bytes. Stored report checks are
  substring assertions, not a complete rendered-screen or source-equality test.
- `status.rs`: exact stable text/JSON facts, overview/default equivalence,
  changed-document digests/diff guidance, blocked/decision reasons and next
  commands, invalid/uninitialized diagnostics, and filters. `project_state.rs`
  and `discovery_limits.rs` assert Markdown doctor diagnostics and selected
  nonmutation/error propagation.
- `tree.rs` asserts exact heading/fenced ASCII tree output and native JSON.
  `task_commands.rs` asserts exact Markdown task-next/transition output and
  retained task-log text. `vcs.rs` asserts Markdown VCS facts/diagnostics and
  unchanged Git status for its fixture.
- `prompt.rs` asserts headless delegation arguments, explicit JSON forwarding,
  stdin/editor prompt acquisition, explicit model/effort, child failure,
  absent executable, conflicting sources, oversized input and linked-file
  refusal. `shared_registry.rs` asserts catalogue/scope facts and the projected
  legacy heading form.
- Inspected completion/prefill unit tests assert scoped task/attempt values,
  next-ready ordering, deletion persistence, quoting, and dash-leading paths.

These are assertions present in source, not evidence that the full suite passes.
In particular, direct renderer and substring-based PTY tests do not establish
lossless ordinary-output streaming.

## Concrete source-corroborated defects

The defects below were not dynamically reproduced here.

1. **Ordinary streamed Markdown gains extra newlines and can defeat multiline
   redaction.** In `maerg/src/bark/tui/process.rs:247-284`, `Stream::push` drains
   each ordinary line including its newline, passes it through `report_redacted`
   (which retains that newline for plain prose), then appends another newline.
   Thus an ordinary child table with adjacent header/delimiter rows acquires a
   blank line between them and no longer forms the same Markdown table. Fenced
   code also gains blank lines. This affects the plain Markdown emitted by CLI
   children, not just malformed input. With the configured redaction value
   `line\nsecret` and one ordinary chunk `line\nsecret\n`, per-line redaction
   sees neither complete match; the outer streaming redactor instead sees
   `line\n\nsecret\n\n`. The later workspace append redactor sees that same
   changed text and cannot match the original value either. The inspected
   multiline-secret unit case uses JSON, so it does not cover this ordinary-line
   path.

2. **Embedded stdin-sourced prompt input has no successful EOF path.**
   `workspace/actions.rs:146-233` allows bare `prompt` and `prompt --file -`
   to start as children. `maerg/src/prompt_input.rs` delegates acquisition to
   `sav/src/prompt.rs:16-52,122-145`, where nonterminal stdin is read to EOF.
   `Running` keeps its piped stdin writer alive while waiting for answers;
   answers only write a line (`tui/process.rs:90-142,176-189`).
   `workspace.rs:507-513` makes empty-input Ctrl+D cancel/exit rather than close
   child stdin. Consequently ordinary typed prompt input cannot finish that
   acquisition; it waits until cancellation or excessive input. Explicit
   prompt text, an ordinary prompt file, or the editor path avoids this case.
   The standalone stdin test closes its pipe and does not exercise this
   embedded lifecycle.

3. **Legacy prose can concatenate with following metadata.**
   `presentation.rs:265-291` appends an ordinary nonboxed prose line without a
   trailing newline. When the following line is a recognized `key: value`,
   its branch writes `**key:** value` without a preceding separator.
   For input `offline vendoring ready for /opt/proj/kvist\nregistry dependencies: 365`,
   the source therefore constructs
   `offline vendoring ready for /opt/proj/kvist**registry dependencies:** 365`.
   The command/document wrappers add no separator between those fragments.
   This also applies when prose is followed by a recognized bullet. The
   inspected legacy-projection unit test checks fence preservation, not this
   transition. No actual vendor invocation was executed by this observer.

4. **An ordinary incomplete repair bypasses Markdown error presentation.**
   `cli.rs:1606-1611` constructs `JsonCommandFailure` with
   `report.to_string()` when the non-JSON repair branch receives an incomplete
   report. `cli::execute` propagates that error at its `execute_raw(...)?`
   before the success-output wrapper runs. `error.rs:713-715` then writes its
   payload verbatim to stderr, with no output-mode check.
   `RepairReport::complete` is `remaining.is_empty()` and its Display
   (`repair.rs:118-202`) starts `repair: ...`, followed by plain recovery facts.
   Thus an ordinary `kvist repair` or `kvist repair --dry-run` that returns a
   report with remaining artifacts uses native plain report text on failure,
   not a Markdown document. The variant's name does not restrict its payload
   to JSON. By contrast, the inspected accept/finalize commit-failure branches
   only construct this variant when `is_json` is true; their non-JSON branches
   return `VcsCommitFailed`. Explicit JSON decision/repair/validation failures
   are separate, deliberate native-output paths in the inspected dispatch.

5. **Existing custom-theme I/O can block or exceed its nominal read bound.**
   `bark/theme.rs:491-510` checks `symlink_metadata().len()` against
   `MAX_THEME_SPEC_BYTES` (1536), then calls unbounded `read_to_string`.
   It does not require a regular file, open nonblocking/nonfollowing, or enforce
   a byte limit on the opened descriptor. In particular, a FIFO can pass the
   size precheck and block the synchronous workspace settings action in
   `workspace/actions.rs:332-350`; canonicalizing its path does not turn it
   into a regular file. A regular file growing/replaced after the precheck can
   exceed that bound. The direct preference/environment loading path can also
   follow a short symlink whose target exceeds the checked link length
   (settings canonicalization does not apply to every loader caller).
   `read_preference_file` at `theme.rs:444-449` independently reads its entire
   input before selecting the first line, with no type or size check.
   These are bounds of the existing theme loader exposed through workspace
   settings, not evidence that theme loading itself was newly implemented.
   No FIFO, race, oversized-target or terminal-hang experiment was run.

## Commands personally run and execution limits

The sole shell command run during this observation was:

```sh
wc -l maerg/src/{presentation,status_markdown,cli,error,file_io,bark}.rs maerg/src/bark/{completion,prefill,tree,theme,tui/editor,tui/process,tui/render,tui/storage,tui/workspace,tui/workspace/actions,tui/workspace/view}.rs maerg/src/agent/redaction.rs maerg/tests/{markdown_output,tui_workspace,prompt,status,shared_registry,vcs,tree,task_commands,project_state,discovery_limits}.rs
```

It exited 0 and reported 22,303 total lines across those files. That count is
file size inventory, not a claim that every line was inspected.

Other operations were scoped `glob`, `rg`, and `view` tool reads, followed by
`apply_patch` creating this record and a readback of this new record. One `rg`
query was rejected for a literal-newline regex and was rerun with a corrected
pattern. No build, formatter, linter, Cargo test, PTY session, live model,
terminal probe, or Git command was personally executed. No parent-run or prior
result is imported as executed evidence.

This record therefore establishes source observations and identified test
assertions only. It does not establish runtime portability beyond the
Unix-specific code inspected, crash/power-loss recovery under injected faults,
all terminal sizes/capabilities, renderer behavior for all syntax, performance
under sustained output, or comprehensive coverage of every CLI path.

## Correction addendum — 2026-11-04

This narrow source-only follow-up supersedes the current applicability of the
five historical findings above; their original text is preserved. Inspection
was restricted to the corrective bodies and directly related tests in
`presentation.rs`, `cli.rs`, `file_io.rs`, `bark/theme.rs`,
`bark/tui/{process,workspace,storage}.rs`, the workspace keyboard-help text,
`tests/markdown_output.rs`, and the added stdin/EOF portion of
`tests/tui_workspace.rs`. No broader re-review or intent comparison was made.

### Observed corrections and test-source assertions

1. **Ordinary stream line preservation and multiline redaction:** the
   ordinary-line branch of `Stream::push` no longer adds a newline after
   `report_redacted`. The retained newline therefore reaches the outer
   streaming redactor without the historical insertion. The new
   `ordinary_markdown_is_preserved_exactly_across_every_read_boundary` test
   checks exact equality for heading/table/fenced-code source using chunk
   sizes 1, 7, and the whole input, with nonfinal flushes after chunks.
   Despite its name, these are selected chunkings, not exhaustive enumeration
   of every possible segmentation. The new ordinary multiline-secret test
   checks both whole-input and split-input cases and asserts exactly
   `[REDACTED]\n`. These bodies address the concrete examples in finding 1.

2. **Child EOF:** `Running::close_input` drops the answer sender. Once queued
   answers have been handled, the writer's receiver loop ends and drops its
   child-stdin handle. Empty-input Ctrl+D invokes this operation while a
   command runs, rather than cancelling/quitting; idle Ctrl+D still exits.
   Ctrl+Q separately sets quit and requests cancellation of an active child.
   Keyboard help describes both operations. The PTY test now submits
   `prompt --file -`, writes `stdin prompt body` plus Enter and Ctrl+D,
   waits for a fifth successful-command transcript marker, and asserts that
   the body is absent from command history. This supplies a source-observed
   successful EOF path missing in finding 2; Ctrl+Q was inspected in code,
   not personally exercised.

3. **Legacy line separation:** before processing each nonborder line,
   `legacy_report` inserts a hard line break when accumulated output lacks
   a trailing newline. Its ordinary-prose branch no longer independently
   prefixes later lines. Consequently the historical prose-to-metadata and
   prose-to-bullet transitions receive separators without an extra blank
   line. The new regression asserts `Ready  \n**registry dependencies:** 365`
   and `Next  \n- choice`. The concrete concatenation in finding 3 is no
   longer constructed by these bodies.

4. **Incomplete human repair:** the non-JSON incomplete-repair branch now
   places `document("Artifact Recovery", legacy_report(...))` in the error
   payload. `JsonCommandFailure` still represents a verbatim-print boundary;
   this caller now supplies Markdown rather than plain recovery text.
   The explicit-JSON incomplete branch still supplies its serialized response
   directly. The new integration test asserts unsuccessful human repair with
   stderr beginning `# Artifact Recovery\n\n`, and unsuccessful explicit-JSON
   repair with parseable JSON stderr. Finding 4's ordinary plain-text exception
   is addressed without changing the inspected native JSON branch.

5. **Theme read bounds and file types:** theme loading canonicalizes the
   supplied path, then uses `read_text_bounded` with the 1536-byte spec limit.
   That helper opens nonfollowing/nonblocking, checks that the opened file is
   regular, reads at most limit plus one, and returns `FileTooLarge` on
   overflow; the loader maps overflow to `ThemeSpecTooLarge`. A valid explicit
   symlink target can still be selected through canonicalization, but its
   actual opened contents are bounded. Preferences now use the same helper
   with an 8192-byte limit; absent files are ignored, other read failures warn
   and are ignored. Optional user-theme loading also warns on errors other
   than absence. The added test asserts FIFO refusal for spec/preference
   reads, oversized linked-target refusal, valid linked-target acceptance,
   and oversized-preference refusal. These are source-observed corrections
   to finding 5, not a claim that arbitrary ancestor-path races were audited.

### Related transcript adjustments

`Transcript::drop` now explicitly unlocks its writer file and warns if that
operation fails. The new duplicated-descriptor regression clones the lock
descriptor, drops the transcript, and asserts reopening succeeds while the
duplicate remains alive. It models the inherited-descriptor lifetime without
requiring a concurrent process race.

For appends exceeding the 1-MiB segment target but remaining a single valid
segment, `Transcript::append` now appends only the incoming bytes and calls
`sync_data`; it does not enter the atomic-replacement/rotation branch.
It still reads and parses the assembled source to decide whether rotation is
needed. This is a reduced-write observation, not a measured runtime-speed
claim or a new fault-injection result.

### Follow-up execution attribution and conclusion

Only scoped `rg`/`view` reads, this `apply_patch` addendum, and a readback of
the addendum were performed in this follow-up. No shell command, test, build,
lint, formatter, live PTY, or Git operation was personally run. Reported parent
test/lint results are not imported as executed evidence.

No remaining concrete discrepancy was found in the five corrective paths
examined. This conclusion is limited to the inspected source and test
assertions; it is not an independently executed aggregate validation or a
certification of the workspace.

## Host-diagnostic routing addendum — 2026-11-04

This final source-only supplement inspected only `maerg/src/logging.rs` and
the relevant workspace entry/drain, theme-warning, and PTY assertion changes.
It does not reopen the preceding implementation scope or incorporate intent.

### Observed behavior

`logging::filter` is shared by the global subscriber and workspace subscriber.
It checks `KVIST_LOG`, then `RUST_LOG` when the former cannot be read, uses
lossy parsing with a warn default directive for a nonempty supplied filter,
and otherwise uses the caller's default. Workspace entry uses the production
warn default. Global logging retains its stderr writer; the new workspace
subscriber uses compact formatting with ANSI disabled and a channel writer.

`WorkspaceDiagnostics::enter` installs the scoped default subscriber before
`Workspace::new`, so initialization and theme-resolution events on that thread
are captured before terminal raw-mode entry. At the top of each event-loop
iteration, the receiver is drained; nonempty text is appended under
`### Host diagnostics` in a collision-resistant `text` fence through the
existing workspace append path. The scoped guard remains alive throughout
the workspace closure. The inspected theme-resolution fallback, failed
preference read, and optional user-theme failure now emit `tracing::warn!`
instead of directly printing warnings.

The channel has capacity 32. Each writer call validates UTF-8, retains at
most 16,384 bytes of its text at a character boundary, and adds an explicit
truncation marker if shortened. It uses `try_send`, counts unsuccessful sends,
and reports the original write length rather than blocking the producer.
Drain consumes at most 32 queued fragments, concatenates them, atomically
resets the dropped counter, and appends a notice when fragments were not
retained. Invalid UTF-8 writes return an error rather than entering the channel.

### Test-source assertions, not executed results

The new logging unit test constructs its subscriber with an explicit `error`
filter, independently of environment filter settings. It asserts capture of a
fixture event, an explicit loss notice after 100 burst events, and a truncation
marker plus output shorter than 17,000 bytes for a 32,768-character event.
It does not explicitly test multibyte truncation boundaries or routing from
another thread.

The inspected PTY scenario sets `KVIST_LOG=debug` and waits for a
`### Host diagnostics` section in the project transcript before continuing.
This asserts transcript capture in that scenario, not the absence of every
possible out-of-band terminal write.

### Concrete bounds and discrepancy

- The scoped subscriber is thread-local: this does not automatically redirect
  diagnostics from separately spawned host threads. Child-process stderr is
  handled by the existing process-capture path, not this subscriber.
- Bounds apply to retained writer fragments and channel occupancy, not whole
  event-formatting allocations, total diagnostic volume, or durable transcript
  retention. Dropping/truncating diagnostic content is explicit and intentional.
- No final drain is present on initialization failure or closure exit, so
  diagnostics queued immediately before those paths need not reach a transcript.
- Minor source-documentation discrepancy: the `logging.rs` module introduction
  still says diagnostic events are directed “exclusively to standard error.”
  That describes the global route, but no longer the scoped workspace route.

No concrete functional defect was identified in these inspected changes.
Only scoped `view`/`rg` reads, this `apply_patch`, and addendum readback were
performed. No additional shell commands or tests were personally run; parent
validation remains separately attributed.
