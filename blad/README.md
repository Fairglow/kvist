# Blad

First-class terminal Markdown viewer with interactive navigation, search, and
rich rendering. Blad reads skott agent session transcripts and standalone
Markdown files, and renders them with full CommonMark and GitHub-Flavored
Markdown support.

Blad is a standalone binary (`blad`) that is independent of the `kvist` CLI:
it needs no Kvist project and no skott configuration. It reads skott session
transcripts from `.skott/runs/` (or a `--log-dir` override) and any Markdown
file you point it at. It is also the Markdown rendering library that `skott`
uses for its live TUI transcript, so the rendering you see in skott is the same
rendering blad uses to display sessions afterwards.

## Features

- **Full CommonMark + GFM rendering:** Headers, bold, italic, strikethrough,
  links, images, code blocks, tables, blockquotes, task lists and footnotes
- **Text notation:** Curated Unicode sub/superscripts and bounded mathematical
  text, with whole-expression source fallback for unsupported TeX
- **Syntax highlighting:** 50+ languages via syntect
- **Appearance:** Dark, light, or terminal-native palettes; explicit color
  capabilities for embedding applications and escape-free rendered dumps
- **Interactive viewer:** Keyboard navigation, mouse wheel, search with
  highlight, scrollbar
- **Edit mode:** Press `e` to edit files in your preferred editor
- **Session viewing:** Read and display skott agent session transcripts
- **Live streaming:** Follow an active session transcript as it streams
- **Export:** Export sessions to combined Markdown documents
- **Mermaid support:** Detects and attempts to render mermaid diagrams
- **GFM alerts:** Color-coded [!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION]

## Prerequisites

**Building.** Rust **1.95** (MSRV) on Linux. Blad is a workspace member, so
`cargo build -p blad` at the repository root builds it together with its
dependency (`sav`). No other tools are needed to build or run the test suite.
The Linux viewer tests need `setsid` and `stty` from util-linux/coreutils; they
attach each child to its own controlling PTY, so they run unchanged from an
interactive terminal or a noninteractive CI runner.

**Running.** A terminal. No network access, no skott configuration, and no Kvist
project are required. Edit mode requires `$EDITOR`. Markdown rendering never
queries stdin or launches document-selected commands.

## Usage

```bash
# List recent sessions
blad list

# View a session
blad show <session-id>

# Follow an active session as it streams
blad stream <session-id>

# View a markdown file
blad view <file.md>

# Smart shortcut: auto-detects a session ID or a file path
blad <session-id-or-file>

# Export a session
blad export <session-id> [output.md]

# Plain text output (no interactive viewer)
blad show <session-id> --dump

# Show rendered-line numbers in the interactive viewer
blad show <session-id> --line-numbers

# Choose appearance independently of terminal color capability
blad --theme light view README.md
blad --theme terminal --color never view README.md

# Plain rendered text, even when color is explicitly requested
blad --color always --dump view README.md
```

## Keyboard Shortcuts

### Navigation

- `↑`/`k` - Up, `↓`/`j` - Down
- `PageUp`/`u` - Page up, `PageDown`/`d` - Page down
- `g`/`H` - Top, `G`/`L` - Bottom
- Mouse wheel - Scroll

### Search

- `/` - Enter search
- `n` - Next match, `p` - Previous match
- `Enter` - Exit search, `Esc` - Cancel

### Other

- `e` - Edit file
- `q`/`Esc` - Quit

## Documentation

- [User's Guide](docs/USAGE.md) - Complete usage documentation
- [Text-Only Rendering Plan](docs/TEXT_RENDERING_PLAN.md) - Accepted themes,
  Unicode scripts, limited math, and text-box Mermaid support
- [Design](DESIGN.md) - Component design
- [Requirements](REQUIREMENTS.md) - Functional and non-functional requirements
- [Contract](CONTRACT.md) - CLI interface specification

## Development

```bash
# Build
cargo build -p blad

# Run tests
cargo test -p blad
```

Blad is the single source of truth for Markdown parsing and rendering in the
workspace. `skott` depends on the `blad` library and converts blad's ANSI output
to ratatui-compatible styled lines for its TUI. See
[`skott/README.md`](../skott/README.md) for the consumer side.
