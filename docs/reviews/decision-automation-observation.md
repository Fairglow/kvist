# Maerg decision automation: source-only observation

Provisional observation on 2026-10-30, pending a source-stabilization refresh.
This records only the source actually read; it must not be treated as a record
of subsequent edits. This is a scoped behavior record, not an acceptance,
security-audit report, compliance comparison, or approval receipt.

## Evidence boundary

The evidence read was the actual `maerg/src/task_commands/decisions.rs`, all
Rust files beneath `maerg/src/task_commands/decisions/`, and selected related
source in `task_commands.rs`, `task_queue.rs`, `file_io.rs`,
`task_commands/continuous.rs`, `task_commands/attention.rs`, `agent/review.rs`,
`cli.rs`, `bark.rs`, `bark/completion.rs`, `status.rs`, `help.rs`, and
`repair.rs`. Selected decision, continuation, pause, shell, and status tests
were read in `maerg/tests/task_commands.rs`, `shell_smoke.rs`, and `status.rs`.
Paths below are relative to the repository.

No actual intent artifacts, existing implementation record, guides/readmes,
peer implementations, earlier review records, conversation/history, or Git
history/diffs were read. Artifact names and template/help text quoted below
come from Rust source, not from reading those artifacts. No commands were
executed against project tasks, and no tests, builds, formatter, or linter
were run by this observer. The only execution evidence inspected was the
permitted `decision-tests.log`, described at the end.

Statements about code paths are direct source observations. Identified risks
are interpretations of those paths; unexecuted reproduction cases are labelled
as such. Called validators and configuration/discovery/VCS/sandbox backends
outside the inspected source were not independently audited.

## Public interfaces and presentation

`kvist decision COMPONENT_DIR` requires a visible component argument and defaults
to `show`. `decisions::execute(project_dir, component, action)` is the shared
library entry point used by ordinary CLI dispatch, JSON dispatch, and Bark.
Its serializable `Output` has `message`, `markdown_path`, `plan_id`, and
`approval`; the latter three are optional.

| Action | Observed semantics |
| --- | --- |
| `show [TASK_ID]` | Reads the queue and lists blocked/awaiting-decision tasks when there is not exactly one pause and no explicit task. An explicit task may be any existing task, not necessarily paused. Shows recorded reason, response instructions, an attention-file pointer when present, and retained options/discussion Markdown links. Does not authenticate an applied receipt or diagnose pending-journal contents. |
| `options [--task TASK_ID]` | Captures a paused revision, asks an architect model for structured alternatives, validates them, and saves advisory JSON/Markdown plus history. Does not change controlled intent or task state. |
| `discuss MESSAGE [--task TASK_ID]` | Sends a bounded question and retained conversation to a fresh model request and persists the appended exchange. Does not resolve a pause. |
| `plan [--task TASK_ID] [--option ID] [--guidance TEXT] [--import PATH] [--review-exception REASON]` | Captures inputs, obtains/imports a strict typed draft, obtains advisory review or records an explicit exception, validates host-produced outputs, and retains exact JSON/Markdown. `--option` conflicts with `--import`; guidance can accompany either. Without import, a choice or meaningful guidance is required. Import without an exception still asks a model for review. |
| `apply PLAN_ID --approve SHA256 [--continue]` | Requires exact saved-plan authentication, an unchanged generated preview, and the exact approval token. Applies or recovers the retained transaction without model inference. Continuation is a separate ordinary run after application. |

JSON decision success is an envelope with `status: "success"`,
`command: "decision"`, and `result: Output`. Errors from `execute` are wrapped
as `status: "error"`, `command: "decision"`, and an error string; preceding
project/component resolution occurs outside that wrapper. Human/Bark dispatch
prints `Output.message`.

