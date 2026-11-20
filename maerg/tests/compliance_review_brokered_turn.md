<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Brokered Turn and Effect Loop (`maerg`)

**Component:** `maerg` (Kvist root component)
**Task:** `brokered-turn-compliance-review`
**Scope:** Source-derived brokered-turn and effect-loop behavior — the host-side
model turn against a loopback gateway, the bounded liveness probe, the closed
authoring tool set, the broker's deny-by-default intent reduction, the
`request_dependency` / `propose_decision` outcomes, the shared wall-clock budget,
the bounded retry policy, output redaction, and the supervised-execution
completion/fence semantics — compared against the agent-integration,
supervised-execution, and execution-boundary requirements and the
behavioral-guarantees contract.
**Reviewer stance:** Source-blind, independent compliance reviewer. This is the
compliance-review stage of the `brokered-turn` chain (tests → implementation →
security audit → compliance review). The implementer and the security auditor did
not author this record. This reviewer did not read implementation source,
dependency manifests, or Git history as evidence.

## Review boundary and independence

Per `REVIEW_RUNBOOK.md` *Source-blind compliance pass*, the reviewer may read only
the target `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`; the `IMPL.md` and
bounded test evidence; the nearest ancestor component `CONTRACT.md` (none — `maerg`
is the root component, `parent_contract: null`); and `ROOT_CONTRACT.md`. Each
clause is classified **compliant**, **mismatched**, **approved-deferred**, or
**underspecified**, with component path, stable locator, intended text, observed
behavior/evidence, severity, and owner.

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
`REVIEW_RUNBOOK.md` (procedure), and bounded test evidence:
`maerg/tests/agent.rs`
(`sha256:9f159d51777c3dd384be34d498b274c5489f83381633feb4a9b5552351700503`),
`maerg/tests/live_runtime.rs`
(`sha256:3b60feb4d213706163df1ce3329977330493fd125f25ba2c0a1a4ed124e49d00`),
`maerg/tests/live_runtime/generation.rs`
(`sha256:322efad8816f77e3e598e728188c97bd33b309efcdad56c6ec6fd3bcdd2d5f9e`),
`maerg/tests/live_runtime/progress.rs`
(`sha256:a2e0c7ba288ad4ef50a53e6e91c836ccf2d5804e7ba21922092b986bb974bcfa`),
`maerg/tests/task_commands.rs`
(`sha256:b30e17736fe2a3a8565a3c9f3daf8ac0317549f37397532dab17d7498ef4c5af`),
`maerg/tests/model_startup.rs`
(`sha256:803d9845b3ec0ae876722e9cc83026d1f225921127dbdc89fbc68b5f3357771c`),
`maerg/tests/agn_authority.rs`
(`sha256:12ffc18027876bd672ad7ba53ac33ea1a525bc8afbae19562781b34886498349`),
`maerg/tests/dogfood_boundary/workflow.rs`
(`sha256:e1ec4a57850d4c3831a100fa2ebcb1d9d6b4cfd33bd7550f9a450d476bf5d7b8`),
and `maerg/tests/dogfood_boundary/support.rs`
(`sha256:dae9928948177ed5003035bf2aa21a91ea02738d1bebe193b5b7f2110fe79dff`).

**Not read:** `maerg/src/**` implementation source (including `src/agent.rs`,
`src/authoring/mod.rs`, `src/task_commands.rs`), `sav/` and `galla/` source,
`Cargo.toml`/`Cargo.lock`, Git history or diffs, prior compliance conclusions,
the completed security-audit report, and peer or parent implementation
artifacts. No tests, builds, linters, or `cargo` invocations were executed in
this review; the agent, live-runtime, task-command, and dogfood-boundary suites
are run automatically by Kvist in the verification sandbox after this turn, and
no pass/fail count from that run is asserted here. The `REVIEW_RUNBOOK.md`
precondition to run the documented quality gate and `kvist doctor .` and resolve
any non-`current` state was not executed in this context; the bounded substitute
available to this reviewer is the digest-bound intent set recorded above.

## Method

