<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Scoped source-only diagnostic and acquisition observation

Derived by clean-slate `diagnostic-source-observer`; the comparison below came
from separate source-blind `diagnostic-intent-comparator`. Both inspected their
permitted inputs without running validation commands.

`SandboxUnavailable` formats failures as runner preparation **or execution** failures. Path-aware helpers retain operation, path, and underlying error text. Cargo-context diagnostics identify rejected filesystem types, hard-link counts, metadata sizes, UTF-8/TOML failures, and detected changes, often suggesting corrective actions. Some installed-Rust errors remain generic.

Cargo-context preparation traverses workspace members and declared local dependencies, restricts dependency roots to non-hidden in-project directories, rejects links and special entries, filters operational material, and constructs read-only grants. Bounds include 1 MiB metadata, 128 crates, 100,000 tracked entries, 32 MiB path bytes, depth 128, 256 selected mounts, and a checked 30-second budget. Metadata reads use non-following, nonblocking opens and before/after fingerprints. Acquisition staging copies manifests/locks but substitutes empty source files, generates approved registry settings, checks Git approvals, and requires a root lockfile.

Installed-Rust preparation queries host rustup with a restricted environment and auto-install disabled, inventories toolchains, creates private wrappers/settings, and supplies read-only grants. Application rechecks tracked fingerprints and writable overlaps, then sets offline Cargo variables. These checks are not continuous snapshots; reviewed code depends on Unix facilities and fixed host paths.

Configured acquisition resolves/probes tooling, creates transaction-local scratch, invokes acquisition, compares staged/project lock digests, then requests host-side offline repacking. Scratch cleanup is attempted after success or failure; combined failures retain both causes. There is no configured-acquisition host fallback. Failure diagnostics aggregate cancellation/time/output-limit flags; exit status appears when those flags are absent. Captured stderr takes precedence over stdout; text undergoes redaction/sanitization and UTF-8-safe truncation to 4,096 bytes, excluding surrounding error text.

Reviewed assertions cover diagnostic details, retained special entries, redaction/truncation, and a local-only native workspace expecting unchanged lock bytes, no project target, empty vendor output, and cleaned scratch. This fixture does not exercise downloading external dependencies.

Supplied execution evidence—not executions performed during this observation—reports four diagnostic tests failing previously; current sandbox tests 27 passed, acquisition-diagnostic tests 2 passed, shared-vendoring tests 5 passed, and formatting/strict Clippy passed. Native acquisition currently fails with status 3: payload succeeded but promotion lacked a provider-owned generation-parent capability. Unchanged archived HEAD also fails, previously hiding the cause; 16 other baseline vendor tests passed. Real-project integration remains blocked by a stale receipt; root Cargo.lock is unchanged. These observations establish neither acceptance nor compliance.

**High-confidence scoped defects:** None established. The supplied native-promotion failure remains unresolved; its provider implementation was outside this inspection.

### Source-blind comparison

The supplied observation broadly supports the scoped diagnostic intent: neutral preparation/execution wording, operation/path/cause retention, explicit invalid filesystem conditions, bounded metadata checks, preserved isolation, and configured-acquisition no-host-fallback behavior. Observed acquisition diagnostics support flag aggregation, stderr preference, redaction/sanitization, UTF-8-safe bounding, and retention of combined cleanup failures.

No high-confidence contradiction is established. Some installed-Rust errors reportedly remain generic; this potentially conflicts with the shared-resource diagnostic obligation, but the supplement does not identify sufficiently specific cases to establish a violation. Any covered filesystem/metadata failure lacking its available path or underlying cause would require human arbitration, not automatic intent revision.

Evidence does not explicitly establish every promised detail, including configured-runner naming, redaction ordering, visible truncation markers, empty-output wording, or complete focused-test coverage. Passing scoped tests and formatting/Clippy results are supporting evidence, not compliance proof; execution results were supplied rather than independently reproduced.

The native acquisition/promotion failure remains unresolved and recorded. Baseline reproduction explains its prior existence but does not waive it. Provider implementation was outside observation; real-project integration remains receipt-blocked, and the local-only fixture does not establish external dependency acquisition.

The broader informative-errors policy remains unverified; exhaustive repository-wide coverage and acceptance are not certified.

## Final configured Rust acquisition repairs - 2026-11-06

Clean-slate `final-rust-impl-observer` inspected current source/test assertions,
excluding intent, queues, prior records/reviews and history; it executed no
tests. The configured adapter retains closed sandbox fetch authority and never
falls back to host acquisition after failure.

Cargo-context discovery follows workspace, target, patch, replacement and
declared in-project path dependencies under existing bounds. Acquisition stages
stable regular single-link manifest/lock bytes and empty implementation/target
placeholders with relative layout preserved. It excludes implementation content,
host Cargo settings/credentials and component intent. Alternate registry settings
come only from validated acquisition policy.

The initial `rust-source-observer` security review reported a medium-severity
(confidence 9/10) native Git proxy-bypass admission defect. Subsequent
independent source observation found manifest dependency tables and root lock
Git sources checked before fetch against exact approved HTTPS/full-revision
pairs; source policy rejects unsupported transports and noncanonical bindings.
Two admission regressions failed before repair in
`d353668b-3512-4fe6-afed-cb19da4c082d`. No live unauthorized network request was
executed, and these admission checks are not proof of total backend egress
confinement, DNS behavior or redirect handling.

After fetch both staged and original lock digests must match the prefetch
identity before host offline repack. Owned acquisition scratch closes before
publication; cleanup failure is not success. Protected fulfillment still always
refuses unbound model requests. Separate protected readiness uses the full shared
gate, cancellation, budget and approved runner identity without fetching.
Authoring checks full root readiness when lock or managed Rust state exists;
an unmanaged standalone child retains its empty offline build view.

Observed assertions include fences, untouched protected state/host sentinels,
native workspace/path fetch without builds/downloads, unchanged lock and scratch
cleanup, complete installed Rust inventory, offline authoring and foreign-root
manifest relocation with valid package/checksum/resolver material.
Implementer-attributed final nextest
`2e9bc4ba-255b-4878-b85f-aafe0ae1e259` passed all 389 selected tests.
The earlier incomplete fixtures and malformed reopened queue timestamps were
repaired without weakening source/checksum validation or isolation assertions.
This record is not acceptance or a blanket compliance/security certificate.

Separate source-blind `rust-source-blind-comparator` found correspondence for
the scoped locked operator adapter, source admission, full readiness and child
authoring, but retained three broader intent discrepancies for human arbitration:
`REQ-DEPENDENCY-ACQUISITION` targets lock-changing immutable-generation resolution,
not this unchanged-lock operator repack; `REQ-DEPENDENCY-REQUEST` still targets
automatic approved model fulfillment, which remains explicitly unimplemented;
and `REQ-RUST-TOOLCHAIN` durable-baseline wording differs from the enforcement
design's permitted baseline-free verification. The latter branch was not
established by the scoped new observations. These are not silently deferred,
accepted or fixed by this repair; no automatic acquisition was enabled.
HTTPS admission is not total backend egress/DNS/redirect proof.

Subsequent production-runner nextest
`5c1a5560-129c-461a-b27b-4c29eac74ef1` passed all 389 affected cases with
`KVIST_GALLA=/opt/target/release/galla-runner` after final code/queue updates.
The release Kvist binary was atomically installed and byte/version verified;
running processes were not stopped.

## Independent shared dependency observations - 2026-11-06

Fresh `offline-dependency-source-observer` read only scoped current Rust
source/tests and explicit process-provider source, excluding intent, queues,
prior records/reviews and history. Its initial read and two selective corrective
follow-ups changed/delegated nothing, ran no tests and established no reportable
scoped security vulnerability. These are observations, not acceptance.

Carried forward from the preceding corrective source read: acquisition uses an owned private temporary directory with explicit cleanup before returning publishable configuration; cleanup failure prevents success. Simulated assertions cover default/custom shared publication, while CLI assertions cover explicit custom destination selection. Configured acquisition still stages **only Cargo.lock**, so general native configured-fetch success is unestablished; failure stops without host fallback, followed on success by host Cargo-level offline repacking. Protected readiness remains the legacy cheap inspection, not full data verification. Sources: `/opt/proj/kvist/maerg/src/vendor_command.rs:437–458,504–599,1157–1196`; `/opt/proj/kvist/maerg/tests/shared_vendoring.rs:40–64`. Protected-readiness attribution: first review, `/opt/proj/kvist/maerg/src/vendor_command.rs:162–228`.

The first read additionally observed shared catalogue reexports, error-domain
and cancellation adaptation, both Vendor CLI branches propagating malformed
configuration, and missing configuration alone selecting no configured sandbox.
Rust publication delegates to common transactions; unconfigured operator
acquisition uses host Cargo, while configured failure never falls back.
Protected fulfillment refuses unbound requests before path inspection.
Optional runtime resolver staging is bounded, no-follow, single-link and
source-only, retargeted to `/rust/vendor` and staged read-only.

The initial scratch/candidate-layout discrepancy was independently observed
corrected through explicit temporary-directory cleanup and default/custom
publication assertions. Those simulate acquisition composition; they do not
establish successful native configured fetching. Fetch/repack retain separate
allowances/cancellation arrangements. Checksums are metadata consistency, not
origin attestation; subprocess supervision is not OS confinement. The source
observer ran no tests; parent-run results are separately attributed evidence.

## Final reader observations - resilience phase 4

Fresh `maerg-p4-reader-source-observer` inspected only the local bounded reader/
budget declarations and private completion assertions, excluding intent, queues,
records/reviews, history and peers. It changed/delegated nothing. Unix opens use
no-follow and nonblocking flags, require a regular descriptor and verify lengths
around the bounded read. The original inclusive file/material/entry thresholds
are unchanged. Same-length mutation/ancestor races, atomic snapshots and
interruptible ordinary kernel reads are not established.

Independent offline/locked execution passes 12 distinct completion tests: two
collection, six inventory, three exact-bound and one FIFO. The FIFO helper is
not an additional test; its observed response fits two seconds, needs no rescue
writer, exits zero, is reaped and its exact fixture is removed. Exact 1 MiB/+1
uses real file reads; exact 8 MiB/+1 material and 4096/4097 use existing budget
counters, not peak allocator or physical-inventory acceptance claims.
A separate narrow static security reviewer reports no vulnerabilities.

Implementer-attributed archives retain the one genuine blocking-open red,
including its safely unblocked/reaped helper before assertion; three boundary
tests already passed against the old reader. A temporary mistargeted flag patch
is restored and not a second distinct regression. The final 19 phase-four unit
cases pass independently in the parent. No API, numerical limit, grant,
inventory scope or accounting reset changed. Generic cooperative filesystem/
clock/write limits remain explicit, not silently certified away.

Separate `maerg-p4-final-source-blind` directly compares only scoped requirements
686-710, contract 422-475, design 5-56 and phase-four observations 4-182 plus
the canonical task projection/reported execution. It reads no source/tests/
history, searches or other artifacts, executes/delegates nothing and certifies
nothing. It supports the chronological recording/sequence/finalization/inventory/
reader corrections, retaining overlapping and attributed execution, partial
transition/disposition and cooperative/allocation/mutation evidence limits.
It identifies one design wording discrepancy: no initial per-file inventory
exists; initial recursive root hashes are separate from final shallow records.
Under the user's fix-all-findings authorization, the wording is corrected to
preserve those existing distinct structures, not invent or expand implementation.
The comparison finding and its explicit documentation disposition remain here.
Separate narrow `maerg-p4-inventory-wording-comparator` confirms that the shown
exclusion of an initial per-file array/before-after diff resolves the concrete
conflation. Its 45-57 excerpt begins mid-sentence, so it does not independently
establish the preceding recursive-versus-shallow distinction. The full prior
source observation and explicit conservative wording disposition remain separate
evidence; no certification or acceptance is claimed.

Final parent full library passes 696 tests with one existing ignore after the
reader/boundary corrections; all 476 integration cases passed with six existing
ignores. Distinct final Maerg coverage is 1172, not a sum of overlapping selectors.

## Final inventory observations - resilience phase 4

Fresh `maerg-p4-inventory-source-observer` inspected only local completion,
initial/success caller and reused inspection helper source/assertions, without
intent, queues, records/reviews, history, peers, edits or delegation.
The existing array observes immediate regular files under component `tests`,
not root/src/nested files or a VCS diff. Records retain project-relative lexical
UTF-8 paths, `operation=create`, null pre-digest and byte SHA-256 post-digest;
unchanged files are not distinguished and deletions are not listed.

Names are UTF-8 validated, budgeted and sorted. Limits are existing 1,048,576
bytes/file, 8,388,608 material bytes and 4,096 enumerated entries; skipped
directory names count. File/name/serialized-record material is charged, not
whole journal events/allocator overhead; one bounded read/serialization occurs
before its aggregate charge. Final inventory does not recurse or use depth.
Observed/opened/final lengths must agree within the per-file ceiling, and the
reader takes at most ceiling+1 bytes. The current Unix open is no-follow; its
regular-to-FIFO pre-validation blocking window has a final narrow correction
assigned rather than claimed safe.

Missing selected tests root retains empty semantics; other root/enumeration/
UTF-8/entry/open/read/reinspection errors fail. Links/nonregular entries fail;
ordinary directories are skipped. No failed collection returns its prefix.
Initial source/test recursive root hashes are separate, bounded evidence before
sequence creation, not the same changes array or its before/after comparison.
The original tree helper uses depth 64 and charges both name enumerations;
its entry budget is not a count solely of distinct files.

Final collection checks before/after success. Errors precede construction of
final success events, are wrapped as evidence failure, block the attempt and
return; blocking internals were not source-verified here. Appends/canonical
transition remain separate operations and completed effects are not undone.
Same-length mutations, ancestor rebinding, synchronous kernel calls and
non-atomic snapshot/clock/write windows remain explicit limits.

Independent offline/locked executions pass six inventory, two late-collection
and one protected inventory CLI case (nine distinct). A nonexistent package
selection and an unqualified exact selector executed nothing. Assertions
establish observed-file disappearance, retained effects, ceiling+1 rejection,
nine ceiling-sized file aggregate rejection, invalid/absent roots, sorted
shallow hashes, exact lexical CLI fields and late cancellation/expiry.
Entry/exact-aggregate acceptance, read-time growth, enumeration/link/nonregular/
invalid-name races were not executed here. A separate narrow static security
reviewer reports no vulnerabilities in this inventory correction.

The implementing-context archive attributes four genuine inventory reds and
15 phase-four unit/one CLI passes, preserving scope/fields and compilation/path
fixture failures separately. Parent full normal-temp integration targets pass
476 distinct tests with six existing ignores; full library with explicit
absolute external temp passes 692 with one existing ignore. Repo temp ancestry
and unset relative executable-fixture fallback caused earlier broad validation
failures, not observed production regressions. No checks were skipped.

## Recording and finalization observations - resilience phase 4

Fresh `maerg-p4-final-source-observer` inspected only local agent, sequence/
accounting/live/retry/failure/review/regression and completion/outcome/repair/
success caller source, excluding intent, queues, records/reviews, history and
peers. It made no edits or delegation and independently passed nine current
phase-four library tests, with 678 filtered out. Its initial nonexistent
`-p maerg` selection executed nothing.

The synchronous attempt-local `Cell` hands back the underlying returned outcome
after required observation rejects it. Failed stream callbacks are latched and
never re-entered; proposals are not accepted. The counted owner reconciles usage
before observation failure/exhaustion/generation rejection. Required result
record failure preserves a typed provider error when already returned;
callback-first failure prioritizes that observation, with later evidence layers
possibly flattening errors into strings. Known usage never refunds conservative
debit; higher actual usage increases it. Missing/failed/zero usage permanently
loses complete provenance, while known totals remain lower bounds.

Decisions precede this turn's effects; dependency acquisition can precede
denied-intent rejection. An uncertain placeholder precedes effect dispatch,
observed completion replaces it and cancellation/non-applied outcomes stop.
One mutable sequence spans authoring/repairs. Verification retains its separate
enforcement and is checked for sequence interruption afterward, not preempted by
the sequence's token/deadline.

