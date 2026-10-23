# Blad User's Guide

Blad is a terminal Markdown viewer for files, with rich formatting,
interactive navigation, and search. The default build has no session commands.
Build with `cargo build -p blad --features sessions` to add skott transcript
support; Kvist's all-feature build recipes include it.

## Quick Start

### View a recent session (`sessions` feature)

```bash
# List recent sessions
blad list

# View a specific session by ID or prefix
blad session 20261012T120000
```

### View a Markdown file

```bash
# View any markdown file
blad README.md

# Open several files as tabs
blad README.md GUIDE.md

# Print the installed version
blad --version
```

### Export a session (`sessions` feature)

```bash
# Export to stdout
blad export 20261012T120000

# Export to file
blad export 20261012T120000 --output transcript.md
```

## Commands and files

`list`, `session`, and `export` require the `sessions` Cargo feature.
Without it, only filenames and general options are accepted. The old `show`,
`view`, and `stream` command spellings are not aliases.

### `blad list [--limit N]`

List recent skott sessions with metadata (ID, model, message count, status).

```bash
blad list            # List 20 most recent
blad list --limit 5  # List 5 most recent
```

### `blad session <session-id>`

Print a session transcript snapshot. Accepts full session ID, ID prefix, or
full path to session directory. Active sessions include a snapshot notice;
re-run the command for newer events. This is not a live-following viewer.

```bash
blad session 20261012T120000Z-12345
blad session 20261012T120000        # Prefix match
blad session /path/to/session-dir   # Path
```

### `blad FILE...`

Open standalone files in tabs, in argument order, with the first active.
Canonical duplicates reuse one tab. All initial files must be readable before
the viewer starts or any dump output is emitted. With redirected output or
`--dump`, render each unique input once in order.

```bash
blad document.md
blad /path/to/file.md
blad README.md GUIDE.md
blad -- session  # A filename matching an enabled command
```

Session IDs are never autodetected from positional filenames.

### `blad export <session-id> [--output OUTPUT.md]`

Export a session to a combined Markdown document suitable for sharing or
archival.

```bash
blad export 20261012T120000 > transcript.md
blad export 20261012T120000 --output transcript.md
```

## Global Options

| Option | Description |
|--------|-------------|
| `--log-dir PATH` | Session-only: directory containing skott sessions (default: `.skott/runs`) |
| `--version`, `-V` | Print version without opening files or loading themes |
| `--help`, `-h` | Print available options and, when enabled, commands |
| `--dump` | Output plain text without interactive viewer |
| `--line-numbers` | Show rendered-line numbers in the interactive viewer |
| `--theme auto\|dark\|light\|terminal` | Appearance; auto uses `COLORFGBG`, otherwise dark |
| `--theme-file PATH` | Explicit JSON appearance/palette overrides, up to 64 KiB |
| `--color auto\|always\|never` | Color independently of appearance; auto honors TTY, `NO_COLOR`, `TERM=dumb` |
| `--math unicode\|source` | Mathematical projection policy |
| `--mermaid text\|source` | Diagram projection policy |
| `--diagram-charset unicode\|ascii` | Diagram structural glyphs, not authored labels |

```bash
# Render escape-free session text
blad session 20261012T120000 --dump

# Specify custom log directory
blad list --log-dir /path/to/logs
```

Dark and light appearances choose matching syntax highlighting and subtle panel
fills. Terminal-native appearance leaves code, table and quote backgrounds unchanged.
Automatic color uses `COLORTERM=truecolor`/`24bit`, then `TERM=*256color*`, then
the standard sixteen colors. A nonempty `NO_COLOR` disables automatic colors;
explicit `--color always` overrides it. Monochrome retains emphasis.
`--dump` removes **all** styling, including authored terminal escapes, and
renders message bodies consistently for files and session snapshots.
`export` instead preserves the original Markdown.

### Theme files

```bash
blad --theme-file blad/docs/theme.example.json README.md
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
stops. Home/End move vertically. Frames are buffered and only changed rows are
written; scrolling and tab changes do not clear the screen or re-enter the
alternate screen. Synchronized updates further reduce flicker on terminals
that support them. `Ctrl+L` forces a repaint if another program damages the
display. Folding and live follow remain deferred.

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
| `PageUp` / `Ctrl+B` | Scroll up one page |
| `PageDown` / `Ctrl+F` / `Space` | Scroll down one page |
| `u` / `Ctrl+U`, `d` / `Ctrl+D` | Half page up/down |
| `g` / `H` | Go to top |
| `G` / `L` | Go to bottom |
| Mouse wheel | Scroll |
| `b` | Return to the previous file |
| `Alt+Left` / `Alt+Right` | Switch open-file tabs, retaining scroll positions |
| `[` / `]` | Previous/next heading (no wrap) |
| `q` / `Esc` | Close active tab; exit if it was the last |
| `Q` / `Ctrl+C` | Quit all tabs |
| `h` | Keyboard help; any key returns |
| `Ctrl+L` | Force repaint |
| `r` | Reload the active file, retaining viewport position |

Closing a tab selects the next surviving tab, or the previous one if closing
the rightmost tab. Surviving tabs keep their order and scroll positions;
back history no longer refers to closed tabs. Esc cancels search, link mode
or help without closing a tab. Search cancellation applies while the prompt is
open: after Enter commits the search, Esc is again the normal close-tab key.
To clear committed highlights without closing, press `/` then Esc.
Ctrl+C remains a global quit, including from help; other help keys return to
the document. These precedence choices are recorded for review in `IMPL.md`.

### Link Navigation

The top row lists open files in fixed opening order; brackets mark the active
tab without moving it. Narrow terminals clip the row rather than reordering
tabs. Opening an existing file reuses its tab. Search/link status and
navigation errors appear in the footer.

Use `Tab` to select the next link, `Shift+Tab` for the previous link, without
entering a special mode. Only the selected link label is emphasized, including
its wrapped spans; adjacent links and surrounding prose are not highlighted.
`Ctrl+Left`/`Right` selects a link to the left/right on the same rendered row;
`Ctrl+Up`/`Down` selects the nearest row in that direction, then nearest
column. There is no spatial wrap. Navigation reveals offscreen links both
vertically and horizontally; resize recomputes their positions.
Without a selection, the origin is the viewport's top-left.
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

Browser-launch output is captured rather than written over the viewer.
Success warnings or launch errors remain in the footer across redraws and
resize until another user action replaces them. Captured diagnostics retain
at most 4,096 bytes and identify truncation; control characters stay inert.
The launcher has a five-second deadline after spawn. On timeout Blad stops
and reaps the directly owned launcher, keeps the document open, and reports
the timeout with captured diagnostics. A browser that already started may
still open: Blad does not terminate independent browsers. Launch remains
synchronous, so input waits during those five seconds; process creation,
kernel scheduling and termination calls are not hard real-time bounded.

You do not need link mode to follow a link: pressing `Enter` in normal mode
opens the first link at or below the top of the viewport. Bare URLs
(`https://…`) are treated as links in addition to bracketed and named links.
Left-click link labels to follow them directly, including wrapped labels,
table cells and bare URLs. Repeated bare URLs are independent occurrences.

