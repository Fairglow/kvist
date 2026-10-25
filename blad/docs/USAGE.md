# Blad User's Guide

Blad is a terminal Markdown viewer for files, with rich formatting,
interactive navigation, search and external-editor handoff. It runs locally
without project configuration, credentials or a service. Linux x86_64 is the
verified executable target.

## Installation

The registry package is **`kvist-blad`**, while the executable and Rust library
are **`blad`**. Version **0.3.0 is not yet published**. To install the current
checkout, run from the repository root:

```bash
cargo install --locked --path blad
```

Inside the component directory, use `cargo install --locked --path .`.
Rust **1.95 or newer** is required; the MSRV is a minimum, not a toolchain pin.
Once the planned release is published:

```bash
cargo install --locked kvist-blad --version 0.3.0
```

## Quick start

```bash
blad README.md
blad README.md GUIDE.md
blad --dump README.md > readme.txt
cat README.md | blad
blad README.md | head -n 30
cat generated.md | blad README.md - GUIDE.md
blad --version
blad --help
```

## Files: `blad [OPTIONS] FILE...`

Open standalone files in tabs, in argument order, with the first active.
Canonical duplicates reuse one tab. All initial files must be readable before
the viewer starts or any dump output is emitted. With redirected output or
`--dump`, render each unique input once in order. Filename extensions do not
matter. Empty invocation with terminal stdin displays usage.

```bash
blad document.md
blad /path/to/file.md
blad README.md GUIDE.md
blad -- -notes.md
```

With no files, redirected stdin supplies a UTF-8 Markdown document. Explicit
`-` reads stdin among named files in argument order; repeated `-` reuses it.
Use `./-` to open a literal file named `-`. Named files otherwise leave stdin
unread. Explicit `-` with terminal stdin fails rather than waiting for input.
Empty redirected input succeeds. Input must end (EOF) before rendering: this
is a finite snapshot, not streaming or an interactive stdin pager.

If stdin or stdout is not a terminal, inputs render to stdout without the
viewer or automatic reload. Output is escape-free by default; `--color always`
explicitly opts into styles for a pipe. `--dump` always suppresses all styling.
Closing the downstream pipe (for example, `head -n 30`) exits quietly with
success. Other read/write failures remain errors. Stdin shares the inclusive
16 MiB input ceiling and single overflow probe; invalid UTF-8 or oversized
input fails before output from any initial source.

### File input limits

Interactive file tabs reload saves automatically. Blad polls the active path
every 250 ms and waits for metadata to remain stable for at least another
250 ms, including rename-based editor saves. Inactive tabs refresh when
revisited. Scroll offsets, search and rendering settings are retained;
offsets clamp if the document shrinks. Read/render failure keeps the previous
document and shows a footer error; another save or `Ctrl+R`/`F5` retries.
No watch flag, worker or service is required. This is not tail-following or
a guarantee of source-anchored position after arbitrary edits. Session snapshots,
pathless documents and noninteractive output do not watch.

Standalone files must be regular UTF-8 files of at most **16 MiB (16,777,216
bytes) each**, inclusively. The same fixed ceiling applies to initial files,
newly followed file links, reload and editor rereads. There is no silent
truncation or size-limit override: split oversized documents into smaller files.
Reported file size is checked before decoding/rendering, and actual reading
stops after at most the limit plus one probe byte if metadata understates content.

Initial validation fails before terminal entry or any dump output. Failed
links/reloads leave the previous document usable without adding tabs/history.
Post-editor reread errors retain the existing failure policy: report an error
and exit after restoring the terminal.

Files open nonblockingly and the opened descriptor must be regular before
reading, so ordinary FIFOs are rejected without waiting for a writer. Explicit
initial file/link/theme paths still resolve authored symlinks normally.
Reload/editor rereads refuse a final symlink substituted at the opening
canonical path. Ancestor traversal, device-open effects and concurrent writes
are not isolated; file contents are not snapshots.