Final collection checks before/after synchronous digest/change work. Required
success appends have preceding checks and a following check; continuous
completion checks after timestamp acquisition immediately before transition.
The canonical transition appends prepared, replaces queue state, appends
committed; no interruption check occurs within or after it. Journal writes
synchronize, but append/clock errors may propagate rather than guarantee a
blocked disposition. Clock/cancellation/kernel I/O and hashing are not atomic.

Executed callback/result/provider-usage, conservative debit, recurrence and
late-collection cancellation/expiry assertions are fixture/helper evidence,
not end-to-end canonical-transition interruption. The observer identified the
current change-list collection's silent read/entry errors, ordering/shallow
scope and unbounded reads; a separate bounded correction is assigned before
phase closure. Bounded required tree digests are not an atomic change snapshot.
A separate final scoped static security reviewer finds no vulnerabilities in
the recording/interruption corrections, without executing tests or assessing
other implementations.

Implementer-attributed exact archives record nine initial and six corrective
behavioral reds, excluding fixture compilation failures. The corrected result
tests use closed-journal injection and actual counted returns, including 10,100
tokens against an 11,000-token policy and typed HTTP 503. Late-fence tests hash
real files, then cancel or sleep under a one-second policy, retaining completed
fixture effects. Its final 78 library/22 live/6 task checks cover 106 distinct
tests, with chronology and non-atomic limits retained in private evidence.
The initial 216-test attribution was not one final-source combined invocation.

## Initial scoped sequence observations - resilience phase 4

Fresh source-only observation inspected only local agent sequence/accounting/
retry/failure/progress, task repair/outcome/authority code and local assertions.
It excluded intent, queues, records/reviews, root/architecture, peers and
history, changed nothing and did not delegate or certify implementation.

One mutable sequence owns original start/deadline, cancellation, conservative
model debit, known/unknown totals, recovery allowance, tool and generation
history across initial authoring/repairs. New repair conversations/logs do not
recreate resources. Each dispatched attempt reserves input/output; positive
reported usage debits at least the reservation and increases debit when larger.
Missing/failed/zero-usage attempts retain reservations and unavailable
provenance. Retry/length attempts share the ledger; construction/probe failures
consume attempts/time, not a model reservation.

Guidance/turns/repairs can reset local model-attempt counters, not elapsed time,
debit or corrections. Actual changed bytes/new observations can clear generation
continuity; runner success alone cannot. Verification keeps independent approved
limits while consuming elapsed sequence time. Four repairs follow at most five
verification results. Recurrence includes retained command/status/flags/output
and source/test digests. One unchanged completed repair gets reverification;
identical recurring evidence then prevents another repair.

Unknown/non-applied effects, dispatch failures, limits, cancellations, retained
decisions, recording/digest failures stop scheduling. The operation lock spans
the sequence, effects are sequential and the state has one mutable owner; live
journaling uses a mutex. Attempt JSONL links original/repair logs, accounting,
dispositions and causes; verification/trajectory/decision artifacts are
inspectable filesystem evidence, not chat-only state. Configured redaction
applies to retained causes and diagnostics identify elapsed/remaining resources.

Initial observation found two local gaps: required live-result recording could
replace the original model error or conceal returned high known usage before
accounting; final successful digest/evidence work occurred after the last
sequence check, permitting late cancellation/expiry to finalize success.
Both are assigned bounded red-first correction before this phase closes.

Independent offline/locked execution, with `TMPDIR="$PWD"`, passed three
phase-four library and six integration cases, five resilience-repair cases,
five accounting units and one verification-repair authority guard (20 executions,
18 distinct tests). Its initial nonexistent `-p maerg` selection did not execute
tests. Loopback/fake-runner/direct-revision fixtures do not establish imported
detector or sandbox behavior; substring assertions do not prove event-specific
guarantees. Historical nine-red claims were not validated from current source.
A separate scoped security reviewer found no vulnerabilities in this initial
sequence work, not blanket certification.

## Scoped progress observations - resilience phase 3

Fresh `maerg-p3-source-observation` read local agent, controls, progress,
regression, broker and applier source and live-runtime assertions. It excluded
intent, queues, old records/reviews, peer internals and history, made no edits,
and did not delegate. The following is its factual record, not certification.

The adapter supplies actual outcomes to the imported tool tracker; failed
inspections retain nonzero status. Only read/list observations are repeatable.
Successful no-op advisory writes are not new inspection evidence. Effects
compare bounded destination revisions before/after dispatch. Missing files have
no revision; inspection errors propagate. A successful dispatch with differing
revisions supplies state-change evidence and clears accepted-generation history.
Recovery, tool and accepted-generation state remain outside the turn loop;
guidance and compaction do not recreate them. Generation and tool stalls share
two corrections. The first batch stall pairs withheld results before another
request; the five-duplicate fixture asserts exactly one result per proposal and
both distinct correction opportunities.

Hard dispatch failures precede revision/tracker recovery and retain original
causes. Dispatch errors retain uncertain effect placeholders; completed failed
dispatches retain diagnostics and stop later effects. Finalization retains
completed turns, partial effects and known usage on errors. Supplemental
guidance preserves original task wording and undispatched proposals are
superseded. Imported compaction is recorded as noncanonical diagnostic evidence.

The applier's ancestor-validation helper is now crate-visible for reuse by
revision observation; its body, authorization/content identity checks, atomic
replacement and dispatch semantics are unchanged. Existing applier tests cover
creation, overwrite, edit, drift and identity mismatch. A successful dispatch's
`applied` flag is process confirmation, not independent expected-byte
verification. Post-success revision failure aborts, but the retained effect
still has that dispatch-confirmed flag. Trajectory mutation conservatively
includes applied or uncertain effects, including successful no-ops.

Broker filesystem reads occur eagerly during authorization: paired
"not executed" describes withheld adapter results/effect dispatch, not absence
of all authorization reads. Broker no-op comparison trims content despite an
"exact content" message. The actual-change fixture dispatches once and later
identical content becomes advisory, not a second identical effect dispatch.
These limits are retained rather than silently changing intent/evidence.

Independently executed, with `TMPDIR="$PWD"` and offline/locked Cargo:
69 `agent::` units, 53 `authoring::` units (including 13 applier tests), eight
agent CLI and five repair tests passed. A serial full live-runtime run announced
48 tests and exited zero; output truncation obscured terminal counters. A
separate serial `phase3_` run explicitly passed all ten new cases. Shared
fingerprints/window bounds, session compaction/accounting and sandbox backend
internals were not inspected.

Implementer-attributed logs in the private session files directory establish
six initial failures, separate cross-turn, terminal-precedence and linked
ancestor failures, then 48 live/69 agent/53 authoring/eight agent CLI/five
task-command resilience passes. Failed-read and alternating-no-op red runs hit
fixture deadlines, not their intended final assertions. Formatting and strict
all-target lint passed. Synthetic-gate effects use the real applier but do not
qualify Bubblewrap isolation; timing-flake elimination is not established.
Fresh repair invocations still recreate correction/tracker state; sequence
aggregation is assigned to phase four. No generation exception configuration
surface is currently exposed by this adapter.

A separate scoped phase-three security reviewer found no vulnerabilities in
the changed local progress paths. This is neither acceptance nor a universal
security claim.

The separate source-blind `maerg-p3-source-blind` compared only the scoped
intent, phase-three queue definitions, this observation and Sav's public
contract; it did not inspect source/tests or execute checks. It established no
concrete phase-three contradiction, not full compliance. It retained coverage
limits for repeated real effects, novel investigations, changed reads and
imported mechanisms, plus the dispatch/read semantics above. Cross-repair
history recreation remains a known whole-sequence gap for phase four.

## Scoped generation observations - resilience phase 2

Clean-slate `maerg-p2-source-observer` read only local agent generation,
accounting, live/control/retry/review source and generation test assertions.
It excluded all intent, queues, records/reviews, peers and history, made no
changes and executed no checks.

Streaming generation wraps the observed transport with a fresh imported
detector; every returned complete turn receives independent terminal inspection
before length growth, acceptance, broker authorization or effects. Redacted
provisional text can be journaled before rejection, but rejecting output is
not presented or applied. The imported detector's algorithms/bounds were
outside the observer's context.

Each authoring operation receives a separate imported operation token. Detected
repetition records/checkpoints metadata before cancelling that token. Recording
failure and whole-task controls retain precedence; repetition is not a temporal
retry. One recovery budget outside the authoring loop provides numbered feedback,
checkpointed before insertion into preserved conversation. Local code does not
reset it after accepted effects or guidance. Exhaustion produces an unsuccessful,
non-decision result, not completion. The exact two-step default is imported;
inspected tests assert distinct 1/2 and 2/2 opportunities.

Rejected turns never reach authorization, decision/dependency evidence or
effect dispatch. Accepted effects require controls and dispatch-ready checks,
durable dispatch recording, and an uncertain-outcome placeholder before
execution. Complete guidance-superseded calls are paired as not executed.

Task-wide accounting observes every counted return before completed-turn
inspection. Positive usage replaces that attempt's estimate with normalized
provider usage; missing/zero/error usage retains conservative charges and
permanently unavailable cumulative provenance. Known partial totals remain
explicit lower bounds. Length and superseded complete responses are included.
The observer noted that a transport ignoring a rejected callback and returning
success can lose usage when converted to repetition; reachability with native
transports was not established. No high-confidence production defect was found.

Required journal/checkpoint failures propagate. Finalization retains prior
effects, turns and accounting on failure/cancellation, and combines recording
or closure causes where applicable. Review requests use counted inspection but
do not own autonomous authoring recovery or execute returned tools.

Inspected assertions cover all six answer/reasoning 100/150/200-word combinations,
no discarded effects, unary repetition before length growth, unchanged parent
token, recovery numbering across guidance, known/unknown exhaustion, cumulative
length usage and unavailable provenance after failed retry. Injection during
recovery fsync and a short full-owner journal-failure seam remain untested.

Implementer evidence: three corrective HTTP tests reproduced failures before
fixes; known-usage steering already passed. All 11 generation tests pass
concurrently and serially, 63 agent units and 38 full live tests pass; scoped
formatting and strict lint pass. Parent independently reran 11 generation and
63 agent tests successfully. A separate scoped security reviewer reported no
vulnerabilities. No live-model qualification, acceptance or certification is
claimed. Owned operational logs were archived privately and exact paths cleaned;
the one-line review initializer change is implementer-attributed.

### Final completed-stream handoff

The conditional loss reproduced in two deterministic unit regressions:
0 passed/2 failed before the private correction, 2 passed afterward.
Fresh clean-slate `maerg-p2-handoff-observer` inspected only the final private
adapter, counted owner, local observation/latch source and those two assertions,
without intent, records, peers or history. It made no changes and ran no checks.

It observed that the adapter retains the first rejection in an attempt-local
cell while passing a successful underlying turn through required accounting and
journal recording. The counted owner then rejects before reinspection or
acceptance. A later callback or different assembled answer cannot overwrite
latched repetition with its own operation cancellation. Known usage is retained;
token-budget and required-recording failures still have their defined priority.
The tests inspect one attempt, 10,000/100/10,100 accounting, complete provenance,
recorded two proposed tools but no dispatch/decision, and untouched parent
cancellation. No further high-confidence handoff defect was established.

Parent execution after the final fix: all 65 agent units and all 38 live-runtime
tests pass (live serially); strict all-target Kvist/Sav/Skott Clippy passes.
These checks are separately attributed, not the observer's own execution.

Fresh separate source-blind `maerg-p2-final-comparator` read only local intent
and the exact phase-two observation supplement. It found no established
remaining phase-two contradiction after the private handoff correction. Imported
mechanisms, fsync/failure-seam coverage and live-model qualification remain
limits; later progress/aggregate/typed-operational delivery is not certified.
It read no source/tests/history, ran no checks and granted no acceptance.

## Final scoped resilience observations - phase 1

Clean-slate `maerg-p1-final-observer` inspected only bounded current agent/broker
source and the `resilience_review_*` integration fixtures. It read no intent,
queue, prior record/review, root/architecture, peer implementation or chat/Git
history, made no edits, ran no checks and did not delegate. A separate narrow
clean-slate `maerg-p1-patch-observer` subsequently inspected proposal screening
and its contradictory-patch fixture after the final patch-field correction.
Together their current factual observations supersede affected initial details:

- Failed virtual observations map to exit 1; inspection/advisory/completion
  observations map to 0. Model and legacy trajectory use the same mapping,
  and read trajectory records report no mutation. This is virtual outcome
  evidence, not proof that an external process ran.
- Effect success requires an uninterrupted successful exit. Otherwise applied
  is false and mutation may remain uncertain; trajectory mutation conservatively
  means applied or uncertain. Finalization retains completed turns/effects and
  does not permit automatic replay.
- Recognized alternatives are inspected in summary, rationale and optional
  patch. Markdown alternatives headings and labeled option/alternative tokens
  cover later letters/numbers as well as the first label. They prevent completion
  and routine-note demotion. Every broker-retained decision pauses before
  same-turn effects, including after the first turn, without rolling back earlier
  effects.
- Final no-effect termination rejects recognized current incomplete/blocker
  declarations. Blockquotes, fenced code, closed inline quotations and immediate
  negation are excluded; unclosed inline quotes remain inspectable.
- Proposal completion combines summary, rationale and optional patch before
  screening. A contradictory unfinished-work patch cannot be discarded as a
  completion summary. Proposal screening also rejects raw conditional/partial
  substrings and is stricter than ordinary final-report screening.
- Failed-read/edit fixtures assert trajectory exits 1, 1, 0, unchanged source
  and no mutation. Decision fixtures assert two requests, unsuccessful paused
  results, unchanged destinations and actual write/edit `tool_dispatch` absence.
  Legitimate completion, quoted/negated reports and patch-blocker fixtures retain
  exact answer/call-count or decision-disposition assertions.

These are lexical operational checks, not exhaustive natural-language
recognition or semantic task certification. Stream/progress/aggregate-budget
work is outside this phase's observation scope. Initial source observations
still supply the unchanged repair/digest/recording facts below.

Final runner evidence, not independently executed by the observers: initial
review tests failed 3/passed 1, then all four passed; final-blocker,
summary-alternative, inline-quote, Option B and contradictory-patch cases each
reproduced before their fixes. Seven review functions and all 16 live-runtime +
5 task-command resilience cases now pass. Whole affected integrations passed
8 agent / 26 live runtime / 81 task commands before the last one-line patch
screening addition; the complete live-runtime target then passed all 27 cases
after it. Libraries passed 61 agent / 53 authoring / 92 task-command cases;
strict targeted Clippy and formatting passed.

No acceptance, compliance certification or Kvist receipt is claimed.

## Initial scoped resilience observations - phase 1

Clean-slate `maerg-p1-observer` inspected only local agent, broker, task runner,
repair-outcome helper and two integration test files. It excluded intent,
queues, prior records/reviews, chat/Git history and peer implementations. It
edited nothing and executed no checks. Observations at that inspection:

- Four duplicate observation increments or three repeated nonempty assistant
  text increments fail as `authoring stalled`; no synthetic loop-completion
  answer is produced. Novel successful reads reset streaks and can continue
  beyond 30 turns. Warnings permit justified alternative inspection, supported
  completion or a blocker report rather than requiring unnecessary edits.
- Suppression hashes tool, normalized path and the complete bounded observation
  content before delivery redaction/truncation. Range coverage alone cannot
  suppress changed content; confirmed destination writes invalidate read cache
  entries. This does not prove unobserved file bytes are unchanged.
- Failed inspections/guarded edits carry `ReadOutcome::Failed`, do not count as
  useful observation progress and return model-facing status 1 with live
  `applied:false`. Completion notes are heuristic classifications of validated
  proposals; mixed notes/inspections continue.
- Repairs have at most four executions and require a known completed
  verification failure with diagnostics and no cancellation, timeout or output
  overflow. Shared outcome mapping preserves preceding verification cause,
  repair errors, surfaced decisions and cancellation. Source/test digests are
  fallible and unchanged scope explicitly blocks no-progress repair.