Generated component arguments are single-quoted with embedded apostrophes
escaped; leading-hyphen paths acquire `./`. Status prints decision-specific
`show` instructions and reasons. Bark completion tests exercise the editable
component slot before `plan` and retention of `--task` scope. Shell smoke source
exercises default inspection for `.` and `./-component`, without a component
selection prompt.

## Model authority and context

Decision support uses `agent.architect` and
`ReviewSession::decision`, with the architect role, streaming disabled, no
private control server, no live guidance, and no tools. Signal cancellation
and model time/token/retry accounting remain active. A non-working/none model
fails clearly; manual import plus a review exception and inspection/application
do not need a model.

Each `ask` creates a new session. Draft generation and intent review therefore
use distinct sessions, but the same configured architect profile. Retained
discussion is explicitly supplied data, not provider conversation memory.
Model requests consist of system instructions and one evidence message, an
empty tool list, `ToolChoice::None`, and a strict outer response shape:
`report` (nonblank string) and `decision_required` (boolean). Returned tool
calls are rejected without execution. `ask` uses only `report`; the advisory
boolean does not authorize, veto, resolve, or certify anything.

Evidence contains:

- The selected full task, local requirements/contract/design text, root contract
  text, and the immediate declared ancestor contract where applicable.
- A queue graph projection of IDs, titles, kind, status, dependencies, and
  requirement references, not every task's complete authored details.
- Markdown from attention and continuous-review directories whose filename
  starts with the selected task ID, and all Markdown authoring proposals.
- The captured revision's persisted discussion; planning additionally supplies
  the selected substantive alternative and human guidance. Review receives the
  proposed draft, not a recomputed final canonical output queue.

Source/tests are hashed by the host but are not supplied as model source
evidence. Vision/architecture bytes bind a plan but their text is not inserted
in this decision-model frame. The system explicitly labels supplied artifacts,
previous model answers, and other evidence untrusted and forbids accepting work
or modifying project files. This is a tool boundary and prompt framing, not
proof that an advisory answer is correct or free of quoted source fragments
already present in retained Markdown.

### Concrete support limits and persistence

- Catalogs contain 1–32 alternatives. Summary, title, consequences, and
  recommendation must be nonblank and bounded at respectively 8,192, 1,024,
  8,192, and 8,192 bytes. IDs are distinct 1–128-byte ASCII
  alphanumeric/hyphen/underscore identifiers.
- Questions and original guidance are limited to 16 KiB. Explicit review
  exceptions are nonblank and limited to 8,192 bytes.
- Conversations are version 1, at most 32 exchanges and 256 KiB serialized
  JSON; each retained answer is nonblank and at most 65,536 bytes. Attempting
  another exchange at the limit fails without truncating prior history.
- Decision response/draft parsing and individual support JSON/Markdown files
  have 1 MiB limits. Evidence and support inspection have 8 MiB aggregate
  limits; evidence enumeration and support-index enumeration have 4,096-entry
  bounds. Model capacity checks may impose a smaller practical limit.
- Decision phases reject oversized context instead of fragmenting/truncating
  it. Response reserve comes from model configuration or a context-derived
  default; absent context configuration uses a provisional 32,768-token
  capacity calculation. Actual request-size checking also occurs.

Support records use a `decision-<capture-hash>` revision prefix. Capture
identity excludes the current options/discussion bindings so successive
conversation/option aliases retain that revision identity. Immutable history
files include a content digest; mutable latest aliases are also written.
`save_support` acquires the component operation lock, checks the pending fence,
and compares the current JSON alias with the pre-inference bytes. A concurrent
JSON update causes refusal rather than silent history loss.

History and aliases are written JSON then Markdown, not as one transaction.
History collisions with different bytes fail. The function does not refresh
the entire capture after inference or revalidate the operation lock before
each support write. Its support files are not HMAC-authenticated. A final plan
does bind the exact current catalog/discussion JSON bytes; their support
Markdown is not itself an input binding.

## Draft validation and queue effects

