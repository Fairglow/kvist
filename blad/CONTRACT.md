<!-- kvist-contract-version: 1 -->

# Blad Contract

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Boundary and ownership

The crates.io package is `kvist-blad`; the installed executable and imported
Rust library are both `blad`. Rust 1.95 is the supported compiler minimum, not
an exact toolchain pin. The default installation is a standalone file viewer.

- **Component ID:** `kvist.blad`
- **Contract IDs:** `blad.cli/v1`
- **Owner:** Blad component
- **Consumers:** human CLI users, automation, scripts

Blad is a terminal Markdown viewer that reads skott session transcripts and
standalone Markdown files. It owns rendering, navigation, and interactive
display. It does not own session creation or modification.

## Provided interfaces

The `blad` executable provides:

- `blad [OPTIONS] <FILE>...` — open files in one viewer, in argument order
- `blad --version` / `blad -V` — package version, without opening files
- With Cargo feature `sessions` only: `blad list [--limit <n>]`,
  `blad session <ID-OR-DIRECTORY>`, and `blad export <ID> [--output FILE]`

The default feature set is empty. Session APIs and session-only options
(`--log-dir`, list `--limit`, export `--output`) MUST be absent without
`sessions`. General rendering options MUST remain in both builds. Kvist
all-feature builds enable sessions; embedders can use the renderer alone.
No command aliases or positional session auto-detection are retained.
`view`, `show` and `stream` are not subcommands. Filenames matching enabled
subcommands are passed after `--`. Empty invocation with terminal stdin displays help.
Active sessions are current snapshots, explicitly identified as such; no
background/live follow is promised.

Files need not have Markdown extensions. Canonical duplicates reuse one tab.
All initial files are loaded before viewer entry; read errors fail explicitly.
Noninteractive/dump output renders each distinct canonical file once, in
first-argument order.

Without file arguments, redirected stdin supplies one Markdown document.
`blad -` explicitly reads stdin, including among named files; repeated `-`
inputs reuse that source and render once. A literal file named `-` is addressed
as `./-`. Explicit stdin requires redirection, not a terminal. Named file
arguments otherwise leave stdin unread. Stdin is UTF-8 with the same inclusive
16 MiB bound and single overflow probe as file sources. Empty stdin is a
successful empty document. Named files validate before terminal entry;
noninteractive/dump sources all validate through EOF before output.

The CLI automatically enters the viewer with terminal stdout and no `--dump`,
including piped or file-redirected stdin. With redirected stdin it MUST open
the controlling terminal for keyboard input before reading document bytes.
If unavailable, it MUST fail promptly with an actionable `--dump` diagnostic,
not consume stdin as keystrokes or silently select another mode. No `--pager`
or `--no-pager` option is needed: `--dump` explicitly disables paging.
This deliberately replaces the previous both-stdin-and-stdout-TTY routing.
Noninteractive snapshots default to escape-free rendered text. Explicit
`--color always` opts into styling even through a pipe; `--dump` still overrides
it. Downstream `BrokenPipe` is quiet success, including optional session stdout
routes; other I/O failures retain status 1 and escaped diagnostics.

### Live stdin pager

An interactive stdin tab starts empty and displays incoming text without
waiting for a newline, completed Markdown block or EOF. Partial UTF-8 sequences
wait for their remaining bytes; invalid UTF-8 is never replaced or silently
dropped. Source newlines are preserved across read boundaries. The same
whole-document renderer and options apply to every provisional snapshot.
Later input MAY revise earlier rendering, including reference links, footnotes,
lists and incomplete fences. At EOF rendering/navigation match one-shot
rendering of the complete source at the same width/options.

The stdin tab initially follows the bottom. Upward navigation, search, link
selection and heading/anchor browsing pause following; normal `G`/`L` or
`Ctrl+End` resumes it, as does scrolling down to the bottom. Paused numeric
offsets are retained and clamped to the layout. Resize and tab switching retain
the follow state. Search query/mode/direction persist and results refresh;
source changes clear obsolete link selection. The footer identifies live
following, live paused, EOF or a persistent input error alongside normal
search/navigation feedback.