Every brokered-turn / effect-loop obligation was drawn from `REQUIREMENTS.md`
(`REQ-AGENT-INTEGRATION` ~L946, `REQ-SUPERVISED-EXECUTION` ~L854,
`REQ-MULTI-TURN-EXECUTION`/`REQ-AGENT-RESILIENCE` ~L872/874,
`REQ-EXECUTION-BOUNDARY` ~L596, `REQ-AGENT-READ-SCOPE` ~L623,
`REQ-DEPENDENCY-REQUEST` ~L676), `CONTRACT.md` `Behavioral guarantees`
(the target multi-turn agent tier, ~L1349–1405), `DESIGN.md` (the multi-turn
loop and phase-4 proposal/dependency tools, ~L47200–54600), and `ROOT_CONTRACT.md`
(untrusted-input rule; no agent command outside the effect sandbox). DESIGN.md
locators are the multi-turn loop and phase-4 sections (~L740–830). All locators
are approximate line numbers in the digest-bound file at review time. Compared
with the observed behavior recorded in `IMPL.md` and the bounded test evidence.
The test assertions are the executable statement of observed behavior in the
places `IMPL.md` is silent; a term sweep of `IMPL.md` and `tests/` for
`request_dependency`, `U+FFFD`, `0xFFFD`, and `non-utf8` informed the findings
below.

## Satisfied clauses (compliant)

