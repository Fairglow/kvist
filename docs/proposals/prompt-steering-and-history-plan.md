# Prompt Steering, History, and Queueing Plan

## Status and purpose

Proposed implementation plan, based on the current Skott implementation and
human-requested interaction changes. This document does not approve new
consumer-visible semantics, replace component intent, certify compliance, or
claim any of the proposed interfaces already exist.

Deliver the work in four independently usable increments:

1. Immediate steering that preserves the active task and conversation.
2. Durable prompt records, saved drafts, and lifecycle management.
3. A prompt browser opened by Ctrl+P and the action menu.
4. Explicit, editable execution queueing.

Executable queueing is deliberately deferred until steering works. Saving a
draft must never authorize execution.

## Vocabulary and recommended interaction

| Term | Meaning |
| --- | --- |
| Session | The conversation and its accumulated context. |
| Task/run | Work initiated by a submitted prompt; may span many model turns. |
| Model turn | One model request/response and associated tool proposals. |
| Steering message | Additional human direction for the active task, not an implicit replacement task. |
| Saved draft | Editable future text with no execution authorization. |
| Queue entry | Explicitly authorized future work with a target and execution order. |
| Delivery claim | The atomic point after which an accepted message can no longer be edited or withdrawn. |

Recommended actions:

- **Send / Steer now:** use the existing submission gestures. While busy,
  interrupt obsolete work cooperatively, add guidance, and continue the same
  task. While idle, start work in the current conversation.
- **Save for later:** save a draft without executing it.
- **Run next:** explicitly queue work after normal completion of the current
  task; when idle, the first explicitly queued entry may start immediately.
- **Stop:** stop the active work and pause automatic queue advancement.
- **New session:** retain its explicit clean-slate meaning; steering must never
  use this path.

Do not ask "send now or queue?" on every submission. Make the default visible
in the composer and expose alternatives through the menu. Choose additional
shortcuts only after checking existing bindings and terminal compatibility.

## Current implementation and investigation starting points

These paths are navigation aids, not immutable descriptions; recheck them when
starting a task:

| Path | Relevant responsibility |
| --- | --- |
| `skott/src/tui/app.rs` | Submission, in-memory prompt history, Ctrl+P, editor restoration, overlays, event folding, OSC 52 copy staging. |
| `skott/src/tui/mod.rs` | UI event loop, worker/bootstrap ownership, prompt dispatch, cancellation and session replacement. |
| `skott/src/tui/render.rs` | Composer, help, menus, overlay rendering and scrollbar integration. |
| `skott/src/run.rs` | Worker ownership, prompt channel, shared cancellation reset and shutdown. |
| `skott/src/session.rs` | Conversation, autonomous model/tool loop, tool-result pairing, recording and per-prompt budgets. |
| `skott/src/context.rs` | Context preflight, compaction and preservation of outstanding human messages. |
| `skott/src/session_log.rs` | Private operational journal and prompt/run records. |
| `skott/src/session_transcript.rs` | User-message and execution transcript recording. |
| `skott/src/history.rs` | Bounded, validated reads of existing session history. |
| `skott/tests/loop_integration.rs` | Existing multi-turn, cancellation and tool-loop integration coverage. |
| `skott/tests/context_preflight.rs` | Context fitting and conversation-group validation coverage. |

Currently, the worker receives another prompt only after `Skott::run` returns.
That call can encompass many model/tool turns. The UI echoes submissions before
the worker consumes them. Ctrl+P replaces the editor with earlier entries from
an in-memory list; it does not open a prompt browser.

## Global implementation constraints

- Preserve completed filesystem effects, conversation context, observed tool
  results, outstanding objectives and applicable budgets when steering.
- Do not widen sandbox, network, credential, approval or promotion authority.
- Do not automatically replay interrupted effects or claim they were undone.
- Distinguish user steering, user stop, deadline expiry, failure and shutdown.
- Serialize authoritative state transitions through an explicit owner.
- Correlate messages, tasks and worker events so late events cannot overwrite
  newer state. Preserve ordering for rapid consecutive submissions.
