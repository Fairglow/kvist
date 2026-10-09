# Blad Design

Blad is a viewer for skott agent sessions. It reads skott's session format
from `.skott/runs/` and provides:
- Interactive viewing of completed session transcripts
- Live streaming display of active sessions
- Session listing with metadata
- Export of session transcripts

Blad does not create or continue sessions; that is skott's responsibility.
Blad reads skott's NDJSON journal and reconstructs the transcript for display.

## Session Format

Skott writes sessions to `.skott/runs/` as paired files:
- `session-{timestamp}-{pid}-{counter}-{slug}.jsonl` - NDJSON event stream
- `session-{timestamp}-{pid}-{counter}-{slug}.log` - Human-readable log

Each NDJSON event has the structure:
```json
{"schema_version": 1, "sequence": N, "event": {
    "type": "session_start" | "user_message" | "model_request" | 
           "turn_start" | "turn_finish" | "tool_dispatch" | "tool_result" |
           "notice" | "session_end" | "summary",
    ...
}}
```

## CLI Commands

### `blad list`
Lists recent sessions from `.skott/runs/` with metadata (time, model, duration).

### `blad show <session-id>`
Displays a session transcript, reconstructing user messages, assistant responses,
and tool interactions from the NDJSON event stream.

### `blad stream <session-id>`
Follows an active session, displaying updates as they are written to the journal.

### `blad export <session-id> [output.md]`
Exports a session as a combined Markdown document.
