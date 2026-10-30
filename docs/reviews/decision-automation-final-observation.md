# Final bounded decision-support implementation observation

## Scope and evidence status

This is a fresh source-derived diagnostic supplement, not a native Kvist
receipt, an intent comparison, or whole-component compliance certification.
The observer read the decision implementation and its tests, and the related
CLI, shell, attention, status, continuation, and model-review source/test
sections identified below. No intent artifacts, prior implementation records,
prior reviews, Git history/diffs, or planning records were consulted. The
observer did not run Cargo, execute project workflows, change live state, or
test a real model gateway.

One read of the initial portion of
`/home/stefan/.copilot/session-state/778fd089-f61b-4bbd-8891-e55cc9d22944/files/decision-tests-final.log`
showed an ongoing `running 587 tests` run and progress output, without an
observed final result. It was not polled. Test coverage below describes
assertions in test code, not a claim that the final validation run completed
or that every assertion passed.

## Actual command grammar and presentation

`maerg/src/cli.rs::Command::Decision` requires an explicit `COMPONENT_DIR`
before the optional decision action. `maerg/src/task_commands/decisions.rs::Action`
defines:

- No action, or `show [TASK_ID]`: inspect pauses and retained guidance.
- `options [--task TASK_ID]`: request structured advisory alternatives.
- `discuss MESSAGE [--task TASK_ID]`: request a Markdown answer and retain
  bounded discussion.
- `plan [--task TASK_ID] [--option ID] [--guidance TEXT] [--import PATH]
  [--review-exception REASON]`: stage a plan. `--option` conflicts with
  `--import`; guidance may accompany either.
- `apply PLAN_ID --approve SHA256 [--continue]`: apply an authenticated plan
  and optionally enter ordinary component execution afterward.

`show` uses the queue directly rather than taking a full decision capture.
Without a task selector, zero or multiple paused tasks produce a list rather
than a planning error. An explicit task can be inspected even if it is not
currently paused. Planning, options, and discussion instead require one known
blocked/awaiting-decision task, with `--task` resolving ambiguity.
`show` reports evidence paths; it does not ingest the referenced attention
Markdown into its response. `support_index` links retained capture-specific
options/discussion aliases and states that they are historical.

Human and JSON CLI dispatch share `decisions::Output`; JSON success includes
`message`, `markdown_path`, `plan_id`, and `approval` under `result`.
Decision execution failures in that JSON dispatch are wrapped as an error
object. Human and shell dispatch present the message. `status.rs::guidance_entries`
gives awaiting-decision tasks a concrete `decision COMPONENT show TASK`
command. Help and attention output distinguish proposal, exact application,
implementation acceptance, and compliance.

## Payloads and advisory/manual behavior

`decisions/types.rs` uses deny-unknown-field structs and a tagged, typed
`TaskEdit`. A version-1 draft has a nonblank `rationale`, `documents` map,
`tasks` array, and `disposition` equal to `resolve` or `withdraw`.
`document_edits` defaults to an empty array. Task definitions contain exactly
`id`, `title`, `description`, `context`, `purpose`, `expected_outcome`, `kind`,
`depends_on`, and `requirements`. Edits are `add`, `update`, or `retire`;
retirement takes an ID and reason. The input cannot directly supply task
status, timestamps, acceptance, disposition metadata, or recovery state.

Only literal local `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md` names are
document targets. `engine/documents.rs::materialize` accepts either complete
replacement text or sequential exact excerpt edits for a given document,
never both. Each excerpt has a nonempty `before` that must occur uniquely,
including overlapping occurrences. Later edits see earlier edit results.
There is no fuzzy patching. Complete resulting documents are checked by the
native document validator, including recovery-placeholder rejection.
There may be at most 128 excerpt edits; final documents and aggregate
candidate data are bounded.

`options` validates a strict catalog containing a summary and 1–32
alternatives. IDs must be distinct bounded ASCII identifiers; title,
consequences, and recommendation must be meaningful bounded strings. Model
instructions request substantive alternatives, but host validation establishes
shape and text bounds, not the substantive correctness of an alternative or
the stability of an ID across regenerated catalogs.