- Report acceptance, interruption and model-context delivery separately.
  Delivery does not prove that a model understood or followed a message.
- Keep persistence private, bounded, inspectable and separate from tracked
  project intent. Respect no-log mode and surface storage failures.
- Keep historical submissions immutable. Editing delivered text produces a
  new draft/correction rather than rewriting the conversation.
- Remain headless-compatible; the terminal must not become the business-logic
  owner. Do not add headless interactive control APIs without separate intent.
- Do not introduce compatibility or migration for retired pre-release formats.
- Use existing dependencies and rendering/storage helpers where suitable.

## Task execution convention

Each task below includes its prerequisites, local context, work and completion
criteria. Read this section and the global constraints with any individual task.

Before changing implementation, inspect `VISION.md`, `ARCHITECTURE.md`,
`ROOT_CONTRACT.md`, `docs/standards.md`, and Skott's `REQUIREMENTS.md`,
`CONTRACT.md`, `DESIGN.md` and `TODOS.yaml`. Inspect an immediate parent
component contract when present. Provider internals are not implicit component
context; use declared provider contracts at those boundaries.

For each increment, approve its exact intent before writing failing tests.
Record atomic, requirement-traceable tasks in `skott/TODOS.yaml` with
`write_tests`, `implement_code`, `security_audit` and `compliance_review` in
that order. This Markdown plan is not a substitute for that queue.

### T01 — Resolve semantics and prepare steering intent

**Dependencies:** None.

**Context:** The user wants immediate complementary guidance, not a reset.
The implementation currently queues whole prompts between autonomous runs.

**Work:**
- Specify the recommended actions and vocabulary in Skott's authoritative
  intent documents, keeping requirements, public semantics and private design
  separate.
- Define the delivery claim, editable/withdrawable pending states, rapid-message
  ordering and completion-versus-steering race.
- Define interruption policy for generation, retry waits, tool execution,
  compaction and any actual awaiting-input state. Do not invent a question API.
- Decide which operations finish atomically before steering can proceed and
  how uncertain effects block automatic continuation.
- Define budget accounting so steering cannot silently reset task safety
  limits, retry accounting or repeated-action protection.
- Decide how later model/effort changes interact with steering; do not silently
  route steering through the current fresh-session replacement behavior.
- Specify minimal pending-message edit/withdraw controls available before the
  full browser exists.
- Record unresolved choices for human approval. Obtain the available advisory
  review opportunity or record an explicit exception; fabricate no receipts.

**Done when:** Approved steering requirements, contract, design, verification
obligations and an ordered component task bundle exist.

### T02 — Write failing steering and continuity tests

**Dependencies:** T01.

**Context:** Exercise `run.rs`, `session.rs`, context preflight and TUI event
handling with deterministic fake transports/executors and synchronization
barriers, not timing-dependent sleeps.

**Work:** Cover steering during generation, backoff, an executing tool and
between tools; repeated steering; edit/withdraw versus claim; a concurrent final
answer; late events; stop; shutdown; recording failure; and budget exhaustion.
Assert preservation of original goal/context and completed changes, valid
tool-call/result pairing, no stale next-tool dispatch, no automatic effect
replay, and no new session. Verify that guidance reaches a subsequent model
request before the original autonomous run would otherwise finish.

**Done when:** Tests fail for the current FIFO-only behavior and pin the exact
approved semantics, including a measurable fake-controlled interruption bound.

### T03 — Implement coordinated immediate steering

**Dependencies:** T02.

**Context:** Primary files are `run.rs`, `session.rs`, `tui/mod.rs` and
`tui/app.rs`. A bare "cancel, then send String" is insufficient because token
reset, queued work and terminal events can race.

