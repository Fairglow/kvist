# Blad Implementation Record

## Overview

Blad is a first-class Markdown-native transcript system for agent sessions. Each session produces a durable directory of Markdown message bodies and a JSON manifest, enabling streaming updates, selective re-rendering, navigation, and context import for session continuation.

## Implementation Details

### Session Manager (`session_manager.rs`)

The `SessionManager` struct manages the lifecycle of a session:
- Generates unique session IDs using UTC timestamp, process ID, and per-process counter
- Creates session directories with `manifest.json` and `messages/` subdirectory
- Writes manifests atomically using `serde_json`
- Tracks message entries with sequence numbers, roles, paths, sizes, and completion status

### Transcript Writer (`transcript_writer.rs`)

The `TranscriptWriter` coordinates streaming writes:
- Buffers streaming chunks in memory
- Writes to message files as they arrive (incremental updates)
- Updates manifest after each message completion
- Manages truncation state (`complete` and `truncated` fields)

### Interactive Session (`interactive.rs`)

The interactive session provides:
- Live streaming of assistant output
- Session persistence via `SessionManager` and `TranscriptWriter`
- Basic input/output loop (user prompts, assistant responses)

### Session Continuation (`continuation.rs`)

The `import_session_context` function:
- Loads an original session's manifest and message files
- Extracts only user prompts and final assistant answers
- Estimates token count (1 token per 4 bytes)
- Constructs a continuation context string with original session reference

### CLI (`cli.rs`)

Commands implemented:
- `blad new` - Start new interactive session
- `blad continue <session-id>` - Continue previous session as new
- `blad list` - List past sessions with metadata
- `blad show <session-id>` - View session transcript
- `blad export <session-id>` - Export session as combined Markdown

## Design Decisions

1. **Manifest as index:** Manifest is an index pointing to message files, not the source of truth for content.
2. **Streaming with truncation:** Messages are written incrementally; manifest tracks completion status.
3. **Continuation as new session:** Creates fresh session with synthetic system message containing context import.
4. **Reuse sav infrastructure:** Uses `sav::ReasoningEffort` and other sav types where appropriate.

## Dependencies

- `sav` - Provider-neutral prompt acquisition and model transport
- `clap` - CLI argument parsing
- `serde` / `serde_json` - JSON serialization
- `chrono` - Timestamp formatting
- `crossterm` - Terminal interaction (for future TUI enhancements)
- `tempfile` - Temporary directory management

## Testing

Integration tests verify:
- Session creation and directory structure
- Message addition and file writing
- Manifest updates
- Session completion
