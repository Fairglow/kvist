<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: TUI Layout and Transcript Persistence (`maerg`)

**Component:** `maerg`
**Task:** `tui-layout-compliance-review`
**Scope:** The full-screen interactive TUI (entry and degradation, host
ownership and child isolation, Markdown projection of live output and JSON
reports, the `skott`-shaped layout of status heading, scrollable transcript
window, and command footer, visible component prefill, completion and
keyboard-shortcut help) and the durable per-component transcript persistence
(canonical Markdown segments, block-aligned rotation, bounded reads, segment
scrollback and seeking, per-scope isolation and restoration, private state
guards, recording-failure reporting, and the local-uninspected-state status),
compared against `REQUIREMENTS.md` (`REQ-INTERACTIVE-TUI`,
`REQ-TRANSCRIPT-PERSISTENCE`), `CONTRACT.md` (`Provided interfaces`, including
the Markdown-first output and interactive TUI section and the `blad`
requirement), and `ROOT_CONTRACT.md` (`Change and compliance rules`).

**Reviewer stance:** Independent, source-blind compliance reviewer. This
record is the disposition for the `tui-layout-compliance-review` gate. The
implementer and the security auditor did not author it, and no clause below is
certified from a prior compliance conclusion or from the implementer's own
evidence. Per the root contract's change and compliance rules, discrepancies
are recorded here for explicit human arbitration; no artifact is silently
altered.

## Review boundary and independence

**Read (permitted intent context):** `maerg/REQUIREMENTS.md` (the
`REQ-INTERACTIVE-TUI` and `REQ-TRANSCRIPT-PERSISTENCE` sections and adjacent
degradation clauses), `maerg/CONTRACT.md` (`Provided interfaces`, the command
surface, the Markdown-first output and interactive TUI section, and the
Required-interfaces `blad` clause), `ROOT_CONTRACT.md`, and the canonical
task-definition projection of this task in `maerg/TODOS.yaml`.

**Read (implementation record and bounded test evidence):** `maerg/IMPL.md` —
the freshly appended "Clean-slate TUI and transcript observations -
2026-11-20" section (produced in a separate clean-slate source-only context
from the changed sources, per the task's directed method) — plus the in-crate
`#[cfg(test)]` suites of `src/bark/tui/workspace.rs`, `workspace/view.rs`,
`workspace/menu_tests.rs`, `workspace/actions.rs`, `workspace/menus.rs`,
`workspace/bindings.rs`, `storage.rs`, `process.rs`, `render.rs`, `editor.rs`,
`prefill.rs`, `completion.rs`, `status.rs`, and `src/bark.rs`, and the
integration evidence `tests/tui_workspace.rs`, `tests/shell_smoke.rs`,
`tests/view_switching.rs`, and `tests/completions.rs`.

**Not read as evidence:** Git history or diffs, the security-audit report of
this chain (`tests/security_audit_tui_transcript.md`) — its conclusions are
neither relied upon nor repeated — prior compliance conclusions, peer or
parent implementation artifacts, or the implementer's execution summaries.

**Context note (disclosed, not a boundary breach):** the task definition for
this gate explicitly directs that the observed behavior be derived from the
changed source and test evidence in a clean-slate context before the
source-blind comparison. That derivation is the new `IMPL.md` section named
above; the two passes remain separately attributed. Every "observed"
statement below cites that section or the named test assertion, and no
requirement clause is certified from a prior review.

## Bounded evidence relied upon

- `IMPL.md` "Clean-slate TUI and transcript observations - 2026-11-20": entry
  and degradation gates; argument-vector child spawn with isolated I/O,
  process-group cancellation and reap; the `report_redacted`/`sanitize`
  Markdown projection; the `vte`-guarded `blad` render boundary; the responsive
  event loop; paste-to-single-line handling; the heading/transcript/footer
  layout; the prefill slot list and dispatch fallback list; the completion
  engine and `BINDINGS` help; the storage, rotation, locking, and history
  bounds; and the recording-failure reporting paths.
