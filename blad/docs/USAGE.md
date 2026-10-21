# Blad User's Guide

Blad is a terminal Markdown viewer that displays skott agent session transcripts
and standalone Markdown files with rich formatting, interactive navigation,
and search.

## Quick Start

### View a recent session

```bash
# List recent sessions
blad list

# View a specific session by ID or prefix
blad show 20261012T120000
```

### View a Markdown file

```bash
# View any markdown file
blad view README.md

# Smart shortcut (auto-detects file vs session)
blad README.md
```

### Export a session

```bash
# Export to stdout
blad export 20261012T120000

# Export to file
blad export 20261012T120000 --output transcript.md
```

## Commands

### `blad list [--limit N]`

List recent skott sessions with metadata (ID, model, message count, status).

```bash
blad list            # List 20 most recent
blad list --limit 5  # List 5 most recent
```

### `blad show <session-id>`

View a completed session transcript interactively. Accepts full session ID,
ID prefix, or full path to session directory.

```bash
blad show 20261012T120000Z-12345
blad show 20261012T120000        # Prefix match
blad show /path/to/session-dir   # Path
```

### `blad stream <session-id>`

Follow a session transcript. Same as `show` but intended for active sessions.

### `blad view <file.md>`

View a standalone Markdown file with the interactive viewer.

```bash
blad view document.md
blad view /path/to/file.md
```

### `blad export <session-id> [OUTPUT.md]`

Export a session to a combined Markdown document suitable for sharing or
archival.

```bash
blad export 20261012T120000 > transcript.md
blad export 20261012T120000 --output transcript.md
```

## Global Options

| Option | Description |
|--------|-------------|
| `--log-dir PATH` | Directory containing skott sessions (default: `.skott/runs`) |
| `--dump` | Output plain text without interactive viewer |
| `--line-numbers` | Show rendered-line numbers in the interactive viewer |
| `--theme auto\|dark\|light\|terminal` | Appearance; auto uses `COLORFGBG`, otherwise dark |
| `--color auto\|always\|never` | Color independently of appearance; auto honors TTY, `NO_COLOR`, `TERM=dumb` |
| `--math unicode\|source` | Mathematical projection policy |
| `--mermaid text\|source` | Diagram projection policy |
| `--diagram-charset unicode\|ascii` | Diagram structural glyphs, not authored labels |

```bash
# View session without interactive viewer
blad show 20261012T120000 --dump

# Specify custom log directory
blad list --log-dir /path/to/logs
```

Dark and light appearances choose matching syntax highlighting and subtle panel
fills. Terminal-native appearance leaves code and table backgrounds unchanged.
Automatic color uses `COLORTERM=truecolor`/`24bit`, then `TERM=*256color*`, then
the standard sixteen colors. A nonempty `NO_COLOR` disables automatic colors;
explicit `--color always` overrides it. Monochrome retains emphasis.
`--dump` removes **all** styling, including authored terminal escapes, and
renders message bodies consistently for files, sessions and stream snapshots.
`export` instead preserves the original Markdown.

Library callers use `MarkdownRenderer::with_options(RenderOptions { .. })`.
The options contain a host-provided semantic `Palette`, `ColorDepth`, appearance,
and projection choices. Library defaults never inspect environment variables,
read stdin, issue OSC queries, or launch subprocesses. Existing width, syntax
theme and panel-background setters remain available; unknown explicit syntax
themes return `InvalidInput`. Syntax definitions and built-in themes are reused
across renders.

## Interactive Viewer

The interactive viewer provides navigation, search, and edit capabilities.

Prose and headings wrap to the available terminal columns after reserving the
scrollbar and optional line-number gutter. Resize recalculates the layout.
Paragraphs and hard breaks start without extra leading spaces, and formatting
boundaries do not add spaces between adjacent characters.

Code block lines retain their whitespace and do not wrap. Use Left/Right to
read horizontal overflow; short lines no longer reset the horizontal position.
Header, footer and content rows are clipped independently so long lines cannot
scroll the first document lines off the screen. Tabs display at eight-column
stops. Home/End still move vertically; the planned key change, folding,
occurrence search and live follow are not part of these layout corrections.

### Table sizing

