//! Security audit: decision authorization and recovery controls
//!
//! This module documents observed authorization boundaries, recovery fencing,
//! input guards, and remaining limitations in the Sav component.
//!
//! Findings are observational — they describe what the code does, not what
//! it should do. Self-certification is avoided; each finding references
//! concrete source locations.

// === DECISION AUTHORIZATION ===

/// **Finding:** Tool execution is never authorized by Sav itself.
/// The `ToolExecutor` trait is supplied by the embedding host, and Sav
/// delegates all effect authorization to it.
///
/// Source: `src/runtime/session.rs` — `ToolExecutor` trait definition
/// Source: `src/runtime/driver.rs` — `fold_outcome` calls `executor.execute`
///
/// **Assessment:** Correct separation of concerns. Sav provides the loop
/// machinery but does not grant authority over which tools may execute.
pub const AUTHORIZATION_DELEGATED: &str =
    "Tool execution authorized by host-supplied ToolExecutor, not by Sav";

/// **Finding:** Guidance submission is atomically classified against run state.
/// `ControlMailbox::submit` returns either `SubmissionKind::Task` (idle) or
/// `SubmissionKind::Guidance` (running), preventing race conditions where
/// guidance could be misclassified as a task.
///
/// Source: `src/runtime/control.rs` — `enqueue` method
///
/// **Assessment:** Correct. The classification is performed under a single
/// mutex lock, ensuring consistency.
pub const GUIDANCE_CLASSIFICATION_ATOMIC: &str =
    "Guidance vs task classification is atomic under mutex";

/// **Finding:** Guidance cannot be edited or withdrawn after claim.
/// `ControlMailbox::edit` and `withdraw` check the `claimed` list and fail
/// if the message has already been claimed by the worker.
///
/// Source: `src/runtime/control.rs` — `edit`, `withdraw` methods
///
/// **Assessment:** Correct. This prevents mid-execution tampering with
/// guidance that the worker has already begun processing.
pub const GUIDANCE_IMMUTABLE_AFTER_CLAIM: &str = "Guidance cannot be edited/withdrawn after claim";

/// **Finding:** External process output does not become native tool intent.
/// The supervisor captures output from external agents but does not
/// interpret it as tool proposals. Only the native model loop generates
/// `ToolIntent` objects.
///
/// Source: `src/supervisor.rs` — `run_supervised` returns `ExecutionReport`
/// Source: `src/runtime/session.rs` — `apply_assistant` generates intents
///
/// **Assessment:** Correct boundary. External agents cannot mint task
/// approval or inject tool calls into the native loop.
pub const EXTERNAL_OUTPUT_NOT_TOOL_INTENT: &str =
    "External process output does not become native tool intent";

// === RECOVERY FENCING ===

/// **Finding:** Recovery budget is bounded and non-resettable.
/// `RecoveryBudget::take()` returns at most two numbered steps per prompt.
/// There is no `reset()` method, so the budget cannot be replenished
/// within a single prompt's lifetime.
///
/// Source: `src/loop_detection/mod.rs` — `RecoveryBudget`
///
/// **Assessment:** Correct. This prevents infinite recovery attempts and
/// ensures that persistent stalls eventually terminate the prompt.
pub const RECOVERY_BUDGET_BOUNDED: &str =
    "Recovery budget limited to 2 steps per prompt, non-resettable";

/// **Finding:** Tool loop detection uses bounded history.
/// `ToolLoopTracker` retains 128 action fingerprints and detects cycles
/// up to 32 actions. This bounds memory usage while still catching
/// common repetition patterns.
///
/// Source: `src/loop_detection/mod.rs` — `ToolLoopTracker`
///
/// **Assessment:** Reasonable bounds. Longer cycles or more diverse
/// repetition may escape detection, but this is a documented limitation.
pub const TOOL_LOOP_HISTORY_BOUNDED: &str =
    "Tool loop detection retains 128 fingerprints, 32-action cycle window";

/// **Finding:** Progress evidence must be host-validated.
/// `ProgressEvidence::StateChanged` requires the host to have validated
/// a revision change; it is not inferred from the tool's own success claim.
/// `ToolExecutor::progress` defaults to `None`.
///
/// Source: `src/loop_detection/mod.rs` — `ProgressEvidence` enum
/// Source: `src/runtime/session.rs` — `ToolExecutor::progress` default
///
/// **Assessment:** Correct. This prevents a tool from self-certifying
/// progress to escape loop detection.
pub const PROGRESS_HOST_VALIDATED: &str =
    "Progress evidence requires host validation, not tool self-claim";

/// **Finding:** Operation tokens are fresh per provider request.
/// `Operation::start` creates a new cancellation token for each provider
/// request, which mirrors prompt cancellation, shutdown, and pending
/// guidance. It never resets the whole-prompt token.
///
/// Source: `src/runtime/driver.rs` — `Operation::start`
///
/// **Assessment:** Correct. This allows fine-grained cancellation of
/// individual model requests without affecting the overall prompt lifecycle.
pub const OPERATION_TOKENS_FRESH: &str =
    "Fresh provider-operation token per request, never resets prompt token";

/// **Finding:** Wall time and token budgets are tracked cumulatively.
/// `PromptBudget` tracks wall time from prompt start, and `AttemptUsage`
/// accumulates token usage across all turns. These are not reset on retries.
///
/// Source: `src/runtime/driver.rs` — `PromptBudget`, `AttemptUsage`
///
/// **Assessment:** Correct. This prevents budget evasion through retries.
pub const BUDGETS_CUMULATIVE: &str = "Wall time and token budgets are cumulative across retries";

// === PATH AND INPUT GUARDS ===

/// **Finding:** Registry files are bounded in size and complexity.
/// Registry TOML files are limited to 64 KiB, with at most 128 profiles.
/// Profile names are validated against a strict ASCII pattern.
///
/// Source: `src/registry.rs` — validation logic
///
/// **Assessment:** Correct. This prevents resource exhaustion from
/// maliciously large or complex registry files.
pub const REGISTRY_BOUNDED: &str =
    "Registry files bounded to 64 KiB, 128 profiles, validated names";

/// **Finding:** Command rendering is bounded.
/// `render_command` produces at most 256 argv entries and 2 MiB of total
/// argument bytes. The prompt substitution is bounded to 1 MiB.
///
/// Source: `src/command.rs` — `render_command`
///
/// **Assessment:** Correct. This prevents argument injection and resource
/// exhaustion through oversized command templates.
pub const COMMAND_RENDERING_BOUNDED: &str =
    "Command rendering bounded to 256 entries, 2 MiB total, 1 MiB prompt";

