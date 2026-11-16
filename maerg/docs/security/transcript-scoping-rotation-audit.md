# Security Audit: Transcript Scoping, Rotation, and Private Access

**Scope:** `maerg/src/bark/tui/storage.rs`  
**Date:** 2026-03-06  
**Auditor:** Independent code review (source-blind, clean-slate)

## Findings

### Transcript Scoping

The transcript is scoped to a per-workspace directory via `scope.join(".kvist/transcript")`. The `private_directory()` function validates the path has no parent traversal (`..` components) and creates all intermediate directories with 0700 permissions. Each scope has its own isolated transcript directory, preventing cross-scope reads or writes.

**Observation:** Scoping is enforced at the filesystem level. A transcript opened for one scope cannot read or clobber another scope's segments because each scope has a distinct directory path.

### Rotation Bounds

- **Segment bound:** `SEGMENT_BYTES = 1_048_576` (1 MiB)
- **Document bound:** `DOCUMENT_BYTES = 16_777_216` (16 MiB)

Rotation is triggered when `self.bytes.saturating_add(markdown.len()) > SEGMENT_BYTES`. The `block_boundary()` function splits at Markdown block boundaries to avoid mid-line truncation. Each segment is validated against the 16 MiB document bound before writing.

**Observation:** Rotation preserves the private-file guarantees. New segments are created with the same `open_private()` constraints (0600 mode, single link, no symlinks). The old segment is replaced only after new segments are persisted, preventing silent data loss on interrupted rotation.

### Private Access Rules

The `open_private()` function enforces three invariants:

1. **No symlinks:** Uses `O_NOFOLLOW` to prevent symlink-based TOCTOU attacks
2. **Single link:** Validates `metadata.nlink() == 1` to prevent hardlink-based confusion
3. **Owner-only permissions:** Validates `metadata.mode() & 0o077 == 0` (0600 mode)

The `private_directory()` function validates the entire path has no symlink components before creating or opening files.

**Observation:** All transcript operations (open, read, append, rotation) pass through these private access checks. No code path bypasses the private-file guarantees.

## Residual Risk

- **Race condition on rotation:** If two writers attempt rotation simultaneously, the `try_lock()` on the `.writer` file prevents concurrent writes. However, the lock is advisory and relies on all writers using the same locking mechanism.
- **Large single blocks:** A single Markdown block exceeding 16 MiB will cause a write failure, but this is a documented limitation rather than a security issue.

## Conclusion

The transcript scoping, rotation, and private access rules are consistently enforced. Rotation does not relax the private-file guarantees, and cross-scope transcript access is prevented by filesystem isolation.

**Status:** PASS — no violations observed.