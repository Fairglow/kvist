# Agent resilience implementation plan

This human-directed plan addresses the November 6, 2026 review. Sav owns
mechanisms; Skott owns workspace integration/presentation; Maerg owns protected
task authority/evidence. Galla grants and productive uncapped native defaults
are unchanged. A phase commit records work, not human acceptance or a Kvist
review receipt.

| Phase | Work tasks | Required evidence | State |
| --- | --- | --- | --- |
| 1: Correctness | Honest Maerg loop stops; revision-bound read delivery; failed-observation semantics; rejected-proposal recurrence; repair error/decision/digest propagation | Failing regressions first, targeted passing checks, source-only observations and separate source-blind comparison | Complete |
| 2: Streaming | Bounded incremental exact/near repetition; independent answer/reasoning; 100/150/200-word periods; unbroken character loops; two autonomous corrections; intentional-output exception; no partial effects, sampling changes or reserve growth for loops | Chunk-boundary, false-positive, recovery, budget/cancellation/recording fixtures through both native hosts | Complete |
| 3: Progress | State-aware repeat protection; discovery/failure/observation tracking; bounded multi-action cycles; no-op resistance; fair batch intervention; pagination/reverification preservation | Alternating/long cycles, repeated refusals, changed-state testing, bulk investigation and compaction regressions | Complete |
| 4: Operations | Typed failures/recovery/effect certainty; actionable TUI/headless/transcript output; first-cause resource termination; aggregate authoring/repair model budgets; recurring repair signatures; cancellable backpressure; honest external capabilities | Integrated terminal/headless and protected-task evidence, offline lint/build/tests, independent final observations/comparison | Complete |

Each component phase has ordered `write_tests`, `implement_code`,
`security_audit` and `compliance_review` tasks in its adjacent `TODOS.yaml`.
Acceptance revisions are not rewritten to hide staleness. Advisory intent
review is recorded as ordinary feedback, not an implemented receipt.

## Recovery rules

Activity is not proof of progress. Useful evidence includes newly observed file
pages/revisions and changed verification diagnostics. Repetitive generation is
interrupted before accepting proposals; completed effects remain. Each stalled
prompt (or protected authoring-and-repair sequence) allows two real correction
opportunities, not one charge per duplicate call. Progress clears local streaks,
not spent corrections. Guidance and compaction cannot replenish that allowance.
Uncertain effects
require reconciliation, never blind replay.

Failure messages identify what failed, what remains known, the recovery action
and the next safe step. Missing offline resources are host prerequisites, not
permission to download or broaden tool authority. Explicit budgets retain their
resource identity. Opaque external-agent output is not native tool evidence.

## Evidence ledger

Ordinary clean-slate local advisory reviews found that universal semantic-loop
guarantees, correction reset rules and aggregate verification accounting were
underspecified. Those findings were acknowledged and the intent narrowed to
documented bounded heuristics, two corrections for the whole prompt/sequence,
four outer repair rounds and explicit resource/replay rules. No Kvist receipt,
component acceptance or compliance evidence is claimed by these reviews.

Phase-specific executed commands, regression results, independent observations,
comparison limitations and commit identities are recorded here as delivery
proceeds. Evidence does not claim exhaustive detection, compliance certification,
rollback or successful live-model qualification.

### Phase 1 evidence

Sav's repeated render/policy regression first failed with an incorrect
successful answer after 11 turns and 10 refused proposals. Attempt tracking now
includes refusals without inventing execution. Afterward:

- `cargo test --offline --locked -p sav -p skott --test loop_detection --test runtime_controls --test loop_integration --quiet`: 11 + 18 + 47 passed.
- An additional refusal-notice/session-reuse fence regression raised the
  runtime-control count to 19; all passed.
- `cargo test --offline --locked -p sav --lib --quiet`: 36 passed.
- `cargo clippy --offline --locked -p sav --all-targets -- -D warnings`: passed.

The clean-slate source observer recorded refused-action/result/control behavior.
Its conditional session-forwarding concern was not confirmed: preflight rejects
unpaired history before another provider call, now covered by failure injection.
A separate source-blind comparator found no high-confidence scoped phase-one
discrepancy. Both reviews distinguish runner-provided test output from
independently executed evidence.

All three changed intent bundles pass structural `component validate`.
Read-only `task next sav` correctly rejects stale accepted revisions instead
of silently accepting changed intent. No acceptance operation was invoked.

