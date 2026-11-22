<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Layered Runtime Seam (`sav`)

**Component:** `sav`
**Task:** `sa-08-compliance-review` (Review layered runtime compliance)
**Date:** 2026-11-21
**Verdict:** **The one-way ownership and authority split is preserved on the
source.** The canonical runtime seam is stable enough to begin host-broker and
transport experiments, subject to arbitration of M-1 (the canonical
capability-state and redacted-event interfaces are defined but not yet consumed
by the loop) and the retained observations O-1 through O-7. No model or
adapter type authorizes or executes an effect, enables a capability, or carries
credentials; no Kvist, Rig, or provider-SDK type is imported.

## Review basis, scope, and method

This is the final stage of the `sa-08` chain
(`sa-08-write-tests` → `sa-08-implement-code` → `sa-08-security-audit` →
`sa-08-compliance-review`) in `sav/TODOS.yaml`. The implementer cannot certify
that the provider-neutral types preserve the intended one-way ownership and
authority split; this record is the independent evidence that settles it.

**Method (recorded deviation from the canonical source-blind pass).** The
`sa-08` task definition directs this reviewer to *derive the backend,
capability, model, tool-intent, host-service, and runtime-event interfaces from
source* and then compare them with the layered architecture contract. That is
a **source-derived** comparison, not the `REVIEW_RUNBOOK.md` "source-blind
compliance pass" (which compares `REQUIREMENTS.md`/`CONTRACT.md`/`DESIGN.md`
against `IMPL.md` and bounded test evidence without reading implementation
source). Because the task authorizes source as the derivation basis, this
record cites source with `path:line` anchors; it is therefore *not* a
runbook-source-blind record. The methodological difference is recorded here
rather than silently reconciled. It is an independent record: the implementer
and the security auditor did not author it, and it is not self-certified.

**Derived from (source, per the task mandate):**
- `sav/src/model.rs` — canonical model and tool-intent types.
- `sav/src/runtime/contracts.rs` — provider-independent backend, capability,
  extension, and redacted-event contract types.
- `sav/src/runtime/session.rs` — `AgentSession`, `Event`, `RunSummary`,
  `RunLimits`, and the host-service traits `ToolExecutor` and `Recorder`.
- `sav/src/runtime/execution_backend.rs` — the concrete `ToolExecutor` that
  delegates to the installed runner (plus `RunnerIdentity`,
  `BackendIdentity`, `Capabilities`, `Namespaces`, and the version-one request
  and probe wire shapes).
- `sav/src/runtime/diagnostics.rs` — `StopReason`, `BudgetResource`,
  `ToolFailure`, `ToolFailureCategory`, `EffectCertainty`, `RecoveryAction`.
- `sav/src/runtime/error.rs` — the runtime's narrow `Error` surface and the
  `From<crate::Error>` mapping.
- `sav/src/runtime/outcome.rs` — `ToolOutcome`.
- `sav/src/runtime/driver.rs` — the loop driver (the path from model output to
  effectful host services).
- `sav/src/lib.rs`, `sav/src/runtime/mod.rs` — the public export surface.

**Compared against (the layered architecture contract):**
- `docs/architecture.md` — the layered agent architecture: ownership topology,
  the seven authority layers (run coordinator, model transport, native agent
  loop, tool catalog and broker, policy and authorization, execution backend,
  evidence and checkpoints), and the delivery order that defines this seam.
- `ROOT_CONTRACT.md` §`Non-negotiable architecture` and
  §`Change and compliance rules` — the one-way dependency direction, the
  separation of authoring/acquisition/verification/promotion, and the
  change-and-compliance process for this chain.
- `sav/TODOS.yaml` `sa-08` task-definition projection (bounded context).

**Not used as derivation evidence:** `sav/REQUIREMENTS.md`, `sav/CONTRACT.md`,
`sav/DESIGN.md`, `sav/IMPL.md`, Git history, peer or parent implementation
artifacts, the `sa-08` security-audit report, or agent transcripts. Where a
prior retained record is cited, it is cited only as an attributed precedent, not
re-derived here.

**Bounded test evidence (static; named test functions and doctests, listed in
the classification sections below).**

**Not executed:** this reviewer ran no tests, builds, or probes. The suites are
run automatically by Kvist in the verification sandbox; no pass/fail count is
asserted here. Static test assertions are cited as executable evidence, not as
a fresh run result.

## Derived interface inventory

Every public item in the six named families is derived below with its source
location and the authority it carries. "Authority" states what the type can
cause or grant; "carries" states the data it holds.