**Work:** Add typed, correlated control messages and a worker-owned delivery
handoff. Use operation-scoped interruption without ending the session or
misreporting the task as completed/cancelled. Reconcile observed and skipped
tool results, append claimed guidance in order, preserve task accounting and
resume through context preflight. Expose pending edit/withdraw controls and
truthful statuses such as "interrupting" and "added to model context."
Handle idle submission and bootstrap-held initial prompts without regressions.

**Done when:** T02 passes, the existing targeted lifecycle/context tests remain
valid, help explains the new default, and normal submission no longer silently
waits for task completion. Run the smallest applicable formatter, test, lint
and build commands.

### T04 — Independently audit steering implementation

**Dependencies:** T03.

**Context:** Review the changed control path and its execution boundaries, not
only the visible TUI behavior.

**Work:** Independently inspect cancellation ownership, stale tool dispatch,
effect replay, recording-before-effects, budget preservation, event correlation,
shutdown and authority invariants. Record concrete findings and limitations;
fix accepted findings surgically and rerun their targeted coverage.

**Done when:** Findings have explicit dispositions and no unresolved blocker is
silently accepted. Audit output does not substitute for compliance review.

### T05 — Derive steering evidence and compare against intent

**Dependencies:** T04.

**Context:** The implementer must not certify its own implementation.

**Work:** Have a clean-slate documenter derive the affected `IMPL.md` record from
source and test evidence, excluding intent, queue, prior records/reviews, chat
and Git history. Have a separate source-blind reviewer compare approved
requirements, contract and design with that record and test evidence.

**Done when:** Discrepancies and evidence limits are recorded for human
arbitration. Release the steering increment only after explicit disposition.

### T06 — Define durable prompt records and retention intent

**Dependencies:** T05.

**Context:** Existing prompt recall is memory-only; existing session transcripts
are not an editable future-prompt store.

**Work:** Define stable IDs, revisions, project/session/task association, ordering,
timestamps, source text, delivery state and saved-draft state. Keep pin/archive
attributes orthogonal. Specify storage ownership, exact format/version, bounds,
atomic writes, concurrent-process behavior, failure handling and no-log behavior.
Define deletion scope relative to transcript copies and active model context.
Propose auto-archival after 30 days for terminal unpinned entries; obtain approval
for the actual policy. Exclude active, pending, pinned and unsent saved entries.
Prepare the intent/review opportunity and ordered task bundle.

**Done when:** Approved semantics distinguish saved text from execution
authorization and make retention/deletion guarantees explicit.

### T07 — Write failing prompt-store tests

**Dependencies:** T06.

**Context:** Reuse existing private-path and bounded-read patterns through their
supported interfaces; keep filesystem fixtures isolated.

**Work:** Test exact multiline/Unicode text round trips, stable ordering, revision
conflicts, restart recovery, malformed/oversized records, links/special files,
write interruption and permission failures. Test pinning, archival thresholds,
unarchive, deletion scope and no-log behavior using an injected clock.
Verify saved drafts never enter execution and recovery never replays effects.

**Done when:** The approved storage and retention invariants have failing tests.

### T08 — Implement the prompt store and saved-draft operations

**Dependencies:** T07.

**Context:** Keep storage and lifecycle logic separate from TUI widgets and from
canonical Kvist workflow evidence.

**Work:** Implement bounded, private, atomic persistence and explicit lifecycle
operations. Integrate accepted/claimed/delivered messages from steering without
creating contradictory transcript and store histories. Add save, restore,
pin/unpin, archive/unarchive and delete operations with observable errors.
Retain exact submitted text; deriving a heading must not alter it.
Document storage location, retention and no-log behavior.

**Done when:** T07 and targeted recording tests pass; saved drafts survive an
allowed restart without executing. Run applicable quality gates.

### T09 — Independently audit prompt persistence

**Dependencies:** T08.

**Context:** Prompt contents may include secrets; private operational records
must not become public/project intent or execution authorization.

**Work:** Inspect path validation, bounded reads, permissions, atomicity,
concurrent updates, retention, deletion claims and no-log behavior. Check that
store failures cannot produce false delivery acknowledgements or later effects.