- Source/test hashing distinguishes absent/file/directory, sorts names,
  bounds material and rejects unsupported/link-like objects. A changed digest
  does not prove semantic improvement. Recording failures do not establish
  completion or permit replay of uncertain effects.

The initial observer identified three discrepancies requiring corrective work:
legacy failed-read trajectory events hard-coded exit zero; later-turn
alternatives could be downgraded despite broker retention; explicit unfinished
work could match completion phrases. These are review findings, not certification
or an accepted implementation claim. Corrective observations supersede affected
details when recorded below.

Runner-provided evidence at the initial inspection: new cases first failed
5 live-runtime + 4 task-command regressions, plus later completion/feedback
cases. After the initial changes, 9 + 5 resilience cases and library counts
61 agent / 53 authoring / 92 task commands passed. Affected integration counts
were 8 agent / 20 live runtime / 81 task commands. Strict targeted Clippy and
formatting passed. The observer did not independently execute those commands.

This is scoped factual evidence with disclosed unresolved discrepancies, not
compliance certification, acceptance or a Kvist receipt.

Bounded observation of current `maerg` source and test source. This record
describes implemented behavior and its limits; it is not a compliance
determination, approval, certification, or review receipt.

## Runtime shim ownership observations - 2026-11-05

Independent clean-slate `repair-source-observer` read only bounded source/test
ranges, excluding intent, queues, prior records/reviews, chat and Git history.
It edited nothing and executed no checks.

`provision_toolchain_bin` returns a `TempDir` under the project's parent,
containing sandbox-target `rustc`/`rustdoc` links and standard-host `cc`/`ar`/`as`
links. Provisioning errors propagate. Acquisition and offline-verification
callers retain that owner while passing its path to synchronous sandbox
execution. The unit test asserts external placement, no project-local runtime
bin, distinct simultaneous owners, exact links, five entries and independent
drop cleanup. These are inspected assertions, not independently run tests.
Shared acquisition/Cargo-home paths remain; this is not whole-operation
concurrency isolation or a forced-termination cleanup guarantee.

The shell fixture restores a space-containing child directory after component
creation, retains dash-prefixed path coverage, configures inert executables,
and asserts successful acceptance and current project/component states before
capturing queue snapshots.

Attributed implementer evidence: the runtime-bin regression failed first on
project-local placement. A separate hidden-directory regression exposed the
uncommitted `.kvist` scan exemption, which was removed. The actual checkout
then failed scope inspection on five previously generated toolchain links;
those links were moved intact to private session storage outside the checkout.
Both investigation-only terminal debug statements were removed. All 202
selected sandbox/vendor, dogfood, viewer and terminal-host tests subsequently
passed. The two reported acceptance tests also passed 25 repeated runs each
before the runtime-staging change; their original failures were not reproduced,
and neither their assertions nor execution limits were relaxed.

Separate source-blind `repair-intent-comparator` found no concrete narrow
intent/observation mismatch in external staging, owner lifetime and cleanup.
Broader toolchain obligations were outside its scope; full workspace/native
verification was pending. It disclosed incidental prior-record snippets from a
text lookup and excluded them from its comparison. It read no source/tests;
this is a bounded diagnostic comparison, not acceptance or certification.

Final implementer verification: all-feature workspace Nextest run
`e88356b1-350a-460b-8410-a15d620d2c2c` passed all 2,330 tests with 13 existing
skips, including the actual-checkout native sandbox trial (85.810 seconds).
Strict all-target/all-feature workspace Clippy, formatting, doctests and
structural validation of the three changed component bundles passed.
Earlier exploratory runs encountered existing cold-process Mermaid deadline
failures; production deadlines and test scheduling were unchanged, and the
final complete run passed those tests too. Accepted revisions remain unchanged.

## Scoped status and component navigation observations - 2026-11-04

The independent
[`source-only scoped-status observation`](../docs/reviews/scoped-status-observation.md)
and its separate clean-slate correction observation describe the current
implementation. Project status defaults to a compact Markdown table; exact
component selection uses details, and `--details` expands only the chosen scope.
Native version-one text/JSON report shapes and positional project targeting
remain, with `--project` separating a project from a component argument.
Component filtering removes healthy peer/descendant artifact and VCS detail
without recomputing away global diagnostics. Queue reads are bounded and
failures remain explicit rather than becoming zero-task progress.

Both editors visibly insert selected focus for bare `status`, including idle
TUI history recall, and preserve explicit values/deletion. `cd` remains
tab-completable but never prefills a destination; bare `cd` and `cd -` clear
focus, with TUI project transcript/history restoration. Current project and
component states gate displayed execution advice in both Markdown projections.

Observed limits: `--unfinished` preserves the existing non-current-state
filter rather than selecting all incomplete task queues; exact output
filtering does not isolate filesystem inspection from peer/global failures.
The source observation distinguishes test-source assertions from executed
evidence. Protected audit/compliance gates and intent acceptance remain pending.

Implementation-agent execution, separately attributed: all-feature Maerg
Nextest run `4dc3a369-acbe-4159-ac10-5d1ee25b2c00` passed 1083 tests with
7 existing skips. Seven targeted integration suites passed 107 tests, the
bark library selector passed 271, and the corrected explicit-project namespace
fixture passed. Formatting, strict all-target/all-feature Maerg Clippy, release
build, component structural validation and release scoped-JSON smoke passed.
The separate
[`source-blind comparison`](../docs/reviews/scoped-status-comparison.md)
records the bounded agreement and remaining evidence limits, not protected
review completion or intent acceptance.

## Scoped workspace settings observations - 2026-11-04

The fresh
[`source-only settings supplement`](../docs/reviews/workspace-settings-observation.md)
and its correction addendum derive the current settings behavior without
intent, queues, prior implementation records, reviews, or Git history.
Its four original findings are historical: actual idle-Esc dispatch, canonical
profile preservation during role clearing, XDG theme lookup, and absent-user
fallback disclosure were repaired and independently reobserved.

The observed implementation provides independent RGB dark/light palettes with
17 semantic roles, raised surfaces and selection colors. Contrast assertions
require 4.5:1 for tested text/accent combinations and 3:1 for borders; they do
not certify arbitrary custom themes or syntax highlighting. Typed Esc/F2 menus
retain parent cursor/filter and command drafts, distinguish current markers
from selection, and share the idle/active-confirmed Quit path. Markdown detail
and confirmation documents scroll through all warnings.
The final source-only addendum observes F2 from every overlay, cancellation
of a staged confirmation without authorization, and refusal of persisted
theme/shortcut changes while an owned command is active. These supersede the
earlier observation's contextual F2 and active-preference limitations.

Searchable component shortcuts persist privately with at most 12 pins and
12 recent entries. Typed filesystem-only inspection supports all three roles,
project/user scope, effort inheritance distinct from explicit `none`, model
details and source-specific lifecycle actions. Registry mutations pin source
and revision, refuse known references before disable/removal, and disclose
known fallbacks and unknown external consumers. Setup/check retain explicit
host acknowledgement; none of these settings approve protected execution.

Observed limits remain explicit: registry thinking/default-effort metadata can
be unknown; lower registry layers are not a complete independently editable
inventory; references cannot enumerate external projects; cooperative locks
do not cover arbitrary writers or every older unpinned setup/removal path.
The source-only record distinguishes inspected assertions from executed
evidence and does not establish universal concurrency or provider behavior.

Implementation-agent execution, separately attributed: the final all-feature
workspace Nextest run `4d20f182-7fb9-43d9-a043-4fb943c4e90e` passed 2304 tests
with 15 existing skips at two-test concurrency. The concurrent all-feature
Maerg library run after the final F2/preference corrections passed 652 tests
with one existing ignore; six targeted
suites passed 52 tests. Formatting, strict all-target/all-feature Maerg Clippy,
component document validation and the final release build passed. Release
smoke checks verified finite bare Markdown/help and structured role-list JSON.

The separate
[`source-blind settings comparison`](../docs/reviews/workspace-settings-comparison.md)
and correction addendum record bounded agreement. Its F2 discrepancy and
active-preference interpretation boundary remain historical: the former was
repaired, and persisted preference changes now follow the broader active-command
restriction without weakening the requirement. The comparison read the
post-correction library/targeted evidence before the final workspace/release
reruns; the latter results above are attributed to the implementation agent.

The earlier workspace run exposed two queue-order failures, repaired by
sorting the new task requirement references, plus unchanged Blad deadline and
checkpoint index-lock failures. Those latter tests passed in isolation and
the final workspace run; their causes are not established. Protected audit
and compliance queue gates and intent acceptance remain separate and pending.

## Scoped Markdown workspace observations - 2026-11-03

The independent
[`source-only workspace observation`](../docs/reviews/markdown-workspace-observation.md)
and its correction and host-diagnostic addenda describe bare interactive entry,
Markdown command presentation, recursive JSON report projection, Blad rendering, the editable
completion footer, isolated child I/O/cancellation/EOF, settings, and private
component-scoped history. The observer excluded intent, queues, prior records
and Git history. Five concrete fidelity/I/O findings were corrected; the
original findings remain historical, not descriptions of the corrected source.

History uses a 1 MiB page target and 16 MiB individual-block ceiling, preserving
top-level Markdown blocks rather than arbitrary byte slices. The global
scrollbar uses segment bands, not an unbounded line index. Each page renders
independently; cross-page reference links/footnotes are not a whole-history
document. A failed/interrupted rotation can duplicate diagnostics. Filesystem
checks do not isolate ancestor races or kernel stalls. Configured redaction is
not general secret discovery, and a child may echo its stdin answers.

Host-thread tracing uses the existing environment filter and a bounded,
nonblocking diagnostic channel, then fenced Markdown rather than direct terminal
writes. Fragment truncation preserves UTF-8 boundaries and saturation is
reported explicitly. This is scoped thread-local capture, not interception of
arbitrary background-thread logging or a final-drain guarantee on failure/exit.

Implementation-agent evidence, separately attributed: the final all-feature
workspace run passed 2251 tests with 15 existing skips at two-test concurrency.
The preceding default-concurrency run passed 2249 tests but two unchanged Blad
assertions exceeded their rendering deadlines. An earlier concurrent library
run hit an unchanged checkpoint signer-start timing assertion. No root cause
is established for those failures. The final concurrent single-process library
rerun passed 625 tests with one existing ignore. Targeted logging/Markdown/PTY
regressions, formatting, strict all-target Maerg Clippy and the release build
also passed.

The separate
[`source-blind workspace comparison`](../docs/reviews/markdown-workspace-comparison.md)
records scoped agreement and remaining evidence limits, including its disclosed
initial overbroad document read. Protected audit/compliance queue gates remain
pending; no native receipt, intent acceptance, policy approval or Git publication
is implied.

## Scoped numbered decision observations - 2026-10-31

The fresh
[`source-only shortcut observation`](../docs/reviews/decision-shortcuts-observation.md)
describes the authenticated component-scoped response registry, typed native
dispatch, retained-plan discovery and read-only completion snapshots. It
excludes intent, queues, prior records and Git history. Its narrow correction
addendum records native `plan --catalog` replay equivalence after the observer
identified a hidden numbered-only freshness pin. The original finding remains
historical evidence, not a description of the corrected dispatch.

The observer records retained-registry numbering, historical/stale listings,
bounded storage with no pruning, and repeated ordinary continuation rather than
an exactly-once continuation promise. The real PTY test source exercises Tab
selection and deliberately stale refusal; successful numbered application and
interrupted recovery are separate integration/unit assertions.

Implementation-agent execution, separately attributed: the missing short
commands and later expanded-command discrepancy were reproduced before their
repairs. The final post-correction Maerg suite passed 982 tests with seven
existing ignores using one test thread. An unchanged timing-sensitive checkpoint
assertion failed in earlier concurrent runs and passed in isolation and the
final suite. Formatting, strict all-target Clippy, release build, command help
inspection and component validation passed.

The separate
[`source-blind shortcut comparison`](../docs/reviews/decision-shortcuts-comparison.md)
independently read the final aggregate log and established no current concrete
intent contradiction within its permitted evidence. It retains replay/editor
coverage and aggregate-log proof limits. These diagnostic records do not close
the pending protected security/compliance tasks or authorize human acceptance.
No live Galla decision, intent acceptance, installed-binary replacement or Git
publication was performed.

## Scoped deterministic decision observations - 2026-10-30

The fresh
[`source-only decision observation`](../docs/reviews/decision-automation-final-observation.md)
describes the actual structured alternatives, bounded discussion, agent/manual
planning, authenticated exact application, queue edits and recoverable
continuation. Its evidence boundary excludes intent, prior implementation
records, review history and Git history; it is a scoped diagnostic supplement,
not protected execution, independent compliance certification or human approval.
The earlier
[`provisional observation`](../docs/reviews/decision-automation-observation.md)
records the pre-hardening state rather than subsequent behavior.

Implementation-agent execution, separately attributed: the final Maerg suite
passed 973 tests with seven existing ignores. Formatting, strict all-target
Clippy, release build and component validation passed; the test-only lint
correction was also covered by a 39-test component-focused rerun.
The separate
[`source-blind diagnostic comparison`](../docs/reviews/decision-automation-comparison.md)
established no concrete unmet decision-workflow requirement in its permitted
evidence and retained semantic, bounded-capture and platform proof limitations.
No live Galla decision, protected security/compliance task execution, native
review receipt, intent acceptance or separate installed-binary replacement
was performed.

## Scoped human attention observations - 2026-10-30

The separate clean-slate source/test observations are retained in
[`attention-brief-observation.md`](../docs/reviews/attention-brief-observation.md)
and the later narrow
[`retention observation`](../docs/reviews/attention-retention-observation.md).
They did not read intent or execute checks. Selected source-derived observations:

- Proposals and reviews request agent-written Markdown explanations; section
  presence and factual quality are not mechanically established.
- Review JSON has attempt/task Markdown companions. Decisions name an
  operational attention brief with the agent explanation and host-written
  choose/clarify/reject/resume instructions. Error wrappers explain task and
  continuous-run failures without another model call.
- Briefs use redaction, a 256 KiB bound, content-addressed history and replaceable
  task/run aliases. Invalid configuration, unsafe paths and persistence errors
  can prevent a brief, with explicit diagnostics. Summaries do not grant approval
  or clear recovery fences.
- The initial observer identified reused call IDs overwriting raw proposals.
  The later observer describes session/turn/ordinal-bound names and no-clobber
  JSON/Markdown publication. Publication is not pair-transactional: partial
  evidence can remain on failure, and replaying the same identity is refused.
- Output-limited explanations are disclosed; raw companions preserve proposal
  data subject to upstream bounds. Source observations do not establish model
  recommendation quality, actual rendered UX, freshness or concurrent path safety.

Implementation-agent execution, separately attributed: the missing decision
Markdown and reused-call-ID overwrite were reproduced before their repairs.
The final Maerg suite passed 920 tests with seven existing ignores; format
checking, strict all-target Clippy, release build and document validation passed.
One unchanged checkpoint timing assertion failed in an earlier full run, then
passed in isolation and in the final unchanged full run. Attempt-specific and
latest Markdown summaries for Galla's retained attempt-0002 were generated from existing report
and intent, without source validation or changing its paused task/ledger.
No protected review execution, receipt, human acceptance or installed-binary
replacement is implied.

The separate
[`source-blind diagnostic comparison`](../docs/reviews/attention-brief-comparison.md)
found no established current blocking contradiction in the supplied evidence.
It retains partial multi-file publication, prompt quality, bounded excerpts and
unsafe/unavailable publication paths as limitations or arbitration points.
The comparator disclosed exposure to unrelated IMPL snippets during its initial
search; this is not claimed as a strictly exclusive-read review or a protected
gate. The queued security/compliance tasks remain pending.

## Scoped continuous-execution observations - 2026-10-30

The independent clean-slate source/test observation is retained in
[`continuous-execution-observation.md`](../docs/reviews/continuous-execution-observation.md).
For scheduling, checkpoints, review orchestration and shell prefill, this scoped
record replaces older behavior descriptions below; it does not certify
compliance or acceptance. Selected observations from that record:

- `task run COMPONENT_DIR [TASK_ID]` requires the component argument; an exact
  ID runs only that task, with no item-prefix or special `all` expansion.
  Omitting the ID enters continuous execution.
- Continuous execution holds one component operation lock, re-reads the queue,
  and chooses the first declared pending task whose direct and transitive
  dependencies are completed.
- Successful implementation work undergoes sandboxed verification. Continuous
  mode then marks execution completed without human finalization; an explicitly
  selected implementation task instead remains pending human disposition.
- Item commits follow completion of all inferred members; component policy
  commits only at final completion; manual policy skips Git cleanliness
  preflight and automatic commits, but still inventories owned files.
- Owned paths are only component `src/**`, `tests/**`, `TODOS.yaml`, `IMPL.md`,
  and `.kvist/continuous-reviews/**`; the ledger, attempt journals, live logs,
  intent files, and unrelated staging are outside checkpoint ownership.
- Security tasks receive a source/test audit frame. Compliance tasks first
  request source-only observations, persist an attempt-specific observation,
  replace `IMPL.md` with that observation record, then request comparison using
  local intent, root/optional parent contract, the new record, and verification
  evidence, not a raw source field.
- Focus insertion edits the real Reedline buffer using shared component-slot
  parsing; explicit/partial values win, deletion is remembered until the next
  submission, quoting and dash-leading paths are handled.
- Paused edits do not automatically invalidate completed reviews or rerun
  verification. Source frames omit root build inputs; binary content is
  digest-only. Model truth and every abrupt-crash interval remain unverified.

The initial observation also identified diagnostic-escaping and unbounded
checkpoint-process gaps. Corrections and their independent follow-up evidence
are recorded separately; the initial findings are preserved rather than
represented as a clean certification.

The fresh source-only
[`correction observation`](../docs/reviews/continuous-execution-corrections-observation.md)
records bounded checkpoint supervision, decoded/escaped diagnostic redaction,
capacity-aware review framing, terminal new-work baselines and explicit path
dispatch. Its whitespace-only validation finding was subsequently reproduced;
the separate fresh
[`predicate observation`](../docs/reviews/continuous-redaction-predicate-observation.md)
describes the corrected rejection while retaining nonblank literal bytes.
Administrative path announcements, approximate token accounting, escaped
descendants and non-exhaustive crash recovery remain disclosed limitations.

Implementation-agent execution, separately attributed: the final Maerg package
suite passed 914 tests with seven existing ignores and no failures.
Workspace format checking and strict all-target Maerg Clippy passed. The release
binary was rebuilt and `component validate maerg` passed. Real PTY assertions
submitted both a visibly prefilled dash-leading child component and an explicit
absolute child override, reaching the missing-policy-approval gate without task
effects. Source observation and source-blind comparison remain separate from
these execution results; no real provider task, acceptance, installed-binary
replacement or publication is implied.

The separate
[`source-blind comparison`](../docs/reviews/continuous-execution-comparison.md)
retains one concrete discrepancy for human arbitration: design wording promises
group-wide reaping, while observations establish process-group termination and
direct-child reaping only. It proves neither a surviving live helper nor a
runtime hang. Coverage gaps, evidence freshness after paused edits and
administrative-path redaction ambiguity remain explicit. The queued protected
security/compliance tasks remain pending; these diagnostic records do not
fabricate their execution, acceptance or receipts.

## Scoped runtime-budget observations - 2026-10-30

Independent clean-slate `maerg-budget-observer` read only scoped current
`src/config.rs`, `src/agent.rs`, `src/agent/live.rs`, `src/task_commands.rs`
and `tests/task_commands.rs`. It excluded intent, queues, prior records,
reviews, configuration files, execution logs, peer implementations and
chat/Git history, made no edits and executed no checks. Its observations:

- Role defaults and the generated template specify 7200 seconds and
  16777216 bytes (16 MiB). Configuration ceilings are 30 days and 16 MiB.
  `token_limit` defaults to absent; configured values must be positive.
- Configuration test assertions retain a 17-second timeout and byte caps of
  1, 65536 and 16 MiB, and reject zero and above-maximum output caps.
- Model requests and effects receive time from one elapsed-time budget, with
  a separate 86400-second per-operation ceiling. Retry callers receive the
  same budget; this does not establish a deadline for the entire CLI lifecycle.
- After context startup, stderr and `execution_limits` report task timeout,
  a remaining-time/native-policy request ceiling and response bytes. They
  distinguish protocol overhead/reasoning bytes from generated tokens.
- The byte limit is passed separately to transport and effect capture.
  Retained output and logs are redacted before UTF-8-safe truncation. Live
  text is recorded in UTF-8-aligned chunks of at most 8192 bytes after
  redaction, leaving JSON-escaping headroom under the journal record bound.
- Test source asserts responses exceeding 2 MiB, escaped/UTF-8 deltas,
  exact streamed-text reconstruction, records below 65536 bytes, and split
  secret redaction. The explicit 64-byte cap asserts blocked execution,
  bounded logs and secret-free evidence.
- The overflow branch blocks execution and requests reviewed limits, policy
  reapproval and deliberate retry. Approval material binds budgets; the
  changed-policy test asserts refusal before queue mutation or run creation.
- These are source observations and unexecuted test assertions, not actual
  model behavior, dependency-enforcement proof or independent compliance
  orchestration. Retry internals were outside this observer's scope.

Implementation-agent execution, separately attributed: the new budget tests
failed against the old defaults/1-MiB maximum; the large-response integration
reproduced the 65536-byte failure. Enlarged text then reproduced the separate
65536-byte journal-record failure before chunking. After repair,
`cargo test --offline --locked -p kvist -p sav --quiet` passed 1084 tests
with seven existing ignores. Strict all-target Clippy for both packages,
format checks, 57 Skott configuration/model-selection tests, the Kvist release
build and both components' document validation passed. This is execution
evidence, not independent review or acceptance. The reported shared-runtime
compliance task, explicit user policy and retained attempt evidence were not
replayed, accepted or certified.

### Independent source-blind budget comparison - 2026-10-30

Separate `maerg-budget-comparator` read only scoped local requirements,
contract, design and the observation/execution supplement above, without
source, tests, queues, prior records/reviews, peers or history. It executed
no checks and made no edits:

- Observed 7200-second defaults and 16-MiB byte defaults/maxima match scoped
  intent; smaller explicit caps and approval binding have supporting
  observations and test assertions.
- Startup task/request/response diagnostics align with intent, but underlying
  transport/profile enforcement is not independently established by this scope.
- Overflow blocking, byte/token distinctions and reapproval guidance align.
  Excluded retry internals leave automatic retry/replay prohibition unproven
  by the scoped source-only observation.
- Redacted UTF-8-safe 8192-byte chunks and reconstruction assertions support
  lossless recording beneath the unchanged 64-KiB record bound.
- No concrete scoped discrepancy was established. Executions remain
  implementer-attributed; acceptance, compliance certification and independence
  of the unrelated review task are not inferred.

## Scoped absent-timestamp recovery observations - 2026-10-29

Independent `timestamp-metadata-observer` read only scoped current Maerg
implementation and test source, excluding all intent, queues, prior records,
reviews, architecture/root intent, implementation chat, Git/history and
execution logs. It neither edited files nor executed checks. Its observations
supersede the prior missing-timestamp blocked-record behavior below:

- Missing/null timestamp containers and absent creation/update dates receive
  synthetic values. Creation uses the earliest of the epoch and valid available
  date bounds; missing update copies creation. Supplied dates remain unchanged.
  Unknown completion remains null; `recovered: true` identifies uncertainty,
  not observed completion.
- Known dates retain whole-second UTC calendar and chronology validation.
  Completed tasks may have null completion only with recovered provenance;
  non-completed tasks still cannot carry completion dates.
- Augmentation preserves task status, authored data, dependencies, blockers,
  finalization references and component metadata. Separate normalization sorts
  set-like lists and dependency order. Malformed supplied history still uses
  the approval-gated blocked/stale salvage path.
- Lossless normalization independently needs no approval. Any approval-required
  operation holds the entire plan; dry runs never apply it. Application
  rechecks snapshots and evidence under locks, creates the original-byte
  backup before mutation and records progress. Partial failures remain possible
  and explicitly reported.
- Evidence guards inspect validated normalization candidates or salvage
  originals. Active/fenced tasks are rejected; application repeats ordinary
  guards. Source assertions cover empty evidence storage and refusal of
  unreadable/schema-bearing unresolved journals.
- Inspection and status recommend ordinary repair for missing metadata.
  Assertions cover preserved fields/authority, pre-epoch dates, backups,
  idempotence, mixed-plan deferral, explicit resumption, supplied-date
  validation and valid siblings.

The observer identified no concrete scoped defect. Provenance remains
task-wide rather than field-specific; it did not inspect backup-storage or
evidence-parser internals and did not execute the assertions.

Separately attributed implementer execution observed failing metadata,
unknown-completion and obsolete-guidance regressions before their fixes.
Formatting, strict all-target Clippy and the release build passed. The full
Maerg suite passed **845 tests**, with seven existing ignores and no failures.
An actual-project nonmutating preview planned only `blad/TODOS.yaml`, retaining
the four completed task statuses without requiring approval or changing live
bytes. These observations and results are not acceptance or certification.

Independent source-blind `timestamp-metadata-comparator` compared the scoped
requirements, contract and design with these source observations and attributed
execution evidence, without source access or execution. It found no definite
scoped contradiction and confirmed the 845-pass/seven-ignore totals without
double-counting targeted runs. It retained unobserved backup/evidence internals,
the lack of independent assertion inspection and the distinction between preview
and application evidence for human arbitration. Its comparison grants no
certification, acceptance or authorization.

Subsequent implementer application installed the verified release and ran
ordinary repair without an approval, reset or clean slate. Only
`blad/TODOS.yaml` changed: all 156 task identities/statuses and the exact
component metadata header were preserved; the four absent timestamp records
received explicit unknown-history metadata. Other component queues were
unchanged by repair. The complete manifest and byte-verified original remain at
`.kvist-recovery/recovery-gPqp2N/originals/blad/TODOS.yaml`. Installed `doctor`
reports a current, structurally valid project, and repeated repair proposes no
operations or new backup. Existing component staleness/task blockers were not
accepted or bypassed. This live execution is implementer-attributed, not an
independent compliance determination.

## Scoped recovery-defaults observations - 2026-10-29

Independent `recovery-defaults-observer` read only current Maerg implementation
and test source. Its context excluded intent documents, the queue, prior
records/reviews, peers, chat/Git history and execution logs. It did not edit
files or execute checks. These source-derived observations supersede the older
recovery defaults, timestamp, fence and placeholder statements below within
this changed scope:

- Salvage planning is enabled by default. Valid queues remain byte-for-byte
  unchanged. Invalid queues first try lossless set normalization and stable
  dependency ordering, which can apply without approval but with a backup.
  Salvage, Markdown reconstruction, reset, clean slate and AI replacement
  require exact approval; one such operation holds the whole plan.
- Missing, partial, invalid or completion-deficient timestamp history retains
  otherwise-valid task data and dependency edges as blocked records. Synthetic
  epoch creation/update values, null completion and
  `timestamps.recovered: true` explicitly identify unknown history. Prior
  finalization references are cleared with an explanation; the original
  blocker and claimed status remain explained or backed up. Canonical
  serialization emits true provenance and omits false. Explicit resumption
  preserves it while later transitions record real dates.
- Native queue validation still gates candidates. Other salvage can reconcile
  contradictory dates under explicit assumptions, remove unknown fields,
  merge identical duplicates, and quarantine invalid/conflicting tasks and
  affected dependants without weakening edges. Fragment recovery makes a
  same-indentation segmentation assumption. Unsupported schemas require reset.
- The bounded Markdown scanner recognizes ATX headings, backtick/tilde fences
  and HTML comments, not full Markdown semantics. Subordinate sections can
  contain required-section content; fenced example headings are inert.
  Reconstruction preserves material and adds active recovery blockers to
  missing/empty sections. Reset drafts are blocked too; formatting-only repair
  need not introduce a blocker.
- Structural validity and readiness are separate. Component validation can
  succeed while reporting blockers. Global placeholders block project
  readiness; local placeholders and nearest-parent contract placeholders
  block components, including across namespaces. Parent private intent does
  not propagate. Acceptance explicitly rejects placeholders; ordinary task
  selection requires current state. Removing markers is not semantic
  verification of the replacement prose.
- Explicit native AI inference has no tools or workspace executor and stages
  a new private, no-clobber JSON proposal instead of live edits. Each context
  contains one target, available local intent, nearest ancestor contract and
  root contract, not peer implementations. Limits are eight targets, 64 KiB
  target/replacement text, 256 KiB context/response, 1 MiB proposals, reserved
  output capacity and a shared 120-second deadline. External profiles,
  incomplete output and unsuitable destinations fail explicitly.
- Frozen-proposal inspection/application does not perform inference.
  Replacements pass native validation. AI queues become stale blocked
  unknown-history drafts without completion/finalization authority; Markdown
  receives an active review blocker. Faithful preservation of task identities,
  edges and authored meaning is requested from the model, not proven by a
  semantic comparison. Exact complete old/new text is exposed for review.
- Approval binds the project, inventoried snapshots and ignore policy,
  operation contents/reasons, backup storage and frozen-proposal fingerprint.
  Application rechecks inputs and locks, privately copies/synchronizes/verifies
  ordinary originals, and quarantines links/directories intact without
  following targets. Quarantine requires same-filesystem storage. Copies
  preserve bytes and permission bits, not every filesystem attribute.
- Active/fenced attempts, unresolved evidence and live/stale/malformed locks
  are not silently bypassed. Approved clean slate archives workflow authority
  without asserting prior effects are absent. Application is per-file, not
  transactional; partial changes and lagging manifests remain possible, with
  retained originals and no automatic rollback/replay.
- Shell completion offers flags on empty arguments and dash prefixes, but
  suppresses them for pending values and completed separators. It does not
  invent approval tokens or provide filesystem-path suggestions.

The observer inspected assertions for timestamp retention/provenance, blocker
gates and parent boundaries, approval/backups, AI staging/tamper rejection,
completion and status output. These were source observations, not its execution.
Source-only follow-ups observed action-qualified discard explanations in AI
eligibility, including conflicting duplicate identities, without interpreting
action words inside authored text as loss. Classification still matches
explanations, not a structural loss analysis. They also observed clarified queue
reporting that acknowledges listed status/data changes. An intermediate test
placement error was corrected and its module-level declaration independently
confirmed before final verification.
The final diagnostic follow-up observed artifact-specific placeholder guidance
instead of generic blocked-task transition advice, with a matching regression.

Separately attributed implementer execution: formatting and strict all-target
Clippy passed; the complete Maerg suite passed **840 tests**, with seven
existing ignores and no failures. Exact 64 KiB candidate, 256 KiB context and
64 MiB aggregate text boundaries were exercised. An isolated interactive PTY
check displayed repair flags after Tab on bare `repair `. An actual-project
nonmutating preview retained all four timestamp-less Blad tasks in a blocked-record
proposal and recognized its nested design section as valid. Before/after hashes
confirmed both live Blad files were unchanged. Destructive applications and
native-model responses were exercised in isolated fixtures, not against live
project artifacts or a production model. No acceptance or certification is
asserted.

Independent source-blind `recovery-defaults-comparator` compared the scoped
requirements, contract and design with these observations and attributed
execution evidence, without source access or executing checks. It identified
one concrete reporting discrepancy: a generic status-preservation explanation
contradicted the explicit completed-to-blocked conversions in the preview.
That finding is retained for human arbitration; the generic explanation has
since been clarified to acknowledge all listed status/data changes, with a
regression assertion and updated nonmutating preview. The initial comparison
established no other definite behavioral contradiction. It could not establish
every diagnostic, filesystem failure mode, command gate or AI grammar/capacity
obligation, nor prove AI semantic faithfulness. Live destructive application
and production-model behavior remain unverified. These limits are not
certification, an acceptance receipt or permission to apply a recovery plan.
Its source-blind follow-up found the current preview's reporting inconsistency
resolved, confirmed the attributed 840-pass/seven-ignore/zero-failure totals
without double-counting targeted runs, and retained the earlier finding and
limitations. It did not execute checks, certify compliance or authorize
recovery.