/// **Finding:** Journal records are bounded.
/// Each journal record is limited to 64 KiB, and segments to 64 MiB.
/// Invalid records are rejected before writing.
///
/// Source: `src/observation/journal.rs` — `RunJournal`
///
/// **Assessment:** Correct. This prevents unbounded journal growth from
/// single oversized records.
pub const JOURNAL_RECORDS_BOUNDED: &str = "Journal records bounded to 64 KiB, segments to 64 MiB";

/// **Finding:** Control socket messages are bounded.
/// Socket messages are limited to 20 KiB, with individual I/O waits and
/// the complete exchange bounded by timeouts.
///
/// Source: `src/observation/control.rs` — `ControlServer`
///
/// **Assessment:** Correct. This prevents resource exhaustion through
/// oversized or slow control messages.
pub const CONTROL_MESSAGES_BOUNDED: &str =
    "Control socket messages bounded to 20 KiB with timeouts";

// === APPROVAL SCOPE ===

/// **Finding:** Sav does not own task state or policy decisions.
/// The component explicitly delegates task state, policy decisions, grants,
/// and approved resources to the embedding host.
///
/// Source: `CONTRACT.md` — "Boundary and ownership" section
///
/// **Assessment:** Correct scope. Sav provides reusable infrastructure
/// without claiming authority over application-level decisions.
pub const SAV_NO_TASK_STATE_OWNERSHIP: &str =
    "Sav does not own task state, policy decisions, or grants";

/// **Finding:** No automatic tool replay authorization.
/// Tool failures are categorized but no failure classification authorizes
/// replay. The host must explicitly decide whether to retry.
///
/// Source: `src/runtime/diagnostics.rs` — `ToolFailureCategory`
/// Source: `CONTRACT.md` — "No failure classification authorizes replay"
///
/// **Assessment:** Correct. This prevents unintended side effects from
/// automatic retries of non-idempotent operations.
pub const NO_AUTO_REPLAY_AUTHORIZATION: &str =
    "No tool failure classification authorizes automatic replay";

/// **Finding:** Guidance delivery is durable before next provider request.
/// The recorder persists the delivered guidance state before the next
/// provider request is made, ensuring recovery can reconstruct the
/// conversation state.
///
/// Source: `src/runtime/driver.rs` — `deliver_guidance`
///
/// **Assessment:** Correct. This provides a crash-consistent recovery point.
pub const GUIDANCE_DELIVERY_DURABLE: &str =
    "Guidance delivery persisted before next provider request";

// === RETIREMENT RECORDS ===

/// **Finding:** Run journals provide durable retirement records.
/// `RunJournal` creates a no-clobber regular file with `run_start` and
/// `run_end` events. Sequence numbers are contiguous and one-based.
///
/// Source: `src/observation/journal.rs` — `RunJournal`
///
/// **Assessment:** Correct. This provides a complete, ordered record of
/// each run's lifecycle.
pub const JOURNAL_RETIREMENT_RECORDS: &str =
    "Run journals provide durable, ordered retirement records";

/// **Finding:** Failed journal writes poison the writer.
/// If a journal write fails, subsequent writes also fail, preventing
/// silent data loss.
///
/// Source: `src/observation/journal.rs` — `RunJournal`
///
/// **Assessment:** Correct. This ensures that journal integrity failures
/// are detected and propagated.
pub const JOURNAL_WRITE_POISONING: &str =
    "Failed journal writes poison the writer, preventing silent loss";

/// **Finding:** Dropping the journal does not invent terminal results.
/// If the journal is dropped without an explicit `finish`, no terminal
/// event is fabricated.
///
/// Source: `src/observation/journal.rs` — `RunJournal::drop`
///
/// **Assessment:** Correct. This prevents false completion records.
pub const JOURNAL_NO_FAKE_TERMINATION: &str = "Dropping journal does not fabricate terminal result";

// === EXECUTION-BACKEND DELEGATION ===

/// **Finding:** The installed galla runner is resolved to a canonical,
/// regular, non-symlink file and content-hashed before any effect is
/// dispatched. A missing, unreadable, or symlinked runner is a
/// `RunnerUnavailable` refusal with no unconstrained host fallback.
///
/// Source: `src/runtime/execution_backend.rs` — `resolve_runner`
///
/// **Assessment:** Correct. The runner identity is bound to the exact
/// executable bytes, preventing substitution attacks.
pub const RUNNER_RESOLUTION_CANONICAL: &str =
    "Runner resolved to canonical regular non-symlink file, content-hashed before dispatch";

/// **Finding:** The runner's capability probe is bounded by a fixed deadline
/// (30 s) and a maximum response size (64 KiB). A runner that times out,
/// emits diagnostics on standard error, declares an unsupported protocol or
/// version, reports a non-Bubblewrap backend, or confirms incomplete
/// namespace/session capabilities is a `BackendUnavailable` refusal.
///
/// Source: `src/runtime/execution_backend.rs` — `probe`
///
/// **Assessment:** Correct. The probe is fail-closed: any deviation from the
/// version-one Bubblewrap attestation refuses before dispatch.
pub const PROBE_TRUST_FAIL_CLOSED: &str =
    "Capability probe bounded by deadline and size, fail-closed on any deviation";

/// **Finding:** The probe response is deserialized with `deny_unknown_fields`,
/// so an unrecognized or omitted capability is rejected rather than silently
/// accepted. The runner digest the probe self-attests is compared against the
/// bound identity; a mismatch is a `RunnerUnavailable` refusal.
///
/// Source: `src/runtime/execution_backend.rs` — `ProbeWire`, `ExecutionBackend::new`
///
/// **Assessment:** Correct. The closed wire shape prevents a runner from
/// smuggling extra fields or omitting required capabilities.
pub const PROBE_RESPONSE_CLOSED: &str =
    "Probe response is closed (deny_unknown_fields) and runner digest is cross-checked";

/// **Finding:** The version-one request is rendered with a fixed protocol
/// identifier (`kvist-sandbox-request-v1`), a fixed phase (`authoring`), and
/// approval-bound identities (runner, backend, command, mount plan, policy,
/// toolchain) recomputed with the canonical encodings the runner re-digests.
/// The runner independently verifies these identities before enforcement.
///
/// Source: `src/runtime/execution_backend.rs` — `render_request`, `command_identity`, `mount_plan_identity`
/// Source: `galla/src/enforcement.rs` — `verify_request_identities`
///
/// **Assessment:** Correct. The identity binding is end-to-end: the producer
/// and the runner both recompute the same digests, so a tampered request is
/// refused before any effect.
pub const PROTOCOL_BINDING_END_TO_END: &str =
    "Request identities recomputed by both producer and runner, end-to-end binding";