`discuss` retains question/answer exchanges for the same capture identity.
It refuses a 33rd exchange or a conversation above 256 KiB instead of dropping
history. Questions are bounded at 16 KiB; recorded answers at 65,536 bytes.
`save_support` takes a component operation lock, rejects a changed previous
JSON alias, retains content-addressed JSON/Markdown history, then updates
latest aliases. These advisory records are not the authenticated application
plan. Their JSON and Markdown writes are separate operations.

Agent planning requires an option or nonblank guidance; it does not infer a
choice. A selected option must exist in the current capture's catalog; its
substantive fields are copied into selection guidance and model evidence.
Manual planning reads strict bounded JSON from `--import`. Supplying a
nonblank review exception avoids the advisory-review model request; manual
import without that exception still requests a model review.

`decisions.rs::ask` creates a distinct architect `ReviewSession::decision`
for each options, discussion, planning, or advisory-review invocation.
The advisory review receives the proposed draft, local context, and retained
conversation/evidence; it is not a clean-slate independent compliance session.
The returned `decision_required` flag is not an acceptance gate here:
`ask` returns the report text. Advisory findings/exception are preserved for
exact approval rather than used to certify compliance.

## Model context and report bounds

`decisions.rs::intent_evidence` supplies complete local intent documents,
the root contract, the nearest ancestor contract when declared, the selected
task, a queue graph projection, relevant retained attention/review Markdown,
all local authoring-proposal Markdown, and current discussion. Other queue
tasks are projected to ID/title/kind/status/dependencies/requirement links,
not their full authored descriptions. Source files and `IMPL.md` contents are
not included in this decision frame. Root vision/architecture are freshness
bindings, not model-frame document contents.

`agent/review.rs::ReviewSession` sends system/user messages with no tools and
`ToolChoice::None`, validates strict outer report JSON, and rejects returned
tool intents without executing them. Decision requests disable streaming and
private control. They have configured timeout, token accounting, bounded
transport output and combined diagnostic limits, cancellation, redacted logs,
and live-event recording. Calls pass through the counted retry mechanism;
the observer did not inspect that mechanism's implementation outside the
bounded source list.

For decision support without a configured native context limit, `request`
uses complete evidence bounded at 8 MiB and explicitly warns that capacity is
unconfigured; it does not assert a 32K serving limit. With configured capacity,
it computes an evidence budget after response reserve/headroom, then also
checks estimated full request size before inference. Oversized decision
context is refused, not fragmented or silently truncated. Independent review
has a separate fragmentation/synthesis path; that is not used for decision
requests.

Decision reports are bounded to 65,536 UTF-8 bytes. The host validates before
and after redaction, so replacement expansion cannot bypass bounds.
Options/drafts must subsequently parse as strict JSON inside the report.
Manual drafts allow up to 1 MiB, but an agent draft must also fit inside the
smaller report bound. Exact excerpt edits permit a small draft to describe
changes to a much larger preserved document.

## Exact approval, queue effects, and deterministic application

`engine/transaction.rs::prepare` checks capture equality before building
outputs and again before persistence. A plan includes its full captured
queue/task/documents/bindings, selection, rationale, review or exception,
exact output text, retired full task records and reasons, disposition,
timestamp, ID, approval, and authentication tag. The preview shows exact
before/after text for all four controlled files, archived records, review,
bindings, and the application command.

`identity` computes separate domain-prefixed SHA-256 plan-ID and human-approval
values from canonicalized serialized payload with ID, approval, and
authentication fields cleared. `storage::authenticated` computes HMAC-SHA-256
over serialized content with its authentication field cleared.
`validate_plan` checks identities, HMAC, version, project/component binding,
controlled-file scope, and output validation. `load` also requires preview
bytes to equal the generated preview. IDs must be 64 lowercase hexadecimal
characters; approval comparison requires the exact recorded string.
`storage::secret` uses approval state outside the project and rejects linked
or changed secret-file reads. The underlying secret creation/ownership helper
was not independently inspected in this scoped observation.

