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
pub const GUIDANCE_IMMUTABLE_AFTER_CLAIM: &str =
    "Guidance cannot be edited/withdrawn after claim";

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
pub const BUDGETS_CUMULATIVE: &str =
    "Wall time and token budgets are cumulative across retries";

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
pub const JOURNAL_RECORDS_BOUNDED: &str =
    "Journal records bounded to 64 KiB, segments to 64 MiB";

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
pub const JOURNAL_NO_FAKE_TERMINATION: &str =
    "Dropping journal does not fabricate terminal result";

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
pub const RECOVERY_BUDGET_PER_PROMPT: &str =
    "Recovery budget resets per prompt, not per session";

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
    }
}