/// **Finding:** The sandbox environment starts empty (`--clearenv`) and only
/// explicitly mapped variables are set. The `HOME` and `PATH` variables are
/// fixed to sandbox destinations. The galla validation layer rejects
/// dangerous environment names (`LD_*`, `*_PROXY`, `GIT_ASKPASS`,
/// `CARGO_REGISTRY_TOKEN`, etc.) in non-authoring phases, preventing
/// credential and loader injection.
///
/// Source: `galla/src/enforcement.rs` — bwrap argument construction (`--clearenv`, `--setenv`)
/// Source: `galla/src/validation.rs` — `validate_environment_shape`, `is_dangerous_environment_name`
///
/// **Assessment:** Correct. The environment is closed by default; only
/// explicitly approved variables are mapped, and dangerous names are
/// rejected in phases where they could exfiltrate credentials or redirect
/// the dynamic linker.
pub const ENVIRONMENT_CLOSED_BY_DEFAULT: &str =
    "Sandbox environment starts empty, only explicit variables mapped, dangerous names rejected";

/// **Finding:** The delegated effect is supervised with a wall-time deadline
/// (default 15 minutes) and a combined output capture limit (default 1 MiB).
/// The supervision loop polls cancellation and the deadline at a 10 ms
/// interval, draining pipes non-blockingly so a silent runner cannot block
/// the loop indefinitely.
///
/// Source: `src/runtime/execution_backend.rs` — `delegate`, `drain_bounded`, `set_nonblocking`
///
/// **Assessment:** Correct. Both wall time and output are bounded, and the
/// supervision loop cannot be blocked by a runner that produces no output.
pub const DELEGATION_TIMEOUT_AND_OUTPUT_BOUNDED: &str =
    "Delegated effect supervised with wall-time deadline and output capture limit";

/// **Finding:** On every delegation outcome — success, timeout, cancellation,
/// or error — the complete runner process group is reaped with `SIGKILL`
/// before the outcome is returned. No runner descendant outlives the
/// delegation.
///
/// Source: `src/runtime/execution_backend.rs` — `delegate` (killpg on every path)
///
/// **Assessment:** Correct. Cleanup is unconditional; the process group is
/// killed before the outcome is constructed, so no orphaned runner process
/// can outlive the delegation.
pub const DELEGATION_CLEANUP_UNCONDITIONAL: &str =
    "Runner process group reaped with SIGKILL on every delegation outcome";

/// **Finding:** When the runner or its backend is unavailable, the backend
/// refuses before dispatch with `RunnerUnavailable` or `BackendUnavailable`.
/// The runtime surfaces the refusal, records the effect as not executed
/// (`tools_executed == 0`), and never substitutes an unconstrained host
/// execution. The `rebind` check re-hashes the runner and backend
/// immediately before dispatch and fails closed when either identity has
/// drifted since the probe bound it.
///
/// Source: `src/runtime/execution_backend.rs` — `ExecutionBackend::new`, `rebind`, `ToolExecutor::execute`
/// Source: `sav/tests/execution_backend_delegation.rs` — fail-closed tests
///
/// **Assessment:** Correct. The fail-closed refusal path is end-to-end:
/// the backend refuses, the runtime surfaces the refusal, and no host
/// execution is substituted. The `rebind` check prevents a runner or
/// backend that is replaced between the probe and the dispatch from
/// silently changing the enforcement backend.
pub const FAIL_CLOSED_NO_HOST_FALLBACK: &str =
    "Unavailable runner or backend refuses before dispatch, no unconstrained host fallback";

/// **Finding:** The workspace directory mounted read-write at `/workspace`
/// is a host-authority value bound at construction
/// (`ExecutionBackend::new`); the model's intent argument can no longer
/// redirect the read-write grant to an arbitrary host path. The `/usr` tree
/// is mounted read-only at `/usr`. The mount plan identity is recomputed
/// from these grants and verified by the runner before enforcement. The
/// workspace vendor mask (`/workspace/.kvist/vendored`) is additionally
/// masked read-only in the authoring phase, preventing the sandbox from
/// modifying the vendored dependency material.
///
/// Source: `src/runtime/execution_backend.rs` — `ExecutionBackend::new` (workspace binding), `render_request` (grants, mount plan)
/// Source: `galla/src/enforcement.rs` — `add_workspace_vendor_mask`
///
/// **Assessment:** Correct. The workspace grant source is host-bound, so the
/// model cannot redirect the read-write mount to an arbitrary host directory;
/// the mount plan is approval-bound and the workspace vendor mask prevents
/// the sandbox from tampering with the read-only vendored registry.
pub const MOUNT_PLAN_APPROVAL_BOUND: &str =
    "Mount plan identity recomputed and verified, workspace vendor masked read-only";

/// **Finding:** The workspace directory and the extra environment variables
/// are host-authority values bound at `ExecutionBackend::new` construction.
/// The model's intent argument supplies only the command line (`argv`); a
/// model-supplied `working_directory` or `environment` object is ignored.
/// This closes two confused-deputy paths: the model could not redirect the
/// read-write workspace grant to an arbitrary host path, and it could not
/// inject loader or credential variables (e.g. `LD_PRELOAD`,
/// `CARGO_REGISTRY_TOKEN`) into the sandbox environment.
///
/// Source: `src/runtime/execution_backend.rs` — `ExecutionBackend::new`, `render_request`
///
/// **Assessment:** Correct. Both the workspace grant source and the extra
/// environment variables are bound by the host at construction, so the
/// untrusted model cannot widen its own authority by redirecting the
/// read-write mount or injecting environment variables. Only the command
/// line remains model-controlled.
pub const WORKSPACE_AND_ENVIRONMENT_HOST_BOUND: &str = "Workspace grant and extra environment variables are host-bound at construction; the model supplies only argv";

// === REMAINING LIMITATIONS ===

/// **Limitation:** Loop detection is heuristic.
/// Both generation and tool loop detection use heuristic methods (n-gram
/// similarity, action fingerprinting) that can produce false positives
/// or miss subtle repetition patterns.
///
/// Source: `src/loop_detection/mod.rs`
///
/// **Impact:** Occasional unnecessary corrections or missed stalls.
pub const LOOP_DETECTION_HEURISTIC: &str =
    "Loop detection uses heuristics that may false-positive or miss subtle patterns";

