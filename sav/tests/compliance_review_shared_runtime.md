<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Shared Registry, Runtime and Observation (ADR 0015, `sav`)

**Component:** `sav`
**Task:** `shared-runtime-compliance-review`
**Compared artifacts:**
- `sav/REQUIREMENTS.md` §`Shared registry configuration acceptance (ADR 0015)`,
  §`AR-REQ-SHARED-RUNTIME`, §`AR-REQ-SETUP`, §`AR-REQ-NATIVE-RUNTIME`,
  §`AR-REQ-SUPERVISION`, §`Shared native runtime verification (ADR 0015)`
- `sav/CONTRACT.md` §`Shared native runtime API (ADR 0015)`,
  §`Shared registry configuration (ADR 0015)`, §`Provided interfaces` →
  "Live diagnostic journals and private controls", §`Behavioral guarantees`,
  §`Security and authority`
- `sav/DESIGN.md` §`Shared native runtime extraction (ADR 0015)`,
  §`Shared registry realization (ADR 0015)`, §`Algorithms and decisions`,
  §`Security and resource design`
- `sav/IMPL.md` ("Evidence scope", "Observed public contract",
  "Observed internal design and failure paths", "Bounded repair observations,
  unresolved findings, and limits", "Test-source coverage and execution
  limitations")
- `ROOT_CONTRACT.md` §`Change and compliance rules`
- bounded test evidence (named test functions, listed below)

**Reviewer stance:** Independent source-blind compliance reviewer. This record
is the retained comparison for the `shared-runtime` chain; the implementer and
the security auditor did not author it, and it is not self-certified. The
source-only observations this record compares were derived in a separate
context (clean-slate documenter) and are retained in `sav/IMPL.md`; the
separate source-blind diagnostic comparison produced alongside this record is
persisted at `docs/reviews/0015-sav-comparison.md`.

## Review boundary and independence

This is the final stage of the `shared-runtime` chain (tests → implementation →
security audit → compliance review). Per `REVIEW_RUNBOOK.md`, the source-blind
compliance reviewer compares the intended artifacts with the implementation
record and bounded test evidence.

**Read (permitted intent context):** `sav/REQUIREMENTS.md`, `sav/CONTRACT.md`,
`sav/DESIGN.md`, `ROOT_CONTRACT.md`, `REVIEW_RUNBOOK.md` (the review procedure),
and the canonical task-definition projection for this chain in
`sav/TODOS.yaml`.

**Read (implementation record and bounded test evidence):** the ADR 0015
sections of `sav/IMPL.md` listed above (clean-slate source-only observations,
including the re-inspected selector-consistency mechanisms and the "Unresolved
independent finding"), and the named bounded test evidence below (test function
names and their assertions as recorded in `IMPL.md`).

**Not read as evidence:** implementation source, dependency manifests, Git
history/diffs, prior compliance conclusions, peer or parent implementation
artifacts, queues beyond this chain's task projection, or the security-audit
report of this chain. Where this record cites an `IMPL.md` claim, the claim is
attributed to the documenter record, not re-verified here against source.

**Not executed:** the reviewer ran no tests, builds, or probes. This record is
not execution proof; the bounded suites are run automatically by Kvist in the
verification sandbox, and no pass/fail count from that run is asserted here.

## Bounded evidence relied upon

Registry and resolution (`tests/registry.rs`, `tests/registry_cli_policy.rs`):
`registry_resolution_is_ordered_pinned_and_does_not_probe`,
`native_model_selector_rejects_values_the_transport_cannot_use`,
`native_model_selector_accepts_the_exact_transport_boundary`,
`native_deadlines_support_slow_models_and_retain_host_clipping`,
`rejects_unknown_fields_credentials_bounds_and_retired_command_schema`,
`external_identity_is_structured_and_resolution_never_executes`,
`disabled_profiles_and_invalid_present_sources_fail_closed`,
`registry_upsert_roundtrips_and_refuses_symlinks`,
`structured_argv_never_reinterprets_prompt_placeholders_or_shell_syntax`,
`rejects_inline_credentials_and_independent_registry_bounds`,
`explicit_source_is_required_and_whole_profiles_override_not_fields`,
`native_cli_executes_the_selected_structured_profile_and_inspection_is_read_only`,
`external_profiles_default_to_one_attempt_even_after_retryable_failure`,
`external_profiles_reject_positive_retries_before_spawning`,
`external_profiles_accept_explicit_zero_retries`,
`explicit_command_preserves_default_and_explicit_retry_policy`,
`native_profiles_honor_cli_response_limit_for_unary_and_streaming`,
`native_profiles_honor_short_cli_deadlines_without_implicit_watchdog_conflicts`,
`native_profiles_reject_inapplicable_watchdogs_and_invalid_limits_before_contact`.

Runtime controls and recording (`tests/runtime_controls.rs`):
`quiet_generation_is_interrupted_without_cancelling_prompt`,
`mailbox_bounds_edits_and_withdrawals_are_atomic_before_claim`,
`repeated_render_and_policy_failures_stop_without_reporting_execution`,
`refusal_notice_failure_fences_unpaired_history_before_session_reuse`,
`duplicate_read_notices_forget_old_observations_without_blocking_reads`,
`effect_finishes_stale_calls_are_paired_then_ordered_guidance_reaches_provider`,
`shutdown_during_effect_retains_result_but_never_delivers_or_restarts`,
`guidance_interrupts_backoff_and_renews_generation_attempts`,
`delivery_recording_failure_stops_before_another_provider_or_effect`,
`guidance_accepted_during_dispatch_recording_skips_every_stale_effect`,
`live_model_event_recording_failure_stops_before_presentation_or_effects`,
`withdrawal_after_operation_interrupt_does_not_cancel_the_prompt`,
`guidance_racing_final_answer_cannot_become_a_replacement_task`,
`repeated_human_guidance_renews_turn_and_generation_allowances`,
`new_tasks_reset_explicit_token_budgets_and_invalid_limits_fail`,
`interrupted_generation_keeps_token_charge_and_prompt_deadline`,
`completion_and_submission_classification_have_one_linearization_point`,
`repeated_guidance_cannot_reset_explicit_token_limit`.

Journals and controls (`tests/observation.rs`):
`follower_sees_live_checkpoints_and_terminal_without_reopening`,
`incomplete_tail_is_pending_but_gaps_and_oversized_lines_fail`,
`readers_reject_links_special_files_versions_and_mixed_runs`,
`record_validation_is_atomic_and_eof_is_not_completion`,
`oversized_initial_metadata_never_reserves_a_filename_and_serialization_is_bounded`,
`runtime_control_bridge_never_promotes_late_guidance_to_new_tasks`,
`follower_keeps_original_descriptor_and_limits_batches`,
`journals_rotate_past_64_mib_without_ending_the_run`,
`terminal_at_poll_boundary_cannot_hide_trailing_data`,
`followers_reject_invalid_missing_and_linked_continuations`,
`private_controls_are_validated_acknowledged_and_removed`,
`control_rejects_malformed_requests_without_calling_host_and_bounds_idle_clients`,
`journal_rotation_collision_stops_without_partial_append`.

External-agent boundary (`tests/external_supervision.rs`):
`opaque_process_streams_once_without_native_tool_interpretation`,
`cancellation_and_recording_failure_terminate_the_owned_process`,
`opaque_effects_cannot_be_retried_and_pre_cancel_never_spawns`.

Setup (`tests/setup.rs`): `llama_server_lists_models_and_keeps_profile_name_separate`,
`llama_server_keeps_explicit_default_when_discovery_is_unavailable`,
`ollama_default_materializes_the_selected_endpoint`, plus the loopback and
host-acknowledgement refusals named above.

Accounting (`src/runtime/accounting_tests.rs`):
`native_request_uses_none_without_tools_and_auto_with_tools`,
`no_tool_native_request_passes_direct_transport_validation`,
`decode_clock_needs_two_observations_and_excludes_callback_time`,
`live_progress_keeps_input_output_and_total_cumulative`,
`generation_timing_excludes_first_token_wait_and_presentation`,
`generation_timing_accumulates_retry_decode_without_backoff`,
`native_live_totals_preserve_provider_usage_and_post_turn_rate_convention`,
`progress_elapsed_uses_prompt_start_including_initial_recording`.

Execution evidence is attributed, not independently executed here: the chain
record reports the final post-selector workspace run
**1746 passed, 0 failed, 14 existing ignored, 104 successful target summaries**,
finished 2026-10-24T00:00:55Z, with strict workspace all-target Clippy,
formatting, release binaries, ordinary workspace rustdoc and strict Sav/Skott
rustdoc passing. `kvist doctor` blockers (a pre-existing Skott
`lifecycle-write-tests` `completed_at`/`updated_at` inconsistency, invalid
root state/VCS readiness, and an unavailable Kvist-managed toolchain Cargo
path) are recorded as unchanged pre-existing preconditions, not repaired by
this review.

## Clause classifications

R/C/D denote Sav requirements/contract/design; I denotes the `IMPL.md`
headings cited. Statuses use the runbook terms.

### C-1 — One bounded typed registry with distinct backends and pinned,
source-bound resolution. Compliant.

`R` "Shared registry configuration acceptance (ADR 0015)"; `C` "Shared
registry configuration (ADR 0015)"; `D` "Shared registry realization";
I "Shared registry, setup, and pinned resolution".
Observed per I: tagged `Backend` variants for native HTTP models and external
agents; schema-one TOML with unknown-field/type rejection; explicit/project/
user whole-definition precedence, missing optional sources skipped, malformed
present sources failing closed, explicit sources required; resolution returning
name, profile, source path and SHA-256 digest of the entire original source
text with no process, endpoint, loaded-model or executable probing. Bounded
evidence: `registry_resolution_is_ordered_pinned_and_does_not_probe`,
`external_identity_is_structured_and_resolution_never_executes`,
`explicit_source_is_required_and_whole_profiles_override_not_fields`.
Note N-1: the snapshot is not executable authentication.

### C-2 — Capability separation; credentials are references, not inline
secret material. Compliant, subject to N-2 and N-3.

`R` `AR-REQ-SHARED-RUNTIME`; `C` "Shared registry configuration (ADR 0015)".
Observed per I: three distinct `advertised`, `tested`, `policy_enabled`
identifier lists whose contents grant no tool authority; unknown capabilities
default to empty and are not enabled; parsing refuses inline credentials and
credential-bearing external arguments. Bounded evidence:
`rejects_unknown_fields_credentials_bounds_and_retired_command_schema`,
`rejects_inline_credentials_and_independent_registry_bounds`.

### C-3 — Filesystem-only, fail-closed parsing and bounds; atomic, private,
link-safe persistence. Compliant, subject to N-3.

`R` "Shared registry configuration acceptance (ADR 0015)";
`C` "Shared registry configuration (ADR 0015)"; `C` "Behavioral guarantees".
Observed per I: 64 KiB per file, 128 profiles per file and merged registry,
1–128-byte ASCII selector names, 32 unique capability identifiers; bounded
UTF-8 reads, leaf `O_NOFOLLOW`/`O_NONBLOCK`, regularity/size checks; no-clobber
first creation and validate-before/after atomic synchronized replacement;
symlink final-parent refusal. Bounded evidence:
`registry_upsert_roundtrips_and_refuses_symlinks`,
`disabled_profiles_and_invalid_present_sources_fail_closed`.
Limit in I: no descriptor-walk of every ancestor and no serialized concurrent
read-modify-write updates.

### C-4 — Shared setup reuses discovery and qualification with bounded
output, no test-prompt prompt, and no capability-test promotion on
`--force`. Compliant, subject to N-4.

`R` `AR-REQ-SETUP`; `C` "Behavioral guarantees"; I "Shared registry, setup,
and pinned resolution" and "Representative existing helpers".
Observed per I: explicit discovery through shared catalogs (Ollama
`/api/tags`, llama-server `/v1/models`, ACP `session/new` for Copilot/Gemini),
numbered choices with a final explicit custom choice, the fixed qualification
prompt `Reply with exactly: OK`, tools disabled, 32 reserved output tokens,
30-second idle / 300-second attempt / 64 KiB qualification bounds,
qualification output captured and not forwarded, and `--force` persisting a
failed profile without marking any capability tested. Bounded evidence:
`llama_server_lists_models_and_keeps_profile_name_separate`,
`llama_server_keeps_explicit_default_when_discovery_is_unavailable`,
`ollama_default_materializes_the_selected_endpoint`.
Note N-4: exact equality of the returned answer with `OK` is neither checked
nor explicitly required.

### C-5 — Native registry selections and frontend overrides satisfy the
direct transport's model-selector bounds before use. Compliant.

`R` `AR-REQ-SHARED-RUNTIME`; `C` "Shared registry configuration (ADR 0015)";
`D` "Algorithms and decisions".
Observed per I "Shared registry, setup, and pinned resolution" and
"Re-inspected mechanisms": one shared validator (nonblank, control-free ASCII,
at most 256 bytes) used by registry parsing, `NativeModel::validate`, and
direct request validation before connection; no trimming or rewriting.
Bounded evidence: `native_model_selector_rejects_values_the_transport_cannot_use`
(257-byte and non-ASCII refusal through parsing and explicit validation),
`native_model_selector_accepts_the_exact_transport_boundary` (exact 256-byte
acceptance), and the direct-transport invalid-before-connect refusals recorded
by I. The separately scoped 1024-byte external-model bound is a different
identity, not a native incompatibility.

### C-6 — Runtime mechanics independent of terminal and maerg types; hosts
inject executor, policy, and recording. Compliant, subject to N-5.

`R` `AR-REQ-SHARED-RUNTIME`; `C` "Shared native runtime API (ADR 0015)";
`D` "Shared native runtime extraction (ADR 0015)".
Observed per I "Native model/runtime interfaces": `NativeRuntime` consumes an
injected `ToolExecutor`, fallible `EventSink`, cancellation token, context
manager, optional `Recorder`, and optional private `ControlMailbox`; the
executor owns effects and policy; there is no implicit process fallback or
registry reload inside the loop; no terminal or application task type is
provided by the crate. Bounded evidence:
`native_request_uses_none_without_tools_and_auto_with_tools`,
`no_tool_native_request_passes_direct_transport_validation`.
Note N-5: the broader reusable typed broker/selected-backend realization
described in `C` "Security and authority" is not demonstrated by the observed
injected-executor loop.

### C-7 — Steering is bounded and ordered; allowances renew on delivery;
provisional generations are discarded; skipped calls are paired; effects are
never replayed. Compliant.

`R` `AR-REQ-SHARED-RUNTIME`; `C` "Shared native runtime API (ADR 0015)";
`D` "Shared native runtime extraction (ADR 0015)".
Observed per I "Native execution, accounting, and controls": a mutex
linearizes submission classification, claim/edit/withdraw, dispatch readiness
and final-answer completion; guidance interrupts quiet generation and backoff
without cancelling the prompt or interrupting an executing effect; partial
interrupted model output is not added as an assistant message; superseded
tool calls receive explicit rejected results; delivered state is recorded
before the next provider request; non-repeatable effects use an eight-entry
repeat ring and the runtime never replays a tool. Bounded evidence:
`quiet_generation_is_interrupted_without_cancelling_prompt`,
`guidance_interrupts_backoff_and_renews_generation_attempts`,
`effect_finishes_stale_calls_are_paired_then_ordered_guidance_reaches_provider`,
`repeated_human_guidance_renews_turn_and_generation_allowances`,
`guidance_racing_final_answer_cannot_become_a_replacement_task`,
`new_tasks_reset_explicit_token_budgets_and_invalid_limits_fail`,
`interrupted_generation_keeps_token_charge_and_prompt_deadline`,
`completion_and_submission_classification_have_one_linearization_point`,
`repeated_guidance_cannot_reset_explicit_token_limit`.

### C-8 — Versioned live journals with bounded records and reads, explicit
incomplete/terminal state, and required-recording failure stopping further
effects. Compliant, subject to N-6.

`R` `AR-REQ-SHARED-RUNTIME`; `R` "Shared native runtime verification (ADR
0015)"; `C` "Live diagnostic journals and private controls".
Observed per I "Bounded live journal and local IPC" and "Native execution,
accounting, and controls": schema-one JSONL with one run ID and contiguous
one-based sequence; 64 KiB records, 64 MiB segments, 256-record poll batches,
no-clobber private files, held-descriptor readers; explicit terminal
`run_end` rather than EOF completion; rotation links preserving run identity;
required recording failures stop subsequent work and presentation; failed
writes poison the writer without inventing a terminal result. Bounded
evidence: `follower_sees_live_checkpoints_and_terminal_without_reopening`,
`incomplete_tail_is_pending_but_gaps_and_oversized_lines_fail`,
`readers_reject_links_special_files_versions_and_mixed_runs`,
`record_validation_is_atomic_and_eof_is_not_completion`,
`oversized_initial_metadata_never_reserves_a_filename_and_serialization_is_bounded`,
`follower_keeps_original_descriptor_and_limits_batches`,
`journals_rotate_past_64_mib_without_ending_the_run`,
`terminal_at_poll_boundary_cannot_hide_trailing_data`,
`followers_reject_invalid_missing_and_linked_continuations`,
`journal_rotation_collision_stops_without_partial_append`,
`delivery_recording_failure_stops_before_another_provider_or_effect`,
`live_model_event_recording_failure_stops_before_presentation_or_effects`.
Note N-6: the separate trajectory API is materially less restrictive.