Protected-agent initial regressions independently reran with 9 live-runtime
and 5 task-command cases passing. Source-only inspection then identified three
coupled discrepancies: failed observations recorded as exit-zero in legacy
trajectory, later broker-retained alternatives downgraded to advisory notes,
and explicit unfinished-work statements qualifying as completion. Corrective
tests/source work and a narrower independent reinspection are required before
the phase-one source-blind comparison and commit. Intent is unchanged.

The corrective lifecycle also covers explicit final blockers, summary-only
later choice labels, inline quoted prior errors and contradictory optional
patches. Each behavior reproduced before its fix; legitimate complete,
negated, quoted and fenced reports retain success. The actual dispatch event
name is asserted rather than an absent alias.

Corrective tasks are a separate complete, contiguous four-stage item; new
dependencies are lexically sorted. Two repository-queue validation failures
exposed metadata mistakes, which were fixed rather than weakening validators.
All 92 task-command units pass. A narrow advisory followup failed its explicit
no-delegation constraint and is not used as required clean-context evidence;
fresh bounded source-only observers provide the final factual record.

Final protected checks:

- `cargo test --offline --locked -p kvist --test live_runtime --test task_commands resilience_ --quiet`: 16 + 5 passed.
- `cargo test --offline --locked -p kvist --test live_runtime --quiet`: all 27 passed after the final patch-field correction.
- Full affected integration targets: 8 agent / 27 live runtime / 81 task commands passed.
- Library selectors: 61 agent / 53 authoring / 92 task commands passed.
- Targeted Kvist and Sav Clippy with warnings denied and affected formatting passed.
- Shared detector/control/Skott targets independently reran: 11 + 19 + 47 passed.

The final separate source-blind comparator found no high-confidence phase-one
behavioral discrepancy. It explicitly limits conclusions to supplied execution
and scoped source observations; lexical checks are not semantic certification,
and unchanged repair conclusions rely on the initial clean source record.
Phase-one implementation and corrective items are complete. Streaming,
progress-aware protection and operational hardening remain phases 2-4.

Phase-one commit: `31ddf51` (`fix: preserve honest agent outcomes and failure evidence`).

Phase-two commit: `214e209` (`fix: bound repetitive generation with honest recovery accounting`).

Phase-three commit: `5b2801c` (`fix: detect stalled tool work using validated progress`).

### Phase 2 evidence

The five initial NativeRuntime regressions all failed before implementation:
repeated output was accepted as success, one discarded-turn tool ran, a unary
length response enlarged its reserve, and recovery/accounting/recording
expectations were absent. Bounded shared detection and finite correction now
cover both answer and reasoning at 100/150/200-word periods.

Calibration initially passed nine of thirteen pure cases and exposed four
false positives: numeric-bearing changes, multiline quotations, multiline
inline code and a shorter internal code fence. Those cases reproduced before
their fixes. Additional regressions reproduced a terminal near-match missed
by all three fast anchors, late cancellation presenting stale text, and known
rejected usage being ignored. All were corrected. The length-only two-copy
prose rule was clarified during implementation and remains independently
reviewable, not a universal semantic-loop guarantee. An additional unbroken
JSON/XML payload false positive reproduced before its lexical exclusion.

- `cargo test --offline --locked -p sav -p skott --test generation_resilience --test stream_loop_detection --test loop_detection --test runtime_controls --test loop_integration --quiet`: 12 + 14 + 11 + 19 + 50 passed.
- `cargo test --offline --locked -p sav --lib --quiet`: 39 passed.
- `cargo clippy --offline --locked -p sav -p skott --all-targets -- -D warnings`: passed.

Tests assert exact 100/150/200/512-word metadata, 94-versus-93-percent identity,
Unicode scalar handling, 2,048-entry retention, 256-scalar control polling,
unchanged reserves, bounded corrections, no partial effects and no replenishment
after actual compaction/intervening effects. The old compaction fixture used
artificially repeated final text; equally sized novel counter words replace
that padding without weakening its compaction or complete-record assertions.

Protected integration initially reproduced six failures among eight cases;
its cadence test stopped at the first failing combination rather than proving
six separate red runs. All six combinations subsequently passed. Its loopback
fixture's 400-ms completion fallback raced interruption under concurrent load;
TCP_NODELAY and a five-second bounded fallback preserve the assertions while
removing that artificial race. Final protected control checks and independent
phase-two observations/comparison are still pending.

