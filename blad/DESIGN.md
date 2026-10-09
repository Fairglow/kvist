# Blad Design

## Architecture

Blad is a standalone Rust binary that wraps the existing `sav` runtime with
first-class structured transcript management. It reuses skott's session
infrastructure (model transport, sandbox, tools) while replacing the flat
JSONL+text logging with the new hierarchical Markdown format.

## Components

### Session Manager (`session.rs`)

Manages the lifecycle of a session directory:
- Creates session directory and manifest
- Writes message files atomically
- Updates manifest on message completion
- Handles streaming updates with truncation state

### Transcript Writer (`transcript.rs`)

Coordinates writing to both the terminal and durable artifacts:
- Buffers streaming chunks
- Writes to message files as they arrive
- Updates manifest after each message
- Manages truncation indicators

### Interactive TUI (`tui/`)

Borrowed heavily from skott's TUI:
- Live streaming display
- Page up/down navigation
- Enter navigation mode (Esc)
- Return to live (Ctrl+End)
- Markdown rendering

### Context Importer (`continue.rs`)

For session continuation:
- Reads original session manifest
- Extracts user prompts and final assistant answers
- Computes token count estimate
- Constructs continuation context string

### CLI (`cli.rs`)

Command parsing and dispatch:
- `new`, `continue`, `list`, `show`, `export`
- Option validation

## Data Flow

```
User prompt → sav runtime → Model response chunks
                            ↓
                    TranscriptWriter
                   ↙           ↘
            Terminal display  Durable artifacts
```

## Key Design Decisions

1. **Manifest as index:** The manifest is not the source of truth for content;
   it's an index. Each entry points to its message file. This enables efficient
   random access and selective re-rendering.

2. **Streaming with truncation:** While a message is being streamed, it's written
   to its file incrementally. The manifest entry is marked `complete: false`
   until streaming ends. The TUI shows a truncation indicator.

3. **Continuation as new session:** Continuing creates a fresh session directory
   with a new ID. The continuation context is a synthetic system message at the
   start of the new session. The original session remains immutable.

4. **Reuse skott's infrastructure:** Rather than reimplementing the sandbox,
   model transport, and tool execution, Blad uses the same `sav` library and
   session machinery as skott, only changing the transcript layer.