The interactive stdin entry point exclusively consumes process stdin. Before
viewer entry it duplicates the document descriptor with close-on-exec and
replaces fd 0 with the controlling terminal. Callers MUST NOT concurrently
read stdin or pre-buffer it through another reader while invoking this
interactive helper. Rendering-only APIs and finite routes do not replace stdin.
Editor handoffs receive terminal input, never the document descriptor.

Ingestion continues while another tab or help is visible; inactive stdin
renders on activation, rather than being fully rendered on each incoming
fragment. Closing stdin stops ingestion and releases its document descriptor.
EOF/input failure also releases that descriptor while retaining display/error
metadata. EOF, including empty input, leaves the viewer open for browsing.
Quit never drains input or supervises/kills the
producer. Browser/editor handoffs retain their existing synchronous waiting
semantics; the kernel pipe provides backpressure while Blad is busy.

Malformed UTF-8 (including an incomplete sequence at EOF), source overflow and
read errors stop ingestion. The valid retained document remains browsable with
a persistent footer error; closing/quitting subsequently returns status 1 with
an escaped diagnostic after terminal restoration. Rendering or terminal writer
errors propagate after restoration. A producer's nonzero exit cannot be
inferred from pipe EOF; shell users needing its status should use `pipefail`.

Document and keyboard readiness wake the event loop independently. Input is
pumped in bounded batches and renders are coalesced on a timed
cadence, never a producer-idle debounce. The retained source limit is 16 MiB,
not an unlimited-history promise. There is no disk spool, new worker/daemon,
unbounded input queue or total-memory/hard render/search/I/O latency guarantee.
This is raw Markdown stdin paging, not live following of session directories
or a `tail -f` mode for named files. The renderer library remains a finite
string-in/writer-out API without its own streaming parser state.

### File input bounds

Standalone Markdown files MUST be regular UTF-8 files of at most **16 MiB
(16,777,216 bytes)** each. This inclusive bound applies to initial files,
new file-link targets, reload and successful-editor rereads, in interactive
and dump routes. Size is checked before decoding/rendering; actual reading
is limited to the bound plus one probe byte even if metadata understates
content. Oversized input produces a distinct `FileTooLarge` I/O error naming
the byte limit, never successful truncation. Initial input errors precede
all output/terminal entry. Link/reload failures leave the previous document
and tab/history structure usable; post-editor reread errors propagate after
terminal restoration, as other editor errors do.

Document and explicit theme reads open nonblockingly without following a
final symlink, require an opened regular-file descriptor and read that same
descriptor. Initial file arguments, new file-link targets and theme paths
are canonicalized first, preserving explicitly selected symlinks and canonical
deduplication. Reload/editor rereads do not recanonicalize a substituted final
symlink. Themes retain their separate inclusive 64 KiB (65,536-byte) limit.

These are per-file source bounds, not a total retained-tab-memory limit or
a latency guarantee for rendering/search, path resolution, open, filesystem
reads or editor execution. Ancestor symlinks, concurrent content snapshots,
device-open effects and kernel/FUSE stalls are not isolated. Session artifact
loading and caller-supplied rendering strings are outside these file-read
limits; this is not an unlimited-stream or giant-file pager.

### Options

| Option | Description |
|--------|-------------|
| `--log-dir <PATH>` | With `sessions` only: session runs directory (default: `.skott/runs`) |
| `--version`, `-V` | Print the package version |
| `--dump` | Output plain text without interactive viewer |
| `--line-numbers` | Show rendered-line numbers in the interactive viewer |
| `--theme auto\|dark\|light\|terminal` | Appearance; defaults to `auto`, conservatively dark when unknown |
| `--theme-file <PATH>` | Explicit JSON palette file, at most 64 KiB; no auto-discovery |
| `--color auto\|always\|never` | Color policy; defaults to `auto`; explicit `always` overrides `NO_COLOR` |
| `--math unicode\|source` | Supported mathematical text projection or original TeX; defaults to `unicode` |
| `--mermaid text\|source` | Supported text diagrams or original source; defaults to `text` |
| `--diagram-charset unicode\|ascii` | Structural diagram glyphs; defaults to `unicode`; preserves authored Unicode |

