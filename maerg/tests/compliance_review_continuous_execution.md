<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Continuous Execution / Whole-Component Run
# (`maerg`)

**Component:** `maerg`
**Task:** `continuous-run-compliance-review` — compare continuous
execution against intent.
**Compared artifacts:**

- `maerg/REQUIREMENTS.md` §`REQ-CONTINUOUS-EXECUTION`
- `maerg/REQUIREMENTS.md` §`REQ-RUN-COMPONENT`
- `maerg/CONTRACT.md` §`Provided interfaces` (command surface, context
  resolution, continuous execution, checkpoint/ledger, independent review
  framing)
- `ROOT_CONTRACT.md` §"Change and compliance rules"
- `maerg/IMPL.md` "Scoped continuous-execution observations - 2026-10-30"
  and the retained clean-slate records it points to under
  `docs/reviews/` (listed in the evidence section)
- bounded test evidence (listed below)

**Reviewer stance:** Independent compliance reviewer for the
`continuous-run` chain (tests → implementation → security audit →
compliance review). This record is the disposition for the
`continuous-run-compliance-review` gate. The implementer and the
security auditor did not author it; it is not self-certified, and it
is not acceptance.

## Review boundary and independence

This is the terminal, independent stage of the `continuous-run` chain.
Per the runbook, the source-blind compliance reviewer compares the
intended artifacts with the implementation observation and bounded
test evidence; the implementer must not certify the work.

**Independence that is preserved:** the implementer (a separate
authoring role) and the security auditor (`continuous-run-security-audit`,
recorded in `tests/security_audit_continuous_execution.md`) are
distinct contexts from this reviewer. The security audit's conclusions
are not relied upon and are not repeated here.

**Independence that is NOT preserved, disclosed:** the runbook ideal
splits the pass into a clean-slate source-only *documenter* and a
*source-blind* *comparator*. This gate was performed in a single
context that (a) relied on the retained clean-slate source-only
observation records for the bulk of the observation, and (b)
additionally re-verified, against the current `maerg/src`
continuous-execution and checkpoint paths, only the points that bear
directly on `REQ-CONTINUOUS-EXECUTION` and `REQ-RUN-COMPONENT`.
The reason for the disclosed source re-verification is that the
retained observation (`continuous-execution-observation.md`, 2026-10-30)
predates two current changes: the deadline/cancellation-bounded
checkpoint Git supervision (`checkpoint/process.rs`) and the TUI
cross-component continuous path (`bark/tui/workspace/*`). Every
"observed" statement below is attributed to one of exactly three
sources, in this order of preference:

1. the retained clean-slate observation records;
2. the bounded test evidence (assertions, not fresh runs); or
3. this reviewer's disclosed re-verification of current source.

No clause is certified from a prior compliance conclusion. The prior
`docs/reviews/continuous-execution-comparison.md` retained discrepancy
(group-wide reaping) is re-examined below on current evidence and is
not rubber-stamped.

**Not relied on as evidence:** peer or parent implementation
artifacts, Git history/diffs, and prior compliance reports beyond the
continuous-execution records named above. The security-audit report of
this chain (`tests/security_audit_continuous_execution.md`) was read
only to understand the chain's provenance and scope; no control is
asserted from it — each control above is re-observed from the
retained observation, the bounded test evidence, or the disclosed
source re-verification.

## Bounded evidence relied upon

Retained clean-slate observation records (source-only; no intent):

- `docs/reviews/continuous-execution-observation.md`
- `docs/reviews/continuous-execution-corrections-observation.md`
- `docs/reviews/continuous-redaction-predicate-observation.md`
- `maerg/IMPL.md`, "Scoped continuous-execution observations - 2026-10-30"

Bounded test evidence (assertions inspected; the suites are run
automatically by Kvist in the verification sandbox after this turn, so
no pass/fail count from that run is asserted here):

- `maerg/tests/task_commands.rs`:
  `continuous_run_completes_two_items_with_separate_unaccepted_checkpoints`,
  `continuous_run_new_work_uses_a_fresh_baseline_and_approved_commit_policy`,
  `continuous_run_commit_policy_and_overlap_refusal`,
  `continuous_run_pauses_on_review_decision_without_running_later_items`,
  `continuous_run_retries_failed_checkpoint_without_replaying_completed_tasks`,
  `continuous_run_explicit_selection_is_single_and_prerequisite_checked`,
  `continuous_run_batches_large_independent_review_inputs_without_dropping_evidence`,
  `continuous_run_rejects_malformed_reviews_and_forged_ledger_state`,
  `continuous_run_interrupt_cancels_quiet_inference_without_scheduling_later_tasks`