### C-9 — Required-regression evidence for the shared runtime verification
list. Compliant, subject to N-7.

`R` "Shared native runtime verification (ADR 0015)".
Observed bounded evidence: quiet-provider interruption
(`quiet_generation_is_interrupted_without_cancelling_prompt`); backoff wakeup
(`guidance_interrupts_backoff_and_renews_generation_attempts`); effect
completion before guidance and skipped-result pairing
(`effect_finishes_stale_calls_are_paired_then_ordered_guidance_reaches_provider`);
final-answer/submission arbitration
(`completion_and_submission_classification_have_one_linearization_point`,
`guidance_racing_final_answer_cannot_become_a_replacement_task`); pre-claim
edit/withdraw (`mailbox_bounds_edits_and_withdrawals_are_atomic_before_claim`);
required-delivery recording failure
(`delivery_recording_failure_stops_before_another_provider_or_effect`);
shutdown (`shutdown_during_effect_retains_result_but_never_delivers_or_restarts`);
preserved explicit wall/token budgets and finite retries
(`new_tasks_reset_explicit_token_budgets_and_invalid_limits_fail`,
`interrupted_generation_keeps_token_charge_and_prompt_deadline`,
`repeated_guidance_cannot_reset_explicit_token_limit`).
Note N-7: dedicated preparation-failure and some timing-interleaving regressions
were not found in the bounded evidence.

