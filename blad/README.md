# Blad

First-class terminal Markdown viewer with interactive navigation, search,
and rich rendering.

## Features

- **Full CommonMark + GFM rendering:** Headers, bold, italic, strikethrough,
  links, images, code blocks, tables, blockquotes, task lists, footnotes,
  superscript/subscript, math expressions
- **Syntax highlighting:** 50+ languages via syntect
- **Interactive viewer:** Keyboard navigation, mouse wheel, search with
  highlight, scrollbar
- **Edit mode:** Press `e` to edit files in your preferred editor
- **Session viewing:** Read and display skott agent session transcripts
- **Export:** Export sessions to combined Markdown documents
- **Mermaid support:** Detects and attempts to render mermaid diagrams
- **GFM alerts:** Color-coded [!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION]

## Usage

```bash
# List recent sessions
blad list

# View a session
blad show <session-id>

# View a markdown file
blad view <file.md>

# Export a session
blad export <session-id> [output.md]

# Plain text output (no interactive viewer)
blad show <session-id> --dump
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
- [Architecture](ARCHITECTURE.md) - System design
- [Requirements](REQUIREMENTS.md) - Functional and non-functional requirements
- [Contract](CONTRACT.md) - CLI interface specification

## Development

```bash
# Build
cargo build -p blad

# Run tests
cargo test -p blad
```
