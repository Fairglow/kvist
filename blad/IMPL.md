# Blad Implementation Record

## Overview

Blad is a first-class terminal Markdown viewer that reads skott session
transcripts and standalone Markdown files. It provides interactive viewing
with search, navigation, and optional edit support.

## Implementation Details

### CLI (`main.rs`)

The CLI uses clap with a subcommand pattern. Commands are:

- `list` - list recent sessions
- `show` - view a completed session
- `stream` - follow a session transcript
- `view` - view a standalone markdown file
- `export` - export session to combined markdown

A smart shortcut allows `blad <target>` to automatically interpret the target
as a file path, session path, or session ID.

### Session Management (`session.rs`)

- `list_sessions`: Reads `.skott/runs/` directory, loads session manifests,
  sorts by creation time (newest first), returns up to `limit` sessions.
- `load_session`: Matches session by ID (exact first, then prefix match).
- `load_session_by_path`: Loads session from a given directory path.
- `load_messages`: Reads all message files referenced in the session manifest.

Session data is represented by the `Session` struct with fields: id, path,
manifest (JSON Value), model, completed, created_at, message_count.

### Display (`display.rs`)

- `show_session`: Prints session header, then renders each message with its
  kind label and markdown content.
- `dump_session`: Same as show but without ANSI formatting.
- `stream_session`: Same as show, but notes if session is still active.
- `view_markdown`/`view_markdown_file`: Renders markdown and opens interactive viewer.
- `interactive_viewer`: Full-screen viewer with:
  - Scroll navigation (up/down, page up/down, g/G, k/j, u/d)
  - Mouse wheel support
  - Search mode (`/` key) with highlight and match navigation (n/p)
  - Status bar with scroll position and search info
  - Scrollbar on right side
  - Edit mode (`e` key) opens file in $EDITOR
- `export_session`: Generates combined markdown document with all messages.

### Markdown Rendering (`markdown.rs`)

Uses `pulldown-cmark` parser with GFM extensions enabled:

- ENABLE_TABLES
- ENABLE_STRIKETHROUGH
- ENABLE_TASKLISTS
- ENABLE_HEADING_ATTRIBUTES
- ENABLE_FOOTNOTES
- ENABLE_SUPERSCRIPT
- ENABLE_SUBSCRIPT
- ENABLE_GFM (alert blockquotes)

Syntax highlighting uses `syntect` with the base16-ocean.dark theme. Code
blocks are highlighted per-line with ANSI escape sequences.

Special rendering:

- GFM alert blockquotes: Color-coded labels ([NOTE], [TIP], [IMPORTANT],
  [WARNING], [CAUTION])
- Footnotes: Blue bracketed references
- Math: Purple-colored inline and display math
- Tables: Manual alignment with column width calculation
- Mermaid: Attempts to render via mmdc subprocess, falls back to plain text

## Error Handling

- All filesystem operations use match/unwrap_or rather than panic.
- Parser errors are handled gracefully (fallback to plain text).
- Terminal operations use expect() with clear messages (raw mode, alternate
  screen are fundamental to the viewer).
- Editor subprocess errors are reported, not fatal.

## Testing

- 13 unit tests in markdown.rs covering all rendering features
- 4 integration tests in tests/integration.rs covering session operations