### C-10 — Read-only observers cannot control; optional controls require a
private local capability, validate bounded messages, and acknowledge actual
acceptance/delivery. Compliant, subject to N-8.

`R` `AR-REQ-SHARED-RUNTIME`; `C` "Live diagnostic journals and private
controls".
Observed per I "Observation and control interfaces" and "Bounded live journal
and local IPC": followers only read the held descriptor and report explicit
terminal completion; the private socket is a generated 0700 directory with a
0600 socket, separate from the journal path; schema-one, nonzero-ID requests
are validated before the host callback; replies distinguish `queued`,
`cancellation_requested` and `rejected`, and queueing is not delivery;
malformed requests never invoke the handler; idle clients are bounded; late
guidance is rejected rather than promoted to a task; drop removes the owned
directory and socket. Bounded evidence:
`private_controls_are_validated_acknowledged_and_removed`,
`control_rejects_malformed_requests_without_calling_host_and_bounds_idle_clients`,
`runtime_control_bridge_never_promotes_late_guidance_to_new_tasks`.
Note N-8: trusted callback, same-UID exposure and unredacted payloads remain
mechanism limits.

### C-11 — External-agent execution requires an explicit execution boundary
and does not become an unconfined native transport. Compliant.

`R` `AR-REQ-SHARED-RUNTIME`; `C` "Live diagnostic journals and private
controls".
Observed per I "Opaque supervision and cancellation" and "Shared registry,
setup, and pinned resolution": `run_supervised_observed` requires
`max_retries = 0`, uses literal argv with null stdin and a fresh process
group, keeps bytes opaque so they never become native tool authority or task
approval, and pre-cancellation refusal happens before spawn; the CLI
separates `--allow-host-execution` and `--allow-host-discovery` and neither is
a sandbox. Bounded evidence:
`opaque_process_streams_once_without_native_tool_interpretation`,
`cancellation_and_recording_failure_terminate_the_owned_process`,
`opaque_effects_cannot_be_retried_and_pre_cancel_never_spawns`,
`external_profiles_default_to_one_attempt_even_after_retryable_failure`,
`external_profiles_reject_positive_retries_before_spawning`.

