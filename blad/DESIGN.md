# Blad Design

Blad is a viewer for skott agent sessions and standalone Markdown files. It
reads skott's structured session format from `.skott/runs/` and provides:

- Interactive viewing of completed session transcripts
- Live streaming display of active sessions
- Session listing with metadata
- Export of session transcripts
- Full Markdown rendering with syntax highlighting
- Search and navigation in the interactive viewer
- Optional edit mode

## Architecture

```
blad/
├── src/
│   ├── main.rs          # CLI entry point and command dispatch
│   ├── lib.rs           # Library exports
│   ├── session.rs       # Session loading and listing
│   ├── display.rs       # Terminal display and interactive viewer
│   ├── markdown.rs      # Markdown rendering with syntax highlighting
│   └── debug_events.rs  # Debug event logging
├── tests/
│   └── integration.rs   # Integration tests
├── REQUIREMENTS.md
├── CONTRACT.md
├── DESIGN.md
├── IMPL.md
├── TODOS.yaml
└── Cargo.toml
```

## Module Responsibilities

### `session.rs`

- `list_sessions(log_dir, limit)`: List recent sessions, sorted newest first
- `load_session(log_dir, session_id)`: Load session by ID (exact or prefix match)
- `load_session_by_path(path)`: Load session by directory path
- `load_messages(session)`: Load all messages for a session

### `display.rs`

- `show_session(session)`: Display completed session transcript
- `dump_session(session)`: Output session as plain text
- `stream_session(session)`: Stream session transcript
- `view_markdown(content)`: View rendered markdown interactively
- `view_markdown_file(path, content)`: View with edit support
- `dump_markdown(content)`: Output rendered markdown as plain text
- `export_session(session, output)`: Export to combined markdown document
- `interactive_viewer(path, content)`: Full interactive viewer with navigation and search

### `markdown.rs`

- `render_markdown(md, writer)`: Render markdown with syntax highlighting
- `find_syntax(ss, lang)`: Find syntax definition for a language
- `render_mermaid(content)`: Attempt to render mermaid diagram via mmdc
- `render_table(rows)`: Render aligned table
- `write_emphasized(...)`: Write text with ANSI emphasis
- `to_ansi_line(ranges)`: Convert syntect highlighting to ANSI

## Markdown Rendering

Blad uses `pulldown-cmark` for parsing and `syntect` for syntax highlighting.

Supported Markdown features:

- All CommonMark elements
- GitHub Flavored Markdown extensions:
  - Tables
  - Strikethrough
  - Task lists
  - Autolinks
  - Footnotes
  - Superscript/subscript
  - Alert blockquotes ([!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION])
- Fenced code blocks with syntax highlighting (50+ languages)
- Inline and display math (LaTeX)
- Mermaid diagrams (with mmdc integration)

## Interactive Viewer

The interactive viewer provides:

- Scroll navigation (keyboard and mouse wheel)
- Search with highlight and match navigation
- Status bar showing scroll position and search results
- Scrollbar for visual position feedback
- Edit mode (press `e`) to open file in $EDITOR

## Design Decisions

1. **Read skott format:** Blad reads skott's structured session format rather
   than defining its own. This avoids duplication and ensures compatibility.
2. **Separate rendering from display:** Markdown rendering is in `markdown.rs`,
   terminal display logic is in `display.rs`. This allows testing rendering
   without a terminal.
3. **Search on rendered content:** Search operates on the rendered ANSI output,
   stripping escape codes before matching.
4. **Mermaid fallback:** If `mmdc` is not installed, mermaid diagrams are
   displayed as plain text with a note.
5. **No state management:** Blad is stateless; it reads files and displays
   them without maintaining persistent state.

## Targeted Layout Correction Design

This is a bounded correction, not the full document-model redesign in `PLAN.md`.
Keep the public ANSI rendering API and existing viewer commands/key semantics.

- A private inline writer buffers a contiguous word across parser/style events,
  with text/style spans and source-contributed pending whitespace. It measures
  complete words in Unicode display columns, emits spaces only between source
  words, includes separators/prefixes in fit decisions, resets at block/hard
  breaks, and keeps inline code atomic. Long prose words break by grapheme.
- Immutable Unicode segmentation/width support uses versions already present in
  the workspace lockfile. A private terminal-text helper separates SGR styling
  from measurable text and clips complete graphemes; other terminal controls
  are represented visibly rather than executed during painting.
- A private viewport helper renders original Markdown at the content budget.
  Number-gutter sizing converges by monotonically reserving enough digits for
  the resulting rendered-line count. A single global horizontal bound derives
  from the widest rendered line, not from individual visible lines.
- The frame painter positions every header/content/footer/scrollbar row
  explicitly and never writes line endings to advance between screen rows.
  Scrollbar glyphs are positioned before writing. Tiny dimensions use saturated
  geometry and leave key handling active even when there is no content area.
- A terminal guard tracks acquired raw mode, alternate screen and cursor hiding.
  It explicitly restores them on normal/editor exits and attempts restoration
  on error/unwind; painting and event failures are reported to the caller.
- Resize re-renders source and recomputes search's existing line-index matches;
  scroll offsets are clamped once to the new layout. Full source-anchor/search
  redesign remains deferred. The width-aware and post-editor entry paths pass
  original Markdown, not an already-rendered ANSI string.
- Tests assert exact text, display-column limits, adjacent inline styles,
  partial constructs, Unicode, gutters and overflow. Linux PTY tests decode
  terminal output and assert the visible first line and zero unintended screen
  scrolling, including the tagged compliance document. Each viewer subprocess
  runs in a new session with the PTY slave as its controlling terminal, so
  terminal queries and raw mode cannot use the test runner's inherited terminal.
  Run this coverage both with and without an inherited controlling terminal.

No new background work, async runtime, unsafe code, terminal graphics, persistent
UI state or session-format changes are part of this correction.
