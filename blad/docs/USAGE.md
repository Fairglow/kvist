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
| `--theme-file PATH` | Explicit JSON appearance/palette overrides, up to 64 KiB |
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
fills. Terminal-native appearance leaves code, table and quote backgrounds unchanged.
Automatic color uses `COLORTERM=truecolor`/`24bit`, then `TERM=*256color*`, then
the standard sixteen colors. A nonempty `NO_COLOR` disables automatic colors;
explicit `--color always` overrides it. Monochrome retains emphasis.
`--dump` removes **all** styling, including authored terminal escapes, and
renders message bodies consistently for files, sessions and stream snapshots.
`export` instead preserves the original Markdown.

### Theme files

```bash
blad --theme-file blad/docs/theme.example.json view README.md
```

Files are strict JSON objects with optional `theme` (`dark`, `light`,
`terminal`) and a partial `palette`. Unspecified roles inherit the selected
appearance. Explicit non-auto `--theme` overrides the file's appearance, while
its palette overrides remain in force. Color policy, `NO_COLOR` and `--dump`
still apply independently. There is no implicit file discovery.

Palette roles: `heading`, `link`, `muted`, `list_marker`, `math`, `chrome`,
`search`, `alerts` (five colors in NOTE/TIP/IMPORTANT/WARNING/CAUTION order),
`code_background`, `table_background`, `quote_background`. A color is an ANSI
index 0–255 or `"#RRGGBB"`. Backgrounds also accept `null` for no fixed fill.
Unknown keys, invalid values/JSON, files above 65,536 bytes and read errors
are reported as failures, never silently ignored.

Library callers use `MarkdownRenderer::with_options(RenderOptions { .. })`.
The options contain a host-provided semantic `Palette`, `ColorDepth`, appearance,
and projection choices. Library defaults never inspect environment variables,
read stdin, issue OSC queries, or launch subprocesses. Existing width, syntax
theme and panel-background setters remain available; unknown explicit syntax
themes return `InvalidInput`. Syntax definitions and built-in themes are reused
across renders.

## Text Rendering

### Unicode script notation

Blad recognizes both standalone `^2^`/`~2~` and adjacent `E=mc^2^`/`H~2~O`,
rendering `²`, `₂`, `E=mc²` and `H₂O`. This is a Blad Markdown dialect extension.
Superscripts map digits, `+ - = ( )`, `i` and `n`. Subscripts map digits,
`+ - = ( )` and `a e h i j k l m n o p r s t u v x`. Any unmapped character
causes a whole-span fallback: `x^2q^` becomes `x^(2q)`, never a partially
converted expression.

Spans stay within one literal parser text run: they do not cross whitespace,
formatting, code, HTML or math boundaries. Surrounding emphasis stays intact;
`~~strikethrough~~` retains its meaning. Backslash-escaped delimiters, encoded
delimiters and code remain literal. Unmatched delimiters are unchanged.
Touching spans such as `^2^^3^` produce `²³`; empty `^^` stays literal.
An entity or escaped token inside a prospective span also breaks its literal
run, leaving that notation unprojected.

### Limited mathematical text

Parser-recognized `$...$` and `$$...$$` project a deliberately small TeX-like
grammar into terminal text. Inline formulas stay atomic; display formulas are
separate, unwrapped text blocks. Use horizontal scrolling for overflow.
`--math source` preserves original expression text and delimiters.

| Source | Terminal text |
|--------|---------------|
| `$E=mc^2$` | `E=mc²` |
| `$\alpha_i^{n+1}$` | `αᵢⁿ⁺¹` |
| `$\frac{a+b}{c}$` | `(a+b)/(c)` |
| `$\sqrt{x+1}$` | `√(x+1)` |
| `$\frac{a}{b}^2$` | `((a)/(b))²` |
| `$x/\frac{a}{b}$` | `x/((a)/(b))` |
| `$x^{a+b}$` | `x^(a+b)` |

Groups retain binding. Scripts accept a single atom or braced expression, with
at most one of each position per atom. `\text{...}` accepts literal text,
balanced literal braces and escaped punctuation, not nested commands.
`\left`/`\right` accept matching `()`, `[]`, `\{\}`, `||`,
`\langle`/`\rangle`, and `\lVert`/`\rVert` (also `\Vert` or `\|`).
Delimiter size is not graphical. Escaped `\{ \} \$ \% \_ \# \&` are literal.
Spacing `\, \; \: \quad \qquad \space` produces a space; `\!` is omitted.

The complete admitted command vocabulary, beyond structural commands above:

| Category | Commands (each prefixed with `\`) |
|----------|-----------------------------------|
| Lower-case Greek | `alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega` |
| Greek variants | `varepsilon vartheta varpi varrho varsigma varphi` |
| Upper-case Greek | `Gamma Delta Theta Lambda Xi Pi Sigma Upsilon Phi Psi Omega` |
| Arithmetic/comparison | `times cdot div pm mp le leq ge geq ne neq approx equiv sim propto infty` |
| Sets/logic | `in notin subset subseteq supset supseteq cup cap emptyset forall exists neg land lor` |
| Arrows | `to rightarrow leftarrow leftrightarrow Rightarrow Leftarrow Leftrightarrow mapsto` |
| Calculus/other | `sum prod int oint partial nabla degree ldots cdots` |
| Named functions | `sin cos tan log ln lim exp max min det gcd` |

Conventional TeX variants remain distinct: `\epsilon` → `ϵ`, `\varepsilon` →
`ε`, `\phi` → `ϕ`, and `\varphi` → `φ`. This is not full TeX: matrices,
environments, macros, includes, optional root indices and unknown commands
retain the **whole original expression** with an `unsupported` explanation.
Recognized but malformed expressions receive a distinct `invalid` explanation.
Syntax the Markdown parser does not recognize as math stays literal.
No partial projection, execution, external resources or graphical output occurs.

Inline signed numeric amounts such as `$12.50$` stay literal. Ambiguous
numeric-leading dollar spans immediately followed by another digit also stay
literal when they contain no TeX/script/group syntax, preserving `$5 and $10`.
Explicit display `$$5$$` is math. Escaped or unmatched dollars and code stay
literal.

Limits are inclusive: 16,384 input bytes per formula, 32 structural nesting
levels and 65,536 projected UTF-8 bytes. A resource failure has a distinct
diagnostic; source fallback above 16,384 bytes explicitly reports omitted bytes
at a valid UTF-8 boundary. Other fallback retains complete original delimiters
and source; tables still apply their normal whitespace normalization. Terminal
controls are always shown inertly. Source mode skips projection.
CRLF line endings normalize to LF; standalone carriage returns are visible
`\u{d}` text rather than terminal cursor movement.

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
| `PageDown` / `d` / `Space` | Scroll down one page |
| `g` / `H` | Go to top |
| `G` / `L` | Go to bottom |
| Mouse wheel | Scroll |
| `b` | Return to the previous file |
| `Alt+Left` / `Alt+Right` | Switch open-file tabs, retaining scroll positions |
| `q` / `Esc` | Quit viewer |

### Link Navigation

The top row lists open files; brackets mark the active tab, shown first so it
stays visible in narrow terminals. Opening an existing file reuses its tab.
Search/link status and navigation errors appear in the footer.

Use `Tab` to select the next link, `Shift+Tab` for the previous link, without
entering a special mode. The selected link's rows are highlighted.
Optional `f` link mode remains available.
Press `Enter` to follow the link:

- **File links** (relative or absolute paths) open the linked file in Blad.
  A relative path is resolved against the directory of the file you are
  viewing. Press `b` to return to the previous file.
- **URL links** (`http://`, `https://`, `mailto:`, `tel:`) open in your
  default browser via `xdg-open`.
- **Anchor links** (`#heading`) jump to local headings; `other.md#heading`
  opens the file and jumps there. Heading slugs are lowercase, replace spaces
  with hyphens, preserve Unicode/hyphens/underscores, and omit punctuation.
  Duplicate slugs gain `-1`, `-2`, etc. Explicit `{#id}` headings also work.
  Percent-encoded UTF-8 paths/fragments are decoded; missing headings and
  invalid escapes produce visible diagnostics without discarding the document.

You do not need link mode to follow a link: pressing `Enter` in normal mode
opens the first link at or below the top of the viewport. Bare URLs
(`https://…`) are treated as links in addition to bracketed and named links.
Left-click inline/named links to follow them directly, including wrapped labels.
Pointer hit testing currently covers prose/heading labels; table links and bare
URLs retain keyboard following rather than recorded mouse hit-test spans.

Press `Esc` to exit link mode without following.

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
Matching rows keep their configured search indication across heading, emphasis
and syntax-highlighting resets, including monochrome reverse-video cues.

### Edit Mode

In the viewer for a file (not a session), press `e` to open the file in
your configured editor. After saving and closing the editor, Blad
re-renders the file with your changes.
Appearance, color capability and projection options persist across editing
and resize. Re-entry releases the previous viewport instead of accumulating
recursive viewers. Editor launch, exit-status and reread failures propagate,
with terminal state restored.
This is a whole-file handoff, not an embedded or per-section editor.

Blockquotes retain a left edge on every row, including paragraph separators,
code and tables; dark/light themes add a subtle full-row fill. Alerts use their
own colored edges and labels. Numeric footnote references and definitions share
superscript tags, with `[name]` fallback for nonnumeric tags. Ordered lists resume
the parent's numbering after nested lists. `***` is a heavy rule; `---`/`___`
are thin rules, and `~~~` remains a CommonMark code fence.

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
- Footnotes (references are superscripted and the definition is prefixed with
  the same marker)
- Superscript and subscript
- Math expressions (inline `$...$` and display `$$...$$`)
- Horizontal rules (`***` renders as a stronger double line than `---`)

### Rendering appearance

- **Code blocks** are framed with a full-width panel line above and below so
  the block reads as a closed rectangle, and are syntax-highlighted (the
  highlighting theme is chosen to stay legible at the terminal's color depth).
- **Blockquotes** carry a left-edge bar so they stand out from surrounding
  prose; GFM alerts use the same bar tinted with the alert's color.
- **Lists** are enumerated for ordered lists, and unordered lists alternate
  the bullet glyph by nesting level.

### Mermaid Diagrams

Blad renders ` ```mermaid ` fences directly into terminal text using pinned
Merman 0.9.1. No PNG/SVG, graphical intermediates, browsers, runtime tools or
network calls are involved. `--diagram-charset ascii` changes structural glyphs,
not authored Unicode. `--mermaid source` skips the engine and shows source.

| Family/header | Admitted examples | Boundaries/approximations |
|---------------|-------------------|--------------------------|
| `flowchart` / `graph` | Labelled boxes/decisions, arrows/edge labels, branches/cycles, subgraphs, multiline labels; LR/RL/TB/TD/BT | Shapes are terminal approximations; endpoint-product `&` shorthand, callbacks, icons/images and animation are unsupported |
| `sequenceDiagram` | Ordered participants, messages/replies, nested loop/alt/else frames, `Note over` | Actors use participant boxes; actor properties, links and extended actor types are unsupported |
| `stateDiagram` / `stateDiagram-v2` | Transitions, root cycles, simple composites, attached notes; LR/RL/TB/BT | Some composite crossings/cycles exceed the admitted routing profile |
| `classDiagram` | Members/methods/visibility, interface annotations, inheritance, labelled directed relations, namespaces, attached notes; LR/RL/TB/BT | Namespace-plus-external-note combinations and some relation topologies fall back |
| `erDiagram` | Entities, attributes, key tokens/comments, labelled cardinality relations; LR/RL/TB/BT | Cardinalities are textual; some relation topologies fall back |

ER cardinalities deliberately use textual Mermaid endpoint tags, not graphical
crow's-foot glyphs: `||` means exactly one, `o|` zero or one, `|{` one or more,
and `o{` zero or more. Thus `SESSION ||--o{ MESSAGE : contains` retains both
endpoint tags around the routed connector. The literal `o{` is intentional,
not an unparsed fragment. Attribute rows currently use the backend's verbose
text representation rather than graphical ER notation.

This is an admitted **subset**, not full Mermaid compatibility. Styles are not
represented in plain diagrams. Rich HTML other than line breaks, configuration
directives/front matter, accessibility metadata and browser interactions are
unsupported. Conservative source-level gates can also reject such syntax in
comments/literals. A non-spatial `relations:` summary is rejected rather than
presented as a successfully rendered diagram.

Direction, endpoint identity and labels are retained. Labels can wrap **inside**
their boxes; completed diagram rows never viewport-wrap, rotate or truncate to
fit the terminal. Canonical geometry is unchanged by resize: use Left/Right for
overflow. Unsupported syntax, invalid syntax and resource limits each show
distinct explanations plus source, with explicit byte omissions if source
exceeds the 65,536-byte fallback ceiling.

Rejection ceilings are 65,536 source bytes, 8,192 lexical units, 256
nodes/participants/containers, 512 edges/messages, nesting 32, 32,768 model
items, 262,144 model-text bytes, 262,144 grid/document cells, 2,097,152 output
bytes, 256 bytes/grapheme and 2,000,000 layout-work units. A 500 ms cooperative
deadline supplements these bounds; it is not hard preemption or a memory
sandbox. Smaller scenes can still be unsupported or exceed routing budgets.
Model size checks run after parsing; these are not pre-allocation memory limits.
Terminal protocol controls and bidirectional override/isolate controls are made
visible in diagram output, diagnostics and source presentations.

Library hosts may call `render_markdown_with_cancellation` with a cloneable
`RenderCancellation`, optionally configured with a shorter deadline. A retained
clone can request cancellation; cancellation/deadlines return `Interrupted`,
never a successful source fallback. Writer failures retain their original
error. Output can already have been written when either error occurs. Rendering
itself creates no worker thread.

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

Read the visible diagnostic: unsupported syntax, invalid syntax and resource
limits have different causes. Simplify the scene or use `--mermaid source`.
There is no external Mermaid executable to install.

### No color output

Automatic color is disabled for redirected stdout, nonempty `NO_COLOR` and
`TERM=dumb`. Use `--color always` for intentional colored redirection, or
`--color never` for monochrome emphasis. `--dump` always overrides color and
removes styling. `--theme` changes appearance, not color capability.

Exports remain raw source Markdown, not sanitized rendered terminal text.
Treat authored terminal controls in exported files as untrusted source.