### C-12 — Supervision streams bounded output, enforces limits, terminates
and reaps the process group, and treats unknown side effects as retained.
Compliant per run, subject to M-2 and N-9.

`R` `AR-REQ-SUPERVISION`; `C` "Security and authority",
`C` "Errors and failure semantics".
Observed per I "Opaque supervision and cancellation": bounded combined output
with a 16-slot chunked channel, idle/repetition conditions, cancellation and
wall/output limits, owned process-group kill, child wait, drained accepted
output, one-second drain timeout, and explicit prior-effect uncertainty
notices before any retry; descendants that escape the group produce
`OutputStreamsRetained` rather than an indefinitely joined reader, and no
containment guarantee is claimed. Bounded evidence:
`retry_context_warns_about_prior_side_effects`,
`nonzero_exit_is_not_retried`,
`idle_process_is_terminated_before_retry_exhaustion_returns`,
`attempt_timeout_is_independent_of_continuing_output`,
`output_limit_is_terminal_and_not_retried`,
`descendants_holding_output_pipes_are_terminated_after_parent_exit`,
`escaped_descendant_retaining_output_fails_without_hanging`, plus the
external-supervision cases in C-11.
The per-run obligations are met by the bounded evidence; the cross-run
limitation of the process-global signal owner is retained as M-2, and the
precondition/evidence limits as N-9.

