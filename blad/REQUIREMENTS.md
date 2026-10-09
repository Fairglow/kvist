# Blad Requirements

## Purpose

Blad provides first-class, structured, Markdown-native transcripts for agent
sessions. Each session produces a durable directory of Markdown message bodies
and a JSON manifest, replacing the legacy flat JSONL+plain-text log pair with a
hierarchy that supports streaming updates, selective re-rendering, navigation,
and context import for session continuation.

## Functional Requirements

### Session Lifecycle

1. **R-001 Create session:** Users can start a new interactive session that
   streams assistant output to the terminal in real time and writes durable
   artifacts under the `.blad/sessions/` directory.
2. **R-002 Continue session:** Users can select a past session from the history
   list to "continue as a new session." The new session receives a summarized
   context import (user prompts and final assistant answers only), the original
   session identifier and path, and the original size in tokens.
3. **R-003 List sessions:** Users can list all past sessions with their
   metadata (ID, timestamp, status, size).
4. **R-004 Show session:** Users can view the transcript of a specific session,
   either replaying the full transcript or jumping to a specific message.
5. **R-005 Export session:** Users can export a session as a single combined
   Markdown document for sharing or archival.

### Live Streaming and Navigation

6. **R-006 Streaming updates:** Assistant output is streamed to the terminal as
   it arrives. The display updates live, showing partial content while the
   model is generating.
7. **R-007 Truncation indicator:** When a block is very long and has not
   completed streaming, the display shows a truncation indicator (e.g., "…
   generating") at the cutoff point so the user knows more is coming.
8. **R-008 Page navigation:** During a session, users can press Page Up and
   Page Down to scroll through the transcript history while the session is
   still active.
9. **R-009 Enter navigation mode:** Users can enter a dedicated navigation mode
   (e.g., pressing Esc or Ctrl+Home) to scroll through the complete transcript
   even while streaming continues.
10. **R-010 Return to live:** From navigation mode, users can return to the
    live bottom of the transcript (e.g., pressing Ctrl+End or Enter).

### Artifact Structure

11. **R-011 Session directory:** Each session creates a dedicated directory
    under `.blad/sessions/{session-id}/`.
12. **R-012 Manifest:** Each session directory contains a `manifest.json` file
    that indexes all messages with sequence numbers, roles, paths, sizes, and
    completion status.
13. **R-013 Message bodies:** Each message is stored as a separate Markdown
    file under `messages/{seq:06}.md` within the session directory.
14. **R-014 Streaming writes:** Messages are written as they stream in; the
    file grows with each chunk and is marked complete when streaming ends.
15. **R-015 Truncation state:** The manifest records whether each entry is
    complete or still being streamed, enabling clients to show truncation
    indicators.

### Continuation Context Import

16. **R-016 Summarized import:** When continuing a session, only user prompts
    and final assistant answers (not tool calls or intermediate steps) are
    imported into the new session context.
17. **R-017 Size reporting:** The continuation context includes the original
    session's token count so the model can gauge how much history is available
    without reading everything.
18. **R-018 Original reference:** The continuation context includes a link
    (session ID and path) to the original session for lookup.
19. **R-019 New session:** Continuation creates a new session with a new ID,
    not an append to the old session.

### Format

20. **R-020 New format only:** Blad supports only the new structured format.
    Legacy JSONL+plain-text log format is not recognized or migrated.
21. **R-021 Markdown profile:** Message bodies use a GFM subset with fenced
    code blocks, lists, tables, and inline formatting.
22. **R-022 Manifest schema version:** The manifest declares a `schema_version`
    field for future compatibility.

## Non-Functional Requirements

23. **R-023 Deterministic IDs:** Session IDs use a timestamp plus a unique
    suffix (PID + counter) to avoid collisions.
24. **R-024 Atomic writes:** Message files are written atomically where
    possible; the manifest is updated after each message completion.
25. **R-025 Error resilience:** I/O errors are reported clearly; a failure to
    write an artifact does not crash the interactive session.
26. **R-026 Performance:** Live updates must not block on disk I/O; writes are
    buffered and flushed periodically.
27. **R-027 Filesystem safety:** All paths are validated; symlinks are not
    followed when reading session artifacts.

## Constraints

28. **C-001 Dependencies:** Use existing workspace dependencies where
    possible. Avoid adding new dependencies for this component.
29. **C-002 Platform:** Linux only for now.
30. **C-003 No network:** All operations are local; no network access required.

## Out of Scope

- Backward compatibility with legacy log format
- Multi-user concurrent editing of sessions
- Encryption of session files
- Automatic cleanup or retention policies
