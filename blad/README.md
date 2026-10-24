# Blad

A terminal Markdown viewer with tabbed browsing, link navigation, and search. Blad renders Markdown documents directly in your terminal with syntax highlighting, math notation, and diagram support — no browser, no network, no configuration required.

## Features

- **Tabbed browsing** — Open multiple documents, switch between them, and navigate back through history
- **Interactive navigation** — Follow links, jump between headings, spatial link selection
- **Search** — Forward search with regex support, navigate matches
- **Rich rendering** — Syntax-highlighted code blocks, math notation, Mermaid diagrams
- **Theming** — Dark, light, and terminal-native themes with custom JSON theme support
- **Editor integration** — Open documents in your external editor and reload changes
- **Plain output** — Dump rendered text without terminal escapes for scripting

## Quick Start

```bash
# Open a single file
blad README.md

# Open multiple files as tabs
blad README.md GUIDE.md ARCHITECTURE.md

# With line numbers
blad --line-numbers README.md

# Plain text output (no terminal viewer)
blad --dump README.md > readme.txt
```

## Install

Install from the repository:

```bash
# From repository root
cargo install --locked --path blad

# Or from the blad/ directory
cd blad && cargo install --locked --path .
```

Requires Rust 1.95 or newer.

## Usage

```bash
blad [OPTIONS] FILE...

Options:
  --theme <THEME>           Terminal appearance (auto, dark, light, terminal)
  --theme-file <PATH>       Custom JSON theme file
  --color <MODE>            Color policy (auto, always, never)
  --line-numbers            Show line numbers in viewer
  --dump                    Output plain text without interactive viewer
  --math <MODE>             Math rendering (unicode, source)
  --mermaid <MODE>          Mermaid rendering (text, source)
  --diagram-charset <SET>   Diagram character set (unicode, ascii)
  --version                 Show version
  --help                    Show help
```

Filenames need no particular extension. Use `--` before filenames beginning with `-`.

## Viewer Keyboard Shortcuts

### Navigation

| Keys | Action |
|------|--------|
| `↑`/`k`, `↓`/`j` | Scroll one line |
| `PageUp`/`Ctrl+B`, `PageDown`/`Ctrl+F`/`Space` | Scroll full page |
| `Shift+PageUp`/`u`/`Ctrl+U`, `Shift+PageDown`/`d`/`Ctrl+D` | Scroll half page |
| `Home`, `End` | Beginning/end of line |
| `Ctrl+Home`/`g`/`H`, `Ctrl+End`/`G`/`L` | Top/bottom of document |
| `Left`/`Right` | Scroll horizontally |
| `[`/`]` | Previous/next heading |

### Search

| Keys | Action |
|------|--------|
| `/` | Start or refine forward search |
| `Ctrl+R` (in search) | Toggle regex mode |
| `n`/`>` | Next match |
| `N`/`<` | Previous match |
| `Esc` | Cancel search |
| `Enter` | Confirm search |

### Links

| Keys | Action |
|------|--------|
| `Tab`/`Shift+Tab` | Navigate links |
| `Enter` | Follow selected or first visible link |
| `Ctrl+Arrows` | Spatial link navigation |
| `f` | Enter link mode |

### Tabs

| Keys | Action |
|------|--------|
| `Alt+Left`/`Alt+Right` | Switch tabs |
| `Ctrl+PageUp`/`Ctrl+PageDown` | Switch tabs |
| `b` | Go back in tab history |
| `q`/`Esc` | Close current tab |

### Other

| Keys | Action |
|------|--------|
| `e` | Open in external editor |
| `Ctrl+R`/`F5` | Reload file |
| `Ctrl+L` | Redraw |
| `F1`/`?`/`h` | Show help |
| `Ctrl+Q`/`Ctrl+C` | Quit all tabs |

The mouse wheel scrolls; left-click a link to follow it.

## Rendering

Blad renders:

- Headings, emphasis, strikethrough, inline/fenced code
- Lists (ordered, unordered, task lists)
- Blockquotes, links, footnotes, tables
- Syntax-highlighted code blocks (via syntect)
- Math notation (Unicode output with source fallback)
- Mermaid diagrams (text-only rendering)
- Alert callouts (NOTE, TIP, IMPORTANT, WARNING, CAUTION)

Blad uses CommonMark parsing with selected extensions. It is not a full CommonMark or GFM implementation — raw HTML is omitted, images show alt text only, and some table features are not supported.

## Architecture

Blad consists of:

- **Blad CLI** — The interactive terminal viewer with tabbed browsing
- **Blad library** — Reusable Markdown rendering and parsing (published as `kvist-blad`)

The library can be used independently for Markdown rendering in other Rust applications.

## License

See `LICENSE` in the repository root.