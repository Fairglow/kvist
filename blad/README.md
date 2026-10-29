# Blad

A terminal Markdown viewer and streaming pager with tabbed browsing, link navigation, and search. Blad renders Markdown documents directly in your terminal with syntax highlighting, math notation, and diagram support — no browser, no network, no configuration required.

## Features

- **Tabbed browsing** — Open multiple documents, switch between them, and navigate back through history
- **Interactive navigation** — Follow links, jump between headings, spatial link selection
- **Search** — Forward search with regex support, navigate matches
- **Rich rendering** — Syntax-highlighted code blocks, math notation, Mermaid diagrams
- **HTML text** — Expanded details/summary, breaks and inline styles; unknown markup stays visible
- **YAML frontmatter** — Source-preserving metadata highlighting in any Markdown document
- **Theming** — Dark, light, and terminal-native themes with custom JSON theme support
- **Editor integration** — Open documents in your external editor and reload changes
- **Automatic reload** — See file saves without losing your scroll position; inactive tabs refresh when revisited
- **Streaming pager** — Pipe Markdown into an automatic live viewer; scroll back to pause following and press `G` to resume
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

# Automatically page incoming Markdown when output is a terminal
cat README.md | blad

# Disable paging, or redirect output for finite plain rendering
cat README.md | blad --dump
blad README.md | head -n 30

# Read a shell process substitution as a finite snapshot
blad <(printf '# Generated\n\nMarkdown content\n')
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
  --strip-html              Strip unsupported HTML markup, keeping its text
  --version                 Show version
  --help                    Show help
```

Filenames need no particular extension. Use `--` before filenames beginning with `-`.
With no files, redirected stdin supplies one document; `-` also selects stdin
among files, once. Use `./-` for a literal file named `-`. With terminal stdout,
stdin is paged **as it arrives**, including unfinished paragraphs and code.
Keyboard input uses the controlling terminal, separately from the pipe.
EOF leaves the document open; `q` closes it. The footer shows live following,
paused, EOF or a persistent input error. Scroll upward or search to pause
following; `G`/`Ctrl+End` resumes.

**Behavior change:** piped stdin now opens the viewer automatically when stdout
is a terminal. Use `--dump` to opt out; no pager flags are required. Without a
controlling terminal, automatic paging fails with a `--dump` hint.
Redirected stdout or `--dump` still renders one finite snapshot after EOF.
File and stdin sources are limited to 16 MiB each; this is not unlimited
scrollback or session/file tail-following. `--color always` retains styles in
redirected output; `--dump` always removes them. Closed downstream pipes exit quietly.

Shell process substitutions (`blad <(cmd ...)`) and inherited `/dev/fd/N` or
`/proc/self/fd/N` paths accept readable regular files and pipes as bounded
snapshots through EOF. They cannot be edited or automatically reloaded.
Relative links use the working directory. For live display before EOF, use
`cmd ... | blad` instead. Other named devices and FIFOs remain unsupported.

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
| `Ctrl+R` | Start forward search in regex mode |
| `Ctrl+R` (in search) | Toggle regex mode |
| `n`/`>` | Next match |
| `N`/`<` | Match in reverse search direction |
| `Esc` | Leave search and clear highlights |
| `q` (confirmed search) | Leave search without closing the tab |
| `Enter` | Confirm the query; then move to the next match |

### Links

| Keys | Action |
|------|--------|
| `Tab`/`Shift+Tab` | Navigate links |
| `Enter` | Follow selected or first visible link |
| `Ctrl+Arrows` | Spatial link navigation |

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
| `Ctrl+L` | Redraw |
| `?`/`F1`/`h` | Show help |
| `Q`/`Ctrl+Q` | Quit all tabs in normal mode |
| `Ctrl+C` | Quit all tabs from any mode |

The mouse wheel scrolls; left-click a link to follow it. The footer shows
**FIND** while entering a query and **SEARCH** after Enter confirms it, with
exit/navigation hints. All matches are underlined; the current match also uses
reverse video, including with color disabled. `n`/`N` move between matches;
Enter also moves forward after confirmation. Esc leaves either search phase;
`q` leaves confirmed search (and remains query text while typing). Both clear
search without closing the document. Press `q`/Esc again in normal viewing to
close the tab. In help, any key except Ctrl+C returns to the document.

## Rendering

Blad renders:

- Headings, emphasis, strikethrough, inline/fenced code
- Lists (ordered, unordered, task lists)
- Blockquotes, links, footnotes, tables
- Syntax-highlighted code blocks (via syntect)
- Leading YAML frontmatter, including Agent Skills `SKILL.md` metadata
- Math notation (Unicode output with source fallback)
- Mermaid diagrams (text-only rendering)
- Alert callouts (NOTE, TIP, IMPORTANT, WARNING, CAUTION)

### YAML frontmatter

A document starting with an unindented `---` line and closed by an unindented
`---` or `...` line displays that header as syntax-highlighted YAML:

```markdown
---
title: Example document
metadata:
  author: Someone