No full-host recovery-journal failure is claimed: the protected owner constructs
its held-descriptor journal internally. Three safe lower-level recording-failure
tests pass, but path removal/chmod is not a valid late-write failure injection.

The independent shared observer found three additional coupled hazards: a
correction silently spent when guidance arrives during notice callbacks,
returned known usage skipped by steering, and length/failed-retry usage omitted
from progress provenance. Four existing-API regressions all failed first, then
passed after correction. Guidance now accompanies rather than erases recorded
feedback; all returned attempts reconcile once before control precedence.
A separate accounting observer exposed a misplaced terminal-progress flush;
two strengthened rejected-budget assertions reproduced it and pass after moving
the flush outside the turn loop.

Current shared checks pass 16 generation, 14 stream, 11 action, 19 controls,
50 Skott integrations and 39 Sav library cases, plus strict all-target Clippy.
Scoped security reviews reported no vulnerabilities. Source-derived records
and execution attribution are retained in local IMPL supplements, not treated
as acceptance or full compliance.

The accounting observer also highlighted error-return terminal-event omissions
and claimed-guidance disposition loss on recording failure. These are explicit
phase-four operational tasks, not concealed as finished behavior. Combining
late human guidance with recorded feedback is intentional; continuing recording
after a failed callback is not an acceptable way to capture a transport that
ignores the failure.

The separate source-blind Sav/Skott comparisons found no established remaining
phase-two contradiction, with missing assertion-level evidence and final-flush
reinspection limits explicitly disclosed. Both initially read older IMPL
sections and excluded them from reasoning; neither read source/tests or executed
checks. Their scope is diagnostic comparison, not acceptance/certification.

The broad shared/frontend suites additionally reproduced the backpressure
fixture stopping before its required 128-slot fill because it concatenated
identical provisional text. Novel counter-bearing text preserves that queue
threshold, cancellation/teardown bounds and default-on detector.

After that correction, `cargo test --offline --locked -p sav -p skott --quiet`
passed all 784 tests with three existing ignored tests. The unchanged offline
sandbox/toolchain integration tests also passed. Strict all-target Sav/Skott
Clippy and whitespace checks pass; all three component bundles structurally
validate while retaining stale acceptance. Protected follow-up and independent
comparison are still required before the phase-two commit.

Protected follow-up reproduced three additional HTTP failures before fixes;
completed-usage steering already passed. All 11 generation tests pass both
concurrently and serially, 63 agent units and 38 full-live serial cases pass.
Parent independently reran 11 generation and 63 agent cases. Distinct recovery
1/2 and 2/2 feedback survives later guidance; known length totals are 24/12/36,
and unknown failure provenance stays unavailable. Scoped formatting/lint pass.
The protected source-only observer found no established high-confidence
production defect; a separate security reviewer found no vulnerabilities.
Its imported-detector and late-fsync/transport boundaries are recorded in IMPL.

The protected source-blind comparison identified conditional lost known usage
when a transport ignores callback rejection and returns a completed turn.
Rather than assuming native transports always cooperate, this is assigned a
deterministic private-adapter correction before committing phase two. The native
host already passes the corresponding adversarial case with no discarded
effects, a 10,100-token debit and exact final provider progress.

A final clean-slate native accounting observer confirms the corrected flush
position and latched rejection; no further normal-exit defect was established.
Its assertion-strength feedback tightened five tests to inspect final progress
and exact known lower bounds. All 17 native generation tests pass. Propagated
error publication remains explicitly operational-phase work.

The final protected handoff regression reproduced two failures (lost 10,100
usage and repetition replaced by its own cancellation), then passed after a
private attempt-local rejection handoff. A fresh source-only observer confirms
accounting/journal recording before typed rejection and unchanged no-effect
semantics. Parent reran 65 agent units and all 38 live-runtime cases successfully;
strict all-target Kvist/Sav/Skott Clippy passes. No shared API change was needed.

The final fresh protected source-blind comparator used the exact observation
supplement, found no established remaining scoped contradiction, and retained
imported-mechanism and failure-injection limits. All phase-two lifecycle tasks
are complete; this phase does not certify the planned progress-aware tools,
aggregate repair budgets or operational diagnostics.

### Phase 3 evidence