Press `Esc` to exit link mode without following.

### Search

Press `/` for forward or `?` for backward search. Type a query, then press `Enter`
to exit search mode and keep the matches highlighted.

| Key | Action |
|-----|--------|
| `/` / `?` | Start forward/backward literal search |
| Type text | Search query |
| `Ctrl+R` | Toggle literal/regex in this prompt |
| `Ctrl+U` | Clear the prompt |
| `Enter` | Exit search, keep highlights |
| `n` | Next occurrence in the search direction, wrapping |
| `N` / `p` | Occurrence in the reverse direction, wrapping |
| `Esc` | Cancel search |

The footer shows mode, direction and occurrence position (e.g., "3/12 matches"),
or an explicit no-match/invalid-pattern diagnostic, even after Enter. Multiple
matches on one row count separately. Only matching text is highlighted, across
heading, emphasis and syntax resets. Reverse video and underline retain a
non-color cue, including inside inline code and reversed table headers.
Search starts near the current viewport and reveals horizontally hidden
matches. Tab or spatial link selection clears search emphasis.

Each new prompt starts in **case-insensitive literal mode**, so `a.b` searches
for that exact text. Toggle `Ctrl+R` to use Rust regex syntax in the same prompt:
`hit[0-9]+`, `^heading`, `foo|bar`, Unicode classes and inline flags such as
`(?-i:ExactCase)`. Lookaround and backreferences are not supported. Searches
operate on rendered lines, not raw Markdown or across soft wraps. Zero-width
matches are ignored; a match within a grapheme emphasizes the entire grapheme
instead of splitting it. Input is limited to 4,096 UTF-8 bytes; compiled
regex and DFA caches are limited to 2 MiB each.

### Relationship to `less` and future priorities

Blad shares line/page/half-page movement, `/`/`?`, `n`/`N`, `g`/`G`,
`Ctrl+L` and `h` with familiar pagers. It deliberately keeps `b` for tab
history and `f` for optional link mode; use `Ctrl+B`/`Ctrl+F` for full pages.
Tab/Shift+Tab retain conventional link traversal rather than becoming heading
keys; use `[`/`]` for headings. Unlike `less`, search starts literally and
regex is explicit, preventing punctuation from unexpectedly becoming syntax.
Normal `q`/Esc closes one tab rather than always quitting.

Useful next increments are a heading outline, numeric movement and line/percent
jumps, marks/jump history within a document,
per-tab retained searches and visible tab overflow. Real live follow needs
explicit file-replacement/truncation semantics. Giant-file responsiveness needs
measured search/reflow budgets before claiming bounded latency. Shell escapes
and document-triggered external commands are intentionally not pager goals.

### Reloading changed files

Press `r` in normal mode to reread the active file from its opening canonical
path. A successful reload preserves render/theme options, tab history and
numeric vertical/horizontal positions, clamped to the new document.
Search query, direction and literal/regex mode persist; results and link
positions are rebuilt, and obsolete link selection is cleared. It does not
try to recover the same source paragraph after arbitrary edits.

Read/UTF-8/render failures leave the previous document usable and show a footer
error. A shorter replacement erases stale rows and clamps scroll offsets.
Reload opens nonblockingly, refuses final symlinks and validates the opened
descriptor before reading; FIFO/symlink replacement therefore fails safely.
This is not ancestor-path isolation or a snapshot of concurrent writes. Embedded
documents without a file path report that reload is unavailable.
Reload is explicit, not automatic live following. While entering a search,
`r` remains ordinary query text.

The PTY regression model now accounts for display-width continuation cells,
combining text and joined emoji, including across SGR transitions. This
strengthens Unicode positioning evidence but does not certify every terminal,
Unicode version, ambiguous-width setting or font.

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
blad document.md
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
blad session 20261012T140000

# Save a plain snapshot for inspection or sharing
blad --dump session 20261012T140000 > snapshot.txt
```

### View and edit documentation

```bash
# Open README
blad README.md

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
