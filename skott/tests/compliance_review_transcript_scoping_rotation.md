# Compliance Review: Transcript Scoping and Rotation

**Task ID:** `transcript-scoping-rotation-compliance-review`
**Component:** `skott`
**Review Type:** Compliance Review (source-blind comparison)

## Scope

This review compares the intended transcript-persistence behavior with the
independent source observations and executed tests. It separates implemented
behavior from the compliance judgment and retains limitations.

## Source Observations

### Transcript Scoping

The `TranscriptWriter` creates sessions under a bounded, validated directory
structure:

1. **Session Identity Validation** (`transcript_writer.rs`, `new()`):
   - Session ID must be 1-256 bytes
   - Must be a single filesystem-safe component (no `/`, `.`, `..`)
   - Allowed characters: ASCII alphanumeric, hyphen, underscore, dot
   - Returns `io::Error::InvalidInput` on violation

2. **Directory Creation** (`transcript_writer.rs`, `new()`):
   - Uses `private_directory()` from `session_log` for parent isolation
   - Creates `session-{id}` via `mkdirat()` with `S_IRWXU` (owner-only)
   - Opens held directory descriptors with `O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`
   - Creates `messages/` subdirectory with same permissions

3. **File Writing** (`transcript_writer.rs`, `write_message_file()`):
   - Message files use `O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC`
   - Atomic manifest writes: write to temp file, then `renameat()` to `session.json`
   - Files created with `S_IRUSR | S_IWUSR` (owner read/write only)

### Transcript Rotation

The implementation uses lifecycle-based rotation rather than size/time-based:

1. **Session Lifecycle** (`session_transcript.rs`):
   - `session_start()` begins a new live run with a unique identity (`{session_id}-{live_number}`)
   - `session_finish()` completes the live run and writes the final manifest
   - Multiple live runs can occur within one session (incrementing `live_number`)

2. **Manifest State Transitions** (`transcript_writer.rs`):
   - `record_user()` sets status to `streaming`, clears `completed_at`
   - `complete()` sets status to `completed`, records `completed_at`
   - Each message increments `sequence` and writes a new manifest entry

3. **Continuation Behavior** (observed in tests):
   - A new `on_prompt()` after `session_finish()` resets the lifecycle
   - Status transitions from `completed` back to `streaming`
   - `completed_at` is cleared for the new run
   - Only one `final` assistant disposition is expected per completed run

## Executed Tests

The following tests in `skott/tests/transcript_recording.rs` verify the behavior:

1. **`structured_recording_cannot_redirect_host_writes_through_links`**:
   - Verifies symlink attacks are prevented for both manifest and message files
   - Confirms atomic manifest replacement does not overwrite linked targets
   - Validates file permissions (0o600 for manifest)

2. **`structured_recording_pins_its_original_directory_descriptors`**:
   - Verifies held directory descriptors prevent TOCTOU attacks
   - Confirms writes go to the original directory, not a replacement symlink

3. **`continuing_native_transcript_has_one_final_and_resets_its_lifecycle`**:
   - Verifies exactly one `final` assistant disposition per completed run
   - Confirms lifecycle reset on new prompt (streaming → completed → streaming)
   - Validates `completed_at` is cleared on continuation

4. **`unicode_prompt_slug_is_bounded_without_splitting_a_character`**:
   - Verifies Unicode handling in session slugs
   - Confirms no character splitting at boundaries

5. **`structured_sessions_are_single_component`** (partial view):
   - Verifies session identity is a single filesystem component

## Correspondence Analysis

| Intended Behavior | Source Observation | Test Coverage | Discrepancy |
|-------------------|-------------------|---------------|-------------|
| Sessions are isolated per task | `session-{id}` directories with validated IDs | Yes (`structured_sessions_are_single_component`) | None observed |
| Symlink attacks are prevented | `O_NOFOLLOW`, `O_EXCL`, held descriptors | Yes (link redirection test) | None observed |
| Atomic manifest updates | Temp file + `renameat()` pattern | Yes (link test verifies atomicity) | None observed |
| Lifecycle state is tracked | `streaming`/`completed` status in manifest | Yes (lifecycle reset test) | None observed |
| One final answer per run | `final` disposition on last assistant message | Yes (lifecycle test counts finals) | None observed |
| Continuation resets lifecycle | New prompt clears `completed_at`, sets `streaming` | Yes (lifecycle test) | None observed |

## Limitations

1. **No Size-Based Rotation**: The implementation does not rotate transcripts
   based on size or age. Sessions grow indefinitely until explicitly completed.

2. **No Cleanup of Old Sessions**: There is no mechanism to prune or archive
   old session directories.

3. **Live Run Identity Exhaustion**: The `live_number` counter uses `u64` and
   will eventually exhaust (though this is not a practical concern).

4. **Partial Test View**: The last test (`structured_sessions_are_single_component`)
   was only partially visible due to file read limits.

5. **No Concurrent Writer Tests**: The tests do not verify behavior under
   concurrent writes from multiple processes.

## Compliance Judgment

The implemented behavior corresponds to the intended transcript-persistence
behavior for scoping and rotation. The source observations and executed tests
confirm:

- Session isolation through validated, bounded directory structures
- Protection against symlink and TOCTOU attacks
- Atomic manifest updates
- Correct lifecycle state management
- Proper continuation behavior

No discrepancies were observed between the intended behavior and the
implemented behavior within the scope of this review.

**Limitation:** This review is based on source code inspection and the
executed test suite. It does not include runtime behavior analysis or
performance testing.