/// **Limitation:** Tool loop detection cannot observe filesystem changes.
/// The detector only sees tool invocations and outcomes, not the actual
/// state of the filesystem. A tool that makes changes but reports success
/// may not be detected as making progress.
///
/// Source: `src/loop_detection/mod.rs` — `ProgressEvidence`
///
/// **Impact:** Tools that silently fail to make changes may loop undetected.
pub const TOOL_LOOP_NO_FILESYSTEM_OBSERVATION: &str =
    "Tool loop detection cannot observe filesystem changes";

/// **Limitation:** Recovery budget is per-prompt, not per-session.
/// Each new prompt gets a fresh recovery budget, so a session with many
/// prompts can consume many recovery steps in total.
///
/// Source: `src/runtime/driver.rs` — `RecoveryBudget` created per `run_inner`
///
/// **Impact:** Long sessions with many prompts may use more recovery steps
/// than intended.
pub const RECOVERY_BUDGET_PER_PROMPT: &str = "Recovery budget resets per prompt, not per session";

/// **Limitation:** Guidance text is not validated for structure.
/// Guidance messages are validated for size and NUL bytes, but not for
/// any structural or semantic correctness.
///
/// Source: `src/runtime/control.rs` — `validate` function
///
/// **Impact:** Malformed guidance may confuse the model or cause unexpected
/// behavior.
pub const GUIDANCE_NO_STRUCTURE_VALIDATION: &str =
    "Guidance text validated only for size and NUL bytes, not structure";

/// **Limitation:** No cryptographic authentication of journal records.
/// Journal records are written as JSON lines without cryptographic
/// signatures or hashes. Tampering is detectable through sequence number
/// gaps but not through content modification.
///
/// Source: `src/observation/journal.rs`
///
/// **Impact:** Journal integrity relies on filesystem permissions rather
/// than cryptographic verification.
pub const JOURNAL_NO_CRYPTO_AUTHENTICATION: &str =
    "Journal records lack cryptographic authentication";

/// **Limitation:** The probe qualifies the backend with `--unshare-all`
/// (including cgroup), but the enforcement path uses `--unshare-cgroup-try`,
/// which tolerates cgroup unshare failure. The reported `Namespaces`
/// capabilities do not include cgroup, so the probe's `complete()` check does
/// not require it. The probe is therefore stricter than the enforcement: a
/// host that fails the probe's cgroup requirement is refused, but a host that
/// passes the probe may still run the enforcement without cgroup isolation if
/// cgroup unshare is flaky at enforcement time.
///
/// Source: `galla/src/probe.rs` — `qualify_backend` (`--unshare-all`)
/// Source: `galla/src/enforcement.rs` — bwrap argument construction (`--unshare-cgroup-try`)
///
/// **Impact:** On a host with flaky cgroup support, the enforcement may run
/// without cgroup isolation even though the probe confirmed it. This is not
/// a high-confidence escape (the other namespaces are still enforced) but is
/// a silent degradation of the cgroup isolation guarantee.
pub const CGROUP_PROBE_ENFORCEMENT_DISCREPANCY: &str = "Probe requires cgroup unshare (--unshare-all) but enforcement tolerates its failure (--unshare-cgroup-try)";

/// **Limitation:** The probe honors a `GALLA_TEST_BACKEND` environment
/// override that lets it exercise a non-trusted executable (for example a
/// failing stub). The override is honored only in the advisory capability
/// probe; the enforcement path re-locates the backend independently and never
/// consults this variable. A test could therefore pass the probe with a stub
/// backend while the enforcement uses the real one.
///
/// Source: `galla/src/probe.rs` — `probe` (`GALLA_TEST_BACKEND`)
/// Source: `galla/src/enforcement.rs` — `locate_backend` (independent re-location)
///
/// **Impact:** The probe's capability attestation may not reflect the backend
/// the enforcement actually uses. This is by design for testing and is not a
/// host-escape, but it means the probe's trust is advisory, not binding on
/// the enforcement path.
pub const PROBE_TEST_BACKEND_OVERRIDE: &str =
    "Probe honors a test-only backend override that the enforcement path ignores";

/// **Limitation:** The runner recomputes the runner, command, and mount-plan
/// identities from the request content but only shape-checks the policy
/// identity. The policy identity is a deterministic function of the command
/// identity (which is recomputed), so a tampered policy identity cannot
/// decouple the approval from the approved command. The host is trusted to
/// have set a consistent policy identity. This is not a bypass, but the
/// runner does not independently verify the policy identity against the
/// command identity.
///
/// Source: `galla/src/enforcement.rs` — `verify_request_identities`
/// Source: `galla/src/validation.rs` — `validate_identities`
/// Source: `sav/src/runtime/execution_backend.rs` — `render_request` (policy identity)
///
/// **Impact:** The policy identity is host-asserted and not independently
/// recomputed by the runner. A host that sets an inconsistent policy identity
/// would not be caught by the runner, but the command identity (which the
/// policy identity is derived from) is recomputed, so the approval is still
/// bound to the approved command.
pub const POLICY_IDENTITY_HOST_ASSERTED: &str = "Runner recomputes runner/command/mount-plan identities but only shape-checks the policy identity";

/// **Limitation:** The runner's environment validation rejects dangerous
/// environment names (`LD_*`, `*_PROXY`, `GIT_ASKPASS`, `CARGO_REGISTRY_TOKEN`,
/// etc.) in non-authoring phases but allows them in the authoring phase. The
/// authoring environment is host-bound (the host supplies the extra environment
/// at construction, and the model's environment argument is ignored), so the
/// host controls the authoring environment and would not set a dangerous name.
/// This is not a bypass, but the authoring phase relaxes the check.
///
/// Source: `galla/src/validation.rs` — `validate_environment_shape`, `is_dangerous_environment_name`
/// Source: `sav/src/runtime/execution_backend.rs` — `render_request` (environment)
///
/// **Impact:** The authoring phase allows dangerous environment names because
/// the environment is host-bound. A host that sets a dangerous environment name
/// in the authoring phase would not be caught by the runner, but the host is
/// trusted to have set a safe environment.
pub const AUTHORING_ENVIRONMENT_HOST_BOUND: &str = "Authoring phase relaxes the dangerous-environment-name check because the environment is host-bound";

// === PROVIDER CLI QUALIFICATION ===

