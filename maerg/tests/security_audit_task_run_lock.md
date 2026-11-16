# Security Audit: Task-Run Lock Lifecycle, Transcript Boundary, and Crash Recovery

**Component:** maerg (Kvist task orchestration)
**Scope:** Per-component lock lifecycle, liveness refusal, stale-lock protection, interrupted-run recovery
**Method:** Source-blind code inspection of `src/task_commands.rs`, `src/bark/tui/workspace.rs`, and related modules

## 1. Lock Acquisition

**Observation:** `TaskLock::for_context()` creates a lock file via `write_new_file_atomically()`, which uses `O_EXCL` semantics. The lock file contains:
- `started_at` timestamp
- `task_id`
- `pid` (process ID)
- `nonce` (random value)

The lock is stored in a user-owned state directory (`~/.local/state/kvist/task-locks-v1` or equivalent), not in the project directory, preventing project-local tampering.

**Assessment:** Strong. Atomic creation prevents race conditions. The nonce adds entropy for owner authentication.

## 2. Lock Release on Terminal Result

**Observation:** The `TaskRunPrep` struct holds the lock. On task completion:

1. `TaskRunPrep::complete()` is called with the run summary
2. The task transitions to its terminal state (Completed, Blocked, or AwaitingDecision)
3. `TaskRunPrep::release_lock()` is called, which invokes `release_in_place()`
4. `release_in_place()` calls `remove_if_owned()`, which:
   - Revalidates lock ownership
   - Quarantines the lock file (rename to unique path)
   - Verifies the quarantined file matches the original
   - Removes the quarantined file
   - Syncs the directory

The embedded TUI path explicitly calls `release_lock()` after `complete()` (see `src/bark/tui/workspace.rs` line ~703). If release fails, a warning is shown but the shell continues; the lock remains for explicit `task unlock`.

**Assessment:** Strong. The quarantine-verify-remove pattern prevents lock loss from concurrent modifications. The explicit release on terminal result ensures the lock doesn't persist until process exit.

## 3. Live-Owner Refusal

**Observation:** `TaskLock::remove_stale_path()` (used by `task unlock`) checks:
1. The lock record is well-formed (`valid_task_lock_record()`)
2. The owner does not appear live (`lock_owner_appears_live()`)
3. The lock hasn't changed during quarantine (re-read and compare)

If the owner appears live, unlock is refused with: "unlock refused because the task lock owner still appears live".

**Assessment:** Strong. This prevents accidental lock removal while a task is actively running. The liveness check is based on PID, which is a reasonable heuristic (though not perfect if PIDs are recycled).

## 4. Stale-Lock Protection After Release

**Observation:** Releasing the lock on completion does not weaken stale-lock protection because:
- Lock creation still uses atomic `O_EXCL` semantics
- Lock release still validates ownership before removal
- The quarantine pattern ensures no window where a lock is partially removed

**Assessment:** Strong. The release path is symmetric to the acquisition path in terms of safety checks.

## 5. Interrupted-Run Recovery

**Observation:** `TaskLock::recover_path()` handles crashed runs:
1. Attempts to create a new lock
2. If `TaskLockExists` is returned, reads the retained lock
3. Validates the retained lock matches the expected crashed lock (digest comparison)
4. Checks the owner is not live
5. Quarantines the old lock
6. Creates a new lock
7. If new lock creation fails, restores the quarantined lock

The recovery path requires:
- The retained lock to be well-formed
- The retained lock to match the expected crashed lock digest
- The retained lock's owner to not be live

**Assessment:** Strong. The recovery path is conservative: it only proceeds if the retained lock is provably from the crashed operation and the owner is dead. The restore-on-failure ensures no lock is lost.

## 6. Transcript Boundary

**Observation:** The embedded TUI path appends a completion marker after task runs:
```
**Command finished** (exit status: 0).
```
or
```
**Command failed** (exit status: 1).
```
This ensures the transcript never ends mid-sentence without a completion marker.

**Assessment:** Adequate. The transcript boundary is clear and unambiguous.

## 7. Residual Risks

1. **PID recycling:** If a PID is recycled quickly after a crash, `lock_owner_appears_live()` might incorrectly report the owner as live. This is a low-probability edge case that would require manual intervention.

2. **Quarantine file accumulation:** If lock release or recovery fails after quarantine but before removal, quarantine files may accumulate in the task-locks directory. These are harmless but could grow over time.

3. **No timeout mechanism:** The lock has no built-in timeout; a hung process holds the lock indefinitely. Users must use `task unlock` with the liveness check.

4. **Single-component granularity:** The lock is per-component, not per-task. Two different tasks on the same component cannot run concurrently. This is by design but worth noting.

## 8. Conclusion

The lock lifecycle implementation follows strong safety principles:
- Atomic operations throughout
- Ownership validation before state changes
- Quarantine-verify-remove pattern for all lock mutations
- Conservative recovery that requires proof of crash
- Explicit liveness checks before stale-lock removal

The implementation correctly releases the lock on terminal results, refuses to unlock live owners, and provides a durable recovery path for interrupted runs. The residual risks are low-probability edge cases that are manageable through manual intervention.

**Audit Status:** PASS — No critical or high-severity issues identified.