Table width floors depend on each cell's own soft-wrapped text, not the height
of neighboring cells. Empty lines and vertical padding never widen a column.
The renderer assumes terminal character cells are twice as tall as wide, so
it aims for four display columns per occupied text line (a visual 2:1 ratio).
Actual font dimensions are not detected.

A column never grows beyond its widest unwrapped text value. Short values and
explicitly multiline text are left at their natural widths when meeting the
ratio would require extra padding; entirely empty columns have zero content
width. Thus a multi-line description cannot inflate an adjacent ID or path.

Within the available width, the integer solver minimizes the tallest wrapped
row. If that cannot fit, it chooses the narrowest layout that preserves the
aspect floor and allows horizontal overflow; use Left/Right to read it.
Resizing recomputes the layout. This is exact minimax sizing, not a claim that
the sum of all row heights is globally minimal.

Cells wrap between whole words, never inside a Unicode grapheme or long file
path. A long unbroken token may therefore force overflow. Whitespace within a
cell line becomes one space; `<br>`, `<br/>`, and `<br />` preserve explicit
breaks and empty lines. Without a configured width, tables stay unwrapped.

### Navigation

| Key | Action |
|-----|--------|
| `↑` / `k` | Scroll up one line |
| `↓` / `j` | Scroll down one line |
| `PageUp` / `u` | Scroll up one page |
| `PageDown` / `d` | Scroll down one page |
| `g` / `H` | Go to top |
| `G` / `L` | Go to bottom |
| Mouse wheel | Scroll |
| `q` / `Esc` | Quit viewer |

### Search

Press `/` to enter search mode. Type your search query, then press `Enter`
to exit search mode and keep the matches highlighted.

| Key | Action |
|-----|--------|
| `/` | Enter search mode |
| Type text | Search query |
| `Enter` | Exit search, keep highlights |
| `n` | Next match |
| `p` | Previous match |
| `Esc` | Cancel search |

The footer shows match position (e.g., "3/12 matches" for match 3 of 12).

### Edit Mode

In the viewer for a file (not a session), press `e` to open the file in
your configured editor. After saving and closing the editor, Blad
re-renders the file with your changes.

Blad uses the `$EDITOR` environment variable. If not set, it defaults to
`nvim`.

```bash
# Set your preferred editor
export EDITOR="code --wait"
blad view document.md
# Press 'e' in the viewer to edit
```

## Markdown Features

Blad renders full CommonMark and GitHub Flavored Markdown (GFM):

- **Headers** (H1-H6) with color coding
- **Bold**, *italic*, ~~strikethrough~~ text
- Links and autolinks (URLs are clickable in supporting terminals)
- Images (shown as alt text in terminal)
- Code blocks with syntax highlighting (50+ languages)
- Inline code with inverted video
- Tables with column alignment
- Blockquotes, including GFM alert styles:
  - [!NOTE] - Blue
  - [!TIP] - Green
  - [!IMPORTANT] - Purple
  - [!WARNING] - Yellow
  - [!CAUTION] - Red
- Task lists
- Footnotes
- Superscript and subscript
- Math expressions (inline `$...$` and display `$$...$$`)
- Horizontal rules

### Mermaid Diagrams

Blad detects ` ```mermaid ` code blocks and attempts to render them using
`mmdc` (mermaid-cli). If `mmdc` is not installed, the diagram source is
displayed as plain text.

Install mermaid-cli:
```bash
npm install -g @mermaid-js/mermaid-cli
```

## Examples

### Review a coding session

```bash
# List recent sessions
blad list

# Find the session about the bug fix
blad show 20261012T140000

# Search for the bug description
# Press '/' then type "segmentation fault"
```

### View and edit documentation

```bash
# Open README
blad view README.md

# Edit it
# Press 'e', make changes in editor, save and close
# Blad re-renders automatically
```

### Export for sharing

```bash
# Export a session for a team member
blad export 20261012T140000 --output session-transcript.md
```

## Troubleshooting

### Viewer doesn't restore terminal

If the terminal appears corrupted after exiting Blad, run:
```bash
reset
```

### Search doesn't find text

Search is case-insensitive and operates on the rendered content. ANSI
escape sequences are stripped before matching.

### Mermaid diagrams not rendering

Ensure `mmdc` is installed and in your PATH:
```bash
which mmdc
```

### No color output

Check that your terminal supports ANSI colors. Most modern terminals do.