### C-13 — Tool intent is untrusted; effects require an explicit host
decision through the selected backend. Mismatched (retained as M-1).

`R` `AR-REQ-NATIVE-RUNTIME` ("separate model transport, bounded agent loop,
typed tool broker, host authorization, execution backend, and redacted
runtime events"); `C` "Security and authority".
Observed per I: the implemented loop separates transport, loop, host
authorization (injected executor), backend and events, and tool intent cannot
execute without the host executor. However, the `IMPL.md` record identifies no
generic secret redaction for runtime events and states that full `ToolOutcome`
data reaches the host recorder whose storage/redaction policy is host-owned.
The "redacted runtime events" obligation has no demonstrated owner in the
observed implementation. See M-1.

### C-14 — The change-and-compliance process for this chain. Compliant.

`ROOT_CONTRACT.md` §`Change and compliance rules`.
Observed: the tests → implementation → security audit → compliance review
order is reflected in the task projection; the clean-slate documenter
(deriving `IMPL.md` from source only) and this source-blind reviewer are
separate contexts; this record retains its discrepancies for explicit
arbitration and does not alter `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`,
`IMPL.md`, or the implementation.

## Discrepancies and observations (retained for human arbitration)

### M-1 (HIGH, mismatched) — "Redacted runtime events" has no demonstrated
owner.

- *Locator:* `REQUIREMENTS.md` §`AR-REQ-NATIVE-RUNTIME` ("…execution backend,
  and redacted runtime events"); `CONTRACT.md` §`Security and authority`
  ("validate/redact the result, and emit a bounded runtime event");
  `IMPL.md` "Native execution, accounting, and controls" ("Text is not
  generically secret-redacted. Full `ToolOutcome` data still reaches a
  recorder, whose storage/redaction policy is owned by the host") and
  "Bounded live journal and local IPC" ("Diagnostic/event payloads are not
  generically redacted").
- *Intended:* runtime events emitted into the shared journal are redacted.
- *Observed:* the record shows bounded, versioned event emission with
  host-owned redaction only as an explicit host responsibility; no runtime
  redaction point is recorded, and full outcomes reach host recorders.
- *Why it matters:* an embedding host that trusts the shared journal will
  retain unredacted tool payloads and raw output metadata unless it imposes
  redaction itself; the requirement names redaction as part of the runtime
  separation.
- *Arbitration options:* (a) amend the intent to name host-owned redaction
  as the precise obligation and classify the clause compliant under that
  reading; (b) require a runtime redaction or opt-in redaction boundary in
  the journal/event path; (c) record a reasoned deferral with the retention
  condition.

### M-2 (MEDIUM, mismatched) — singular process-global signal ownership.

- *Locator:* `IMPL.md` "Unresolved independent finding" (registration
  overwrites one global PGID; supervisor cleanup clears it unconditionally).
- *Intended:* `AR-REQ-SUPERVISION` cancellation and cleanup per run;
  `AR-REQ-SHARED-RUNTIME` cancellation distinct from steering.
- *Observed:* one process-global SIGINT/SIGTERM handler forwards to a single
  registered process group; concurrent independent supervisors share one group
  slot and a consuming interrupt flag, so concurrent runs can lose another
  live run's forwarding registration or consume its interrupt state. No
  serialization is enforced by the public supervision APIs.
- *Why it matters:* hosts running two supervised processes in one process can
  lose interrupt forwarding for one of them; the limitation is explicit in
  the record but not resolved.
- *Arbitration options:* (a) accept the documented limitation with a
  caller-serialization obligation added to the contract; (b) require a
  per-run signal registry or independent cooperative tokens; (c) record a
  reasoned exception naming the supported usage (one supervised run per
  process).

### N-1 (LOW, evidence limit) — resolution digests are in-memory snapshots,
not authentication.

- *Locator:* `CONTRACT.md` "Shared registry configuration (ADR 0015)"
  ("pins the selected name, complete typed profile, source path and content
  digest"); `IMPL.md` "Shared registry, setup, and pinned resolution".
- *Observed:* the SHA-256 digest covers the original source text at load time;
  later filesystem changes do not alter the loaded selection, and the digest
  authenticates neither the producer nor executable content at use time;
  `ResolvedProfile` fields are public and mutable by the embedding caller.
- *Arbitration options:* accept the snapshot semantics as the pinned
  resolution contract, or require revalidation before effects.

### N-2 (LOW, approved-deferred) — credential refusal is heuristic.

- *Locator:* `CONTRACT.md` "Shared registry configuration (ADR 0015)"
  ("inline credentials… fail closed"; "credential-bearing flags/values are
  refused"); `IMPL.md` "Shared registry, setup, and pinned resolution".
- *Observed:* parsing refuses inline credentials and a heuristic list of
  credential-like argument substrings; the record states this is not
  comprehensive secret detection. Unknown identifiers' policy-enablement
  semantics and universal inline-secret exclusion are not established.
- *Arbitration options:* accept the heuristic with the disclosed limit, or
  strengthen detection to the claimed guarantee.

### N-3 (LOW, approved-deferred) — ancestor traversal and concurrent updates
are not fully protected.

- *Locator:* `REQUIREMENTS.md` `AR-REQ-SHARED-RUNTIME` ("Persistence MUST be
  atomic, private, bounded and reject links and special files");
  `IMPL.md` "Shared registry, setup, and pinned resolution".
- *Observed:* the final parent is checked against symlinks and the leaf opens
  no-follow, but the record notes the persistence path does not
  descriptor-walk every ancestor and does not serialize concurrent
  read-modify-write updates.
- *Arbitration options:* accept the current bounds with the caller
  obligation, or require full ancestor verification and update serialization.

### N-4 (LOW, interpretation boundary) — qualification does not verify the
exact `OK` answer.

- *Locator:* `CONTRACT.md` "Behavioral guarantees" ("Setup always tests the
  generated profile with the fixed prompt `Reply with exactly: OK`… setup
  reports only qualification status"); `IMPL.md` "Shared registry, setup, and
  pinned resolution".
- *Observed:* qualification success means the transport/process call
  succeeded; the record states it does not verify the returned answer equals
  `OK`, and does not populate capability-test lists or authorize later work.
- *Arbitration options:* accept transport-success qualification as intended,
  or require exact-answer verification as a distinct recorded outcome.

### N-5 (LOW, broader target unresolved) — typed broker/selected-backend
realization not demonstrated.

- *Locator:* `REQUIREMENTS.md` `AR-REQ-NATIVE-RUNTIME`; `CONTRACT.md`
  "Security and authority" ("Only a typed broker may validate and canonicalize
  arguments, request a host decision, dispatch an allowed request through the
  selected backend"); `DESIGN.md` "Security and resource design" (injected
  executor; protected hosts bind `galla`).
- *Observed:* the recorded loop uses the injected executor with no implicit
  fallback, and the design wording now agrees with the contract (the prior
  ownership contradiction is withdrawn by the comparison); the record does not
  establish the full reusable typed broker/selected-backend realization, and
  protected-host binding remains unverified.
- *Arbitration options:* accept the injected-executor boundary as the current
  realization with the broader target retained, or schedule the typed-broker
  target as explicit work.

### N-6 (LOW, underspecified) — the trajectory API is less restrictive than
the journal guarantee.

- *Locator:* `CONTRACT.md` "Live diagnostic journals and private controls";
  `IMPL.md` "Representative existing helpers" ("the separate trajectory API…
  reopens a pathname, follows normal filesystem resolution, and has no
  comparable record/total size, private-file, sequence, or durability
  enforcement").
- *Observed:* `RunJournal` carries the versioned, bounded, private,
  sequence-enforced guarantees; the older trajectory recorder reopens
  pathnames and enforces none of them. The journal guarantee's breadth in the
  intent does not specify whether the trajectory API is inside or outside it.
- *Arbitration options:* scope the intent's journal guarantees to
  `sav::observation` explicitly, or bring the trajectory API to parity.

### N-7 (LOW, evidence gap) — some required regression evidence not found
in the bounded evidence.

- *Locator:* `REQUIREMENTS.md` "Shared native runtime verification (ADR
  0015)"; `IMPL.md` "Test-source coverage and execution limitations".
- *Observed:* the enumerated bounded evidence covers the required list (see
  C-9); dedicated preparation-failure (context-manager `prepare` failure
  leaving the request unchanged) and some timing-interleaving regressions
  were not found in the named test sources.
- *Arbitration options:* accept the coverage as sufficient, or add the
  named missing regressions.

### N-8 (LOW, mechanism limit) — control/journal exposure limits.

- *Locator:* `CONTRACT.md` "Live diagnostic journals and private controls";
  `IMPL.md` "Bounded live journal and local IPC".
- *Observed:* the record names trusted nonblocking callbacks (the handler is
  not time-limited), no peer-credential authentication, no per-request
  deduplication, no same-UID isolation, metadata checks that cover the socket
  and final parent but not a descriptor-anchored full ancestor walk, and
  payloads that are not safe terminal-rendering output.
- *Arbitration options:* accept the mechanism limits with the private-capability
  contract, or require peer authentication and payload rendering safety.

### N-9 (LOW, approved-deferred) — preconditions and execution evidence are
attributed, not independently reproduced.

- *Locator:* `REVIEW_RUNBOOK.md` (preconditions); the chain task contexts.
- *Observed:* `kvist doctor` records pre-existing blockers (Skott
  `lifecycle-write-tests` timestamp inconsistency, invalid root state/VCS
  readiness, unavailable Kvist-managed toolchain Cargo path) that this review
  does not repair; the 1746-pass workspace run, strict Clippy/fmt/release/docs
  gates, and catalogue smoke are caller-reported and were not independently
  executed by this reviewer.
- *Arbitration options:* accept the attributed evidence with the pre-existing
  blockers recorded, or require a fresh attributed run plus precondition
  repair before sign-off.

## Discrepancy summary

| ID | Clause / finding | Status | Severity |
|----|------------------|--------|----------|
| C-1 | Typed registry, distinct backends, pinned source-bound resolution | compliant | none |
| C-2 | Capability separation; credentials as references | compliant (N-2) | low |
| C-3 | Fail-closed parsing; atomic private link-safe persistence | compliant (N-3) | low |
| C-4 | Shared setup discovery/qualification; `--force` no promotion | compliant (N-4) | low |
| C-5 | Selector bounds shared with the direct transport | compliant | none |
| C-6 | Terminal/maerg independence; injected executor and recording | compliant (N-5) | low |
| C-7 | Bounded ordered steering; renewal; pairing; no replay | compliant | none |
| C-8 | Versioned bounded journals; required-recording stop | compliant (N-6) | low |
| C-9 | Required verification regression evidence | compliant (N-7) | low |
| C-10 | Read-only observers; private validated controls | compliant (N-8) | low |
| C-11 | Explicit external execution boundary; no native promotion | compliant | none |
| C-12 | Bounded supervision; group reap; retained effects | compliant (M-2, N-9) | medium |
| C-13 | Typed broker and redacted runtime events | mismatched (M-1) | high |
| C-14 | Root change-and-compliance process | compliant | none |
| M-1 | Redacted runtime events: no demonstrated owner | mismatched | high |
| M-2 | Singular process-global signal ownership | mismatched | medium |
| N-1 | Resolution digests are in-memory snapshots | evidence limit | low |
| N-2 | Heuristic credential refusal | approved-deferred | low |
| N-3 | Ancestor traversal / concurrent updates | approved-deferred | low |
| N-4 | Qualification does not verify the exact `OK` answer | interpretation boundary | low |
| N-5 | Typed broker/selected-broker realization not demonstrated | broader target unresolved | low |
| N-6 | Trajectory API less restrictive than journal guarantees | underspecified | low |
| N-7 | Missing preparation-failure / timing-interleaving regressions | evidence gap | low |
| N-8 | Control/journal exposure limits | mechanism limit | low |
| N-9 | Attributed execution evidence; pre-existing doctor blockers | evidence limit | low |

**Total hard mismatches:** 2 (M-1 high, M-2 medium)
**Observations retained for arbitration:** 9 (all LOW)

## Conclusion

The ADR 0015 shared registry, runtime, observation and supervision surface of
`sav` corresponds to its intent on thirteen of fourteen clause comparisons;
only C-13 (redacted runtime events, M-1) is classified mismatched, and the
supervision signal-ownership finding (M-2) is retained on the otherwise
compliant-per-run C-12. Both are retained for explicit human arbitration,
alongside nine LOW observations. No clause was classified **approved-deferred**
in its own right — each deferred observation is retained for arbitration. This
record is the independent gate disposition for the `shared-runtime` chain; it
is not self-certified, establishes no clause-level certification beyond the
record above, and does not constitute acceptance or sign-off. Formal sign-off
is blocked while M-1 and M-2 remain unarbitrated; acceptance, if required,
remains subject to the separate review receipt / acknowledgement or a
reasoned exception per the change-and-compliance rules.