`Draft`, `TaskDefinition`, and `TaskEdit` deny unknown fields. Draft version
must be 1 with nonblank rationale; serialized draft size is bounded. Required
draft fields are `schema_version`, `rationale`, `documents`, `tasks`, and
`disposition` (`resolve` or `withdraw`). Empty maps/arrays are accepted.

Documents are complete replacement text for only local `REQUIREMENTS.md`,
`CONTRACT.md`, and `DESIGN.md`. Other names and traversal paths are rejected.
Text passes the called native structural validator and must have no recovery
blockers. The validator's complete heading grammar was not independently
inspected in this observation.

Task definitions contain exactly `id`, `title`, `description`, `context`,
`purpose`, `expected_outcome`, `kind`, `depends_on`, and `requirements`.
Operations are tagged `add`, `update`, or `retire`. They cannot supply raw
queue/component state, lifecycle status, timestamps, recovery state,
acceptance identity, or finalization disposition. Duplicate/conflicting edits
for an ID fail.

- Add requires a new queue-local ID. New tasks are pending, with host-created
  creation/update timestamps, no completion/recovery/acceptance/disposition,
  and `recovered: false`.
- Update requires an existing pending task or the selected blocked/
  awaiting-decision task, and no recovery fence. Only authored fields and the
  update timestamp change. Completed, live, and unrelated paused tasks cannot
  be updated. Existing host-owned metadata is otherwise retained.
- Retire requires a known uncompleted, non-live, non-fenced task and a
  nonblank reason of at most 4,096 bytes.
- Resolve rejects any retirement, reopens the selected task as pending, clears
  its reason/completion timestamp, and adds **every** newly added task as its
  prerequisite. Additions are inserted immediately before it in authored
  addition order. Selected dependencies are sorted/deduplicated.
- Withdraw computes the transitive dependant closure from the **original**
  queue, starting with the selected task. Explicit retirements must equal that
  closure and include no unrelated task. Full original task records and exact
  reasons are retained in the plan/receipt; active queue records are removed.
  Neither source nor attempt evidence is removed. Completed dependants make
  such withdrawal fail rather than become fictitiously uncompleted.

Both dispositions recalculate local document SHA-256 revisions and the
immediate-parent binding, set revalidation current with the plan timestamp, and
clear stale evidence. Only changed controlled-file bytes enter `outputs`.
This can accept structurally valid local document staleness; stale
immediate-parent authority must first be separately revalidated.

Canonical queue serialization validates the result:

- IDs: 1–64 lowercase ASCII letters/digits/hyphens, no leading/trailing or
  doubled hyphen. Titles: trimmed, nonblank, one line, at most 120 characters.
  Four detail fields: trimmed, nonblank, at most 4,096 characters.
- Dependencies and requirements: lexically sorted, unique, nonblank entries.
  Requirement strings need a nonblank source and `#` locator; they may be an
  empty list. The validator does not resolve the locator against document text.
- Dependencies must exist, not be self-references/cyclic, and refer only to
  earlier declared tasks.
- Implementation needs a transitive test predecessor; security-audit needs
  implementation; compliance-review needs security-audit. These are
  predecessor-existence checks, not a requirement that each addition introduces
  a fresh complete four-task lifecycle or shares one item stem.
- Timestamps and lifecycle/reason/recovery invariants are validated. Completed
  task records are preserved by these edits, although the resulting queue is
  serialized canonically rather than preserving its original YAML formatting.

## Exact identity, authentication, and input binding

Plan identity is computed from the entire plan after clearing `id`, `approval`,
and `authentication_tag`, converting to a JSON value, and compact-encoding it.
The ID is lowercase hex of SHA-256 over
`"kvist decision plan v1\n" + JSON`. Approval is `sha256:<hex>` over
`"kvist decision human approval v1\n" + JSON`. This includes creation time,
capture, selection/guidance, rationale, review/exception, output bytes,
retirements/reasons, and disposition. Identical drafts prepared at different
times need not have identical IDs.