## Scoped artifact-recovery observations - 2026-10-29

Independent `recovery-observer` read only recovery, CLI, queue, storage,
discovery, task-lock/evidence and relevant test source. Independent
`recovery-scope-observer` subsequently observed only the new Markdown,
namespace inventory, selected-root, normalized-path, ignore-policy and CLI
deltas and their test source. Both contexts excluded intent, queues, prior
records/reviews, peers and chat/Git history; neither edited files or executed
checks. Their source-derived observations are:

- Plain/JSON repair share library options and reporting. Valid queues remain
  exact. Invalid set lists and acyclic task declaration order can normalize
  automatically; candidates pass ordinary strict validation. Reports succeed
  only when their remaining-problem list is empty; JSON includes operations,
  approval and backup location as well as existing state/queue fields.
- Approved queue salvage reconciles existing timestamps under stated
  assumptions, removes unknown fields, preserves independently valid tasks,
  merges identical recovered duplicates, and removes conflicting/invalid tasks
  and affected dependants. Missing completion evidence is not invented and
  dependency edges are not weakened. Version-one fragment salvage explicitly
  assumes same-indent segmentation; active/fenced indications refuse it.
- Salvaged/reset queues carry synthetic stale/unaccepted metadata, not
  observed revisions, acceptance or successful verification.
- Markdown salvage preserves prose while reconstructing required sections and
  labeled draft content; known unsupported versions and ambiguous fenced
  section rewrites refuse salvage. Output remains bounded to 1 MiB.
- Configuration-independent inventory and ordinary inspection supply known
  components. Configured/selected namespace roots are seeded explicitly;
  relative aliases and visited paths are deduplicated. `--component-root`
  requires configuration reset or clean slate. Root `.gitignore` is an
  approval input; its directory rules exclude ignored scratch worktrees.
- Managed artifact resets use native drafts; clean slate archives workflow
  objects without resetting source/VCS files. Component-bearing state
  directories preserve source. Existing effects remain unknown; no task replay
  or global-grant reset occurs.
- Exact approval binds originals, object permissions, paths, replacements,
  reasons and backup location. Dry runs and unapproved proposals write nothing.
  Regular originals are privately backed up, synchronized and verified;
  wrong-type objects are quarantined intact without following link targets.
  Same-filesystem quarantine may require alternative in-project storage.
- Shared task-operation locks block mutation. Inputs are rechecked under locks,
  and non-clean-slate writes guard original queue/evidence, including
  authenticated unresolved attempts.
  Clean slate archives evidence only under approval, without clearing unknown
  execution effects or taking over locks.
- Replacement is atomic per file, not across the project. Progress records can
  lag actual mutation after interruption/manifest failure; no automatic
  rollback or replay is provided. Resource, permission and storage limits are
  explicit blockers rather than permission to discard unbacked data.

The supplemental observer noted that ignore matching is the existing bounded
`*`/`?` subset, not full Git semantics; only root ignore rules are used for
recovery traversal. Markdown version detection examines the first nonblank
declaration, not an arbitrary embedded declaration. Structural validity is not
component acceptance, readiness or compliance. Reset does not bypass filesystem
inventory/snapshot bounds; stale/malformed locks can still require ordinary
recovery or a separate newly initialized project.

Execution attributed to the implementing context, not either observer:
initial regressions failed before implementation; subsequent targeted recovery
coverage passed 32 integration tests. The full Maerg suite passed 819 tests,
with seven existing ignores. Strict all-target Clippy and formatting passed.
An actual-project nonmutating dry run reports all root/discovered component
artifacts structurally valid, including the corrected completion timestamp;
ignored scratch worktrees are excluded. Destructive recovery was exercised
only in temporary fixtures, never applied to the live project. No component
acceptance or operational-task replay was performed.

Independent source-blind `recovery-comparator` compared the scoped requirements,
contract and design against these observations and attributed execution,
without implementation/test-source access. It established no definite
contradiction, but did not independently verify every obligation: non-queue
evidence preservation, all nonzero/partial-failure reporting paths, all
filesystem/aggregate bounds and detailed proposal display remained unverified
in its permitted record. These are retained for human arbitration, not silently
classified as compliance or approval.

After that comparison, implementing-context regressions additionally verified
that a Markdown reset cannot bypass an active component task and that exactly
64 MiB of retained queue text is accepted while one additional queue is refused.
Existing executed tests cover invalid UTF-8 resets for all nine native artifact
paths, backup-failure immutability and alternative storage, and link/parent
protection. These checks supplement, but do not replace or retrospectively
broaden, the independent observations/comparison.

## Scoped transition-matrix verification - 2026-10-29

Independent clean-slate `transition-observer` derived observations from
Maerg's task commands, queue, guidance and scoped test source only. It excluded
all intent, queues, prior records/reviews, peers, chat/Git history and execution
logs, made no edits and ran no checks. Its initial observation exposed a
missing other-task scan on acceptance. After that repair, a separately scoped
clean-slate `transition-fence-observer` observed only the shared scan, its two
call sites and two new test sequences, under the same exclusions. The delta
observation was not a repeat whole-implementation review. Their source-derived
observations are:

- The five-state matrix contains ten legal edges; completed is terminal and
  self-transitions are refused. In-progress entry checks the complete
  dependency chain, including decision resumption. Blocked/decision entry
  requires and persists a nonblank reason in both queue and transition events;
  leaving those states clears the queue reason. Direct human
  in-progress-to-completed transition remains an ordinary legal operation.
- Ordinary transitions permit current/blocked components; execution, selection
  and result acceptance require current. Unlock, no-launch recovery, block
  disposition and log inspection permit current/blocked/stale, retaining
  current-project, tracked-artifact, discovery and structural checks.
- Unlock validates lock shape and Linux process liveness, quarantines and
  rechecks the stale lock before removal. Force skips only confirmation.
  Unresolved-attempt checking follows removal; an error can truthfully report
  that the lock was removed while task state remains unresolved.
- No-launch recovery validates authenticated prepared/failure/recovery-chain
  evidence, launch/write-exposure false, current approval, runner, scope and
  exact queue digests. It resumes reconstructed before/after replacement states
  and restores only pending/in-progress origins without accepting intent.
  Ordinary prepared execution evidence additionally accepts a decision origin.
- Both finalization dispositions check bounded latest attempt identity,
  selected-record schema/task identity, prepared presence and conflicting
  dispositions/recovery decisions. They reject their own explicit fence.
  The final shared other-task scan rejects sibling fences and unresolved
  journals before either completion or block queue mutation. Its resolved
  predicate includes disposed execution and fully authenticated recovered
  chains, not merely the existence of a recovery receipt.
- Acceptance stores completed status, attempt acceptance identity, disposition
  and timestamps before synchronously appending final evidence. A matching
  completed non-committing repeat finishes missing recording or returns the
  existing decision without queue rewrite or duplicate event. A repeated commit
  request is refused with separate pending-commit guidance. Block retains its
  matching-reason, queue-first append-only repeat semantics.
- Status, shell and blocked-state hints reopen with a reason-free pending
  transition, rather than suggesting execution under blocked component state.
  Help no longer describes verification alone as completion.

The delta observer specifically traced a test which first leaves one task
awaiting human disposition, then creates an authentic no-launch fence for an
independent task. Acceptance must refuse with queue and both journal byte
sequences unchanged, then succeed only after sibling recovery. Its other test
places a pending ancestor above a completed direct predecessor and checks
pending/blocked/decision in-progress entries all refuse without queue/journal
mutation. These are source assertions, not execution claims by the observers.

**Attributed execution, not independent observation:** the implementing session
reproduced the repaired defects before their production fixes. Its final
`cargo test --offline --locked -p kvist` passed 772 tests across 37 suites with
seven existing ignores. Eleven new integration tests include 200
state/reason/dependency combinations, all 25 stale-intent pairs, structural/VCS
gates, live/dead/malformed locks, stale authenticated recovery, decision
resumption, finalization identity and repeat/interrupted recording, sibling
recovery preservation and indirect dependencies. Strict all-target Clippy,
format/whitespace checks and component structural validation passed. Full named
execution output is retained in the session's `transition-verification-tests.log`.

**Observed limitations retained for arbitration:** selected finalization's
identity check does not itself run prepared-field/HMAC assessment or require
successful execution/verification/pending-human-disposition records. Acceptance
change inventory still trusts reported scopes when present, checks optional
post-digests only when reads succeed, and does not reject every unknown operation.
Pending commit-record creation/write failures are still ignored. Those
pre-existing broader acceptance limitations are not certified by this repair.
Reconstructed partial states are not actual power-loss or fault injection:
torn appends, disk-full/fsync failures, check/write races, quarantine races,
PID reuse and non-Linux behavior remain unverified. An interrupted unversioned
ordinary-transition prepared tail remains a refusal, not authenticated
no-launch recovery. No universal crash recovery, unsafe force-reset, provider
behavior, production sandbox isolation, security or overall compliance
certification is claimed.

The existing operational Maerg review task was observed pending at
`2026-10-29T09:50:40Z`; this request did not change it, its attempt journal or
the human's accepted revision metadata. Local intent changed deliberately and
remains stale pending human review/acceptance. The installed binary was not
replaced and no commit was created.

**Independent source-blind comparison:** `transition-comparator` received only
the scoped requirements/contract/design/task definitions, the independent
observations and attributed execution summary, with no tools or source access.
It found no demonstrated contradiction in transition adjacency, state gates,
supported no-launch restoration, sibling fencing or non-committing repeat
semantics; that is bounded support, not certification or proof of deadlock
absence. It retained for explicit human arbitration the acceptance read-failure
and optional-digest ambiguity, durable pending-commit record failures, and the
operational treatment of unsupported interrupted ordinary-transition tails.
Snapshot checks do not establish fail-closed validation of all accepted file
contents. None of these broader limitations was silently repaired, waived or
treated as approved by the comparison.

## Scoped stale-attempt abandonment repair - 2026-10-29

Independent clean-slate `stale-abandon-observer` inspected only current
task-command validators, finalization, queue serialization, lock/journal
helpers, guidance and scoped test source. It excluded intent, queues, prior
records, reviews, peer implementations, chat and Git history. It edited
nothing, ran no tests and made no compliance determination. Its bounded
source-derived observations are:

- Block disposition and log inspection permit current, blocked and stale
  components while retaining project, tracked-artifact, discovery and
  structural-state checks. Execution and result acceptance require current
  state; ordinary transitions permit current/blocked, not stale.
- Intent acceptance retains its unresolved-attempt guard. Its message derives
  remaining blocked state from the queue and names the pending-transition
  command instead of claiming all task state became current.
- Rejection requires a reason, bounded latest exact task/attempt, supported
  record schema and matching task identity, with prepared evidence. Explicit
  target recovery fences, other tasks' unresolved attempts, conflicting
  dispositions and recovery-in-progress evidence refuse the write. Another
  merely blocked task does not. Rejection bypasses result-digest checks,
  refuses commit and does not certify execution, rollback or completion.
- The same exclusive component operation lock guards rejection. Queue and
  journal snapshots are checked before mutation. The blocked queue decision
  is stored first; ownership and snapshots are checked again before appending
  synchronized human-finalized evidence. Prior evidence is not replaced.
- A repeated identical stored block decision can finish a missing event
  without rewriting the queue; an identical existing event is not duplicated.
  Recording errors report partial persistence and inspection/repetition
  guidance. Rejection leaves intent revisions and acceptance identity alone.
- Canonical serialization now retains non-null disposition and acceptance
  identity fields rather than silently dropping parsed values.
- Guidance names inspection/block before acceptance, then deliberate pending
  transition. Task log remains read-only. Authenticated no-launch recovery is
  not broadened into a user assertion of no execution.

The observer read assertions for stale authenticated-attempt rejection,
unchanged intent revisions and journal prefixes, repeated/interrupted
recording, result drift, unrelated blocked tasks, refusal guards, queue
round-trips and active-lock refusal. Those were source assertions, not its
execution results. Interruption is modeled at the queue-only boundary;
append/sync failures and snapshot races were not fault-injected. Torn JSON
remains malformed evidence and is refused, not repaired by repetition.
These limitations remain explicit; the change is not a journal-repair or
whole-component compliance claim.

Implementer-attributed execution: `cargo test --offline --locked -p kvist`
passes 761 tests across 37 suites with seven existing ignores, including
65 dogfood-boundary tests, 54 task-command tests and nine queue tests.
Named output is retained in the session's `stale-abandon-tests.log`.
Strict all-target Maerg Clippy and workspace formatting pass. The stale
gate and dropped-disposition regressions failed before their respective
production fixes. No independent execution or intent acceptance is claimed.

Independent source-blind `stale-abandon-comparator` used only the isolated
scoped intent/task definitions, source-derived observations and attributed
execution results, with no tools or source access. It established no concrete
repair-scoped contradiction. Actual append/sync fault injection, torn-record
recovery and snapshot-race execution remain unqualified; malformed journal
protection is intentional. This is not whole-component certification.

After that comparison, the implementer used the rebuilt CLI to apply the
human's already-requested block disposition to the real
`shared-runtime-compliance-review`, `attempt-0001`. The original journal bytes
were verified unchanged as a prefix and exactly one blocked human disposition
was appended. Intent metadata was verified unchanged. The task is blocked,
not completed or deferred; intent remains stale pending human review.
The installed executable was not replaced. This operational action does not
become independent execution or proof of absent effects.

## Scoped resilient task execution repair - 2026-10-28

Independent clean-slate `resilient-source-observer` inspected the current
retry, agent, task-command, shell and corresponding test source only.
`disposition-followup-observer` independently observed the narrow subsequent
cause-preservation, disposition-fence and evidence-identity changes.
Both excluded intent, queues, prior records, reviews, peer implementations,
chat and Git history. Neither edited files, executed tests or certified
compliance. Their source-derived observations are:

- Capacity discovery and generation have separately counted retry scopes,
  sharing the approved task wall budget. Native policy supplies attempt and
  backoff limits; explicit endpoints use defaults. Upper-half jitter is
  capped by remaining time; cancellation interrupts backoff.
- Required records precede requests/retries. Recording and entropy failures
  stop dispatch and preserve the preceding model cause text alongside the
  secondary error. This preservation is textual, not a typed source chain.
- Stream events are provisional. Only a completed validated turn reaches the
  broker; retries explicitly discard partial output without executing partial
  proposals. Token reservations remain cumulative. No retry loop encloses
  effects, verification or the whole task.
- Failed execution evidence precedes blocked disposition. Execution evidence
  alone no longer releases an in-progress/pending attempt fence. Observed
  execution plus a terminal queue status, an associated committed disposition,
  or pending/explicit human disposition resolves the guard. Only the
  lock-owning run's terminal transition exempts its exact task/attempt pair.
  Other attempts and recovery states remain checked. Durable disposition
  remains recognized after deliberate blocked-to-pending transitions.
- Failed persistence reports unconfirmed completion and both causes.
  Required evidence/control closure failures are returned even when execution
  already failed. Wrapped model errors retain timeout/output-limit flags.
- Successful implementation and verification leave pending human disposition;
  batches reread status and stop unless the preceding task actually completed.
  Guarded next-task selection refuses unresolved attempts.
- Suggested-task confirmation precedes working output and spinner creation;
  streaming dispatch suppresses a competing spinner. Full multiline and long
  diagnostics reach the existing wrapping renderer, which was not re-observed.
- Exclusive log creation tries the timestamp identity and 1023 ordered
  suffixes, never overwriting prior logs. Live journal and trajectory names
  derive from that identity. Log and journal reservation is not transactional:
  journal creation can still fail explicitly after an empty log is reserved.

The first observer identified cause loss during failed retry recording and
premature fence release. Implementer-attributed regressions reproduced both;
the narrow follow-up observed the repairs and identified no concrete defect
in those changed routines. A rapid explicit retry also reproduced same-second
log-name collision, which was repaired and independently observed.