- Integration assertions: `tests/tui_workspace.rs` (bare `kvist` over a real
  PTY: workspace seed, JSON `report` rendered as readable Markdown with no
  `"report":` artifact and no raw escape byte, `cd .`/`cd` per-scope
  transcript and `# Focus:` restoration, tab-completion acceptance, theme/
  model/effort persistence through the real menu, prompt via stdin with
  non-retained answers, project- and component-level status tables, and a
  clean exit after the owned child is reaped), `tests/shell_smoke.rs`
  (piped-stdin rejection, line-oriented path behavior, and the
  `KVIST_NO_PAGER` height-aware pager policy), `tests/view_switching.rs`
  (embedded Skott view keys), `tests/completions.rs` (the `completions`
  script surface).
- In-crate assertions: `storage.rs` (rotation, restore, old-segment reads,
  link and competing-writer refusal, component scoping, forked-descriptor
  lock release), `process.rs` (record-boundary decoding, Markdown
  preservation across read sizes), `render.rs` (contrast, `blad` rendering,
  authored-control inertness, custom palette), `view.rs` (layout fragments,
  segment seeking and focus restoration, resize/split rewrap, theme picker
  preselect, nested back navigation), `menu_tests.rs` (escape ordering, paste
  routing, quit/cancel paths, active-command setting refusal, task-run
  auto-selection), `prefill.rs` (real-buffer insertion, `cd` exclusion,
  deletion effectiveness, quoting, abbreviation gating), `completion.rs`
  (component position resolution), `bark.rs` (focus injection, paste
  splitting), `status.rs` (context loading and labels).

This record is not execution proof. These suites are run automatically by
Kvist in the verification sandbox after this turn; no pass/fail count from
that run is asserted here.

## Method

Each obligation of `REQ-INTERACTIVE-TUI` and `REQ-TRANSCRIPT-PERSISTENCE`,
each `CONTRACT.md` clause in `Provided interfaces` bearing on these surfaces
(the command surface, the Markdown-first output and interactive TUI section,
and the `blad.library/v1` requirement), and the root `Change and compliance
rules` were compared against the implementation record and the bounded test
evidence. Each item is classified **compliant**, **mismatched**, or
**note** (an observation recorded for arbitration that is not asserted to be a
defect), with the intended text, the observed behavior or evidence, and a
severity.

## Satisfied clauses (compliant)

**T-1 — Bare `kvist` opens the workspace on capable interactive
stdin/stdout.** Requirement: "Bare `kvist` MUST open the workspace on capable
interactive stdin/stdout." Observed: `bare_invocation` starts the shell only
when neither `--json` nor non-interactive stdio applies; the PTY test drives
bare `kvist` over a real terminal to the workspace seed.

**T-2 — Redirected invocation stays finite; `--json` never enters a TUI.**
Observed: redirected invocations print the finite status overview (help
outside a project) and `--json` returns the finite machine-readable report in
every case; the TUI gate requires both stdin and stdout terminals plus a
minimum 40x10 size. `tests/shell_smoke.rs` pins the piped-stdin refusal on the
explicit `shell` entry.

**T-3 — The explicit `shell` entry remains available.** Requirement: for a
project-directory argument and for the line-oriented degradation path.
Observed: `Command::Shell { path }` resolves the project directory (omitted =
nearest project) and calls the same shell entry; `NO_COLOR`, `CLICOLOR=0`,
`TERM=dumb`, redirected stdio, and undersized terminals select the line-
oriented `run_shell` path, which retains its own degradation contract.

**T-4 — Host owns drawing and keyboard input; children use argument-vector
dispatch with isolated stdout/stderr and the existing cancellation/authority
boundaries.** Observed: the TUI owns raw mode, the alternate screen, mouse
capture, and bracketed paste through `TerminalGuard`; children are spawned as
the current executable with an argument vector, forced non-interactive
environment (`NO_COLOR`, `CLICOLOR=0`, no `CLICOLOR_FORCE`,
`KVIST_NO_PAGER`, `TERM=dumb`), piped streams, and their own process group.
Destructive commands pass through the same `destructive_gate` confirmation as
the CLI, and `prompt --allow-host-execution` is refused in the embedded path.

