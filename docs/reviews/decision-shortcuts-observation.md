# Numbered decision responses: bounded source observation

Observed 2026-10-31. This is a fresh, narrow source-derived diagnostic, not a
native receipt, task disposition, compliance certification, or whole-component
review. No production code, tests, live decision state, or task status was
changed. No Cargo commands, models, tasks, or delegated observers were run.

## Evidence boundary

Read only current implementation/test evidence in:

- `maerg/src/task_commands/decisions.rs`, `decisions/types.rs`, and
  `decisions/engine/{shortcuts,storage,capture,transaction}.rs`;
- `maerg/src/bark.rs`, `bark/{state,completion,prefill}.rs`, relevant CLI
  dispatch/grammar in `maerg/src/cli.rs`, and generated response guidance in
  `maerg/src/task_commands/attention.rs` and `maerg/src/help.rs`;
- the four requested numbered-response integration tests in
  `maerg/tests/task_commands.rs`, the PTY interaction in
  `maerg/tests/shell_smoke.rs`, and
  `numbered_approval_recovers_identical_interrupted_apply_without_replanning`
  in `decisions/engine/context_tests.rs`.

No intent artifacts, prior observations/reviews, Git history/diffs, guides, or
implementation records were consulted. Functions outside this boundary were
observed as calls, not independently audited.

## Implemented registry and dispatch

`shortcuts.rs` retains component-local `.kvist/decisions/shortcuts.json`:
version 1, absolute project/component identity, `next_number`, a numerically
ordered map, and an authentication tag. The only stored command variants are
`OptionPlan` and `ApplyPlan`; unknown fields are rejected. Option entries retain
task, capture identity, exact catalog-text digest, option ID, and title. Apply
entries retain task, exact plan ID, exact approval token, and continuation flag.

Within a retained registry, allocation starts at 1, deduplicates the complete
typed command, and only increments for a new command. Different continuation
flags and changed option catalogs receive different entries. Numbers are never
removed or reused by this code. Read validation rejects zero, invalid
`next_number`, keys outside its range, more than 2048 entries, identity mismatch,
or a bad HMAC-SHA256 tag. Arithmetic exhaustion and the 2048-action limit fail
with full-command guidance, without publishing the partially updated registry.
Registry JSON is additionally limited to 1 MiB.

Storage guards check real non-link ancestors, regular single-link input files
on Unix, bounded UTF-8 reads, and file/directory identity around I/O. Linux uses
pinned directory descriptors. Authentication uses user-owned approval state
outside the project; registry reads require an existing secret. Publication
holds the component task lock, revalidates it, fences pending decision
transactions, and atomically persists/replaces the registry.

The grammar is `decision COMPONENT actions` or `decision COMPONENT do NUMBER`;
the component is required and the number parses as positive `u32`. There is no
bare-number shell shorthand or latest-plan fallback. `execute` authenticates
the registry and matches its typed command directly into `Action::Plan` or
`Action::Apply`. The displayed command string is never evaluated, interpolated
into a shell, or used as executable authority. Shell dispatch tokenizes the
submitted line, parses the native Clap grammar, resolves the component, and
calls the same decision executor.

## Choice freshness, exact approval, and continuation

A numbered choice supplies an additional capture/catalog-digest pin to
`prepare`. Capture requires a known paused task and no ordinary recovery fence,
live task, unrecovered attempt, operation lock, or pending decision transaction.
Before the planning model is called, the current catalog must match the pinned
capture and exact text digest, supported schema, task, and selected option.
The substantive option is included in model evidence; its number does not
authorize application. Discussion can precede selection because the stable
capture ID excludes the changing decision-support bindings, while the eventual
plan binds the then-current options and discussion. Transaction preparation
recaptures before and after drafting/review to reject intervening changes.

Choosing generates a complete frozen plan and preview plus two separate
approval responses: apply and stop, or apply and continue ordinary execution.
It does not change controlled files. Numbered planning offers no import,
guidance, or review-exception override; it follows the model planning and
advisory-review path. Manual import and explicit exceptions remain full-command
features.