This is authentication of host-produced local content, not proof that a
particular human read the preview. Possession of the supplied approval string
and invocation of the command is the explicit approval surface. A same-user
process able to access the approval secret is outside any repository-only
tampering guarantee established here.

`engine/tasks.rs::outputs` clones the captured queue. New tasks are host-created
pending records, with fresh timestamps and no acceptance/recovery metadata.
Updates preserve host lifecycle fields and are restricted to pending tasks
or the selected blocked/awaiting-decision task. Completed, live, fenced, and
unrelated paused task updates are rejected. Duplicate/conflicting edits,
unknown additions/updates/retirements, and invalid queue graphs are rejected
by local checks or the native queue serialization/validation path.

Resolve clears only the selected pause to pending, clears its reason and
completion timestamp, inserts additions before it, and adds every new task ID
as its prerequisite. Existing dependencies are retained and deduplicated.
The tests establish native lifecycle-dependency rejection and acceptance of
a complete test/implementation/security/compliance chain.

Withdraw computes the selected task's full transitive dependant closure from
the original queue. Exactly that closure must have explicit retirement edits,
with bounded nonblank reasons. Completed, live, and fenced records cannot be
retired. Retired full original records and reasons stay in the plan/receipt;
source and attempt journals are not deleted, and withdrawal does not mark
tasks completed. Remaining queue edits/additions still undergo validation.

The host records resulting local document revision hashes, nearest parent
contract authority, and current revalidation metadata in `TODOS.yaml`.
It does not rewrite implementation observations or certify that the changed
intent is implemented. A resolve with empty document/task edits is supported;
document changes themselves do not automatically synthesize follow-up tasks.

Application without `--continue` invokes no model or Git publication. Its
outputs are frozen; it does not regenerate a draft or rerun advisory review.
Output iteration is stable `BTreeMap` order. Planning IDs include timestamps;
deterministic application does not imply identical IDs for separate planning
invocations.

## Freshness, filesystem bounds, recovery, and continuation

`engine/capture.rs::snapshot` binds exact controlled documents/queue, root
artifacts and configuration bytes, selected effective configuration fields,
absolute project/component identities, and nearest-parent contract authority.
It inventories an explicit list including `src`, `tests`, `IMPL.md`,
attempts, continuous ledger/reviews, attention, authoring proposals, reports,
evidence, and Rust manifests/build metadata. Inventory files have a 1 MiB
limit; aggregate capture is 8 MiB/10,000 entries, with depth/path limits.
Files outside that explicit list are not generically inventoried.
Directories recognized as nested component boundaries stop implementation
recursion. This is a bounded local capture, not a whole repository snapshot.

Catalog/discussion latest JSON aliases for the current decision workspace
are separately bound, including absence. `capture_id` excludes only those
two binding values, keeping workspace identity stable as discussion grows.
New/changed consumed aliases invalidate approval. Unrelated decision records,
model logs, and live-run diagnostics do not stale a plan simply because the
decision commands produced them.

Capture refuses pending decision transactions, existing component operation
locks, live/fenced tasks, unrecovered attempts, unknown attempt journals, and
unauthenticated/ineligible continuous state. `continuous::decision_preflight`
actually permits paused or awaiting-approval ledgers without a pending
checkpoint. Structurally valid local staleness can be captured and accepted
by a decision; changed nearest-parent authority must first be separately
revalidated. `verify` requires all frozen bindings unchanged and controlled
files to match exact before bytes, or exact approved after bytes during
recovery.

`storage.rs` rejects traversal, links, non-regular inputs, and Unix hard links.
Reads compare metadata before/opened/after states. Linux parent directories
are traversed with no-follow `openat`, pinned by descriptors, and checked by
identity. Non-Linux handling is different; the source/tests read here do not
establish equivalent race resistance on every platform.