/// **Finding:** Executable discovery is bounded by a 10-second idle and
/// attempt timeout with a 64 KiB output limit and zero retries. A provider
/// executable that hangs, emits repeating output, or exceeds the output bound
/// is refused before any profile is persisted.
///
/// Source: `src/setup.rs` — `probe_cli_version` (idle 10 s, attempt 10 s,
/// 64 KiB, `max_retries: 0`, `detect_loops: false`)
///
/// **Assessment:** Correct. The probe is fail-closed: any timeout, output
/// limit, or non-zero exit status is a refusal. The `detect_loops: false`
/// setting is acceptable because the 10-second attempt timeout bounds a
/// repeating-output provider to 10 seconds of wall time.
pub const EXECUTABLE_DISCOVERY_BOUNDED: &str =
    "Executable discovery bounded by 10 s idle/attempt, 64 KiB output, zero retries";

/// **Finding:** Fallback executable paths are validated as regular,
/// non-symlink files before and after canonicalization. A symlinked or
/// non-regular fallback path is refused before any probe or qualification.
///
/// Source: `src/setup.rs` — `resolve_setup_file` (validate_regular_file before
/// and after canonicalize), `validate_custom_executable` (executable bit check)
///
/// **Assessment:** Correct. The double validation closes the TOCTOU window in
/// which a locally writable parent directory lets an attacker swap a regular
/// file for a link to a privileged file between the initial check and the
/// canonicalize. The executable-bit check ensures the fallback is actually
/// runnable.
pub const FALLBACK_PATH_VALIDATED: &str =
    "Fallback paths validated as regular non-symlink files before and after canonicalization";

/// **Finding:** The supervised command's stdin is set to `Stdio::null()`, so a
/// probed or qualified provider executable cannot block on interactive stdin.
/// The supervisor's own stdin is read via `read_input`, which treats EOF as
/// `Error::Cancelled` and treats `\x1b`/`cancel`/`q` as cancellation.
///
/// Source: `src/supervisor.rs` — `run_attempt` (`.stdin(Stdio::null())`)
/// Source: `src/setup.rs` — `read_input` (EOF → `Error::Cancelled`)
///
/// **Assessment:** Correct. A provider executable that tries to read from
/// stdin receives immediate EOF, not a blocking read. The supervisor's own
/// stdin handling is cancellation-safe: EOF and escape sequences are treated
/// as cancellation, not as I/O errors.
pub const STDIN_HANDLING_NULL_AND_CANCELLATION_SAFE: &str =
    "Supervised command stdin is null; supervisor stdin treats EOF and escape as cancellation";

/// **Finding:** Generated permission flags in external-agent profiles are
/// checked for inline credentials. The `secret_argument` function rejects any
/// argv entry containing `api-key`, `access-token`, `password`,
/// `authorization`, `bearer `, `--token=`, `--secret`, `credential`,
/// `private_key`, `private-key`, `client_secret`, `client-secret`, or `cookie`.
///
/// Source: `src/registry.rs` — `Profile::validate` (calls `secret_argument`
/// on each argv entry)
///
/// **Assessment:** Correct. The credential check is applied to every argv
/// entry in the external-agent profile, not just the first. A profile that
/// embeds a credential in any argument is refused before persistence.
pub const GENERATED_PERMISSION_FLAGS_CREDENTIAL_CHECKED: &str =
    "Generated permission flags checked for inline credentials in every argv entry";

/// **Finding:** Model selectors are bounded to 1–256 printable ASCII bytes
/// without braces or quotes. The `validate_model_id` function rejects empty,
/// oversized, non-printable, or brace/quote-bearing model IDs. The
/// `validate_model_selector` function rejects blank, oversized, or
/// control-bearing model selectors.
///
/// Source: `src/catalog.rs` — `validate_model_id` (1–256 printable ASCII, no
/// braces)
/// Source: `src/direct_transport.rs` — `validate_model_selector` (nonblank,
/// control-free ASCII ≤ 256 bytes)
///
/// **Assessment:** Correct. The model ID bound prevents a model ID from
/// smuggling shell metacharacters or template placeholders into the rendered
/// command. The brace/quote rejection prevents a model ID from escaping the
/// template's quoting context.
pub const MODEL_SELECTORS_BOUNDED: &str =
    "Model selectors bounded to 1–256 printable ASCII without braces or quotes";

/// **Finding:** Exact command verification renders the stored command through
/// `render_command` (shell-free, quoted) and runs it under bounded supervision
/// with a 30-second idle timeout, 300-second attempt timeout, 64 KiB output
/// limit, zero retries, and loop detection. The `command_spec` function
/// substitutes `{prompt}`, `{model}`, `{cwd}`, `{reasoning_effort}`, and
/// `{context_files}` without a shell.
///
/// Source: `src/setup.rs` — `verify_profile` (renders via `render_command`,
/// runs under `run_supervised_capture` with 30 s / 300 s / 64 KiB / 0 retry /
/// `detect_loops: true`)
/// Source: `src/registry.rs` — `ExternalAgent::command_spec` (shell-free
/// substitution, bounded to 256 entries / 2 MiB / no NUL)
///
/// **Assessment:** Correct. The exact-command verification is fail-closed:
/// any timeout, output limit, loop detection, or non-zero exit status is a
/// refusal. The shell-free rendering prevents command injection through the
/// prompt or model ID. The zero-retry policy prevents automatic replay of a
/// non-idempotent qualification.
pub const EXACT_COMMAND_VERIFICATION_FAIL_CLOSED: &str =
    "Exact command verification is shell-free, bounded, and fail-closed with zero retries";

/// **Finding:** Error redaction is streaming longest-match at UTF-8
/// boundaries. The `StreamRedactor` absorbs deltas, scans for the longest
/// matching secret, and emits `[REDACTED]` in its place. The carry buffer
/// retains a suffix that could be the prefix of a longer secret, so a secret
/// split across two deltas is still redacted.
///
/// Source: `maerg/src/agent/redaction.rs` — `StreamRedactor::absorb` /
/// `StreamRedactor::scan` (longest-match, UTF-8 boundary carry)
///
/// **Assessment:** Correct. The streaming redaction is split-safe: a secret
/// that spans two deltas is held in the carry buffer until the full match is
/// available. The longest-match rule prevents a shorter secret from being
/// redacted in preference to a longer one that shares a prefix.
pub const ERROR_REDACTION_STREAMING_AND_SPLIT_SAFE: &str =
    "Error redaction is streaming longest-match at UTF-8 boundaries, split-safe across deltas";