Application loads the exact retained plan and preview, validates plan identity,
content-bound approval, authentication, component/project bindings, and output
scope. Initial application checks captured source/evidence/configuration and
controlled inputs; interrupted recovery accepts only the identical approved
plan and known before/after bytes. The authenticated journal records progress;
the receipt marks source-derived evidence stale and does not certify execution,
compliance, findings, or Git publication. A valid already-applied receipt makes
application idempotent.

Continuation is a subsequent `run_task_or_item(..., None, false)` call, not a
forced retry of the paused task. It selects ordinary component work, allowing
prerequisites first. If it fails, the error explicitly says the plan was already
applied and its effects remain. Exact application itself performs no model
inference or Git commit; optional continuation can execute ordinary work.

## Discovery and presentation

`actions` scans retained lowercase 64-hex plan JSON names, excluding those with
an existing applied-receipt filename. Candidates are sorted and loaded through
plan/preview authentication before both approval variants are registered.
Scanning is bounded to 4096 directory entries and 8 MiB of encoded plans.
Discovery does not infer a decision or call a model. With `pending.json`, it
only lists existing responses: it does not publish new numbers while fenced.
Completion uses read-only registry listing, not plan discovery.

JSON success contains `status`, `command: decision`, and `result` with message,
Markdown path, plan ID, approval, and actions; each action contains number,
description, and displayed command. Plain CLI and shell render the same message.
`do` echoes the expansion to stderr, including under `--json`; continuation
progress is likewise separate from the tested JSON stdout. Plans/options and
`actions` display numbered instructions. Single-task `show` lists retained
responses; the zero/multiple-pause early return does not. `discuss` does not
return actions. Saved option Markdown is written before the numbered section
is appended to the command response.

Completion snapshots authenticate registry entries independently per component,
warn on unavailable shortcut state, and refresh between REPL submissions.
`RESPONSE_NUMBER` candidates require the explicitly typed component scope,
normalize `./` spelling, prefix-filter numbers, and show description plus
expansion. They do not fall back to shell focus or another component's numbers.
The adapter supplies replacement spans to Reedline. Component prefill edits
the real buffer, respects explicit/partial arguments, and disambiguates
dash-leading and command-named components.

## Test evidence versus runtime evidence

Current test source asserts:

- `numbered_decision_responses_keep_exact_plans_without_hash_copying`: two
  plans receive distinct numbers; choice of the first exact plan never selects
  the latest; unknown numbers preserve the queue; repeat application succeeds;
  a later stale plan fails; JSON contains the expected identifiers and tokens.
- `numbered_options_refuse_retargeting_and_prepare_the_exact_selected_choice`:
  replacing an option catalog gives a new number; the old number fails before
  inference; discussion then current selection includes the selected revision;
  planning preserves the queue; registry continuation tampering is rejected.
- `actions_discovers_preexisting_authenticated_plans_without_inference`:
  after deleting the index, authenticated retained plan discovery and numbered
  application work with an unreachable model endpoint.
- `applied_decision_continues_prerequisites_before_retrying_a_real_paused_review`:
  the continuing number completes the extended queue, runs investigation
  review before the original paused review, emits parseable JSON stdout, and
  retries the original review with a second attempt rather than marking it done.
- The numbered recovery unit test injects interruption after a durable write,
  invokes the original approval number, and verifies the pending fence clears.
- Shortcut unit tests assert deduplication/scope, tampering/unknown-field/link
  refusal, publication fencing, and action/file bounds. Completion unit tests
  assert candidate numbers, descriptions, prefix matching, and explicit scope;
  they do not themselves prove a numbered candidate changed an editor buffer.
  Prefill unit tests do inspect real Reedline buffer/cursor and undo behavior.
- The PTY test types `decision`, observes real selected-component insertion,
  opens the response menu with Tab, accepts response 1, and checks the submitted
  command in the shell journal. The deliberately stale plan is refused without
  queue mutation or a confirmation prompt. All phases must complete. This
  exercises real numbered completion/submission, not successful PTY application.

The permitted `decision-shortcuts-tests.log` was read once, only its initial
range. That snapshot showed an in-progress 595-test run and
`checkpoint_transaction_deadline_covers_every_blocked_reply_and_ref_retry ---
FAILED`. The output was truncated and contained no final suite summary in the
range observed. No numbered-feature test pass or complete Cargo success is
claimed here; the checkpoint failure is reported without attributing its cause
to this feature. The assertions above are source evidence, not passing runtime
evidence.