Five of six initial native regressions failed before production changes:
unchanged/failed repeatable reads completed falsely, nine-action cycles escaped,
successful no-op output cleared protection, and one five-duplicate batch spent
the entire breaker before any model response. A separate cross-accepted-turn
150-word passage with unique no-op calls also falsely completed after 11 turns.
All now pass through shared typed progress/observation/cycle protection and
transactional generation continuity. New actual observations remain productive.

The source-only observer additionally identified terminal executor failures
being overridden by tool recovery. Both extra-request and overwritten-cause
regressions reproduced before the precedence guard, then passed. Current shared
checks pass 10 tool, six exact-bound/rollback, 17 generation, 19 control, 39 library
and 14 stream cases, plus strict Clippy. Protected/workspace integration and
independent final comparisons remain outstanding.

The final shared precedence observer confirms hard errors precede corrective
budget allocation/exhaustion and that guidance/compaction never reset it.
The tool-specific late-guidance callback test proves combined delivery with
distinct correction 1/2 and 2/2 in three real requests; all 11 tool tests pass.

Workspace integration initially passes 57 loop and 38 native-file cases.
Its independent source observer identified ambiguous identical pending progress
evidence plus rebinding/cancellation observation gaps; corrective work is in
progress. An existing explicit-host staged-shell context issue is assigned to
phase four rather than silently left outside the error-handling plan.

The complete final Sav suite passes 288 tests. Protected implementation and
fresh source-only observation independently pass 69 agent and 53 authoring
units, eight agent CLI and five repair cases; ten new progress live cases pass.
The observer's serial full-live run exits zero after announcing 48 tests but
its terminal counters were truncated; the implementer's private log explicitly
reports all 48 passed. Deadline-based red fixtures are distinguished from
assertion failures in Maerg's record. The applier production change only exposes
the existing ancestor-validation helper for revision inspection.

Workspace identity/rebinding/cancellation corrections reproduce nine failures
before fixes; independent selected reruns pass 123 tests. A fresh observation
identified a sandbox prepared-selection mismatch requiring one further bounded
correction before closing this phase. Separate protected/workspace security
reviews found no vulnerabilities. Source-blind comparisons remain pending.
Executor errors after possible effects, malformed read outcome retention,
non-atomic external-writer observation, dispatch confirmation versus byte
certainty, and aggregate cross-repair state are explicitly retained; operational
error/budget handling is phase-four work, not fabricated completed capability.

The final sandbox correction reproduced three prepared-selection failures
before binding revision observation to the exact prepared authoring grant.
A fresh source-only observer independently passes all three prepared cases and
12 progress cases; a narrow security follow-up finds no vulnerabilities.
Synthetic closures establish mapping behavior, not sandbox isolation or an
atomic transaction against external writers. Strict all-target Sav/Skott/Kvist
lint, affected formatting and whitespace checks pass.

Final `cargo test --offline --locked -p sav -p skott --quiet` passes 830
tests with three existing ignores. Separate protected and final exact-range
workspace source-blind comparisons establish no concrete scoped phase-three
contradiction, while retaining evidence/imported-mechanism limits. The first
workspace comparator's overly broad locator exposure disqualifies its pristine
scope claim; only the replacement exact-range comparison closes that lifecycle.
All phase-three lifecycle tasks are complete, with acceptance intentionally stale.
This does not complete the operational diagnostics, aggregate sequence budget
and error-certainty work assigned to phase four.

### Phase 4 work breakdown

Sav defines typed failure/effect/recovery and stopping-resource APIs, freezes
observed causes before cleanup, publishes accounting/terminal state on error
returns, and closes claimed guidance dispositions without fabricating records.
Skott consumes those APIs in TUI/headless/readable/private records, makes
full-channel prompt cancellation responsive, preserves failed observation
outcomes, exposes the intentional-output exception, discloses opaque external
capabilities and fixes literal staged-host shell context.
Maerg retains one authoring/repair sequence budget and stall/progress state,
detects repeated unchanged verification signatures, and preserves every
decision/error/cancellation/effect fence. Verification keeps separate approved
enforcement while its elapsed time consumes the model sequence deadline.

Each local phase follows tests-before-code, independent source/security
observation and separate source-blind comparison. Exact operational intent is
recorded in each bundle before implementation under the existing human-directed
advisory exception; no implemented review receipt or acceptance is invented.

### Phase 4 shared evidence

