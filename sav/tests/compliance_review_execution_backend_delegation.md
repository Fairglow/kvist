<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Execution-Backend Delegation (`sav`)

**Component:** `sav`
**Task:** `sa-03-compliance-review` (Review execution-backend delegation compliance)
**Date:** 2026-11-21
**Verdict:** **BLOCKED pending human arbitration.** The delegation *behavior*
corresponds to the execution-backend contract on the permitted static evidence,
but the *documented assurance tier* cannot be confirmed from the contract, the
expected *capability reporting* is neither specified nor evidenced, and the
delegation-seam implementation record is absent. Precise discrepancies M-1,
M-2 and M-3 are retained for arbitration; O-1, O-2 and O-3 are retained as
LOW observations.

## Review basis, scope, and independence

This is the final stage of the `sa-03` chain
(`sa-03-write-tests` → `sa-03-implement-code` → `sa-03-security-audit` →
`sa-03-compliance-review`), recorded in `sav/TODOS.yaml`. Per
`REVIEW_RUNBOOK.md` ("Source-blind compliance pass"), this comparison was made
without reading implementation source, dependency manifests, Git history, or
private peer artifacts. The implementer and the security auditor did not author
this record, and this record is not self-certified.

**Read (permitted intent context):** `sav/REQUIREMENTS.md`, `sav/CONTRACT.md`,
`sav/DESIGN.md` (§`Security and resource design`),
`docs/decisions/0015-shared-runtime-and-run-inspection.md`,
`ROOT_CONTRACT.md` (§`Change and compliance rules`), the
`REVIEW_RUNBOOK.md` procedure, and the canonical `sa-03` task-definition
projection in `sav/TODOS.yaml`.

**Read (permitted implementation record and bounded test evidence):** the
`sav::runtime` sections of `sav/IMPL.md` (clean-slate source-only observations;
read for the *absence* of a delegation-seam record), and the bounded test
evidence `sav/tests/execution_backend_delegation.rs` (six named tests, listed
below).

**Cited only as an attributed record, not re-derived here:**
`sav/SECURITY_AUDIT.md` (component-level, dated 2025-12-18), used solely to
assess chain provenance in M-3; and the retained `shared-runtime` comparison
(`sav/tests/compliance_review_shared_runtime.md`), used solely to carry forward
its M-1/N-5 precedent on the injected-executor boundary.

**Not read as evidence:** implementation source, manifests, build artifacts,
Git history/diffs, prior compliance conclusions beyond the named precedent,
peer or parent implementation artifacts, or agent transcripts.

**Not executed:** the reviewer ran no tests, builds, or probes. The test
evidence below is static: named test functions and their assertions, cited as
executable evidence rather than as a fresh run result. No pass/fail count is
asserted; the suites are run automatically by Kvist in the verification
sandbox.

## Bounded test evidence relied upon

`sav/tests/execution_backend_delegation.rs` drives the loop against a scripted
model and a recording executor that "models the host's delegation to galla";
"the runtime under test is the delegation seam itself." Six tests:

1. `delegates_every_effect_to_the_executor` — with runner and backend
   available, `executor.effects() == 1`, `summary.tools_executed == 1`,
   `summary.answer == Some("done")`.
