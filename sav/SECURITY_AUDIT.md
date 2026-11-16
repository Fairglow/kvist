# Security Audit: Decision Authorization and Recovery

**Component:** sav  
**Date:** 2025-12-18  
**Auditor:** Autonomous security audit agent  
**Scope:** Decision authorization boundaries, recovery fencing, input guards, approval scope, retirement records, and bounded model data.

## Summary

The sav component implements a bounded, host-neutral conversation and execution loop with explicit separation of concerns between the runtime infrastructure and the embedding host's authority. Key security properties are enforced through bounded data structures, explicit capability passing, and non-resettable recovery budgets.

## Findings

### 1. Decision Authorization Boundaries

**Finding:** Tool execution authorization is correctly delegated to the host.

The `ToolExecutor` trait is supplied by the embedding host. Sav never authorizes tool execution itself; it only proposes intents and delegates to `executor.execute()`. This maintains a clear authority boundary where Sav provides the loop machinery but the host retains control over what effects are permitted.

**Source:** `src/runtime/session.rs` (ToolExecutor trait), `src/runtime/driver.rs` (fold_outcome)

**Assessment:** ✅ Correct. No authority overreach by the runtime.

---

### 2. Proposal Authentication and Classification

**Finding:** Guidance vs. task classification is atomic and race-condition-free.

`ControlMailbox::submit()` atomically classifies submissions against the worker's run state under a single mutex lock. When idle, submissions become tasks; when running, they become guidance. This prevents race conditions where guidance could be misclassified.

**Source:** `src/runtime/control.rs` (enqueue method)

**Assessment:** ✅ Correct. Atomic classification under mutex.

---

### 3. Guidance Immutability After Claim

**Finding:** Guidance cannot be edited or withdrawn after claim.

The `ControlMailbox` tracks claimed messages and rejects `edit()` and `withdraw()` calls for any message that has already been claimed by the worker. This prevents mid-execution tampering with guidance that the worker has already begun processing.

**Source:** `src/runtime/control.rs` (edit, withdraw, claim methods)

**Assessment:** ✅ Correct. Provides integrity of claimed guidance.

---

### 4. Recovery Fencing: Bounded Recovery Budget

**Finding:** Recovery budget is bounded and non-resettable per prompt.

`RecoveryBudget::take()` returns at most two numbered recovery steps per prompt. There is no `reset()` method, so the budget cannot be replenished within a single prompt's lifetime. This prevents infinite recovery attempts and ensures persistent stalls eventually terminate the prompt.

**Source:** `src/loop_detection/generation.rs` (RecoveryBudget)

**Assessment:** ✅ Correct. Effective recovery fencing.

**Limitation:** The budget resets per prompt, not per session. A session with many prompts can consume many recovery steps in total.

---

### 5. Tool Loop Detection

**Finding:** Tool loop detection uses bounded history with action fingerprinting.

`ToolLoopTracker` retains a sliding window of recent action hashes and detects:
- Consecutive identical actions (limit: 2)
- Window-based identical action+observation patterns (limit: 3)
- Observation-invariant loops (limit: 3)
- Maximum stalled turns (limit: 4)

**Source:** `src/loop_detection/actions.rs`, `src/loop_detection.rs`

**Assessment:** ✅ Reasonable bounds. Catches common repetition patterns.

**Limitation:** Detection is heuristic and cannot observe filesystem changes. A tool that makes changes but reports success may not be detected as making progress.

---

### 6. Progress Evidence Validation

**Finding:** Progress evidence requires host validation.

`ProgressEvidence::StateChanged` requires the host to have validated a revision change; it is not inferred from the tool's own success claim. `ToolExecutor::progress()` defaults to `None`, meaning tools cannot self-certify progress to escape loop detection.

**Source:** `src/loop_detection/actions.rs` (ProgressEvidence enum), `src/runtime/session.rs`

**Assessment:** ✅ Correct. Prevents tool self-certification of progress.

---

### 7. Operation Token Isolation

**Finding:** Fresh provider-operation tokens are created per request.

`Operation::start()` creates a new cancellation token for each provider request, which mirrors prompt cancellation, shutdown, and pending guidance within a 10ms polling interval. It never resets the whole-prompt token.

**Source:** `src/runtime/driver.rs` (Operation::start)

**Assessment:** ✅ Correct. Fine-grained cancellation without affecting overall prompt lifecycle.

---

### 8. Cumulative Budget Tracking

**Finding:** Wall time and token budgets are tracked cumulatively across retries.