These are per-file source limits, not bounds on total tab memory, rendered
expansion, search/rendering time or kernel/FUSE I/O latency. This is not a
streaming pager. Strings supplied directly to the rendering library are outside
these file-read limits.

## Global Options

| Option | Description |
|--------|-------------|
| `--version`, `-V` | Print version without opening files or loading themes |
| `--help`, `-h` | Print usage and available options |
| `--dump` | Output plain text without interactive viewer |
| `--line-numbers` | Show rendered-line numbers in the interactive viewer |
| `--theme auto\|dark\|light\|terminal` | Appearance; default auto uses file appearance, then `COLORFGBG`, otherwise dark |
| `--theme-file PATH` | Explicit JSON appearance/palette overrides, up to 64 KiB |
| `--color auto\|always\|never` | Color independently of appearance; auto honors TTY, `NO_COLOR`, `TERM=dumb` |
| `--math unicode\|source` | Mathematical projection policy; default `unicode` |
| `--mermaid text\|source` | Diagram projection policy; default `text` |
| `--diagram-charset unicode\|ascii` | Structural glyphs, not authored labels; default `unicode` |

```bash
blad --theme light --line-numbers document.md
blad --color always --dump document.md > document.txt
```

Dark and light appearances choose matching syntax highlighting and subtle panel
fills. Terminal-native defaults leave code, table and quote backgrounds unchanged;
explicit theme-file background overrides can still add fills.
Automatic color uses `COLORTERM=truecolor`/`24bit`, then `TERM=*256color*`, then
the standard sixteen colors. A nonempty `NO_COLOR` disables automatic colors;
explicit `--color always` overrides it. Monochrome retains emphasis.
`--dump` removes **all renderer styling**. Authored terminal controls remain
visible and inert rather than becoming executable escape sequences.

Appearance and color policy resolve independently:

1. An explicit `--theme dark`, `light` or `terminal` selects appearance.
   With `--theme auto` (the default), the theme file's `theme` wins if present;
   otherwise `COLORFGBG`'s last semicolon-separated value of `7` or `15` selects
   light; any other/missing hint selects dark. No terminal query is sent.
2. Missing palette roles inherit that appearance; explicit file roles override
   it, even when `--theme` overrides the file's appearance.
3. `--color never` removes added foreground/background colors, not all SGR
   emphasis. `--color auto` (default) does the same when stdout is not a terminal,
   `TERM=dumb`, or `NO_COLOR` is nonempty. `--color always` bypasses those
   automatic disabling conditions.
4. When colors are enabled, `COLORTERM` (case-insensitive `truecolor` or `24bit`)
   selects truecolor, otherwise a `TERM` containing `256color` selects ANSI256,
   otherwise ANSI16. The CLI has no explicit depth selector.
5. Noninteractive output is escape-free unless `--color always` is explicit.
   `--dump` overrides every color/style choice with escape-free rendered text.

### Theme files

```bash
blad --theme-file blad/docs/theme.example.json README.md
```

Files are strict JSON objects with optional `theme` (`dark`, `light`,
`terminal`) and a partial `palette`. Unspecified roles inherit the selected
appearance. Explicit non-auto `--theme` overrides the file's appearance, while
its palette overrides remain in force. Color policy, `NO_COLOR` and `--dump`
still apply independently. There is no implicit file discovery.

| Palette key | Semantic role |
|-------------|---------------|
| `heading` | Heading text |
| `link` | Link labels and bare URLs |
| `muted` | Secondary text and rules |
| `list_marker` | List and task markers |
| `math` | Mathematical notation |
| `chrome` | Viewer header/footer text |
| `search` | Viewer match/selection indication |
| `alerts` | Exactly five colors, in NOTE, TIP, IMPORTANT, WARNING, CAUTION order |
| `code_background` | Code-panel fill |
| `table_background` | Alternating table-row fill |
| `quote_background` | Blockquote fill |

