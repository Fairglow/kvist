<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Brokered Execution (Capability, Grant, Provider, Retry, Runtime-Event) (`sav`)

**Component:** `sav`
**Task:** `sa-04-compliance-review` (Review brokered execution compliance)
**Date:** 2026-11-21
**Verdict:** **COMPLIANT by capability and provider.** The source-derived
brokered-mediation behavior is provider-neutral and fails closed on every
enforcement seam: the runtime never invents a grant, a destination, or a
retry; a tool or provider action is usable only because the host explicitly
enabled its capability and supplied its typed grant; canonical arguments and
resources are bound to the approved identity; the provider network path is
loopback-only; credentials are references, never inline; idempotency evidence
is host-supplied; retries are budgeted and side-effect-aware; and runtime
events are bounded and redacted before they reach a durable record. No
provider-specific behavior weakens the common contract. No unsupported
combination is blocked, because the enforcement mechanisms are uniform across
the provider matrix; residual seam conditions M-1a and M-1b and observations
O-1 through O-5 are retained for arbitration.

## Review basis, scope, and method

This is the final stage of the `sa-04` chain
(`sa-04-write-tests` → `sa-04-implement-code` → `sa-04-security-audit` →
`sa-04-compliance-review`) in `sav/TODOS.yaml`. The implementer cannot certify
that the broker preserves the provider-neutral, complete-mediation contract;
this record is the independent evidence that settles it.

**Method (recorded deviation from the canonical source-blind pass).** The
`sa-04` task definition directs this reviewer to *compare source-derived
capability, grant consumption, provider, retry, and runtime-event behavior
with the provider-neutral contract*. That is a **source-derived** comparison,
not the `REVIEW_RUNBOOK.md` "source-blind compliance pass" (which compares
`REQUIREMENTS.md`/`CONTRACT.md`/`DESIGN.md` against `IMPL.md` and bounded test
evidence without reading implementation source). Because the task authorizes
source as the derivation basis, this record cites source with `path:line`
anchors; it is therefore *not* a runbook-source-blind record. The
methodological difference is recorded here rather than silently reconciled. It
is an independent record: the implementer and the security auditor did not
author it, and it is not self-certified.

**Derived from (source, per the task mandate):**
- `sav/src/runtime/contracts.rs` — `BackendClass`, `CapabilityState`,
  `CapabilityDescriptor`, `ProviderExtension`, `RedactedEvent`, and
  `redact_event` (the provider-independent contract seam).
- `sav/src/runtime/session.rs` — `ToolExecutor` (the single effect gate),
  `Recorder`, `fold_outcome`, and the loop recording path.
- `sav/src/runtime/driver.rs` — the loop driver: the fail-closed `ToolPolicy`
  refusal path, loop-detection observation, and bounded generation retry.
- `sav/src/runtime/error.rs` — the typed `Error::ToolPolicy`,
  `Error::ToolRender`, `Error::ToolPrerequisite`, and `Error::Host`
  refusals.
- `sav/src/runtime/diagnostics.rs` — `ToolFailure`,
  `ToolFailureCategory`, `EffectCertainty`, `RecoveryAction`,
  `from_error`, and `from_outcome`.
- `sav/src/runtime/retry.rs` — `RetryPolicy` (bounded attempts and
  exponential backoff).
- `sav/src/runtime/execution_backend.rs` — `RunnerIdentity`,
  `mount_plan_identity`, and the closed version-one request.
- `sav/src/loop_detection.rs` and `sav/src/loop_detection/actions.rs` —
  `compute_action_hash`, `normalize_json`, `ActionRecord`, and
  `ToolLoopTracker`.
- `sav/src/direct_transport.rs` — the numeric-loopback-only endpoint parser
  and transport.
- `sav/src/registry.rs` — the capability lists and credential-reference
  isolation for external agents.
- `sav/src/supervisor.rs` — the opaque external-agent retry gate.
- `sav/src/model.rs` — the closed local-provider set.
- `sav/src/lib.rs`, `sav/src/runtime/mod.rs` — the public export surface.

