# Maerg scoped status and navigation: source-only observation

## Provenance and limits

Observed on 2026-11-04 from current working-tree Rust production and test
source. This is a bounded implementation observation, not an intent comparison,
compliance determination, security audit, or acceptance record.

The inspected production scope was status declaration/dispatch and bare
invocation in `maerg/src/cli.rs`; status resolution and normalization in
`context.rs`; `status.rs` and `status_markdown.rs`; inspection/component
discovery in `project_state.rs`; degraded-host status/navigation and Reedline
wiring in `bark.rs`; `bark/{prefill,completion,tree}.rs`;
`bark/tui/editor.rs`; and relevant editor/dispatch/focus paths in
`bark/tui/workspace.rs` and `bark/tui/workspace/actions.rs`. Test-source
observations use `maerg/tests/{scoped_status,status,markdown_output,shell_smoke,tui_workspace}.rs`
and directly relevant in-source tests.

No project/component intent artifacts, implementation records, previous review
records, session history, or Git commands/history/diffs were consulted. No
builds, tests, component workflows, or acceptance commands were executed. No
executed test evidence was supplied to this observer. All test coverage below
means **assertions present in source**, not observed passes. Only this
supplement was written; source, tests, queues, and implementation records were
not changed.

## Current behavior

### Target resolution and scope