Plans, pending journals, and applied receipts are HMAC-SHA-256 authenticated
over their JSON-value form with the authentication-tag field cleared. The
actual helper's emitted tag is `hmac-sha256:sha256:<hex>`: `hmac_sha256` itself
returns a `sha256:`-prefixed value. Token/tag comparisons XOR all bytes when
lengths match. A locally edited ID/approval alone cannot authorize changed plan
content.

The 32-byte shared approval secret lives outside the project under
`kvist/approval-v1/approval-secret`, rooted in `XDG_STATE_HOME`, then
`HOME/.local/state` on non-Windows, or `LOCALAPPDATA` on Windows. Decision
storage requires an absolute state base, safe non-link paths, a regular
single-link secret, and matching reread bytes; the shared secret reader checks
32-byte length and user-private Unix permissions. Plan/load/apply paths use
load-or-create; ledger/retired-evidence verification uses an existing secret.
Authentication is tied to this user state, not a portable project-only token.

Capture stores exact local requirements/contract/design/queue text, the parsed
full queue and selected task, normalized component path, and these bindings:

1. Exact bytes of root `kvist.toml`, `VISION.md`, `ARCHITECTURE.md`, and
   `ROOT_CONTRACT.md`, each required and checked by the called project-artifact
   recovery validator.
2. A digest of resolved component root, discovery bounds, VCS setting, agent
   settings, test policy, sandbox settings, and task-run settings.
3. Absolute project/component path identities.
4. Serialized nearest ancestor component contract path/revision. Transparent
   namespace directories can be skipped; the first ancestor with any managed
   component marker requires a valid contract. The queue's recorded parent must
   match.
5. Recursive byte digests/absence/directory markers for local `src`, `tests`,
   `IMPL.md`, `.kvist-attempts`, `.kvist/continuous.json`,
   `.kvist/continuous-reviews`, `.kvist/authoring/proposals`,
   `.kvist/attention`, `.kvist/reviews`, `.kvist/evidence`, `reports`,
   `COMPLIANCE.md`, `SECURITY.md`, `Cargo.toml`, `Cargo.lock`, `build.rs`, and
   `.gitignore`.
6. Exact current options/discussion JSON alias digests (or absence), using the
   capture revision identity. These aliases remain bound to the original
   revision during interrupted recovery.

Directories encountered with local component-marker files are recorded as
`component-boundary` without descending into their implementation. Inventories
are sorted, limited to 10,000 budgeted entries and 8 MiB captured data, with
depth at most 64 and relative path length at most 4,096. Individual files must
be regular, UTF-8 where text is required, and no larger than 1 MiB. Symlinks,
special files, parent traversal, and Unix regular-file hardlinks are rejected.

Capture refuses a pending decision, any operation lock, any live/fenced task,
unresolved known attempts, and unknown attempt JSONL unless authenticated
retirement recognition succeeds. Continuous ledgers must authenticate, have no
pending checkpoint, and be paused or awaiting-approval.

Prepare refreshes and byte-compares capture before constructing outputs and
again before persisting a plan. Plan JSON and its generated preview are bounded
individually; their aggregate with original and output text must fit 8 MiB.
History is retained without clobbering, Markdown before JSON. There is no
operation lock around the complete prepare procedure.

Load validates versions, 64-character lowercase-hex ID, recomputed ID/approval,
HMAC, component and project identity, exact original controlled-file scope,
presence of output queue, and output native validation. It regenerates and
byte-compares the Markdown preview; missing/edited preview refuses approval.

## Transaction ordering and recovery

Application first loads/authenticates the saved plan and approval, then
authenticates pending progress and requires its embedded plan to be identical.
The component operation lock is acquired/recovered before controlled mutation.

