# Blad

First-class terminal Markdown viewer with interactive navigation, search, and
rich rendering. Blad reads standalone Markdown files and, with optional
session support, skott agent transcripts, using CommonMark and GitHub-Flavored
Markdown support.

Blad is a standalone binary (`blad`) that is independent of the `kvist` CLI:
it needs no Kvist project and no skott configuration. The default build
opens files directly; the `sessions` Cargo feature adds transcript commands
and `.skott/runs/` discovery (or a `--log-dir` override). It is also the
Markdown rendering library that `skott`
uses for its live TUI transcript, so the rendering you see in skott is the same
rendering blad uses to display sessions afterwards.

## Features

- **Full CommonMark + GFM rendering:** Headers, bold, italic, strikethrough,
  links, images, code blocks, tables, blockquotes, task lists and footnotes
- **Text notation:** Curated Unicode sub/superscripts and bounded mathematical
  text, with whole-expression source fallback for unsupported TeX
- **Syntax highlighting:** 50+ languages via syntect
- **Appearance:** Dark, light, or terminal-native palettes and `--theme-file`
  JSON overrides; explicit color
  capabilities for embedding applications and escape-free rendered dumps
- **Interactive viewer:** Keyboard navigation, mouse wheel, search with
  highlight, scrollbar, link navigation (Tab/Shift+Tab), Space for page down
- **Edit mode:** Press `e` to edit files in your preferred editor
- **Link following:** Tab/Shift+Tab and Enter without a special mode, or click
  inline/named links; local/cross-file heading jumps and open-file tabs
- **Optional sessions:** List and read skott transcript snapshots, or export
  them to combined Markdown documents (`sessions` feature)
- **Text-only Mermaid:** Spatial Unicode/ASCII boxes, connectors and labels for
  admitted flowchart, sequence, state, class and ER diagrams; no graphics/tools
- **GFM alerts:** Color-coded [!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION]

## Prerequisites

**Building.** Rust **1.95** (MSRV) on Linux. Blad is a workspace member, so
`cargo build -p blad` at the repository root builds the standalone viewer;
`cargo build -p blad --features sessions` includes session integration.
Neither needs the other workspace components at runtime.
The Linux viewer tests need `setsid` and `stty` from util-linux/coreutils; they
attach each child to its own controlling PTY, so they run unchanged from an
interactive terminal or a noninteractive CI runner.

**Running.** A terminal. No network access, no skott configuration, and no Kvist
project are required. Edit mode requires `$EDITOR`. Markdown rendering never
queries stdin or launches document-selected commands.

## Usage

```bash
# Open files in tabs, preserving argument order
blad README.md GUIDE.md
blad --version

# A filename matching an enabled command
blad -- session

# These commands require a build with --features sessions
blad list
blad session <session-id-or-directory>
blad export <session-id> --output output.md

# Plain text output (no interactive viewer)
blad session <session-id> --dump

# Show rendered-line numbers in the interactive viewer
blad --line-numbers README.md

# Choose appearance independently of terminal color capability
blad --theme light README.md
blad --theme terminal --color never README.md
blad --theme-file blad/docs/theme.example.json README.md

# Plain rendered text, even when color is explicitly requested
blad --color always --dump README.md

# Text boxes/arrows, with ASCII structural glyphs
blad --diagram-charset ascii diagrams.md

# Inspect unprojected expressions and diagram source
blad --math source --mermaid source document.md
```

General viewer options, including `--help` and `--version`, remain available
without session support. Only session commands and `--log-dir` are gated.
There is no `view`, `show`, or `stream` command, nor session autodetection.
Active sessions are snapshots; re-run `session` for newer events.

Standalone files must be regular UTF-8 files of at most **16 MiB each**,
including new link targets, reloads and editor rereads. Oversized files fail
explicitly rather than being truncated. Selected file/theme symlinks still
resolve normally; substituted final symlinks are rejected on reload/reread.
This bounds source input, not total tab memory or rendering time.

## Keyboard Shortcuts

### Navigation

- `↑`/`k` - Up, `↓`/`j` - Down
- `PageUp`/`Ctrl+B` - Page up, `PageDown`/`Ctrl+F`/`Space` - Page down
- `u`/`d` or `Ctrl+U`/`Ctrl+D` - Half page up/down
- `g`/`H` - Top, `G`/`L` - Bottom
- Mouse wheel - Scroll

### Search

- `/` / `?` - Forward / backward literal search; `Ctrl+R` toggles regex
- `n` - Repeat direction, `N`/`p` - Reverse direction (wraps)
- `Enter` - Exit search, `Esc` - Cancel

### Link Navigation

- `f` - Enter optional link navigation mode
- `Tab` - Next link, `Shift+Tab` - Previous link
- `Ctrl+Arrows` - Select links by visual position
- `[` / `]` - Previous / next heading
- `Enter` - Follow link (open file or URL)
- `Esc` - Exit link mode

Links can also be followed without link mode: press `Enter` to open the first
link at or below the top of the viewport. Bare URLs (`https://…`) are
recognised as links as well as bracketed and named links.
Tab/Shift+Tab select links directly; rendered link labels also accept a left
mouse click. `#heading` and `other.md#heading` jump to their headings.

### Other

- `e` - Edit file
- `b` - Return to the previous file
- `Alt+Left` / `Alt+Right` - Switch open-file tabs (top row)
- `q`/`Esc` - Close current tab (exit on the last tab)
- `Q`/`Ctrl+C` - Quit all tabs
- `h` - Keyboard help, `Ctrl+L` - Repaint
- `r` - Reload the active file, retaining position and options

Browser launches time out after five seconds without closing the document;
only the directly owned launcher is stopped, not an independent browser.

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

The supported script, math and Mermaid profiles and their explicit fallback/
resource behavior are documented in the [User's Guide](docs/USAGE.md).
These are bounded text projections, not full TeX or full Mermaid compatibility.
Library hosts can supply their own palette/capabilities and cooperative
cancellation without inheriting CLI environment detection.