**Done when:** Independent findings are explicitly resolved or escalated.

### T10 — Derive prompt-store evidence and compare against intent

**Dependencies:** T09.

**Work:** Repeat T05's clean-slate source/test derivation and separate source-blind
comparison for the prompt-store increment.

**Done when:** Storage, privacy, recovery and retention discrepancies have human
dispositions; no automatic compliance claim is made.

### T11 — Define the prompt browser and clipboard intent

**Dependencies:** T10.

**Context:** Ctrl+P currently recalls prior prompts; the action menu already has
a separate session-history viewer.

**Work:** Define Ctrl+P and a menu item opening **Prompts**, without conflating it
with **Sessions**. Specify list/detail focus, search, project scope, stable time
ordering, precise lifecycle badges, full-text wrapping and scrolling. Define
All/Past/Present/Future filters, combinable pin filtering and archived visibility.
Specify explicit restore without send, preservation of the current editor draft,
pending edit/withdraw conflicts, and actions for save/pin/archive/delete.
Define copy and paste capability/failure behavior, size bounds and terminal
fallbacks. Prepare approved intent and the ordered task bundle.

**Done when:** Every action, focus transition and data-loss boundary has
acceptance criteria, including narrow-terminal and keyboard-only use.

### T12 — Write failing browser and clipboard tests

**Dependencies:** T11.

**Context:** Use existing App key tests and render tests; test adapters rather
than relying on an actual desktop clipboard.

**Work:** Cover menu/Ctrl+P entry, filtering/search, stable selection during live
updates, draft preservation on close/restore, full prompt visibility,
independent scrolling, resize and long Unicode text. Cover pending-message claim
conflicts and visible copy/paste errors. Assert that selection, restore and
clipboard paste never execute a prompt. Preserve existing session replay.

**Done when:** Browser behavior and terminal fallback requirements have failing
state/render tests.

### T13 — Implement the prompt browser and clipboard integration

**Dependencies:** T12.

**Context:** Main integration points are `tui/app.rs`, `tui/render.rs` and
`tui/mod.rs`, backed by the prompt-store interface.

**Work:** Add the browser and menu/help changes, reuse themed scrollbar/wrapping
helpers, and provide the approved lifecycle actions. Keep expensive history
reads off the input-critical path. Reuse OSC 52 for copying where appropriate;
support terminal paste into editable text without automatic submission.
Do not automatically query clipboard contents. Handle capabilities explicitly
and avoid unbounded preview allocation or blocking UI-thread clipboard calls.

**Done when:** T12 passes; Ctrl+P opens the browser; long prompts are fully
reachable; draft text survives navigation. Run applicable quality gates and
document the changed binding.

### T14 — Independently audit browser actions and clipboard boundaries

**Dependencies:** T13.

**Work:** Inspect terminal-control sanitization, clipboard disclosure boundaries,
size limits, action/selection races, accidental execution, blocking input paths
and draft loss. Record findings and targeted corrections.

**Done when:** Independent findings have explicit dispositions.

### T15 — Derive browser evidence and compare against intent

**Dependencies:** T14.

**Work:** Repeat T05's independent derivation/comparison process for browser
behavior, clipboard support and documented limitations.

**Done when:** Human arbitration has disposed of discrepancies before release.

### T16 — Define executable queue intent

**Dependencies:** T15.

**Context:** Future saved drafts exist, but do not authorize execution. Queueing
is new behavior, not a reinterpretation of all saved future text.

**Work:** Define explicit Run next authorization, target project/conversation,
FIFO order, edit/reorder/remove operations and the claim boundary. Specify
idle first-entry start and one-at-a-time dispatch after normal completion.
Define pause on stop, failure, exhaustion, unanswered questions, uncertain
effects and shutdown. Define steering priority over queued work. Require
explicit resumption after application restart; decide how New session and
model/effort changes affect queue targets without silent retargeting.
Display execution order separately from chronological browser order.
Prepare approved intent and the ordered task bundle.