`transaction::apply` authenticates the plan/approval, obtains or recovers the
component operation lock, and writes an authenticated `pending.json`.
Each controlled replacement is staged beside the destination, synchronized,
checked against its exact before bytes, persisted, and followed by progress
recording. Bindings and lock ownership are rechecked during application.
This is a recoverable multi-file transaction, not one atomic multi-file
filesystem operation or a rollback mechanism.

Recovery accepts only the identical authenticated plan and known before/after
states. External intermediate edits, recorded-completed writes subsequently
reverted, binding drift, or journal tampering fence recovery. Dead-lock
takeover is bound to the authenticated lock digest. Pending state fences
ordinary writers through the task/repair guards. Attention failure/publication
paths check the fence and avoid rewriting bound attention evidence while
recovery is pending.

After exact final output checks, application writes an authenticated applied
receipt, then removes the pending marker. The receipt marks source-derived
evidence stale and execution/compliance/model-findings/Git-publication claims
false. A receipt-before-cleanup interruption can be resumed without rewriting
the receipt. Once a receipt exists with no pending marker, repeat application
returns idempotently without requiring the later working tree to equal the
old outputs; subsequent legitimate execution is therefore not overwritten.

Retired-journal recognition checks an authenticated applied receipt, exact
archived original task, explicit withdrawal/reason, component identity, and
the frozen journal digest. Copied receipts or edited journals do not become
authority for unknown historical attempts.

`decisions.rs::execute` releases application handling before `--continue`
calls ordinary `run_task_or_item(..., None, false)`. Ordinary execution policy,
readiness, recovery, and checkpoint requirements still apply. Continuation
can perform implementation/model work and ordinary checkpoint publication;
those are not properties of the isolated deterministic apply step. If it
fails, the error explicitly says accepted decision effects remain recorded.
The continuous runner accepts definition changes at an eligible paused
boundary and resets scheduling bookkeeping accordingly.

## Shell behavior and concrete test coverage

`bark/prefill.rs::ComponentPrefill` inserts focus into the actual Reedline
component slot, not an invisible execution fallback. It respects typed or
recalled components anywhere in the buffer, partial/quoted tokens, cursor
position, option values, absent focus, and deliberate deletion until the next
submission. Leading-dash/command-named focus values gain `./`; quoting
preserves spaces and Unicode. `bark.rs` resolves the explicitly parsed
decision component against the shell's project.

`bark/completion.rs::walk` retains component scope through action traversal;
`--task` and `--task=value` complete task IDs for the explicit component
rather than shell focus. Completion offers the component's task domain;
it is not restricted to paused tasks, so planning still validates eligibility.
Component prefill advances `task run` completion to that component's task IDs.

Concrete source tests include:

- `decisions/tests.rs`: command-word path disambiguation, more than three
  alternatives, duplicate/path-like option IDs, forbidden lifecycle fields,
  stale support-write rejection, invalid UTF-8, and linked inputs.
- `decisions/engine/tests.rs`: exact outputs/idempotence; wrong approval;
  source/root/configuration/parent/evidence drift; plan/preview tampering;
  full retirement closure; completed-record and Git-index preservation;
  invalid graphs and lifecycle prerequisites; local staleness; malformed
  documents/placeholders; capture bounds; unknown/live attempt guards;
  authenticated continuous checkpoint guards; retirement receipt/journal
  checks; interrupted writes, receipt-before-cleanup, pending tampering,
  lock takeover, and repeat application after later execution.
- `engine/document_tests.rs`: a compact draft preserving over 200 KiB of
  unchanged Unicode text, absent/empty/multiple/overlapping anchors,
  nonlocal paths and mixed replacements/edits, sequential Unicode edits,
  128/129 edit boundary, expanded final-document bound, and strict fields.
- `engine/context_tests.rs`: absence-to-present/current-alias drift and
  stable workspace identity, plus exact/bare task-run refusal preserving
  attention bytes and allowing later identical decision recovery.
