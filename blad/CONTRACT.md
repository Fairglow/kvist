<!-- kvist-contract-version: 1 -->

# Blad Contract

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Boundary and ownership

- **Component ID:** `kvist.blad`
- **Contract IDs:** `blad.cli/v1`
- **Owner:** Blad component
- **Consumers:** human CLI users, automation, scripts

Blad is a terminal Markdown viewer that reads skott session transcripts and
standalone Markdown files. It owns rendering, navigation, and interactive
display. It does not own session creation or modification.

## Provided interfaces

The `blad` executable provides:

- `blad [OPTIONS] <TARGET>` — smart shortcut that resolves session IDs, paths, or files
- `blad list [--limit <n>]` — list recent skott sessions with metadata
- `blad show <session-id>` — view a completed session transcript interactively
- `blad stream <session-id>` — follow an active session transcript
- `blad view <file.md>` — view a standalone Markdown file interactively
- `blad export <session-id> [OUTPUT.md]` — export a session to Markdown document

### Options

| Option | Description |
|--------|-------------|
| `--log-dir <PATH>` | Directory containing skott session runs (default: `.skott/runs`) |
| `--dump` | Output plain text without interactive viewer |
| `--line-numbers` | Show rendered-line numbers in the interactive viewer |

### Keyboard shortcuts (interactive viewer)

Navigation: `↑`/`k`, `↓`/`j`, `PageUp`/`u`, `PageDown`/`d`, `g`/`H` (top), `G`/`L` (bottom), mouse wheel.
Search: `/` enter search mode, type query, `Enter` exit, `n`/`p` navigate matches, `Esc` cancel.
Other: `e` edit file (requires `$EDITOR`), `q`/`Esc` quit.

## Required interfaces

Blad depends on:

- Skott's session format: `.skott/runs/<session-id>/session.json` and `messages/` directory
- `$EDITOR` environment variable for edit mode
- Optional: `mmdc` executable for Mermaid diagram rendering

## Data and schemas

Blad reads skott's session format and produces ANSI-colored terminal output.

### Session format (input)

```
.skott/runs/
└── {session-id}/
    ├── session.json    # Manifest with metadata
    └── messages/
        ├── 000001.md
        ├── 000002.md
        └── ...
```

The session manifest (`session.json`) contains:
- `session_id`: Unique session identifier
- `model`: Model identifier used
- `created_at`: ISO 8601 timestamp
- `status`: "completed" or other status
- `entries`: Array of message entries with sequence, kind, path, etc.

### Output format

Blad renders Markdown to ANSI-colored terminal output with:
- Syntax-highlighted code blocks (via syntect)
- Bold, italic, strikethrough formatting
- Table rendering with column alignment
- GFM alert blockquotes with color-coded labels
- Footnote references and definitions
- Math expressions (inline and display)

### Width and viewport semantics

Interactive prose and headings wrap to the terminal width minus one scrollbar
column and, when requested, the rendered-line-number gutter. There is no
additional wrap margin. Resize recalculates that budget. Code block lines and
atomic inline-code spans are not broken to fit the viewport; the existing
Left/Right keys reveal horizontal overflow without resetting on shorter lines.
Home/End retain their current vertical behavior.

Each screen row is positioned and clipped independently, including header/footer
text. Wide graphemes may extend past the content column into the scrollbar area.

## Behavioral guarantees

- Session listing is deterministic, sorted by creation time (newest first)
- All operations are local; no network access required
- I/O errors are reported clearly; failures do not crash the interactive session
- Paths are validated; symlinks are not followed when reading session artifacts
- Rendering is fast even for large transcripts (thousands of messages)
- The viewer retains the first document line at the first content row
- Prose and headings wrap at the available display-column width
- Resize triggers reflow to the new content width

## Errors and failure semantics

- Invalid session ID: clear error message with available IDs
- File not found: standard CLI error with path
- Terminal too small: functional with minimum dimensions, no panic
- Missing optional tools (mmdc): fallback to plain text with note

## Security and authority

- Read-only access to skott session files
- No write operations without explicit edit mode
- No network access
- No credential or secret exposure

## Compatibility and verification

- Linux only (uses crossterm for terminal interaction)
- Depends on existing workspace dependencies: clap, serde, pulldown-cmark, syntect, crossterm
- Version tracking via Kvist artifact version markers