**Done when:** Queue lifecycle, authorization, recovery and all automatic-start
conditions are unambiguous.

### T17 — Write failing queue scheduling and recovery tests

**Dependencies:** T16.

**Context:** Use deterministic worker/controller tests and persisted-store
fixtures, not a live provider.

**Work:** Cover FIFO, edit/reorder/remove versus claim, idle start, normal
completion advancement, steering priority and concurrent completion/submission.
Verify pauses for every non-success terminal condition. Test target mismatch,
New session, model changes, restart and shutdown. Assert saved drafts never
run, only one task dispatches at a time, and recovered/in-flight work never
silently re-executes.

**Done when:** Exact scheduling and recovery obligations have failing tests.

### T18 — Implement queue controls and scheduling

**Dependencies:** T17.

**Context:** Reuse prompt identities, revision checks, persistence and correlated
worker control. Do not add a second independent scheduler inside the UI.

**Work:** Implement explicit queue authorization, ordering, pause/resume and
claim transitions under one authoritative owner. Integrate normal-completion
events and steering priority. Add Run next, queue position, reorder/remove and
visible pause reasons to the browser/menu. Ensure restart restores inspectable
state without automatic execution. Document queued versus saved prompts.

**Done when:** T17 passes and all automatic starts match approved semantics.
Run applicable quality gates and relevant cross-increment regression coverage.

### T19 — Independently audit automatic execution and recovery

**Dependencies:** T18.

**Work:** Inspect execution authorization, target binding, duplicate dispatch,
stop/shutdown races, stale completion events, persistence ambiguity and
replay-after-crash risks. Verify that queue scheduling does not widen execution
authority or bypass existing resource validation.

**Done when:** Independent findings are explicitly resolved or escalated.

### T20 — Derive queue evidence and compare against intent

**Dependencies:** T19.

**Work:** Repeat T05's independent derivation/comparison process, covering both
queue-specific behavior and continuity with steering, persistence and browsing.

**Done when:** Discrepancies and remaining limits have human dispositions.
Update user documentation and component task state without claiming unsupported
delivery, erasure, replay safety or compliance guarantees.

## Verification scenarios for human acceptance

Use a disposable workspace and deterministic local fake provider where possible.
Do not rely on destructive interruption of real project work.

1. Start a multi-step task. Send "Keep going, but do not modify tests."
   Observe interruption acknowledgement and delivery before the old task would
   finish; the original goal and completed work remain.
2. Send guidance while a tool is executing. Observe truthful interruption status,
   no subsequent stale dispatch and explicit handling of uncertain effects.
3. Edit or withdraw a pending message. Race that action with delivery; exactly
   one outcome wins, and the UI reports it without losing text.
4. Open Ctrl+P with an unsent multiline draft. Browse, filter and scroll a long
   prompt; close without losing the draft. Restore a past prompt without sending.
5. Save a future draft, restart when persistence is enabled, and restore it.
   It must not execute merely because it was saved or recovered.
6. Pin an old completed prompt and advance the retention clock. It remains
   unarchived; an eligible unpinned prompt archives reversibly.
7. Copy and paste text on supported and unsupported clipboard configurations.
   Failures are actionable; paste never submits.
8. Explicitly queue two tasks. Normal completion advances once in FIFO order.
   Stop or fail the first task and verify the second stays paused.
9. Exit with queued and in-flight work. Reopen and inspect recovery state;
   nothing executes until explicitly authorized to resume.

## Reference patterns

These are interaction precedents, not normative dependencies or a reason to
copy shortcuts unchanged:

- [Pi: change direction](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/usage.md#change-direction)
  distinguishes steering from follow-up work and supports recalling queued text.
- [Claude Code: queue messages while working](https://code.claude.com/docs/en/interactive-mode#queue-messages-while-claude-works)
  distinguishes queued delivery from sending now and supports taking back
  pending input.

The shared design principle is explicit delivery semantics, visible state and
recoverable user input. Exact interruption timing remains a Skott contract
decision.