- Linux integration tests in `maerg/tests/task_commands.rs`:
  `decision_agent_options_discussion_and_plan_never_apply_unseen_effects`
  uses a mock gateway, verifies five options, follow-up discussion context,
  advisory review, unchanged queue until apply, no source marker in support
  requests, and no model request during isolated application.
  `decision_unknown_capacity_keeps_large_intent_instead_of_asserting_32k_limit`
  verifies both large-input markers arrive at the mock and configured
  insufficient capacity refuses before another request.
  `manual_decision_plan_and_inspection_need_no_model_and_bind_exact_approval`
  uses an unavailable endpoint, rejects wrong approval without queue writes,
  and checks JSON application output.
  `applied_decision_continues_prerequisites_before_retrying_a_real_paused_review`
  creates an actual host pause using mocked model arbitration, inserts a full
  investigation lifecycle, asserts investigation compliance precedes retry,
  observes a second original attempt, and expects all 12 tasks completed.
- Related integration assertions preserve later pending items at a review
  pause, retain advisory authoring Markdown, and retain unresolved attempts
  when attention publication fails.
- CLI/prefill/completion unit tests cover required visible component and
  approval syntax, command-named explicit paths, editable prefill, deletion,
  quoting, inline task flags, and explicit-component completion. The Linux PTY
  smoke test exercises root/dash-component decision inspection and task
  prefill/completion while execution is policy-refused; it is not an
  interactive plan/apply or successful model execution test.
- `maerg/tests/status.rs::status_blocked_details_list_reasons_and_exact_next_commands`
  checks decision guidance in text and overview reports.
  `agent/review.rs` unit tests check escaped diagnostic redaction and report
  bounds after redaction expansion.

## Earlier source-observed defects and limits

The following two findings describe the source at the initial observation.
They are preserved as earlier findings, not asserted as unresolved defects
in the subsequently inspected source. The narrow follow-up below records
the attributable corrections.

1. **Attention instructions mishandled command-named components.**
   Initially, `task_commands/attention.rs::resolution` prefixed `./` only for a
   leading-dash component, then printed that quoted value in every
   `kvist decision ...` instruction. For a component named `plan`, for
   example, it emits `kvist decision 'plan' options ...`, rather than the
   unambiguous `kvist decision './plan' options ...`. Quotes do not remove
   action-name ambiguity in Clap. The decision module's
   `component_argument` already explicitly handles this case, and
   `decisions/tests.rs::printed_decision_component_words_are_unambiguous_paths`,
   CLI tests, and prefill tests establish the `./` representation for
   `show/options/discuss/plan/apply/help`. Attention-generated response
   instructions should use the same disambiguation. This observation did
   not edit that production code.
2. **Shell builtin completion help was stale for omitted task selection.**
   Initially, `bark/tree.rs::inject_shell_builtins` described `run` as running the next
   ready task when omitted, and its task positional similarly said it
   defaulted to the next ready task. `bark.rs::handle_run` actually dispatches
   omitted-task `task run COMPONENT`, whose
   `task_commands.rs::run_task_or_item_with_controls` enters continuous
   execution. This is a presentation mismatch, not evidence that the
   continuous dispatcher fails.

The inspected tests do not establish real gateway/model reliability,
semantic preservation of every proposed requirement, proof of human preview
inspection, universal filesystem race resistance, exhaustive crash/power-loss
behavior, or freshness of files outside the capture allowlist. Interruption
tests use host failure injection and restored bytes; the dead-lock test
constructs/re-authenticates a journal fixture rather than observing every
possible operating-system crash. No final test-run success is asserted here.

## Narrow source-only follow-up: 2026-10-31

This follow-up inspected only the two named presentation functions, their
named regression tests, and the two named task-command integration tests.
It did not re-observe the full decision engine, inspect other review/intent
records or Git history, run Cargo, poll validation, or inspect new test output.
Changes to the earlier findings below are grounded in current source, not
the coordinator's statement that they were fixed.

