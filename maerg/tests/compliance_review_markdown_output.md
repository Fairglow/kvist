<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Markdown Output (`maerg`)

**Component:** `maerg` (Kvist root component)
**Task:** `md-output-compliance-review`
**Scope:** The Markdown output layer — the canonical Markdown projection of command
results (`status_markdown.rs` and its `presentation.rs` helpers, dispatched from
`cli.rs`), the unchanged native `text`/`json` report forms (`status.rs`), the
Markdown wrapper applied to every human command (`cli.rs::execute`), and the
`blad`-rendered TUI status heading/transcript that presents the document
(`bark/tui/workspace/view.rs`) — compared against `REQ-MARKDOWN-OUTPUT` and the
contract's `Provided interfaces` / "Markdown-first output and the interactive
TUI" clauses.
**Reviewer stance:** Independent compliance reviewer, in two phases per the task
prescription: (1) a clean-slate derivation of the observed Markdown-output
behavior from the implementation source and its tests, recorded here as the
derived `IMPL.md` observation; then (2) a source-blind comparison of that
derivation, `IMPL.md`, and the bounded test evidence against
`REQ-MARKDOWN-OUTPUT` and the Provided-interfaces contract. The implementer did
not author this record.

## Review boundary and independence

Per `REVIEW_RUNBOOK.md` *Clean-slate implementation-record pass* and *Source-blind
compliance pass*, the reviewer records each requirement clause and contract
guarantee as **compliant**, **mismatched**, **approved-deferred**, or
**underspecified**, with a stable locator, intended text, observed
behavior/evidence, severity, and owner. The nearest-ancestor component contract is
`null` (`maerg` is the root component); `ROOT_CONTRACT.md` is the only propagated
higher-level context.

**Read (permitted intent evidence):**
- `maerg/REQUIREMENTS.md`
  `sha256:c786ec71ab7aa367e6305ae1a715adb6a16c7126410fe72dd760c83d9f3e712e`
  (clause `REQ-MARKDOWN-OUTPUT` and the degradation clause of
  `REQ-INTERACTIVE-SHELL`),
- `maerg/CONTRACT.md`
  `sha256:957ce69d562103d49f26cac15691f96f1fe3ab9ab0e43962af6cb89776d9d53a`
  (`Provided interfaces` → `status` command clause; "Markdown-first output and
  the interactive TUI" → well-formed-document clause, status-heading clause,
  TUI-degradation clause, and the unchanged `text`/`json` forms clause),
