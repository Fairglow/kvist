# Blad Contract

## Version

`blad.cli/v1`

## CLI Interface

### Commands

```
blad [OPTIONS] <TARGET>
blad [OPTIONS] <COMMAND> [ARGS]
```

### Options

| Option             | Description                                                      |
| ------------------ | ---------------------------------------------------------------- |
| `--log-dir <PATH>` | Directory containing skott session runs (default: `.skott/runs`) |
| `--dump`           | Output plain text without interactive viewer                     |

### Commands

#### `blad list [--limit <n>]`

List recent skott sessions with metadata.

```bash
blad list
blad list --limit 5
```

#### `blad show <session-id>`

View a completed session transcript interactively.

```bash
blad show 20261012T120000Z-12345
blad show /path/to/session-dir
```

#### `blad stream <session-id>`

Follow a session transcript (same as show, but indicates live/active sessions).

```bash
blad stream 20261012T120000Z-12345
```

#### `blad view <file.md>`

View a standalone Markdown file interactively.

```bash
blad view README.md
blad view /path/to/document.md
```

#### `blad export <session-id> [OUTPUT.md]`

Export a session to a combined Markdown document.

```bash
blad export 20261012T120000Z-12345
blad export 20261012T120000Z-12345 --output transcript.md
```

#### Smart Shortcut

```bash
# Treats .md files as file paths
blad README.md

# Treats session directory paths
blad /path/to/session-dir

# Treats session IDs (exact or prefix match)
blad 20261012T120000
```

## Keyboard Shortcuts (Interactive Viewer)

### Navigation

| Key              | Action               |
| ---------------- | -------------------- |
| `↑` / `k`        | Scroll up one line   |
| `↓` / `j`        | Scroll down one line |
| `PageUp` / `u`   | Scroll up one page   |
| `PageDown` / `d` | Scroll down one page |
| `g` / `H`        | Go to top            |
| `G` / `L`        | Go to bottom         |
| Mouse wheel      | Scroll               |

### Search

| Key       | Action               |
| --------- | -------------------- |
| `/`       | Enter search mode    |
| Type text | Search query         |
| `Enter`   | Exit search mode     |
| `n`       | Next match           |
| `p`       | Previous match       |
| `Esc`     | Exit search / cancel |

### Other

| Key         | Action                       |
| ----------- | ---------------------------- |
| `e`         | Edit file (requires $EDITOR) |
| `q` / `Esc` | Quit viewer                  |

## Input/Output Format

### Session Format (Input)

Blad reads skott's structured session format from `.skott/runs/`:

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

### Markdown Rendering (Output)

Blad renders Markdown to ANSI-colored terminal output with:

- Syntax-highlighted code blocks (via syntect)
- Bold, italic, strikethrough formatting
- Table rendering with column alignment
- GFM alert blockquotes with color-coded labels
- Footnote references and definitions
- Math expressions (inline and display)

## Exit Codes

| Code | Meaning                                   |
| ---- | ----------------------------------------- |
| 0    | Success                                   |
| 1    | Error (file not found, parse error, etc.) |
