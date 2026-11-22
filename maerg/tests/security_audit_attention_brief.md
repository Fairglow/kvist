# Security Audit: Attention Evidence Boundaries

**Component:** maerg (Kvist task orchestration)
**Scope:** Human attention brief presentation layer — bounded redacted summaries, path/link refusal, no new model effects, preserved task/recovery authority
**Method:** Source inspection of `src/task_commands/attention.rs`, `src/task_commands.rs`, `src/task_commands/continuous.rs`, `src/task_commands/decisions.rs`, `src/file_io.rs`, and `src/filesystem.rs`
**Boundary:** Internal implementation review. Independent source observation and comparison remain separate. This is not compliance certification, acceptance, or a protected review receipt.

## 1. Bounded Redacted Summaries

### 1.1 Hard 256 KiB Cap with Fail-Closed Rejection

**Observation:** `attention::redact()` (`attention.rs`) applies configured literal redactions, then checks the result against `MAX_BRIEF_BYTES` (256 KiB). If the redacted text exceeds the cap, it returns `KvistError::AgentEvidenceFailed` with the reason "Markdown attention brief exceeds 256 KiB; no brief was silently truncated." The brief is not written; the error propagates to the caller.

**Assessment:** Strong. The boundary is fail-closed: an oversized brief is rejected rather than silently truncated. This prevents unbounded evidence files from accumulating in `.kvist/attention/`.

### 1.2 Host-Derived Cause Truncation at 128 KiB

**Observation:** In `attention::failure()`, the host-derived error cause is truncated at `MAX_BRIEF_BYTES / 2` (128 KiB) via `truncate_utf8()`, followed by an explicit marker: `[Diagnostic excerpt truncated; consult retained logs and attempt evidence.]`. The truncation is byte-boundary-safe.

**Assessment:** Strong. The host diagnostic is bounded and the truncation is explicitly marked. However, the agent explanation and the queued decisions/blockers listing are not individually truncated before the 256 KiB check. A large explanation or a large queue can cause the entire brief to exceed the cap, in which case `persist()` returns an error and no brief is written. The failure path then reports "Markdown brief persistence also failed" and "Retained effects and recovery evidence were not cleared." This is a known limitation, not a defect: the brief is advisory and its absence does not clear task state.

### 1.3 Redaction Is Literal Substitution, Not Secret Detection

**Observation:** `evidence_redactions()` (`task_commands.rs`) collects configured redaction values from the `architect`, `developer`, and `security_reviewer` agent profiles, plus sandbox `allowed_environment` values. `redact()` performs literal substring replacement with `[REDACTED]`. There is no pattern-based secret detection, no entropy analysis, and no Markdown sanitization.

**Assessment:** Adequate for the stated purpose. Redaction is deterministic and configuration-driven. It is not a general secret scanner; unconfigured secrets in agent output or error text will pass through unredacted. The brief is advisory evidence, not a compliance artifact, so this limitation is consistent with its trust boundary.

### 1.4 Advisory Labeling of Agent Content

**Observation:** The decision brief header (`attention::decision()`) labels the agent explanation as "advisory" and warns "The retained explanation may be output-limited; consult the raw Markdown proposal/review companions for full evidence." The failure brief (`attention::failure()`) labels any retained explanation as "This text predates the stop; it is not independently verified." The review companion (`attention::review()`) labels the report as "agent-written advisory evidence, not human acceptance or verified facts."

**Assessment:** Strong. Every agent-authored section is explicitly labeled advisory. The trust boundary between host-written diagnostics and agent-authored explanations is preserved in the presentation layer.

## 2. Path and Link Refusal

### 2.1 Ancestor Directory Chain Verification

**Observation:** `require_real_directory_chain()` (`attention.rs`) walks every ancestor from the project root to the component directory using `fs::symlink_metadata` (non-following) at each step. Any link-like entry (`is_link_like()`) or non-directory entry is rejected with `journal_error()`. The target must remain inside the project root (`strip_prefix` check).