- `maerg/DESIGN.md`
  `sha256:bc195225b837d3416c7d9fe9bb154759606bdbd4246dc768687e0349f0acef3f`
  ("Markdown-first output and the Blad-rendered TUI"; "Guided help and actionable
  status guidance"),
- `maerg/IMPL.md`
  `sha256:6f9996f689b588b8fdc02b2b344973cddc845d0f9f9c595db140331144d7b1b1`
  (checked for a scoped Markdown-output observation; see F-1),
- `ROOT_CONTRACT.md` (Change and compliance rules), and
- `REVIEW_RUNBOOK.md` (procedure).

**Clean-slate observation carrier (phase 1):** this task prescribes deriving
`IMPL.md` from the changed source and test evidence before comparing. Phase 1
derived the observed behavior by reading the implementation and its tests, with
intent artifacts, prior records/reviews, and history out of view, and recorded it
in **Phase 1 — Clean-slate `IMPL.md` observation (derived)** below. The carriers
read were:

- `maerg/src/status_markdown.rs`
  `sha256:d6ac7b55e292334b864210f3fa31dec0aa98efa1545d6bf4c790b94c71ec334c`
  (the primary changed file: `render`, `render_component`, `render_summary`,
  `render_context`, `render_details`, `read_queue`),
- `maerg/src/presentation.rs`
  `sha256:d2cf40fe75f0020082890aa186451b6ff6ff3fd95613f8d6d50a564b05a8df39`
  (`sanitize`, `literal`, `fence`, `report`/`report_redacted`, `document`,
  `legacy_report`),
- `maerg/src/markdown_structure.rs`
  `sha256:36551ff114ed166d056c974845460c43211919e498e55d1bf4cbec9812fe81b0`
  (ATX/fence/comment Markdown scanner, `blockers`, `draft`),
- `maerg/src/status.rs`
  `sha256:13c37dfb3029752e01a5d215ef89bbacf41724a37079772abbd8de15d52c8ad4`
  (`render_markdown`/`render_component_markdown`/`render_summary_markdown`
  wrappers; the native `render` → `render_text`/`render_json`/`render_overview`
  dispatch; `project_action_line`),
- `maerg/src/cli.rs`
  `sha256:3cf0da3366b97954689178cff34d22bac3e41b70f7040887a2ab58a1df47394b`
  (the `Command::Status` human/native routing, the `execute` Markdown wrapper, the
  `--json` native route, and `bare_invocation`),
- `maerg/src/bark.rs` `sha256:e110cd3a316ac9fc9f0524aff110694f3dc622c845ffa3e87c3b4131870509b8`
  and `maerg/src/bark/tui/workspace/view.rs`
  `sha256:b75337ce1408b57af2c83fd956137d7141bb18922311779b11493a802d78d2b8`
  (the TUI status heading, stats line, and `blad`-rendered transcript),
- `maerg/src/task_queue.rs`
  `sha256:0dfb0e79d4d9d2a6c1b9071b1c24f992d910279fc716e28e7e8710b029828508`
  (`next_ready_task_id`, `ready_task_ids`, the single-line `blocked_reason`
  bound, the blocked/awaiting-decision reason invariant),
- `maerg/src/project_state.rs`
  `sha256:f37575ee2cefb291d3a99860dcfb62c852fed575a4cf962b1631e513528d5261`
  (read-only `inspect`, `ProjectState`/`ComponentState` names),
- `maerg/src/repair.rs`
  `sha256:f16b8232ebb4635d0c7bac05aa5573ddb19ad485cada50fe1c9fc03de7ea7030`
  (`RepairReport::complete`/`Display`, the `# Artifact Recovery` failure document),
- and the test evidence `maerg/tests/markdown_output.rs`
  `sha256:7886e7852e339f93b55ebfeac6878a7e0ee09b06de2daa8678dad0c930b9c564`
  (3 integration tests) and `maerg/tests/status.rs`
  `sha256:95572c72412ffd5403334af3bbd08f0f04d177d59ca8a48990fc432e7dc623f4`
  (the native-form suite).

**Write-authority limitation (stated for the arbitrator):** the component-root
`maerg/IMPL.md` is outside this reviewer's write scope (which is limited to
`maerg/src/` and `maerg/tests/`). The clean-slate `IMPL.md` observation is
therefore recorded here, verbatim and self-contained, in **Phase 1** for the
clean-slate documenter to promote into the root `IMPL.md`. This is why F-1 is
retained rather than closed by a direct edit: the observed carrier is present in
this record but absent from the artifact the source-blind pass is meant to read.

**Not read as evidence:** peer component implementations, `sav/`, `galla/`,
`skott/`, `blad/` source, Git history or diffs, prior compliance conclusions, and
the security-audit stage's report (`tests/security_audit_md_output.md` is present
but is not in this reviewer's permitted read set and its conclusions are neither
relied on nor repeated). No tests, builds, linters, or `cargo` invocations were
executed in this review; `maerg/tests/markdown_output.rs` and the related suites
are run automatically by Kvist in the verification sandbox after this turn, and no
pass/fail count from that run is asserted here.

*Context-isolation note (stated for the arbitrator):* both phases were performed in
a single session, so this is the analytical separation the task prescribes (derive
the observed `IMPL.md` from source and tests, then judge the intent against that
derivation) rather than a fully isolated separate-agent source-blind context. The
intent texts and the phase-1 source read were in the same session. The retained
findings do not depend on the phase-1 read: F-1 is established by a term sweep of
`IMPL.md` that a fully isolated source-blind reviewer would perform identically,
and F-2 is established by the `render_summary` source plus the integration test,
both of which are independent of the phase-1 read.

## Phase 1 — Clean-slate `IMPL.md` observation (derived)

The following is the observed behavior of the Markdown output layer, derived
clean-slate from the source and test evidence listed above and recorded for
promotion into the component-root `IMPL.md`. It describes observed behavior only,
marks uncertainty, and does not copy intended behavior.

### Canonical Markdown projection of a command result