### Family 1 — Backend classification

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `BackendClass { NativeModel, ExternalAgent }` | `contracts.rs:35` | None. `mediates_tool_intents()` is read-only classification. | A two-value closed enum; serializes to `native_model` / `external_agent`. |
| `BackendIdentity { kind, path, digest }` | `execution_backend.rs:78` | None. | The runner-confirmed enforcement-backend identity. |
| `Capabilities { namespaces, new_session, parent_death_signal }` | `execution_backend.rs:100` | None. `complete()` is a read-only predicate. | Verified namespace/session booleans. |
| `Namespaces { mount, network, pid, ipc, uts, user }` | `execution_backend.rs:89` | None. | Six verified namespace booleans. |

### Family 2 — Capability state

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `CapabilityState { Advertised, Tested, PolicyEnabled }` | `contracts.rs:75` | None. `enabled()`/`next()` are read-only. | One of three monotonic tiers; only `PolicyEnabled` reports enabled. |
| `CapabilityDescriptor { name, state }` | `contracts.rs:125` | None. Pure data. | A capability name and its highest tier; `advertised()` starts at `Advertised`. |

### Family 3 — Model (canonical messages, turns, transport)

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `ModelMessage { System, User, Assistant{ text, tool_intents }, ToolResult{ call_id, name, content } }` | `model.rs:87` | None. | Ordered conversation, including untrusted tool intents and host-mediated bounded results. |
| `ModelRequest { model, messages, tools, tool_choice, reasoning_effort, output_schema, max_output_tokens }` | `model.rs:135` | None. | Provider-neutral input for one turn. |
| `ModelTurn { text, reasoning, tool_intents, finish_reason, provider, model, response_id, provider_request_id, usage }` | `model.rs:197` | None. | The complete provider-neutral result for one turn. |
| `ModelStreamEvent { TextDelta, ReasoningDelta, ToolIntent }` | `model.rs:223` | None. | Ordered decode events. |
| `FinishReason { Stop, Length, ToolCalls, ContentFilter, Other(String) }` | `model.rs:171` | None. | Normalized terminal reason. |
| `ModelUsage { input_tokens, output_tokens, total_tokens }` | `model.rs:186` | None. | Normalized provider token accounting. |
| `ToolDefinition { name, description, parameters }` | `model.rs:112` | None. | A model-facing descriptor with no executable implementation. |
| `ToolChoice { None, Auto, Required }` | `model.rs:124` | None. | Requested tool-selection behavior. |
| `ReasoningEffort { None, Minimal, Low, Medium, High, Xhigh, Max }` | `model.rs:14` | None. | Requested reasoning effort. |
| `LocalModelProvider { Ollama, LlamaServer }` | `model.rs:68` | None. | A closed, *named* local-HTTP provider protocol. |
| `ModelTransport` (trait) | `model.rs:261` | Grants nothing. A host-supplied transport performs one turn and returns a `ModelTurn`; it cannot authorize or execute a tool. | `complete`, `stream`, `stream_with_deadline`, `deadline`. |
| `AgentSession` | `session.rs:293` | None. Owns the ordered conversation and builds turns; `apply_assistant` returns the intents the *caller* must execute. | `ModelIdentity`, effort, system prompt, messages, tool definitions, answer. |
| `ModelIdentity { id, model }` | `session.rs:32` | None. "Contains no policy or credentials" (doc). | Host-facing selector and provider-facing model. |

### Family 4 — Tool intent

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `ToolIntent { id, provider_id, name, arguments }` | `model.rs:157` | None. It is an untrusted proposal; it carries no grant, credential, or execution handle. | Turn-local call identity, optional provider call identity, tool name, parsed JSON-object arguments. |
| `ToolFailure { category, effects, message, recovery, next_step }` | `diagnostics.rs:68` | None. Advisory only; "no failure classification authorizes replay." | Bounded printable failure evidence and a safe next action. |
| `ToolFailureCategory { InvalidRequest, Denied, Prerequisite, Timeout, OutputLimit, Cancelled, ProcessExit, Unobserved, Stalled }` | `diagnostics.rs:34` | None. | A stable failure domain. |
| `EffectCertainty { NotExecuted, MayHaveEffects }` | `diagnostics.rs:49` | None. | What failure evidence establishes about possible effects. |
| `RecoveryAction { ReviseRequest, InspectState, FixPrerequisite, ReduceOutput, ReviewLimits, StopAndReconcile }` | `diagnostics.rs:57` | None. "Never permission to retry or broaden authority." | A suggested recovery. |