/// **Limitation:** The `sav setup` command treats the setup action itself as
/// the host-execution acknowledgement without a separate
/// `--allow-host-execution` flag, while `sav run` and `sav models` require
/// explicit `--allow-host-execution` and `--allow-host-discovery` flags
/// respectively. The asymmetry is by design: the setup action is an explicit
/// operator choice that acknowledges host discovery and qualification, not
/// any later external-agent execution. However, a non-interactive host that
/// invokes `sav setup` without an interactive terminal will still perform
/// host discovery and qualification without a separate acknowledgement flag.
///
/// Source: `src/setup.rs` — `run_registry_setup` (no `--allow-host-execution`
/// flag; the setup action is the acknowledgement)
/// Source: `src/main.rs` — `run` (requires `--allow-host-execution`),
/// `models` (requires `--allow-host-discovery`)
///
/// **Impact:** A non-interactive host that invokes `sav setup` will perform
/// host discovery and qualification without a separate acknowledgement flag.
/// This is not a high-confidence escape (the setup action is still an
/// explicit operator choice) but is a silent degradation of the
/// acknowledgement boundary compared to `sav run` and `sav models`.
pub const SETUP_HOST_ACKNOWLEDGEMENT_ASYMMETRY: &str = "sav setup treats the setup action as the host-execution acknowledgement without a separate flag, unlike sav run and sav models";

// === DIRECT MODEL TRANSPORT ===

/// **Finding:** The direct transport's destination is fixed to a numeric
/// loopback address and cannot escape it. `parse_endpoint` requires an
/// ASCII `http://` endpoint with no user information, query, fragment, or
/// provider path, then rejects any host that is not a numeric loopback IP
/// (`IpAddr::from_str` followed by `ip.is_loopback()`). A hostname, a
/// non-loopback literal, or a missing port is refused at construction, before
/// any socket is opened. The `Host` header is the same bounded authority, so
/// a crafted endpoint cannot redirect the request to another destination.
///
/// Source: `src/direct_transport.rs` — `parse_endpoint`, `split_authority`,
/// `parse_port` (numeric loopback only)
/// Source: `sav/tests/model_transport.rs` — `rejects_non_loopback_and_non_http_endpoints`
///
/// **Assessment:** Correct. Destination escape is impossible: the only
/// connectable socket address is a loopback literal, and no redirect or
/// alternate authority is ever followed.
pub const TRANSPORT_LOOPBACK_ONLY: &str = "Direct transport binds to a numeric loopback address; no destination escape or redirect following";

/// **Finding:** The direct transport follows no HTTP redirects. It issues a
/// single `POST` with `Connection: close` and reads exactly one response. A
/// non-2xx status is surfaced as `ModelProviderStatus { status }` with the
/// response body intentionally omitted; the transport never re-issues the
/// request against a `Location` header or any other destination.
///
/// Source: `src/direct_transport.rs` — `execute_with_body`, `read_response`
/// (single request, `Connection: close`, non-2xx → `ModelProviderStatus`)
///
/// **Assessment:** Correct. There is no redirect-handling code path, so a
/// provider cannot steer the request off loopback or to a second host.
pub const TRANSPORT_NO_REDIRECT_FOLLOWING: &str =
    "Direct transport issues one request and follows no redirect";

/// **Finding:** The direct transport routes no credentials. It sends only
/// `Host`, `Content-Type`, `Accept`, `Content-Length`, and `Connection`
/// headers — never an `Authorization` header, cookie, or bearer token — and
/// it is constructed from a plain loopback endpoint string, not from a
/// credential-bearing URL. The model selector is bounded to 1–256 control-free
/// ASCII bytes and is percent-encoded (`query_component`) before it appears in
/// the `/props?model=` query, so a selector cannot smuggle a header line or
/// inject a credential into the request.
///
/// Source: `src/direct_transport.rs` — `execute_with_body` (fixed header set),
/// `validate_model_selector`, `query_component`
///
/// **Assessment:** Correct. No credential material is read, embedded, or
/// echoed; the model selector is bounded and percent-encoded before use.
pub const TRANSPORT_NO_CREDENTIAL_ROUTING: &str = "Direct transport sends no Authorization/cookie header and percent-encodes the bounded model selector";

/// **Finding:** Every untrusted input to the direct transport is bounded.
/// Response headers are capped at 64 KiB (`MAX_HEADER_BYTES`); each SSE/NDJSON
/// record at 1 MiB (`MAX_RECORD_BYTES`); the whole response body at the
/// configured `max_response_bytes` (hard ceiling 16 MiB); the serialized
/// request at 8 MiB (`MAX_REQUEST_BYTES`); the canonical message text at 8
/// MiB (`MAX_MESSAGE_BYTES`); the output schema at 256 KiB / 32 levels / 4096
/// nodes; and the message and tool counts at 1024 and 128 respectively.
/// Chunked bodies are bounded both by the declared size and by the running
/// total before any byte is delivered.
///
/// Source: `src/direct_transport.rs` — `read_response`, `read_chunked_body`,
/// `validate_request`, `validate_output_schema`, `checked_text_total`
///
/// **Assessment:** Correct. No unbounded input path exists; every counter is
/// checked before the corresponding allocation or delivery.
pub const TRANSPORT_INPUT_BOUNDED: &str =
    "Direct transport bounds headers, records, body, request, messages, schema, and counts";

/// **Finding:** Both stream decoders require an explicit terminal record
/// before they assemble a turn, so a truncated stream cannot be reported as a
/// complete success. The OpenAI decoder sets `terminal` only on `[DONE]` and
/// `finish` returns `MalformedModelResponse` when it is absent; the Ollama
/// decoder sets `terminal` only on `done: true` and likewise fails without it.
/// A data payload after the terminal marker is a `MalformedModelResponse`, and
/// an SSE `error` event or a chunk with an `error` field is a
/// `MalformedModelResponse`, so a provider that reports failure cannot be
/// folded into a silent success.
///
/// Source: `src/direct_transport.rs` — `OpenAiStreamState::finish`,
/// `OllamaStreamState::finish`, `process_data_payload` (terminal + error)
/// Source: `sav/tests/model_transport.rs` — `streaming_without_terminal_record_fails`,
/// `ollama_stream_requires_terminal_record_and_emits_complete_intent`
///
/// **Assessment:** Correct. No false terminal success: a stream that never
/// reaches its terminal record, or that reports an error, is a failure.
pub const TRANSPORT_TERMINAL_REQUIRED: &str =
    "Direct transport requires an explicit terminal record and treats provider errors as failures";