Every human command result is emitted as a Markdown document. The dispatch layer
wraps a non-native, non-empty result that does not already begin with an H1 in
`# <Title>` plus the report body (`presentation::document`), where the title is
`Project Status`, `Project Inspection`, `Component Tree`, `Kvist Help`, `Task
Result`, `Component Result`, `Decision`, `Artifact Recovery`, `Agent
Configuration`, or `Kvist` by command. The `status`, `doctor`, `tree`, `help`,
and `task next` human routes already produce Markdown beginning with `# Project
Status` / their respective headings, so the wrapper is a no-op for them. The
Markdown builders never emit ANSI styling: `presentation` is documented as
"terminal-capability-independent," `sanitize` neutralizes control characters,
`literal` escapes table-cell delimiters, and `fence` emits backtick fences whose
length exceeds the longest backtick run in the content.

### What the status/doctor Markdown states

- **Observed state.** A `# Project Status` H1, then `**Project:** <dir>` and
  `**State:** <project state>`; per component, `## Component: <path>` and
  `**State:** <component state>`. The compact default view is a single
  `| Component | State | Artifacts | Tasks | Next action |` table row per
  component (omitted when `--unfinished` and the component is `Current`).
- **Document validity.** A `| Artifact | State |` table for root documents
  (shown when `--details`, or whenever any root artifact is invalid), a per
  component `| Artifact | State |` table (respecting `--only-documents` /
  `--only-impls`), and an `Artifacts` column of `N/M valid` in the compact view.
- **Task progress.** `**Task progress:** done/total completed` plus per-status
  counts (`blocked`, `in-progress`, `awaiting-decision`), read from a bounded
  (1 MiB) parse of the component `TODOS.yaml`; a queue read failure yields
  `**Task progress unavailable:** <error>` rather than a zero. The compact view
  carries the same progress in its `Tasks` column.
- **Next ready task.** `**Next ready:** <task id>` from the first dependency-ready
  task in queue order, or `no ready task; inspect blockers/dependencies or review
  completed work.` The compact view names the ready task in the `Next action`
  column as `kvist task run <comp> <task>` when project and component are
  `Current`.
- **Actionable next step for non-current states.** At project scope, a non-`Current`
  state emits `**Next Step:**` (a state-specific command from
  `project_action_line`) plus `**Guidance:**`. At component scope, a non-`Current`
  component emits `**Next Step:** review the reported state, then <command>`:
  `kvist component accept <comp>` for `Stale`, `kvist help task-states` for
  `Blocked`, otherwise `kvist doctor; kvist repair --dry-run`. A ready task adds
  `**Next command:** kvist task run <comp> <task>` when project and component are
  `Current`, else `**Execution unavailable:** resolve the project/component state
  before running queued work.` **The compact default view instead sets the
  per-component `Next action` to `kvist status <comp>` (a drill-down) for every
  non-`Current` component; the direct resolver appears only in the detailed /
  component-scoped view.** (See F-2.)
- **Blocked and awaiting-decision reasons as structured Markdown.** Each task with
  a `blocked_reason` (the queue parser requires a nonblank, single-line reason for
  `Blocked` and `AwaitingDecision`) is rendered as a `### <status label>: <task
  id>` heading, the reason in a `fence(reason, "text")` fenced block, and a
  `**Next:**` fenced command from `next_command_for` (reopen-to-`pending` with the
  incomplete-dependency count, or `kvist decision <comp> show <task>` for
  awaiting-decision).
- **Stale-document revision details as structured Markdown.** Each revalidation
  cause is a `### Changed document: <path>` heading, a `| Revision | Digest |`
  table with `expected` / `observed` rows, and a `**Review under Git:**` fenced
  `git diff HEAD -- <argument>` block (the argument is shell-quoted when it is
  not a plain path).
- **Diagnostics, VCS, toolchain.** Root/discovery diagnostics, a VCS summary and
  optional per-artifact tracking table (details only), and a toolchain line with
  availability and diagnostic.
- **Machine routes retained, not Markdown.** Global `--json` and
  `status --format json` return the native version-one JSON report;
  `status --format text` returns the `status-format-version: 1` text form;
  `completions <shell>` returns a shell script. `--json status` is byte-identical
  to `status --format json`. `authoring-apply` is a hidden internal command
  dispatched before presentation. `repair --dry-run` on an incomplete project
  fails with a `# Artifact Recovery` Markdown document on stderr (the human
  route); the same command under `--json` fails with JSON on stderr. A successful
  (complete) `repair` result is also Markdown.

