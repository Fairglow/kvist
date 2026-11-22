<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Human Attention Briefs (`maerg`)

**Component:** `maerg`
**Task:** `attention-brief-compliance-review` — compare attention briefs with
intent.
**Compared artifacts:**

- `maerg/REQUIREMENTS.md` §`REQ-DECISION-SURFACING` (the controlling
  requirement)
- `maerg/CONTRACT.md` §**Human attention briefs** and the independent-review
  command surface it names
- `maerg/DESIGN.md` §**Human decision and blocker presentation** (and the
  deterministic-decision workspace's attention-publication clause)
- `ROOT_CONTRACT.md` §"Change and compliance rules"
- `maerg/IMPL.md`, section "Scoped human attention observations - 2026-10-30"
  (treated as an external implementation-record, not observed behavior)
- bounded test evidence (assertions inspected; listed below)

**Reviewer stance:** Independent compliance reviewer for the `attention-brief`
chain (write-tests → implementation → security audit → compliance review). The
implementer and the security auditor are separate contexts and did not author
this record. This is not self-certification, not a protected review receipt,
and not acceptance. Discrepancies and coverage limits are retained for human
arbitration.

## Review boundary and independence

**Two-phase method, as required by the task.** The runbook ideal splits the
pass into a clean-slate source-only *documenter* and a *source-blind*
*comparator*. This gate was performed in a single reviewer context, disclosed
as such:

1. **Phase A — source-only observation.** The bulk of the observation is the retained
   clean-slate records produced in *separate* contexts that read no intent:
   `docs/reviews/attention-brief-observation.md` and
   `docs/reviews/attention-retention-observation.md`. This turn additionally
   re-verified, against the current `maerg/src` attention presentation layer,
   its call sites, and scoped test source, the specific points that bear
   directly on `REQ-DECISION-SURFACING`. That re-verification was performed
   after the intent was already in this context, so it is a current-source
   cross-check, not a fresh clean-slate observation.
2. **Phase B — source-blind comparison.** The comparison phase mapped the intent
   artifacts against (1) and the bounded test evidence.

Every "observed" statement below is attributed to one of exactly two sources:
the retained clean-slate observation records, or this reviewer's current-source
re-verification of the files listed in the evidence section. No control is
asserted from the security audit of this chain.

**Independence that is preserved:** the security audit
(`attention-brief-security-audit`, `tests/security_audit_attention_brief.md`)
and the implementer are distinct contexts. The security audit's assessments
were not used as evidence; where the same behavior is stated below, it is
re-observed from source.

**Independence that is NOT preserved, disclosed:**

1. This gate was performed in a single reviewer context rather than two
   strictly separated clean-slate contexts (documenter + source-blind
   comparator). The genuine clean-slate source-only observations are the
   retained separate-context records; this turn's additional current-source
   re-verification occurred with the intent already read. The strict
   two-context isolation is disclosed, not achieved.
2. An earlier source-blind diagnostic comparison for this feature exists at
   `/workspace/docs/reviews/attention-brief-comparison.md` (2026-10-30) and
   was read for context. Its conclusions are **not relied upon**; the findings
   below are re-derived from current source and current intent, and that record
   is named only where it is chronologically relevant.

**Not relied on as evidence:** peer or parent implementation artifacts, Git
history/diffs, execution logs, and prior compliance conclusions beyond the
named attention-brief records.

## Bounded evidence relied upon

**Retained clean-slate observation records (source-only; separate contexts,
no intent read; Phase A):**

- `/workspace/docs/reviews/attention-brief-observation.md` (2026-10-30)
- `/workspace/docs/reviews/attention-retention-observation.md` (2026-10-30)

**Current-source re-verification (inspected this turn; Phase A):**

- `maerg/src/task_commands/attention.rs` (whole module: `resolution`,
  `decision`, `failure`, `persist`, `redact`, `review`, `review_index`,
  `require_real_directory_chain`, and its unit test module)
- `maerg/src/task_commands.rs` (task-run wrappers `run_task_under_lock` /
  `execute_task_under_lock`, the embedded `complete` surfaced-decision branch,
  `evidence_redactions`, `unresolved_run_error` behavior)
- `maerg/src/task_commands/agent_outcome.rs` (`AgentAttempt::decision`)
- `maerg/src/task_commands/continuous.rs` (`run` failure wrapper,
  `record_final_state`, `execute_review`, `sanitize_report`,
  `checked_directory`)
- `maerg/src/task_commands/decisions/engine.rs` (`ensure_no_pending`)
- scoped test source: `maerg/tests/task_commands.rs`, `maerg/tests/agent.rs`,
  the in-crate test module of `maerg/src/task_commands/attention.rs`, and
  `maerg/src/agent.rs` proposal-recorder assertions (inspected, not executed)

**Bounded test evidence (assertions inspected; the suite is run automatically
by Kvist in the verification sandbox after this turn, so no pass/fail count
from that run is asserted here):**

- `maerg/tests/task_commands.rs`:
  `task_run_refuses_missing_sandbox_before_transition` (preflight blocker
  brief content); `continuous_run_pauses_on_review_decision_without_running_later_items`
  (decision brief content, named in output, run-level `run.md` on re-run);
  `tool_only_authoring_decision_has_agent_markdown_and_response_steps`;
  `decision_brief_write_failure_retains_the_original_stop_and_unresolved_attempt`
  (link-replaced evidence dir, persistence-failure diagnostic, queue
  immutability, retained attempt); `continuous_run_retries_failed_checkpoint_without_replaying_completed_tasks`
  (checkpoint blocker brief `run.md`); `continuous_run_rejects_malformed_reviews_and_forged_ledger_state`
  (malformed-review blocker brief);
  `independent_review_redacts_escaped_secrets_from_stream_logs_and_reports`
  (redaction across stream, logs and review sinks);
  `task_run_cancels_agent_output_and_persists_redacted_bounded_evidence`
  (output-limit failure, bounded redacted evidence).
- `maerg/tests/agent.rs`: `execute_agent_surfaces_a_proposed_decision_and_pauses`
  (tool-only `propose_decision` pause), `execute_agent_redacts_the_decision_patch_in_the_recorded_evidence`.
- `maerg/src/task_commands/attention.rs` unit tests:
  `briefs_are_redacted_bounded_and_keep_content_addressed_history`,
  `resolution_disambiguates_command_named_components`,
  `unsafe_task_names_cannot_escape_and_commands_quote_component_names`,
  `linked_evidence_directory_alias_and_ancestor_are_refused` (unix).
- `maerg/src/agent.rs` test `decisions_reusing_call_ids_retain_separate_evidence`
  (session/turn/decision-index identity; no-clobber retention of raw
  proposal JSON/Markdown).

**Execution evidence (external, attributed):** `maerg/IMPL.md`
"Scoped human attention observations - 2026-10-30" records the retained
clean-slate source-only observation and the narrow retention observation, and
a separately attributed implementation claim of a 920-test pass with seven
existing ignores. That execution claim was neither executed nor verified here;
it is used only as chronology, not as pass evidence.

## Method

Each obligation in `REQ-DECISION-SURFACING`, the contract's **Human attention
briefs** clause, and the design's **Human decision and blocker presentation**
section was mapped to the Phase A observations and the bounded test evidence,
then classified as `compliant`, `mismatched`, `approved-deferred`, or
`underspecified`. Discrepancies and coverage limits are retained for explicit
human arbitration; no intent or implementation artifact was modified to erase
one.

## Findings

### F-1 Decisions and blockers get a bounded, redacted Markdown brief — compliant

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("Every task decision or
  execution blocker MUST have a bounded, redacted Markdown brief, not merely a
  pointer to a JSON directory"); CONTRACT §**Human attention briefs**.
- **Observed (Phase A):** A surfaced decision writes a Markdown brief to
  `.kvist/attention/<task>.md` before the task transitions to
  `awaiting-decision`, and the stored reason and the printed output name that
  specific file (`maerg/src/task_commands.rs` surfaced-decision branch;
  `maerg/src/task_commands/agent_outcome.rs` `decision`). Every task-run and
  continuous-run error is wrapped so a run- or task-level Markdown brief is
  attempted for preflight, execution, checkpoint and malformed-review failures
  (`maerg/src/task_commands.rs` `run_task_under_lock`;
  `maerg/src/task_commands/continuous.rs` `run`). The brief is redacted
  against configured values and fail-closed bounded at 256 KiB
  (`maerg/src/task_commands/attention.rs` `redact`).
- **Test evidence:** preflight blocker brief (`task_run_refuses_missing_sandbox_before_transition`),
  decision brief and run-level `run.md` (`continuous_run_pauses_on_review_decision_without_running_later_items`),
  checkpoint and malformed-review blocker briefs, tool-only authoring decision
  brief.
- **Severity:** none.
- **Coverage limit:** the failure wrappers make a best-effort bounded attempt;
  when the configuration, safe path, or persistence is unavailable the brief
  is refused with an explicit diagnostic (see A-1). The literal "every"
  obligation is thus satisfied as attempted delivery with an explicit
  unavailability path, not as unconditional publication.

### F-2 Agent explanation is authored in the existing proposal/review response — compliant (mechanism)

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("Available agents MUST
  author the explanation in their existing proposal/review response:
  situation, design implications, alternatives and tradeoffs, recommendation
  with rationale, and missing evidence"); CONTRACT §**Human attention briefs**
  ("Agent explanations cover the issue, design impact, alternatives/tradeoffs,
  recommendation/rationale and uncertainty").
- **Observed (Phase A):** the decision brief embeds the turn's collected
  explanation verbatim under an "Agent explanation (advisory)" heading, with a
  warning that the retained explanation may be output-limited and a pointer to
  the raw Markdown proposal/review companions for full evidence
  (`maerg/src/task_commands/attention.rs` `decision`). The explanation is the
  existing agent response (stdout/`propose_decision` `why`), not a new model
  call. No structural check establishes that all five requested sections are
  present or factually sound.
- **Test evidence:** `tool_only_authoring_decision_has_agent_markdown_and_response_steps`
  pins that a tool-only `propose_decision` response reaches the brief with its
  sections intact; `execute_agent_surfaces_a_proposed_decision_and_pauses`.
- **Severity:** none for the mechanism.
- **Coverage limit (retained):** section completeness and recommendation
  quality are not mechanically established; this is a coverage limitation,
  not an observed defect (A-2).

### F-3 Host adds identity, evidence references and actionable response steps — compliant

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("The host MUST add exact
  task/attempt and evidence references plus instructions for choosing/approving
  an approach, requesting clarification, rejecting with replacement
  instructions, and resuming safely"); DESIGN §**Human decision and blocker
  presentation** ("Host-authored resolution steps use only existing commands").
- **Observed (Phase A):** the decision brief header carries task and attempt
  identity and a raw-evidence directory reference; `resolution()` emits
  host-authored sections for choose/approve (`kvist decision … actions` / `do
  NUMBER` / `options` / `plan … --option … --guidance`), clarification
  (`discuss`), reject/redirect (`plan --guidance`, withdrawal closure rules),
  manual/offline planning (`plan --import`), and safe resumption (retry the
  task or checkpoint "without rerunning completed tasks", `approve-policy`
  note, "do not delete evidence or replay unknown effects"). Component names
  that collide with command names are disambiguated with `./` prefixes and
  shell quoting (`decisions::component_argument`,
  `decisions::shell_token`; unit test
  `resolution_disambiguates_command_named_components`). The failure brief adds
  a structured action list (review logs, transition to pending, re-run) with
  the exact commands.
- **Test evidence:** the decision-brief test asserts `## Choose or approve`,
  `## Ask for clarification`, `## Reject or redirect`, exact command lines and
  `not human acceptance`; the malformed-review blocker brief test asserts a
  `task transition . one-security-audit pending` step.
- **Severity:** none.

### F-4 Recommendations remain advisory; findings are not presented as facts — compliant

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("Recommendations MUST
  remain advisory; suspected findings MUST NOT be presented as established
  facts or approvals").
- **Observed (Phase A):** the agent explanation is labelled "advisory" and
  output-limited; the review companion is labelled "agent-written advisory
  evidence, not human acceptance or verified facts"; the resolution text says
  "A recommendation is not human acceptance. Intent approval never approves
  implementation or a reviewer's claims." The failure brief labels any
  pre-stop explanation "This text predates the stop; it is not independently
  verified", and its design-implications/alternatives/recommendation sections
  are explicit host boilerplate stating that no new design conclusion is
  inferred from the error.
- **Test evidence:** `not human acceptance` asserted in the decision brief.
- **Severity:** none.

### F-5 Failed inference/verification yields a labelled host diagnostic without another model call — compliant

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("When inference or
  verification fails, the host MUST retain a clearly labelled diagnostic brief
  from observed errors instead of requiring another model call or inventing
  design alternatives").
- **Observed (Phase A):** `attention::failure()` documents "No queue/ledger
  mutation or model call: even preflight failures can be explained." It builds
  the "Work stopped" brief from the redacted host cause (truncated at 128 KiB
  with an explicit marker), the retained pre-stop agent explanation when
  present, the queued blocked/awaiting-decision tasks (or an explicit
  "queue inspection is unavailable" note), and fixed host-written design /
  alternatives / recommendation sections.
- **Test evidence:** preflight brief contains `Host diagnostic` and `No extra
  inference`; checkpoint and malformed-review blocker briefs contain `Host
  diagnostic`.
- **Severity:** none.

### F-6 Continuous-run scheduling/checkpoint blockers receive the same presentation — compliant

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("Continuous-run
  scheduling/checkpoint blockers MUST receive the same Markdown presentation").
- **Observed (Phase A):** the continuous `run` entry point wraps every error,
  including preflight, through the same `attention::failure()` with the
  run-level `run.md` alias; per-task decision briefs still use the task alias.
  A blocked/awaiting-decision task's queue entry is listed inside the run
  brief.
- **Test evidence:** `continuous_run_pauses_on_review_decision_without_running_later_items`
  (task brief plus `run.md` naming the paused task and `awaiting-decision`);
  `continuous_run_retries_failed_checkpoint_without_replaying_completed_tasks`
  (`run.md` checkpoint blocker brief with the retry command).
- **Severity:** none.

### F-7 No new model effects; no task, approval or recovery authority — compliant

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("Summary generation MUST
  NOT accept intent, choose an approach, clear a blocker or replay work");
  CONTRACT §**Human attention briefs** ("These are advisory presentation
  records, not approvals or new task authority").
- **Observed (Phase A):** the presentation module's header states it is
  "human-facing presentation only; never task disposition or approval
  authority". `decision()` and `failure()` do not call a model or mutate the
  queue; `review()` is pure formatting of an already-collected report;
  `resolution()` is pure string construction. `persist()` performs a read-only
  queue inspection, checks the pending-decision fence
  (`decisions::ensure_no_pending` — a pending transaction refuses brief
  publication rather than disturbing it), and writes only under the existing
  or a freshly acquired component lock. The task transition that follows a
  decision brief is a separate operation of the caller.
- **Test evidence:** `decision_brief_write_failure_retains_the_original_stop_and_unresolved_attempt`
  (queue bytes unchanged, attempt evidence retained, outside symlink target
  untouched); `continuous_run_pauses_on_review_decision_without_running_later_items`
  (queue bytes unchanged after a blocked re-run).
- **Severity:** none.

### F-8 Summary persistence failure is explicit and preserves cause, effects and recovery fences — compliant (observed paths)

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("Summary persistence
  failure MUST be explicit and MUST NOT erase the original cause, effects or
  recovery fences").
- **Observed (Phase A):** a failed decision-brief write propagates before the
  `awaiting-decision` transition, so no status is claimed; the run error
  states the attempt remains unresolved ("do not replay or delete its
  evidence"). A failed failure-brief write keeps the original cause and appends
  "Markdown brief persistence also failed … Retained effects and recovery
  evidence were not cleared."
- **Test evidence:** `decision_brief_write_failure_retains_the_original_stop_and_unresolved_attempt`
  asserts exactly both diagnostics, the untouched outside directory, and the
  unchanged queue.
- **Severity:** none.
- **Coverage limit (retained):** pair-transactional publication of raw proposal
  JSON + Markdown is not established (A-3), and not all combined persistence
  error paths were exercised by the inspected assertions.

### F-9 Existing JSON evidence remains available — compliant

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("Existing JSON evidence
  MUST remain available").
- **Observed (Phase A):** briefs reference, not replace, raw evidence: the
  decision brief names the raw proposal/review directory, the review companion
  names its `<task>.<attempt>.json`, and the final review index names
  `component.json`. The attention writer creates its own directory and never
  deletes JSON. Raw proposal JSON/Markdown are published no-clobber with
  session/turn/decision-index-bound identities
  (`maerg/src/agent.rs`; test `decisions_reusing_call_ids_retain_separate_evidence`).
- **Test evidence:** decision-brief and review-companion tests read the JSON
  and Markdown side by side; the write-failure test confirms the attempt JSON
  survives.
- **Severity:** none.

### F-10 Output names the specific Markdown file — compliant

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("Until full transcript
  rendering exists, output MUST name the specific Markdown file").
- **Observed (Phase A):** the decision outcome prints "Human-readable summary:
  <path>" and "Open this Markdown file…"; the failure brief is named in the
  returned error reason.
- **Test evidence:** the decision-brief test asserts the printed stdout
  contains the brief's exact path.
- **Severity:** none.
- **Coverage limit:** actual rendered TUI behavior is not established by the
  inspected evidence.

### F-11 Historical briefs remain distinguishable from the current state — compliant

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` ("Historical briefs MUST
  remain distinguishable from the current state").
- **Observed (Phase A):** decision briefs are labelled "Snapshot:
  awaiting-decision … describes the retained attempt, not a guarantee of
  current source or task state"; failure briefs are labelled "a historical
  stop diagnostic; inspect current task state before acting"; the final review
  index is labelled "a snapshot, not human acceptance". Content-addressed
  history files (`<stem>.<sha256>.md`) are retained and verified against their
  identity, while the replaceable `<stem>.md` alias marks the latest copy.
- **Test evidence:** `briefs_are_redacted_bounded_and_keep_content_addressed_history`
  (re-published identical text adds no file; changed text adds a new history
  file and replaces the alias); `linked_evidence_directory_alias_and_ancestor_are_refused`
  (alias and directory link refusal, outside targets untouched).
- **Severity:** none.

### F-12 Review companions are same-stem Markdown; the final index notes absences — compliant

- **Locator:** CONTRACT §**Human attention briefs** ("Review JSON has a
  same-stem `.md` companion containing the agent-written report"); DESIGN
  §**Human decision and blocker presentation**.
- **Observed (Phase A):** `execute_review` publishes the attempt JSON and a
  redacted, bounded (64 KiB) same-stem Markdown companion via
  `attention::review()`, and `record_final_state` writes `component.json` then
  `component.md` via `attention::review_index()`, which links only verified
  regular non-link companions and explicitly notes missing historical ones.
  Observation and comparison sections are kept distinct in the companion.
- **Test evidence:** `continuous_run_pauses_on_review_decision_without_running_later_items`
  reads the attempt Markdown companion; `independent_review_redacts_escaped_secrets_from_stream_logs_and_reports`
  checks redaction across review sinks.
- **Severity:** none.
- **Coverage limit:** the index checks file type, not source-digest freshness
  of each report; neither the index nor a report establishes review coverage or
  human acceptance.

### F-13 Bounds, redaction and safe-path behavior match the design — compliant

- **Locator:** DESIGN §**Human decision and blocker presentation** ("redacted
  bounded briefs … 256 KiB"; "unsafe/unavailable state directories fail
  visibly"; "no-clobber publication").
- **Observed (Phase A):** `redact()` performs literal configured-value
  substitution, then rejects (never silently truncates) briefs above 256 KiB;
  the host cause is separately truncated at 128 KiB with a marker.
  `require_real_directory_chain` walks every ancestor with non-following
  metadata, refusing link-like or non-directory entries and anything outside
  the project root; the attention directory and alias are re-checked with
  `symlink_metadata` before creation/replacement; unsafe task names become a
  `task-<sha256>` stem.
- **Test evidence:** unit tests for redaction, bounds, history, stem
  sanitization, quoting, and symlinked directory/alias/ancestor refusal.
- **Severity:** none.
- **Coverage limit (retained):** redaction is literal substitution, not general
  secret detection or Markdown sanitization; the check-then-write window is not
  atomic, and the component lock that mitigates it is advisory.

### F-14 Operational-evidence exclusion from checkpoint ownership — underspecified in the supplied evidence

- **Locator:** DESIGN §**Human decision and blocker presentation** ("This
  directory is operational evidence outside checkpoint inventory/effect
  hashes: explaining a failed cleanliness check or checkpoint cannot itself
  make its retry dirty or invalidate the pending tree").
- **Observed (Phase A):** the attention module writes under the component lock
  and changes no queue, ledger, or attempt state; the inspected failure-path
  tests confirm queue immutability. The supplied observations do not include
  the checkpoint inventory/effect-hash computation itself, so the claim that
  attention writes cannot dirty a checkpoint retry or invalidate a pending
  tree is not established from the permitted evidence of this review.
- **Severity:** none (design intent is coherent; evidence gap only).
- **Classification:** `underspecified` — retained as arbitration point A-4.

### F-15 The "every" brief obligation versus unsafe/unavailable publication — arbitration point

- **Locator:** REQUIREMENTS §`REQ-DECISION-SURFACING` first sentence versus the
  same section's explicit exception ("unsafe/unavailable state directories
  fail visibly" is the design's stated behavior) and the advisory exception
  for this presentation change.
- **Observed (Phase A):** when component/configuration resolution fails, the
  error reports "Markdown brief unavailable: safe component/configuration
  resolution failed"; an unsafe path or persistence failure reports the
  persistence failure while keeping the original cause. Publication is refused
  rather than forced.
- **Severity:** none as an observed defect; literal tension retained as A-1.

## Discrepancy summary

| ID | Finding | Status | Severity |
|----|---------|--------|----------|
| F-1 | Bounded redacted Markdown brief for decisions and blockers | compliant (with explicit unavailability path) | none |
| F-2 | Agent-authored explanation in the existing response | compliant (mechanism); completeness unestablished | none |
| F-3 | Host identity, evidence refs, response steps | compliant | none |
| F-4 | Advisory recommendations, no fact/approval presentation | compliant | none |
| F-5 | Labelled host diagnostic without another model call | compliant | none |
| F-6 | Continuous-run blockers share the presentation | compliant | none |
| F-7 | No new model effects or task/approval authority | compliant | none |
| F-8 | Explicit persistence failure preserving cause/effects/fences | compliant (observed paths) | none |
| F-9 | Existing JSON evidence remains available | compliant | none |
| F-10 | Output names the specific Markdown file | compliant | none |
| F-11 | Historical vs current state distinguishable | compliant | none |
| F-12 | Same-stem review companions and final index | compliant | none |
| F-13 | Bounds, redaction, safe-path behavior | compliant | none |
| F-14 | Checkpoint-ownership exclusion of the attention directory | underspecified | none |
| F-15 | "Every" brief vs unsafe/unavailable publication | arbitration point | none |

**Established mismatches requiring arbitration:** 0
**Underspecified / arbitration points retained:** F-14 plus A-1…A-5 below.

## Arbitration points retained for the human

1. **A-1 (from F-15 / F-1):** clarify the delivery obligation when no safe
   publication path exists. The current behavior refuses the brief with an
   explicit diagnostic; the literal "every decision or blocker MUST have a
   … brief" sentence is read as bounded-attempt with explicit unavailability.
   No intent was changed.
2. **A-2 (from F-2):** decide whether section completeness and factual quality
   of agent-authored explanations should be mechanically established, or remain
   a prompt-level obligation.
3. **A-3 (from F-8):** resolve expectations for partially published raw
   proposal JSON/Markdown pairs (JSON remains when the Markdown companion
   fails; same-identity replay is refused rather than completing the pair) and
   for preservation of every original error in combined persistence failures.
4. **A-4 (from F-14):** supply or reference the checkpoint
   inventory/effect-hash evidence needed to establish that attention
   publication cannot dirty a checkpoint retry or invalidate a pending tree.
5. **A-5 (from F-13):** confirm the advisory-boundary limits are accepted as
   designed: literal-only redaction (no general secret detection), no
   Markdown sanitization, non-atomic check-then-write with an advisory
   component lock, and the 256 KiB cap that a large explanation or queue can
   hit, preventing the whole brief.

## Coverage and limits of this review

- The bounded test evidence was **inspected, not executed** in this review.
  The verification sandbox runs `cargo test` automatically after this turn;
  this record asserts no pass/fail count from that run.
- The `IMPL.md` execution claim (920 tests passing, seven existing ignores)
  is a separately attributed implementation record, not independent evidence;
  it was used only for chronology.
- Realistic model explanation quality, rendered TUI behavior, index freshness,
  and concurrent filesystem-path safety are not established by the supplied
  evidence; they are coverage limitations, not proven defects.
- The strict two-context isolation (separate clean-slate documenter and
  source-blind comparator) is disclosed as not achieved; the Phase A / Phase B
  separation within this single context is the boundary that was kept.
- Nothing here accepts intent, approves an approach, clears a blocker, replays
  work, or certifies the implementation.

## Conclusion

Across `REQ-DECISION-SURFACING`, the contract's **Human attention briefs**
clause, and the design's **Human decision and blocker presentation** section,
the inspected current source and bounded test evidence establish the intended
attention-brief mechanism: bounded, redacted, advisory Markdown briefs for
decisions and blockers (task and continuous-run), host-authored identity and
actionable response steps, labelled host diagnostics without additional model
calls, explicit persistence-failure semantics that preserve causes, effects
and recovery fences, retained JSON evidence, snapshot/historical labeling,
same-stem review companions, and safe-path/identity retention behavior. No
established current implementation contradiction (mismatch) is found in the
permitted evidence. The five arbitration points above and the recorded
coverage limits remain open for human arbitration, and this record is a
diagnostic comparison, not acceptance.