All five initial native operational regressions failed behaviorally before
production changes: missing terminal failure, lost final known usage, cancelled
cleanup relabeled as deadline exhaustion, missing final claimed-guidance drops,
and silent terminal-record failure. Typed diagnostics/resources and finalization
correct them without fabricated complete-turn recording.

Independent source observation drove three additional evidence/cancellation
corrections. Two exhaustion-notice and two final-presentation cause combinations
also reproduced separately before correction. The final shared stopping helper
resolves before final accounting/cleanup and preserves original plus secondary
causes. Independent source-derived records retain callback/cooperative timing,
repeated final notices and execution-versus-presentation completeness limits.

Current shared targeted checks pass 24 operational, 17 generation, 19 controls
and 11 tool cases; the full shared suite passes 312 tests with strict all-target
lint/formatting. A scoped security reviewer found no vulnerabilities. Exact
typed resource tests distinguish wall, model tokens, turns, model attempts and
irreducible context; an initially invalid-reserve fixture was corrected rather
than mislabeled as context exhaustion. Workspace/protected final operational
integration and independent comparison remain in progress.

### Phase 4 workspace evidence

The workspace implementation reports 435 serial passes, including 21 new
operations cases, with strict lint/formatting. Genuine reds cover failed-tool
marks/diagnostics, full-channel prompt cancellation, completed-read/count
retention, CLI exception, external capability disclosure and actual literal
staged-host scripts. Mid-edit API/fixture errors are not behavioral evidence.
Two existing parallel IPC/startup timeouts pass serially without changed
deadlines or skipped cases.

A fresh source-only observer independently passes 37 distinct relevant cases,
including all 21 new tests, actual PTY/subprocess paths and exact 128-slot/
four-handoff bounds. It establishes no concrete new defect and retains
event-count-versus-byte, partial assertion, imported runtime/runner and private
content/redaction limits. A separate scoped security reviewer finds no
vulnerabilities. Workspace source-blind comparison and protected sequence
completion remain pending.

Parent reran all 435 affected workspace cases serially with the final shared
runtime; all pass. The fresh exact-range source-blind comparison establishes
no concrete scoped contradiction and explicitly retains a staged-script
permission evidence gap, now assigned a targeted follow-up rather than
assuming private journal modes prove script privacy.

The targeted permission follow-up requires no production change: actual script
assertions verify payload 0600, immediate staging directory 0700, non-links,
literal content and cleanup. Parent independently reruns two host cases and
the private-outside-workspace staging unit, all passing. That specific evidence
gap is addressed; all workspace phase-four lifecycle stages are complete.

Final broad serial execution then exposed an uncovered direct-executor
missing-runner error assertion and a new PTY fixture's cross-device hard-link
assumption with ordinary `TMPDIR`. All targets were allowed to finish; only
those two targets failed, with three existing ignores. A bounded native
follow-up must establish/fix the cause and portable fixture without weakening
authority, assertions or deadlines before final phase closure.

The native follow-up reproduces the actual wrapper-caused direct domain change,
preserves established executor domains and retains typed native prerequisites.
The PTY fixture copies byte-identical executable launchers and owns its scratch;
ambient runner `TMPDIR` is not required to share the binary's filesystem.
Implementer-selected checks pass 65; fresh source-only observation independently
passes 14 distinct boundary tests, with imported/cooperative/fixture limits.
A narrow security reviewer finds no vulnerabilities. Parent final full
Sav/Skott serial execution passes 876 tests with three existing ignores,
including every previously failed target.
Fresh exact-range source-blind comparison establishes no concrete native
phase-four contradiction, retaining the narrow excerpt and imported/assertion/
cooperative/portability/privacy qualifications. All native phase-four stages
are again complete; protected final corrections/comparison remain pending.

### Phase 4 protected initial evidence

Protected implementation reports 71 agent units, one repair authority and one
recurrence unit, eight agent CLI, 48 live and 87 task-command cases passing,
plus six phase-four/five repair rechecks and strict lint/formatting. Its claimed
nine behavioral reds remain implementer-attributed until the exact report is
archived.

Fresh source-only observation independently passes 18 distinct covering tests
and confirms uniquely owned original sequence budgets/history, independent
verification enforcement, bounded recurring repair, retained outcomes/decisions
and filesystem evidence. It identifies two concrete local gaps in live-result
recording/known accounting and the final-success cancellation/deadline boundary.
Both are assigned bounded red-first corrections before phase completion. A
separate scoped security reviewer finds no vulnerabilities in the initial work.