### Presentation in the TUI

The full-screen host presents the command's Markdown document as the transcript,
rendered by the `blad` renderer at the host width/theme. The status heading is a
separately composed chrome line: `Kvist | <component or (project)> | <state> |
<progress>`, with a secondary stats line carrying the VCS branch, sandbox
backend, default model, and `locks: N live / M stale`. When no component is
selected the heading shows `(project)`. The host degrades to the line-oriented
shell (Reedline) when stdin/stdout is not a terminal or styling is disabled; the
degraded path reports the same terminal-capability-independent Markdown as plain
text. The TUI is not reachable under `--json` or redirected invocation.

### Well-formedness and read-only-ness

The Markdown is built from a read-only `ProjectInspection` (no filesystem
mutation), reads queues only for display through a bounded reader, and uses
escaping/fencing so that table cells, control characters, and backtick runs in
untrusted content cannot break the document structure. It is a presentation of
state, not the durable store.

**Observation uncertainty:** the exact on-screen TUI geometry and `blad`
color/contrast behavior are delegated to the `blad` renderer and the TUI host and
are not established by this observation; the native `text`/`json` field-level
shapes are established by the `status.rs` native suite, not re-derived here. The
box-drawing `render_overview` in `status.rs` is a legacy/internal path (the CLI
human route uses the Markdown `render_summary_markdown` for the default Overview
form, and `render_overview` is otherwise reached only from in-crate tests); it is
not user-facing for `kvist status` and is not part of the observed Markdown
output.

## Bounded evidence relied upon

The executable statements of observed behavior are:

- `maerg/tests/markdown_output.rs` (3 integration tests):
  - `ordinary_commands_emit_markdown_but_machine_routes_remain_native` — after
    `init`, each of `status`, `doctor`, `tree`, `help`, and `task next .` emits
    stdout beginning with `# ` and containing no `ESC` byte; `status --format
    text` begins with `status-format-version: 1`; `status --format json` parses
    as JSON; `--json status` is byte-identical to `status --format json`;
    `completions bash` begins with `_kvist()`.
  - `status_markdown_has_structured_document_and_task_facts` — `status --details`
    contains `| Artifact | State |`, `Task progress`, and `Next`, and does not
    contain the box border `╭──`.
  - `incomplete_human_repair_is_markdown_and_explicit_json_stays_native` —
    `repair --dry-run` (incomplete) fails with stderr beginning
    `# Artifact Recovery`; `--json repair --dry-run` fails with JSON on stderr.
- `maerg/tests/status.rs` — the native `text`/`json`/`overview` suite pinning the
  unchanged version-one report forms.

These run automatically by Kvist after this turn; no result from that run is
asserted here.

## Method