**Assessment:** Strong. This prevents symlink-escape of the evidence directory through any ancestor path component. The check is performed before any file creation.

### 2.2 Evidence Directory and Alias Refusal

**Observation:** `persist()` (`attention.rs`) creates the attention directory via `continuous::checked_directory()`, which creates the directory and then re-inspects it with `symlink_metadata`, rejecting link-like or non-directory entries. The content-addressed history file is inspected with `symlink_metadata` and read via `read_bounded_regular_file()` (`task_commands.rs`), which opens with `O_NOFOLLOW | O_NONBLOCK` on Unix and re-checks metadata after open. The latest alias is inspected with `symlink_metadata` and must be a regular non-link file before `replace_file_atomically()` is called.

**Assessment:** Strong. Multiple layers of link refusal protect the evidence directory, history files, and alias. The `O_NOFOLLOW` flag on open provides a second layer of protection against symlink replacement between the metadata check and the open.

### 2.3 Task Name Sanitization

**Observation:** `persist()` derives the file stem from the task name. If the task name is non-empty, at most 128 bytes, and consists only of ASCII alphanumeric characters, `-`, or `_`, it is used directly. Otherwise, the stem is `task-<sha256-of-task-name>`. This prevents path traversal via task names (e.g., `../escape` becomes a hashed stem).

**Assessment:** Strong. The whitelist approach is conservative. Unsafe task names cannot escape the attention directory.

### 2.4 Review Companion Verification

**Observation:** `attention::review_index()` (`attention.rs`) verifies each Markdown companion with `symlink_metadata` before linking it in the index. A companion that is a link or non-regular file causes an error. A missing companion produces a "no Markdown companion is available" note rather than a link.

**Assessment:** Strong. The index does not link to unverified files. Missing companions are explicitly noted rather than silently omitted.

### 2.5 TOCTOU Window

**Observation:** The `symlink_metadata` check and the subsequent open/persist are not atomic. A concurrent filesystem replacement between the check and the operation is not prevented by these checks alone. The `O_NOFOLLOW` flag on `read_bounded_regular_file()` narrows the window for reads, but `replace_file_atomically()` does not use `O_NOFOLLOW` on the destination.

**Assessment:** Known limitation. These are path checks, not demonstrated resistance to concurrent filesystem replacement. The component lock (`TaskLock`) serializes concurrent writers to the same component, which mitigates the risk in practice, but the lock is advisory and does not prevent a malicious local actor from replacing files between the check and the write.

## 3. No New Model Effects

### 3.1 Failure Path Does Not Call the Model

**Observation:** `attention::failure()` (`attention.rs`) has an explicit doc comment: "No queue/ledger mutation or model call: even preflight failures can be explained." The function constructs the brief from the host error, any retained agent explanation, and the current queue state. It does not invoke the model, does not mutate the task queue, and does not write to the ledger.

**Assessment:** Strong. The failure path is purely presentational. Even preflight failures (component resolution, configuration loading) produce a brief without calling the model.

### 3.2 Decision Brief Is Presentation Only

**Observation:** `attention::decision()` (`attention.rs`) writes the attention brief before the task transitions to `AwaitingDecision`. The brief itself does not call the model or dispose of the task. The transition is a separate operation performed by the caller (`task_commands.rs`). The brief is a snapshot of the decision state, not a new model effect.

**Assessment:** Strong. The presentation layer is separated from the task lifecycle. The brief is written under the existing task lock and does not acquire new authority.

### 3.3 Review Companion Is Formatting Only

**Observation:** `attention::review()` (`attention.rs`) formats the already-collected review report into Markdown. It does not call the model or modify the report content. The report is passed in as a parameter.

**Assessment:** Strong. The review companion is a pure formatting operation. No new model effects are introduced.

### 3.4 Resolution Instructions Are Host-Authored