The user-owned lock is outside the project under `kvist/task-locks-v1`, keyed
by canonical project/component identity. It has schema, timestamp, task ID,
PID, and random nonce. Ordinary acquisition is no-clobber. Recovery can create
a missing lock; takeover of an existing lock requires exact authenticated
contents digest, `decision-apply` operation identity, a valid record, and an
apparently dead owner. Quarantine, reread/identity comparison, and directory
sync protect takeover/release against changed owners. Lock ownership is
revalidated repeatedly during application.

When no applied receipt exists:

1. Refresh capture. All non-controlled bindings must equal the original.
   Controlled files must equal original bytes, or, in recovery, one of that
   file's original/exact planned output bytes. Current and original queue
   recovery guards and known/retired-journal inventory checks also run.
2. Publish authenticated `pending.json` with the full plan, initially empty
   completed-file set, and lock digest. On recovery, update its lock digest and
   authenticated progress before output iteration.
3. Iterate `outputs` in `BTreeMap` order: `CONTRACT.md`, `DESIGN.md`,
   `REQUIREMENTS.md`, then `TODOS.yaml`, for whichever keys changed. Revalidate
   lock and whole capture before each file.
4. If bytes already equal output, avoid rewriting. Otherwise require exact
   original bytes and no recorded completion for that name; stage, sync, check
   original bytes and lock again, atomically replace, sync the parent, and
   reread exact output bytes.
5. Mark the name completed and durably rewrite the authenticated journal.
   Journal updates compare the currently stored authenticated tag with the
   transaction's remembered tag.
6. Reverify bindings and all exact outputs. Publish a no-clobber authenticated
   `<plan-id>.applied.json` receipt.
7. After a lock check, reread/authenticate the matching pending journal and
   remove it, syncing the parent directory. Release the operation lock.

The receipt records application time, the full plan, stale source-derived
evidence and follow-up text. `execution_accepted`, `compliance_certified`,
`model_findings_accepted`, and `git_published` are all false. `IMPL.md` and
source-derived reports are not rewritten; neither implementation nor findings
are certified by the receipt.

This is durable per-file replacement plus a recoverable journal, not atomic
multi-file visibility. Failure after a replacement but before its progress
record can recover from exact after-bytes. A completed write externally
reverted to before-bytes is refused when encountered, rather than reapplied.
Any third-state controlled bytes, bound source/evidence/config/root/parent
drift, tampered progress, or a different plan blocks recovery. Earlier writes
are not rolled back. Readers without the fence can see intermediate states.

Linux decision storage pins real directory ancestors using `openat` with
`O_DIRECTORY|O_NOFOLLOW`, operates through `/proc/self/fd`, and verifies
directory identity. Reads compare file identity, length, mtime/ctime around
opening and reading. Controlled writes use same-directory staging and sync.
Non-Linux directory handling and non-Unix identity/hardlink checks are weaker;
no cross-platform execution evidence was inspected. These mechanisms do not
make arbitrary external writers obey the component lock or provide a globally
atomic capture.

### Applied-receipt idempotence is historical, not current-state attestation

`transaction.rs:326–353` authenticates and identity-checks an existing receipt,
including its stale-evidence marker and false approval/publication flags.
**With no pending journal, it returns success without rechecking current
controlled outputs, source, root bindings, queue, or attempt state.** The
plan/preview, current component identity, approval, secret, and operation-lock
requirements still apply.

This preserves later legitimate execution: repeating an old application does
not overwrite a now-completed queue or newer implementation and does not
change receipt bytes. The dedicated unit test models later execution by
directly modifying source and a completed queue, rather than actually running a
task. The branch also cannot distinguish legitimate later changes from
arbitrary later edits. Success means “this exact application is already
recorded,” not “the component still equals that plan.”

**With pending progress**, even an existing applied receipt requires full
recovery binding checks and exact final outputs before cleanup. Thus receipt
publication alone does not permit drifted recovery or remove the fence.

## Which inspected writers actually fence