Every obligation was drawn from `REQUIREMENTS.md` (`REQ-MARKDOWN-OUTPUT`, and the
degradation clause of `REQ-INTERACTIVE-SHELL`) and `CONTRACT.md` (`Provided
interfaces` → `status` command; "Markdown-first output and the interactive TUI" →
the well-formed-document clause, the status-heading clause, the TUI-degradation
clause, and the unchanged `text`/`json` forms clause), with `DESIGN.md`
("Markdown-first output and the Blad-rendered TUI"; "Guided help and actionable
status guidance") as bounded supporting context. Each was compared against the
phase-1 clean-slate derivation, `IMPL.md`, and the bounded test evidence. The
phase-1 derivation is the primary observation carrier for the behavioral clauses
because `IMPL.md` carries no scoped observation of this change (F-1); each clause
cites its intent text, the clean-slate observation, and the matching test
assertion. Each finding distinguishes **missing evidence** from **contradictory
evidence**.

## Satisfied clauses (compliant)

**C-1 — Every (human) command reports its result as a well-formed Markdown
document that is the canonical description of that result.**
`REQ-MARKDOWN-OUTPUT` (first sentence); `CONTRACT.md` "Markdown-first output"
("Every command reports its result as a well-formed Markdown document that is the
canonical description of that result"). Clean-slate observation: the dispatch
layer wraps every non-native human result in a `# <Title>` Markdown document; the
`status`/`doctor`/`tree`/`help`/`task next` human routes emit `# …` directly;
`literal`/`fence`/`document` make cells, fences, and titles well-formed. Evidence:
test 1 (`init`, `status`, `doctor`, `tree`, `help`, `task next .` all begin with
`# ` and contain no `ESC`); test 3 (`repair --dry-run` → `# Artifact Recovery`).
The explicit machine/script routes (`--json`, `--format json`/`text`,
`completions`, `authoring-apply`) are the acknowledged exceptions the requirement
and contract preserve, not violations of the human-report clause. Compliant.

**C-2 — The document states the observed state.** `REQ-MARKDOWN-OUTPUT`
("MUST state the observed state"). Observation: `**State:** <project state>` and
per-component `**State:** <component state>`, plus a `State` column in the compact
table. Evidence: test 2 (`| Artifact | State |` present in `--details`). Compliant.

**C-3 — The document states document validity.** `REQ-MARKDOWN-OUTPUT` ("document
validity"). Observation: root `| Artifact | State |` table, per-component artifact
table, and `Artifacts` column `N/M valid`. Evidence: test 2 (`| Artifact | State
|`). Compliant.

**C-4 — The document states task progress.** `REQ-MARKDOWN-OUTPUT` ("task
progress"). Observation: `**Task progress:** done/total completed` + per-status
counts from a bounded queue parse, or `**Task progress unavailable:** <error>`;
compact `Tasks` column. Evidence: test 2 (`Task progress`). Compliant.

**C-5 — The document states the next ready task.** `REQ-MARKDOWN-OUTPUT` ("the
next ready task"). Observation: `**Next ready:** <id>` from `next_ready_task_id`
(or the explicit no-ready-task line); compact view names the ready task in `Next
action` as `kvist task run <comp> <task>` when current. Evidence: test 2
(`Next`); `task_queue::next_ready_task_id` source. Compliant.

**C-6 — The document provides an actionable next step for every non-current state
(project scope; component scope in the detailed view).** `REQ-MARKDOWN-OUTPUT`
("an actionable next step for every non-current state"). Observation: project
non-`Current` states emit a state-specific `**Next Step:**` command plus
`**Guidance:**`; component non-`Current` states (detailed/component view) emit a
direct `**Next Step:** review the reported state, then <command>` (accept / help
task-states / doctor+repair); a ready task adds `**Next command:**` or
`**Execution unavailable:**`. Evidence: `project_action_line` and `render_details`
source; test 2 (`Next`). Compliant at project scope and in the detailed/component
view; the compact default view's component-scope next step is a drill-down — see
F-2, retained for arbitration.

**C-7 — Blocked and awaiting-decision reasons are rendered as structured Markdown
(headings + fenced code), not unstyled text.** `REQ-MARKDOWN-OUTPUT` ("MUST render
blocked and awaiting-decision reasons … as structured Markdown (headings, tables,
lists, fenced code) rather than unstyled text"). Observation: each task with a
`blocked_reason` (required nonblank and single-line for `Blocked` and
`AwaitingDecision`) renders as a `### <label>: <id>` heading, the reason in a
`fence(reason, "text")` block, and a fenced `**Next:**` command; `status_label`
yields `blocked` / `awaiting-decision` / `in-progress`. Evidence: `render_details`
and `bark::next_command_for` source (returns a command for both `Blocked` and
`AwaitingDecision`). Compliant.

**C-8 — Stale-document revision details are rendered as structured Markdown.**
`REQ-MARKDOWN-OUTPUT` ("stale-document revision details as structured Markdown").
Observation: each revalidation cause renders as `### Changed document: <path>`, a
`| Revision | Digest |` table with `expected` / `observed` rows, and a fenced
`**Review under Git:**` `git diff HEAD -- <argument>` block (shell-quoted when the
path is not plain). Evidence: `render_details` source; test 2 (the `--details`
document contains the structured content and no box border). Compliant.

**C-9 — The Markdown document is the single source of truth for what a command
reports (human route).** `REQ-MARKDOWN-OUTPUT` ("single source of truth").
Observation: the human/overview route emits the Markdown document as stdout; the
TUI renders that same document string via `blad`; the legacy box-drawing overview
is not user-facing on this route. The `--json`/native-text forms are explicit
opt-in machine routes the requirement itself preserves, so they do not compete as
the human "source of truth." Evidence: `cli.rs::execute`, `render_*` builders, and
`view.rs` (blad rendering of the command document). Compliant for the human route;
the machine-route coexistence is by design (see C-10).

**C-10 — The stable `status --format text` and `--format json` forms are unchanged
for non-interactive consumers.** `REQ-MARKDOWN-OUTPUT` ("MUST be unchanged");
`CONTRACT.md` ("Native `text`/`json` retain their version-one shapes"; "The stable
`status --format text` and `--format json` report forms are unchanged for
non-interactive consumers"). Observation: `--format text` → `render_text`
(`status-format-version: 1`), `--format json` → `render_json`; the native route is
selected independently of the Markdown route; `--json status` is byte-identical to
`status --format json`. Evidence: test 1 (all three assertions); `tests/status.rs`
pins the native shapes. Compliant.

**C-11 — The plain-text degradation contract of `REQ-INTERACTIVE-SHELL` still
applies when the terminal cannot host the TUI or styling is disabled.**
`REQ-MARKDOWN-OUTPUT` (degradation clause); `CONTRACT.md` ("The TUI degrades to
the plain-text line-oriented shell when the terminal cannot host it, and the
plain-text degradation contract of `REQ-INTERACTIVE-SHELL` still applies").
Observation: the Markdown builders are terminal-capability-independent and emit no
ANSI (so the degraded line-oriented path reports the same document as plain
text); `bare_invocation` returns a finite Markdown overview (not a TUI) when
stdin/stdout is not a terminal, the TUI host degrades to the Reedline shell when
the terminal cannot host the layout, and a styling-disabled terminal degrades to
plain text; `--json` never enters the TUI. Evidence: test 1 (no `ESC` in the
Markdown output); `presentation.rs` doc + `sanitize`; `bare_invocation`
(terminal gate) and the host's terminal-size/env gating. Compliant.

**C-12 — The Markdown is the presentation of state, not a substitute for the
durable artifacts or for executable tests.** `REQ-MARKDOWN-OUTPUT` (last
sentence). Observation: the document is built from a read-only
`ProjectInspection` with no mutation, reads queues only through a bounded reader
for display, and is a projection rather than the durable store. Evidence:
`render_context`/`render_details`/`read_queue`; `project_state::inspect`
(read-only). Compliant.

**C-13 — The interactive shell renders the document with the `blad` renderer in a
full-screen TUI shaped like the `skott` layout (status heading + transcript +
footer).** `CONTRACT.md` "Markdown-first output" (the `blad`-rendering clause and
the layout bullets). Observation: the TUI composes a status heading line, a
secondary stats line, a `blad`-rendered transcript of the command's Markdown
document, and a command footer; the transcript is the durable, scrollable record.
Evidence: `view.rs` (heading/stats + `render::markdown` via `blad`). Compliant for
the TUI path (a host surface distinct from, and presenting, the Markdown output).

**C-14 — The status heading shows the active component with its state and task
progress, plus the VCS branch, sandbox backend, default model, and live/stale lock
counts; with no component selected it shows the whole project.** `CONTRACT.md`
"Status heading" bullet. Observation: the heading is `Kvist | <component or
(project)> | <state> | <progress>` and the stats line carries
`<branch> | sandbox: <backend> | model: <default_model> | locks: <live> live /
<stale> stale`; `focus()` is the selected component or `(project)`. Evidence:
`view.rs`. Compliant.

**C-15 — Default human output is one compact table row per component; a component
argument selects exact-component details; `--details` shows all details in project
scope and never overrides an explicit component filter; artifact-only filters imply
details.** `CONTRACT.md` `status` command clause. Observation: the `Command::Status`
human path selects `render_component_markdown` when a component is chosen (first),
else `render_markdown` when `--details` / `--only-documents` / `--only-impls`,
else `render_summary_markdown` (one compact row per component); the component filter
is checked before `--details`, and the two artifact-only filters are in the
`details` disjunction. Evidence: `cli.rs` dispatch; test 2 (`--details` shows the
structured details). Compliant.

## Discrepancies (retained for human arbitration)

**F-1 (LOW, missing evidence, not contradictory) — `IMPL.md` carries no scoped
observation of the Markdown output layer, so the runbook's primary source-blind
observation carrier is absent/stale for this change.**
- *Locator:* `maerg/IMPL.md`
  `sha256:6f9996f689b588b8fdc02b2b344973cddc845d0f9f9c595db140331144d7b1b1`;
  `ROOT_CONTRACT.md` Change and compliance rules (`IMPL.md` describes observed
  behavior and is not copied from intent); `REVIEW_RUNBOOK.md` (the clean-slate
  documenter produces the `IMPL.md` the compliance pass compares against).
- *Intended (declared):* the source-blind pass compares `REQ-MARKDOWN-OUTPUT` and
  the Provided-interfaces contract against a freshly derived `IMPL.md` observation
  of the changed behavior, plus bounded test evidence.
- *Observed:* a term sweep of `IMPL.md` finds **no** scoped observation of the
  Markdown output layer: zero hits for `status_markdown`, `render_summary`,
  `render_markdown`, `Next ready`, `Task progress`, `Next command`,
  `Changed document`, `git diff HEAD`, `Artifact Recovery`, `well-formed Markdown`,
  `single source of truth`, `status --format`, `format text`, and `format json`
  (the only hits are one incidental `compact Markdown` and one `completions`). The
  nearest sections — "Scoped status and component navigation observations
  (2026-11-04)" and "Scoped Markdown workspace observations (2026-11-03)" — describe
  the scoped-status navigation and the TUI workspace (transcript segments, blad
  rendering) respectively, neither of which records the observed shape of the
  status/doctor/tree/help/task Markdown projection, the native-form separation, the
  fenced blocked/awaiting reasons, the changed-document revision tables, or the TUI
  heading fields. The change's source is `status_markdown.rs`
  (`sha256:d6ac7b55…`), introduced by the Markdown-first-TUI and scoped-status
  changes; no `IMPL.md` addendum tracks it.
- *Why it matters:* this is **missing evidence, not contradictory evidence** — no
  observed behavior contradicts `REQ-MARKDOWN-OUTPUT` or the contract; the defect
  is that the artifact the source-blind pass is meant to read lacks the
  observation, so this reviewer had to derive it in Phase 1 (recorded above). It is
  the same class of finding as the soft-wrap review's `IMPL.md`-gap item.
- *Arbitration options:* (a) promote the Phase 1 observation in this record into
  the component-root `IMPL.md` (the clean-slate documenter owns that write; this
  reviewer's scope excludes the root artifact), then re-run the source-blind pass
  against the updated `IMPL.md`; (b) record a reasoned exception that
  `maerg/tests/markdown_output.rs` plus the Phase 1 derivation are the retained
  observation for this chain; (c) approved deferral with an owner. This review does
  not modify `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `IMPL.md`, or the
  implementation.

**F-2 (LOW, requirement granularity / underspecified, not contradictory) — the
default (compact) status document gives a non-current *component* a drill-down next
action rather than the direct resolving command.**
- *Locator:* `REQ-MARKDOWN-OUTPUT` ("an actionable next step for every non-current
  state"); `CONTRACT.md` `status` command clause ("default human output is one
  compact table row per component"); `maerg/src/status_markdown.rs::render_summary`
  (`sha256:d6ac7b55…`).
- *Intended (requirement/contract):* every non-current state is paired with an
  actionable next step; the default view is a compact one-row-per-component table.
- *Observed:* in `render_summary` the per-component `Next action` is
  `kvist status <comp>` for any non-`Current` component (it is only upgraded to
  `kvist task run <comp> <task>` when project *and* component are `Current`). The
  direct resolving command — `kvist component accept <comp>` for `Stale`,
  `kvist help task-states` for `Blocked`, otherwise `kvist doctor; kvist repair
  --dry-run` — is emitted only by `render_details` (the `--details` / component
  view). By contrast, the project-scope next step *is* direct in the compact view
  (`project_action_line`), and the project `**Next Step:**`/`**Guidance:**` lines
  are present via `render_context`. So the direct component-level resolver is
  one drill-down away in the default view.
- *Why it matters:* whether a drill-down (`kvist status <comp>`) satisfies
  "an actionable next step for every non-current state," or whether the requirement
  demands the *direct* resolver in the default view, is not settled by the
  requirement's wording (it says "an actionable next step," not "the resolving
  command," and does not specify which view must carry it). This is **not
  contradictory evidence**: `kvist status <comp>` is a real, executable next step,
  and the direct resolver is present in the detailed/component view. It is
  retained because a stricter reading of the clause would want the direct resolver
  in the default per-component row, and only the arbitrator can decide that
  granularity.
- *Arbitration options:* (a) accept as compliant (a drill-down next step is
  actionable and the resolver is one step away, in the details view); (b) amend the
  `status` rendering so the compact `Next action` carries the direct component
  resolver for non-current components (implementation change); (c) amend
  `REQ-MARKDOWN-OUTPUT` / the `status` contract clause to state explicitly whether
  the default view must carry the direct resolver or a drill-down suffices (an
  intent change that must be approved, not inferred); (d) record a reasoned
  exception with an owner. This review does not modify the requirement, contract,
  or the implementation.

## Discrepancy summary

| ID | Finding | Type | Status | Severity |
|----|---------|------|--------|----------|
| F-1 | `IMPL.md` has no scoped observation of the Markdown output layer (observation carrier absent/stale) | missing evidence | mismatched | low |
| F-2 | Default compact view gives non-current components a drill-down next action; the direct resolver appears only in the detailed/component view | granularity / underspecified | underspecified | low |

**Total discrepancies requiring arbitration:** 2 (both low; neither is
contradictory evidence of a behavioral defect)
**Satisfied (compliant) clauses:** C-1 … C-15 (15)

## Required arbitration

The project architect owns arbitration and, per `REVIEW_RUNBOOK.md`, may (1)
request redesign/implementation changes, (2) explicitly approve an intent-document
change, or (3) record a reasoned exception or approved deferral. For this review:

1. **No behavioral mismatch was established.** All fifteen clauses of
   `REQ-MARKDOWN-OUTPUT` and the corresponding Provided-interfaces Markdown
   clauses (well-formed document, observed state, document validity, task
   progress, next ready task, actionable next step, structured rendering of
   blocked/awaiting reasons and stale revision details, unchanged native
   `text`/`json` forms, plain-text degradation, TUI `blad` rendering, status
   heading fields, and the compact/details/component-filter routing) are satisfied
   by the source and pinned by `tests/markdown_output.rs` and `tests/status.rs`.
2. **F-1** is the substantive evidence item: promote the Phase 1 observation into
   the component-root `IMPL.md` (the documenter owns that write) and re-run the
   source-blind pass, or record a reasoned exception that the integration suite and
   the Phase 1 derivation are the retained observation. Do not silently edit the
   intent documents to erase the gap.
3. **F-2** is a low-severity granularity question, not a defect: decide whether the
   default (compact) view's per-component next action must be the direct resolver
   or whether a drill-down suffices, and if the former, request the small rendering
   change or an explicit intent-document clarification.
4. No compliance pass may be self-certified by the implementer or the security
   auditor; this record is the independent source-blind disposition for the
   `md-output-compliance-review` gate. Per the current enforcement note,
   `component accept` only structurally validates and records revisions and does
   not enforce this gate.

## Evidence limits (explicit)

- No tests, builds, linters, or `cargo` invocations were executed by this
  reviewer; `tests/markdown_output.rs` and the related suites are run
  automatically by Kvist in the verification sandbox after this turn, and no
  result from that run is asserted here.
- The runbook precondition to run the documented quality gate and `kvist doctor .`
  and resolve any non-`current` state was not executed in this context.
- The nearest-ancestor component contract is `null` (`maerg` is the root
  component); `ROOT_CONTRACT.md` is the only propagated higher-level context.
- The component-root `maerg/IMPL.md` is outside this reviewer's write scope; the
  Phase 1 clean-slate observation is recorded in this file for the documenter to
  promote, and F-1 is retained on that basis rather than closed by a direct edit.
- F-1 rests on a term sweep of `IMPL.md` and F-2 on the `render_summary` source
  plus the integration test; both are independent of the phase-1 source read, so a
  fully isolated source-blind reviewer would reach the same retained items from
  `IMPL.md`, the intent artifacts, and `tests/markdown_output.rs` alone.
- The security-audit stage's report (`tests/security_audit_md_output.md`) is
  present but is not in this reviewer's permitted read set; this review neither
  relies on nor repeats its conclusions.
- All "observed" statements are attributed to the Phase 1 clean-slate source
  derivation, the `tests/markdown_output.rs` / `tests/status.rs` assertions, or
  (for the absence of an observation) `IMPL.md`; this review does not modify
  `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `IMPL.md`, or the implementation.