2. `probes_runner_and_backend_before_dispatch` —
   `executor.probe_calls >= 1` and `executor.effects() == 1` ("the effect runs
   only after the probe confirms capability").
3. `fails_closed_when_the_runner_is_unavailable` — `executor.effects() == 0`,
   `summary.tools_executed == 0`, `summary.failure` contains
   `"galla runner is unavailable"`, and some
   `Event::ToolResult { failed: true, .. }` is emitted.
4. `fails_closed_when_the_backend_is_unavailable` — same shape; refusal text
   contains `"Bubblewrap backend is unavailable"`.
5. `binds_delegated_effects_to_the_installed_runner` — with
   `bound_runner != presented_runner`, `executor.effects() == 0`,
   `summary.tools_executed == 0`, refusal text contains
   `"runner binding mismatch"`.
6. `accepts_a_correctly_bound_effect` — with `bound_runner ==
   presented_runner`, `executor.effects() == 1`,
   `summary.tools_executed == 1`, `summary.stop_reason ==
   Some(StopReason::Completed)`.

The recording executor's `execute` probes before dispatch, refuses with
`Error::ToolPrerequisite` on missing runner/backend, refuses with
`Error::Host` on a broken binding, and causes an effect (bounded
`ToolOutcome`) only when the probe passes and the binding holds.

## Clause comparisons

`R` = `REQUIREMENTS.md`, `C` = `CONTRACT.md`, `D` = `DESIGN.md`, `I` =
`IMPL.md`, `T` = the bounded test evidence above.

### C-1 — Every effect is delegated to the host executor; the runtime owns no
executor and has no host fallback. **Compliant.**

- `R` `AR-REQ-NATIVE-RUNTIME`: "The planned native runtime MUST separate model
  transport, bounded agent loop, typed tool broker, host authorization,
  execution backend, and redacted runtime events. Tool intent is untrusted and
  cannot cause effects without an explicit host decision and selected
  backend."
- `C` "Shared native runtime API (ADR 0015)": "`ToolExecutor`, `EventSink` and
  `Recorder` are host interfaces: no executor, policy, selected model
  configuration, terminal or application task type is provided."
- `C` "Provided interfaces": "Native hosts supply their own tools, execution
  and recorders; a refusal never selects another executor."
- `D` "Security and resource design": "Under ADR 0015, the native runtime
  delegates effects only to the executor injected by its embedding host; Sav
  neither selects a runner nor owns grants."
- `I`: "`NativeRuntime` consumes an … injected `ToolExecutor` … The executor
  owns effects and policy. There is no implicit process fallback or registry
  reload inside the loop."
- `T`: test 1 (`delegates_every_effect_to_the_executor`).

### C-2 — Fail-closed refusal: unavailable runner or backend is refused before
the effect, surfaced, and recorded as not executed. **Compliant.**

- `C` "Shared native runtime API (ADR 0015)": "`Error::ToolPrerequisite
  {tool, reason}` explicitly reports an approved local resource unavailable
  before dispatch; generic I/O errors cannot claim that certainty. Missing
  prerequisites stop for host repair without automatic downloads or authority
  growth."
- `C` "Operational diagnostics and stopping causes": "`EffectCertainty`
  distinguishes `NotExecuted` from `MayHaveEffects`. Known refusals are not
  executed."
- `D`: "Protected hosts must bind the installed `galla` and its verified
  capabilities before dispatch, failing closed when prerequisites are absent."
- `T`: tests 3 and 4 (`fails_closed_when_the_runner_is_unavailable`,
  `fails_closed_when_the_backend_is_unavailable`): refusal text surfaced in
  `summary.failure`, `tools_executed == 0`, and a failed `ToolResult` event
  emitted.

### C-3 — No unconstrained host execution is ever substituted. **Compliant.**

- `C` "Provided interfaces": "Native hosts supply their own tools, execution
  and recorders; a refusal never selects another executor."
- `D`: "Sav constructs no isolation namespaces and never substitutes an
  unconstrained executor when a host executor fails." ADR 0015: "there is no
  unconfined fallback or nested native tool loop."
- `T`: all refusal tests assert `executor.effects() == 0` and
  `summary.tools_executed == 0`; the test header pins "never run on the host"
  and "never substitutes an unconstrained host execution."

### C-4 — A correctly bound, probed effect is executed once. **Compliant.**

- `C` "Operational diagnostics and stopping causes": "`StopReason` …
  `Completed`; `RunSummary::stop_reason` records the observed terminal
  cause."
- `T`: test 6 (`accepts_a_correctly_bound_effect`): `effects() == 1`,
  `tools_executed == 1`, `stop_reason == Some(Completed)`.

### C-5 — Capability reporting for the execution backend. **Mismatched.**

- `sa-03-implement-code` expected outcome (`sav/TODOS.yaml`): "Strict Linux
  runs expose only approved resources and tools, **report backend
  capabilities**, and fail before launch on incomplete enforcement."
- `D` "Security and resource design": "Protected hosts must bind the installed
  `galla` and its **verified capabilities** before dispatch."
- `C` "Shared registry configuration (ADR 0015)" (nearest capability-state
  tier): profiles carry "separate `advertised`, `tested`, and
  `policy_enabled` capability lists (default empty)."
- `T`: no test in `execution_backend_delegation.rs` reports or asserts a
  backend capability set. The recording executor's only capability input is
  the two booleans `runner_available` and `backend_available`; it reports no
  capability identifiers, and no test compares a reported capability set
  against an expected tier.
- See M-1.

### C-6 — Protocol binding of delegated effects to the installed runner
identity. **Compliant as to the delegation boundary; not specified as a
contract guarantee (see O-1).**

- `D` "Security and resource design": "Protected hosts must bind the installed
  `galla` and its verified capabilities before dispatch, failing closed when
  prerequisites are absent."
- `C`: names no runner-identity binding and no `Error::Host` variant for a
  broken binding.
- `T`: tests 5 and 6 (`binds_delegated_effects_to_the_installed_runner`,
  `accepts_a_correctly_bound_effect`) pin the behavior, including refusal
  text "runner binding mismatch."
- See O-1.

### C-7 — Change-and-compliance process for this chain. **Compliant.**

- `ROOT_CONTRACT.md` §`Change and compliance rules`: "`TODOS.yaml` orders work
  as tests, implementation, security audit, then compliance review"; "A
  clean-slate documenter and a separate compliance reviewer must verify
  implemented behavior before it is declared compliant"; "Record
  intent-to-implementation discrepancies for explicit arbitration; do not
  silently alter either artifact."
- Observed: the `sa-03` chain projection reflects that order and the
  in-progress state of this review; this record retains its discrepancies for
  arbitration and alters no requirement, contract, design, implementation
  record, or implementation.

## Discrepancies (retained for human arbitration)

### M-1 (HIGH, mismatched) — "report backend capabilities" is not
specified in the contract and is not evidenced in the bounded tests.

- *Locator:* `sav/TODOS.yaml` `sa-03-implement-code` expected outcome
  ("report backend capabilities"); `sav/DESIGN.md` §`Security and resource
  design` ("bind the installed `galla` and its verified capabilities before
  dispatch"); `sav/CONTRACT.md` §`Shared native runtime API (ADR 0015)` and
  §`Shared registry configuration (ADR 0015)`; `sav/tests/
  execution_backend_delegation.rs`.
- *Intended:* the delegation surface reports the verified backend
  capabilities of the installed runner, and that reporting is part of the
  contract consumers may rely on (task purpose: "Delegate least-authority
  isolation to the approved layer with no silent fallback"; task
  expected_outcome: "report backend capabilities").
- *Observed:* `CONTRACT.md` names the capability-state tier (`advertised`,
  `tested`, `policy_enabled`) only for profile registry capabilities
  (`Shared registry configuration (ADR 0015)`), not for the execution backend.
  It provides no execution-backend capability type, field, event, or
  guarantee, and no test in the bounded delegation suite reports or asserts
  any capability set. The recording executor models only the
  available/unavailable boolean state.
- *Why it matters:* the task's own expected outcome names capability
  reporting as a deliverable, and the design says hosts bind "verified
  capabilities"; if the contract does not specify the report, the behavior is
  neither required nor verifiable from the public surface, and the
  compliance review cannot confirm that "report backend capabilities" holds
  for any tier.
- *Arbitration options:* (a) add the execution-backend capability report to
  `CONTRACT.md` (typed surface, tier semantics, fail-closed behavior) and the
  bounded tests, then re-review; (b) amend `sa-03` scope to name the
  availability probes (runner/backend) as the capability reporting the design
  means and record that the tier is `advertised` only; (c) record a reasoned
  deferral of capability reporting with the retention condition and tier
  explicitly named.

### M-2 (HIGH, mismatched) — the delegation seam has no `IMPL.md`
implementation record.

- *Locator:* `sav/IMPL.md` (whole record); `ROOT_CONTRACT.md` §`Change and
  compliance rules` ("A clean-slate documenter and a separate compliance
  reviewer must verify implemented behavior before it is declared
  compliant"); `REVIEW_RUNBOOK.md` §`Source-blind compliance pass` ("the
  newly produced `IMPL.md` and bounded test evidence").
- *Intended:* the delegation seam — resolution and binding of the installed
  galla runner, probing of its verified backend, delegation of every approved
  effect through its version-one protocol, and the fail-closed refusal
  paths — is described in the component's implementation record so the
  compliance reviewer can compare intended and observed behavior beyond the
  test source.
- *Observed:* `IMPL.md` records the `NativeRuntime` loop ("There is no
  implicit process fallback or registry reload inside the loop"; "The
  executor owns effects and policy") but contains no delegation-seam record:
  the words "delegation", "galla", "runner", and "Bubblewrap" appear nowhere
  in it, and no section describes runner resolution, probe trust, protocol
  binding, or the refusal outcomes the chain implemented on 2026-11-21.
- *Why it matters:* this comparison can corroborate the delegation *behavior*
  only against the test source, which is written by the same chain it gates.
  Without the independent clean-slate record, the source-blind reviewer has
  exactly one behavioral evidence source for the new surface, and the
  documented assurance tier cannot be established from the record.
- *Arbitration options:* (a) commission the clean-slate documenter for the
  delegation seam and re-run this comparison against the fresh record;
  (b) accept the test-only evidence with the tier named as "test-evidenced,
  record-pending" and a follow-up task to produce the record; (c) block
  until the record exists.

### M-3 (MEDIUM, mismatched) — the security-audit evidence is not bound to
the delegation seam, and the documented assurance tier is not stated in the
contract.

- *Locator:* `sav/SECURITY_AUDIT.md` ("Security Audit: Decision
  Authorization and Recovery", dated 2025-12-18); `sav/TODOS.yaml`
  `sa-03-implement-code` (`completed_at: "2026-11-21T07:39:47Z"`) and
  `sa-03-security-audit` (`completed_at: "2026-11-21T08:09:50Z"`);
  `sav/CONTRACT.md` (whole document); `sav/DESIGN.md` §`Security and resource
  design`.
- *Intended:* the chain's security audit independently reviewed "runner
  resolution, probe trust, protocol binding, environment and credential
  handling, timeout and output bounds, fail-closed refusal paths, and
  cleanup for every delegation outcome" (task description), and the
  documented assurance tier for the delegation surface is stated somewhere a
  compliance reviewer can confirm it.
- *Observed:* the only audit report present in the component is dated
  2025-12-18 — roughly a year before `sa-03-implement-code` completed — and
  it covers decision authorization and recovery generally; it contains no
  finding on runner resolution, probe trust, or protocol binding, and the
  words "galla", "runner", "delegation", and "Bubblewrap" appear nowhere in
  it. No component-level delegation-specific audit artifact exists in
  `sav/`. Separately, neither `CONTRACT.md` nor `DESIGN.md` names an
  assurance tier for the delegation surface; the only tier-like language is
  the registry capability lists (`advertised`, `tested`, `policy_enabled`)
  and the design's "Capability states distinguish advertised,
  conformance-tested, and policy-enabled behavior" (describing registry
  capability states, not the delegation surface). The task's expected
  outcome ("The review either confirms the documented assurance tier or
  blocks release with precise discrepancies") therefore cannot be satisfied
  by confirmation: no documented tier exists to confirm.
- *Why it matters:* the chain asserts an independent audit for the delegation
  surface, but the retained evidence is a pre-existing, differently-scoped
  report; and a compliance review that must "confirm the documented
  assurance tier" has no tier documented. This is provenance and intent
  drift, not a behavioral defect.
- *Arbitration options:* (a) bind the delegation-specific audit to the seam
  (fresh report or corrected provenance in the queue/IMPL) and state the tier
  in `CONTRACT.md` or `DESIGN.md`, then re-review; (b) record a reasoned
  exception acknowledging the pre-existing audit does not cover the seam and
  pin the tier explicitly (e.g., "delegation boundary test-evidenced;
  host-side enforcement `advertised`, not `conformance-tested`"); (c) block
  promotion of any strict-mode claim on the delegation surface until both
  are present.

## Observations (LOW, retained for arbitration)

### O-1 (LOW, underspecified) — runner-identity binding is test-pinned but
not a contract guarantee.

- *Locator:* `sav/CONTRACT.md` (no runner-binding clause); `sav/DESIGN.md`
  §`Security and resource design` ("bind the installed `galla` … before
  dispatch"); `sav/tests/execution_backend_delegation.rs` tests 5–6.
- *Observed:* the test suite pins identity binding (refusal text "runner
  binding mismatch"; `effects() == 0` on mismatch) and the design requires
  protected hosts to bind the installed runner before dispatch, but
  `CONTRACT.md` gives the surface no typed guarantee, and the refusal in the
  test uses `Error::Host` — a variant the contract does not name — rather
  than `Error::ToolPrerequisite`, which the contract ties to "an approved
  local resource unavailable before dispatch."
- *Arbitration options:* (a) add a short contract clause naming the binding
  guarantee and the refusal type; (b) accept the design-level obligation with
  the test as the retention point; (c) leave as-is and note the design
  already fails closed.

### O-2 (LOW, evidence limit) — "probe before dispatch" is evidenced by an
executor-internal count, not a runtime-enforced ordering.

- *Locator:* `sav/tests/execution_backend_delegation.rs` test
  `probes_runner_and_backend_before_dispatch`; `sav/CONTRACT.md` "Shared
  native runtime API (ADR 0015)" (no probe-ordering clause).
- *Observed:* the probe counter lives inside the recording executor, and the
  test asserts `probe_calls >= 1` after the run; it demonstrates the
  executor modeled the probe before causing an effect, and that the runtime
  does not cause effects without invoking the executor, but no contract
  clause or runtime-level assertion pins that the *runtime* probes the runner
  and backend before it dispatches. The design places the obligation on the
  protected host ("must bind … before dispatch"), which is consistent but
  leaves the ordering as a host-side design property.
- *Arbitration options:* (a) name the probe-before-dispatch obligation in the
  contract for the protected-host mode; (b) accept it as a host obligation
  with the design text as the retention point.

### O-3 (LOW, precedent) — the injected-executor boundary remains the
realization; redaction ownership is unresolved from the shared-runtime chain.

- *Locator:* `sav/tests/compliance_review_shared_runtime.md` (M-1
  "Redacted runtime events: no demonstrated owner", high; N-5 "Typed
  broker/selected-broker realization not demonstrated", low); `sav/
  REQUIREMENTS.md` `AR-REQ-NATIVE-RUNTIME`; `sav/CONTRACT.md` §`Security and
  authority`.
- *Observed:* the retained shared-runtime comparison classified the redacted
  runtime-events obligation as mismatched (M-1) because full `ToolOutcome`
  data reaches host recorders and no runtime redaction point is recorded, and
  retained the typed-broker/selected-backend realization as an unresolved
  broader target (N-5). The delegation suite does not change either: it
  corroborates the executor boundary but adds no redaction or broker
  realization evidence.
- *Arbitration options:* (a) resolve the shared-runtime M-1/N-5 together
  with this surface's arbitration; (b) accept the injected-executor boundary
  as the current realization with the broader targets retained as separate
  work.

## Discrepancy summary

| ID | Clause / finding | Status | Severity |
|----|------------------|--------|----------|
| C-1 | Every effect delegated to host executor; no host fallback | compliant | none |
| C-2 | Fail-closed refusal; surfaced; recorded not executed | compliant | none |
| C-3 | No unconstrained host substitution on refusal | compliant | none |
| C-4 | Bound, probed effect executed once; `Completed` | compliant | none |
| C-5 | Execution-backend capability reporting | mismatched (M-1) | high |
| C-6 | Runner-identity binding of delegated effects | compliant (O-1) | low |
| C-7 | Chain change-and-compliance process | compliant | none |
| M-1 | Capability reporting: no contract clause, no test evidence | mismatched | high |
| M-2 | No `IMPL.md` record for the delegation seam | mismatched | high |
| M-3 | Audit evidence not bound to the seam; tier not documented | mismatched | medium |
| O-1 | Runner-identity binding not a contract guarantee | underspecified | low |
| O-2 | Probe-before-dispatch ordering is host-side, not contract-pinned | evidence limit | low |
| O-3 | Redaction / typed-broker realization unresolved (precedent) | evidence limit | low |

**Total hard mismatches:** 3 (M-1 high, M-2 high, M-3 medium)
**Observations retained for arbitration:** 3 (all LOW)

## Conclusion

The observed delegation *behavior* — every effect delegated to the
host-injected executor, no implicit host fallback, fail-closed refusal when
the runner or its backend is unavailable, refusal surfaced and the effect
recorded as not executed, identity-bound acceptance — corresponds to the
execution-backend contract and design on the permitted static evidence
(C-1 through C-4, C-6).

The review **cannot confirm the documented assurance tier**: no tier is
documented for the delegation surface in `CONTRACT.md` or `DESIGN.md`
(M-3), the expected capability reporting is neither specified in the contract
nor evidenced in the bounded tests (M-1), and the delegation seam has no
clean-slate implementation record for this source-blind comparison to weigh
against the test evidence (M-2). The retained security-audit report predates
the seam and does not cover it (M-3).

This record therefore **blocks release** of the `sa-03` execution-backend
delegation as fully compliant and retains the precise discrepancies above for
human arbitration. It is not self-certified, establishes no clause-level
certification beyond the record above, and does not constitute acceptance or
sign-off. No requirement, contract, design, implementation record, or
implementation was altered by this review.