## Concrete remaining limits and correction candidate

1. **Displayed option “Exact command” is not replay-equivalent.** In
   `shortcuts.rs::expansion`, an option expands only to
   `plan --task TASK --option ID`. `decisions.rs::execute` additionally installs
   the stored capture/catalog pin for `do`, but the native full-command grammar
   has no equivalent pin argument. After replacing the same-capture catalog,
   `do OLD_NUMBER` rejects it while copying its displayed “Exact command” can
   select the new content under the same ID. Numbered dispatch remains guarded;
   the high-confidence defect is the promised equivalence in Markdown,
   completion descriptions, JSON `command`, and stderr expansion. Parent
   correction should distinguish a descriptive command from pinned replay, or
   provide a genuinely equivalent pinned surface.
2. Monotonic numbering depends on retaining the current registry. Absence
   starts a new registry at 1, as the discovery test deliberately demonstrates.
   There is no separate tombstone/high-water mark or authenticated-rollback
   detection. Restoring an older authentic registry can also discard newer
   assignments. This is a retention boundary, not evidence of reassignment
   inside an uninterrupted registry.
3. Historical/stale/applied entries remain listed and completable; listing
   authenticates them but does not label current freshness. Discovery validates
   plan authentication, not whether its capture remains applicable. Invocation
   is where freshness refusal occurs.
4. Reinvoking an already-applied continuing response still calls the ordinary
   scheduler. With no pending journal, receipt validation short-circuits initial
   freshness verification. Thus continuation can run current work beyond the
   original plan's first application; it is not an exactly-once continuation.
5. No pruning or compaction surface exists in the observed code. Retained
   history eventually reaches action/discovery/file limits; full-command
   fallback remains necessary. Local authentication is not protection against
   an actor possessing the approval secret.

## Narrow correction addendum — 2026-10-31

Attribution: the same source-only observer performed this follow-up. Its scope
was limited to `types.rs::Action::Plan`,
`shortcuts.rs::{Command::OptionPlan,expansion,options}`,
`decisions.rs::{execute,prepare}`, and the integration test
`numbered_options_refuse_retargeting_and_prepare_the_exact_selected_choice`.
No broader re-review, Cargo execution, polling, or live-state changes occurred.
The earlier expansion-equivalence finding above remains a historical
observation of the source first inspected.

Current source corrects that specific discrepancy:

- `Action::Plan` now exposes native `--catalog SHA256`, requiring `--option`.
  `prepare` also rejects a pin without an explicit option for direct typed calls.
- `Command::OptionPlan` retains `catalog_digest` but no separate capture field.
  `options` derives that digest from the catalog text bytes.
- `expansion` now includes the quoted digest:
  `kvist decision COMPONENT plan --task TASK --option ID --catalog DIGEST`.
- Numbered dispatch constructs `Action::Plan` with the same task, option, and
  `catalog: Some(catalog_digest)` carried by that displayed command. Both paths
  pass this value to the same `prepare` parameter; there is no extra numbered-only
  freshness pin in the inspected dispatch.
- `prepare` locates the current capture's catalog and rejects an exact-text
  digest mismatch before planning inference. It still checks the catalog's
  embedded capture against the current capture ID, its task against the selected
  task, its schema, and its alternatives. Pinning the complete catalog bytes
  therefore also pins the embedded capture, without retaining it separately in
  the response command.

The revised test source saves the first displayed command, replaces the catalog,
and checks both the old number and the tokenized native expansion fail without
additional gateway requests. It then retains the current-number success,
selected-revision evidence, unchanged controlled queue, and tampering-refusal
assertions. This is direct-expanded-command regression coverage in source, not
a runtime pass observed here. The test does not separately assert successful
planning through a fresh expanded command.

Conclusion limited to inspected source: the previously reported missing-pin
expansion defect is corrected. This does not certify the feature or withdraw
the other earlier limits.

Coordinator-attributed runtime information only: the coordinator reports the
earlier full suite completed with reduced contention, with 982 passed and
7 ignored. A new final run after this correction is pending. Neither result was
independently read or executed by this observer; no post-correction Cargo
success is claimed.