A color is an integer ANSI index **0–255** or an RGB string **`"#RRGGBB"`**.
Only the three background roles also accept `null`, meaning no fixed fill.
Unknown keys, invalid values/JSON, files above 65,536 bytes and read errors
are reported as failures, never silently ignored.
Theme inputs use the same nonblocking regular-descriptor checks as document
files, after resolving explicitly selected symlinks. Their separate 64 KiB
limit is inclusive and is also enforced against actual bytes read.

The [complete example file](theme.example.json) demonstrates every role.
The file controls appearance only, not commands, color depth or projection.

### Library policy

`MarkdownRenderer::with_options(RenderOptions { .. })` accepts typed settings,
including a host-provided `Palette`. `ColorDepth::None`, `Ansi16`, `Ansi256` and
`TrueColor` are all explicitly available to library hosts. Unlike the CLI,
`RenderOptions::default()` is deterministic: dark appearance, truecolor,
`plain: false`, Unicode math, text Mermaid and Unicode diagram structure.
The default renderer has no width limit. `RenderOptions::plain()` suppresses
all styling; choosing only `ColorDepth::None` can retain emphasis.

The renderer never calls `is_terminal()`, inspects environment variables, reads
stdin, issues terminal queries or launches subprocesses. This does not describe
the separate interactive helpers in `blad::display`.
`set_width`, `set_options`, `set_syntax_theme` and `set_background` configure
the renderer; an unknown explicit syntax theme returns `InvalidInput` during
rendering. Syntax definitions and built-in themes are reused across renders.
See the [basic library example](../README.md#embed-the-renderer).

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
Spacing `\, \; \: \quad \qquad \space` and an escaped space produce a space;
`\!` is omitted.

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
stops. Home goes to the top and resets the horizontal offset; End goes to the
bottom without resetting it. Frames are buffered and only changed rows are
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
| `Home` / `g` / `H` | Top of document; reset horizontal offset |
| `End` / `G` / `L` | Bottom of document; retain horizontal offset |
| `Left` / `Right` | Scroll horizontally one column |
| Mouse wheel | Scroll one line per wheel event |
| `b` | Return to the previous file |
| `Alt+Left` / `Alt+Right` | Switch open-file tabs, retaining scroll positions |
| `[` / `]` | Previous/next heading (no wrap) |
| `q` / `Esc` | Close active tab; exit if it was the last |
| `Q` / `Ctrl+C` | Quit all tabs |
| `h` | Keyboard help; any key except global Ctrl+C returns |
| `Ctrl+L` | Force repaint |
| `r` | Reload the active file, retaining viewport position |

Closing a tab selects the next surviving tab, or the previous one if closing
the rightmost tab. Surviving tabs keep their order and scroll positions;
back history no longer refers to closed tabs. Tab switching wraps at the first
and last tab; `b` pops previous-tab history (it is not a previous-heading or
within-document jump). Vertical/horizontal offsets are per-tab; searches and
link selections are not retained across switching, back navigation or editor
re-entry.

### Keyboard precedence

The following order describes actual dispatch, not just the normal-mode key
table. Key-release events are ignored.

1. **Ctrl+C always quits all tabs**, including from help, search and link mode.
2. **Help consumes any other key to dismiss itself**, without performing that
   key's normal action. In particular `Q`, `q`, Esc, arrows and Ctrl+L only
   dismiss help; it is not a scrollable modal pager.
3. **Alt+Left/Right switches tabs only outside an open search prompt or `f`
   link mode.** A normal selected link or committed search does not block it.
4. **`Q` quits all tabs outside the search prompt**, including from `f` mode.
   In the search prompt it is literal query text.
5. **An open search prompt consumes keys.** Printable characters without
   Ctrl/Alt append; Backspace deletes; Ctrl+R toggles regex; Ctrl+U clears;
   Enter keeps results and returns to normal mode; Esc clears the query/results
   and returns to normal mode. Other keys are ignored (apart from global Ctrl+C).
   `q`, `h`, `r`, `n`, `p`, `?` and `/` are query text there.
6. Outside search, **Ctrl+Arrows selects spatial links**, even in `f` mode.
7. **`f` link mode consumes non-Ctrl keys**: Tab/Shift+Tab cycle, Enter follows
   and leaves the mode, Esc leaves it and clears selection. Other non-Ctrl keys,
   including `q`, `h`, arrows, `r`, `b` and Alt+Arrows, do nothing (except `Q`
   handled above). Ctrl+B/F, Ctrl+U/D and Ctrl+L still scroll/repaint in this mode.
8. Remaining Ctrl keys use the page/half-page/repaint bindings; remaining keys
   use normal-mode bindings.

Thus normal-mode `q`/Esc closes a tab even with a directly selected link or
committed search. **After Enter commits a search, Esc closes the active tab**;
to clear committed highlights without closing, press `/` then Esc.

Mouse wheel and left-click link handling are independent of search/link
keyboard modes; they remain active in both. Help ignores mouse input. Resize
reflows the underlying document and help layout without dismissing a mode.

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
For spatial navigation without a selection, the origin is the viewport's top-left;
the selected link itself is excluded from spatial candidates.
Without a selection, Tab starts at the first link whose first rendered span is
at/below the viewport top (or wraps to the first); Shift+Tab starts with the last
at/above it (or the last link). Subsequent Tab/Shift+Tab steps in source order
and wraps. Optional `f` link mode starts at the document's first link, using
Tab/Shift+Tab to cycle, Enter to follow and leave the mode, and Esc to leave
without following and clear selection. The other key exceptions are listed
under [keyboard precedence](#keyboard-precedence).
Press `Enter` to follow the link:

- **File links** (relative or absolute paths) open the linked file in Blad.
  A relative path is resolved against the directory of the file you are
  viewing. Press `b` to return to the previous file.
- **URL links** (URI schemes followed by `://`, plus `mailto:` and `tel:`)
  are handed to `xdg-open` only when explicitly followed.

- **Anchor links** (`#heading`) jump to local headings; `other.md#heading`
  opens the file and jumps there. Heading slugs are lowercase, replace spaces
  with hyphens, preserve Unicode/hyphens/underscores, and omit punctuation.
  Duplicate slugs gain `-1`, `-2`, etc. Explicit `{#id}` headings also work.
  Percent-encoded UTF-8 paths/fragments are decoded; missing headings and
  invalid escapes produce visible diagnostics without discarding the document.

Browser launches run `xdg-open` directly without a shell, with null stdin and
captured stdout/stderr, never inheriting the viewer's streams.
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
follows the selected link, or the first link at/below the top of the viewport
if nothing is selected. That fallback does not wrap when no link remains.
Bare URLs (`https://…`) are treated as links in addition to bracketed and named links.
Left-click link labels to follow them directly, including wrapped labels,
table cells and bare URLs. Repeated bare URLs are independent occurrences.

Press `Esc` to exit link mode without following.

### Search

Press `/` for forward or `?` for backward search. Type a query, then press `Enter`
to exit search mode and keep the matches highlighted.

| Key | Action |
|-----|--------|
| `/` / `?` | Start forward/backward literal search |
| Type text / `Backspace` | Append to / delete from query |
| `Ctrl+R` | Toggle literal/regex in this prompt |
| `Ctrl+U` | Clear the prompt |
| `Enter` | Exit search, keep highlights |
| `n` | Next occurrence in the search direction, wrapping |
| `N` / `p` | Occurrence in the reverse direction, wrapping |
| `Esc` | Cancel only an open prompt; after Enter this closes the tab |

The footer shows mode, direction and occurrence position (e.g., "3/12 matches"),
or an explicit no-match/invalid-pattern diagnostic, even after Enter. Multiple
matches on one row count separately. Only matching text is highlighted, across
heading, emphasis and syntax resets. Reverse video and underline retain a
non-color cue, including inside inline code and reversed table headers.
Forward search starts at the first occurrence at/below the viewport top,
wrapping to the first if necessary. Backward search starts at the last
occurrence at/above that row, wrapping to the last if necessary; it does not
start from the viewport bottom or current horizontal position. Subsequent
`n`/`N`/`p` movement is occurrence-based and reveals horizontal matches.
For a backward `?` search, `n` moves backward and `N`/`p` move forward.
Tab, spatial link selection, `f`, and heading-jump keys clear search emphasis.

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

Folding, live follow, a heading outline, numeric movement/line/percent jumps,
marks, within-document jump history and per-tab retained searches are not
implemented. Giant-file responsiveness is not guaranteed by the byte limits.
There are no shell escapes or document-triggered external commands.

### Reloading changed files

Press `r` in normal mode to reread the active file from its opening canonical
path. A successful reload preserves render/theme options, tab history and
numeric vertical/horizontal positions, clamped to the new document.
Search query, direction and literal/regex mode persist; results and link
positions are rebuilt, and obsolete link selection is cleared. It does not
try to recover the same source paragraph after arbitrary edits.

Read/UTF-8/render failures leave the previous document usable and show a footer
error, including oversized replacements above the 16 MiB source limit.
A shorter replacement erases stale rows and clamps scroll offsets.
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

In the viewer for a file, press `e` to open the file in
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

Blad uses `$EDITOR`, defaulting to `nvim` only when the variable is unset or
cannot be read as Unicode. It splits the value on whitespace into a program
and arguments, appends the file path, and executes directly: there is no shell,
quoting interpretation, variable expansion or glob expansion. Use a wrapper
executable for complex commands; an empty value is an error. The editor receives
the restored terminal and Blad waits for it to exit. Launch failure, nonzero exit
and failed UTF-8/size/regular-file rereads propagate as errors after terminal
restoration rather than resuming with a falsely successful edit.
An embedded document without a file path cannot be edited (`e` has no action).

```bash
# Set your preferred editor
export EDITOR="code --wait"
blad document.md
# Press 'e' in the viewer to edit
```

## Markdown Features

Blad uses CommonMark parsing with selected extensions and a terminal-specific
projection, **not full CommonMark/GFM conformance**:

- **Headers** (H1-H6) with color coding
- **Bold**, *italic*, ~~strikethrough~~ text
- Inline/reference links, autolinks and bare HTTP(S) URLs, followed by viewer actions
- Images (shown as alt text in terminal)
- Code blocks with syntax highlighting (50+ languages)
- Inline code with inverted video
- Tables with width-aware whole-word wrapping, but no delimiter left/center/right alignment
- Blockquotes, including GFM alert styles:
  - [!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION]
  - Labels/edges preserve meaning without color; colors follow the palette
- Task lists with distinct `[x]` / `[ ]` markers in styled and plain output
- Footnotes (references are superscripted and the definition is prefixed with
  the same marker)
- Superscript and subscript
- Math expressions (inline `$...$` and display `$$...$$`)
- Horizontal rules (`***` renders as a stronger double line than `---`)

Raw HTML is omitted rather than rendered or executed; inline `<br>` variants
in table cells are the line-break exception. Images are alternative text only,
not downloaded graphics or terminal image protocols.

### Rendering appearance

- **Code blocks** have cap rows above/below when a width budget is configured
  (as in the viewer), and syntax highlighting when the language is recognized.
  Without a width budget, there are no panel cap rows.
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
and `o{` zero or more. Thus `DOCUMENT ||--o{ SECTION : contains` retains both
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

Text-projection rejection ceilings are 65,536 source bytes, 8,192 lexical units, 256
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

### View and edit documentation

```bash
# Open README
blad README.md

# Edit it
# Press 'e', make changes in editor, save and close
# Blad re-renders automatically
```

### Render text for sharing

```bash
blad --dump README.md > readme.txt
blad --math source --mermaid source README.md
```

Open the [feature showcase](EXAMPLES.md) to exercise the rendering profile.
The [optional Kvist/Skott integration reference](SESSIONS.md) is separate from
standalone file viewing.

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
