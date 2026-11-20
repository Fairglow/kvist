<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Help and Guidance Surfaces (`maerg`)

**Component:** `maerg` (Kvist root component)
**Task:** `help-guidance-compliance-review`
**Scope:** The new guided help (`kvist help [TOPIC]` and the shell `help [TOPIC]`
builtin) and the actionable status guidance (per-task blocked/awaiting-decision
detail lines, invalid-transition legal-move reports, and the `task run` help
text) — compared against `REQ-HELP-GUIDANCE`, the provided-interfaces / behavioral
contract, and the design's "Guided help and actionable status guidance" section.
**Reviewer stance:** Independent compliance reviewer, in two phases per the task
prescription: a clean-slate observation of observed behavior from source and
tests (record, prior reviews, and Git history out of view), then a source-blind
comparison of that derivation against `IMPL.md`, `CONTRACT.md`, and `DESIGN.md`
using `IMPL.md` and the bounded test evidence as the observation carriers. The
implementer and the security auditor did not author this record.

## Review boundary and independence

Per `REVIEW_RUNBOOK.md` *Source-blind compliance pass*, the compliance reviewer
records each requirement clause, contract guarantee, and relevant design claim as
**compliant**, **mismatched**, **approved-deferred**, or **underspecified**, with
component path, stable locator, intended text, observed behavior/evidence,
severity, and owner. The nearest-ancestor component contract is `null` (`maerg`
is the root component); `ROOT_CONTRACT.md` is the only propagated higher-level
context.

**Read (permitted):** `maerg/REQUIREMENTS.md`
(`sha256:c786ec71ab7aa367e6305ae1a715adb6a16c7126410fe72dd760c83d9f3e712e`),
`maerg/CONTRACT.md`
(`sha256:957ce69d562103d49f26cac15691f96f1fe3ab9ab0e43962af6cb89776d9d53a`),
`maerg/DESIGN.md`
(`sha256:bc195225b837d3416c7d9fe9bb154759606bdbd4246dc768687e0349f0acef3f`),
`maerg/IMPL.md`
(`sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`),
`ROOT_CONTRACT.md`
(`sha256:b4dd022afb6237cd20c255a0b9476348e9e76e43bfae4878d4626136be535252`),
`REVIEW_RUNBOOK.md` (procedure), and bounded test evidence: `maerg/tests/help.rs`
(`sha256:2284d482088b24c014056f150baaeac725d6ff52ca0a5fa0c540758253e72fef`),
`maerg/tests/status.rs`
(`sha256:95572c72412ffd5403334af3bbd08f0f04d177d59ca8a48990fc432e7dc623f4`),
`maerg/tests/scoped_status.rs`
(`sha256:6e5e770bdec712fecbd23a012c783f1daa89f954e781503d7d896f5c883eceeb`),
`maerg/tests/task_commands.rs`
(`sha256:b30e17736fe2a3a8565a3c9f3daf8ac0317549f37397532dab17d7498ef4c5af`),
`maerg/tests/task_run_docs_consistency.rs`, and `maerg/tests/shell_smoke.rs`.

**Observation carrier (clean-slate derivation):** This task prescribes two phases.
Phase 1 (clean-slate observation) derived observed behavior by reading the
implementation source and the test suite — `maerg/src/help.rs`, `maerg/src/status.rs`,
`maerg/src/bark.rs` (and its `completion.rs`, `render_tasks`/`next_command_for`
helpers), and `maerg/src/task_commands.rs` (transition validation and the `task
run` target resolution), plus the in-crate `#[cfg(test)]` suites — treating only
the implementation and its tests as the evidence base, with the implementation
record (`IMPL.md`), prior reviews, Git history, and peer artifacts out of view.
Phase 2 (source-blind comparison) compared the implementation record
(`IMPL.md`), `CONTRACT.md`, and `DESIGN.md` against the phase-1 derivation and
the bounded test evidence.

*Context-isolation note (state observation, for the arbitrator):* both phases
were performed in a single session, so this is the analytical separation the
task prescribes (derive from source/tests, then judge against intent) rather than
the runbook's fully isolated separate-agent contexts; the intent texts
(`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`) were part of the session. That
limitation does not weaken the retained findings, because each discrepancy below
(F-1, F-2, F-3) is independently supportable from `IMPL.md`, `CONTRACT.md`,
`DESIGN.md`, and the bounded `tests/` evidence alone — none of them depends on
the phase-1 source read. Where a clause statement does lean on the phase-1 read,
it is attributed as a clean-slate observation and the matching intent and test
carriers are cited alongside it.