`PromptBudget` tracks wall time from prompt start, and `AttemptUsage` accumulates token usage across all turns. These are not reset on retries or guidance delivery.

**Source:** `src/runtime/driver.rs` (PromptBudget, AttemptUsage)

**Assessment:** ✅ Correct. Prevents budget evasion through retries.

---

### 9. Input and Path Guards

**Finding:** All external inputs are bounded and validated.

- Registry files: bounded to 64 KiB, 128 profiles, validated ASCII names
- Command rendering: bounded to 256 argv entries, 2 MiB total, 1 MiB prompt
- Journal records: bounded to 64 KiB per record, 64 MiB per segment
- Control socket messages: bounded to 20 KiB with timeouts
- Guidance messages: bounded to 64 KiB, nonblank, NUL-free
- Queue capacity: 32 messages, 1 MiB total text

**Sources:** `src/registry.rs`, `src/command.rs`, `src/observation/journal.rs`, `src/observation/control.rs`, `src/runtime/control.rs`

**Assessment:** ✅ Correct. Comprehensive input validation and resource bounds.

---

### 10. Journal Integrity and Retirement Records

**Finding:** Run journals provide durable, ordered retirement records.

`RunJournal` creates no-clobber regular files with `run_start` and `run_end` events. Sequence numbers are contiguous and one-based. Failed writes poison the writer, preventing silent data loss. Dropping the journal does not fabricate terminal results.

**Source:** `src/observation/journal.rs`

**Assessment:** ✅ Correct. Complete, ordered record of each run's lifecycle.

**Limitation:** Journal records lack cryptographic authentication. Integrity relies on filesystem permissions rather than cryptographic verification.

---

### 11. Approval Scope

**Finding:** Sav explicitly does not own task state or policy decisions.

The component's contract clearly states that it does not own embedding-host roles, task state, policy decisions, grants, approved resources, or credentials. It provides reusable infrastructure without claiming authority over application-level decisions.

**Source:** `CONTRACT.md` (Boundary and ownership section)

**Assessment:** ✅ Correct scope. No authority overreach.

---

### 12. No Automatic Tool Replay Authorization

**Finding:** Tool failure classification does not authorize replay.

`ToolFailureCategory` distinguishes failure types (InvalidRequest, Denied, Prerequisite, Timeout, etc.), but no failure classification authorizes automatic replay. The host must explicitly decide whether to retry.

**Source:** `src/runtime/diagnostics.rs`, `CONTRACT.md`

**Assessment:** ✅ Correct. Prevents unintended side effects from automatic retries of non-idempotent operations.

---

### 13. External Process Output Isolation

**Finding:** External process output does not become native tool intent.

The supervisor captures output from external agents but does not interpret it as tool proposals. Only the native model loop generates `ToolIntent` objects. External agents cannot mint task approval or inject tool calls into the native loop.

**Source:** `src/supervisor.rs`, `src/runtime/session.rs`

**Assessment:** ✅ Correct boundary. Prevents external output injection.

---

### 14. Guidance Delivery Durability

**Finding:** Guidance delivery is persisted before the next provider request.

The recorder persists the delivered guidance state before the next provider request is made, ensuring recovery can reconstruct the conversation state after a crash.

**Source:** `src/runtime/driver.rs` (deliver_guidance)

**Assessment:** ✅ Correct. Provides crash-consistent recovery point.

---

## Remaining Limitations

1. **Heuristic loop detection:** Both generation and tool loop detection use heuristic methods that can produce false positives or miss subtle repetition patterns.

2. **No filesystem observation in tool loop detection:** The detector only sees tool invocations and outcomes, not the actual state of the filesystem.

3. **Recovery budget per prompt, not per session:** Each new prompt gets a fresh recovery budget.

4. **No guidance structure validation:** Guidance messages are validated for size and NUL bytes, but not for structural or semantic correctness.

5. **No cryptographic authentication of journal records:** Journal integrity relies on filesystem permissions.

6. **Loop detection history is bounded:** Tool loop detection retains only 128 action fingerprints and detects cycles up to 32 actions. Longer cycles may escape detection.

## Conclusion

The sav component implements robust decision authorization and recovery controls with clear separation of concerns between the runtime infrastructure and host authority. Key security properties are enforced through bounded data structures, explicit capability passing, non-resettable recovery budgets, and comprehensive input validation. The remaining limitations are primarily heuristic in nature and do not compromise the core authorization boundaries.

**Overall Assessment:** ✅ Observed controls are adequate for the component's scope. Remaining limitations are documented and do not represent authority overreach or recovery fencing failures.