`--dump` MUST force escape-free rendered text regardless of color/theme flags.
Automatic CLI color MUST honor a nonempty `NO_COLOR`, output TTY state and
`TERM=dumb`. Color depth is independent of appearance. No terminal query is
required: automatic appearance MAY use environment hints, defaulting to dark.
Exports MUST retain source Markdown and MUST NOT render its math or diagrams.

The library exposes typed rendering options and host-provided palette/color
capabilities through `MarkdownRenderer`. Library defaults are deterministic;
embedding hosts supply their own policy rather than inheriting CLI environment
guessing. Width, palette, math mode and diagram options persist across reflow.

### Keyboard shortcuts (interactive viewer)

Navigation: `↑`/`k`, `↓`/`j`, `PageUp`/`Ctrl+B` (full page up),
`PageDown`/`Ctrl+F`/`Space` (full page down), `u`/`Ctrl+U` and
`d`/`Ctrl+D` (half pages), `g`/`H` (top), `G`/`L` (bottom), mouse wheel.
`[`/`]` jump to the previous/next rendered heading without wrapping.
Search: `/` starts a fresh forward literal search; normal-mode `Ctrl+R` starts
a fresh forward regex search. Both are case insensitive. `Ctrl+R` toggles
literal/regex in either prompt, `Ctrl+U` clears the prompt,
`Enter` keeps results and returns to normal mode; `Esc` cancels an active search
prompt. After Enter, normal-mode Esc closes the active tab rather than clearing
retained results. `n`/`>` move forward and `N`/`<` move backward, wrapping at
document boundaries. Plain `p` has no search action. Each nonempty,
nonoverlapping occurrence counts separately. Search is per rendered line, not
across soft wraps. Exact matching spans are highlighted; a partial grapheme
match highlights its whole grapheme. Search begins at/near the viewport;
navigation reveals horizontal matches too. The footer shows mode, direction,
current/total, invalid-regex diagnostics or no matches, including after Enter.
Regex uses Rust `regex` syntax (no lookaround/backreferences), case insensitive
unless overridden inline; pattern input is limited to 4,096 UTF-8 bytes and
compiled regex/DFA caches to 2 MiB each. Zero-width matches are ignored.
Links: `Tab`/`Shift+Tab` select links directly, `Enter` follows the selection
or the first link at/below the viewport. Left-clicking rendered labels
follows their destination. There is no link mode; plain `f` has no action.
Only the active link's rendered spans receive selection emphasis.
Prose, headings, table cells and repeated bare URLs share renderer-owned
position metadata for keyboard selection and mouse hit testing.
`Ctrl+Left`/`Right` select a link to the left/right on the same rendered row;
`Ctrl+Up`/`Down` select the nearest row above/below, then the nearest column.
Wrapped spans participate by position; the selected link is excluded.
With no selection, navigation starts at the viewport's top-left. There is no
spatial wrap; Tab/Shift+Tab retain source-order wrapping.
Heading fragments use lowercase heading text, preserve Unicode, hyphens and underscores,
replace spaces with hyphens, omit punctuation and suffix duplicates with `-1`,
`-2`, etc.; explicit heading IDs are also supported. Percent-encoded fragments
and file paths are decoded as UTF-8; malformed encoding is an actionable error.
Other: `e` hands the whole file to `$EDITOR`, `b` returns to the previous tab,
`Alt+Left`/`Alt+Right` switch open-file tabs. Normal-mode `q`/`Esc` closes
the current tab, selecting the next tab (or previous if last); closing the sole
tab exits. Normal-mode `Q`/`Ctrl+Q` quits all tabs. Modal Esc cancels only that mode.
`?`/`h`/`F1` opens keyboard help (any key except Ctrl+C dismisses);
`Ctrl+C` always quits, including from help/search. `Ctrl+L` forces a repaint.
Interactive file tabs reload automatically from their opening canonical path.
Reload retains options, tab order/history and numeric viewport offsets,
clamping offsets to the new layout. Search query/mode/direction persist and
results are recalculated near the retained viewport; link selection is cleared.
Reload opens the candidate nonblockingly without following a final symlink,
then requires the opened descriptor to be a regular file before reading.
Replacing the final path with a FIFO or symlink therefore fails visibly.
This is not ancestor-path isolation or an atomic snapshot of concurrent writes.
New contents replace the current document only after successful UTF-8 read and
render. Errors retain the previous document and appear in the footer.
The active path is polled
every 250 ms, with changes debounced until metadata is stable for at least
250 ms. Inactive tabs refresh when revisited. In-place saves and atomic
replacements are detected; no worker, network service or watch flag is needed.
Automatic reload uses the transactional read/render and input limits above,
retains the search prompt and numeric viewport offsets, and cancels obsolete
link selection. Failures are shown once per observed metadata state and
retried after a subsequent change. `F5` has no action; `Ctrl+R` starts regex
search in normal mode and toggles regex inside the prompt, never reloads.
Successful automatic
reload does not hide search feedback with a success notice. Pathless documents,
session snapshots and noninteractive output do not watch.
This is not tail-following, conflict detection, a source-anchor-based position
guarantee, or a hard filesystem/rendering deadline.
The top row lists files in stable opening order with the active tab bracketed.
Label columns remain fixed when the active marker changes. Opening an existing
file reuses its tab; switching tabs preserves vertical/horizontal position.
Missing files/anchors and browser errors appear in the footer.
Explicit browser launches have null stdin and captured stdout/stderr, never
inheriting the viewer terminal. At most 4,096 diagnostic bytes are retained;
truncation is explicit. Successful launcher diagnostics are labelled as such,
not reported as failure. Nonzero exit and spawn failures remain visible errors.
The latest result survives redraw/focus events until the next user action.
The synchronous launcher has a five-second deadline measured after spawn.
On timeout Blad terminates and reaps the directly owned launcher, reports a
timeout with retained diagnostics, and keeps the document open. It does not
wait for descendant-held output EOF or terminate independently launched
browsers. Input waits while launch is pending; no worker or daemon is added.
The deadline bounds normal launcher waiting, not kernel scheduling or process
creation/termination system calls.