- `maerg/tests/task_run_reliability.rs`:
  `lock_released_after_successful_run`,
  `completion_message_includes_full_answer`,
  `interrupted_run_is_recoverable_with_diagnostic`
- `maerg/tests/shell_smoke.rs`: editable component prefill round-trips
  (dash-leading and absolute), the `task run .` after-prefill
  policy-gate refusal, and the missing-policy-approval gate
- `maerg/src/bark.rs` in-crate test
  `parse_run_args_requires_component_and_accepts_optional_task`
- `maerg/src/bark/tui/workspace/menu_tests.rs`:
  `task_run_without_component_searches_all_components_for_ready_task`
  (pins the cross-component continuous path — see D-1)

## Method

Each obligation in `REQ-CONTINUOUS-EXECUTION`, `REQ-RUN-COMPONENT`, and
the `Provided interfaces` continuous-execution/contract clauses was
mapped to observed behavior and test evidence, then classified as
`compliant`, `mismatched`, `approved-deferred`, or `underspecified`.
Discrepancies are retained for explicit human arbitration; no intent
or implementation artifact was modified to erase one.

## Findings

### F-1 Whole-component iteration in dependency order — compliant

- **Locator:** REQ-RUN-COMPONENT ("MUST iterate over the component's
  tasks in dependency order, running each ready task");
  REQ-CONTINUOUS-EXECUTION ("MUST execute dependency-ready tasks
  iteratively").
- **Observed:** `continuous.rs::run_locked` holds one component
  operation lock, re-reads the queue each pass, and selects the first
  declared pending task whose direct and transitive dependencies are
  completed (`task_commands.rs::task_is_ready`,
  `transitive_dependencies_completed`). It loops until the queue is
  all-completed, a decision/blocker occurs, or interruption is
  detected. No task-count ceiling is present.
- **Evidence:** `continuous_run_completes_two_items_with_separate_unaccepted_checkpoints`
  (two items run to completion, output contains `awaiting`/`approval`,
  no `Run suggested` confirmation), `continuous_run_pauses_on_review_decision...`
  (later tasks left `Pending`).

### F-2 Explicit single-task selection with prerequisite checks —
compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("An explicit task ID MUST
  execute only that task and retain prerequisite checks").
- **Observed:** `task_commands.rs::resolve_task_spec` resolves an exact
  ID first, then a bare positive integer as a 1-based position in the
  ready-task list; `run_task_or_item_with_controls` with a `Some(spec)`
  runs exactly that task through the same locked, policy-approved path.
  Prerequisite readiness is enforced by `task_is_ready`.
- **Evidence:** `continuous_run_explicit_selection_is_single_and_prerequisite_checked`
  (one task completes, its dependents stay `Pending`, a not-ready task
  is denied with `not ready`, and no `.kvist/continuous.json` ledger is
  created for a single explicit run).

### F-3 No `all`, prefix, or `run-all` batch interface (headless) —
compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("`all`, item-prefix execution
  and `run-all` MUST NOT be alternate batch interfaces"); CONTRACT
  command surface ("There is no `all`, prefix selector, or separate
  whole-component command").
- **Observed:** No `run-all` subcommand and no `all`/prefix selector
  exist in the CLI; `resolve_task_spec` accepts only an exact ID or a
  bare position. The shell `run` builtin requires a component argument
  (`bark.rs::parse_run_args` errors on zero or three+ arguments).
- **Evidence:** `parse_run_args_requires_component_and_accepts_optional_task`;
  absence of any `run-all`/`all` selector in the parsed command set.
- **Caveat:** this holds for the headless CLI and the `run` builtin.
  The interactive TUI exposes a cross-component continuous path that
  is a `run-all`-shaped interface. See D-1.

### F-4 Component argument required (headless and `run` builtin) —
compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("The component argument MUST
  remain required ... neither focus nor cwd may supply it implicitly");
  CONTRACT context resolution ("`task run` ... its editable shell
  prefill never replaces that required argument with an implicit
  focus/cwd fallback").
- **Observed:** `context.rs::resolve_component` returns
  `ComponentNotResolvable` when no explicit argument is given and the
  cwd is not inside a discovered component, so a bare headless
  `task run` fails rather than guessing. `bark.rs::parse_run_args`
  requires the component argument for the `run` builtin.
- **Evidence:** `parse_run_args_requires_component_and_accepts_optional_task`
  (the shell `run` builtin errors without a component);
  `resolve_component`'s explicit-argument-or-containment rule
  (disclosed re-verification). The `shell_smoke.rs` fixture submits
  `task run .` (a bare `task run` after the prefill inserts the
  focused root component) and pins that it is refused by the
  missing-policy-approval gate with no `[Y/n]` confirmation, no
  `Run suggested`, and the queue unchanged; it does not pin a
  missing-argument error on the TUI.
- **Caveat:** in the interactive TUI, `task run` with no component (or
  `.`) enters an all-components mode. See D-1.

### F-5 Editable shell prefill: explicit wins, deletion remembered —
compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("including after an editable
  shell prefill has been deleted").
- **Observed:** `bark/prefill.rs::available` marks a recalled or
  manually supplied value as deliberate (`position.supplied`) so
  removing it does not re-activate prefill on the next repaint;
  `insertion` fills only a genuinely empty argument and never completes
  a partial token; quoting and dash-leading paths are handled.
- **Evidence:** `shell_smoke.rs` prefill round-trips (dash-leading and
  absolute child components reach the missing-policy-approval gate
  without task effects); the in-crate prefill tests pin that a recalled
  or manually supplied value stays deliberate after deletion and never
  completes a partial token.

### F-6 Execution completion is distinct from human acceptance —
compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Successful execution MUST be
  distinct from human acceptance; default runs MUST not invoke human
  acceptance automatically"); CONTRACT ("Successful continuous
  implementation execution ... completes execution, not acceptance").
- **Observed:** `continuous.rs::record_final_state` writes
  `.kvist/continuous-reviews/component.json` with
  `state: "awaiting-approval"` and `human_acceptance: false` plus a
  notice that execution and checkpoint commits are not human
  acceptance. The run loop never calls `component accept` or mints an
  acceptance ID. Continuous implementation tasks transition to
  `Completed` without human finalization; an explicitly selected
  implementation task instead remains pending human finalization.
- **Evidence:** `continuous_run_completes_two_items...` (all
  `acceptance_id` are `None`, final `state` is `awaiting-approval`,
  `human_acceptance` is `false`), `task_run_reliability.rs`
  `completion_message_includes_full_answer`.

### F-7 Completed component durably awaits final human approval —
compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Completed component execution
  MUST durably await final human approval").
- **Observed:** the ledger `Phase::AwaitingApproval` and the durable
  `component.json`/`component.md` under `.kvist/continuous-reviews/`
  are written before the loop returns; the run reports the review
  index and ledger paths.
- **Evidence:** `continuous_run_completes_two_items...` (final
  `component.json` state/digests asserted).

### F-8 Fresh approved run/baseline after terminal execution —
compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Explicitly scheduling new
  work after that terminal execution state MUST permit a fresh
  approved run/baseline without replaying or accepting previous work").
- **Observed:** a retained `AwaitingApproval` ledger with no pending
  checkpoint may be replaced only when the authored task definitions
  change and unfinished work exists, under a fresh approved commit
  policy; prior completed task statuses are retained rather than
  replayed.
- **Evidence:** `continuous_run_new_work_uses_a_fresh_baseline_and_approved_commit_policy`
  (newly appended pending tasks run to completion under a changed,
  re-approved commit policy; no replay of prior work; all
  `acceptance_id` `None`).

### F-9 Item lifecycle convention and per-item checkpoints — compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION (item = common prefix before
  `write-tests`/`implement-code`/`security-audit`/`compliance-review`;
  nested implementation prefixes belong to the enclosing test
  lifecycle; historical tasks must not block; item checkpoints wait
  for all defined item tasks); CONTRACT (same).
- **Observed:** `continuous.rs::item_groups`/`item_stem` derive items
  from the exact lifecycle suffixes, resolve matching prefixes
  longest-first, assign implementation subtasks to the enclosing test
  stem, and skip already-completed tasks that have no inferable stem.
  Interleaved active items are rejected. Item commits follow
  completion of every inferred member.
- **Evidence:** `continuous_run_completes_two_items...` (two separate
  unaccepted item checkpoints, one commit per item).

### F-10 Commit frequency: item (default), component, manual;
approval-bound — compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Configurable local commit
  frequency MUST default to per-item, permit per-component or manual
  commits, and be approval-bound"); CONTRACT
  (`[task_run] commit = "item"` default).
- **Observed:** `config.rs::CommitFrequency` has `Item` as `#[default]`,
  `Component`, and `Manual`; the policy is part of the authenticated
  execution-approval material, so changing it requires re-approval.
  Manual skips Git cleanliness preflight and automatic commits but
  still inventories owned files.
- **Evidence:** `continuous_run_commit_policy_and_overlap_refusal`
  (component → 2 commits, manual → 1 commit);
  `continuous_run_new_work_uses_a_fresh_baseline...` (policy change
  requires `task approve-policy`).

### F-11 Checkpoint commits are scoped, unaccepted, and sign-policy
honoring — compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Checkpoint commits MUST NOT
  imply acceptance, include unrelated changes, push, amend, or bypass
  signing policy"); CONTRACT.
- **Observed:** `checkpoint.rs` captures only owned paths
  (`src/**`, `tests/**`, `TODOS.yaml`, `IMPL.md`,
  `.kvist/continuous-reviews/**`); a prepared commit is built in an
  isolated index, message-stamped `(unaccepted)`, and ref-advanced with
  a compare-and-swap `update-ref` transaction. Signing is honored
  (`commit.gpgsign` observed). Unrelated staged files are preserved.
- **Evidence:** `continuous_run_completes_two_items...` (the staged
  `unrelated.txt` remains in `git diff --cached` after the run; commit
  messages are the two item checkpoints);
  `continuous_run_commit_policy_and_overlap_refusal`.

### F-12 Dirty overlap, concurrent change, and failed commits stop
clearly and remain resumable — compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Dirty overlapping paths,
  concurrent changes and failed commits MUST stop clearly; retained
  pending commits MUST be resumable without rerunning successful
  tasks"); CONTRACT.
- **Observed:** `checkpoint.rs::ensure_clean`/`prepare`/`finish`
  re-validate the worktree, index, branch, and lock before and after
  publication; a dirty overlapping owned path is refused; a failed
  checkpoint retains the prepared commit, and repeating the run
  retries it without rerunning completed tasks.
- **Evidence:** `continuous_run_commit_policy_and_overlap_refusal`
  (dirty → `dirty` diagnostic, queue and file unchanged);
  `continuous_run_retries_failed_checkpoint_without_replaying_completed_tasks`
  (a signing failure produces a blocker brief naming
  `without rerunning completed tasks`; the retry advances only the
  remaining item).

### F-13 Crash recovery does not infer unknown effects are safe —
compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Crash recovery MUST NOT infer
  that unknown effects are safe").
- **Observed:** `run_locked` calls `ensure_component_attempts_recovered`
  before starting and refuses while an unresolved fenced attempt or
  pending decision exists; the ledger is HMAC-authenticated and
  version-checked (`continuous.rs::load`), so a forged or stale ledger
  is refused without deleting recovery evidence.
- **Evidence:** `continuous_run_rejects_malformed_reviews_and_forged_ledger_state`
  (a forged `awaiting-approval` phase fails the next run with
  authentication failure and leaves the queue unchanged).

### F-14 Checkpoint Git/signing: bounded capture, deadline, and
cancellation — compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Checkpoint Git/signing
  commands MUST have bounded capture and deadlines and honor
  cancellation, rather than hanging unattended runs"); CONTRACT
  ("five-minute per-command deadline and honor cancellation").
- **Observed (disclosed re-verification):** `checkpoint/process.rs`
  runs every Git command with `COMMAND_TIMEOUT` of five minutes;
  `Control::check` inspects the deadline and
  `sav::take_interrupted` on each I/O tick, and a breach reports that
  the ref mutation may be unknown and the retained checkpoint and
  locks must be inspected before retrying. This closes the prior
  observation's "no timeout or cancellation checks" gap.
- **Evidence:** `continuous_run_retries_failed_checkpoint...` (bounded
  signing failure path); `checkpoint/process.rs` supervision and the
  in-crate `checkpoint/tests.rs` suite.
- **Residual:** see D-2 for the group-wide-reaping wording gap.

### F-15 Security review uses a fresh source-only context — compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Security task observations
  MUST use a fresh source-only context"); CONTRACT.
- **Observed:** `continuous.rs::execute_review` issues a
  `source-only-security` request to a fresh, tool-free
  `ReviewSession`; `source_evidence` supplies only `src/` and `tests/`
  (bounded, non-link) and explicitly excludes `REQUIREMENTS.md`,
  `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`,
  `ROOT_CONTRACT.md`, `ARCHITECTURE.md`, and `VISION.md`.
- **Evidence:** `continuous_run_completes_two_items...` (source-only
  requests contain the observed marker and no intent text).

### F-16 Compliance review: source-only observation, then separate
source-blind comparison, no authoring tools — compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Compliance task work MUST
  derive source-only observations in one fresh request and compare
  them against intent in a separate source-blind request, without
  authoring tools").
- **Observed:** `continuous.rs::execute_review` for a compliance task
  first issues `source-only-observation`, persists an attempt-specific
  observation and replaces `IMPL.md` with that record, then issues a
  separate `independent-comparison` request whose frame is intent +
  observed/test evidence (no raw source field). The `ReviewSession` is
  tool-free; advisory decisions and independent compliance are forced
  into separate model sessions.
- **Evidence:** `continuous_run_completes_two_items...` (two
  `source-only-observation` and two `independent-comparison`
  requests; comparison requests do not contain source text);
  `continuous_run_pauses_on_review_decision...`.

### F-17 Host-written observations/reports durable; decisions pause;
report existence is not acceptance — compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Host-written observations/reports
  MUST be durable; decisions MUST pause work, and report existence MUST
  NOT itself be treated as acceptance").
- **Observed:** observations and per-attempt/final reports are written
  under `.kvist/continuous-reviews/`; every report carries
  `human_acceptance: false`. A surfaced decision sets
  `awaiting-decision`, pauses the run, and leaves later tasks
  `Pending`.
- **Evidence:** `continuous_run_pauses_on_review_decision_without_running_later_items`
  (task 3 `AwaitingDecision`, tasks 4+ `Pending`, a Markdown decision
  brief names `kvist decision . options --task ...` and
  `kvist task run . one-compliance-review`, and a re-run refuses to
  proceed).

### F-18 Large review evidence in bounded fragments, phase-preserving,
shared budget — compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Large review evidence MUST be
  processed in bounded fresh-context fragments, with complete fragment
  coverage, phase-preserving synthesis and shared task wall/token
  accounting"); CONTRACT (UTF-8 fragmentation, shared observed/test
  frame, provisional 32,768-token capacity).
- **Observed:** `agent/review.rs::request` fragments oversized evidence
  at UTF-8 boundaries (`split_utf8`), retains the shared observed/test
  frame in every comparison fragment, synthesizes hierarchically with
  OR-preserved decision flags, and fails if bounded reduction makes no
  progress (nothing is silently dropped). All requests, retries, and
  both phases share one task budget.
- **Evidence:** `continuous_run_batches_large_independent_review_inputs_without_dropping_evidence`.

### F-19 Fragment sizing uses configured capacity else a conservative
provisional — compliant

- **Locator:** REQ-CONTINUOUS-EXECUTION ("Fragment sizing MUST use
  configured model capacity when available, retaining a conservative
  provisional capacity otherwise"); CONTRACT.
- **Observed:** `agent/review.rs::request` uses the configured
  `model.context_limit` when present, otherwise a provisional
  32,768-token budget; an advisory decision path with unconfigured
  capacity serves bounded complete input (8 MiB) without asserting a
  server limit.
- **Evidence:** `continuous_run_batches_large_independent_review_inputs_without_dropping_evidence`;
  `agent/review.rs` capacity handling and in-crate tests.

### F-20 Whole-component stop conditions and exact resolving command —
compliant

- **Locator:** REQ-RUN-COMPONENT ("MUST stop when the component is
  complete, when a task becomes blocked, or when a task reaches an
  awaiting-decision point. It MUST NOT auto-resolve a decision or
  unblock a blocked task; it MUST pause and report the exact state and
  the command that resolves it ... MUST leave durable task state").
- **Observed:** the loop stops on all-complete (AwaitingApproval), on a
  decision (AwaitingDecision, with a Markdown brief naming the
  resolving commands), and on no-ready-work (Paused, listing the
  waiting tasks and their reasons, directing the operator to
  `kvist task next <component>` and the retained attempt evidence, and
  stating no task was unblocked automatically). The durable ledger and
  attention brief are written on each stop.
- **Evidence:** `continuous_run_pauses_on_review_decision_without_running_later_items`;
  `continuous_run_interrupt_cancels_quiet_inference_without_scheduling_later_tasks`
  (SIGINT → task `Blocked`, later tasks `Pending`, run fails);
  the no-ready-work diagnostic in `continuous.rs::run_locked`.

## Discrepancies for human arbitration

### D-1 — TUI all-components continuous mode and `.` wildcard (High)

**Intended (REQ-CONTINUOUS-EXECUTION):** "The component argument MUST
remain required ... neither focus nor cwd may supply it implicitly."
and "`all`, item-prefix execution and `run-all` MUST NOT be alternate
batch interfaces." **REQ-RUN-COMPONENT:** "Bare `task run COMPONENT`
MUST run the whole selected component" (a single component). **CONTRACT
command surface:** "`task run COMPONENT_DIR [TASK_ID|N]` ... Without a
target, execute ready tasks continuously in queue order ... There is
no `all`, prefix selector, or separate whole-component command."

**Observed (disclosed re-verification of current source):** the
interactive TUI workspace does not treat the component argument as
required, and provides a cross-component (`run-all`) continuous
interface:

- `src/bark/tui/workspace/actions.rs` (dispatch):
  `let all_components = component_dir.is_none()
  || component_dir == Path::new(".")` — so `task run` with no
  component **and** `task run .` both enter all-components mode, each
  seeded from the focused component (`self.focus()`).
- In all-components mode, when the focused component has no ready
  task, the dispatch searches across all `Current` components and
  starts a run in the first one that has ready work
  (`actions.rs`, "No task found in the target component ... search
  across all components").
- `actions.rs::try_next_component` walks all `Current` components after
  a component's tasks finish and starts a task in the next one that
  has ready work ("Moving to component ..."), and `workspace.rs`
  (pump) calls it whenever `task_run_all_components` is set.
- `src/bark/tui/workspace.rs` documents the field: "the continuous
  task run should proceed across all components (no specific component
  was requested)."
- `src/cli.rs` `TaskCommand::Run.component_dir` is `Option<PathBuf>`
  with the help text "Explicit component directory; **omit to run
  across all components continuously**," which contradicts the
  contract's required `COMPONENT_DIR` positional and the "there is no
  `all` ... whole-component command" clause.
- This cross-component path drives per-task `prepare_task_run` and the
  embedded session directly; it does not use the durable
  `.kvist/continuous.json` ledger, the item-checkpoint machinery, the
  definition-digest guard, or the awaiting-approval terminal state that
  the headless `continuous::run` path provides.

**Test evidence:** `menu_tests.rs::task_run_without_component_searches_all_components_for_ready_task`
explicitly pins this behavior, with a comment that `task run` (no
component) was changed to "search across all components in continuous
mode" as a regression fix.

**Assessment:** `mismatched` (High). The headless CLI and the `run`
builtin enforce the required component argument (F-4), but the
interactive TUI does not: it seeds from focus/cwd, treats `.` as a
wildcard, and spreads execution across components without naming them.
That is precisely the "run-all" batch interface the requirement forbids
and an implicit focus/cwd supply the requirement prohibits. It also
makes the same input (`task run .`) behave differently in the TUI
(all components, seeded from focus) than in the headless CLI
(`resolve_component` maps `.` to a single discovered component and
never iterates across components), and the CLI help advertises the
forbidden behavior.

**Arbitration options:**
1. The intent stands: remove the TUI all-components continuous path,
   the `.`-wildcard branch, the `try_next_component` spread, and the
   `omit to run across all components continuously` help text, so the
   component argument is required and no run-all interface exists on
   any surface; route the TUI `task run` through the same
   single-component, ledger-backed continuous path.
2. A cross-component run-all is actually desired: amend
   `REQ-CONTINUOUS-EXECUTION`/`REQ-RUN-COMPONENT` and the CONTRACT
   command surface to permit it, define its checkpoint/ledger/terminal
   semantics, and reconcile the `.` (root component) vs. all-components
   meaning — rather than leaving the headless and TUI surfaces
   inconsistent.

Either way, the two artifacts must be made consistent; this reviewer
changed neither.

### D-2 — "kills and reaps that owned group" vs. group kill +
direct-child reap (Low; retained from the prior comparison)

**Intended (DESIGN.md checkpoint section):** "deadline/cancellation
kills and reaps that owned group, unblocks transaction pipes, and
retains exact prepared/ref/index recovery state."

**Observed (disclosed re-verification of current source):**
`checkpoint/process.rs::reap` sends `killpg(group, SIGKILL)` to the
isolated process group (including helpers that retained a pipe
descriptor after the Git leader exits) and then explicitly waits for
the direct child only (`child.wait()`); group members other than the
direct child are killed by the group signal but not individually
reaped. The deadline and cancellation checks (F-14) are now present,
which closes the prior observation's "no timeout or cancellation"
gap.

**Assessment:** `mismatched` (Low, wording/guarantee). The current
code kills the group and reaps the direct child, but the design's
"reaps that owned group" is a stronger guarantee than the code states.
This does not prove a surviving live helper or a runtime hang; the
residual is whether non-leader descendants can be left as unreaped
zombies in an edge case. Retained from the prior
`continuous-execution-comparison.md` and still present in current
source.

**Arbitration options:** (a) tighten the design wording to
"sends SIGKILL to the owned group and reaps the direct child," or
(b) extend supervision to explicitly reap group members and pin it with
a test.

## Coverage gaps and limitations — not established defects

- No protected native-runner execution of this chain's security or
  compliance tasks is evidenced here; no protected receipt or final
  acceptance is implied. Mock responses and source assertions cannot
  substitute for that missing evidence.
- The retained observation's host-attributed counts (e.g., the 521
  library / 65 `task_commands` tests) are attributed to the host and
  are not independently re-run by this reviewer.
- The cross-component TUI path is exercised by an in-crate unit test
  that asserts the search *would* select another component; no test
  drives an actual multi-component run through the embedded session, so
  its terminal/lock behavior across components is not pinned.
- Mock model responses and structural validation do not establish
  truthful or complete findings, semantic synthesis coverage, or
  recovery from every abrupt-crash interval.

## Discrepancy summary

| ID | Finding | Status | Severity |
|----|---------|--------|----------|
| F-1..F-20 | Whole-component run loop, selection, prefill, acceptance distinction, checkpoints, review framing | compliant | none |
| D-1 | TUI all-components continuous mode and `.` wildcard; CLI help advertises run-all | mismatched | high |
| D-2 | "kills and reaps that owned group" vs. group kill + direct-child reap | mismatched | low |

**Total discrepancies requiring arbitration: 2**
(High: 1, Low: 1)

## Conclusion

The headless whole-component run loop, its stop conditions, its
approval-bound item/component/manual checkpoints, its durable
awaiting-approval state, and its fresh source-only / source-blind
independent review framing are supported by the observation and test
evidence as compliant with `REQ-CONTINUOUS-EXECUTION`,
`REQ-RUN-COMPONENT`, and the `Provided interfaces` contract.

Two discrepancies are retained for explicit human arbitration, and
neither intent nor implementation artifact was modified to erase one:
**D-1** (High) — the interactive TUI exposes a cross-component
"all components" continuous mode, treats `.` as a wildcard, and its CLI
help advertises omitting the component argument, in direct conflict
with the requirement that the component argument remain required and
that no `run-all`/`all` batch interface exist; and **D-2** (Low) — the
checkpoint design's "reaps that owned group" wording exceeds the
observed group-kill-plus-direct-child-reap guarantee. This record is
the compliance disposition for the `continuous-run` gate; it is
independent observation and comparison, not acceptance.
