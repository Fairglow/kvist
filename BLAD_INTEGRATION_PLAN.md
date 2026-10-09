# Blad-Skott Integration Plan

## Overview

Make Skott fully utilize Blad as its transcript viewer. Skott writes structured transcripts that Blad can read, render with themes, and display for both live and historical sessions.

## Current State

- **Skott writes sessions as:** `session-{timestamp}-{pid}-{counter}.jsonl` (NDJSON journal) + `session-{timestamp}-{pid}-{counter}.log` (bounded 64KB transcript)
- **Blad has its own format** (`.blad/sessions/{id}/manifest.json` + `.md` files) that Skott never writes to
- Skott has complete TUI with markdown rendering, themes, navigation
- No integration between the two tools

## Design: Skott Writes, Blad Reads

### Skott Writes Structured Format

Skott writes the structured transcript format (JSON manifest + .md message bodies) alongside the existing NDJSON journal. The structured format captures everything needed for faithful reproduction:

```
.skott/runs/
  session-2026-10-09T10-00-00Z-12345-0-my-task.jsonl   # operational journal (unchanged)
  session-2026-10-09T10-00-00Z-12345-0-my-task.log     # bounded log (unchanged)
  session-2026-10-09T10-00-00Z-12345-0-my-task/        # NEW: structured transcript
    session.json                                      # manifest with message index
    messages/
      000001.md   # user prompt
      000002.md   # assistant reasoning
      000003.md   # assistant final answer
```

### Blad Reads and Renders

Blad reads the structured format and provides:
1. `blad list` - list sessions from `.skott/runs/`
2. `blad session <id>` - view completed session with full markdown rendering
3. `blad stream <id>` - follow live session as it unfolds
4. `blad view <file>` - view any standalone .md file
5. `blad export <id>` - export to combined .md document

Blad becomes a library that Skott uses for its history feature, plus a standalone CLI.

> Consider allowing `blad <id|file>` as a short-hand for either `blad session <id>` or `blad view <file>` depending on the argument value.

### Theme Consistency

Skott uses Blad's rendering library for its history feature, so historical sessions in Skott look the same as live sessions (same renderer, same theme support). Themes are applied from source, not persisted in the transcript.

## Implementation Steps

1. **Skott writes structured transcripts** alongside the existing NDJSON journal
   - Create `skott/src/transcript_writer.rs`
   - Hook into session events (user_message, assistant_message, reasoning, tool calls)
   - Write session.json manifest and individual .md message files

  > We don't need to retain any backwards compatibility and can retire the old format, as it is completely replaced by the new handling. No need to support any migration path either. A clean break is sufficient.

2. **Blad reads skott's structured transcripts** (and any standalone .md files)
   - Remove `new` and `continue` commands (those belong to skott)
   - Add `--log-dir` option to specify where skott sessions live
   - Read JSON manifest and .md message bodies
   - Add `blad <id>` shortcut command

3. **Skott uses Blad's rendering library** for its history display feature
   - Make Blad a library that skott depends on
   - Use Blad's renderer for historical session display in Skott's TUI

4. **Test the integration**
   - Verify skott writes transcripts correctly
   - Verify blad can read and display them
   - Verify themes apply consistently

## Key Decisions

- **JSON over YAML** for the manifest: Both skott and blad already use serde_json, so no new dependency needed.
- **Separate .md files per message**: Enables efficient random access, selective re-rendering, and theme changes without re-parsing the entire transcript.
- **Blad is a viewer, not a session manager**: Session creation and continuation belong to skott. Blad only reads and renders.
- **Backward compatibility**: Skott continues to write its existing NDJSON journal and .log files. The new structured format is additional, not replacement.

## Benefits

- Historical sessions can be viewed with the same rendering quality as live sessions
- Theme changes are applied from source, not persisted
- Efficient random access to specific messages
- Blad can view any .md file, not just skott sessions
- Clear separation of concerns: skott manages sessions, blad renders transcripts