Other bounded observations remain outside this repair: explicit human
finalization does not itself require successful execution/verification evidence;
its digest reads can be skipped on failure; produced change inventory covers
only immediate readable test files and is not a complete change set.
These are retained limitations, not automatically arbitrated intent changes.

Implementer-attributed execution: `cargo test --offline --locked -p kvist -p sav`
passes 988 tests across 58 suites with seven existing ignored tests. This
includes real PTY confirmation, retry exhaustion/recovery, interrupted-stream
discard, failure-persistence and deliberate reattempt regressions. Execution
output is retained in the session's `affected-tests-final.log`; it is not an
independently executed qualification or whole-component acceptance.

Independent source-blind `isolated-repair-comparator` compared the bounded
intent/task-definition bundle with these source-derived observations and
attributed named execution results, without tools, source or older records.
It established no concrete repair-scoped mismatch. Execution remains
implementer-attributed; exhaustive retry-status/boundary coverage, universal
EOF precedence and re-observation of the wrapping renderer remain gaps.
The earlier comparator read outside its allowlist and is not counted as
independence evidence. Neither comparison accepts revisions, resolves the
historical attempt or certifies the broader finalization/change-inventory
limitations. Those remain available for explicit human arbitration.

## Scoped Rust inventory parity repair - 2026-10-27

Independent clean-slate `rust-inventory-observer` read only the authoring Rust
environment, native toolchain tests and shared runtime script source. It read
no intent, queue, prior record, review or Git history, ran no tests and made no
compliance determination. Its source-derived observations are:

- Preparation requires agreement between rustup's installed names and the
  host installation directory entries. Private registrations preserve those
  names exactly; no synthetic `default` registration is created.
- The selected installation is mounted read-only at `/rust/toolchain`;
  others use `/rust/toolchains/{sorted-index}`. Generated settings choose the
  selected name. Initialization copies settings into scratch and links the
  read-only registrations. Its separate `/rust/toolchains/default` alias is
  outside the registration directory.
- Wrappers select through rustup or an explicit leading `+name`; Cargo and
  cargo-clippy inject offline/locked resolution against `/rust/vendor`.
- The native trial compares host and sandbox inventory/default, compiler,
  rustdoc, targets and components; changes private defaults; compiles each
  installed target; and runs library tests and doctests. It asserts no
  synthetic registration, read-only installations, failed uninstall and
  unavailable selection, and compares recursive host state snapshots.
- These are finite source assertions, not execution evidence. The trial
  assumes multiple toolchains including `stable`, does not exercise pins or
  registry dependencies, and retains its pre-existing in-worktree-runner skip.
  Preparation uses `$HOME/.rustup` while the test helper honors `RUSTUP_HOME`;
  custom-home parity is not established. Executable digests and metadata
  rechecks are not whole-resource content authentication.

Implementer-attributed execution reproduced the inventory mismatch before the
repair and the explicit synthetic-registration assertion before removal.
After removal, all three ordinary `sandbox_toolchain` tests pass, including
the native parity/build/non-mutation and read-only workspace-provider trials;
four existing opt-in tests remain ignored. Strict all-target/all-feature
Clippy for maerg and Blad, workspace formatting and whitespace checks pass.
The three reported Blad failures did not reproduce in default/all-feature or
unoptimized library runs, 100 repeated library runs, or CPU-count nextest.
No Blad rendering behavior or production resource limit was changed.

The standalone all-feature CPU-count nextest run executed 1834 tests:
1833 passed, with 15 existing skips. The remaining failure was Skott's
`native_repository_tests_execute_inside_the_workspace_sandbox`, which exceeded
the sandbox preflight budget; whole-workspace success is not claimed.

Separate source-blind `rust-inventory-comparator` compared this supplement and
attributed execution with local intent and the root contract, without reading
source or test source. It identified no concrete repair-scoped mismatch.
Pins, registry dependencies, custom homes and the pre-existing skip versus
mandatory native coverage remain evidence gaps. The contract's all-indexed
mount wording versus the selected singular destination is pre-existing and
explicitly accommodated by design, not introduced by registration removal.
This is a scoped comparison, not acceptance or whole-component certification.

## Scoped model-budget regression repair - 2026-10-25

Independent clean-slate `budget-source-observer` inspected only the budget,
transport-construction and corresponding regression/fixture source. It excluded
intent, queues, prior records, reviews and history, ran no tests and made no
compliance judgment. The observations below are transcribed from that report:

- `TurnBudget` subtracts elapsed wall time saturating at zero.
  `model_deadline` rejects cancellation before expiry, returning the respective
  model-transport cancellation or timeout error. Initial native/explicit
  transport construction and each retry use that check; shared-native capacity
  discovery uses it when capacity is unspecified.
- Model identity resolution and token preflight still precede the host-turn
  deadline check. Endpoint construction still precedes the liveness probe;
  no configured agent command is executed by this path. These checks do not
  authorize effects. Retry limits, emitted-output restrictions and the shared
  wall-clock budget remain visible in the source.
- The new host-turn test supplies zero time for native and explicit paths,
  asserting timeout without cancellation and cancellation when both hold.
  It does not instrument construction/network calls or exercise
  positive-budget cancellation.
- Live steering/accounting assertions retain exact known-usage totals,
  cumulative token-limit failure and trajectory totals. The cancellation
  fixture retains assertions for an explicit cancellation error, the previous
  answer, known 11/7 usage and unsuccessful replay with total 18.
- Those two fixtures allow 60 seconds for model work rather than asserting
  durable-recording latency. Their provider accept helper polls nonblockingly
  with a 60-second guard and skips empty liveness connections. Accepted streams
  retain three-second per-read timeouts; this is not a hard end-to-end read
  bound. Steering and provider-close interleavings remain finite tested cases,
  not exhaustive proofs. Cancellation/expiry checks are not atomic with
  subsequent work.

Implementer-attributed execution: the zero-budget regression first failed with
an invalid-deadline configuration error, then passed after the guard repair.
All eight live-runtime integration tests pass, including ten repeated
four-worker nextest iterations (80 executions). Final all-feature workspace
verification with four workers passes all 1817 tests, with 15 existing skips;
strict workspace lint and formatting pass. CPU-count-concurrency runs still
encounter intermittent native-render and repository-preparation timeouts under
observed host I/O/memory pressure, including after diagram-test isolation.
Their production limits were not relaxed, and that full-concurrency limitation
remains unresolved. No component acceptance or whole-runtime certification
is implied.

The separate source-blind `budget-intent-comparator` compared this supplement
and attributed execution with local intent and the root contract, without
reading implementation/test source or running tests. It found no concrete
scoped discrepancy in cancellation-first deadline checks, retained usage or
cumulative accounting. It retained that zero-budget cases do not prove
positive-budget cancellation, absence of network/construction calls or every
race; checks are non-atomic and per-read fixture timeouts are not end-to-end
bounds. Fixture scheduling slack changes no production limit or authority.
This comparison is neither blanket compliance nor acceptance.

## Observed public contract

### Command and workflow surface

The package exposes the `kvist` executable and library modules for command
dispatch, project/component inspection, durable task queues, supervised
execution, provisioning, and domain errors. The library forbids unsafe code
and rejects non-Linux compilation.

The CLI includes project initialization, conversion, import, reverse
discovery, tree/status/doctor, queue repair, component document
creation/validation/acceptance/commit, task selection/transition/run/log/replay,
policy approval, unlock/recovery/finalization, agent configuration and
qualification, dependency/toolchain provisioning, custom prompts, an
interactive workspace shell, help, and completions. A hidden `authoring-apply`
command applies staged effects. This is a summary, not an exhaustive
option-by-option interface specification.

Task execution is distinct from arbitrary custom prompts. Tasks use validated
project/component state, VCS tracking checks, explicit execution approval,
lifecycle locking, task-kind role selection, and the protected broker/effect
path. Implementation success additionally invokes verification and records
pending human disposition. Other successful task kinds can transition directly
to completed. Surfaced decisions pause in awaiting-decision; ordinary recorded
agent/verification failures can lead to blocked state. Errors can instead leave
an unfinished prepared attempt. Exact task selection, item-prefix batches, and
`all` are supported; omission of a task requires interactive confirmation.

Custom prompts delegate to a standalone `skott` subprocess, forwarding prompt
and model/effort selection. Child runtime and sandbox behavior are delegated,
not established by this inspection. Headless prompt dispatch rejects the host
execution opt-out. The interactive workspace shell is a separate front end to
these workflows.

Evidence: `src/lib.rs`, `src/cli.rs`, and
`src/task_commands.rs::{run_task_with_controls,run_task_or_item_with_controls,
verify_task,finalize}`.

### Shared selection and pinned authority

`AgentConfig` retains typed, enabled shared registry profiles as loaded
snapshots. Named and role-based resolution select the configured profile;
resolution does not itself rediscover a backend. Shared bindings disallow
mixing copied local provider/profile definitions and restrict role tables to
profile references plus role policy. Registry parsing and lookup are delegated
to `sav::registry`.

The agent configuration source digest incorporates resolved profile names,
source paths, and digests. Execution approval also incorporates serialized role
configuration, limits/redaction policy, root-context digest, sandbox settings,
runner/backend identity, and test policy. Task execution passes its approved
runner, capability probe, and policy digest into effect execution.

Protected shared task selection requires the resolved native backend. Disabled
or incompatible selections are refused; an external-agent profile is not
silently executed instead. Explicit local command configurations remain a
separate supported path: Kvist extracts model endpoint/provider/model data and
uses structured transport rather than executing the configured agent command.

Evidence: `src/config.rs::{resolve_shared_registry,AgentConfig}`,
`src/task_commands.rs::build_execution_approval_for_runner`, and
`src/agent.rs::{select_model_choice,execute_host_turn}`.

### Observation and explicit controls

Agent execution creates a required live journal at
`.kvist/runs/<session>.events.jsonl` and announces its path on stderr before
generation. Events include shared-profile identity, model budgets and request
counts, redacted model output/failures, broker counts, dispatch/results,
guidance/control notices, compaction, and terminal disposition. These live
records are labeled noncanonical evidence, separate from workflow acceptance.

`task run --allow-control` is explicit opt-in and is passed through text/JSON
dispatch and batch execution. Default execution opens no control server.
With opt-in, binding a private endpoint and enforcing its protocol are
delegated to `sav::observation::bind_runtime_controls`; the endpoint is
announced on stderr. A library entry point accepts caller-owned mailbox and
cancellation objects without opening IPC.

Guidance is supplemental: the original task and selected artifacts are pinned
in the session's system context, the original task is also supplied as the
initial user message, and guidance appends another user message explicitly
preserving task and approved scope. Mailbox notices distinguish accepted,
claimed, delivered, withdrawn, and dropped guidance. Superseded calls receive
not-executed tool results rather than being silently treated as applied.

Evidence: `src/agent.rs::{execute_agent_with_controls,execute_agent_controlled,
execute_agent_inner}`, `src/agent/live.rs`, `src/agent/controls.rs`,
`src/cli.rs::TaskCommand::Run`, and `tests/control_cli.rs`.

## Observed requirements and constraints

The following are source-enforced checks and observed limits, not inferred
normative requirements.

- Agent execution has a 64-generation ceiling and a shared elapsed-time budget.
  Transient generation retries are capped at three attempts, additionally
  bounded by shared native retry settings. Steering interruptions consume
  generation/attempt budgets; they do not create unlimited retries. A normal
  completed turn resets its generation-attempt counter.
- The endpoint transport is constructed for validation before liveness contact.
  The probe independently requires a numeric loopback address, uses short
  connects, checks cancellation, and performs cancellation-aware backoff.
  Malformed/oversized responses, response-level failures, and cancellation are
  not ordinary availability retries. Text-emitting streamed attempts are not
  retried.
- Shared native context capacity is explicit or obtained through a delegated
  capacity lookup. If unavailable, the run refuses inference rather than
  guessing capacity or choosing an opaque backend. Response reserve is
  configured or derived from capacity. Explicit local endpoint selections use
  the runtime library's default context capacity and a default response reserve.
- Selected context is restricted to the five local component artifacts and
  exact root/parent contract file mounts. At most seven paths are accepted,
  each path is bounded to 4,096 bytes, each file to 1 MiB, and aggregate file
  content to 4 MiB. Files must be regular UTF-8 files; descriptor-relative
  no-follow opening covers each path segment. Size and modification/change
  timestamps are compared around bounded reads. There is no recursive
  source/peer discovery.
- Context files and the original task are quoted into system context before
  request preparation. Delegated context management can compact completed
  conversation groups but a request whose retained context cannot fit fails;
  selected artifacts are not silently omitted to obtain a request.
- A configured role token limit is cumulative. Each inference attempt reserves
  estimated input plus output allowance before dispatch, reducing output
  allowance to fit remaining budget. Known positive usage replaces that
  reservation; failed, missing-usage, and zero-usage attempts retain conservative
  charges. Retries and steering share this accounting.
- Reported input/output totals sum known usage across retained turns, not just
  the last turn. The log distinguishes reported/unavailable turns and
  conservatively charged or unknown attempts. Known totals are not a claim of
  exact usage for unreported work.
- The closed authoring vocabulary is `write_file`, `edit_file`,
  `propose_decision`, and `request_dependency`. Default file authority is
  `src/` and `tests/`, with protected document names and absolute/traversal
  destinations rejected. Default depth/content bounds are 12 levels, 1 MiB
  per effect, and 8 MiB per plan. Edits require a single matching substring.
  These tools do not grant arbitrary shell execution or general file reading.

Evidence: `src/agent.rs`, `src/agent/context.rs`,
`src/agent/accounting.rs`, and `src/authoring/mod.rs`.

## Observed internal design and failure paths

### Conversation and effect ownership

`execute_agent_inner` selects the backend, creates log/live state, establishes
mailbox ownership and optional IPC, resolves capacity, materializes selected
context, and starts a `sav::runtime::AgentSession`. Each iteration delivers
guidance, obtains a request, prepares context, performs token preflight, and
calls observed structured transport under the remaining time budget.
Conversation, context preparation, operation-token linkage, transport protocol,
and shared control internals are library delegation.

A completed model turn is retained before token-limit checks, turn validation,
steering processing, broker decisions, or effects can return an error.
Superseded completed turns therefore retain available usage. Interrupted
attempts without a completed response cannot supply an invented turn or usage
report.

The broker reduces untrusted proposals to file effects, decisions, accepted
dependency requests, or dropped intents. A decision records bounded proposal
evidence and pauses before that turn's effects. Dropped intents prevent file
effect application. Accepted dependency requests are recorded, but protected
fulfillment is explicitly unsupported as described below.

File effects are staged under component state with content identity and then
applied by the current Kvist executable inside the effect sandbox. Requests
carry the task's pinned runner/probe/policy, read-only context and staged input,
network-denied authoring phase, remaining timeout, output bound, and owner
cancellation token. The applier revalidates destination/content and edit
preconditions before replacement. Effects are sequential, not transactional:
earlier writes are not rolled back when later work fails.

### Steering, cancellation, and retained outcomes

Dispatch readiness is checked before staging and before launching another
effect. A conservative uncertain-effect record is inserted into the
owner-retained turn before dispatch. Confirmed results replace it; dispatch
errors leave it explicitly uncertain. Cancelled, timed-out, output-limited, or
otherwise unsuccessful dispatched effects are treated as potentially mutating,
not as proof that nothing changed. Staged-file cleanup is scope-owned.

The turn/effect collection lives outside the execution closure. The finalizer
attempts one aggregate log/trajectory record after ordinary success, early
error, or cancellation from that closure. The text log is synchronized;
trajectory recording errors propagate rather than being silently ignored.
Original execution errors remain errors even if recording also fails; a
recording failure is then logged. Mailbox closure and live finish are both
attempted without replacing an existing execution error.

Trajectory entries redact session/task/call/tool/destination diagnostics and
retain dispatch/result correlation. Its legacy mutation boolean cannot express
uncertainty, so possible mutation is represented conservatively as true with
an explanatory diagnostic. Decision/dependency evidence filenames are SHA-256
hashes of call IDs; untrusted IDs are payload data, not path selectors.
Replacement uses the shared synchronized atomic file helper.