**T-5 — Live output, errors, tool results, and JSON `report` strings become
readable Markdown, not opaque escaped JSON; structured metadata stays
inspectable.** Observed: every record or line passes `report_redacted`,
projecting JSON reports into headings/lists/tables (with the `report`/
`markdown`/`message`/`content`/`text`/`output`/`log_content`/`replay` fields
re-rendered as Markdown) and sanitizing non-JSON lines; unknown fields are
retained as `- **key:** value` lines. The PTY test pins the readable rendering
of a stubbed agent report in the stored transcript with no `"report":`
artifact; in-crate tests pin record-boundary decoding and byte-exact
Markdown preservation.

**T-6 — Terminal controls from children do not execute in the host
terminal.** Observed: the `vte` render boundary maps only a fixed SGR set and
ignores every other escape; storage sanitizes before persistence.
`authored_controls_do_not_become_terminal_actions` pins no raw escape byte in
rendered spans for an embedded OSC `ESC]52;c;secretBEL` sequence; the PTY test
pins the same for the stored transcript.

**T-7 — Execution stays responsive to transcript navigation and
cancellation; quitting reaps the owned command.** Observed: the loop pumps the
child before each draw with a 50 ms input poll; PageUp/PageDown, the mouse
wheel, scrollbar-column dragging, and Ctrl+Home/End remain live while a
command runs; Ctrl+C cancels without quitting; Ctrl+Q and menu Quit share the
confirmation-plus-reap path. `quitting_active_work_requires_confirmation_and_
reaps_the_owned_child` pins the reap; the PTY test's clean exit requires it.

**T-8 — Pasted multiline input does not execute multiple commands without
individual submission.** Observed: bracketed paste inserts into the editor,
whose `insert` rewrites embedded newlines/tabs to spaces and drops other
control characters, so a multiline paste is one editable line submitted only
by an individual Enter; in menus, paste filters the searchable list and never
mutates the hidden command draft.

**T-9 — Full-screen layout in the shape of the `skott` layout: status
heading, scrollable main transcript area, command footer.** Observed:
`view.rs` draws the two-line heading, the bounded transcript window with a
right-hand scrollbar, and the three-row footer. `layout_has_status_transcript_
scrollbar_and_command_footer` pins the heading fragments, the `Transcript`
panel with rendered Markdown, the `Command` footer, and the `F1 help` hint in
a rendered 80x24 buffer.

**T-10 — The status heading shows the active component with its state and
task progress, plus VCS branch, sandbox backend, default model, and live/
stale lock counts; the whole project when none is selected.** Observed: the
heading renders `{component|(project)} | {state} | {N/M tasks}` and the stats
row renders branch, `sandbox:`, `model:`, and `N live / M stale`; the state
and progress filter to the focused component or the whole project.

**T-11 — Selected component inserted visibly into the editable buffer as
soon as the next argument expects a component, except `cd`; bare `cd` returns
to project scope; actual prefill, not an omitted-argument fallback or a
completion suggestion.** Observed: `ComponentPrefill` inserts the focus into
the real reedline buffer at a vacant component slot — every command whose
next positional is a `COMPONENT_DIR` (`task run`, `task next`,
`task transition`, `task log`, `task unlock`, `task recover`,
`task finalize`, `component validate`, `component accept`, with the `recover`
slot synthesized by the tree builder), plus the `tasks`/`run` builtins and
bare `status` — while `cd` is never prefilled including the abbreviation and
highlight hooks; bare `cd` and `cd -` clear focus. In-crate tests pin a
real-buffer insertion (`"task run "` → `"task run maerg "`), the `cd`
exclusions, clap round-trips of the inserted text, and the PTY test pins the
end-to-end `cd .` / bare `cd` behavior. See note N-2 for the separately
maintained dispatch fallback list.

