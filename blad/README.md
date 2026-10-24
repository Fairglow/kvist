# Blad

A standalone terminal Markdown viewer and reusable Rust renderer. Open local
documents in tabs, follow links, search rendered text, and hand a file to your
editor without needing a project configuration, service, credentials or network.

The planned registry package is **`kvist-blad` 0.3.0**; the executable and Rust
library are both **`blad`**. **This release is not yet published.**
Linux x86_64 is the verified executable target. Rust **1.95 or newer** is required
to build it: 1.95 is the minimum supported version, not an exact toolchain pin.

## Install

Install the current checkout from the repository root:

```bash
cargo install --locked --path blad
```

Or run `cargo install --locked --path .` from the `blad/` directory.
For the planned registry release, once published:

```bash
cargo install --locked kvist-blad --version 0.3.0
```

## Use

```bash
blad README.md GUIDE.md                   # Files in opening-order tabs
blad --line-numbers README.md             # Rendered-line numbers
blad --theme light README.md
blad --theme terminal --color never README.md
blad --theme-file blad/docs/theme.example.json README.md
blad --dump README.md > readme.txt        # Escape-free rendered text
blad --diagram-charset ascii diagrams.md
blad --math source --mermaid source document.md
blad --version
blad --help
```

Paths above assume the repository root. Filenames need no particular extension;
use `--` before filenames beginning with `-`. There is **no stdin/piped-input
mode**: provide file paths. When stdin or stdout is not a terminal, Blad renders
the supplied files without opening the interactive viewer. Use `--dump` when
the output must contain no terminal escapes; `--color never` can retain emphasis.

## Rendering

- Headings, emphasis, strikethrough, inline/fenced code, lists, blockquotes,
  links, footnotes, tables and `[x]`/`[ ]` task markers.
- Syntax highlighting through syntect; unknown languages remain readable text.
- Dark, light and terminal-native appearances; strict explicit JSON theme
  overrides, automatic color capability detection and typed library options.
- Curated Unicode scripts and bounded mathematical text, with whole-expression
  source fallback for unsupported TeX.
- Spatial text-only Mermaid flowchart, sequence, state, class and ER subsets,
  with Unicode or ASCII structure and source fallback.
- Alert labels and edges for NOTE, TIP, IMPORTANT, WARNING and CAUTION.

This is a terminal projection using CommonMark parsing and selected extensions,
**not full CommonMark or GFM conformance**. Raw HTML is omitted (table-cell
`<br>` is a line-break exception), images show alternative text only, and table
delimiter left/center/right alignment is not implemented. Math and Mermaid are
bounded subsets, not full TeX or Mermaid compatibility; see the
[complete profiles and limits](https://github.com/Fairglow/kvist/blob/main/blad/docs/USAGE.md#text-rendering).

## Viewer keys

| Keys | Action in normal mode |
|------|-----------------------|
| `↑`/`k`, `↓`/`j` | One line up/down |
| `PageUp`/`Ctrl+B`, `PageDown`/`Ctrl+F`/`Space` | Full pages |
| `Shift+PageUp`/`u`/`Ctrl+U`, `Shift+PageDown`/`d`/`Ctrl+D` | Half pages |
| `Home`, `End` | Horizontal beginning/end of line |
| `Ctrl+Home`/`g`/`H`, `Ctrl+End`/`G`/`L` | Document top/bottom |
| `Left`/`Right` | Horizontal scrolling |
| `/` | Start or refine forward search; `Ctrl+R` toggles regex in prompt |
| `n`/`>`, `N`/`<` | Next/previous search match |
| `Tab`/`Shift+Tab`, `Enter` | Select links, follow selection or first link at/below viewport |
| `Ctrl+Arrows`, `[`/`]` | Spatial link selection, previous/next heading |
| `Alt+Left`/`Alt+Right`/`Ctrl+PageUp`/`Ctrl+PageDown`, `b` | Switch tabs, return through tab history |
| `e`, `Ctrl+R`/`F5`, `Ctrl+L`, `F1`/`?`/`h` | External editor, reload, repaint, help |
| `q`/`Esc`, `Ctrl+Q`/`Ctrl+C` | Close one tab, quit all tabs |

The mouse wheel scrolls; left-click a rendered link label to follow it.
Optional `f` link mode uses Tab/Shift+Tab, Enter and Esc. Modes take precedence:
Esc cancels an open search prompt or link mode; **after Enter commits a search,
Esc closes the tab**. Help consumes the next key, including `Q`; **Ctrl+C quits
globally**, even from help/search. See the
[exact key and modal behavior](https://github.com/Fairglow/kvist/blob/main/blad/docs/USAGE.md#keyboard-precedence).

`e` runs whitespace-split `$EDITOR` directly (unset: `nvim`), without shell
expansion, then rereads after a successful editor exit. Explicitly following
an external URL runs `xdg-open`, with a five-second launcher wait and bounded
captured diagnostics. Neither action occurs merely by rendering a document.

## Input and resource boundaries

Documents must be regular UTF-8 files, at most **16 MiB (16,777,216 bytes) each**.
All initial files validate before terminal entry or dump output. Newly followed
files, reloads and editor rereads use the same limit. Explicitly selected
initial/link/theme symlinks resolve normally; rereads reject a substituted
final symlink at the opening canonical path. This is not ancestor-path isolation.

Limits are per-file source bounds, **not streaming, aggregate-memory, rendering
latency or hard I/O-time guarantees**. Rendering can expand input. There is no
live follow, folding, embedded editor or `less`-style shell escape.

## Embed the renderer

For the planned registry release:

```toml
[dependencies]
blad = { package = "kvist-blad", version = "0.3.0" }
```

```rust
use blad::{MarkdownRenderer, RenderOptions};

fn main() -> std::io::Result<()> {
    let mut renderer = MarkdownRenderer::with_options(RenderOptions::plain());
    renderer.set_width(Some(80));
    let mut output = Vec::new();
    renderer.render_markdown("# Hello\n\nReadable **Markdown**.\n", &mut output)?;
    assert!(!output.contains(&0x1b));
    Ok(())
}
```

Renderer defaults are deterministic: dark appearance, truecolor, styling,
Unicode math, text diagrams and Unicode diagram structure, without a width
limit. Hosts explicitly choose palette, width, plain output and color depth
(`None`, `Ansi16`, `Ansi256`, `TrueColor`). The renderer does not query
`is_terminal()`, environment variables or terminal input, or launch processes.
Interactive helpers in `blad::display` have separate terminal/process behavior.

## Documentation and development

- [User's guide](https://github.com/Fairglow/kvist/blob/main/blad/docs/USAGE.md): options, themes, keys and complete rendering profiles.
- [Examples](https://github.com/Fairglow/kvist/blob/main/blad/docs/EXAMPLES.md): documents to open in the viewer.
- [Theme example](https://github.com/Fairglow/kvist/blob/main/blad/docs/theme.example.json): all semantic palette roles.
- [Optional Kvist/Skott integration reference](https://github.com/Fairglow/kvist/blob/main/blad/docs/SESSIONS.md).

From the repository root:

```bash
cargo build --locked -p kvist-blad
cargo test --locked -p kvist-blad
```

The Linux viewer tests use `setsid` and `stty` to give children controlling PTYs.
Blad is licensed under AGPL-3.0-or-later; the source distribution includes the
[license text](https://github.com/Fairglow/kvist/blob/main/blad/LICENSE).
Separate commercial licensing is described by the project.