Final protected source-only observation independently passes nine units and
confirms corrected result/callback/accounting and success fences. It also finds
a tightly coupled final per-file change collection weakness: silent observation
skips, unordered enumeration and unbounded ordinary reads can yield incomplete
success evidence. A bounded final correction is assigned; private/consumer intent
now requires deterministic fallible bounded inspection of the existing inventory
scope, not wider peer traversal or atomic snapshot promises. Earlier record and
late-finalization corrections remain independently supported, not concealed.

Fresh inventory observation independently passes nine scoped tests and confirms
the preserved immediate-tests inventory, existing 1 MiB/file, 8 MiB material
and 4,096-entry bounds, explicit errors and original success fences. It exposes
one precise reused-reader open-window hazard: regular-to-FIFO substitution can
block before descriptor validation. A final Unix nonblocking/no-follow open
correction with owned bounded red regression is assigned; generic synchronous
kernel/clock/snapshot limits remain deliberate, not claimed away.

Protected broad execution completes all targets. A repository `TMPDIR` causes
11 ancestor-project/VCS fixture failures; normal unset `TMPDIR` resolves these
but exposes ten existing executable fixtures' relative-path fallback. Five
ancestor-sensitive units pass independently. An explicit absolute external
temp directory then passes the entire library: 692 tests, one existing ignore.
Combined with all normal-temp integration targets (476 passes, six existing
ignores), every 1,168 protected test passes without skipped checks or production
changes for fixture environment. Final narrow reader rechecks follow separately.

The final reader correction preserves no-follow and adds Unix nonblocking open
before descriptor regular validation. One owned FIFO regression fails old
behavior, unblocks/reaps its helper before assertion, then passes without a
writer. Three exact-bound tests establish real 1 MiB/+1 reads and existing
8 MiB/+1, 4096/4097 counters. Fresh source-only observation independently passes
12 completion cases; parent passes all 19 phase-four units. A narrow security
reviewer finds no vulnerabilities. Generic kernel/mutation/ancestor/atomicity
limits remain explicit. This closes assigned source defects, not certification.

Final source-blind comparison identifies one documentation discrepancy: initial
recursive root hashes are not an initial per-file change array. The fix-all
authorization is applied conservatively to correct that wording, preserving
existing separate snapshots and shallow final inventory rather than expanding
implementation. The finding/disposition is retained; no other definite scoped
contradiction is established. Final full library now passes 696 tests with one
existing ignore after all reader/boundary changes.

The separate narrow wording comparison confirms the core exclusion/disposition,
retaining its mid-sentence excerpt limitation rather than claiming preceding
text independently checked. All scoped discrepancies have explicit corrective
dispositions; lifecycle completion is diagnostic delivery, not acceptance.
Final distinct affected coverage is 2,048 passes with ten existing ignores:
876 Sav/Skott plus 1,172 Maerg (696 library, 476 integration). Scoped security
reviews report no vulnerabilities. Offline release workspace build, strict
all-target affected lint, formatting, whitespace and structural validation pass.
All four phases and their requested local tasks are complete; the final phase
commit contains this evidence record and retains stale accepted revisions.

## External practice and deliberate limits

The design uses bounded deterministic evidence and goal-preserving correction,
not an obligatory model judge. Gemini CLI's
[pinned loop detector](https://github.com/google-gemini/gemini-cli/blob/acae7124bdd849e554eaa5e090199a0cf08cd782/packages/core/src/services/loopDetectionService.ts)
illustrates separate tool/content protection; its short recurrence-distance
window is not copied because it misses the reported long cadence.
[Codex retries](https://github.com/openai/codex/blob/9d8de196748b57d7f463a7757eaa447b6483a331/codex-rs/codex-client/src/retry.rs)
and [OpenAI Agents run guidance](https://openai.github.io/openai-agents-python/running_agents/)
support explicit bounded recovery rather than treating activity as completion.
[Anthropic tool handling](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls)
and [stop reasons](https://platform.claude.com/docs/en/build-with-claude/handling-stop-reasons)
reinforce complete tool-result pairing and distinct failure/truncation semantics.
Opaque effectful agents are not automatically restarted to emulate these
native capabilities.