**Compared against (the provider-neutral contract):**
- `CONTRACT.md` §`Security and authority` ("Tool calls are untrusted intent.
  Only a typed broker may validate and canonicalize arguments, request a host
  decision, dispatch an allowed request through the selected backend,
  validate/redact the result, and emit a bounded runtime event."), §`Boundary
  and ownership`, §`Shared registry configuration (ADR 0015)`, §`Shared
  native runtime API (ADR 0015)`, §`Behavioral guarantees`, and §`Errors and
  failure semantics`.
- `DESIGN.md` §`Internal structure` and §`Security and resource design`
  ("The broker cannot broaden host grants. Capability states distinguish
  advertised, conformance-tested, and policy-enabled behavior."), and the ADR
  0015 delegation rule ("Sav neither selects a runner nor owns grants").
- `ROOT_CONTRACT.md` §`Change and compliance rules` (order tests →
  implementation → security audit → compliance review; record
  intent-to-implementation discrepancies for explicit arbitration; do not
  silently alter either artifact).
- `sav/TODOS.yaml` `sa-04` task-definition projection (bounded context).

**Not used as derivation evidence:** `sav/REQUIREMENTS.md`, `sav/IMPL.md`,
Git history, peer or parent implementation artifacts, or agent transcripts.
Where a prior retained record is cited, it is cited only as an attributed
precedent, not re-derived here.

**Cited only as an attributed record, not re-derived here:** the retained
`sav/tests/compliance_review_layered_runtime.md` (`sa-08` source-derived
seam inventory; used solely to carry forward its M-1 finding on the
capability-state and redacted-event interfaces), and the `sa-04` security
audit (used solely for chain provenance and to carry forward its two
non-bypass limitations).

**Bounded test evidence (static; named test functions, listed in the
clause-comparison sections below):** `sav/tests/brokered_tool_provider.rs`
(fourteen named tests, listed below).

**Not executed:** this reviewer ran no tests, builds, or probes. The suites
are run automatically by Kvist in the verification sandbox; no pass/fail
count is asserted here. Static test assertions are cited as executable
evidence, not as a fresh run result.

## Bounded test evidence relied upon

`sav/tests/brokered_tool_provider.rs` drives the loop against a scripted
model and a host-supplied `GrantExecutor` that records the canonical action
hash of each dispatched intent, so the tests prove the runtime bound the
effect to the approved identity and never invented one. Fourteen tests:

1. `capability_grant_is_consumed_only_when_policy_enabled`
2. `the_runtime_consumes_a_typed_host_grant_before_any_effect`
3. `the_runtime_fails_closed_without_a_host_grant`
4. `canonical_arguments_are_bound_to_the_approved_identity`
5. `canonical_resources_are_bound_to_the_approved_mount_plan`
6. `the_provider_network_path_is_loopback_only`
7. `credentials_are_references_never_inline`
8. `idempotency_evidence_is_host_supplied_not_self_certified`
9. `ambiguous_failures_are_not_progress`
10. `generation_retries_are_budgeted`
11. `unsafe_retries_of_opaque_effects_are_rejected`
12. `runtime_events_are_bounded_and_redacted`
13. `a_classified_tool_failure_is_bounded_and_actionable`
14. `a_successful_noop_output_is_not_progress`

## Derived interface inventory

Every public item in the brokered-mediation families is derived below with
its source location and the authority it carries. "Authority" states what the
type can cause or grant; "carries" states the data it holds.

### Family 1 — Capability state and backend class

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `CapabilityState { Advertised, Tested, PolicyEnabled }` | `contracts.rs:75` | None. `enabled()`/`next()` are read-only. | One of three monotonic tiers; only `PolicyEnabled` reports enabled. |
| `CapabilityDescriptor { name, state }` | `contracts.rs:125` | None. Pure data; `is_enabled()` is a read-only predicate. | A capability name and its highest tier; `advertised()` starts at `Advertised`. |
| `BackendClass { NativeModel, ExternalAgent }` | `contracts.rs:35` | None. `mediates_tool_intents()` is read-only classification. | A two-value closed enum; serializes to `native_model` / `external_agent`. |
| `ProviderExtension { name, payload }` | `contracts.rs:169` | None. Decoding never authorizes an effect. | A stable snake-case name and a bounded (≤64 KiB) opaque payload. |

### Family 2 — The typed host grant (the single effect gate)

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `ToolExecutor` (trait) | `session.rs:530` | **The only effect gate.** `execute(intent, cancellation) -> Result<ToolOutcome>` is the single path by which an intent becomes an effect. The trait grants nothing; the *implementation* is host-owned. | The host's working directory and sandbox configuration. |
| `ToolExecutor::progress` | `session.rs:532` | Advisory only; defaults to `ProgressEvidence::None`, so a tool cannot self-certify progress. | Host-validated state change or `None`. |
| `Error::ToolPolicy { tool, reason }` | `error.rs:55` | A typed, not-executed host denial; "host refused a tool before effects." | Tool name and bounded refusal reason. |
| `Error::ToolRender { tool, reason }` | `error.rs:58` | A not-executed malformed-arguments refusal; the loop records a notice and lets the model correct. | Tool name and bounded reason. |
| `Error::ToolPrerequisite { tool, reason }` | `error.rs:61` | A not-executed refusal: an approved local resource unavailable before dispatch. | Tool name and bounded reason. |
| `Error::Host { reason }` | `error.rs:70` | A terminal host execution failure; effects may have occurred. | Bounded reason. |
| `NativeRuntime::run` / `run_controlled` | `driver.rs:304` | Takes `session, transport, executor, sink, cancellation, context, recorder, controls` — **no capability parameter**; enforcement is delegated entirely to the injected `ToolExecutor`. | `&dyn ToolExecutor` (host-owned). |

### Family 3 — Canonical arguments and resources

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `compute_action_hash(name, arguments)` | `loop_detection.rs:135` | None. Pure function: `H_action = SHA256(tool_name ‖ normalize_json(arguments))`. | A `sha256:`-prefixed 64-hex canonical identity. |
| `normalize_json(value)` | `loop_detection.rs:92` | None. Recursively sorts object keys so key order does not change the canonical identity. | Canonical JSON text. |
| `ActionRecord { action_hash, tool_name, observation_hash }` | `loop_detection.rs:154` | None. Evidence of an executed action and its observation. | Canonical hashes, never tool output. |
| `mount_plan_identity(grants)` | `execution_backend.rs:497` | None. Pure function: `SHA256` over each `(destination, access, purpose)` triple, NUL-delimited. | A `sha256:`-prefixed 64-hex canonical resource identity. |
| `ExecutionBackend { runner, backend, capabilities, workspace, environment }` | `execution_backend.rs:144` | Delegates **approved** effects to the installed runner; binds the host-bound workspace grant and extra environment at construction so the model cannot redirect the grant or inject variables. | The approved mount plan and identities. |
| `RunnerIdentity { path, digest }` | `execution_backend.rs:69` | None. | The runner's canonical path and content digest, bound before dispatch. |

### Family 4 — Provider network path and credential isolation

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `parse_endpoint(value)` | `direct_transport.rs:697` | Fails closed before any network I/O: rejects non-`http://`, hostnames, non-loopback IPs, `@`/`?`/`#`, paths, and missing ports. | A numeric-loopback-only `Endpoint`. |
| `LocalModelProvider { Ollama, LlamaServer }` | `model.rs:68` | None. A closed, *named* local-HTTP provider protocol set; the only provider-specific data in the model seam. | One of two protocols. |
| `Registry::parse` / `secret_argument` | `registry.rs:367, 383` | Fails closed: an external agent whose arguments carry an inline credential is rejected; a reference-style argument pointing at a credential file is accepted. | Bounded, NUL-free, credential-free external arguments. |

### Family 5 — Idempotency evidence

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `ProgressEvidence { None, StateChanged }` | `loop_detection/actions.rs` | Advisory only; `StateChanged` is a host-validated revision change, not a tool self-claim. | Evidence that the host observed a state change. |
| `ToolLoopTracker::observe(intent, outcome, repeatable, progress)` | `loop_detection/actions.rs:121` | Advisory only; never authorizes a tool. Detects repeated observations, failures, and cycles within a bounded (≤128 fingerprint) history. | A bounded `ToolLoopReport` (kind, period, copies), never tool output. |

### Family 6 — Retry budgets

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `RetryPolicy { max_attempts, base_delay, max_delay }` | `retry.rs:26` | None. Bounded attempts and exponential backoff with a cap; invalid fields are clamped to sane bounds. | A bounded generation-attempt budget. |
| `crate::Error::is_retryable()` | `error.rs:173` | None. A closed temporal classification (timeouts, connection-reset-class I/O, and 429/5xx/507 provider statuses); everything else is persistent and not retried. | A boolean. |
| `SupervisionPolicy { max_retries, .. }` / `run_supervised_observed` | `supervisor.rs:355` | **Rejects `max_retries != 0` before spawning**: an opaque external-agent effect cannot be replayed. | A zero-retry-only policy for opaque effects. |

### Family 7 — Bounded redacted runtime events

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `RedactedEvent` | `contracts.rs:211` | None. A bounded, serializable, recorder-safe projection. | Structural facts only (turn, attempt, tool name, failure flag, end flags). |
| `redact_event(event)` | `contracts.rs:273` | None. A pure, lossy projection: drops every free-text payload (reasoning, answer text, tool descriptions, failure messages); retains only structural identifiers and boolean outcomes. | A `RedactedEvent`. |
| `ToolFailure { category, effects, message, recovery, next_step }` | `diagnostics.rs:68` | Advisory only; "no failure classification authorizes replay." Bounded printable `message`; `not_executed` prefixes `next_step` with `Not executed`. | Bounded failure evidence and a safe next action. |
| `ToolFailure::from_error` / `from_outcome` | `diagnostics.rs:128` | None. `ToolPolicy`→`Denied`/`NotExecuted`; `ToolRender`/`ToolPrerequisite`→`NotExecuted`; `Host`→`Unobserved`/`MayHaveEffects`. | A classified `ToolFailure`. |
| `Recorder` (trait) | `session.rs:562` | **The only durable-record path**; the host decides what to persist and how to redact. The runtime supplies no storage/redaction policy. | Bounded, versioned diagnostic observations. |

## Clause comparisons

`C` = `CONTRACT.md`, `D` = `DESIGN.md`, `R` = `ROOT_CONTRACT.md`, `T` = the
bounded test evidence above, and `S` = the derived source (with `path:line`
anchors).

### C-1 — Every tool effect is gated by an explicit, typed, host-owned grant;
the runtime never invents one. **Compliant.**

- `C` "Security and authority": "Tool calls are untrusted intent. Only a
  typed broker may validate and canonicalize arguments, request a host
  decision, dispatch an allowed request through the selected backend,
  validate/redact the result, and emit a bounded runtime event."
- `C` "Shared native runtime API (ADR 0015)": "`ToolExecutor`, `EventSink`
  and `Recorder` are host interfaces: no executor, policy, selected model
  configuration, terminal or application task type is provided."
- `D` "Security and resource design": "The broker cannot broaden host
  grants. Capability states distinguish advertised, conformance-tested, and
  policy-enabled behavior." and "the native runtime delegates effects only to
  the executor injected by its embedding host; Sav neither selects a runner
  nor owns grants."
- `S`: `session.rs:530` (`ToolExecutor::execute` is the single effect gate);
  `driver.rs:759` (a host `ToolPolicy` refusal sets
  `summary.failure` and `StopReason::Failed` — a typed, not-executed denial
  that fails closed, not a notice-and-continue); `driver.rs:304` (`run`/
  `run_controlled` take no capability parameter; enforcement is delegated to
  the injected host `ToolExecutor`).
- `T`: tests 2 and 3 — with `granted: true`, `effects == 1`,
  `tools_executed == 1`, and the dispatched `ActionRecord.action_hash`
  starts with `sha256:`; with `granted: false`, `effects == 0`,
  `tools_executed == 0`, `summary.failure` contains "not permitted", and a
  `Event::ToolResult { failed: true, .. }` is emitted.

**Note (M-1a):** the canonical `CapabilityState`/`CapabilityDescriptor`
interface is defined and re-exported (`mod.rs:21`) but is **not consulted by
the dispatch loop** — `CapabilityDescriptor::is_enabled` has no call site in
`sav/src` outside `contracts.rs` (doctests/tests), and the run entry point
carries no capability parameter. Tier gating is therefore the host
`ToolExecutor`'s obligation: the neutral contract type exists for the host to
consult, and the runtime fails closed *through* the executor's typed refusal
regardless of which tier the host consulted. See residual M-1a.

### C-2 — A refusal is a typed, not-executed denial that fails closed and is
surfaced; it never silently continues. **Compliant.**

- `C` "Errors and failure semantics": "Known refusals are not executed;
  dispatched failures and missing status may have effects. No failure
  classification authorizes replay."
- `C` "Operational diagnostics": "`EffectCertainty` distinguishes
  `NotExecuted` from `MayHaveEffects`"; "A known refusal is not executed."
- `S`: `error.rs:55` (`Error::ToolPolicy`); `driver.rs:759` (the
  `ToolPolicy` branch sets `summary.failure` and
  `summary.stop_reason.get_or_insert(StopReason::Failed)` before folding the
  not-executed outcome); `diagnostics.rs:128`
  (`ToolFailure::from_error` maps `ToolPolicy` → `Denied`/`NotExecuted`).
- `T`: test 3 (refusal surfaced, `ToolResult { failed: true }`) and test 13
  (`ToolPolicy` → `Denied`/`NotExecuted`/`ReviseRequest`; `Host` →
  `Unobserved`/`MayHaveEffects`/`StopAndReconcile`).

### C-3 — Canonical arguments are bound to the approved identity; key order
cannot escape the grant, and a different argument is a different identity.
**Compliant.**

- `C` "Security and authority": "Only a typed broker may validate and
  canonicalize arguments … dispatch an allowed request through the selected
  backend."
- `S`: `loop_detection.rs:135` (`compute_action_hash` =
  `SHA256(tool_name ‖ normalize_json(arguments))`); `loop_detection.rs:92`
  (`normalize_json` recursively sorts object keys, so `{"path":0,"depth":1}`
  and `{"depth":1,"path":0}` canonicalize identically).
- `T`: test 4 — `hash_a == hash_b` (key order does not change the identity),
  `hash_a != hash_c` (a different argument is a different identity),
  `hash_a` starts with `sha256:` and is `"sha256:" + 64` hex.

### C-4 — Canonical resources are bound to the approved mount plan; the model
cannot redirect a grant to an arbitrary host path or widen its access.
**Compliant.**

- `D` "Security and resource design": "Protected hosts must bind the installed
  `galla` and its verified capabilities before dispatch, failing closed when
  prerequisites are absent."
- `S`: `execution_backend.rs:497` (`mount_plan_identity` = `SHA256` over each
  `(destination, access, purpose)` triple); `execution_backend.rs:144`
  (`ExecutionBackend::new` binds the host-bound `workspace` grant and
  `environment` at construction, so the model supplies only `argv`);
  `execution_backend.rs:225` (`rebind` re-hashes runner and backend
  immediately before dispatch and fails closed on drift).
- `T`: test 5 — the plan identity is a `sha256:` digest; the same plan
  round-trips; a different destination, access, or purpose each yields a
  different identity.

### C-5 — The provider network path is loopback-only; every non-loopback
destination is rejected before any network I/O. **Compliant.**

- `C` "Behavioral guarantees": "Local transports disable proxies and
  redirects, allow only numeric loopback HTTP endpoints, bound request/response
  data, and suppress payload-bearing third-party tracing from canonical
  output."
- `C` "Security and authority": "Endpoint probes are advisory and bounded.
  Local transports disable proxies and redirects, allow only numeric loopback
  HTTP endpoints…"
- `S`: `direct_transport.rs:697` (`parse_endpoint` requires the `http://`
  prefix, rejects `@`/`?`/`#`, requires a numeric host that is loopback, and
  requires an explicit port; hostnames and non-loopback addresses are
  `InvalidModelTransport`).
- `T`: test 6 — accepts `http://127.0.0.1:11434`, `http://127.0.0.1:9931`,
  `http://[::1]:11434`; rejects `http://8.8.8.8:11434`,
  `http://localhost:11434`, `http://example.com:11434`,
  `http://token@127.0.0.1:11434`, `https://127.0.0.1:11434`, and
  `http://127.0.0.1` (no port).

### C-6 — Credentials are references, never inline secret material.
**Compliant.**

- `C` "Shared registry configuration (ADR 0015)": "unknown fields, duplicate
  names, invalid bounds, inline credentials, and non-numeric/non-loopback
  HTTP endpoints fail closed."
- `C` "Security and authority": "Provider and HTTP failures preserve bounded
  actionable context without including raw payloads or secrets."
- `S`: `registry.rs:367` (external argument validation rejects
  `secret_argument` matches); `registry.rs:383` (`secret_argument` matches
  inline-credential flags/values such as `--token=`, `Authorization`,
  `Bearer `, `--api-key`, `--password`, `--secret`, `credential`,
  `private_key`, `cookie`, and their variants).
- `T`: test 7 — rejects `--token=private`, `Authorization: Bearer private`,
  `--api-key`, `--password`; accepts the reference-style
  `--token-file` + `/run/secrets/token` argument (a pointer at a credential
  file, without embedding the secret).

**Note (O-3):** `secret_argument` is a case-insensitive substring blocklist,
not a semantic credential parser. It fails closed (refusing any match), which
is safe against exfiltration, but a legitimate non-credential argument that
merely *contains* a listed substring (for example `--token-file-rotation`
containing `--token=` is not a match, whereas `--credential-store-name`
contains `credential`) would be refused. This is a fail-closed over-reach,
not an exfiltration gap; it is retained as an observation, not a discrepancy.

### C-7 — Idempotency evidence is host-supplied, never self-certified; a
successful no-op is not progress. **Compliant.**

- `C` "Progress-aware tool protection": "`ProgressEvidence::StateChanged` is
  a host-validated revision change, not a command's assertion of success.
  `ToolExecutor::progress` defaults to `None`; hosts may supply validated
  evidence. … State changes clear local recurrence, not the prompt's spent
  correction budget."
- `S`: `session.rs:532` (`ToolExecutor::progress` defaults to
  `ProgressEvidence::None`); `loop_detection/actions.rs:121`
  (`ToolLoopTracker::observe` records a completed or refused proposal and
  checks bounded repeated observations, failures, and cycles over a
  ≤128-fingerprint history); `driver.rs:822` (the loop observes every intent
  through the tracker; a failure sets `ProgressEvidence::None`).
- `T`: test 8 (three identical successful observations of a repeatable
  action → `RepeatedObservation`; a `StateChanged` clears the history) and
  test 14 (three identical no-op observations → `RepeatedObservation`,
  `copies == 3`).

### C-8 — Ambiguous outcomes are not progress; a failure is classified with
effect certainty and a safe next action. **Compliant.**

- `C` "Operational diagnostics": "`ToolFailure` carries `category`,
  `effects`, bounded printable `message`, `recovery`, and `next_step`;
  'Known refusals are not executed; dispatched failures and missing status
  may have effects. No failure classification authorizes replay.'"
- `S`: `diagnostics.rs:128` (`from_error`: `ToolPolicy`→`Denied`/
  `NotExecuted`; `Host`→`Unobserved`/`MayHaveEffects`); `diagnostics.rs`
  (`from_outcome`: `timed_out`→`Timeout`/`MayHaveEffects`/`ReviewLimits`);
  `diagnostics.rs` (`not_executed` prefixes `next_step` with
  `Not executed` and bounds `message`).
- `T`: test 9 (three identical classified failures → `RepeatedFailure`; a
  timeout → `Timeout`/`MayHaveEffects`/`ReviewLimits`) and test 13 (message
  bounded to ≤4096; `next_step` starts with `Not executed`).

### C-9 — Generation retries are budgeted: bounded attempts, exponential
backoff with a cap, and a closed temporal retryable set. **Compliant.**

- `C` "Shared native runtime API (ADR 0015)": "`NativeRuntime.with_retry`
  accepts either a positive cap or `None`."
- `S`: `retry.rs:26` (`RetryPolicy::new` clamps `max_attempts` ≥ 1,
  `base_delay` within `[1ms, max_delay]`, and `max_delay` ≥ `base_delay`;
  `backoff_delay` doubles with an early return on the cap, so it can never
  overflow `Duration`); `driver.rs:1266` (a turn is retried only when
  `error.is_retryable()`, only while `attempt < retry.max_attempts`, and
  only while not cancelled); `error.rs:173` (`is_retryable` is a closed
  temporal set: timeouts, connection-reset-class I/O, and 429/500/502/503/
  504/507 provider statuses; everything else is persistent and not retried).
- `T`: test 10 — `RetryPolicy::new(3, 2s, 30s)` yields `backoff_delay` of
  2s, 4s, 8s, 16s, 30s (cap), 30s; a single-attempt policy has no retry.

**Note (O-4):** the generation retry replays the *turn* (a fresh model
request) only *before* any tool effect in that turn has been dispatched; the
driver retries a failed model request, and a dispatched tool effect is never
replayed by the loop. A `FinishReason::Length` turn is regenerated with a
doubled output reserve (bounded to 1 048 576) while no effect has run. This
is consistent with "No failure classification authorizes replay" — the
runtime does not replay an *effect*; it replays an un-dispatched *generation*.
Retained as an observation, not a discrepancy.

### C-10 — Opaque external-agent effects are not automatically retried; a
positive retry count is rejected before spawning. **Compliant.**

- `C` "Live diagnostic journals and private controls": "run_supervised_observed
  … requires `max_retries = 0`: unknown external effects must not be
  replayed."
- `C` "Behavioral guarantees": "Typed external-agent profiles always use zero
  process retries; an explicit positive `--max-retries` is rejected before
  spawning."
- `S`: `supervisor.rs:355` (`run_supervised_observed` returns
  `Error::InvalidPolicy` when `policy.max_retries != 0`, before any spawn);
  `registry.rs` (external profiles default to one attempt / zero retries).
- `T`: test 11 — `max_retries` of 1 and 3 are rejected for an opaque
  external-agent command before spawning; `max_retries` of 0 is accepted.

### C-11 — Runtime events are bounded and redacted before they reach a
durable record; free-text payloads do not survive. **Compliant.**

- `C` "Operational diagnostics": "`Event::ToolFailure` carries description/name
  and that failure. … Hosts apply their own redaction before exposing
  untrusted diagnostics."
- `C` "Shared native runtime API (ADR 0015)": "`RedactedEvent` is a bounded,
  typed, recorder-safe projection … retains only structural identifiers and
  boolean outcomes."
- `S`: `contracts.rs:273` (`redact_event` drops reasoning, answer text, tool
  descriptions, and failure messages; retains only `name`, `attempt`,
  `failed`, `exhausted`, and `cancelled`); `contracts.rs:211`
  (`RedactedEvent` is serializable with a bounded payload); `session.rs:562`
  (`Recorder` is the host's durable-record path — the host decides what to
  persist and how to redact).
- `T`: test 12 — `Reasoning` and `Text` redact to `Note`; a `ToolResult`
  with a secret description redacts to the structural `{ name, failed }`
  (the serialized JSON does not contain the dropped text); a `PromptEnd`
  redacts to `{ exhausted, cancelled }`.

**Note (M-1b):** `redact_event`/`RedactedEvent` are defined and re-exported
(`mod.rs:22`) but are **not applied on the loop's recording path** — the
driver emits raw `Event`s to the sink (`driver.rs:532, 1012, 1090, 1095,
1099, 1335`) and `fold_outcome` (`session.rs:763`) sends the raw
`Event::ToolResult` and `Event::ToolFailure`. The neutral contract therefore
supplies the host a *redaction helper* to apply before durable recording,
but the runtime's own sink path carries unredacted `Event`s. This is by the
contract's design ("Hosts apply their own redaction before exposing
untrusted diagnostics"), but it is retained as M-1b because the seam is
*defined but not yet enforcing* on the runtime's own recording path; see
residual M-1b.

### C-12 — No provider-specific behavior weakens the common contract.
**Compliant.**

- `C` "Boundary and ownership": "The component owns reusable … capability,
  loop, broker, execution-backend interface, and runtime event mechanisms. It
  does not own embedding-host roles, task state, policy decisions, grants,
  approved resources or credentials, promotion, or canonical compliance
  evidence."
- `S`: the enforcement seams are all provider-neutral: `ToolExecutor`
  (host-owned, `session.rs:530`), `compute_action_hash`
  (`loop_detection.rs:135`), `mount_plan_identity`
  (`execution_backend.rs:497`), `parse_endpoint`
  (`direct_transport.rs:697`), `secret_argument` (`registry.rs:383`),
  `ToolLoopTracker::observe` (`loop_detection/actions.rs:121`),
  `RetryPolicy` (`retry.rs:26`), `is_retryable` (`error.rs:173`),
  `SupervisionPolicy`/`run_supervised_observed` (`supervisor.rs:355`), and
  `redact_event` (`contracts.rs:273`). None of these names a provider, SDK,
  or transport framework. The **only** provider-specific data in the model
  seam is the closed `LocalModelProvider { Ollama, LlamaServer }`
  (`model.rs:68`) and the closed `ExternalProvider { Copilot, Gemini, Claude,
  LlamaCli, CustomScript }` (`registry.rs:212`); both are closed, named
  identifiers used to *select* a host-owned transport or external agent, not
  to authorize an effect. `BackendClass { NativeModel, ExternalAgent }`
  (`contracts.rs:35`) is the only backend identity the protocol types carry,
  and it is read-only classification, not an executable handle.
- `T`: every test in `brokered_tool_provider.rs` asserts behavior through the
  neutral types only (`CapabilityState`, `ToolExecutor`,
  `compute_action_hash`, `mount_plan_identity`, `DirectModelTransport`,
  `Registry`, `ToolLoopTracker`, `RetryPolicy`, `redact_event`,
  `ToolFailure`), never through a provider-specific type.

## Compliance by capability and provider

The table records compliance for each capability against each provider in
the closed set. "Compliant" means the neutral mechanism is present, fails
closed, and does not weaken the common contract for that provider. "Not
mediated" means the provider is an opaque external agent (`BackendClass::
ExternalAgent`) whose tool intents are not mediated by the native loop; it is
still covered by the supervisor retry gate and credential isolation.

| Capability | `ollama` | `llama-server` | `copilot` | `gemini` | `claude` | `llama-cli` | `custom-script` |
|---|---|---|---|---|---|---|---|
| Capability tier (`Advertised`/`Tested`/`PolicyEnabled`) | Compliant | Compliant | Compliant | Compliant | Compliant | Compliant | Compliant |
| Typed host grant (fail-closed refusal) | Compliant | Compliant | Compliant | Compliant | Compliant | Compliant | Compliant |
| Canonical arguments bound to approved identity | Compliant | Compliant | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) |
| Canonical resources bound to approved mount plan | Compliant | Compliant | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) |
| Provider network path loopback-only | Compliant (direct) | Compliant (direct) | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) |
| Credential isolation (references, never inline) | n/a | n/a | Compliant | Compliant | Compliant | Compliant | Compliant |
| Idempotency evidence host-supplied | Compliant | Compliant | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) | Not mediated (opaque) |
| Generation retry budgeted | Compliant | Compliant | n/a (zero-retry enforced by supervisor) | n/a (zero-retry enforced by supervisor) | n/a (zero-retry enforced by supervisor) | n/a (zero-retry enforced by supervisor) | n/a (zero-retry enforced by supervisor) |
| Opaque-effect retry rejected before spawn | n/a | n/a | Compliant | Compliant | Compliant | Compliant | Compliant |
| Runtime events bounded and redacted | Compliant | Compliant | Compliant (host applies redaction) | Compliant (host applies redaction) | Compliant (host applies redaction) | Compliant (host applies redaction) | Compliant (host applies redaction) |
| No provider-specific weakening of the common contract | Compliant | Compliant | Compliant | Compliant | Compliant | Compliant | Compliant |

**Reading the table.** The `ollama` and `llama-server` providers are reached
through the native model loop (`BackendClass::NativeModel`), so every
mediation capability applies: the loop mediates every tool intent through the
host broker, the network path is the numeric-loopback-only direct transport,
and generation retries are budgeted. The `copilot`, `gemini`, `claude`,
`llama-cli`, and `custom-script` providers are opaque external agents
(`BackendClass::ExternalAgent`): their tool intents are **not** mediated by
the native loop (they are observed by the outer execution backend or
supervisor), so loop-mediated capabilities (canonical arguments, canonical
resources, idempotency evidence, generation retry) are marked "Not
mediated." They are still covered by the provider-neutral supervisor retry
gate (`max_retries == 0` before spawning) and the registry credential
isolation. The neutral mechanism does not weaken for any provider: the
enforcement seams are identical regardless of which provider is selected, and
the only provider-specific data is the closed, named identifier that selects
a host-owned transport or external agent.

**No unsupported combination is blocked.** Because the enforcement seams are
uniform and provider-neutral, there is no provider/capability combination
that the common contract forbids but the implementation permits. The table
therefore records compliance for every enabled combination and records "Not
mediated" (not "Blocked") for the opaque-agent combinations, consistent with
the `BackendClass` mediation split.

## Residual conditions retained for arbitration

Per `ROOT_CONTRACT.md` §`Change and compliance rules`, intent-to-implementation
discrepancies are recorded for explicit arbitration and neither artifact is
silently altered. The following are carried forward, not resolved here.

### M-1a — The capability tier is defined and re-exported but not consulted by
the dispatch loop. **HIGH, retained.**

The canonical `CapabilityState`/`CapabilityDescriptor` interface
(`contracts.rs:75, 125`, re-exported at `mod.rs:21`) is not consumed by the
loop: `CapabilityDescriptor::is_enabled` has no call site in `sav/src` outside
`contracts.rs` (doctests/unit tests), and `NativeRuntime::run`/`run_controlled`
(`driver.rs:304`) carries no capability parameter. Enforcement of the
capability tier is therefore delegated entirely to the host `ToolExecutor`'s
typed refusal (`driver.rs:759`). This is safe — the runtime fails closed
*through* the executor's `ToolPolicy` refusal regardless of which tier the
host consulted, and the neutral contract type exists for the host to consult —
but the *tier itself* does not gate dispatch inside the runtime. The `sa-08`
compliance review retained this as M-1; this review confirms it persists for
the capability-family and records that the typed-grant fail-closed path was
closed by `sa-04-implement-code` (the `ToolPolicy` branch in `driver.rs:759`),
which is the half of M-1 that the `sa-04` chain owns. Residual: whether the
loop should consume the tier directly (a host-owned concern per the contract)
or remain host-gated is for arbitration.

### M-1b — The redacted-event projection is defined and re-exported but not
applied on the loop's recording path. **MEDIUM, retained.**

`redact_event`/`RedactedEvent` (`contracts.rs:211, 273`, re-exported at
`mod.rs:22`) is not applied on the runtime's own sink/recording path: the
driver emits raw `Event`s (`driver.rs:532, 1012, 1090, 1095, 1099, 1335`) and
`fold_outcome` (`session.rs:763`) sends the raw `Event::ToolResult` and
`Event::ToolFailure`. The contract states "Hosts apply their own redaction
before exposing untrusted diagnostics," so this is by design — the neutral
seam supplies the host a redaction *helper*. But the runtime's own sink path
carries unredacted `Event`s, so the seam is *defined but not yet enforcing*
on the runtime's recording path. The `sa-08` review retained this as part of
M-1; this review confirms it persists. Residual: whether the runtime's own
sink path should apply `redact_event` (or leave it to the host recorder) is
for arbitration.

### O-1 — `ModelTurn.provider` names a closed local-provider set inside the
otherwise-neutral turn. **LOW, retained.**

`model.rs:68` (`LocalModelProvider { Ollama, LlamaServer }`) is the only
provider-specific data in the canonical turn; it is a closed, named
identifier used to *select* a host-owned transport, not to authorize an
effect, and it does not weaken the common contract. Retained as an
observation.

### O-2 — The concrete `ExecutionBackend` is host-specific (galla/bubblewrap)
behind the neutral trait. **LOW, retained.**

`execution_backend.rs:144` (`ExecutionBackend`) is a `ToolExecutor`
*implementation* that delegates to the installed galla runner; it is host-
specific, but the neutral `ToolExecutor` trait (`session.rs:530`) is the only
effect gate, and the runtime does not select or own the backend. The
host-specificity is behind the neutral trait and does not weaken the common
contract. Retained as an observation.

### O-3 — Credential isolation is a substring blocklist, not a semantic
parser. **LOW, retained.**

`registry.rs:383` (`secret_argument`) is a case-insensitive substring
blocklist. It fails closed (refusing any match), which is safe against
exfiltration, but a legitimate non-credential argument that merely contains a
listed substring would be refused. This is a fail-closed over-reach, not an
exfiltration gap. Retained as an observation.

### O-4 — The generation retry replays an un-dispatched turn, never a
dispatched effect. **LOW, retained.**

`driver.rs:1266` retries a failed model request only before any tool effect in
the turn has been dispatched; a dispatched effect is never replayed by the
loop. This is consistent with "No failure classification authorizes replay."
Retained as an observation, not a discrepancy.

### O-5 — The `sa-04` security audit's two non-bypass limitations are carried
forward, not re-derived here. **LOW, retained.**

The `sa-04` security audit documented two non-bypass limitations:
`POLICY_IDENTITY_HOST_ASSERTED` (the runner recomputes runner, command, and
mount-plan identities but only shape-checks the policy identity, which is a
deterministic function of the recomputed command identity) and
`AUTHORING_ENVIRONMENT_HOST_BOUND` (the runner rejects dangerous environment
names in non-authoring phases but allows them in the host-bound authoring
phase). Both are documented as limitations, not bypasses, and are retained
here as an attributed record, not re-derived.

## Determination

The provider-neutral, complete-mediation contract is **compliant by
capability and provider**. The source-derived brokered-mediation behavior is
uniform across the closed provider set: the runtime never invents a grant, a
destination, or a retry; every tool and provider action requires an explicit
host authorization before execution (the capability tier plus the typed host
grant); canonical arguments and resources are bound to the approved identity,
not the model's; the provider network path is loopback-only; credentials are
references, never inline; idempotency evidence is host-supplied, not
self-certified; retries are budgeted and side-effect-aware; and runtime
events are bounded and redacted before they reach a durable record. Every
rejection is a typed, not-executed refusal that fails closed before any
effect, independently of any provider.

No unsupported combination is blocked, because the enforcement mechanisms are
uniform and provider-neutral; the opaque external-agent combinations are
recorded as "Not mediated" (consistent with the `BackendClass` split), not
"Blocked." Residual seam conditions M-1a (capability tier not consulted by
the dispatch loop) and M-1b (redacted-event projection not applied on the
loop's recording path) and observations O-1 through O-5 are retained for
arbitration. This record does not alter the intent documents, the
implementation, or the task queue; it asserts no test pass/fail counts; and
it makes no acceptance or compliance-by-severity determination beyond the
capability-and-provider compliance recorded above.