Theme files are JSON objects (serde_json's JSON dialect) with optional `theme`
(`dark`, `light`, `terminal`) and `palette` objects. Palette keys are `heading`,
`link`, `muted`, `list_marker`, `math`, `alerts` (five colors), `chrome`,
`search`, `code_background`, `table_background`, and `quote_background`.
Colors are `"#RRGGBB"` strings or integer ANSI indices 0–255; nullable
backgrounds accept `null`. Unknown keys, malformed colors, oversized/unreadable
files and invalid JSON MUST fail explicitly. Unspecified roles inherit the
resolved appearance. An explicit non-auto `--theme` overrides file appearance;
file palette overrides still apply. No file controls process execution.

## Required interfaces

Blad depends on:

- Skott's session format: `.skott/runs/<session-id>/session.json` and `messages/` directory
- `$EDITOR` environment variable for edit mode
- A pinned, vendored native Rust text renderer for admitted Mermaid families;
  no Mermaid runtime executable or browser is required

## Data and schemas

Blad does not persist user data; all rendering operates on provided input files.

### Terminal Markdown profile

CommonMark parsing and documented extensions are terminal projections, not
full CommonMark/GFM conformance claims. Raw HTML is omitted; image alternative
text remains visible without graphics. Task items retain `[x]` or `[ ]`
markers. Tables retain source column order and honor delimiter left/center/right
alignment as specified below. Table cells support bold, italic and strikethrough,
including combinations and emphasis within link labels. Dedicated inline-code
appearance and nonbreaking/whitespace semantics, and table link colors, remain
unsupported. HTML tables and merged cells are outside this profile. Script,
math and alert extensions are Blad's documented dialect.

Footnote definitions use the palette's muted color for ordinary prose and
matching definition tags, including continuation rows and paragraphs. Inline
links and math keep their semantic colors; bold, italic, strikethrough and
inline-code reversal compose with the footnote's prose color. After each inline
span, footnote prose resumes its muted color. Styling ends with the definition,
does not leak into following prose, and does not change wrapping or plain text.

### YAML frontmatter

All Markdown inputs, including library strings and stdin, recognize one leading
YAML frontmatter block. The first line MUST be an unindented `---`, optionally
preceded by a UTF-8 BOM. A subsequent unindented `---` or `...` closes the block;
delimiter lines MAY have trailing spaces/tabs. LF and CRLF, an empty body or
leading blank lines within the header, and a closing line without a final
newline are supported. The BOM is not displayed. No filename or metadata schema
is required. Blank lines before the opener, indented delimiters and later
blocks are not frontmatter. `+++` metadata is not supported.

The header, including both delimiters, MUST be displayed as literal YAML through
the same syntax highlighting, background/cap rows, unwrapped overflow and
color/plain policy as a fenced `yaml` code block. Authored layout and text
remain intact except existing inert-control and LF output normalization.
There is no YAML parsing/validation, key sorting, reserialization, alias
expansion, schema enforcement or configuration/permission effect. Malformed
YAML in a closed block remains visible. Metadata MUST NOT create Markdown
headings, links, reference definitions or footnotes; its rendered text remains
searchable. The following body retains source-owned link/heading offsets and
normal Markdown semantics.

Without a closing delimiter, the entire input retains normal Markdown parsing,
not an implicitly closed or hidden metadata block. This also applies to live
stdin snapshots; completing the delimiter MAY revise previous output. EOF MUST
match finite rendering at the same width/options. Ordinary separators elsewhere
and fenced/quoted examples retain their existing meaning. No option or new
dependency is required.

### Optional Kvist/Skott integration reliability

`sessions` is an opt-in published feature, not a private Cargo feature or a
runtime dependency on Kvist/Skott. Its user reference is `docs/SESSIONS.md`.
Session APIs return `std::io::Result`; lookup returns `NotFound` for unknown IDs
and `InvalidInput` for empty/ambiguous prefixes. Lookup has no 1,000-item cutoff.
Listing a missing runs root succeeds with an empty list; a directory without
`session.json` is ignored. Other input/parse failures propagate with context,
not empty message bodies or a successful empty result.

Each UTF-8 manifest/message is limited to 16,777,216 bytes inclusively.
Required manifest/entry fields are validated; additional producer fields are
allowed. Manifests require `schema_version: 1`, `kind: "skott-conversation"`,
nonempty `session_id` and `status` strings, and an `entries` array (which may be
empty); each entry requires a positive
`sequence`, nonblank `kind` and a `path` in the shape below. `model` and `created_at` are optional
strings. Entry paths must have exactly `messages/<filename>` shape with normal
path components. Opened session roots, candidate directories, manifests,
message directories and message files reject final symlinks. Descendant reads
use directory descriptors with no-follow traversal, nonblocking opens and
regular-file validation. An explicitly selected root's ancestor paths are not
isolated. Messages load before snapshot/export output; read failure emits no
successful partial transcript. Export to a named output retains its existing
explicit overwrite behavior and is not an atomic-save protocol.

These are per-artifact limits, not aggregate-memory bounds, concurrency
snapshots, hard filesystem deadlines or a sandbox. Listing is newest-first by
the tuple (creation-time string, session ID), descending; missing timestamps
sort after present timestamps. IDs break timestamp ties.

Blad reads skott's session format and produces ANSI-colored terminal output.

### Session format (input)

```
.skott/runs/
└── {session-id}/
    ├── session.json    # Manifest with metadata
    └── messages/
        ├── 000001.md
        ├── 000002.md
        └── ...
```

The session manifest (`session.json`) contains:
- `schema_version`: Required integer `1`
- `kind`: Required string `"skott-conversation"`
- `session_id`: Unique session identifier
- `model`: Model identifier used
- `created_at`: ISO 8601 timestamp
- `status`: "completed" or other status
- `entries`: Array of message entries with sequence, kind, path, etc.

### Output format

Blad renders Markdown to ANSI-colored terminal output with:
- Syntax-highlighted code blocks (via syntect)
- Bold, italic, strikethrough formatting
- Table rendering with column alignment
- GFM alert blockquotes with color-coded labels
- Footnote references and definitions
- Math expressions (inline and display)

### Width and viewport semantics

Interactive prose and headings wrap to the terminal width minus one scrollbar
column and, when requested, the rendered-line-number gutter. There is no
additional wrap margin. Resize recalculates that budget. Code block lines and
atomic inline-code spans are not broken to fit the viewport; the existing
Left/Right keys reveal horizontal overflow without resetting on shorter lines.
Home/End select horizontal start/end; Ctrl+Home/End retain vertical top/bottom.

Each screen row is positioned and clipped independently, including header/footer
text. Wide graphemes may extend past the content column into the scrollbar area.
Frames are buffered, synchronized where the terminal supports it, and only
changed rows are written. Normal scrolling and tab switching never clear the
screen or re-enter the alternate screen. Resize/refresh invalidates row state;
editor handoff releases and restores terminal ownership.

### Table layout semantics

Tables MUST preserve their existing borderless, reverse-video header and
alternating data-row backgrounds when styling permits, with ` | ` between
columns. Terminal-native/monochrome/plain output MAY omit fixed backgrounds
without changing geometry. The width budget
includes these separators. A row's height is the maximum wrapped cell height;
shorter cells are padded to that height.

Delimiter markers `:---`, `:---:` and `---:` MUST select left, center and right
alignment respectively, for the header and every rendered body-cell line.
Unspecified alignment MUST remain left. Padding MUST use Unicode display
columns; an odd extra center-padding space MUST go on the right. Alignment
MUST NOT alter column widths, wrapping, row heights or row styling. Empty
lines/cells retain the full column width. Link spans MUST cover their rendered
labels after leading alignment padding, never that padding, including after
resize and inside quotes or numbered viewports.

Cell bold, italic and strikethrough MUST follow parser-recognized nesting and
combinations through normalization and wrapping. Retained inter-word spaces
inherit the attributes of the first source whitespace they represent.
Attributes MUST end before alignment padding, column separators and other
cells/rows, without clearing header reversal, row backgrounds or quote styling.
Plain output MUST suppress attributes without changing text or geometry.
Link marks and viewer search/selection overlays MUST remain correct through
attribute transitions, Unicode, clipping and resize. Existing cell whitespace
normalization and wrapping also apply to inline-code text; code does not gain
its own reverse-video styling or atomic spans. Supported enclosing emphasis
MAY apply to code/math/footnote text without adding their dedicated styling.

Under the fixed terminal geometry model (display-column width : line height =
1:2), each cell's width floor MUST come from its own nonempty wrapped text
lines: four display columns per occupied text line, capped at the cell's
natural width (its widest normalized explicit line without soft wrapping).
Empty explicit lines and vertical padding MUST NOT contribute to this floor.
A fully unwrapped value MUST NOT be widened solely for aspect ratio. This
models a visual 2:1 text ratio where possible without adding artificial width;
actual font metrics are not queried.

Each column's lower bound MUST be the maximum of its cells' text-derived
floors. Its upper bound MUST be the maximum of its cells' natural widths.
An entirely empty column MUST occupy zero content columns; separators remain.
Neither a tall neighboring cell nor unused viewport space may widen a column
beyond that upper bound.

If a valid layout fits, the renderer MUST choose the smallest possible maximum
row height within the width budget. If none fits, it MUST overflow horizontally
using the smallest valid total width, breaking ties by smaller maximum row
height. Left/Right navigation continues to reveal overflow. With no width
configured, columns MUST use exactly their natural widths without soft
wrapping. Spare viewport space MAY be distributed deterministically
up to natural column widths; total table height is not a globally optimized
objective.

Cells MUST wrap greedily between whole whitespace-delimited tokens. Intra-line
whitespace becomes one space; explicit newlines, including empty lines, remain.
HTML `<br>`, `<br/>`, and `<br />` tags (case-insensitive) inside table cells
MUST contribute explicit newlines.
Unicode display width determines measurement, not byte or character count.
Tokens MUST NOT be split, even when a long identifier or URL forces overflow.
Terminal controls are made inert by the same normalization used by viewport
text. Resizing MUST solve the layout again, not shrink an already valid layout
below its aspect floor.

### Script and mathematical text

Markdown `^...^` and `~...~` spans, including adjacent `E=mc^2^` and `H~2~O`,
MUST use a curated whole-span Unicode mapping or explicit `^(...)`/`_(...)`
fallback. Code and escaped notation MUST remain literal. These are Blad dialect
extensions, not a claim of universal GFM syntax.

Math uses parser-recognized `$...$` and `$$...$$`. The admitted grammar comprises
literals, nested groups, single/grouped scripts, the documented Greek/operator
commands, `\frac`, `\sqrt`, `\text`, and paired supported `\left`/`\right`
delimiters. Fractions and roots MUST retain grouping; unsupported or malformed
expressions MUST preserve the complete original source and delimiters with a
visible explanation. Display math is a separate text block, never graphics.
Escaped dollars and unmatched delimiters remain literal; currency-only
expressions MUST NOT be converted to mathematics.

### Mermaid text profile

Mermaid fences MUST produce spatial text diagrams for admitted flowchart,
sequence, state, class and ER syntax. Unsupported semantics MUST retain source
with an explanation rather than disappear or become a success placeholder.
The admitted profile and approximated presentation MUST be documented.
Diagrams MUST retain authored direction, labels and endpoints and MUST NOT
silently rotate, truncate labels, or wrap completed diagram rows to fit.
Unicode/ASCII selection affects structural glyphs only.

Resource-limit failures MUST be visible and distinct from unsupported syntax.
Cancellation and writer failures MUST propagate rather than become source
fallback successes. No document-triggered external command is permitted.

## Behavioral guarantees

- Session listing is deterministic, sorted by creation time (newest first)
- All operations are local; no network access required
- I/O errors are reported clearly; failures do not crash the interactive session
- Paths are validated; symlinks are not followed when reading session artifacts
- Rendering is fast even for large transcripts (thousands of messages)
- The viewer retains the first document line at the first content row
- Prose and headings wrap at the available display-column width
- Resize triggers reflow to the new content width

## Errors and failure semantics

- Invalid session ID: clear error message with available IDs
- File not found: standard CLI error with path
- Terminal too small: functional with minimum dimensions, no panic
- Unsupported math/diagram syntax: original source with an informative note
- Rendering resource limit: original source and a distinct limit diagnostic,
  or an explicit error if bounded source output cannot be produced

## Security and authority

- Read-only access to skott session files
- Source documents are read-only except for explicit editor handoff.
  Named session export and an explicitly configured diagnostic log are the
  other write surfaces; export may overwrite its selected path.
- No network access
- No credential or secret exposure

### Diagnostic logging

`BLAD_LOG` selects tracing levels, falling back to `RUST_LOG`, default `warn`.
An explicit `BLAD_LOG_FILE` appends diagnostics to a regular file opened once
at initialization, nonblockingly and without following a final symlink. New
files use mode 0600 subject to umask; existing modes are not changed. Invalid
destinations produce an escaped stderr warning and use stderr instead.
No implicit log file is created. Footer notices and fatal CLI diagnostics
remain their own surfaces; this does not promise an event for every action.
As with document input, ancestor paths and hard kernel-I/O deadlines are not
isolated. A host embedding the library may already own the tracing subscriber.

## Compatibility and verification

- Linux only (uses crossterm for terminal interaction)
- Depends on existing workspace dependencies: clap, serde, pulldown-cmark, syntect, crossterm
- Version tracking via Kvist artifact version markers