These changes do not make storage infallible: initialization failures before
the execution closure, recording failures, crashes, and panics do not establish
complete evidence. A bounded text log can also truncate later detail.

Evidence: `src/agent.rs::{execute_agent_inner,apply_authoring_effects_with,
summarize_effect,record_turn_outcome,evidence_filename,write_atomic}`,
`src/agent/controls.rs`, and `src/agent/regression_tests.rs`.

### Redaction and live records

Streaming redaction scans at UTF-8 character boundaries and chooses the longest
matching configured literal. Incomplete candidate prefixes are held across
deltas and final flush is explicit. This avoids arbitrary byte slicing and
preserves matches across the tested Unicode/overlap partitions.

`LiveRun` recursively filters string values in metadata and events using its
stored profile redaction values, preserving `type` discriminators. The run
identity in the journal envelope is derived from a hash. Text/result filtering
also uses the agent redaction set, which includes allowed environment values.
Diagnostic recording remains required whether or not an observer is attached.

The sets are not identical: `LiveRun::create` receives
`profile.redaction_values`, while `redaction_values(profile, sandbox_config)`
later adds environment values for text/outcome filtering. Consequently,
environment-only literals in model/call metadata are not assured the same
recursive filtering as configured profile literals. Literal replacement is
not general secret detection, and announced filesystem paths/tracing output
are not established as universally redacted.

Evidence: `src/agent/redaction.rs`, `src/agent/live.rs`, and
`src/agent.rs::{execute_agent_inner,redaction_values,redact}`.

### Protected provisioning and verification

The agent invokes `vendor_project_protected` with the task-approved runner,
VCS selection, policy identity, owner cancellation token, and remaining budget.
That entry point refuses all model dependency fulfillment: it lacks an exact
requested-dependency binding to the existing lock/manifest. It checks
pre-cancellation/zero budget and returns a clear unsupported error without
inspecting paths, staging work, or launching a host package manager. Existing
ready vendoring is not misrepresented as fulfillment of a fresh request.
The agent propagates this error after retaining request/turn evidence; it does
not log acquisition failure and continue as though dependencies were available.

`check_protected_vendor_readiness` is a separate read-only operation. It checks
controls, policy-identity syntax, comparison against the provided runner
identity, Rust strategy, verified supported manifest, locked readiness, and
absence of stale registry material. It refuses other languages or Rust state
needing repacking. It does not fetch, fulfill a model request, or fall back to
the standalone provisioning command.

Standalone `vendor_project` remains distinct human-invoked provisioning:
its Rust path can fall back from sandbox fetch to host `cargo vendor`, and
other language paths can run host package managers. Host repacking/capture
there does not become protected merely because protected request options exist.

Task verification remains outside the agent control-server scope. It checks
approval and component working-directory policy, then routes through generic
network-denied or vendored offline sandbox topologies. Implementation success
records pending human disposition. Runner/OS enforcement and peer runtime
internals are not independently established here.

Evidence: `src/vendor_command.rs::{ProtectedVendorOptions,
vendor_project_protected,check_protected_vendor_readiness,vendor_project}`,
`src/vendor_command/protected_tests.rs`, `src/agent.rs::execute_agent_inner`,
and `src/task_commands.rs::verify_task`.

## Remaining observed scope and workflow limits

- The optional private server exists only during agent execution; it does not
  control later verification. Readiness checks and sandbox launch are separate
  caller operations. This source inspection does not establish the delegated
  mailbox's concurrency linearization or prove absence of a dispatch/guidance
  race.
- On an agent error, lower-level log/trajectory finalization is attempted, but
  `run_task_with_controls` still propagates `Err` before appending its
  task-level `agent-execution` record. Lower-level evidence preservation is
  not the same as a completed task journal/status transition.
- Successful implementation finalization still builds its change list from
  immediate files in `component/tests`, labeling them create with null
  pre-digests. Source files, nested tests, and deletions are not captured by
  that enumeration; listed-change validation does not supply a complete
  post-execution source-tree snapshot. This is a broader workflow limit,
  separate from runtime turn/effect retention.
- The dependency broker's private-origin predicate still checks prefixes of
  the full origin rather than parsing the host. A private numeric host inside
  an HTTPS URL can be classified as accepted. Protected fulfillment nevertheless
  refuses before any fetch; classification alone does not demonstrate network
  contact.
- Verification's literal set includes architect/developer values and sandbox
  environment values, not security-reviewer-only literals. The additional
  test-policy environment allowlist only restricts the sandbox allowlist:
  `allowed_environment` intersects the lists. There is no source support for
  an assertion that test-policy-only variables are additionally exposed.
- Shared registry qualification, control endpoint permissions/authentication,
  context estimation/compaction, journal protocol/durability, transport
  cancellation, and sandbox OS enforcement depend on delegated libraries or
  processes. This record does not infer their internals from call sites.

Evidence: `src/task_commands.rs:4196-4228`, `:4285-4306`,
`src/task_commands.rs::{finalize,evidence_redactions}`,
`src/authoring/mod.rs::is_unverified_origin`,
`src/sandbox.rs::allowed_environment`, and the agent sources above.

## Evidence and verification limits

The current observation read only permitted `maerg` source/test source.
It did not read standalone intent documents, queues, previous implementation
records, repository metadata/history/diffs, peer implementation, or prior
reviews as evidence. No production/test files were modified. The implementation
record was replaced without reading its prior contents.

**Current execution evidence:** the caller reports that selected maerg
library, agent, live-runtime, control-CLI, and task-command tests passed, and
that all 60 dogfood boundary tests passed. No corresponding fresh run log was
independently inspected for this refresh. No tests, builds, linters, or
reproduction commands were run by this observer. Fresh broader workspace gates
were still pending when this observation was requested; no post-repair
workspace-wide test, Clippy, or rustdoc pass is claimed.

**Inspected test-source coverage, distinct from execution evidence:**

- `src/agent.rs` includes Unicode/overlap redaction cases checked across UTF-8
  partitions, endpoint/probe and retry bounds, streaming retry behavior,
  selection, and effect summaries.
- `src/agent/accounting.rs` covers cumulative known usage, output capping,
  conservative missing/zero usage charges, and reservations retained across
  retries/interruption.
- `src/agent/regression_tests.rs` covers selected-context restrictions,
  no-follow paths, UTF-8/file/aggregate/context-window bounds, retained partial
  and uncertain dispatch outcomes, cumulative known usage with missing reports,
  pre-cancelled probing, redacted trajectory correlation, and hashed evidence
  filenames for hostile call IDs. Injected dispatch tests establish local
  bookkeeping behavior, not real sandbox writes.
- `src/agent/live.rs` covers recursive diagnostic-value redaction with valid
  journal envelopes.
- `tests/live_runtime.rs` checks early journal visibility, profile-literal
  filtering in returned model metadata, quiet-generation steering, original
  task preservation, mailbox closure, opaque-backend refusal, actual selected
  context in native and explicit endpoint requests, cumulative steering usage
  and token limits, forbidden endpoint refusal before probing, retention after
  cancellation, and refusal of context that cannot fit.
- `tests/control_cli.rs` covers default-off/explicit opt-in parsing in both
  output modes, not an independent control-socket round trip.
- `src/vendor_command/protected_tests.rs` covers unconditional unbound-request
  refusal, pre-cancellation/exhausted budgets, read-only ready-Rust inspection,
  pinned runner mismatch, malformed policy, refusal of unsupported/stale/missing
  material, and host-tool sentinels.
- The inspected dogfood fixture in `tests/dogfood_boundary/support.rs` uses a
  shared native mock profile with explicit `context_limit = 65536` and
  `response_reserve = 1024`. Its context capacity is fixture configuration,
  not evidence that production silently omits context or assumes that capacity.

This bounded observation does not establish exhaustive race coverage,
crash/power-loss durability, universal secret filtering, exact billing for
unreported provider work, or absence of other defects.

## Source-only test/helper observation (2026-10-27)

Observed only `maerg/tests/live_runtime.rs`, `tools/profile_tests.py`, and
`tools/test_profile_tests.py`; no files changed or tests executed by this
observer.

- **Effect gate:** A synthetic Bash runner consumes its request, connects to an
  ephemeral loopback TCP port, announces readiness, and waits for `continue`.
  Tests inject guidance before release or release before cancelling the
  subsequent generation. This synchronizes effect execution; it neither
  performs the requested file write nor demonstrates sandbox isolation.
- **Usage evidence:** Assertions retain completed-turn usage across steering:
  24 input/12 output tokens, with trajectory total 36. The token-limited case
  fails while retaining known usage 6500/7 and total 6507; these tests alone do
  not establish the budget calculation. Steering before any usage arrives
  records one conservatively charged/unknown attempt separately from the later
  known 10/4 tokens. Cancellation preserves earlier answer text, known 11/7
  usage, and an unsuccessful trajectory ending with total 18.
- **Profiler failures:** Completed failed-test events are retained and printed
  individually before up to five slowest nonfailed events. A fixture checks
  two fast failures remain visible despite six slower successes, without
  printing their embedded failure details. Command exit status is preserved;
  malformed event data becomes report diagnostics.
- **Scope and limitations:** Gate waits have explicit deadlines, but thread
  joins and some older listener accepts lack independent deadlines; request
  accumulation has no size cap. Helpers require Unix paths and permissions,
  Bash `/dev/tcp`, and external executables. Most sockets are loopback-only;
  the forbidden-endpoint fixture briefly binds all interfaces. The profiler
  launches explicit argument vectors in a new session, cleans up that process
  group, and creates private reports outside the checkout; escaping
  descendants are not contained. It scans host-visible process metadata,
  retains raw command output, and imposes no overall runtime or report-size
  limit. Linux sampling can miss brief/reparented processes and peaks. Coverage
  is synthetic and source-observed, not proof of portability, isolation,
  compliance, or acceptance.

## Long-session source-observation supplement (2026-10-28)

**Evidence boundary:** Source/test inspection only. No commands, tests, or builds were executed. Regression assertions are source evidence rather than executed proof; these observations do not certify compliance.

Each protected execution creates a fresh conversation, context manager, task budget, control ownership, token budget, live journal, and retained-turn collection. There is no fixed generation-count ceiling. Role defaults impose a 300-second task budget, no cumulative-token limit, and 65,536-byte output capture. Configuration accepts task timeouts through thirty days and output limits through 1 MiB.

`TurnBudget::model_deadline` now returns the lesser of remaining task time and **24 hours**, rejecting cancellation or exhausted time first. This limits individual transport requests without shortening the stored task budget. Shared native transports additionally apply their configured attempt deadline. Gateway probes, retries/backoffs, and effect timeouts consume the same remaining task budget. Enforcement is phase-based rather than an independent whole-task deadline watcher.

`RunControls::deliver` now returns whether guidance actually reached the conversation. Withdrawal-only, dropped, or empty batches do not renew attempts. The execution loop resets `model_attempts` when delivery returns true; it also resets after an ordinary completed generation. Guidance therefore renews generation opportunities without resetting elapsed task time, retained evidence, or cumulative token accounting.

Known provider usage accumulates across retained completed turns. Failed/interrupted attempts and unavailable or zero usage retain conservative reservations. Remaining token capacity can reduce the next output allowance; nonzero provider usage replaces its reservation. Transient model attempts are capped at three, further restricted by shared-profile settings. Failed attempts that already streamed text to stdout are not automatically retried.

Generation uses operation-scoped cancellation, while executing effects receive task cancellation. Guidance preserves the system-pinned original task, skips undispatched calls, and retains completed effects. Complete turns are retained before validation/effect handling. Durable dispatch precedes execution; uncertain effects are explicitly retained rather than replayed. Finalization attempts to preserve earlier evidence on cancellation/error.

Selected context is restricted to recognized local artifacts and exact root/parent mounts: at most seven paths, 4,096 encoded bytes per path, 1 MiB per file, and 4 MiB aggregate. Reads reject links, nonregular files, invalid UTF-8, and detected changes. Shared native capacity uses configuration/discovery; explicit endpoint mode uses an 8,192-token window and 1,024-token reserve. Compaction prefers four recent groups, but `run_turns` grows independently of compacted history.

Live diagnostics inherit Sav’s journal rotation and follower behavior. Final logs are bounded/redacted; trajectories retain known usage and possible mutation.

`multi_day_task_deadlines_fit_direct_transport` asserts that a seven-day budget produces a transport-compatible request deadline while retaining over six days. `multi_day_explicit_endpoint_run_accepts_repeated_human_guidance` asserts five interruptions followed by a sixth successful request. Other inspected tests assert 66-generation continuation, cumulative accounting, context refusal, and cancellation evidence. Dependency-acquisition internals were excluded from this refresh.

### Source-derived approval and effect-timeout addendum

**Maerg addendum — source-derived:** Approval remains bound to configuration-source identity/digest, serialized profile digests, and explicit timeout fields for all three roles. Approval loading authenticates the recorded material; execution compares it with current material. The timeout-change regression changes the developer timeout from five to six seconds and asserts rejection with the source-identity/digest diagnostic, unchanged queue contents, and no sandbox-request file. Effect dispatch passes exactly `Some(remaining_task_budget.min(Duration::from_secs(86_400)))` to the sandbox executor: at most 24 hours, further clipped to remaining task time, with no positive minimum. This observes the supplied timeout, not independently verified executor enforcement. The seven-day-configured fixture scripts 65 effect-producing generations followed by a final response and asserts 66 generations and success; it does not assert seven elapsed days or directly test timeout clipping.

No commands, tests, or builds were executed; these are source observations and test-source assertions, not compliance certification.

## Workflow fixture source observation (2026-10-29)

**Evidence boundary:** A fresh `dogfood-fixture-observer` inspected only
`tests/dogfood_boundary.rs`, its `support.rs` and `fixtures.rs` modules, and
native template source. It excluded intent, queues, prior records/reviews,
Git/chat history and execution logs, performed no edits or execution, and
did not inspect real-runner protocol tests. The following is source-derived,
not executed proof or compliance certification.

- Fixture project authority and root/child component documents come from the
  static `artifacts::root_artifacts()` registry. Requirements append a small
  supervised-execution anchor; queues are constructed separately. Revision
  fields hash the actual written documents. Child queues bind
  `../CONTRACT.md` to the root component contract's digest.
- Generated model configuration retains the native llama-server profile,
  a 65,536-token context limit and a 1,024-token response reserve. A default
  loopback mock supplies a write-file call then final text. The existing
  optional model-URL selection uses substring matching rather than parsed
  host validation; that behavior is not changed or certified here.
- The controlled workflow runner consumes/records its request and writes
  synthetic effects directly. Advertised capabilities and configuration
  alone do not demonstrate sandbox enforcement; real-runner tests are
  separate.
- The bounded-context regression sums five local artifacts plus the root
  contract and, for children, parent contract, asserting at most 16 KiB for
  each of three components. It checks native template content, document and
  parent bindings, and unchanged profile limits. Bytes are not token usage.
- The unavailable-manifest regression reexecutes its test binary with an
  empty manifest location and child marker, removing the optional model URL
  only in that subprocess. It asserts fixture creation, validation and three
  current components. Other environment and working-directory state remains
  inherited. This setup is not filesystem isolation or proof that all
  repository authority is globally unavailable.

**Implementing-context execution, not observed by the independent reader:**
the original target reproduced 69 passes and seven context-preflight failures.
Before the helper change, both new regressions failed: selected fixture bytes
were 250,439 against a 16 KiB bound, and creation could not locate authority
under the empty manifest root. After the helper change, all 78 target tests
passed; strict all-target Clippy, formatting, whitespace and structural
validation passed. These executions preceded this record. The affected target
must run again after the final intent, queue and evidence changes; neither
this observation nor the separate comparison substitutes for that rerun.

### Independent source-blind fixture comparison

A separate `dogfood-fixture-comparator` received only the scoped new
requirement, contract/design paragraphs, four task definitions, independent
source observation and attributed execution. It used no tools, source,
history or prior reports.

It found no demonstrated scoped implementation contradiction: observed
template construction, written-document/parent hashes, byte bound and
profile values align with intent. The subprocess setup matches the documented
unavailable-manifest strategy without claiming filesystem isolation.