**Observation:** `attention::resolution()` (`attention.rs`) is a pure string formatting function. It produces host-authored instructions for human response (choose, clarify, reject, resume). It does not call the model, does not perform I/O, and does not modify task state. The instructions explicitly state: "A recommendation is not human acceptance. Intent approval never approves implementation or a reviewer's claims." and "This brief changes no task state, approval or recovery fence."

**Assessment:** Strong. The resolution instructions are deterministic and host-authored. They do not introduce new model effects or task authority.

## 4. Preserved Task and Recovery Authority

### 4.1 Brief Persistence Failure Does Not Clear Task State

**Observation:** `attention::failure()` (`attention.rs`) returns an error on persistence failure that includes "Retained effects and recovery evidence were not cleared." The error preserves the original cause and the persistence failure reason. The task state and recovery evidence are not modified.

**Assessment:** Strong. The brief is advisory. Its absence does not clear task state or recovery evidence. The error message explicitly preserves the trust boundary.

### 4.2 Pending Decision Authority Is Preserved

**Observation:** `attention::failure()` calls `decisions::ensure_no_pending()` before writing the brief. If a pending decision exists, the function returns an error that includes "No attention evidence was rewritten. Finish the identical approved decision apply; task state and retained effects were not cleared." The pending decision is not cleared or modified.

**Assessment:** Strong. The pending decision authority is preserved. The brief does not interfere with the decision lifecycle.

### 4.3 Lock Acquisition and Release

**Observation:** `persist()` (`attention.rs`) acquires a component lock if none is inherited, revalidates the lock before writing, and releases only the lock it acquired. The inherited lock is not released by `persist()`. The lock protects the evidence directory, not task state.

**Assessment:** Strong. The lock lifecycle is correct. The brief does not acquire or release task-level locks. The component lock is released after the brief is written, preserving the task lock for the caller.

### 4.4 Unresolved Attempt Preservation

**Observation:** `unresolved_run_error()` (`task_commands.rs`) produces an error that includes "Completion is unconfirmed; the attempt remains unresolved. Do not replay or delete its evidence." The attempt journal is preserved. The error does not clear the attempt or modify its state.

**Assessment:** Strong. Recovery authority is preserved even when the brief cannot be written. The attempt journal is the authoritative record, not the brief.

### 4.5 Advisory Snapshot Labeling

**Observation:** The decision brief header labels the content as "Snapshot: awaiting-decision" and "This is advisory evidence, not human acceptance. It describes the retained attempt, not a guarantee of current source or task state." The failure brief labels the content as "This is a historical stop diagnostic; inspect current task state before acting. No completion or approval is implied."

**Assessment:** Strong. The briefs are explicitly labeled as snapshots and historical diagnostics. They do not claim to represent current task state or grant any authority.

## Limitations

1. **No general secret detection.** Redaction is literal substring replacement of configured values. Unconfigured secrets in agent output or error text pass through unredacted.
2. **No Markdown sanitization.** The brief is Markdown, but no escaping or sanitization is applied to agent-authored content. Markdown injection is possible but the brief is advisory evidence, not a rendered UI.
3. **TOCTOU window.** The `symlink_metadata` check and subsequent open/persist are not atomic. A concurrent filesystem replacement between the check and the operation is not prevented.
4. **Large queue or explanation can prevent the brief.** The agent explanation and queue listing are not individually truncated. A large explanation or queue can cause the entire brief to exceed 256 KiB, in which case no brief is written.
5. **Lock is advisory.** The component lock serializes concurrent writers but does not prevent a malicious local actor from replacing files between the check and the write.

## Conclusion

The attention evidence boundary is well-designed and correctly implemented. The four trust boundaries — bounded redacted summaries, path/link refusal, no new model effects, and preserved task/recovery authority — are all enforced in the presentation layer. The briefs are advisory snapshots that do not claim certification, acceptance, or new task authority. The limitations are consistent with the advisory trust boundary and do not introduce new security risks.