The pending guard checks safe-path existence of
`.kvist/decisions/pending.json`; it does not decode/authenticate the journal.
Malformed or unauthentic pending state still fences ordinary guarded writers.
Only matching authenticated application provides normal decision cleanup.

| Inspected path | Pending-decision behavior |
| --- | --- |
| Ordinary transition; component intent acceptance; task execution; continuous run | Fence through component-attempt guards before queue/execution mutation, with the component operation lock. |
| Task recovery and accept/block finalization | Fence through the recovered-except/other-task guards, including when an ordinary attempt is permitted for recovery/finalization. |
| Repair planning/applying | Relevant evidence guards fence queue repair; `lock_for_repair` also checks the pending marker before acquiring each repair lock, including clean-slate application. |
| Decision capture/options/discussion/plan | Capture fences; support persistence checks again under the operation lock. |
| Decision application/recovery | Authenticates matching pending state and uses the transaction protocol rather than the ordinary fence. |
| Task `next` | Although not a queue writer, it also uses the component-attempt guard and refuses pending state. |
| Task unlock | May remove a confirmed stale operation lock **before** the component-attempt guard reports pending/unresolved state. It does not delete the decision journal or approve/replay tasks. |
| Attention/stop-diagnostic persistence | No pending guard or task lock in `attention::persist`; creates/replaces bound attention history and aliases, even for preflight failures. See concrete recovery risk below. |
| Generic file helpers | No decision fence; guards are caller responsibility. |
| Public `verify_task` helper | No direct pending guard/operation-lock acquisition in this helper; inspected normal task execution calls it inside a guarded locked attempt. This observation does not prove safety for arbitrary library callers. |
| `commit_accepted` wrapper | Directly delegates to VCS acceptance-record functions without a decision fence in the inspected wrapper. Backend guards were not inspected; no claim of backend bypass is made. |
| Model diagnostic logs/live events | Decision sessions write these without the task lock; `.kvist/logs` and `.kvist/runs` are intentionally outside this capture inventory. |

This is not an inventory of every writer in Maerg or other components.

## Withdrawal and retained journal recognition

Unknown `.kvist-attempts/*.jsonl` files are acceptable only when the exact file
is recognized through an authenticated applied receipt for that component.
Recognition limits directory inventory to 10,000 entries and receipt text to
8 MiB, sorts receipts, uses the existing user secret, and hashes the journal.
It checks receipt/plan authentication, recomputed plan identity/approval,
receipt filename, and the absolute component binding.

For a matching retired ID it requires withdraw disposition, full archived task
equality with the original queue record, a nonblank retained retirement reason,
the original exact journal digest, and an uncompleted/non-live/non-fenced
archived task. Changed journal bytes or a copied receipt from a different
component fail. A merely staged withdrawal has no recognition authority.
Recognition does not parse that retired journal's contents anew: authorization
is archival identity/equality; the original capture already passed its attempt
guards. Invalid receipts encountered during the scan can stop recognition.

Current active queue IDs are checked through ordinary attempt assessment;
retirement recognition is relevant to otherwise unmatched journals in capture
and repair. The ordinary component-attempt guard iterates active tasks rather
than independently enumerating every unknown journal.

## Continuation, commits, and stop reporting

Successful non-continuing apply invokes no model or Git publication. Application
success says intent/scheduling was accepted, not implementation/findings.
`--continue` calls `run_task_or_item(..., None, false)` after the receipt and
cleanup, selecting the ordinary **continuous component run**, not only the
original paused task. Private controls are off. Ordinary execution policy,
sandbox/test preconditions, dependency readiness, and recovery guards apply.

After accepted definition changes, a paused continuous ledger with no pending
checkpoint can update its definition digest and active item. The queue
determines prerequisite ordering. Item/component commit policy can subsequently
produce ordinary **local unaccepted checkpoints**; manual policy does not.
Therefore “no Git commit was created” describes decision application itself,
not a guarantee that `--continue` cannot create a checkpoint. There is no
decision-specific push/publication path in inspected code.