/// **Finding:** Tool-call identity and arguments are bounded, deduplicated,
/// and never silently downgraded. Stream tool-call indices are capped at 128,
/// a single call's identity cannot change mid-stream, and duplicate provider
/// identities are a `DuplicateToolCall`. Arguments are parsed as JSON objects;
/// when a provider emits malformed arguments, the transport either repairs
/// them or records a synthetic `_malformed_arguments_error` payload (with a
/// sanitized preview) so the turn can recover — it never drops the call or
/// invents arguments. The repaired or synthetic payload is still a JSON
/// object, so a downstream executor sees a well-formed, bounded argument
/// object rather than raw provider text.
///
/// Source: `src/direct_transport.rs` — `merge_openai_tool_deltas`,
/// `finish_openai_tool_calls`, `parse_openai_tool_calls`,
/// `parse_lenient_tool_arguments`, `repair_tool_arguments_json`
///
/// **Assessment:** Correct. Tool identity is bounded and deduplicated, and
/// malformed arguments degrade to a bounded synthetic object rather than a
/// silent capability downgrade or an untrusted raw string.
pub const TRANSPORT_TOOL_IDENTITY_AND_ARGS: &str = "Direct transport bounds and deduplicates tool identity and degrades malformed arguments to a bounded object";

/// **Finding:** Cancellation is cooperative and is never retried. Every
/// read, write, and event-delivery step calls `check_cancelled`, and a
/// cancelled request returns `ModelTransportCancelled`, which
/// `Error::is_retryable` classifies as non-retryable. A deadline breach
/// returns `ModelTransportTimedOut` (or the more specific TTFT/cadence/slot
/// timeout), which is retryable and is mapped by the watchdog so the caller
/// can distinguish a stalled provider from a genuine turn deadline.
///
/// Source: `src/direct_transport.rs` — `check_cancelled`, `check_deadline`,
/// `BodyWatchdog::map_error`
/// Source: `src/error.rs` — `Error::is_retryable`
///
/// **Assessment:** Correct. Cancellation is preserved end-to-end and is not
/// disguised as a transport failure or replayed automatically.
pub const TRANSPORT_CANCELLATION_NOT_RETRYABLE: &str =
    "Direct transport cancellation is cooperative, distinct, and non-retryable";

/// **Finding:** The time-to-first-token and inter-token cadence watchdogs are
/// distinct and are not reset by provider heartbeats. The TTFT watchdog
/// (`BodyWatchdog::ttft`) arms from the first body byte and is replaced by the
/// cadence watchdog once any body arrives. The cadence watchdog arms only on
/// real progress (`observe_body(progress)`), so a provider that emits empty
/// control frames, `ping` events, or usage-only frames does not extend the
/// cadence deadline. A stalled stream therefore times out with
/// `InterTokenCadenceTimedOut` rather than hanging until the turn deadline.
///
/// Source: `src/direct_transport.rs` — `BodyWatchdog::observe_body`,
/// `BodyWatchdog::read_deadline`, `process_data_payload` (progress flags)
/// Source: `sav/tests/model_transport.rs` — `inter_token_cadence_watchdog_aborts_hung_stream`,
/// `cadence_ignores_heartbeats_empty_control_and_usage_frames_in_every_framing`
///
/// **Assessment:** Correct. Heartbeats and empty frames do not reset the
/// cadence watchdog, so a hung stream is bounded by the cadence timeout.
pub const TRANSPORT_WATCHDOG_NOT_RESET_BY_HEARTBEATS: &str =
    "Direct transport TTFT and cadence watchdogs are distinct and not reset by heartbeats";

/// **Finding:** Malformed streams fail closed. A non-UTF-8 record, a record
/// that is not valid JSON, an unsupported `Transfer-Encoding`, a conflicting
/// `Content-Length`/`chunked` framing, a chunk missing its CRLF, or a body
/// that exceeds its declared `Content-Length` is a `MalformedModelResponse`
/// or `ModelResponseLimitExceeded`. The transport never substitutes a default
/// or silently recovers a malformed frame.
///
/// Source: `src/direct_transport.rs` — `parse_response_head`,
/// `read_chunked_body`, `StreamDecoder::process_line`, `read_response`
///
/// **Assessment:** Correct. Malformed framing and records are failures, not
/// silent recovery, so a crafted stream cannot be folded into a valid turn.
pub const TRANSPORT_MALFORMED_STREAMS_FAIL_CLOSED: &str =
    "Direct transport fails closed on malformed framing, records, and JSON";

/// **Finding:** Provider error leakage is bounded to the HTTP status and to
/// sanitized diagnostics. A non-2xx response is surfaced as
/// `ModelProviderStatus { status }` with the response body intentionally
/// omitted, so a provider error page cannot leak request or credential data.
/// SSE `event`/`id`/`retry` values and unrecognized JSON fields are passed
/// through `sanitize_sse_diagnostic`, which escapes control characters and
/// truncates to a bounded preview before they reach `tracing` or `eprintln`.
///
/// Source: `src/direct_transport.rs` — `read_response` (non-2xx body omitted),
/// `sanitize_sse_diagnostic`, `warn_extra_json_fields`
///
/// **Assessment:** Correct. No TRACE or error path echoes raw provider or
/// request bytes; diagnostics are sanitized and bounded.
pub const TRANSPORT_ERROR_LEAKAGE_SANITIZED: &str =
    "Direct transport error paths omit the provider body and sanitize diagnostics";

/// **Finding:** Unknown provider extensions are non-fatal and do not widen
/// authority. An unrecognized SSE field, an unrecognized JSON object field in
/// a tool call, or an unsupported `event` type is logged through
/// `warn_extra_json_fields` or `sanitize_sse_diagnostic` and the stream
/// continues; the field is ignored, not interpreted as a capability. The
/// only fields that change turn state are the bounded, validated ones
/// (content, reasoning, tool calls, finish reason, usage, model, id).
///
/// Source: `src/direct_transport.rs` — `warn_extra_json_fields`,
/// `OpenAiStreamState::process_line` (unknown field → warn + continue)
///
/// **Assessment:** Correct. Raw provider extensions are ignored, not
/// promoted to capability, so a provider cannot silently add a tool or widen
/// the turn's authority through an unrecognized field.
pub const TRANSPORT_UNKNOWN_EXTENSIONS_IGNORED: &str =
    "Direct transport ignores unknown provider extensions without widening authority";