### Family 5 — Host-service traits (and the concrete execution backend)

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `ToolExecutor` (trait) | `session.rs:530` | **The only effect gate.** `execute(intent, cancellation) -> Result<ToolOutcome>` is the single path by which an intent becomes an effect; `progress`, `describe`, `repeatable`, `repeated_observation_notice` are advisory. The trait itself grants nothing — the *implementation* is host-owned. | The executor owns the working directory and sandbox configuration. |
| `Recorder` (trait) | `session.rs:562` | **The only durable-record path.** Fallible `session_start`, `request`, `turn_start`, `turn_finish`, `tool_dispatch`, `tool_result`, `session_finish`, plus optional `tool_failure`, `model_event`, `control`, `notice`, `on_prompt`. A required-recorder failure stops the loop before subsequent effects. The runtime supplies no storage/redaction policy. | The host decides what to persist and how to redact. |
| `EventSink` (trait) | `session.rs:287` | Presentation only; a closed sink stops the loop. | `send(Event) -> Result<()>`. |
| `ExecutionBackend` (struct) | `execution_backend.rs:144` | Delegates **approved** effects to the installed runner. It is a `ToolExecutor` *implementation*, not a bypass: `new` fails closed on a missing/unreadable runner (`RunnerUnavailable`) or an incomplete backend (`BackendUnavailable`), `execute` re-binds runner and backend digests before dispatch, and no unconstrained host execution is substituted. It never constructs isolation primitives. | Bound `RunnerIdentity`, `BackendIdentity`, `Capabilities`, host-bound workspace and environment. |
| `RunnerIdentity { canonical_path, digest }` | `execution_backend.rs:69` | None. | The installed runner's canonical path and content digest. |
| `ToolOutcome { exited, status, stdout, stderr, timed_out, output_limit_exceeded, cancelled }` | `outcome.rs:4` | None. | Bounded execution-result data; `rejected()`/`rejected_with` produce a faithful no-execution record without inventing sandbox results. |

### Family 6 — Runtime events (redacted) and operational diagnostics

| Item | Location | Authority | Carries |
|------|----------|-----------|---------|
| `Event` (untrusted, host-bound) | `session.rs:204` | Presentation/record only. Carries untrusted model text, reasoning, tool descriptions, and live accounting. | `Control`, `TurnStart`, `AttemptStart`, `Reasoning`, `Text`, `ToolCall`, `ToolResult`, `ToolFailure`, `Finished`, `PromptEnd{ exhausted, cancelled, reason }`, `Failed`, `Note`, `Progress`. |
| `RedactedEvent { TurnStart, AttemptStart, ToolCall, ToolResult, Finished, PromptEnd, Failed, Note }` | `contracts.rs:211` | None. A pure, lossy projection safe to hand to a host recorder. | Structural facts only: which turn, which tool, whether it failed, how the prompt ended. All free text is dropped. |
| `redact_event(&Event) -> RedactedEvent` | `contracts.rs:273` | None. | Projects an event to its redacted form; `Control`/`Reasoning`/`Text`/`ToolFailure`/`Progress` collapse to `Note`. |
| `ProviderExtension { name, payload }` | `contracts.rs:169` | None. "Decoding one never authorizes an effect." | A typed, bounded (≤ 64 KiB) opaque provider payload; `bounded` rejects unstable names and oversized payloads. |
| `StopReason { Completed, NoWork, Cancelled, Limit(BudgetResource), Stalled, Failed, GuidanceRequired }` | `diagnostics.rs:20` | None. Records the observed terminal cause; never grants authority. | The frozen stop cause; `Limit` carries a `BudgetResource`. |
| `BudgetResource { WallTime, ModelTokens, Turns, ModelAttempts, Context }` | `diagnostics.rs:9` | None. | The explicit allowance that stopped work. |
| `RunSummary { stop_reason, answer, turns, tools_executed, cancelled, exhausted, budget_exhausted, failure }` | `session.rs:617` | None. `success()`/`disposition()` are read-only dispositions. | The loop's terminal state, including the frozen `Option<StopReason>`. |
| `RunLimits { wall_time, max_tokens, response_reserve }` | `session.rs:666` | None. `validate()` rejects invalid limits before any provider or watcher starts. | Shared whole-prompt resource limits. |
| `runtime::Error` (narrow) | `error.rs:5` | None. Typed mechanism failures only; a `From<crate::Error>` maps the crate's `ModelTransportCancelled` → `Cancelled`, `RepetitiveGeneration` → `RepetitiveGeneration`, and everything else → `ModelTransport`. | `Cancelled`, `Steering`, `RepetitiveGeneration`, `Config`, `ModelTransport`, `ContextBudget`, `InvalidModelTurn`, `RunBudget`, `Recording`, `ToolPolicy`, `ToolRender`, `ToolPrerequisite`, `RunnerUnavailable`, `BackendUnavailable`, `Host`, `ChannelClosed`, `Control`, `Io`. |

## Authority-boundary classification

These classify the seam against the ownership topology and the one-way
dependency direction.

### A-1 — One-way dependency direction; no host type imported by the
standalone crate. Compliant.

`docs/architecture.md` "Ownership topology": "The dependency direction is one
way… `sav` never imports Kvist types. Third-party framework types remain behind
private adapters, so neither side depends on Rig as a public contract."
`ROOT_CONTRACT.md` §`Non-negotiable architecture`.