**Not read as evidence:** peer component implementations, `sav/`, `galla/`,
`skott/` source, dependency manifests, Git history, and prior compliance
conclusions. The security-audit stage of this chain is completed per the queue,
but its report is not in this reviewer's permitted read set; this review neither
relies on nor repeats its conclusions. No tests, builds, linters, or `cargo`
invocations were executed in this review; the `help`, `status`, `scoped_status`,
`task_commands`, and `task_run_docs_consistency` suites are run automatically by
Kvist in the verification sandbox after this turn, and no pass/fail count from
that run is asserted here.

## Method

Every obligation was drawn from `REQUIREMENTS.md` (`REQ-HELP-GUIDANCE`),
`CONTRACT.md` (`Provided interfaces` command surface, the shell builtins
paragraph, and the status-guidance paragraph; `Markdown-first output and the
interactive TUI` for the `task run` semantics), and `DESIGN.md` ("Guided help and
actionable status guidance"). Each was compared against the observed behavior in
`IMPL.md` and the bounded test evidence. The bounded tests are the executable
statement of observed behavior in the places `IMPL.md` is silent; the in-crate
unit tests are treated as source-side evidence of a surface, not as the
runbook's "bounded test evidence" carrier.

## Satisfied clauses (compliant)

**C-1 — `kvist help` with no topic renders a short tour of the core concepts and
the component lifecycle, listing the closed topic set.**
`REQ-HELP-GUIDANCE` (second paragraph: "`kvist help` rendering a short tour of the
core concepts (project, component, intent documents, tasks) and the component
lifecycle, plus named topics"). `CONTRACT.md` `Provided interfaces` ("`help
[TOPIC]` with TOPIC one of `concepts`, `lifecycle`, `task-states`
(tab-completable); no topic renders the guided tour listing the available topics").
Evidence: `help_without_topic_renders_tour_listing_topics` — the no-topic output
contains `kvist help concepts`, `kvist help lifecycle`, `kvist help task-states`,
`kvist --help`, `GUIDE.md`, `docs/command-set.md`, and `component accept`.
Compliant.

**C-2 — The `concepts` topic explains the four core concepts and the artifact set.**
`REQ-HELP-GUIDANCE` ("named topics: the core concepts"). `CONTRACT.md` `Provided
interfaces` (the `help [TOPIC]` line). Evidence:
`help_concepts_explains_core_concepts` — the topic names `project`, `component`,
`intent`, `tasks`, and all five artifacts (`ROOT_CONTRACT.md`, `REQUIREMENTS.md`,
`CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`) plus `kvist component accept`
and `kvist task run`. Compliant.

**C-3 — The `lifecycle` topic names the exact commands for every stage.**
`REQ-HELP-GUIDANCE` ("the lifecycle with the exact commands"). Evidence:
`help_lifecycle_names_exact_stage_commands` — the topic names `kvist component
new`, `kvist component validate`, `kvist component accept`, `kvist task run`,
`kvist task finalize`, the `STALE` state, `kvist status`, and `kvist help
task-states`. Compliant.

**C-4 — The `task-states` topic names every durable state, the legal transitions,
and the command that achieves each.** `REQ-HELP-GUIDANCE` ("the durable task
states with the legal transitions and the command that achieves each"). Evidence:
`help_task_states_explains_states_transitions_and_commands` — the topic names all
five states (`pending`, `in-progress`, `blocked`, `awaiting-decision`,
`completed`), the `Legal transitions` block with the exact five transition lines
(e.g. `pending -> in-progress | blocked`; `in-progress -> pending | blocked |
completed | awaiting-decision`; `completed -> (none; terminal)`), and the
achieves-each commands (`kvist task run`, `kvist task transition`, `kvist task
finalize`, `kvist task approve-policy`) plus `blocked_reason`. The observed
`TaskStatus::can_transition_to` state machine matches the five transition lines
exactly (ten legal edges; `completed` terminal). Compliant.

**C-5 — Topics are selectable by name on the command line with tab completion, and
the shell `help` builtin accepts the same topics with completion.**
`REQ-HELP-GUIDANCE` ("Topics MUST be selectable by name on the command line with
tab completion, and the shell's `help` builtin MUST accept the same topics with
completion"). `CONTRACT.md` `Provided interfaces` ("Static completion is derived
from that surface (including the help topics as closed value sets for `help
[TOPIC]`)"; shell builtins paragraph: "`help` with no topic lists the builtins and
the available help topics; with a topic it renders that topic, exactly as `kvist
help [TOPIC]` does"). Evidence: `help_command_parses_the_closed_topic_set` (in
`cli.rs`) and the `help.rs` integration tests pin the CLI closed set and the
`help bogus` failure; `help_unknown_topic_reports_the_closed_set` asserts the
unknown-topic error names all three topics. The shell `help [TOPIC]` builtin
renders through the same `render_named` path as the CLI, and the shell topic
completion is pinned by the in-crate `completion.rs` suites
(`help_builtin_offers_the_closed_topic_set`, `help_topic_prefix_filters`);
`tests/shell_smoke.rs` confirms the shell `help` builtin lists the builtins and
the available topics. Compliant. *Note:* the shell's tab-completion facet is pinned
only by the in-crate `completion.rs` unit tests, not by a `tests/` integration
case; see the evidence limits.

**C-6 — Every topic ends with a pointer to the deeper documentation.**
`REQ-HELP-GUIDANCE` ("Every topic MUST end with a pointer to the deeper
documentation (`GUIDE.md`, `docs/command-set.md`, or a component's `CONTRACT.md`)").
Evidence: each of the tour and the three topics references `GUIDE.md` and/or
`docs/command-set.md` (concepts and lifecycle also reference a component's
`CONTRACT.md`); the in-crate `every_topic_points_at_deeper_documentation` asserts
all four surfaces point at deeper documentation. The integration tests independently
assert `GUIDE.md` in the tour, concepts, lifecycle, and task-states outputs.
Compliant.

**C-7 — An invalid task transition reports the legal transitions from the current
state together with a pointer to the task-states help topic.**
`REQ-HELP-GUIDANCE` (third paragraph). `CONTRACT.md` status-guidance paragraph
("Invalid `task transition` failures report the legal transitions from the current
state plus a pointer to `kvist help task-states`"). `DESIGN.md` ("`task
transition` failures (`transition_error`) enumerate the legal target statuses ...
followed by a pointer to `kvist help task-states`"). Evidence: `task_commands.rs`
— an illegal `pending -> completed` move reports `cannot transition task
`implement-code` from pending to completed`, `legal from `pending`: in-progress,
blocked`, and `kvist help task-states`; an illegal `blocked -> awaiting-decision`
move reports `legal from `blocked`: pending, in-progress` and the same pointer, and
the queue is left unchanged. The observed `transition_error` enumerates the closed
state set in deterministic declaration order and appends the pointer. Compliant.

**C-8 — The `task run` help distinguishes exact single-task execution from
omitted-task continuous execution and documents the checkpoint/decision/final-
approval boundaries.** `REQ-HELP-GUIDANCE` (third paragraph). `CONTRACT.md`
`Provided interfaces` (the `task run COMPONENT_DIR [TASK_ID|N]` command-surface
entry; "there is no `all`, prefix selector, or separate whole-component command").
Evidence: the `task run` help text (clean-slate observation; pinned by the in-crate
`task_run_help_documents_continuous_and_exact_task_execution`) states "An exact
TASK_ID runs only that task. Without it, execute ready tasks continuously,
checkpointing complete items until final approval, a decision, or a blocker" — it
distinguishes the exact form from the omitted-task continuous form and documents
the checkpoint/decision/final-approval boundaries, and the in-crate test asserts it
contains "continuously" and "Exact task ID" and does not advertise an item-prefix
selector. Compliant for the requirement as written; the numbered-`N` target form
is not in this help text — see F-3.

**C-9 — Status output for every non-current component state names the concrete
cause and an actionable next command, or (where the next step is a human decision)
a pointer to the help topic that explains the state; blocked components name each
blocked task with its bounded first-line reason and each awaiting-decision task.**
`REQ-HELP-GUIDANCE` (first paragraph). `CONTRACT.md` status-guidance paragraph
("Status output ... names the concrete cause and an actionable next command or the
`kvist help <TOPIC>` that explains a required human decision"; "The shell's `tasks`
table carries the same per-task reason and next-command details"). Evidence:
`status.rs` — for a blocked component the text report shows
`Next Step: 2 blocked task(s) need resolution; 'kvist help task-states' explains
states and transitions`, then per-task `blocked:`/`decision:` entries with the
reason (shown in full: no ellipsis truncation) and the exact next command
(`next: kvist task transition . blocked-ready pending` for the completed-dependency
task; `next: kvist task transition . blocked-waiting pending (1 dependency task
incomplete)` for the incomplete-dependency task; `next: human decision needed;
kvist decision . show paused` for the awaiting-decision task). The same per-task
details render in the `--details` overview. For the other non-current states the
report names the cause and a command: stale → `Next Step: Component intent
documents have changed. Run 'kvist component accept <DIR>'`; invalid → `Next
Step: Run 'kvist repair'`; missing → `Next Step: ... Run 'kvist component new
<DIR>'`; unsupported → `Next Step: ... upgrade the kvist binary ('kvist help
concepts' ...)`. The shell `tasks` table carries the same reason + next-command
pair per blocked/awaiting-decision row (clean-slate observation; in-crate
`render_tasks` suites). Compliant. *Note:* the missing and unsupported-version
`Next Step` lines are provided by the observed implementation but are not
independently pinned by a `tests/` case; see the evidence limits.

## Discrepancies (retained for human arbitration)

**F-1 (MEDIUM, mismatched) — `DESIGN.md` names `kvist task run` as the next command
for a blocked task whose dependency chain is completed, but the observed
behavior, the bounded tests, and `IMPL.md` all use `kvist task transition ...
pending`.**
- *Locator:* `maerg/DESIGN.md` "Guided help and actionable status guidance"
  ("when the dependency chain is completed the command is `kvist task run
  <COMPONENT_DIR> <TASK_ID>` (blocked → in-progress is legal); otherwise it is
  `kvist task transition <COMPONENT_DIR> <TASK_ID> pending` with the count of
  incomplete dependencies"); `maerg/IMPL.md` ("Status, shell and blocked-state
  hints reopen with a reason-free pending transition, rather than suggesting
  execution under blocked component state"); `maerg/tests/status.rs`.
- *Intended (design):* for a blocked task whose dependencies are all completed,
  the status report offers `kvist task run <DIR> <TASK>` (a direct blocked →
  in-progress move); otherwise it offers `kvist task transition <DIR> <TASK>
  pending` with the incomplete-dependency count.
- *Observed:* the guidance entry for a blocked task is `kvist task transition
  <DIR> <TASK> pending` in **both** branches — with the incomplete-dependency count
  when the chain is not complete, and without it when the chain is complete —
  followed by an `or analyze options: kvist decision <DIR> show <TASK>` tail. The
  bounded test `status.rs` pins this directly: for `blocked-ready` (no
  dependencies, i.e. the chain is completed) the next command is `next: kvist task
  transition . blocked-ready pending`, **not** `kvist task run . blocked-ready`;
  the `scoped_status.rs` overview assertions confirm the overview offers no
  `kvist task run` for the blocked case. `IMPL.md` states the hints "reopen with a
  reason-free pending transition, rather than suggesting execution." The
  phase-1 clean-slate read found the shell `next_command_for` helper produces the
  same `pending`-transition command for both branches, and the in-crate
  `render_tasks` suites use a `kvist task run . c` string only as an inert row
  fixture value — the selection logic itself never emits it for a blocked task.
- *Why it matters:* the design's specific command for the completed-dependency
  branch is contradicted by three independent observation carriers. This is a
  documentation-accuracy defect in `DESIGN.md`, not a `REQ-HELP-GUIDANCE` or
  contract violation (the requirement only requires "an actionable next command,"
  which the observed `pending` transition satisfies), but it is a genuine
  design-vs-observation mismatch the runbook requires to be retained rather than
  silently reconciled. Note the observed behavior is internally defensible: a
  blocked task cannot be started by `task run` while its blocker reason is
  recorded, so reopening to `pending` (then running) is the legal path; the
  design's `task run` claim appears stale relative to that rationale.
- *Arbitration options:* (a) correct the `DESIGN.md` sentence so both branches
  describe `kvist task transition <DIR> <TASK> pending` (with the count when
  incomplete), matching `IMPL.md` and the tests; (b) if a design intent existed to
  offer `kvist task run` for completed-dependency blocked tasks, commission the
  implementation and tests for that branch; (c) record a reasoned exception that
  the design sentence is superseded by the observed `pending`-transition behavior.
  This review does not modify `DESIGN.md`.

**F-2 (LOW, mismatched) — `IMPL.md` carries no scoped observation of the new help
and guidance surface, so the runbook's primary observation carrier is absent for
this change.**
- *Locator:* `maerg/IMPL.md`
  (`sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`);
  `ROOT_CONTRACT.md` `Change and compliance rules` ("`IMPL.md` describes observed
  implementation behavior and is not copied from intended requirements, contracts,
  or designs"); `REVIEW_RUNBOOK.md` (the compliance reviewer reads "the newly
  produced `IMPL.md`").
- *Intended (declared):* `IMPL.md` states public behavior, guarantees, limits,
  state effects, failure paths, and known uncertainty, derived independently from
  source; the source-blind pass compares intent against that fresh observation.
- *Observed:* `IMPL.md` contains no heading, addendum, or scoped observation of
  the guided help tour, the `concepts`/`lifecycle`/`task-states` topics, the
  closed topic set, the per-task blocked/awaiting-decision detail lines, the
  invalid-transition legal-move reports, or the `task run` help text. A term sweep
  of `IMPL.md` for `help topic`, `task-states`, `HelpTopic`, `concepts`, `guided
  help`, and `blocked_reason` found no help-guidance observation; the only
  adjacent line is the incidental note in the transition-fence section ("Status,
  shell and blocked-state hints reopen with a reason-free pending transition;
  Help no longer describes verification alone as completion"), which is part of an
  unrelated repair observation and is not a scoped observation of this change.
- *Why it matters:* with the observed-behavior carrier absent, the source-blind
  comparison for C-1…C-9 degrades to bounded-tests-only for most clauses, and the
  runbook's expectation that the compliance pass compares intent against a freshly
  derived `IMPL.md` is not met for this change. The bounded tests cover the help
  topic set, the per-task detail lines, the legal-move reports, and the `task
  run` help text, but the shell topic completion and the missing/unsupported
  `Next Step` lines are not pinned by `tests/` (see C-5, C-9) and are not carried
  by `IMPL.md`.
- *Arbitration options:* (a) commission a clean-slate documenter pass over the
  help/guidance change so `IMPL.md` carries the observed behavior (the closed
  topic set and per-topic pointers, the shared CLI/shell renderer, the per-task
  reason + next-command derivation, the legal-move enumeration, the `task run`
  help forms), then re-run this compliance pass against it; (b) record a reasoned
  exception that the bounded `tests/help.rs` / `tests/status.rs` /
  `tests/task_commands.rs` suites are the retained observation for this chain;
  (c) approved deferral with an owner. This review does not modify `IMPL.md`.

**F-3 (LOW, underspecified in the help text) — The `task run` help text documents
the exact-`TASK_ID` and omitted-task (continuous) forms but not the bare-`N`
numbered-ready-task form that the contract command surface and the `lifecycle`
topic advertise.**
- *Locator:* `CONTRACT.md` `Provided interfaces` (the `task run COMPONENT_DIR
[TASK_ID|N]` entry: "A bare positive integer N runs the Nth ready task (1-based,
the same ordering the `tasks` listing numbers)"); the `lifecycle` help topic
(clean-slate observation: "`run N` (or `task run DIR N`) executes the Nth ready
task; an exact task ID always wins over a number"); `maerg/tests/task_run_docs_consistency.rs`.
- *Intended (declared):* the `task run` target is optional and a bare positive
  integer `N` runs the Nth ready task (surface `[TASK_ID|N]`); the help text should
  document the task specifications it offers.
- *Observed:* the `task run` help text (clean-slate observation; pinned by the
  in-crate `task_run_help_documents_continuous_and_exact_task_execution`, which
  asserts it names the exact-ID and continuous forms and does **not** advertise an
  item-prefix selector) documents "An exact TASK_ID runs only that task. Without
  it, execute ready tasks continuously ..." — it does not mention the bare-`N`
  form. The `N` form is nonetheless documented on the contracted surface (the
  `README` CLI table and the `GUIDE` task-run examples, per
  `task_run_docs_consistency.rs`) and in the `lifecycle` help topic, and the
  implementation resolves it (`resolve_task_spec`: exact ID wins, else a 1-based
  ready-task position).
- *Why it matters:* this is a documentation-coverage gap in one help surface, not
  a `REQ-HELP-GUIDANCE` violation (C-8's requirement — distinguish exact from
  continuous and document the checkpoint/decision/final-approval boundaries — is
  met) and not a contract violation (the surface does accept `N`). A reader of
  `task run --help` alone would not learn the numbered shortcut, though the
  `lifecycle` topic and the docs do. Retained so the arbitrator can decide whether
  the help text should name all three target forms.
- *Arbitration options:* (a) add the bare-`N` form to the `task run` help text so
  it matches the contracted `[TASK_ID|N]` surface and the `lifecycle` topic; (b)
  record a reasoned exception that the exact-ID/continuous distinction is the
  retained `task run` help scope and the `N` form is covered by the `lifecycle`
  topic and the user docs; (c) approved deferral with an owner. This review does
  not modify the help text.

## Discrepancy summary

| ID | Finding | Status | Severity |
|----|---------|--------|----------|
| F-1 | `DESIGN.md` blocked-branch command (`task run`) contradicts observed/tests/`IMPL.md` (`task transition pending`) | mismatched | medium |
| F-2 | `IMPL.md` has no scoped observation of the help/guidance surface (observation carrier absent) | mismatched | low |
| F-3 | `task run` help text omits the bare-`N` numbered-ready-task form advertised by the contract and `lifecycle` topic | underspecified | low |

**Total discrepancies requiring arbitration:** 3
**Satisfied (compliant) clauses:** C-1 … C-9 (9)

## Required arbitration

The project architect owns arbitration and, per `REVIEW_RUNBOOK.md`, may (1)
request redesign/implementation changes, (2) explicitly approve an intent-document
change, or (3) record a reasoned exception or approved deferral. For this review:

1. **F-1** is the substantive item. The `REQ-HELP-GUIDANCE` and contract
   obligations are met and the bounded tests pass; the defect is that
   `DESIGN.md` names `kvist task run` for the completed-dependency blocked branch
   while the implementation, the bounded tests, and `IMPL.md` use `kvist task
   transition ... pending`. Recommended: approve an intent-document (design)
   correction so `DESIGN.md` matches the observed `pending`-transition behavior.
   Do not silently edit `DESIGN.md` to erase the mismatch.
2. **F-2** and **F-3** are low-severity documentation/evidence gaps. Each can be
   closed by a clean-slate `IMPL.md` observation pass (F-2) and/or a one-line
   `task run` help-text addition (F-3), or recorded as reasoned exceptions.
3. No compliance pass may be self-certified by the implementer or the security
   auditor; this record is the independent source-blind disposition for the
   `help-guidance-compliance-review` gate. Per the current enforcement note,
   `component accept` only structurally validates and records revisions and does
   not enforce this gate.

## Evidence limits (explicit)

- No tests, builds, linters, or `cargo` invocations were executed by this
  reviewer; the `help`, `status`, `scoped_status`, `task_commands`, and
  `task_run_docs_consistency` suites are run automatically by Kvist in the
  verification sandbox after this turn, and no result from that run is asserted
  here.
- The runbook precondition to run the documented quality gate and `kvist doctor
  .` and resolve any non-`current` state was not executed in this context.
- The nearest-ancestor component contract is `null` (`maerg` is the root
  component); `ROOT_CONTRACT.md` is the only propagated higher-level context.
- The security-audit stage of this chain is completed per the queue, but its
  report is not in this reviewer's permitted read set; this review neither relies
  on nor repeats its conclusions.
- The shell tab-completion facet (C-5) and the `task run` help-text content (C-8,
  F-3) are pinned by in-crate `#[cfg(test)]` suites, which are source-side
  evidence, not the runbook's bounded `tests/` carrier; a `tests/shell_smoke.rs`
  case confirms the shell `help` builtin lists the builtins and the available
  topics, and `tests/task_run_docs_consistency.rs` confirms the contracted
  `[TASK_ID|N]` surface in the user docs.
- The `Next Step` lines for the `missing` and `unsupported-version` component
  states (C-9) are provided by the observed implementation but are not
  independently pinned by a `tests/` case in the set read here.
- All "observed" statements are attributed to the bounded test evidence, to
  `IMPL.md`, or to the clean-slate source derivation described above; a fully
  isolated source-blind reviewer would rely on `IMPL.md` plus the bounded tests,
  and the discrepancies F-1, F-2, and F-3 are each independently supportable from
  those carriers and the intent artifacts.