Its limits are explicit: the observer ran no tests, bytes are not tokens,
and absence of authority at tested locations is not global unavailability.
The controlled runner does not prove isolation. Unchanged production and
original lifecycle/protocol assertions, and successful real-runner tests,
remain attributed implementing-context evidence, not independently verified
facts. The reported diff check found no changes in production source or
original workflow, transition, layout and protocol test files.

At comparison time the executions preceded final records. The comparator
therefore left scoped completion pending the complete affected-target rerun
after all intent, queue and evidence updates. This is not a formal receipt,
intent acceptance or whole-component compliance/security certification.

### Recorded fixture verification

The implementing context reran the complete target after the independent
records, using default test concurrency: **78 passed, zero failed or ignored**.
Both new fixture regressions and all seven original failures passed.
Strict all-target Clippy (`-D warnings`), formatting, whitespace and
`component validate maerg` also passed.

One earlier post-review run had 77 passes and a failure in the unchanged
real-runner wall-time test: its process startup marker was absent with a
configured 200 ms budget. That assertion alone does not establish why the
marker was absent. The isolated test and the subsequent complete default-
concurrency target passed. This is separate from the reproducible fixture
context overflow; no runner timeout, assertion or enforcement was weakened
to conceal it.

These are attributed execution results, not independent certification. A
final complete-target rerun follows this last evidence/queue update so the
record itself cannot invalidate the checked fixture inputs. No intent was
accepted, installed executable replaced or operational task replayed.

## Protected model startup supplement (2026-10-29)

### Independent source-only observation

The clean-slate `startup-observer` inspected only the scoped Maerg startup,
context, diagnostic, budget and execution-loop source and test assertions. It
did not read intent, queues, prior records/reviews, chat or Git history, live
logs, operational attempts or peer implementations. It made no edits and ran
no tests. These are source observations, not executed results or certification.

Execution creates logs, its required journal and controls before starting the
wall budget; context construction is inside that budget. Cancellation precedes
expiry in deadline checks. Configured native capacity skips discovery.
Otherwise discovery uses a transport deadline of the smaller of one second
and remaining task time divided by ten, with a 1 MiB response bound. Missing
capacity, transient-classified errors and HTTP 404/405/501 permit provisional
operation. Other discovery errors, transport-construction failures and required
recording failures propagate. Cancellation and expiry are checked again after
discovery.

An explicit native output reserve wins. Otherwise known capacity uses
`clamp(limit/4, 1, 8192)` and unknown capacity uses 8192. Let `I` estimate the
complete initial request after setting its output cap, and `R` be that reserve.
Unknown native capacity uses `clamp(I + R + 8192, 32768, 1048576)` with saturating
addition. Known limits remain unchanged. Explicit command-endpoint profiles
retain their default context constant and 1024 reserve; a test asserts 8192
context. Budget records distinguish configuration, provider, provisional and
default sources and report whether serving capacity is known.

Selected context is limited to five named local artifacts and exact root/parent
mounts, with at most seven selections and 4096 bytes per selected path.
Descriptor-relative reads do not follow links, reject traversal, require regular
UTF-8 files, and check size and modification/change timestamps around reading.
Bounds are 1 MiB per file and 4 MiB aggregate. Raw queue bytes count before
projection. The complete selected queue is parsed; an absent task fails.
Projection contains schema version, a projection marker and only the selected
task's id, title, description, context, purpose, expected outcome, kind,
dependencies and requirement references. Other selected artifacts stay exact.
The original task appears in both system and user context.

Required journal recording includes discovery, unavailability, budget provenance,
request estimates/counts, bounded results/errors, stream text and loop events.
Recording failures propagate, including failures during streamed relay.
Configured string redactions apply to diagnostic values except event type tags.
Request summaries do not contain complete prompts. Stderr distinguishes optional
discovery, provisional fallback and inference estimates.

Inspected tests assert optional missing/unsupported/transient capacity, quiet
probe bounds, authoritative configured/provider limits, malformed metadata and
authorization refusal, preserved large input, hard-ceiling overflow without
truncation, explicit provider rejection, cancellation/expiry/recording failure,
unchanged endpoint defaults, no-follow/bounded context, selected-task projection,
independent inference retry limits and retained blocked outcomes. The observer
had no execution evidence. A subsequent implementing-context test refinement
also checks recorded probe deadlines against both the one-second and 10% bounds;
this refinement was not independently re-observed.

Limits retained by the observer: earlier setup is outside the task timeout;
individual context reads do not check it internally. Raw byte bounds exclude
JSON/prompt expansion and do not guarantee model fit. Duplicate selections are
not rejected. Whole-queue validation means malformed unrelated queue entries
still prevent startup. Estimator internals, transient classification, journal
durability internals and later effect execution were outside its allowed scope.

### Attributed implementing-context evidence

The retained attempt-0002 journal records a discovered 262144-token capacity
and 8192 reserve, followed by inference before timeout. No operational evidence
or task disposition was modified by this repair. The original missing-capacity
regression failed before production changes because unavailable metadata was
fatal. With a valid fixture, the queue-projection regression failed because
unrelated task history remained in the prompt.

The complete Maerg package suite passed **784 tests, zero failures, seven
existing ignores** after implementation, with strict all-target Clippy,
formatting, whitespace and structural component validation passing. These
results precede this record and the final deadline-assertion/task-name refinement.
The final affected-target rerun must follow all intent, queue and evidence
updates. Synthetic loopback providers demonstrate local control flow, not
throughput or successful completion on the operator's large model. No real
inference replay, policy expansion, intent acceptance, installed executable
replacement or completion of the blocked compliance task is claimed.

### Independent comparison and ceiling correction

The separate source-blind `startup-comparator` received only isolated intent,
task definitions, the independent observation and attributed execution. It used
no source or tools. It supported optional discovery, warned fallback,
selected-task projection and provenance, but identified a real headroom
ambiguity: saturating the local budget at 1048576 could silently discard the
promised history allowance. It also retained cooperative-timing and pending
post-record verification limits. That finding was not concealed as compliance.

The implementing context added an explicit ceiling rejection and exact-boundary
assertions. A fresh, narrowly scoped `startup-ceiling-observer` read only startup
source and its tests, excluding all intent, queues, prior records/reviews,
history, logs and peers, and performed no edits or execution. It observed:

- estimated input, reserve and 8192 headroom are saturating-added;
- totals greater than 1048576 fail explicitly; otherwise the minimum is 32768;
- with reserve 8192, input 1032192 is accepted exactly at the ceiling and
  1032193 is rejected; arithmetic overflow cannot wrap to an accepted value;
- only a cloned request receives the estimation output cap, so rejection
  neither mutates nor truncates caller input;
- assertions cover minimum/exact/over-ceiling totals, integer saturation,
  unchanged oversized input, preserved large content, and recorded positive
  probe deadlines no greater than 500/1000 ms for 5/30-second task budgets.

A separate source-blind `startup-ceiling-comparator` compared the narrow intent
and fresh observation with attributed targeted execution, without tools/source.
It found the particular headroom contradiction addressed, with no remaining
narrow contradiction demonstrated. Recorded deadlines and inference within two
seconds do not establish hard real-time enforcement; the tests do not separately
assert the `optional` event field. Estimates are not real-model capacity or
throughput validation. It retained the final post-record rerun as pending at
comparison time and made no certification, receipt or acceptance claim.

After the ceiling correction, the implementing context passed four startup unit
tests, six startup integration tests and strict all-target Clippy. The complete
Maerg package suite then passed **785 tests, zero failures, seven existing
ignores**. Formatting also passed. Those executions precede this final record;
the affected targets and structural checks run again afterward. No accepted
intent revisions or operational task state are rewritten by these observations.

## 2026-11-03 capacity fixture observations

Independent `maerg-fixture-observer` inspected only `tests/model_startup.rs`
and the specified HTTP-helper, continuous-run, retry and explicit-task ranges
of `tests/task_commands.rs`. It excluded intent, queues, prior records/reviews,
history, logs, production implementations and peers. It executed no tests.

- `MockGateway::serve` examines raw HTTP bytes, returns 404 `{}` for `/models`
  and `/props?` GET prefixes, then forwards only inference bodies to callbacks.
  Raw/status callers retain explicit metadata routing. The callback moves into
  one sequential worker and requires `Fn + Send + 'static`, not `Sync`.
- Startup assertions cover router 404 and standalone unavailable statuses,
  provisional provenance, configured discovery bypass, and known router
  capacity. Both router and standalone paths exercise zero capacity, malformed
  JSON, HTTP 401 and HTTP 403, asserting errors and no `model_request` event.
- Metadata bypasses generation counters. Continuous-run assertions require five
  additional generations for fresh work and checkpoint recovery. Retry coverage
  asserts two metadata requests, five inference attempts and four retry events.
  Interrupted-stream assertions discard provisional output without recording
  a sandbox request. Explicit-task assertions leave follow-ups pending.
- Additional assertions preserve large input, bound recorded discovery budgets,
  and retain an unsuccessful provider context rejection with one model request.
  These are source assertions, not verified execution results.

Limits include broad metadata prefixes, Content-Length-oriented fixture readers
without explicit request-size bounds, possible callback invocation after an
incomplete body, ignored worker-join failure, and shutdown waits on active work.
The inspected numeric cases are not comprehensive, and absence of a journal
request is not an independent network-generation count. No broad security or
compliance certification follows.

The separate source-blind `maerg-fixture-comparator` found no demonstrated scoped
contradiction between intent and the metadata/generation separation, invalid
capacity and no-replay assertions. It retained every observation limit above;
test outcomes were implementer-attributed and queue lifecycle was not proof.
The implementing context passed all 151 affected tests and the final all-feature
workspace nextest run passed 2219 tests with zero failures and 15 existing skips.
Strict workspace Clippy, formatting and the workspace documentation test passed.

An earlier full nextest run caught missing required `context` fields in this
repair's task definitions; those were restored and both repository-queue tests
passed. That run also saw one checkpoint-lock cleanup assertion fail. The
unchanged checkpoint test then passed isolated, in the 29-test parallel
checkpoint/continuous group, in 40/40 stress iterations and in the full rerun.
Its cause remains unestablished; no lock-recovery fix or certification is claimed.
The capacity-fixture observation does not cover that transient failure.

## Shared-area source observations - 2026-11-07

Independent `maerg-source-audit` read scoped current local source and executable
assertions without intent, queues, previous records/reviews, peers or history.
It made no changes, executed no tests and identified no high-confidence
exploitable vulnerability in scope. This replaces older descriptions of the
changed vendor receipt/reader mechanics, not historical evidence or unrelated
observations. Shared-library guarantees below are called interfaces, not
independently inspected peer behavior.

- The vendor operator exposes project selection, optional registry destination
  and force. Both presentation branches authorize population and propagate
  configuration errors. Rust provisioning loads shared limits and selects a
  refusing callback, configured sandbox acquisition or explicit host provisioning
  (`src/cli.rs:135-147,876-905,1539-1557`,
  `src/vendor_command.rs:103-109,336-385`).
- Unbound protected dependency requests are refused without filesystem inspection
  or provisioning. Separate inspection checks cancellation/budget, policy identity,
  runner, Rust strategy, receipt version, lock digest and called readiness
  interfaces. It does not fulfill a requested dependency
  (`src/vendor_command.rs:135-239`).
- Acquisition owns private scratch, stages bounded stable single-link
  manifests/locks and empty targets, excludes project Cargo configuration and
  generates approved registry settings. It declares Cargo fetch, package-source
  authority and writable acquisition resources. Success requires unchanged staged
  and project locks before called host offline repacking. Scratch cleanup finishes
  before callback return (`src/vendor_command.rs:400-612`,
  `src/sandbox/cargo_context.rs:559-679`, `src/sandbox.rs:1812-2030`).
- Authoring owns staging and an optional Rust read lease through supervised
  execution. Preparation inventories installed toolchains, rejects linked/special
  resources, bounds metadata reads and tracks fingerprints. Application rechecks
  resources and writable overlap, with a local authoring-context exception, and
  replaces Rust/Cargo settings (`src/sandbox/rust_environment.rs:8-17,150-669`,
  `src/sandbox.rs:1127-1140,1442-1452`).
- Offline Rust adaptation calls readiness interfaces and builds registry/config
  mounts with a lock digest. Verification retains a registry read lease through
  construction and supervision, declaring seven grants, denied network and exact
  `cargo test --locked` arguments. Some identities describe paths/catalogues rather
  than complete content. Component lock normalization can invoke host
  `cargo metadata --offline`; offline is not OS confinement
  (`src/language_vendoring.rs:317-400`,
  `src/sandbox.rs:1509-1760,2160-2190,2523-2592`).

Implementer-attributed production-runner nextest
`21afbe63-ae2a-482d-a3f4-36c6ab3d20b2` passed 399 affected cases across components,
including shared-vendoring, native authoring and closed offline verification.
Strict workspace Clippy, formatting, affected doctests and offline release builds
subsequently passed. The observer did not execute these checks. Queue timestamps
record later bookkeeping; review is not human acceptance or a fabricated receipt.
Earlier broader acquisition and durable-baseline intent discrepancies remain
historical unresolved limits, not silently implemented features.

Separate `maerg-blind-comparison` read scoped local intent and the new independent
observations without source, tests, queues or peers. It found correspondence for
shared-limit loading, separate acquisition authority, bounded scratch/lock checks,
protected refusal and read ownership through verification. It reported a command
inventory omission: the older `vendor` entry did not list the already documented
and observed `--force`. The entry now includes that approved option; the finding
is retained here rather than hidden by rewriting the observation.

The comparison retained missing itemized provider proof of exact receipt fields,
policy dialect/default parity and budget propagation, no independent content
attestation, exclusive publication and force bypassing reuse rather than authority.
Read-lease retention is not independently inspected peer publication proof.
Exact Git admission, non-Rust preservation and each recovery case were not
individually established by this source-only supplement. These are evidence gaps,
not reported behavioral violations. Broader historical discrepancies remain.

Final implementer-run production nextest
`7eb7fff4-82b4-4b8d-83a5-9c8e576971ab` passed 399 affected cases. Both release
frontends reported the same ready project tree; Kvist reused it with offline
host policy and no acquisition. Review completion neither accepts stale revisions
nor certifies all requirements.

## Native offline preparation observation (2026-11-07 UTC)

Independent source-only observer `d38462f8-d2f9-4d78-a5bb-c512f6458890`
derived the Maerg section of
[`native-cache-observation.md`](../docs/reviews/native-cache-observation.md).
Its input exclusions and exact source/test references are retained there.
It executed no checks.

Observed default Rust preparation uses native data and separate explicit
directory exports. Configured metadata stages manifests/locks/empty targets,
uses private administration and bounds execution to 30 seconds/remaining
budget; original and staged lock digests are checked afterwards. Unbound
protected dependency requests are refused. Go reuses native module data, pnpm
does not copy the global store, and host manager failures retain bounded
diagnostics without automatic retries. Offline language verification disables
automatic acquisition. Non-Rust presence checks do not prove exact completeness;
vcpkg's generated configuration still uses its existing direct-write path.
No acceptance or certification is inferred.

### Independent security finding and closure

Security reviewer `bf1b95b7-8a98-4777-b332-d8740de4e31f` identified a
high-severity cold non-Rust verification path into host installers
(confidence 9/10, original `language_verification.rs:60` dispatch).
Introduction timing was not established. The implementer's sentinel regression
reproduced a host npm launch before the fix. Verification now requests existing
material with `auto_vendor=false`; six non-Rust preparation tests pass, including
the no-installer regression. The reviewer independently re-observed that this
closes the reported path, without executing tests. Explicit operator host
installation remains authorized. This record is not a receipt or compliance
certification.

Source-blind comparer `a6add327-2a55-4006-b3f6-b2750041ff26` recorded scoped
correspondence and three obsolete-documentation conflicts in
[`native-cache-comparison.md`](../docs/reviews/native-cache-comparison.md).
Their corrections implement the human-approved native design; the same
source-blind comparer re-observed all three conflicts closed. No production
behavior was re-evaluated; broader test failures remain visible and acceptance
metadata remains unchanged.