If continuation errors, the command reports that the plan was already applied
and recorded and its effects remain. It does not roll back the decision.
A continuous pause can also be a successful command result: when a task stops
awaiting-decision, later tasks are not started and the ledger is paused.

Decision support emits bounded redacted model/profile/phase/budget diagnostics
to stderr and JSONL/live records. Diagnostic writes are synchronized and subject
to configured output limits; finish/close failures propagate, and unfinished
session close failures are logged. Engine errors generally use
`TaskQueueUnavailable` with a `decision:` prefix; support errors use
`AgentEvidenceFailed` with `decision support:`; contextual I/O errors preserve
operation/path information. Success is withheld if operation-lock release
fails, even if receipt creation already succeeded.

Ordinary surfaced decisions write the human attention brief before transitioning
to awaiting-decision and point to raw Markdown/JSON proposal/review evidence.
If brief persistence fails, retained execution evidence remains unresolved
rather than being silently completed. Ordinary run failures attempt another
host-written attention brief without further model inference, preserve the
cause, and report brief-persistence failure as an additional diagnostic.
Bounded excerpts can be explicitly labelled truncated; this is distinct from
the refusal to truncate decision context/history.

## Concrete source-derived gaps and risks

### High-confidence recovery-interference bug: fenced run writes bound evidence

**Source-derived, not executed by this observer.**

- `maerg/src/task_commands/continuous.rs:49–51` maps every `run_inner` error to
  `attention::failure`. Explicit-task execution has the corresponding mapping
  at `maerg/src/task_commands.rs:4473–4484`.
- `maerg/src/task_commands/attention.rs:187–190,203–285` persists the failure
  brief by creating historical Markdown and replacing the latest alias, with
  no pending-decision fence or operation lock.
- `maerg/src/task_commands/decisions/engine/capture.rs:346,440–456` captures
  `.kvist/attention` and rejects changed bindings even during recovery.

Reproduction case for an isolated fixture: prepare a valid decision, interrupt
application after one durable replacement using the existing unit-test
`transaction::apply(..., Some(1))` hook, leaving authentic `pending.json`.
Invoke ordinary `task run COMPONENT` while pending (a queue-only resolution
also suffices after an interrupted queue replacement). The ordinary run
refuses execution, but its error wrapper persists a new `run.<digest>.md` and
`run.md` under `.kvist/attention`, provided that directory is writable. Repeat
the original exact approved apply. Recovery now fails the binding comparison
because the host itself changed captured attention evidence. Source, approved
plan, and controlled files can otherwise still be valid before/after states.

The queue/execution fence remains effective, but a diagnostic side effect
blocks the advertised identical-plan recovery path. New capture is also fenced
while pending. Restoring all exact captured evidence would be needed before
that path succeeds; this record does not recommend deleting evidence. Existing
tests check guard refusal directly, not this error-wrapper interaction.

### Other bounded observations, not proven exploits

- Captured paths form an explicit whitelist. Other local files, environment
  values not reflected in resolved configuration, and arbitrary files outside
  that inventory are not bound. Nested component implementations are
  deliberately excluded. This is scoped authentication, not proof that every
  execution-relevant input is frozen.
- Native validation enforces structure and dependency predecessor kinds, not
  rationale correctness, requirement-locator existence, or fresh lifecycle
  coverage for every document change. A document-changing resolve with no
  additions can pass this path.
- Retained advisory Markdown may contain source snippets or unrelated
  task-prefix matches; no semantic source-content filter exists in the inspected
  evidence builder. “No source access” means no source tools/raw source frame,
  not a guarantee that retained prose contains no source.
- Regenerating a lost secret in load/apply does not recover old authentic
  history: tags fail under the new secret. Missing/malformed plan or preview
  also blocks pending recovery because load precedes journal recovery. No
  repair/rekey fallback for such decision state was observed.