### Attributable corrections to the two earlier findings

- `maerg/src/task_commands/attention.rs::resolution` now obtains its component
  argument from `decisions::component_argument(component)` instead of locally
  handling only leading dashes. All generated decision, resume, and transition
  instructions interpolate that shared rendered argument. This removes the
  earlier inconsistent component-rendering path. The new
  `resolution_disambiguates_command_named_components` test calls `resolution`
  for `show`, `plan`, `apply`, and `help`, and asserts each contains
  `kvist decision './NAME' options`. It is a generated-text assertion, not an
  executed command or end-to-end pause/apply test. The shared helper's behavior
  was observed in the initial bounded inspection; it was not re-read here.
- `maerg/src/bark/tree.rs::inject_shell_builtins` now describes builtin `run`
  as “Run continuously, or stop after an explicit task or TODO item”, and its
  task positional as “Task ID or TODO item; omit for continuous component
  execution.” The earlier next-ready-only help wording is absent from this
  builtin definition. The new
  `builtin_run_help_explains_continuous_default_and_explicit_stop` test finds
  `run` in the built root and checks the command help contains
  `Run continuously` and positional help contains `omit for continuous`.
  Despite its name, that test does not explicitly assert the stop wording or
  execute either selection mode. This follow-up observes the advertised TODO
  item wording but does not independently establish TODO item dispatch.

These corrections address the two specific earlier presentation findings at
their source locations. They do not constitute whole-feature certification
or independently observed test success.

### Directly related diagnostic/retry assertions

- `maerg/tests/task_commands.rs::task_run_executes_successfully_and_transitions_completed`
  currently expects exactly seven stderr lines for its mocked two-turn explicit
  implementation run. It separately asserts one protected-task message, one
  test-command verification message, one execution-limits message, two
  starting-inference messages, a developer/selected-model message, and a live
  journal path line. It still asserts successful execution output, then
  explicitly finalizes the attempt and checks completed status, sandbox
  manifest validation, and exclusion of the unapproved environment variable.
  This observes the test's precise diagnostic-channel expectations, not the
  runtime output of a passing invocation.
- `maerg/src/task_commands/attention.rs::resolution` now says to retry the
  task or checkpoint “without rerunning completed tasks.”
  `maerg/tests/task_commands.rs::continuous_run_retries_failed_checkpoint_without_replaying_completed_tasks`
  requires that exact phrase in the retained blocker brief, alongside host
  diagnostic and signing information. Its fixture forces signing failure,
  expects the first four tasks completed and later tasks pending, disables
  signing, retries ordinary component execution, and expects seven additional
  mock requests and three commit-log entries. The inspected test therefore
  retains both guidance and scheduling/checkpoint assertions; this source-only
  read does not demonstrate that the retry ran successfully.

The coordinator reported a first full task-command run with 71/73 tests
passing and two failures concerning these diagnostic/retry expectations.
That report is explicitly not independently observed validation evidence
here. No result from the targeted or replacement full run was inspected;
no final-run success is claimed.

## Final narrow shell-help follow-up: 2026-10-31

Only `maerg/src/bark/tree.rs::inject_shell_builtins` and
`builtin_run_help_explains_continuous_default_and_explicit_stop` were
re-inspected for this addendum. The preceding follow-up's TODO-item wording
and limited test assertions remain historical observations of that earlier
source, not descriptions of the current function/test.

The builtin `run` command now says “Run continuously, or stop after an explicit
task”; its positional help says “Task ID; omit for continuous component
execution.” Neither inspected help string advertises TODO items. The test now
asserts both `Run continuously` and `stop after an explicit task` in command
help, explicitly rejects `TODO item` in command help, and retains the
`omit for continuous` positional-help assertion. It does not separately
assert absence of TODO-item wording in positional help.

These are directly observed presentation and regression-assertion changes,
not evidence of executed tests or unobserved dispatcher behavior. No engine
re-review, Cargo execution, validation polling, or certification was performed.
