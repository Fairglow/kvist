# Agent resilience implementation plan

This human-directed plan addresses the November 6, 2026 review. Sav owns
mechanisms; Skott owns workspace integration/presentation; Maerg owns protected
task authority/evidence. Galla grants and productive uncapped native defaults
are unchanged. A phase commit records work, not human acceptance or a Kvist
review receipt.

| Phase | Work tasks | Required evidence | State |
| --- | --- | --- | --- |
| 1: Correctness | Honest Maerg loop stops; revision-bound read delivery; failed-observation semantics; rejected-proposal recurrence; repair error/decision/digest propagation | Failing regressions first, targeted passing checks, source-only observations and separate source-blind comparison | Complete |
| 2: Streaming | Bounded incremental exact/near repetition; independent answer/reasoning; 100/150/200-word periods; two autonomous corrections; no partial effects, sampling changes or reserve growth for loops | Chunk-boundary, false-positive, recovery, budget/cancellation/recording fixtures through both native hosts | Planned |
| 3: Progress | State-aware repeat protection; discovery/failure/observation tracking; bounded multi-action cycles; no-op resistance; fair batch intervention; pagination/reverification preservation | Alternating/long cycles, repeated refusals, changed-state testing, bulk investigation and compaction regressions | Planned |
| 4: Operations | Typed failures/recovery/effect certainty; actionable TUI/headless/transcript output; first-cause resource termination; aggregate authoring/repair model budgets; recurring repair signatures; cancellable backpressure; honest external capabilities | Integrated terminal/headless and protected-task evidence, offline lint/build/tests, independent final observations/comparison | Planned |

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