Observed: no `use maerg`, no `use galla`, and no third-party agent/transport
framework type is imported in `sav/src` (verified across the module tree). The
crate depends only on neutral libraries (`serde`, `serde_json`, `thiserror`,
`sha2`, `hex`, `nix`, `tracing`, `tempfile`, `toml_edit`). The concrete
`ExecutionBackend` reaches the runner only through a versioned stdin protocol
(`kvist-sandbox-probe-v1`, `kvist-sandbox-request-v1`), not a type import. The
public surface (`lib.rs`, `runtime/mod.rs`) exposes no host or provider type.

### A-2 — No model or adapter type executes an effect, authorizes a tool, or
supplies an execution fallback. Compliant.

`docs/architecture.md` layers 2 and 4: the transport "does not authorize or
execute tools"; "a model tool call is an untrusted proposal, not an
authorization." The broker "obtains an authorization decision, invokes the
execution backend… A model tool call is an untrusted proposal."

Observed: `ModelTurn`, `ToolIntent`, `ModelMessage`, and `ModelStreamEvent`
carry no `fn` that can spawn a process, open a file, or widen a grant. The
loop driver's single effect call is
`executor.execute(intent, cancellation)` (`driver.rs:751`), where `executor: &E,
E: ToolExecutor` is host-injected. There is no process or filesystem call in
`session.rs` or `driver.rs` outside the injected executor. `validate_turn`
(`session.rs`) bounds and de-duplicates proposals "without authorizing them."
The concrete `ExecutionBackend::execute` is itself a `ToolExecutor`
implementation; it is not a fallback the loop can call directly, and `new`
fails closed before any effect when the runner or backend is unavailable, so
the loop can never reach an unconstrained host path. Bounded evidence:
`delegates_every_effect_to_the_executor`, `fails_closed_when_the_runner_is_unavailable`,
`fails_closed_when_the_backend_is_unavailable`, `render_request_ignores_model_workspace_and_environment`
(in `tests/execution_backend_delegation.rs` and the `execution_backend.rs`
unit tests).

### A-3 — Credential and resource grants are host-bound, not model-supplied.
Compliant.

`docs/architecture.md` layers 5 and 6: policy "binds role, task, tool version,
canonical arguments, resource scope, environment, credentials, network
destination"; the execution backend receives "mounted resources, writable
scope, executable and arguments, environment, credentials, identity, network
policy, resource limits, deadline."

Observed: `ModelIdentity` documents "contains no policy or credentials."
`ExecutionBackend::new(runner_path, workspace, environment)` binds the
read-write workspace and extra environment at construction, before any intent
is seen; `render_request` ignores any model-supplied `working_directory` or
`environment` and uses only the host-bound values plus fixed `HOME`/`PATH`, so
the model cannot redirect the workspace grant or inject a loader variable
(`render_request_ignores_model_workspace_and_environment` pins exactly this: a
model `LD_PRELOAD` is dropped and the workspace grant source is the host
directory). The rendered request fixes `network.mode = "deny"` with an empty
`allowed_sources`, and resource ceilings are constants, not model input. The
model supplies only `argv`.

### A-4 — Cancellation split: the provider gets a fresh operation token;
effects get the prompt token. Compliant.

`docs/architecture.md` layer 2 (cancellation) and the `sa-08` security-audit
scope. Observed: `Operation::start` creates a fresh token and a watcher that
mirrors prompt cancellation, shutdown, and pending guidance (`driver.rs`,
`Operation`), and the transport is driven with `&operation.token`
(`driver.rs:1032–1034`); effects are driven with the prompt `cancellation`
(`driver.rs:751`). So steering/interruption of a generation cancels the
provider operation without cancelling the whole prompt, and an executing
effect honors the prompt token. This matches the documented split.

### A-5 — The seam is stable for host-broker and transport experiments.
Compliant, subject to M-1.

`docs/architecture.md` "Delivery order" step 3: "Define standalone-owned
canonical model, capability, tool-intent, and runtime-event contracts, plus the
host authority traits they consume." The types for that step exist and are
exported: `BackendClass`, `CapabilityState`/`CapabilityDescriptor`,
`ProviderExtension`, `RedactedEvent`/`redact_event` (contracts); `ModelTransport`,
`ModelMessage`/`ModelTurn`/`ToolIntent` (model); `ToolExecutor`/`Recorder`
(session); `StopReason`/`ToolFailure` (diagnostics). A host broker or transport
experiment can be built against these shapes today. The qualification "subject
to M-1" is that the canonical capability-state and redacted-event interfaces
are not yet consumed by the loop (see M-1), so the seam is defined-but-not-yet
enforced for those two families.

## Clause classifications