- `status` accepts an optional positional `COMPONENT_DIR`, `--project
  PROJECT_DIR`, `--details`, `--format overview|text|json`, artifact filters,
  and `--unfinished`
  ([declaration](../../maerg/src/cli.rs#L255-L282)).
- Without a target, status resolves the nearest project and requests project
  scope, even from a nested component directory. Literal `.` instead requests
  the configured root component, not the component containing the current
  directory. An explicit `--project` makes any positional target a component
  argument within that project. Without that option, an explicit project-root
  path is recognized as project scope; an absolute path inside a project can
  select a component in that project. Outside a project, a positional directory
  can fall back to project inspection
  ([resolution](../../maerg/src/context.rs#L36-L90)).
- Component normalization accepts component-root-relative, configured-root-
  prefixed, and in-project absolute spellings. Current-directory segments are
  removed; parent traversal is rejected rather than resolved. Root results
  become `.`. These are lexical operations, not canonicalization of symlinks.
  Positional status resolution also rejects parent segments before deciding
  project versus component scope, whereas an explicit `--project` uses the
  ordinary explicit-project resolver
  ([normalization](../../maerg/src/context.rs#L247-L299),
  [resolution](../../maerg/src/context.rs#L36-L90)).
- Selection requires an exact discovered component path. It neither includes
  descendants nor silently falls back to the project for an unknown component.
  Selection happens before every presentation format. Invalid/discovery-failed
  selections return errors
  ([inspection/selection](../../maerg/src/cli.rs#L16-L72)).

### Compact, detailed, and native presentations

- Ordinary project status with overview format emits a compact Markdown table.
  A component target emits that component's detailed Markdown, independently
  of `--details`. Project `--details`, `--only-documents`, or `--only-impls`
  selects full Markdown details without widening a component target
  ([dispatch](../../maerg/src/cli.rs#L1815-L1848)).
- Compact rows contain component state, valid-artifact count, completed/total
  tasks, nonzero in-progress/blocked/awaiting-decision counts, and a concrete
  action. The default action is `kvist status COMPONENT`; a current component
  with a queue-selected ready task instead receives `kvist task run COMPONENT
  TASK`. Empty results have an explicit message. No per-component artifact
  tables, changed-document digests, task reasons, or VCS tracking table appear
  in this compact projection
  ([summary](../../maerg/src/status_markdown.rs#L265-L329)).
- Detailed Markdown includes component artifact states, changed-document
  revisions and Git-diff suggestions, task progress, ready-task/run commands,
  blocked/decision reasons, and state-specific next steps. Artifact filters
  narrow component artifact rows; `--only-impls` also suppresses revalidation
  details, but neither filter suppresses task guidance
  ([details](../../maerg/src/status_markdown.rs#L110-L263)).
  Blocked-task guidance names a transition to pending and incomplete dependency
  counts; decision guidance names `kvist decision COMPONENT show TASK`
  ([command generation](../../maerg/src/bark.rs#L1253-L1293)).
- Markdown queue facts are read separately using a 1,048,576-byte bounded read
  and queue parsing. Failures become `unavailable: ...` in compact rows or
  `Task progress unavailable` in details, not a fabricated zero-task result
  ([queue read](../../maerg/src/status_markdown.rs#L331-L347),
  [error projections](../../maerg/src/status_markdown.rs#L239-L245),
  [summary](../../maerg/src/status_markdown.rs#L283-L313)).
- Explicit native text and JSON remain version-1 projections, not Markdown.
  Global `--json status` chooses native JSON regardless of overview/details;
  JSON carries inspection fields rather than the Markdown task-progress or
  VCS/toolchain tables. Text has component/artifact/revalidation facts and
  state guidance; its supplementary blocked-task queue read silently yields
  no entries on read/parse failure
  ([JSON dispatch](../../maerg/src/cli.rs#L1203-L1219),
  [text](../../maerg/src/status.rs#L43-L162),
  [JSON](../../maerg/src/status.rs#L353-L460),
  [best-effort guidance read](../../maerg/src/status.rs#L238-L251)).
  The public library `status::render(Overview)` still selects the separate
  legacy overview renderer; the CLI's overview branch bypasses it for Markdown
  ([library dispatch](../../maerg/src/status.rs#L25-L39),
  [CLI dispatch](../../maerg/src/cli.rs#L1827-L1846)).
- Bare CLI invocation starts the interactive host only with terminal stdin and
  stdout and without `--json`. Redirected invocation inside a project emits the
  compact Markdown summary plus a tip; bare JSON invocation wraps its report
  in a success/command/report object
  ([bare invocation](../../maerg/src/cli.rs#L2200-L2249)).

### Global context, health, and peer filtering

- Markdown retains project state, available root/discovery diagnostics,
  non-current project guidance, VCS summary/diagnostic, and optional toolchain
  information. Compact and component-detail context list only non-valid
  project artifacts; full project details list all retained project artifacts
  ([context rendering](../../maerg/src/status_markdown.rs#L31-L107)).
- Ordinary inspection enumerates components only when project state is
  current or blocked. Explicit component status retries component inspection
  when the initial inspection has no component root. Consequently, a project
  with invalid global documents can have no component rows in ordinary
  project status yet still show a specifically selected component when its
  configuration/discovery remain usable. Invalid configuration can prevent
  component resolution before this retry
  ([inspection gate](../../maerg/src/project_state.rs#L498-L509),
  [retry](../../maerg/src/cli.rs#L20-L38),
  [component resolution](../../maerg/src/context.rs#L133-L138)).
- Selected status keeps global artifact entries, all non-valid artifact
  entries, and artifact entries whose immediate directory is the selected
  component directory. VCS artifact entries use exact immediate-directory
  filtering. Thus healthy peer/descendant detail is removed, but invalid peer
  artifacts already in the project's artifact list remain diagnostic context.
  With component root `.`, that project list is populated from discovered
  components; otherwise it lists the configured root component's artifacts.
  VCS summary/diagnostic is computed before filtering and is not recomputed
  for the selected component
  ([filters](../../maerg/src/cli.rs#L50-L72),
  [artifact enumeration](../../maerg/src/project_state.rs#L543-L639),
  [VCS inspection](../../maerg/src/project_state.rs#L1075-L1126)).

### Focus insertion, completion, and navigation

- The completion tree comes from clap plus shell builtins. `cd` has a
  `COMPONENT_DIR` destination, so it remains eligible for discovered-component
  completion and current-component promotion. Prefill separately excludes
  `cd` unconditionally: destination completion is not automatic insertion
  ([tree](../../maerg/src/bark/tree.rs#L124-L156),
  [`cd` positional](../../maerg/src/bark/tree.rs#L180-L185),
  [completion domain/order](../../maerg/src/bark/completion.rs#L536-L591),
  [prefill exclusion](../../maerg/src/bark/prefill.rs#L81-L103)).
- Shared prefill inserts focus into a vacant component slot in the real edit
  buffer. Bare `status` at end of input is a special insertion trigger without
  requiring a trailing space. Supplied/partial values are not overwritten;
  a supplied or inserted component marks that command path as handled until
  the next submission reset, so deliberate deletion is not immediately
  reversed. Whitespace/quotes/backslashes are escaped; dash-leading and
  command-word focus names receive `./`
  ([prefill](../../maerg/src/bark/prefill.rs#L36-L172),
  [slot detection](../../maerg/src/bark/completion.rs#L304-L342)).
- Reedline uses abbreviations, the shared highlighter/break signal, and
  `ExternalBreak` handling to modify its authoritative buffer. The full-screen
  editor calls the same insertion logic after ordinary key edits, paste, and
  completion acceptance
  ([Reedline wiring](../../maerg/src/bark.rs#L1654-L1668),
  [read loop](../../maerg/src/bark.rs#L1754-L1763),
  [TUI dispatch](../../maerg/src/bark/tui/workspace.rs#L593-L670),
  [editor insertion](../../maerg/src/bark/tui/editor.rs#L66-L70)).
- In both hosts, no-argument `cd` and `cd -` clear component focus; `cd .`
  selects the discovered root component. Neither is an operating-system
  directory change. The degraded host updates shared focus and journals the
  result; the TUI switches transcript/history scope, resets input/prefill, and
  records the new focus
  ([degraded host](../../maerg/src/bark.rs#L503-L555),
  [destination resolution](../../maerg/src/bark.rs#L1063-L1090),
  [TUI builtin](../../maerg/src/bark/tui/workspace/actions.rs#L49-L63),
  [TUI scope change](../../maerg/src/bark/tui/workspace/actions.rs#L281-L334)).
  Both `cd` implementations accept discovered names after stripping one
  leading `./`; they do not share status's full prefixed/absolute normalization.
- Status has no dispatch-time hidden focus fallback. A truly bare submitted
  status therefore remains project-scoped after deliberate deletion. The
  degraded host's bare status/overview path renders a project summary; the
  TUI's argument injection covers selected task/component commands, not status
  ([degraded status](../../maerg/src/bark.rs#L403-L408),
  [overview handler](../../maerg/src/bark.rs#L435-L452),
  [degraded injection](../../maerg/src/bark.rs#L1461-L1485),
  [TUI injection](../../maerg/src/bark/tui/workspace/actions.rs#L173-L198)).

## Test-source coverage

- [`scoped_status.rs:55–178`](../../maerg/tests/scoped_status.rs#L55-L178)
  asserts compact row counts, explicit details, Markdown pipe escaping, exact
  selection, normalized spellings, selected native formats/global JSON
  equality, and exclusion of healthy peers/descendants from artifact/VCS output,
  including a `.` component-root configuration.
- [`scoped_status.rs:180–339`](../../maerg/tests/scoped_status.rs#L180-L339)
  asserts task-state counts, compact suppression of execution suggestions for
  a blocked component, a current/ready run suggestion, project/`--project`/`.`
  distinctions, project scope from a nested cwd, unknown/traversal errors,
  malformed-queue diagnostics without queue changes, selected status despite
  an invalid global document, artifact filters, and empty filtered results.
- [`status.rs:61–190`](../../maerg/tests/status.rs#L61-L190),
  [`222–327`](../../maerg/tests/status.rs#L222-L327),
  [`468–660`](../../maerg/tests/status.rs#L468-L660), and
  [`743–824`](../../maerg/tests/status.rs#L743-L824) assert native schema/output,
  state/filter projections, changed-document guidance, exact blocked/decision
  commands, uninitialized-project advice, and invalid global-artifact reasons.
  [`markdown_output.rs:11–65`](../../maerg/tests/markdown_output.rs#L11-L65)
  distinguishes ordinary Markdown from native machine routes.
- [`prefill.rs:278–304`](../../maerg/src/bark/prefill.rs#L278-L304)
  asserts real-buffer bare-status insertion, deletion/explicit-value
  suppression, and no `cd` insertion across hooks. Other adjacent tests cover
  flags, quoting, paste, history observation, resets, and dash-leading focus.
  [`bark.rs:2112–2149`](../../maerg/src/bark.rs#L2112-L2149) explicitly tests
  both bare and dash clearing in the degraded host.
- The degraded-host PTY test source checks prefill before submission,
  deletion/replacement, recalled explicit input, paste, quoted/dash-leading
  focus, focused bare status, no `cd` prefill, and project status after bare
  `cd` ([`shell_smoke.rs:550–695`](../../maerg/tests/shell_smoke.rs#L550-L695)).
  The TUI PTY test asserts focused `status .` in component command history,
  bare `cd` returning to project scope, and subsequent compact project output
  ([`tui_workspace.rs:246–269`](../../maerg/tests/tui_workspace.rs#L246-L269)).
  These assertions do not constitute executed terminal evidence here.

## Concrete observable inconsistencies and limitations

1. **Run advice uses different state gates.** Compact Markdown requires
   component state `current` before suggesting execution; detailed Markdown
   emits a run command whenever queue selection returns a ready task, even
   for a stale/blocked component. Neither branch checks project state before
   constructing that advice. This can place run advice beside global recovery
   guidance or a component's accept/blocker guidance. The scoped test asserts
   compact suppression for blocked work but does not assert equivalent
   suppression in details
   ([compact gate](../../maerg/src/status_markdown.rs#L281-L309),
   [detail advice](../../maerg/src/status_markdown.rs#L212-L258),
   [global guidance](../../maerg/src/project_state.rs#L1496-L1504),
   [test assertions](../../maerg/tests/scoped_status.rs#L230-L249)).
   This observation concerns displayed suggestions, not whether execution
   would succeed.
2. **`--unfinished` means non-current component state, not unfinished tasks.**
   Both Markdown projections and native projections skip a current component
   without examining its task completion count. A valid/current component with
   pending or in-progress work can therefore disappear from this report.
   Component classification checks blocked tasks but does not make pending,
   in-progress, or awaiting-decision status alone non-current
   ([Markdown filtering](../../maerg/src/status_markdown.rs#L118-L121),
   [compact filtering](../../maerg/src/status_markdown.rs#L270-L274),
   [native filtering](../../maerg/src/status.rs#L390-L398),
   [classification](../../maerg/src/project_state.rs#L815-L844)).
3. **Exact output scope is not isolated inspection.** Full project inspection
   and, when needed, full component discovery/inspection precede the retain
   filter. A peer artifact's filesystem error can abort selected-component
   status before rendering; malformed artifact states are not universally
   fatal, but actual metadata/read errors propagate. The project-summary
   health gate can also yield the generic “No components match” message when
   component enumeration was skipped, rather than because no components
   exist
   ([ordering](../../maerg/src/cli.rs#L20-L52),
   [all-component collection](../../maerg/src/project_state.rs#L661-L681),
   [I/O propagation](../../maerg/src/project_state.rs#L977-L1003),
   [health gate](../../maerg/src/project_state.rs#L503-L509),
   [empty message](../../maerg/src/status_markdown.rs#L324-L327)).
   No permission-error reproduction was performed.
4. **TUI history recall bypasses immediate prefill.** Recall sets input directly
   without calling prefill; Enter then submits it directly. Thus a recalled
   bare `status` can remain project-scoped while a freshly typed/pasted bare
   `status` under the same focus receives a component insertion. Reedline's
   highlighter has a bare-status insertion trigger. The cited PTY assertions
   cover freshly submitted focused status, not this recalled-bare-status
   distinction
   ([TUI recall](../../maerg/src/bark/tui/workspace.rs#L673-L692),
   [Enter/edit handling](../../maerg/src/bark/tui/workspace.rs#L621-L629),
   [submission](../../maerg/src/bark/tui/workspace/actions.rs#L17-L23),
   [Reedline trigger](../../maerg/src/bark/prefill.rs#L135-L172)).

These are source-derived observations for subsequent independent comparison;
they neither establish nor reject compliance.

## Correction observation

A separate clean-slate, source-only context observed the following corrected
paths after the original record. It did not read intent, this record, prior
reviews, implementation records or history, and did not execute tests.
Original findings 1 and 4 are superseded by these observations; findings 2 and
3 remain descriptions of the implemented semantics and inspection limits.

Detailed status emits its explicit `kvist task run` suggestion only when queue
selection returns a ready task and both project and component are Current;
otherwise it retains the ready-task label with "Execution unavailable."
Summary uses the same gates, falling back to `kvist status COMPONENT`. Both
retain project-state context and diagnostics
(`maerg/src/status_markdown.rs:30-75,212-227,271-335`).

Idle history recall immediately calls prefill after replacing the real input
buffer. `Input::prefill` inserts actual text, advancing the cursor. Explicit
supplied components are recorded even when insertion is rejected, preventing
automatic replacement after deletion; `begin_submission` clears this
per-command suppression
(`maerg/src/bark/tui/workspace.rs:673-695`;
`maerg/src/bark/tui/editor.rs:31-46,66-70`;
`maerg/src/bark/prefill.rs:36-40,81-123`). While running, history/completion
navigation is ignored and ordinary editing/paste skips prefill. Enter normally
routes to submission; its running-command branch sends the buffer as an answer,
clears it, and returns before history recording
(`maerg/src/bark/tui/workspace.rs:563-630,646-662`;
`maerg/src/bark/tui/workspace/actions.rs:10-24`).

The named test sources assert mixed-state progress and suppressed run advice,
ready-current summary advice, unavailable-project detailed suppression,
recalled bare-status prefill, and explicit-value preservation after deletion
(`maerg/tests/scoped_status.rs:181-275`;
`maerg/src/bark/tui/workspace/menu_tests.rs:16-32`). These are source assertions,
not executed evidence; no tests were run. No high-confidence remaining
discrepancy was identified within these bounded paths; this is not a compliance
determination.