- Pending state is an existence fence, and recovery refuses any bound drift.
  Deliberate fixes to root/parent/config/evidence after interruption do not
  automatically authorize a revised transaction. No alternate pending-abandon
  decision command appears in `Action`.
- Support JSON/Markdown alias publication and plan Markdown/JSON persistence
  have partial-publication windows. Application will not accept a missing or
  mismatched plan preview; support inspection does not authenticate its
  advisory Markdown companions.
- Applied receipts intentionally permit later-state idempotence without
  current-output attestation. Repeating `apply --continue` also repeats the
  request for ordinary continuation; receipt idempotence does not make
  continuation a one-shot action.

## Tests and execution-evidence limitations

The read decision unit tests assert typed-field/path rejection; catalog IDs
and more than three choices; concurrent support alias refusal; link/UTF-8 and
capture bounds; exact output staging; wrong approval/preview/plan tampering;
source/root/config/evidence/parent drift; ambiguous pauses; preserved completed
records and Git index; valid prerequisites; invalid lifecycle/dependency edits;
withdrawal closure, archived records/reasons, and completed/live refusal;
authenticated retired-journal recognition; pending/ledger authentication;
before/after interruption recovery; receipt-before-cleanup recovery; and exact
dead-lock takeover. Context tests exercise current options/discussion alias
creation and edits invalidating application without changing revision identity.

`applied_receipt_is_idempotent_after_later_legitimate_execution` directly
rewrites fixture source and queue after successful apply, then asserts receipt,
newer source, and newer queue are preserved by repeat application. This is
strong branch coverage for historical idempotence, not proof of an actual later
runner execution or arbitrary concurrent-state safety.

Linux-gated integration test source asserts:

- `decision_agent_options_discussion_and_plan_never_apply_unseen_effects`:
  five mock alternatives, retained follow-up discussion, no controlled writes
  before apply, advisory review request, no model call during application,
  and absence of a particular source-marker function from requests.
- `manual_decision_plan_and_inspection_need_no_model_and_bind_exact_approval`:
  offline import plus exception, inspection, wrong-token refusal, and valid
  JSON application.
- `applied_decision_continues_prerequisites_before_retrying_a_real_paused_review`:
  a real mock-model review pause, four inserted investigation tasks, completion
  of twelve tasks, investigation review before the resumed review, and
  `attempt-0002` rather than fictitious completion.
- Continuous pause/authoring tests: later items remain pending; retained
  attention/proposal/review Markdown contains response steps; failed brief
  persistence preserves the original stop and unresolved attempt.

These integration cases were inspected as source; their execution was not
independently established by the available log. Failure injection covers
selected durable boundaries, not every filesystem error or real power loss.
The inspected tests do not cover the fenced-run diagnostic interference above.

The permitted log at
`/home/stefan/.copilot/session-state/778fd089-f61b-4bbd-8891-e55cc9d22944/files/decision-tests.log`
was inspected through its ending at lines 469–487. It records a 574-test group
with **573 passed, 0 failed, 1 ignored**, numerous subsequent passing groups,
and a two-test shell-output group with **2 passed**. Quiet output does not name
most tests or their test binaries, and the command invocation is absent.

It then records **status failure**, not whole-suite success:
`status_blocked_details_list_reasons_and_exact_next_commands`, with 13 passed
and 1 failed in a 14-test group. The logged assertion at
`maerg/tests/status.rs:659:5` expected
`next:   human decision needed; 'kvist help task-states' explains resuming`.
The current status test source read here instead expects a decision-specific
`show` command. That source/log difference does not establish that a rerun
passed. The log ends with `error: test failed, to rerun pass -p kvist --test status`;
no subsequent decision integration-group or full-suite completion was visible.

Formatting, all-target strict Clippy, and release-build results were not
independently inspected. No clean source-only observation can derive their
success, current full-suite success, or compliance from the evidence above.
