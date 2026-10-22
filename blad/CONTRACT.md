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
| `--theme auto\|dark\|light\|terminal` | Appearance; defaults to `auto`, conservatively dark when unknown |
| `--color auto\|always\|never` | Color policy; defaults to `auto`; explicit `always` overrides `NO_COLOR` |
| `--math unicode\|source` | Supported mathematical text projection or original TeX; defaults to `unicode` |
| `--mermaid text\|source` | Supported text diagrams or original source; defaults to `text` |
| `--diagram-charset unicode\|ascii` | Structural diagram glyphs; defaults to `unicode`; preserves authored Unicode |

`--dump` MUST force escape-free rendered text regardless of color/theme flags.
Automatic CLI color MUST honor a nonempty `NO_COLOR`, output TTY state and
`TERM=dumb`. Color depth is independent of appearance. No terminal query is
required: automatic appearance MAY use environment hints, defaulting to dark.
Exports MUST retain source Markdown and MUST NOT render its math or diagrams.

The library exposes typed rendering options and host-provided palette/color
capabilities through `MarkdownRenderer`. Library defaults are deterministic;
embedding hosts supply their own policy rather than inheriting CLI environment
guessing. Width, palette, math mode and diagram options persist across reflow.

### Keyboard shortcuts (interactive viewer)

Navigation: `↑`/`k`, `↓`/`j`, `PageUp`/`u`, `PageDown`/`d`/`Space`, `g`/`H` (top), `G`/`L` (bottom), mouse wheel.
Search: `/` enter search mode, type query, `Enter` exit, `n`/`p` navigate matches, `Esc` cancel.
Links: `f` enter link mode, `Tab`/`Shift+Tab` cycle links, `Enter` follow the selected link, `Esc` exit link mode.
Other: `e` edit file (requires `$EDITOR`), `b` return to the previous file, `q`/`Esc` quit.

## Required interfaces

Blad depends on:

- Skott's session format: `.skott/runs/<session-id>/session.json` and `messages/` directory
- `$EDITOR` environment variable for edit mode
- A pinned, vendored native Rust text renderer for admitted Mermaid families;
  no Mermaid runtime executable or browser is required

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

### Table layout semantics

Tables MUST preserve their existing borderless, reverse-video header and
alternating data-row backgrounds when styling permits, with ` | ` between
columns. Terminal-native/monochrome/plain output MAY omit fixed backgrounds
without changing geometry. The width budget
includes these separators. A row's height is the maximum wrapped cell height;
shorter cells are padded to that height.

Under the fixed terminal geometry model (display-column width : line height =
1:2), each cell's width floor MUST come from its own nonempty wrapped text
lines: four display columns per occupied text line, capped at the cell's
natural width (its widest normalized explicit line without soft wrapping).
Empty explicit lines and vertical padding MUST NOT contribute to this floor.
A fully unwrapped value MUST NOT be widened solely for aspect ratio. This
models a visual 2:1 text ratio where possible without adding artificial width;
actual font metrics are not queried.

Each column's lower bound MUST be the maximum of its cells' text-derived
floors. Its upper bound MUST be the maximum of its cells' natural widths.
An entirely empty column MUST occupy zero content columns; separators remain.
Neither a tall neighboring cell nor unused viewport space may widen a column
beyond that upper bound.

If a valid layout fits, the renderer MUST choose the smallest possible maximum
row height within the width budget. If none fits, it MUST overflow horizontally
using the smallest valid total width, breaking ties by smaller maximum row
height. Left/Right navigation continues to reveal overflow. With no width
configured, columns MUST use exactly their natural widths without soft
wrapping. Spare viewport space MAY be distributed deterministically
up to natural column widths; total table height is not a globally optimized
objective.

Cells MUST wrap greedily between whole whitespace-delimited tokens. Intra-line
whitespace becomes one space; explicit newlines, including empty lines, remain.
HTML `<br>`, `<br/>`, and `<br />` tags (case-insensitive) inside table cells
MUST contribute explicit newlines.
Unicode display width determines measurement, not byte or character count.
Tokens MUST NOT be split, even when a long identifier or URL forces overflow.
Terminal controls are made inert by the same normalization used by viewport
text. Resizing MUST solve the layout again, not shrink an already valid layout
below its aspect floor.

### Script and mathematical text

Markdown `^...^` and `~...~` spans, including adjacent `E=mc^2^` and `H~2~O`,
MUST use a curated whole-span Unicode mapping or explicit `^(...)`/`_(...)`
fallback. Code and escaped notation MUST remain literal. These are Blad dialect
extensions, not a claim of universal GFM syntax.

Math uses parser-recognized `$...$` and `$$...$$`. The admitted grammar comprises
literals, nested groups, single/grouped scripts, the documented Greek/operator
commands, `\frac`, `\sqrt`, `\text`, and paired supported `\left`/`\right`
delimiters. Fractions and roots MUST retain grouping; unsupported or malformed
expressions MUST preserve the complete original source and delimiters with a
visible explanation. Display math is a separate text block, never graphics.
Escaped dollars and unmatched delimiters remain literal; currency-only
expressions MUST NOT be converted to mathematics.

### Mermaid text profile

Mermaid fences MUST produce spatial text diagrams for admitted flowchart,
sequence, state, class and ER syntax. Unsupported semantics MUST retain source
with an explanation rather than disappear or become a success placeholder.
The admitted profile and approximated presentation MUST be documented.
Diagrams MUST retain authored direction, labels and endpoints and MUST NOT
silently rotate, truncate labels, or wrap completed diagram rows to fit.
Unicode/ASCII selection affects structural glyphs only.

Resource-limit failures MUST be visible and distinct from unsupported syntax.
Cancellation and writer failures MUST propagate rather than become source
fallback successes. No document-triggered external command is permitted.

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
- Unsupported math/diagram syntax: original source with an informative note
- Rendering resource limit: original source and a distinct limit diagnostic,
  or an explicit error if bounded source output cannot be produced

## Security and authority

- Read-only access to skott session files
- No write operations without explicit edit mode
- No network access
- No credential or secret exposure

## Compatibility and verification

- Linux only (uses crossterm for terminal interaction)
- Depends on existing workspace dependencies: clap, serde, pulldown-cmark, syntect, crossterm
- Version tracking via Kvist artifact version markers