**T-12 — Manual deletion remains effective and permits another component;
ordinary task arguments retain their order.** Observed: a deletion (or a
recalled/manual value) suppresses refill until the next submission, after
which the next vacant slot is filled again; insertion appends the focus into
the vacant slot without reordering any typed arguments (pinned by the
`task run  sav write-tests` and deletion-effectiveness assertions).

**T-13 — The command footer exposes the same command surface as the CLI with
full tab-completion (static from the clap surface, dynamic from the project
state).** Observed: every submitted line parses through the same `cli::Cli`
parser, and the completion candidates come from the same clap-derived command
tree with dynamic values (components, task IDs with status/title and the
next-ready marker, models, roles, branches, themes) from a `DynamicState`
snapshot reloaded after each command; Tab/Shift+Tab navigate, Enter accepts a
visible candidate before submission.

**T-14 — The TUI degrades to the plain-text line-oriented shell when the
terminal cannot host it.** Observed: the `available()` gate (terminal stdio,
`NO_COLOR`/`CLICOLOR`/`TERM` handling, minimum 40x10) routes to the
line-oriented `run_shell`, whose own degradation contract applies.

**R-1 — Persistence retains canonical Markdown, not terminal escape
sequences.** Observed: `Workspace::append` (the single persistence entry)
applies redaction and then sanitization (dropping `\r`, rewriting every other
control character to `\u{....}` text) before `Transcript::append` writes to
disk. The PTY test asserts the stored transcript contains no `ESC` byte and
no `"report":` escape artifact.

**R-2 — Memory and individual file reads are bounded independently of total
history size.** Observed: one pending child record 16 MiB; one segment 16 MiB
(target 1 MiB); one history file 1 MiB (500 entries, 8192-byte entry cap);
one menu search 256 bytes; the viewport loads exactly one segment into memory.
None grows with total history size.

**R-3 — Older segments remain reachable without loading the entire history.**
Observed: only the selected segment is read; PageUp/PageDown cross segment
boundaries, Ctrl+Home loads segment 0, the History menu lists the 100 newest
segments, and `seek_history` targets a segment fraction directly. The in-crate
`history_seek_spans_all_segments_and_focus_reopens_private_history` test pins
seeking across a multi-segment history and per-scope restoration.

**R-4 — Private history rejects symlinks/nonregular files.** Observed:
`private_directory` rejects parent traversal and symlinked components;
`open_private` opens with `O_NOFOLLOW | O_NONBLOCK` and requires a regular
file with one hard link and mode 0o600. `transcript_rejects_links_and_
competing_writers` pins that a symlinked segment is refused and its target is
untouched.

**R-5 — Recording failures are reported, and the transcript never
masquerades as canonical task evidence.** Observed: append failures surface as
a visible `## Error` document in the notice bar and transcript (with the
in-loop handler continuing rather than spinning), capture failures cancel the
running command and record `Command capture failed`, and terminal-death
failures break the loop with a restored terminal. The `journal` builtin and
the F1 help text state the transcript is local diagnostic state, not workflow
or compliance evidence. See note N-3 for the "fatal to the workspace"
interpretation boundary in the contract.

**R-6 — A bounded on-screen window over a durable, per-component,
disk-persisted record of every interaction (commands and their rendered
Markdown output).** Observed: the transcript window renders one bounded
segment; every submitted command (fenced `## Command` entry), its rendered
Markdown output, errors, focus changes, and settings outcomes are appended to
the per-scope private directory (project scope at the root, focused scope
under the component). The PTY test pins command entries, rendered output, and
`Command finished` markers in the per-scope files.

**R-7 — The record is unlimited in size and splits into successive files
when one grows too large.** Observed: segments target 1 MiB and rotate at
top-level Markdown block boundaries into successive 20-digit `.md` files; an
indivisible block over 16 MiB fails explicitly (`FileTooLarge`, cancel, no
truncation). `transcript_rotates_restores_and_reads_old_segments` pins the
rotation across a multi-segment append and the restore of segment 0.