description: |
  Indentation, comments and literal **Markdown** stay intact.
---

# Document body
```

This works for any filename and metadata fields, not just Agent Skills.
Delimiters, comments, ordering, blank lines and multiline values are preserved,
not reformatted or sorted. Empty headers, LF/CRLF, an initial UTF-8 BOM and
trailing spaces/tabs on delimiters are supported. The header uses the code-panel
theme, stays unwrapped for horizontal scrolling and remains searchable.
`--dump` keeps the YAML text without terminal escapes.

Metadata is display-only: Blad does not validate YAML or skill schemas, expand
aliases, interpret permissions, follow metadata links or create headings from
YAML comments. Invalid YAML stays visible. The following body renders normally.
Only the first line can open frontmatter; later separators and fenced/quoted
examples retain their Markdown meaning. An unclosed header uses normal Markdown
rendering, including during live stdin paging; completing its delimiter can
revise the earlier display. `+++` metadata is not supported.

### Markdown profile

Blad uses a CommonMark parser with documented extensions and a text-only
terminal renderer. It does not claim complete CommonMark/GFM conformance.
The principal limitations are a small HTML text subset, alt-text-only images,
incomplete extended autolinking, and reduced inline-code presentation in tables.
Parsing Markdown structure and projecting it into terminal text are separate
concerns; browser-equivalent appearance is not the goal.

Tables support width-aware whole-word wrapping and delimiter alignment:
`:---` left, `:---:` center, `---:` right; unspecified alignment is left.
Long tokens or minimum column widths can require horizontal scrolling.
Table cells preserve bold, italic and strikethrough, including combinations.
Links remain navigable but lack the normal dedicated link color. Inline code
has no dedicated cell styling and uses ordinary cell whitespace/wrapping rules.
HTML tables and merged cells are not supported.

Inline/reference links, bracketed autolinks and bare HTTP(S) URLs are supported,
but not the full GFM extended-autolink grammar for bare `www` addresses, email
addresses and URL punctuation/parentheses. Footnotes have matching visual
markers, not reference-to-definition navigation. Definition lists and TOML
frontmatter are not supported.

### HTML

`<details>` / `<summary>` always show their contents expanded; there is no
folding. `<br>` adds a line break, `<hr>` a text rule, and `<p>` / `<div>` separate
blocks. `<b>` / `<strong>`, `<i>` / `<em>`, `<s>` / `<strike>` / `<del>` and
`<code>` / `<kbd>` / `<samp>` provide inline styling. Names are case-insensitive;
attributes (including `open`, styles and event handlers) have no effect.
Code labels in table cells use the same reduced styling as Markdown code.

Unsupported tags, comments, scripts and styles stay visible as literal source
by default, rather than disappearing. `--strip-html` removes unsupported markup
but retains its text contents; supported elements still render normally.
Malformed/incomplete markup remains visible. HTML never executes, fetches
resources or creates navigation from attributes. Separate Markdown bodies from
HTML wrapper tags with blank lines, following CommonMark's HTML-block rules.

Markdown images show alt text only, without downloading or displaying graphics.
Math and Mermaid have documented
subsets with visible source fallback for unsupported constructs; see the
[user guide](docs/USAGE.md).

Formal GFM extends CommonMark with tables, task lists, strikethrough, extended
autolinks and disallowed-HTML rules. Footnotes, alerts, math, Mermaid and metadata
are additional conventions or languages, not all part of that formal
specification. Superscript/subscript are also dialect extensions. See the
[CommonMark](https://spec.commonmark.org/0.31.2/) and
[GFM](https://github.github.com/gfm/) specifications.

The pending [Markdown subset backlog](TODOS.yaml) prioritizes autolink and table
gaps, further HTML text projection, then definition lists, footnote navigation
and TOML frontmatter. These are planned improvements, not current support.
The goal is a useful, predictable text-only subset, not full conformance or a
browser engine. Full HTML/CSS/JavaScript, executable MDX/JSX, template expansion,
terminal graphics, merged HTML table cells and complete TeX/Mermaid compatibility
are not goals of this backlog. Competing ecosystem dialects remain deferred.

## Architecture

Blad consists of:

- **Blad CLI** — The interactive terminal viewer with tabbed browsing
- **Blad library** — Reusable Markdown rendering and parsing (published as `kvist-blad`)

The library can be used independently for Markdown rendering in other Rust applications.

## License

AGPL-3.0-or-later; see [LICENSE](LICENSE).

See the [user guide](docs/USAGE.md) for limits, rendering subsets, diagnostics,
palette configuration and complete key precedence. Linux x86_64 is the verified
executable target; each file or live stdin source is limited to 16 MiB.