Statuses use the runbook terms. `A` denotes an authority-boundary
classification above; numbered clauses are the architecture contract points
tested.

### L-1 — Backend classification is closed and provider-independent.
Compliant, subject to O-6.

`docs/architecture.md` "The stable abstraction is therefore not 'provider.' It
is a small set of backend roles connected to host-owned authority layers."
Observed: `BackendClass` is a two-variant closed enum with no provider, SDK, or
framework name (`contracts.rs:35`); `mediates_tool_intents()` is read-only
(`contracts.rs:46`). Bounded evidence: `backend_class_is_provider_independent_and_closed`
(round-trips `external_agent` through its wire form). See O-6 for the parallel
unconnected registry classification.

### L-2 — Capability state is three-tiered, monotonic, and only
policy-enabled. Compliant in type, mismatched in consumption (M-1).

`docs/architecture.md` layer 5: "Capabilities have three distinct states:
advertised, tested, policy-enabled. No advertised capability is enabled
automatically." Observed type: `CapabilityState::enabled()` is true only for
`PolicyEnabled`; `next()` is `Advertised → Tested → PolicyEnabled → None`;
`CapabilityDescriptor::advertised` starts at `Advertised` (`contracts.rs:75,125`).
Bounded evidence: `capability_state_promotes_monotonically_and_only_policy_enables`,
`capability_descriptor_starts_advertised_and_reports_enabled`. However, no loop
path consults `CapabilityState`/`is_enabled`; the loop neither checks that a
capability is `PolicyEnabled` before offering a tool nor refuses a tool whose
capability is merely `Advertised`. See M-1.

### L-3 — Canonical model messages, turns, and streaming are provider-neutral
and bounded. Compliant, subject to O-1.

`docs/architecture.md` layer 2: the transport owns "translation between
standalone canonical messages and provider messages" and "returns a canonical
model turn." Observed: `ModelMessage`, `ModelRequest`, `ModelTurn`,
`ModelStreamEvent`, and `FinishReason` carry no provider SDK type. Bounded
evidence: the `sa-08` test suite (per `sa-08-write-tests`) and the
`sa-04`-precedent `execution_backend_delegation.rs` scripted-model harness.
O-1 records that `ModelTurn.provider: LocalModelProvider` is a *named*
provider field inside the otherwise-neutral turn.

### L-4 — Tool intent is an untrusted proposal with bounded, unique
identities; failure classification never authorizes replay. Compliant.

`docs/architecture.md` layer 4: "A model tool call is an untrusted proposal,
not an authorization." `ROOT_CONTRACT.md` §`Non-negotiable architecture`:
"Treat agent configuration, prompts, and output as untrusted input." Observed:
`ToolIntent` carries no authority (`model.rs:157`); `validate_turn` rejects
incomplete/ambiguous finish reasons and duplicate, over-large, or non-object
proposals "without authorizing them" (`session.rs`); `ToolFailure::from_error`
maps a host `Error` to `EffectCertainty` so an explicit refusal is `NotExecuted`
while a generic host error is `MayHaveEffects`, and `new` sets a `RecoveryAction`
that is explicitly "never permission to retry or broaden authority"
(`diagnostics.rs`). Bounded evidence: the `sa-08` tool-intent regressions and
`render_request_requires_a_non_empty_argv`.

### L-5 — The host-service traits are the only effect and record paths and are
narrow (no executor, policy, terminal, or task type is provided). Compliant.

`docs/architecture.md` ownership topology: "The standalone interfaces accept
that authority through narrow traits; another application may provide its own
policy and evidence services without depending on Kvist." Observed: `ToolExecutor`
(`session.rs:530`) and `Recorder` (`session.rs:562`) are the injected seams; the crate
provides no default executor, policy, terminal, or application task type. The
runtime `Error` is a "narrowly scoped runtime" type distinct from the crate's
`Error`, and `From<crate::Error>` keeps the mapping total (`error.rs`). Bounded
evidence: `delegates_every_effect_to_the_executor`,
`probes_runner_and_backend_before_dispatch`, `binds_delegated_effects_to_the_installed_runner`.

### L-6 — Redacted runtime events drop all free text and retain only
structure; the host owns the durable record. Compliant in type, mismatched in
consumption (M-1).

`docs/architecture.md` layer 7: "It derives durable records from bounded
standalone runtime events and host authority decisions… each model
request/result in bounded redacted form." Observed type: `redact_event`
projects every `Event` to a `RedactedEvent` with no model text, reasoning, or
tool argument surviving; `Control`/`Reasoning`/`Text`/`ToolFailure`/`Progress`
collapse to `Note` (`contracts.rs:273`). Bounded evidence:
`redaction_drops_free_text_and_keeps_structure`. However, the loop's recording
path calls the host `Recorder` directly with the unredacted `Event`s (e.g.
`tool_result` receives the full `ToolOutcome`; `sink.send(Event::*)` forwards
untrusted text); `redact_event` is a public helper the host may call, but it is
not applied by the loop. So "redacted runtime events" exist as a standalone
type with no demonstrated owner inside the loop. See M-1.

