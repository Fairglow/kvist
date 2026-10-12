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

```bash
# View session without interactive viewer
blad show 20261012T120000 --dump

# Specify custom log directory
blad list --log-dir /path/to/logs
```

## Interactive Viewer

The interactive viewer provides navigation, search, and edit capabilities.

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