**R-8 — The operator can scroll back through the session with a scrollbar.**
Observed: the transcript panel draws a vertical `Scrollbar` tracking the
position within the loaded segment, and mouse-drag on the scrollbar column
seeks across segments via `seek_history`; PageUp/PageDown and Ctrl+Home/End
navigate. `history_seek_spans_all_segments_...` pins the seek math across
segments.

**R-9 — Switching component clears the transcript area and restores that
component's former transcript on return.** Observed: `change_focus` reopens
the per-scope `Transcript` (and command history), clears the input and scroll
state, and appends a `# Focus:` marker; the in-crate test pins that the
component's `# Component only` content disappears on focus-out and reappears
on return, with the per-scope history preserved, and the PTY test pins the
same over a real terminal.

**C-1 — Provided interfaces: command surface for bare `kvist` and
`shell [PROJECT_DIR]`.** Observed as T-1/T-2/T-3: bare invocation on capable
interactive stdio opens the workspace, redirected invocation retains a finite
overview or help, and `shell [PROJECT_DIR]` remains the explicit entry.

**C-2 — Provided interfaces: Markdown-first output and interactive TUI
section (layout bullets, transcript storage, key semantics).** Observed as
T-4 through T-13 and R-1 through R-9. The section's key-semantics clauses
(Ctrl+C cancel, Ctrl+D EOF/exit, Ctrl+Q quit-with-confirmation, Esc ordering,
F1/F2, paste-as-input, submenus retaining cursor/filter and draft) are pinned
by `menu_tests.rs` (`escape_dismisses_completion_before_opening_the_menu_...`,
`f2_opens_the_root_from_help_details_and_quit_without_changing_the_draft`,
`nested_settings_back_restores_parent_cursor_and_command_draft`) and the
PTY test.

**C-3 — Required interfaces: `blad.library/v1` for Markdown rendering in the
interactive TUI, with theme, width, and panel background supplied by the
host.** Observed: `render::markdown` builds a `blad::MarkdownRenderer` with
the host theme (dark/light from the host canvas), the host width, and a
palette whose code/table/quote backgrounds are the host panel background;
`custom_palette_reaches_both_chrome_and_markdown` pins the host palette in
both the chrome and the rendered Markdown.

**C-4 — Provided interfaces: the stable `status --format text` and
`--format json` report forms are unchanged for non-interactive consumers.**
Observed: the TUI and transcript changes are confined to `src/bark/*`,
`src/presentation.rs` (additive), and the theme fix; the `status` rendering
paths (`status.rs`, `status_markdown.rs`) are untouched by this surface, and
the stable-format suites (`tests/status.rs`, `tests/json_outputs.rs`,
`tests/scoped_status.rs`) remain the pins for those forms.

**C-5 — Root contract: the clean-slate documenter and the separate
compliance reviewer verify implemented behavior before it is declared
compliant; discrepancies are recorded for explicit arbitration.** Observed:
this chain follows the tests → implementation → security audit → compliance
review order; the observation and comparison passes are separately attributed
above; no intent artifact was altered by either pass.

## Discrepancies and notes for human arbitration

**N-1 (note, low) — Keyboard-shortcut help is not rendered in the same
format as `skott`.**
Intended: `REQ-INTERACTIVE-TUI` — the command footer "MUST provide
keyboard-shortcut help rendered the same way `skott` does it"; `CONTRACT.md`
repeats the clause in the Command footer bullet.
Observed: the maerg TUI renders its full keyboard-shortcut help as a
two-column Markdown table (`| Key | Action |`, rendered with the `blad`
renderer) in the F1 overlay, plus a compact one-line hint strip in the idle
footer, both generated from one `BINDINGS` table. The standalone `skott` TUI
renders its full shortcut help as a plain two-column aligned line list (each
row `  <key>       <description>`) in its help overlay, plus a compact
one-line hint in the prompt block's border title. Both TUIs therefore offer a
compact footer hint and a full help overlay with the same fit-and-drop-tail
family of behavior, but the full-help presentation differs: maerg's is a
Markdown table, `skott`'s is a plain aligned line list. The key→action content
is complete and consistent in maerg. No test pins either format against the
other.
Disposition: recorded for arbitration — either the clause's "the same way"
should be read as "the same presentation family" (compact footer hints plus a
full help overlay, as both TUIs do), or the maerg F1 help should be
re-rendered as `skott`'s plain line list rather than a Markdown table.