### L-7 — The concrete execution backend delegates through a versioned
protocol and fails closed with no unconstrained fallback. Compliant, subject
to O-2 and O-3.

`docs/architecture.md` layer 6: "Only this layer causes local effects. It
receives an explicit request containing mounted resources, writable scope…
network policy, resource limits, deadline, and artifact destinations." Observed:
`ExecutionBackend` renders a closed `phase: "authoring"` version-one request
with fixed `network.mode = "deny"`, constant resource ceilings, and the
approval-bound runner/backend/command/mount-plan identities; it re-hashes the
runner and backend before every dispatch (`rebind`) and refuses on drift
(`RunnerUnavailable`/`BackendUnavailable`); the probe and the delegated
execution are deadline- and size-bounded and reap the process group on every
outcome. Bounded evidence: `refuses_a_missing_runner_before_dispatch`,
`refuses_a_symlinked_runner_before_dispatch`,
`a_probe_that_reports_incomplete_capabilities_is_backend_unavailable`,
`capabilities_complete_requires_every_namespace_and_session_flag`,
`command_identity_matches_the_runner_recomputation`,
`mount_plan_identity_encodes_destination_access_purpose`,
`render_request_builds_a_closed_authoring_request`,
`render_request_ignores_model_workspace_and_environment`.

### L-8 — The typed runtime error surface covers the documented failure
states. Compliant, subject to O-4 and O-5.

`docs/architecture.md` layer 4 (broker failure states) and `sa-08` security-audit
scope ("missing failure states"). Observed: `runtime::Error` distinguishes
`Cancelled`, `ToolPolicy`, `ToolRender`, `ToolPrerequisite`, `RunnerUnavailable`,
`BackendUnavailable`, `Host`, `ContextBudget`, `RunBudget`, `Recording`,
`InvalidModelTurn`, `Config`, `ChannelClosed`, `Control`, `Steering`,
`RepetitiveGeneration`, and `Io`. `ToolFailure::from_error` and
`from_outcome` classify `Denied`/`InvalidRequest`/`Prerequisite` as
`NotExecuted` and `Cancelled`/`Timeout`/`OutputLimit`/`ProcessExit`/`Unobserved`
as `MayHaveEffects`, and `from_outcome` refuses to guess OOM from a missing
status. Bounded evidence: `render_request_requires_a_non_empty_argv`
(`ToolRender`), `refuses_a_missing_runner_before_dispatch` (`RunnerUnavailable`),
`a_runner_that_fails_the_probe_is_backend_unavailable` (`BackendUnavailable`).
O-4 and O-5 record the two specific gaps: a defined-but-unfrozen
`StopReason::GuidanceRequired`, and the `RedactedEvent` collapse of
`ToolFailure` to `Note` (so a redacted journal loses the failure *category*).

### L-9 — The change-and-compliance process for this chain. Compliant, subject
to O-7.

`ROOT_CONTRACT.md` §`Change and compliance rules`: "`TODOS.yaml` orders work as
tests, implementation, security audit, then compliance review"; "Record
intent-to-implementation discrepancies for explicit arbitration; do not
silently alter either artifact." Observed: the `sa-08` chain order is reflected
in `TODOS.yaml`; this record derives the interfaces from source (per the task
mandate) and compares them to the architecture contract, retaining M-1 and
O-1 through O-7 for arbitration without altering `REQUIREMENTS.md`,
`CONTRACT.md`, `DESIGN.md`, `IMPL.md`, or the implementation. O-7 records the
methodological deviation from the runbook's source-blind pass.

## Discrepancies (retained for human arbitration)

### M-1 (HIGH, mismatched) — the canonical capability-state and
redacted-event interfaces are defined but not consumed by the loop.