**C-1 — The model turn runs on the host against a numeric loopback gateway; any
other command is refused before transport work, so no agent command runs on the
host outside the effect sandbox.** `CONTRACT.md` `Behavioral guarantees`
(~L1355–1357: "The selected model command must target a numeric loopback model
gateway; any other command is refused before any transport work with
`AgentCommandNotModelGateway`, so no agent command ever runs on the host outside
the effect sandbox"; ~L1460 `Errors and failure semantics`); `ROOT_CONTRACT.md`
(no agent command outside the effect sandbox). Evidence:
`execute_agent_runs_the_model_turn_on_the_host_and_captures_the_response`
(`agent.rs` ~L173 — "The profile command embeds the numeric loopback endpoint;
the brokered transport extracts it and performs the turn on the host", and "A
loopback host turn needs no authoring sandbox; the runner and probe are None so
no authoring request is ever dispatched"); `task_run_transitions_to_blocked_on_agent_failure`
(`task_commands.rs` ~L1793 — a non-loopback profile yields a blocked task with a
"numeric loopback" diagnostic in both stderr and `TODOS.yaml`, plus a durable
`agent-execution` attempt record with `success:false`);
`forbidden_native_endpoint_is_refused_before_liveness_connect`
(`live_runtime.rs` — binds a `0.0.0.0` (non-numeric-loopback) endpoint and asserts
`listener.accept().expect_err("must not probe forbidden authority").kind() ==
WouldBlock`, i.e. the refusal precedes any connection). Compliant.

**C-2 — The maerg liveness-probes the gateway with a bounded connection before
issuing the model turn; the probe issues no HTTP request.** `CONTRACT.md`
`Behavioral guarantees` (~L1359–1363: "liveness-probes the gateway with a bounded
TCP connect … before issuing the model turn. The probe issues no HTTP request, so
it never loads, selects, or shifts a model slot"). Evidence:
`serve_local_ollama` / `serve_local_ollama_tool_call` (`agent.rs` ~L97/137 — "the
brokered turn liveness-probes the port first, then performs the turn, so the fake
[gateway] must [accept a TCP connection]"); `task_commands.rs` ~L262 and
`dogfood_boundary/support.rs` ~L148–150 (same probe-before-turn requirement, with
the mock skipping a reset/probe connection rather than answering it). The
fail-fast diagnostic for an unreachable gateway is noted as an evidence limit in
F-E. Compliant for the probe mechanism.

**C-3 — The broker reduces each turn's untrusted tool intents to capability-bound
effects under a deny-by-default policy; a dropped intent is recorded with a
reason and fails the turn without ending the run.** `CONTRACT.md` `Behavioral
guarantees` (~L1367–1370: "The broker reduces each turn's untrusted tool intents
to capability-bound effects under a deny-by-default policy; a dropped intent fails
that turn without ending the run"); `ROOT_CONTRACT.md` (untrusted-input rule).
Evidence: `agn_authority.rs` (ungranted and out-of-scope writes land in
`plan.dropped` with a non-empty `reason` matching "not granted"/"unauthorized":
"ungranted tool should be dropped", "write to unbound path should be dropped");
`generation_discarded_unary_tools_never_reach_the_broker`
(`live_runtime/generation.rs` ~L345); `IMPL.md` `Conversation and effect
ownership` (the broker reduces untrusted proposals to file effects, decisions,
accepted dependency requests, or dropped intents; dropped intents prevent file
effect application). Compliant.

**C-4 — `write_file`/`edit_file` are reduced to the writable component scope and
applied by the maerg itself inside the effect sandbox against a read-only
staged-intent mount; the host never writes component state for an effect, and only
the final brokered effect of a run is persisted.** `CONTRACT.md` `Behavioral
guarantees` (~L1371–1374); `DESIGN.md` (~L48819–48992). Evidence:
`task_run_executes_successfully_and_transitions_completed` (`task_commands.rs`
~L953 — "the mock answers the first turn with a `write_file` tool call so the full
effect loop (staging and in-sandbox applier) runs, and later turns with a final
answer so the multi-turn loop completes"); `IMPL.md` `Conversation and effect
ownership` (effects are staged under component state with content identity, then
applied by the current Kvist executable inside the effect sandbox; the applier
revalidates destination/content and edit preconditions before replacement;
effects are sequential, not transactional). Compliant.

**C-5 — `propose_decision` records an impactful, uncovered decision as a redacted
proposal under the component state directory and ends the run by placing the task
in `awaiting-decision`; the run harness never runs verification or jumps to
completed; a decision never writes a protected intent document.** `CONTRACT.md`
`Behavioral guarantees` (~L1381–1385); `DESIGN.md` (~L51431). Evidence:
`execute_agent_surfaces_a_proposed_decision_and_pauses` (`agent.rs` ~L253 —
`!result.success`, `result.surfaced_decision`, `!result.timed_out`,
`!result.output_limit_exceeded`; the proposal is persisted under
`.kvist/authoring/proposals/` as one JSON proposal plus one Markdown companion,
and is never applied as a file effect);
`resilience_repair_decision_uses_the_initial_awaiting_decision_disposition`
(`task_commands.rs` ~L1833 — stdout contains "awaiting-decision", `TODOS.yaml`
gains `status: awaiting-decision`, and the attempt record carries
`"surfaced_decision":true` and `"stop_reason":null`, i.e. it is a pause, not a
fatal or completed stop); `IMPL.md` (a decision records bounded proposal evidence
and pauses before that turn's effects; decision/dependency evidence filenames are
SHA-256 hashes of call IDs, so untrusted IDs are payload data, not path
selectors). Compliant.

**C-6 — A run reaches complete only when the agent reports completion, no
decision worthy of intervention remains surfaced, and every authorized effect
applied; a single turn's success is not completion, and loop termination must not
synthesize completion.** `REQ-MULTI-TURN-EXECUTION` (~L900–906); `REQ-AGENT-
RESILIENCE` ("Loop termination MUST NOT synthesize completion or report authoring
success"; ~L923); `CONTRACT.md` `Behavioral guarantees` (~L1387, ~L1393).
Evidence: `resilience_repeated_reads_are_stalled_not_completed`
(`live_runtime.rs` ~L650 — a read loop is not completion: `!result.success`,
stderr "stalled", `calls <= 6`); `resilience_tool_only_repeated_reads_never_synthesize_an_answer`
(~L661 — `!result.success`, no "Completed" in stdout);
`resilience_more_than_30_productive_reads_reach_explicit_completion` (~L690 —
36 productive reads then explicit completion: "novel inspection must not hit a
turn ceiling"); `task_run_executes_successfully_and_transitions_completed`
(`task_commands.rs` ~L953). Compliant.

**C-7 — Read coverage binds the observed content revision: changed content
reaches the model rather than being suppressed as unchanged; error observations
retain failure semantics and do not count as useful reads.** `REQ-AGENT-
RESILIENCE` (~L885–892); `REQ-AGENT-READ-SCOPE`. Evidence:
`resilience_external_change_reaches_the_model_in_a_covered_range`
(`live_runtime.rs` ~L712 — after an external edit between turns, turn 3's request
contains "externally changed revision": "covered range hid new bytes" guard);
`resilience_pseudo_read_errors_have_failed_outcomes_and_no_progress` (~L739 —
failed reads surface `status=Some(1)` or `status=None` + "not executed" and make
no progress). Compliant.

**C-8 — Completion requires a clean verification result; failed verification
blocks the task, and verification-repair failures are explicit, bounded evidence
failures that preserve the original cause.** `CONTRACT.md` `Errors and failure
semantics` (~L1470: "Failed verification blocks the task with bounded redacted
evidence"); `REQ-AGENT-RESILIENCE` ("Repair rounds remain finite and cannot
advance work without observed progress …"). Evidence:
`resilience_repair_no_progress_is_an_explicit_recorded_failure`
(`task_commands.rs` ~L1839 — `!success`, stderr "no source/test progress" and
"original verification failure", attempt record `"phase":"verification-repair"`);
`resilience_repair_digest_errors_are_explicit_evidence_failures` (~L1843 — digest
failure surfaces "per-file byte limit" alongside the original verification
failure). Compliant.

**C-9 — The shared wall-clock budget equals the configured profile timeout and
covers the probe, every turn, every retry backoff, every read, and every brokered
effect; exhausting it times out the run with durable evidence.** `REQ-MULTI-TURN-
EXECUTION` (~L908–912); `CONTRACT.md` `Behavioral guarantees` (~L1393–1395).
Evidence: `task_run_cancels_agent_timeout_with_durable_evidence`
(`task_commands.rs` ~L4147 — "the gateway delays 2s before answering so the 1s
shared wall-clock budget is exhausted and the turn times out" with
`timeout_seconds = 1`; `!success`, stderr "timed out", `TODOS.yaml` "agent
execution timed out", and a durable attempt file); `IMPL.md` `Scoped
runtime-budget observations` (~L1008–1010: "Model requests and effects receive
time from one elapsed-time budget, with [the] same budget"). Compliant for budget
equality and exhaustion; the per-attempt transport ceiling is noted in F-C.

**C-10 — Only transient gateway availability failures are retried, bounded,
without replaying effects; response-level failures and surfaced decisions end the
loop deterministically; a streamed attempt that already emitted text (or an
interrupted stream) is discarded rather than partially replayed.** `REQ-MULTI-
TURN-EXECUTION` (~L913–915); `CONTRACT.md` `Behavioral guarantees` (~L1397–1405).
Evidence: `task_run_skips_unavailable_capacity_and_retries_inference_without_replaying_the_task`
(`task_commands.rs` ~L3658 — a transient 429 yields four `model_retry` journal
events and "Retrying automatically" notices, while capacity probes are excluded
from the inference-retry count; the task is left pending, not completed or
replayed); `provider context rejection must not be retried or treated as success`
(`model_startup.rs` ~L398 — a response-level rejection leaves `model_request`
count == 1); `interrupted_stream_discards_provisional_output_and_incomplete_tool_proposals_before_retry`
(`task_commands.rs` — an interrupted stream's provisional output and incomplete
tool proposals are discarded before any retry); `IMPL.md` `Scoped resilient task
execution repair` ("Stream events are provisional. Only a completed validated turn
reaches the broker; retries explicitly discard partial output without executing
partial proposals … No retry loop encloses effects, verification or the whole
task"). Compliant for the retry *policy* (transient-only, no effect replay,
streamed-emitted never retried); the exact numeric bound is addressed in F-C.

**C-11 — Configured literal redactions and output bounds apply before durable
attempt evidence is written; streamed deltas are relayed to stdout with the run's
redaction values applied, reassembled across delta boundaries.** `CONTRACT.md`
`Security and authority` (~L1510); `Behavioral guarantees` (~L1401–1404).
Evidence: `task_run_redacts_a_secret_split_across_streams_before_log_and_streaming`
(`task_commands.rs` ~L4199 — a secret split across two deltas, "cross-"/
"stream-secret", is reassembled by the relay redactor: neither the combined
stdout+stderr nor the durable log contains the secret, and the log contains
`[REDACTED]`); `task_run_cancels_agent_output_and_persists_redacted_bounded_evidence`
(~L4075), `task_run_redacts_all_verification_evidence_blockers_and_cli_output`
(~L4265), and `protected_run_is_observable_before_provider_completion_and_is_redacted`
(`live_runtime.rs` — the live journal exists before the provider completes and is
redacted). Compliant.

**C-12 — Supervised execution is a unique, reviewable attempt; completion is
distinct from human acceptance; the supervised tier advertises no automatic
retry; ambiguous or interrupted effects fence rather than replay; and a retained
attempt fences every queue writer until recovery.** `REQ-SUPERVISED-EXECUTION`
(~L856–866); `CONTRACT.md` `Behavioral guarantees` (~L1340–1345, ~L1236);
`ROOT_CONTRACT.md` `Change and compliance rules` ("Do not treat external process
success as human acceptance"). Evidence: `successful_process_and_verification_remain_pending_for_human_disposition`
(`dogfood_boundary/workflow.rs` ~L72 — a successful run leaves the task
`status: in-progress` (not completed), records
`pending-human-disposition`, and creates no VCS commit; only `task finalize …
accept` completes it); `human_can_block_a_successful_attempt_without_automatic_retry`
(~L150 — finalization appends exactly one event to the existing attempt,
count == 5: "finalization must append one event to the existing attempt, not
retry it"); `continuous_cli_requires_approved_execution_and_exposes_recovery_and_finalization`
(~L32 — the CLI advertises no `--max-retries`/`--retry`: "the supervised tier
must not advertise automatic retry"); `ambiguous_effects_remain_fenced_and_cannot_be_force_unlocked`
(~L579) and `unresolved_attempt_fences_every_component_queue_writer` (~L847).
Compliant.

## Discrepancies (mismatched / underspecified — retained for arbitration)

**F-A (HIGH, mismatched) — The closed authoring tool set: the intent and the
observed-behavior carrier disagree about whether `read_file` is a live,
advertised tool.**
- *Locator:* `CONTRACT.md` `Behavioral guarantees` ~L1365 ("The loop advertises
  the closed authoring tool set (`read_file`, `write_file`, `edit_file`,
  `request_dependency`, and `propose_decision`)"); `DESIGN.md` (~L47300: "The
  loop runs a closed authoring tool set: `read_file`, `write_file`, `edit_file`,
  `request_dependency`, and `propose_decision`"); `IMPL.md` `Observed
  requirements and constraints` ~L1808 ("The closed authoring vocabulary is
  `write_file`, `edit_file`, `propose_decision`, and `request_dependency`. …
  These tools do not grant arbitrary shell execution or general file reading.");
  `REQ-AGENT-INTEGRATION` / `REQ-AGENT-READ-SCOPE`.
- *Intended (declared):* the closed set is five tools **including** `read_file`,
  with `read_file` "honored within the read scope and logged to the trajectory
  but [producing] no persisted effect" (`CONTRACT.md` ~L1371).
- *Observed:* the observed-behavior carrier (`IMPL.md` ~L1808) states the closed
  vocabulary is the four **write/decision/dependency** tools and explicitly says
  they grant "no … general file reading" — omitting `read_file`. Yet the bounded
  tests exercise `read_file` as an authorized in-loop tool whose results are
  fed back to the model: `read_call` helper (`live_runtime.rs` ~L646),
  `resilience_repeated_reads_are_stalled_not_completed` /
  `resilience_more_than_30_productive_reads_reach_explicit_completion` /
  `resilience_external_change_reaches_the_model_in_a_covered_range`
  (`live_runtime.rs` ~L650/690/712), and the `read_file` tool call at
  `live_runtime/progress.rs` ~L266; C-6 and C-7 above are pinned *through*
  in-loop reads. So the intent (5 tools) and the observation (4 tools, "no
  general file reading") contradict each other even though the tests show
  `read_file` operating in the loop.
- *Why it matters:* the closed tool set is the security-relevant enumeration of
  what the model may invoke each turn. If `read_file` is live (as the tests show)
  but the observed-behavior record says the vocabulary excludes general file
  reading, then either `IMPL.md` understates the live surface (a documentation
  defect on the observation carrier) or the advertised set and the authorized
  set have diverged. Either way the source-blind comparison cannot record
  "compliant" for the tool-set clause.
- *Arbitration options:* (a) re-derive the `IMPL.md` observation so the closed
  vocabulary lists all five advertised tools and states that `read_file` is
  honored within the bounded read scope and logged (then re-run this pass);
  (b) if `read_file` is intentionally *not* advertised to the model and only the
  four tools are advertised, correct `CONTRACT.md`/`DESIGN.md` and the tests that
  drive in-loop `read_file` calls; (c) reasoned exception recording which of the
  two is authoritative. Owner: project architect.

**F-B (HIGH, mismatched / design-flagged deferral) — The in-policy
`request_dependency` "accepted, agent continues, and acquires the revision
through the build/verification step" path is declared in intent, observed as
refused, and pinned by no bounded test.**
- *Locator:* `CONTRACT.md` `Behavioral guarantees` ~L1376–1380 ("a request whose
  origin is an exact, pinned registry revision or a public VCS origin with an
  exact pinned revision … is recorded and accepted so the agent continues without
  interruption and acquires the revision through the build/verification step; a
  request outside that policy is surfaced as a decision"); `DESIGN.md` ~L761
  ("a request within policy is fetched automatically so the agent continues
  without interruption") but `DESIGN.md` ~L823 ("An in-policy request … is
  evaluated, but automatic fulfillment is not implemented. An out-of-policy
  request is surfaced as a decision and stops the run in `AwaitingDecision`. …
  The automatic design remains an unmet target for explicit arbitration; this
  note is not a compliance pass."); `IMPL.md` `Protected provisioning and
  verification` (~L124485: "That entry point refuses all model dependency
  fulfillment: it lacks an exact requested-dependency binding to the existing
  lock/manifest"); `REQUIREMENTS.md` `REQ-DEPENDENCY-REQUEST` (~L676, which
  records the ADR 0015 "exact request-to-lock identity" limitation as a current
  delivery limitation).
- *Intended (declared):* an in-policy `request_dependency` is *accepted* and the
  agent continues, acquiring the revision via the build/verification step.
- *Observed:* `IMPL.md` and `DESIGN.md` (phase 4) observe that automatic
  fulfillment is **not** implemented — the protected entry point refuses before
  any fetch because exact request-to-lock identity binding is absent — and
  `DESIGN.md` explicitly designates the automatic design as "an unmet target for
  explicit arbitration." A term sweep of `tests/` found **zero** occurrences of
  `request_dependency`: no bounded test pins an in-policy request being accepted
  and the run continuing, and none pins an out-of-policy request surfacing a
  decision. The `IMPL.md` scope note (~L127592) additionally records that the
  dependency broker's private-origin predicate checks prefixes of the full origin
  rather than parsing the host, so a private numeric host inside an HTTPS URL can
  be misclassified as accepted (classification only; protected fulfillment still
  refuses before any fetch).
- *Why it matters:* the contract states the in-policy continue-and-acquire path as
  current behavior, but the observed behavior is a refusal and the retained test
  suite does not exercise either the in-policy or the out-of-policy
  `request_dependency` outcome. This is a live intent-versus-observation
  mismatch, and the out-of-policy surfacing (which *is* implemented per
  `IMPL.md`) is nonetheless untested in the bounded evidence.
- *Arbitration options:* (a) implement and pin the in-policy accepted/continue
  path (then re-run this pass); (b) amend `CONTRACT.md`/`DESIGN.md` to state that
  in-policy `request_dependency` is evaluated and *recorded* but fulfillment is
  refused until request-to-lock identity binding exists (matching the observed
  refusal), and pin an out-of-policy-surfaces-a-decision test; (c) record the
  approved deferral already noted in `DESIGN.md`/`REQUIREMENTS.md` as the
  authoritative status and pin the refusal + out-of-policy surfacing. Owner:
  project architect.

**F-C (MEDIUM, underspecified) — The bounded retry "three attempts" figure does
not reconcile the two observed retry scopes (transient generation retry vs.
inference/transport retry), and the per-attempt transport ceiling is not
independently pinned.**
- *Locator:* `CONTRACT.md` `Behavioral guarantees` ~L1399 ("the maerg retries it
  up to three attempts with a short fixed backoff, then surfaces the error if it
  never succeeds") and ~L1395 ("each per-attempt transport deadline is clipped to
  the remaining budget and independent transport/profile ceiling");
  `REQ-MULTI-TURN-EXECUTION` (~L913: "Only transient gateway availability
  failures are retried a bounded number of times"); `IMPL.md` `Observed
  requirements and constraints` (~L1772: "Agent execution has a 64-generation
  ceiling and a shared elapsed-time budget. Transient generation retries are
  capped at three attempts, **additionally bounded by shared native retry
  settings** …"); `task_commands.rs` ~L3658 (`task_run_skips_unavailable_capacity_and_retries_inference_without_replaying_the_task` —
  comment "profile permits more than three inference attempts"; four
  `model_retry` journal events for a transient 429 sequence).
- *Intended (declared):* a single transient-failure retry bound of "up to three
  attempts."
- *Observed:* `IMPL.md` describes *two* retry scopes — transient **generation**
  retries (capped at three) and **inference/transport** retries "additionally
  bounded by shared native retry settings" — and the bounded retry test allows the
  profile to permit **more than three** inference attempts (four `model_retry`
  events). The contract's lone "three attempts" figure does not say which scope
  it bounds, so it cannot be verified as the operative bound, and the
  independent transport/per-attempt ceiling is asserted only indirectly (the
  budget-exhaustion test C-9 clips against the profile timeout, not against a
  separately declared transport ceiling).
- *Why it matters:* a retry bound is a resource/liveness guarantee. As written,
  the contract under-specifies whether "three attempts" applies to generation
  only (leaving inference retries governed by an additional native setting that a
  profile can raise above three), which is exactly the ambiguity the bounded
  evidence exposes. The *policy* (transient-only, no effect replay,
  response-level never retried, streamed-emitted never retried) is compliant
  (C-10); only the numeric bound across scopes is underspecified.
- *Arbitration options:* (a) state in `CONTRACT.md` which retry scope the
  "three attempts" bound applies to and how the inference/transport retry is
  separately bounded (native setting + remaining budget + transport ceiling),
  with a pinning test; (b) reasoned exception that the generation retry is the
  contract-bound scope and inference retries are governed by the approved native
  retry setting plus the shared budget; (c) deferral with owner. Owner: project
  architect.

**F-D (MEDIUM, underspecified / unverified) — Non-UTF-8 captured bytes represented
with U+FFFD replacement characters is declared but has no bounded test or
observation.**
- *Locator:* `REQ-AGENT-INTEGRATION` (~L962: "Captured bytes that are not valid
  UTF-8 MUST be represented with U+FFFD replacement characters."); `CONTRACT.md`
  / `DESIGN.md` (no separate clause for byte-level encoding of captured agent
  output); `IMPL.md` (no observation of U+FFFD / non-UTF-8 handling; a term sweep
  of both `IMPL.md` and `tests/` for `U+FFFD`, `0xFFFD`, and `non-utf8` found no
  assertion).
- *Intended (declared):* non-UTF-8 captured bytes are rendered as U+FFFD.
- *Observed:* no bounded test feeds non-UTF-8 captured bytes through the brokered
  turn and asserts U+FFFD rendering; `IMPL.md` carries no such observation.
- *Why it matters:* this is an output-fidelity rule on untrusted bytes. It is
  neither pinned nor observed in this source-blind pass, so it cannot be recorded
  compliant.
- *Arbitration options:* (a) pin a bounded test that a non-UTF-8 captured byte
  sequence is surfaced as U+FFFD in the relayed/logged output; (b) reasoned
  exception if the transport layer already guarantees valid UTF-8 and the rule is
  defensive; (c) deferral with owner. Owner: project architect.

**F-E (LOW, evidence limit) — The exact unreachable-gateway fail-fast diagnostic
text is not independently asserted in the bounded evidence.**
- *Locator:* `CONTRACT.md` `Behavioral guarantees` (~L1361–1363: "a gateway that
  does not accept a connection fails fast with `LocalModelGatewayUnreachable` and
  an actionable \"ensure the model server is running and listening on
  `{endpoint}\" hint").
- *Intended (declared):* fail fast with `LocalModelGatewayUnreachable` plus that
  actionable hint.
- *Observed:* the probe-before-turn mechanism and the refusal-before-transport for
  non-loopback endpoints are pinned (C-1, C-2), but the specific
  `LocalModelGatewayUnreachable` hint wording is not asserted in the bounded test
  evidence read here (the "numeric loopback" diagnostic that *is* asserted in
  `task_commands.rs` ~L1835/1841 is the distinct `AgentCommandNotModelGateway`
  refusal path). `IMPL.md` (~L1773: "The probe independently requires a numeric
  loopback address, uses short connects, checks cancellation, and performs
  cancellation-aware backoff") is consistent but does not quote the hint.
- *Why it matters:* low — the mechanism is compliant; only the exact operator-facing
  string is unverified in this pass.
- *Arbitration options:* none required beyond optionally pinning the hint text;
  recorded for evidence-completeness only.

## Evidence limits (explicit)

- No tests, builds, linters, or `cargo` invocations were executed by this
  reviewer; the `agent`, `live_runtime` (incl. `generation`/`progress`),
  `task_commands`, `model_startup`, `agn_authority`, and `dogfood_boundary`
  suites are run automatically by Kvist in the verification sandbox after this
  turn, and no result from that run is asserted.
- The runbook precondition to run the documented quality gate and `kvist doctor .`
  and resolve any non-`current` state was not executed in this context. The
  intent digests are recorded as of this review; they were not cross-checked
  against the queue `revalidation` block, so no queue-revision equality is
  asserted.
- The nearest-ancestor component contract is `null` (`maerg` is the root
  component); `ROOT_CONTRACT.md` is the only propagated higher-level context.
- The security-audit stage of this chain is completed per the queue, but its
  report is not in this reviewer's permitted read set; this review neither relies
  on nor repeats its conclusions.
- Optional native schemas are not referenced by `CONTRACT.md` for the brokered
  turn / effect loop and therefore count as no evidence.
- All "observed" statements are attributed to the bounded test evidence or to
  `IMPL.md`; no implementation source was inspected. Term sweeps of `tests/` and
  `IMPL.md` underpinned the "absent from bounded evidence" portions of F-B and
  F-D.

## Arbitration required

The project architect owns arbitration and may request redesign and
implementation changes, explicitly approve an intent-document change, or
record a reasoned exception or approved deferral. This review does **not**
modify `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, or `IMPL.md` to erase
any mismatch, and it does not treat external process success as acceptance.
The required decisions, in priority order, are:

1. **F-A (HIGH):** Decide which closed authoring tool set is authoritative —
   five tools including `read_file` (matching `CONTRACT.md`/`DESIGN.md` and the
   in-loop `read_file` tests), or four tools (matching `IMPL.md` ~L1808) — and
   re-derive `IMPL.md` or amend the intent + tests so they agree.
2. **F-B (HIGH):** Decide the status of the in-policy `request_dependency`
   accepted/continue/acquire path — implement and pin it, amend the intent to
   the observed refusal (evaluated + recorded, fulfillment refused until
   request-to-lock binding exists) and pin the out-of-policy-surfaces-a-decision
   test, or record the `DESIGN.md` deferral as authoritative. No bounded test
   currently pins either `request_dependency` outcome.
3. **F-C (MEDIUM):** Decide which retry scope the "three attempts" contract bound
   applies to and how the inference/transport retry is separately bounded, or
   record a reasoned exception; pin accordingly.
4. **F-D (MEDIUM):** Decide whether the U+FFFD non-UTF-8 rule is pinned by a
   bounded test, excepted as defensive, or deferred.
5. **F-E (LOW):** No action beyond optionally pinning the fail-fast hint text;
   recorded for evidence-completeness only.

This record, the fresh `IMPL.md`, and any approved arbitration decision are to
be retained in version control. No raw provider transcript, credential,
secret, or temporary review workspace is retained.