**N-2 (note, low) — The component-argument list is maintained in several
places, not as one derived set.**
Intended: `REQ-INTERACTIVE-TUI` — the component "MUST be inserted visibly into
the editable command buffer as soon as the next argument expects a component,
except `cd`".
Observed: the visible insertion is driven by `component_position`, which fills
every command whose next positional is a `COMPONENT_DIR` slot (pinned for the
eleven forms `task run`/`next`/`transition`/`log`/`recover`/`finalize`,
`component validate`/`accept`, and the `tasks`/`run`/`status` builtins; the
slot detection also covers `task unlock`, which the prefill test does not
explicitly pin). Separately, the dispatch-time fallback inserts the focus into
omitted component arguments of eight forms (`task next`/`log`/`transition`/
`unlock`/`recover`/`finalize`, `component validate`/`accept`), and `task run`
plus the `tasks`/`run`/`status` builtins resolve the component from focus in
their own branches instead. On this surface the parts agree in effect — with a
component focused, every command that expects a component receives it (visibly
or via the fallback), and `cd` never does — but the membership is expressed in
three places (the `component_position` slot detection, the `dispatch`
fallback list, and the `task run`/builtin branches) rather than one shared
definition.
Disposition: recorded as a maintenance-risk observation, not a behavioral
defect. A future CLI change that adds a component-arg command to only one of
those places would silently desynchronize the visible prefill from the
dispatch fallback.

**N-3 (note, low) — "Recording failures are fatal to the workspace"
(CONTRACT.md) versus the observed report-and-continue behavior.**
Intended: `CONTRACT.md` — "Recording failures are fatal to the workspace
rather than silently losing interactions." `REQ-TRANSCRIPT-PERSISTENCE`
requires only that private history "report recording failures".
Observed: an append failure never exits the shell: the in-loop handler records
the failure as a visible `## Error` document in the notice bar and the
transcript (when it can), reports it to stderr when even that fails, and keeps
the session alive; only terminal-death I/O errors break the loop, with the
terminal restored. The interaction is never silently lost — it is always
visible — but the process does not terminate on a recording failure.
Disposition: recorded for arbitration — if "fatal to the workspace" is meant
literally (the workspace exits when it cannot record), the observed behavior
is a mismatch; if it means "fatal to silent operation" (a recording failure
must always be surfaced to the operator), the observed behavior satisfies it.
The requirement text (report) is met under either reading.

## Evidence limitations retained

- The PTY test does not execute a `task run` through the embedded Skott path
  inside the TUI; the task-run auto-selection and continuous-mode paths are
  pinned only by in-crate assertions against the canonical queue reader, not
  by an end-to-end TUI run.
- The layout test pins the heading fragments `Kvist`, `(project)`,
  `sandbox:`, and `model:`, not the full heading wording; the exact wording of
  the branch and lock-count fields is pinned only by the source observation.
- The `view.rs`/`menu_tests.rs` suites drive `Workspace` in-process against a
  `TestBackend`; they are not execution proof of the live terminal beyond the
  single PTY test.
- No execution evidence from this review's own turn is asserted: the named
  suites run automatically in the verification sandbox after this turn.

## Summary

Within the bounded evidence, `REQ-INTERACTIVE-TUI`, `REQ-TRANSCRIPT-PERSISTENCE`,
and the `Provided interfaces` clauses bearing on these surfaces are observed
to be satisfied; no behavioral mismatch requiring corrective work was
established. Three low-severity notes (N-1 shortcut-help format, N-2 dual
command lists, N-3 recording-failure fatality reading) are recorded for
explicit human arbitration, with neither artifact altered. This record is the
independent gate's disposition, not acceptance, and does not imply human
approval, policy approval, or publication.