- *Locator:* `docs/architecture.md` layer 5 ("No advertised capability is
  enabled automatically") and layer 7 ("…bounded redacted form");
  `contracts.rs:75,125,211,273`; `session.rs` `ToolExecutor`/`Recorder` and the
  `driver.rs` recording path.
- *Intended:* the standalone-owned canonical capability-state contract gates
  whether a capability may be used, and runtime events reach the durable record
  in redacted form.
- *Observed:* `CapabilityState`, `CapabilityDescriptor`, `is_enabled`, and
  `enabled` are defined and re-exported but referenced only in
  `contracts.rs`'s own tests and doctests — no `driver.rs`/`session.rs` path
  checks a capability's tier before offering or executing a tool, and no loop
  path refuses a tool whose capability is not `PolicyEnabled`. Likewise
  `RedactedEvent` and `redact_event` are defined and re-exported but not called
  by the loop: the loop forwards unredacted `Event`s to the host `EventSink`
  and `Recorder`, and `tool_result` receives the full `ToolOutcome`.
  `BackendClass` and `ProviderExtension` are in the same defined-but-not-loop-consumed
  position (see O-6).
- *Why it matters:* the task asks whether the seam is "stable enough for host
  broker and transport experiments." The shapes are stable and safe, but the
  *enforcement* of the capability gate and of redaction currently has no owner
  inside the standalone runtime — it is delegated entirely to a future host
  broker or to the host's own redaction. A host that assumes the loop already
  enforces `PolicyEnabled` or already emits redacted events would be wrong
  today. This is the same structural concern as the retained `shared-runtime`
  M-1 (redacted events have no demonstrated owner), now scoped to the canonical
  contract types.
- *Arbitration options:* (a) accept the current realization — the loop
  defers capability gating and redaction to the injected `ToolExecutor`/
  `Recorder` and the standalone types are available for the host broker — and
  amend the intent to name that split precisely, classifying L-2 and L-6
  compliant under that reading; (b) require the loop to consult
  `CapabilityState` before offering/executing a tool and to apply
  `redact_event` on the recording path, making the canonical seam self-
  enforcing; (c) record a reasoned deferral with the retention condition that
  the `sa-04` broker (the next queued task, which depends on this review) is
  responsible for wiring these two interfaces before the seam is declared
  enforcing.

## Observations (retained as LOW)

### O-1 — `ModelTurn.provider` names a provider inside the canonical turn.

- *Locator:* `model.rs:208` (`pub provider: LocalModelProvider`);
  `LocalModelProvider { Ollama, LlamaServer }` at `model.rs:68`.
- *Observed:* the "provider-neutral" turn carries a closed, *named* local-HTTP
  provider enum. It is provider-neutral in the sense required here — it names
  no third-party SDK and grants no authority — but it is not provider-*
  agnostic*: a transport for a local provider not in `{ Ollama, LlamaServer }`
  cannot express its turn with the current type. The loop itself does not read
  this field, so it does not affect mediation or effects.
- *Arbitration options:* accept the closed local-provider set as the current
  contract (both local transports are covered); or make `provider` a stable
  string/`Option` so a new local transport need not widen a canonical enum.

### O-2 — the concrete execution backend is host-specific even though the
host-service trait is neutral.

- *Locator:* `execution_backend.rs` (module doc and constants); `session.rs:530`.
- *Observed:* `ToolExecutor` is neutral, but the crate also ships a concrete
  `ExecutionBackend` that resolves the installed `galla` runner and requires a
  `bubblewrap` backend. The concrete type therefore couples the `sav` crate to
  one enforcement backend, while the neutral trait keeps the *loop*
  backend-agnostic. This is the intended layering (layer 6 is a
  pluggable backend), recorded so the seam's neutral surface is not read as the
  concrete backend's surface.
- *Arbitration options:* accept the concrete backend as a reference
  implementation behind the neutral trait (current); or move the concrete
  galla/bubblewrap delegation out of the library's default export so the
  public seam is purely the trait.

### O-3 — the delegated effect carries a fixed resource ceiling and denied
network, not a per-grant ceiling.

- *Locator:* `execution_backend.rs` `SandboxResources` and the
  `DEFAULT_WALL_TIME_MS` / `DEFAULT_MAX_OUTPUT_BYTES` constants.
- *Observed:* the rendered request uses fixed `wall_time_ms`, `max_output_bytes`,
  `max_processes`, `max_files`, `max_file_bytes`, `max_scratch_bytes`, and
  `network.mode = "deny"`. The `RunLimits` the loop exposes are loop-level
  (wall time and tokens) and are not passed into the rendered request. This is
  safe-by-default (it cannot be widened by the model), recorded so it is not
  read as per-effect resource binding from the run's limits.
- *Arbitration options:* accept the fixed safe-default ceiling; or thread the
  run's `RunLimits` (where applicable) into the rendered request as a
  host-bound cap.

### O-4 — `StopReason::GuidanceRequired` is defined but never frozen.

- *Locator:* `diagnostics.rs:28`; `driver.rs` `stopping_reason`
  (`driver.rs:52`–70) and the terminal freeze at `driver.rs:878`.
- *Observed:* the doc comment states "Guidance required: recovery budget
  exhausted but work remains," but the driver's `stopping_reason` and every
  terminal assignment produce only `Completed`, `NoWork`, `Cancelled`,
  `Limit`, `Stalled`, or `Failed`. When the recovery budget is exhausted the
  loop freezes `Stalled` (`driver.rs:625,848`), not `GuidanceRequired`. So the
  variant has no producer; a consumer cannot distinguish "stopped because
  guidance is needed" from "stopped because generation stalled" on the frozen
  summary.
- *Arbitration options:* (a) add the freeze site that sets
  `GuidanceRequired` when the recovery budget is exhausted with work remaining;
  or (b) remove the variant until a producer exists, so the frozen `StopReason`
  surface is closed to what the loop can actually report.

### O-5 — redaction collapses `ToolFailure` to `Note`, dropping the failure
category.

- *Locator:* `contracts.rs:295–303` (`Event::ToolFailure { .. } =>
  RedactedEvent::Note`); `Event::ToolFailure` in `session.rs`.
- *Observed:* the redacted form of a `ToolFailure` event is `Note`, which
  discards the `ToolFailureCategory`, `EffectCertainty`, and `RecoveryAction`
  that the full event carries. For a host journal built from redacted events,
  this loses the *category* and *effect-certainty* of a failed tool, retaining
  only that a tool result was `failed`. The full `Event::ToolFailure` still
  reaches the unredacted `Recorder`/`EventSink`.
- *Arbitration options:* accept that redacted events retain structure-only
  (the category is available to the host through the unredacted path and the
  `ToolFailure` type); or add a category-bearing redacted variant so a
  redacted journal can record *which* failure class occurred without the
  untrusted message.

### O-6 — two parallel, unconnected backend classifications exist.

- *Locator:* `contracts.rs:35` (`BackendClass`); the registry's tagged
  `Backend` (native HTTP model vs external agent) resolved into
  `ResolvedProfile` (see the retained `shared-runtime` record, C-1).
- *Observed:* the canonical `BackendClass` and the registry's backend
  classification are defined independently; nothing in the loop maps a
  `ResolvedProfile`'s backend to a `BackendClass`, and the native loop hard-
  codes native-model behavior while the external path is a separate supervised
  observation. The two are consistent in intent but are separate seams that
  could drift.
- *Arbitration options:* accept both (the registry is configuration, the
  contract type is the protocol identity); or add an explicit
  `ResolvedProfile → BackendClass` mapping so a single source classifies the
  backend end to end.

### O-7 — this review is a source-derived comparison, not the runbook's
source-blind pass.

- *Locator:* `REVIEW_RUNBOOK.md` "Source-blind compliance pass"; the `sa-08`
  task definition in `sav/TODOS.yaml`.
- *Observed:* the task directs source derivation, so this record reads
  implementation source (with `path:line` anchors) rather than comparing
  `REQUIREMENTS.md`/`CONTRACT.md`/`DESIGN.md` to `IMPL.md` and bounded test
  evidence without source. The two records answer different questions: the
  runbook pass certifies that intent matches the record; this record derives
  what the interfaces *are* and whether they preserve the architecture
  contract. Neither was authored by the implementer or the security auditor.
- *Arbitration options:* accept this as the `sa-08` record with the method
  noted (current); or additionally run a runbook source-blind pass against
  `IMPL.md` to certify intent-to-record correspondence for the same seam.

## Discrepancy summary

| ID | Clause / finding | Status | Severity |
|----|------------------|--------|----------|
| A-1 | One-way dependency; no host/SDK type imported | compliant | none |
| A-2 | No model/adapter type executes, authorizes, or falls back | compliant | none |
| A-3 | Credentials and grants are host-bound, not model-supplied | compliant | none |
| A-4 | Provider-token vs prompt-token cancellation split | compliant | none |
| A-5 | Seam stable for host-broker and transport experiments | compliant (M-1) | high |
| L-1 | Backend classification closed and provider-independent | compliant (O-6) | low |
| L-2 | Capability state three-tier, monotonic, policy-enabled-only | mismatched in consumption (M-1) | high |
| L-3 | Canonical model messages/turns neutral and bounded | compliant (O-1) | low |
| L-4 | Tool intent untrusted; bounded unique; no replay authority | compliant | none |
| L-5 | Host-service traits are the only effect/record paths, narrow | compliant | none |
| L-6 | Redacted events drop free text, keep structure | mismatched in consumption (M-1) | high |
| L-7 | Execution backend delegates via versioned protocol, fails closed | compliant (O-2, O-3) | low |
| L-8 | Typed error surface covers documented failure states | compliant (O-4, O-5) | low |
| L-9 | Change-and-compliance process for this chain | compliant (O-7) | low |

## Retention and independence statement

This record is the retained independent comparison for the `sa-08` chain. It
was not authored by the implementer or the security auditor, and it does not
certify acceptance, mint a receipt, or determine compliance by severity. It
retains M-1 and O-1 through O-7 for explicit human arbitration and does not
alter `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `IMPL.md`, the
implementation, or the task queues. The bounded test evidence is static and is
run automatically by Kvist in the verification sandbox; no pass/fail count from
that run is asserted here.