/// **Limitation:** The direct transport trusts the loopback provider it is
/// configured to talk to. A compromised or misconfigured local model server
/// on the same host can serve crafted responses; the transport's parser
/// bounds, terminal requirement, and error handling prevent a silent
/// capability downgrade or a false terminal success, but they do not
/// authenticate the provider. Provider identity is an operator configuration
/// choice, not a receipt-derived grant, and the transport does not mint
/// evidence about the provider's integrity.
///
/// Source: `src/direct_transport.rs` — `DirectModelTransport::new` (operator
/// configuration), `parse_endpoint` (loopback only)
///
/// **Impact:** A local provider that is itself untrusted can still serve a
/// well-formed but adversarial response. The transport bounds the blast
/// radius (no destination escape, no false success, no silent downgrade) but
/// does not authenticate the provider. This is a documented trust boundary,
/// not a defect: the operator selects the loopback provider explicitly.
pub const TRANSPORT_TRUSTS_LOOPBACK_PROVIDER: &str = "Direct transport trusts the operator-selected loopback provider; parser bounds bound the blast radius but do not authenticate it";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_findings_are_documented() {
        // Verify that all findings are documented as constants
        assert!(!AUTHORIZATION_DELEGATED.is_empty());
        assert!(!GUIDANCE_CLASSIFICATION_ATOMIC.is_empty());
        assert!(!GUIDANCE_IMMUTABLE_AFTER_CLAIM.is_empty());
        assert!(!EXTERNAL_OUTPUT_NOT_TOOL_INTENT.is_empty());
        assert!(!RECOVERY_BUDGET_BOUNDED.is_empty());
        assert!(!TOOL_LOOP_HISTORY_BOUNDED.is_empty());
        assert!(!PROGRESS_HOST_VALIDATED.is_empty());
        assert!(!OPERATION_TOKENS_FRESH.is_empty());
        assert!(!BUDGETS_CUMULATIVE.is_empty());
        assert!(!REGISTRY_BOUNDED.is_empty());
        assert!(!COMMAND_RENDERING_BOUNDED.is_empty());
        assert!(!JOURNAL_RECORDS_BOUNDED.is_empty());
        assert!(!CONTROL_MESSAGES_BOUNDED.is_empty());
        assert!(!SAV_NO_TASK_STATE_OWNERSHIP.is_empty());
        assert!(!NO_AUTO_REPLAY_AUTHORIZATION.is_empty());
        assert!(!GUIDANCE_DELIVERY_DURABLE.is_empty());
        assert!(!JOURNAL_RETIREMENT_RECORDS.is_empty());
        assert!(!JOURNAL_WRITE_POISONING.is_empty());
        assert!(!JOURNAL_NO_FAKE_TERMINATION.is_empty());
        assert!(!LOOP_DETECTION_HEURISTIC.is_empty());
        assert!(!TOOL_LOOP_NO_FILESYSTEM_OBSERVATION.is_empty());
        assert!(!RECOVERY_BUDGET_PER_PROMPT.is_empty());
        assert!(!GUIDANCE_NO_STRUCTURE_VALIDATION.is_empty());
        assert!(!JOURNAL_NO_CRYPTO_AUTHENTICATION.is_empty());
        assert!(!RUNNER_RESOLUTION_CANONICAL.is_empty());
        assert!(!PROBE_TRUST_FAIL_CLOSED.is_empty());
        assert!(!PROBE_RESPONSE_CLOSED.is_empty());
        assert!(!PROTOCOL_BINDING_END_TO_END.is_empty());
        assert!(!ENVIRONMENT_CLOSED_BY_DEFAULT.is_empty());
        assert!(!DELEGATION_TIMEOUT_AND_OUTPUT_BOUNDED.is_empty());
        assert!(!DELEGATION_CLEANUP_UNCONDITIONAL.is_empty());
        assert!(!FAIL_CLOSED_NO_HOST_FALLBACK.is_empty());
        assert!(!MOUNT_PLAN_APPROVAL_BOUND.is_empty());
        assert!(!WORKSPACE_AND_ENVIRONMENT_HOST_BOUND.is_empty());
        assert!(!CGROUP_PROBE_ENFORCEMENT_DISCREPANCY.is_empty());
        assert!(!PROBE_TEST_BACKEND_OVERRIDE.is_empty());
        assert!(!POLICY_IDENTITY_HOST_ASSERTED.is_empty());
        assert!(!AUTHORING_ENVIRONMENT_HOST_BOUND.is_empty());
        assert!(!EXECUTABLE_DISCOVERY_BOUNDED.is_empty());
        assert!(!FALLBACK_PATH_VALIDATED.is_empty());
        assert!(!STDIN_HANDLING_NULL_AND_CANCELLATION_SAFE.is_empty());
        assert!(!GENERATED_PERMISSION_FLAGS_CREDENTIAL_CHECKED.is_empty());
        assert!(!MODEL_SELECTORS_BOUNDED.is_empty());
        assert!(!EXACT_COMMAND_VERIFICATION_FAIL_CLOSED.is_empty());
        assert!(!ERROR_REDACTION_STREAMING_AND_SPLIT_SAFE.is_empty());
        assert!(!SETUP_HOST_ACKNOWLEDGEMENT_ASYMMETRY.is_empty());
        assert!(!TRANSPORT_LOOPBACK_ONLY.is_empty());
        assert!(!TRANSPORT_NO_REDIRECT_FOLLOWING.is_empty());
        assert!(!TRANSPORT_NO_CREDENTIAL_ROUTING.is_empty());
        assert!(!TRANSPORT_INPUT_BOUNDED.is_empty());
        assert!(!TRANSPORT_TERMINAL_REQUIRED.is_empty());
        assert!(!TRANSPORT_TOOL_IDENTITY_AND_ARGS.is_empty());
        assert!(!TRANSPORT_CANCELLATION_NOT_RETRYABLE.is_empty());
        assert!(!TRANSPORT_WATCHDOG_NOT_RESET_BY_HEARTBEATS.is_empty());
        assert!(!TRANSPORT_MALFORMED_STREAMS_FAIL_CLOSED.is_empty());
        assert!(!TRANSPORT_ERROR_LEAKAGE_SANITIZED.is_empty());
        assert!(!TRANSPORT_UNKNOWN_EXTENSIONS_IGNORED.is_empty());
        assert!(!TRANSPORT_TRUSTS_LOOPBACK_PROVIDER.is_empty());
    }
}
