# Blad

First-class terminal Markdown viewer with streaming and theming.

## Features

- Full CommonMark rendering with tables, strikethrough, and task lists
- Syntax-highlighted code blocks via syntect (50+ languages)
- Pluggable color themes
- File streaming for live content (`blad stream`)
- Stdin piping support (`cat file.md | blad`)

## Usage

```bash
# View a file
blad document.md

# View piped content
cat document.md | blad

# Use a specific theme
blad --theme dracula document.md

# Stream a live file
blad stream live.md
```

## Themes

Blad uses [syntect](https://github.com/trishume/syntect) for syntax highlighting and supports all its built-in themes:

- base16-ocean.dark (default)
- dracula
- monokai
- gruvbox-dark
- solarized-dark
- ...and many more

List available themes programmatically via the syntect API.

## Architecture

- `src/main.rs` - CLI entry point
- `src/lib.rs` - Library exports
- `src/markdown.rs` - Markdown rendering with syntax highlighting

## Dependencies

- pulldown-cmark (Markdown parsing)
- syntect (syntax highlighting)
- clap (CLI parsing)
