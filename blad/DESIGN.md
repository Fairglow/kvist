<!-- kvist-design-version: 1 -->

# Blad Design

Blad is an interactive Markdown file viewer with syntax highlighting,
search/navigation, tabs, and optional external editing. The opt-in `sessions`
feature reads skott's structured format from `.skott/runs/`, adding session
listing, snapshot output, and transcript export, without live following.

## Design overview

### Publication preparation (2026-10-24)

Publish only `kvist-blad` from the existing workspace, preserving `blad` binary
and library targets and the downstream dependency alias. Keep Rust 1.95:
the admitted Merman/Dugong graph requires it too. Package an explicit set of
source, tests/fixtures, README, changelog, unchanged AGPL license and user docs.
Move the architecture-table regression to a local fixture rather than removing
its assertions. Standalone documentation excludes session instructions; the
opt-in Kvist/Skott integration lives in `docs/SESSIONS.md`.

Task-list parser events emit literal `[x] ` or `[ ] ` through the inline
writer using list-marker styling. Plain rendering preserves the same text.
Clarify existing Markdown projection limitations rather than claiming full
conformance. Document current search/help precedence without claiming that
earlier human-arbitration findings have been accepted.

Replace session Option/empty-result failure erasure with `io::Result`.
Use safe rustix descriptor-relative opens for session candidates and
`messages/<filename>`, with NOFOLLOW/NONBLOCK/CLOEXEC and regularity validation.
The direct rustix edge reuses the already locked package, not a new runtime
process or service. Require the session root directory without following its
final link; do not claim isolation of explicitly supplied root ancestors.
Read a maximum 16 MiB plus one byte from each opened regular artifact before
UTF-8/JSON decoding. Deserialize required typed fields while allowing extra
producer metadata, retaining the public manifest value for existing callers.
Listing skips only directories without a manifest, returns empty for a missing
runs root, and otherwise reports errors. Sort a total descending tuple of
optional creation time and ID. Lookup scans all sessions, prefers an exact ID,
and rejects ambiguous/empty prefixes. Load messages before writing snapshot or
export bytes. No error branch returns successful missing content.

Standalone CI covers both feature modes on stable/MSRV, lint/docs, actual
archive extraction, full archive tests and isolated release installs. A
separate explicitly invoked publication surface must bind an accepted clean
revision and version and publish only this package, never the workspace.
Registry/account approval cannot be inferred from a local successful build.

This increment uses a deliberate advisory-review exception while the planned
receipt mechanism is unavailable. The exception is not human acceptance, a
Kvist receipt or compliance evidence. Independent reviews follow implementation.

Blad is a CLI terminal application built on Rust with clap for argument
parsing, crossterm for interactive terminal I/O, pulldown-cmark for Markdown
parsing, and syntect for syntax highlighting. It reads skott session files
and renders them to ANSI-colored terminal output.

## Internal structure

Appearance lives in `theme.rs`: deterministic `RenderOptions` and semantic
`Palette` values are host inputs, not renderer environment policy. CLI policy is
resolved once in `main.rs`; options are forwarded through sessions, viewer
resize and editor re-entry. `output.rs` streams only renderer-owned SGR through
the chosen color encoder. Authored text is made inert both before parsing and
after parser entity decoding; generated and authored controls never share that
trust boundary. Plain output strips every style without changing geometry.
Syntect syntax/theme assets are immutable `OnceLock` values shared across
renders. Editor errors propagate rather than being printed as successful
completion.

The P1 independent audit found entity-decoded controls, unescaped listing
metadata and editor errors discarded by legacy code. Regression tests pinned
the control failures, fixes were applied, and a separate clean-source
observation confirmed the narrowed boundary. Existing search-navigation offset
handling is not expanded by this milestone. Search overlay composition and
editor integration evidence are retained for P5.

P5 originally composed a row-wide search overlay after each content SGR
transition. The production viewer increment replaces it with exact-span
overlays while retaining reset resilience. Terminal text owns
the shared inert-source boundary. Syntax-cache identity is executable evidence,
not a per-render reload. The viewer edit path returns a typed action to an outer
loop; previous viewport documents and terminal guards are dropped before the
editor runs, avoiding recursive viewer frames while retaining all render options.
Editor/status/reread errors continue to propagate.
The final source audit also found that raw Unicode width treated tabs
differently from viewport expansion, causing code-panel padding to manufacture
horizontal overflow. Both highlighting and plain-code paths now use the same
eight-column tab-stop primitive as terminal text. Library and PTY regressions
failed first and then passed; a separate narrow source observer confirmed the
correction. Authored code whitespace is not rewritten.

P5 text characterization (14,900 UTF-8 source bytes, 100 repeated mixed
heading/script/math/code/table sections; optimized test profile): cold render
31.80 ms, warm mean 5.45 ms truecolor, 5.50 ms ANSI256 and 5.36 ms ANSI16.
These are measured corpus timings, not universal latency guarantees. Immutable
asset identity, empty-tool-PATH rendering, unchanged stdin cursor, PTY palette/
projection persistence and whole-output geometry have executable coverage.

```
blad/
├── src/
│   ├── main.rs          # CLI entry point and command dispatch
│   ├── lib.rs           # Library exports
│   ├── session.rs       # Session loading and listing
│   ├── display.rs       # Terminal display and interactive viewer
│   ├── markdown.rs      # Markdown rendering with syntax highlighting
│   └── debug_events.rs  # Debug event logging
├── tests/
│   └── integration.rs   # Integration tests
├── REQUIREMENTS.md
├── CONTRACT.md
├── DESIGN.md
├── IMPL.md
├── TODOS.yaml
└── Cargo.toml
```

## Interactions and state

Blad is stateless; it reads files and displays them without maintaining
persistent state. The interactive viewer maintains transient UI state
(scroll position, search query, match index) only for the duration of the
viewer session.

### Session loading (`session.rs`)

- `list_sessions(log_dir, limit)`: List recent sessions, sorted newest first
- `load_session(log_dir, session_id)`: Load session by ID (exact or prefix match)
- `load_session_by_path(path)`: Load session by directory path
- `load_messages(session)`: Load all messages for a session

### Terminal display (`display.rs`)

- `show_session(session)`: Display current session snapshot (`sessions` only)
- `dump_session(session)`: Output session as plain text
- `view_markdown(content)`: View rendered markdown interactively
- `view_markdown_file(path, content)`: View with edit support
- `dump_markdown(content)`: Output rendered markdown as plain text
- `export_session(session, output)`: Export to combined markdown document
- `interactive_viewer(path, content)`: Full interactive viewer with navigation and search

### Markdown rendering (`markdown.rs`)

- `render_markdown(md, writer)`: Render markdown with syntax highlighting
- `find_syntax(ss, lang)`: Find syntax definition for a language
- `render_mermaid(content)`: Bounded native text-diagram adapter
- `render_table(rows)`: Apply backgrounds and separators to the solved table
- `write_emphasized(...)`: Write text with ANSI emphasis
- `to_ansi_line(ranges)`: Convert syntect highlighting to ANSI

## Algorithms and decisions

The design involves the following key algorithmic and architectural choices.

### Markdown rendering pipeline

Blad uses `pulldown-cmark` for parsing and `syntect` for syntax highlighting.

Supported Markdown features:
- All CommonMark elements
- GitHub Flavored Markdown extensions:
  - Tables
  - Strikethrough
  - Task lists
  - Autolinks
  - Footnotes
  - Superscript/subscript
  - Alert blockquotes ([!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION])
- Fenced code blocks with syntax highlighting (50+ languages)
- Inline and display math (LaTeX)
- Mermaid diagrams (native text-only integration)

### Interactive viewer

The interactive viewer provides:
- Scroll navigation (keyboard and mouse wheel)
- Search with highlight and match navigation
- Status bar showing scroll position and search results
- Scrollbar for visual position feedback
- Edit mode (press `e`) to open file in `$EDITOR`

### Integer table constraint solver

`table/` separates prepared text and wrapping (`cell.rs`), integer optimization
(`mod.rs`), and the existing styled output in `markdown.rs`. Preparation measures
each whole word once with `unicode-width`, after `TerminalLine` normalization.
The same allocation-free wrapped-line iterator serves both height queries and
final string production. Empty explicit lines produce one line.
Table hard-break events and inline `<br>` variants append a cell newline.
Whole-word greedy wrapping has a nonincreasing height as width increases; allowing
width-dependent hard word splits would invalidate that search invariant.
No new dependency or generic continuous constraint engine is needed.

Let `h_rc(w)` be a prepared cell's exact rendered height and `t_rc(w)` its
number of nonempty wrapped text lines (excluding empty explicit lines and
vertical padding). Let `N_rc` be its natural width and `L_rc` its longest
word. Binary-search the smallest integer cell floor `m_rc` in `[L_rc, N_rc]`
such that `w >= 4 t_rc(w)` or `w >= N_rc`. The latter is an explicit
natural-width exemption: no value is expanded solely for shape, even if short
text or many explicit breaks cannot achieve the aspect ratio. For empty text,
both bounds are zero. The predicate is monotone because whole-word wrapping
never gains occupied lines as width grows.

The column bounds are `m_c = max_r m_rc` and `N_c = max_r N_rc`.
They constrain each width independently: `m_c <= w_c <= N_c`.
In particular, there is no table-wide or row-wide aspect floor. All-empty
columns have zero width, and vertical padding is only a rendering concern.

Let `H` be a maximum rendered-row-height cap and
`B = viewport - 3(n-1)` the saturating content budget. For a candidate `H`,
binary-search each column in `[m_c, N_c]` for the smallest width with
`h_rc(w_c) <= H` for every cell. This yields the exact least content width
`S(H)` at that cap. Since `m_c` no longer depends on `H`, `S(H)` is
nonincreasing: binary-search for the smallest cap with `S(H) <= B`.
The cap lies between the maximum explicit-line count and the maximum actual
height at the column floors. Short-circuit infeasible candidates once their
width sum exceeds the budget. If even the floors exceed the budget, return
them directly: no other valid vector can have a smaller total width.
With no viewport or sufficient width, return natural widths directly.
Integer sums saturate at representational limits; proportional products use
`u128`.

Height queries short-circuit on exceeding the cap and allocate no rendered
text. Bounds depend on content, not the numeric viewport width. After cell
preparation, with `T` words/explicit lines, maximum natural width `W` and
maximum floor height `H_max`, the solve costs
`O(T log(W+1) log(H_max+1) + n log n)` including surplus sorting; it does not
enumerate width combinations or height caps.

After solving the cap and floor, distribute surplus up to natural widths by
integer proportional flexibility and largest remainders, with column index as
the stable tie-break. Increasing a width cannot invalidate feasibility.
Minimax height is deliberate: minimizing the sum of row maxima couples the
columns, so simple proportional water-filling does not prove that different
objective. No such global optimum is claimed.

Verification uses brute-force small tables as an independent oracle, plus
rendered-geometry integration regressions and large prepared-text cases.

### Design decisions

1. **Read skott format:** Blad reads skott's structured session format rather
   than defining its own. This avoids duplication and ensures compatibility.
2. **Separate rendering from display:** Markdown rendering is in `markdown.rs`,
   terminal display logic is in `display.rs`. This allows testing rendering
   without a terminal.
3. **Search on rendered content:** Search operates on the rendered ANSI output,
   stripping escape codes before matching.
4. **Mermaid fallback:** Unsupported diagrams retain their source and a visible
   explanation. No renderer invocation launches `mmdc` or other executables.
5. **No state management:** Blad is stateless; it reads files and displays
   them without maintaining persistent state.

## Failure and recovery

The increments below define recovery at the owning viewer boundary, including
the limits of terminal, subprocess and filesystem ownership.

### Bounded file-input increment (2026-10-23)

The next local reliability gap is inconsistent source reading: initial files,
new linked files and editor rereads use unbounded pathname reads, reload only
checks regularity, and theme input is byte-bounded but can wait on a FIFO.
Use one private `file_input.rs` implementation compiled into both library and
binary targets. This keeps the shared reader private while allowing the binary's
theme parser and library's display module to use identical descriptor rules.
No new public API, dependency, worker or shell invocation is needed.

Conservative scope assumption for this user-directed increment: admit standalone
source files up to a fixed inclusive 16 MiB until a separately designed streaming
viewer exists. This is a deliberate new input policy, not an accepted product
baseline or a universal responsiveness claim. Do not add an unbounded override.
Theme input retains its existing inclusive 64 KiB policy.

Open read-only with `O_NONBLOCK | O_NOFOLLOW`, then inspect descriptor metadata.
Reject nonregular objects before reading and reject reported sizes above the
chosen limit before allocating content. Read through `Read::take(limit + 1)`;
check actual byte count before UTF-8 or JSON decoding. Preserve read-error kinds
and report overflow as `FileTooLarge` with the allowed byte count. The extra byte
distinguishes exact-size success from understated/growing input without trusting
metadata as a snapshot. Never render a silently truncated prefix.

Canonicalize explicitly authored initial/link/theme paths before the shared
open to preserve selected symlinks. Reload and editor rereads operate directly
on the already stored canonical path, rejecting a final symlink substituted
since opening. Initial validation precedes output; failed links do not append
tabs/history; reload remains a candidate transaction. Keep the existing editor
failure exit/restoration policy rather than silently changing mode semantics.

Verify exact real document/theme boundaries, a bounded synthetic endless reader,
read-error propagation, nonregular inputs, authored/final-replacement symlinks,
initial no-partial output and usable viewer recovery. Use deadlines/owned-child
cleanup in hang regressions so a defect cannot strand the test suite. Record
scoped implementer inspection, then source-only observation and a separate
source-blind comparison. This bundle uses a deliberate advisory exception while
the planned receipt mechanism is unavailable; no acceptance/receipt is claimed.

The reader is not an ancestor sandbox, snapshot, total-memory bound or hard
I/O deadline. Nonregular objects are opened before validation; device-open
effects and path/open/kernel/FUSE delays remain possible. Per-file bounds do
not bound total tabs or rendered expansion, nor make synchronous rendering,
search or editing cancellable. Session readers and embedded source strings
remain separate boundaries. Preserve the earlier A-01/A-02 arbitration findings.

Scoped implementer inspection: both library and binary use the same private
reader source. Metadata and reads share one descriptor; actual-byte overflow
cannot become a successful prefix, and read errors retain their kinds. Limits
precede UTF-8/JSON parsing. CLI errors keep the existing escaped-stderr boundary;
viewer errors keep the inert footer boundary. Input rejection does not create
tabs/history or commit reload candidates. Editor reread failure occurs after
terminal release, preserving the established failure/exit behavior. No new
dependency, unsafe block, process, thread, network work or write authority is
introduced. Tests reproduce initial/theme FIFO waits and oversized-reload
acceptance before the fix, then cover exact byte/probe counts and recovery.
This scoped inspection is not an independent security certification; kernel,
device, ancestor, aggregate-memory and rendering-latency limits above remain.

### Viewer reliability follow-up (2026-10-23)

Prioritize the observed unbounded launcher wait, explicit reload and the
character-counting PTY evidence gap before larger navigation surfaces.
The five-second timeout is a conservative fixed default: normal launch remains
synchronous, but an owned launcher cannot intentionally keep the viewer waiting
forever. Use monotonic elapsed time after spawn, bounded drain batches and
short sleeps capped by remaining time. Check process completion before the
deadline. On expiry explicitly kill/wait the directly owned child; include
bounded captured output in a typed timeout error. Drop cleanup remains a
fallback for other failures. Do not terminate process groups: a successfully
started browser can legitimately outlive its launcher. No new dependency,
worker, shell invocation or environment-based timeout override is needed.

For reload, read and render a candidate before replacing any live content.
Keep the active content mutable within the document event loop so successful
reload does not release terminal ownership or change tab history. Recompute
owned links/spans and search against the new layout; clear link selection and
clamp x/y at the normal frame boundary. Retain all host rendering options.
Missing paths, invalid UTF-8 and render failures become inert footer errors
while old content/layout/navigation remain usable. `r` in search is literal
input and does not reload; existing modal key precedence is unchanged.
Use Linux `O_NONBLOCK | O_NOFOLLOW` on candidate open and validate the opened
descriptor's metadata before reading. This replaces the first metadata/read
precheck, whose FIFO-replacement race was independently observed. A direct
edge to already locked/vendored `libc` supplies the named flags without unsafe
code or a new registry package; its MIT/Apache-2.0 licensing is unchanged.
Final-component FIFO/symlink replacement cannot redirect the read after
validation. Ancestor-path isolation, concurrent content snapshots, FUSE/device
behavior and giant-file latency remain outside this local viewer guarantee.

Extract the existing terminal guard into a private module without changing its
ownership/restoration semantics to keep the event-loop file below its size
budget. The PTY test screen stores a grapheme at its first display cell and
empty continuation cells. Join subsequent scalars using Unicode segmentation,
including across source SGR transitions, and adjust cursor width using the
same declared Unicode width model. Cursor movement breaks join adjacency;
delayed wrap occurs only for a new grapheme, not a combining extension.
Keep explicit foreground-independent emphasis assertions. This is a bounded
terminal test model, not terminal probing or a claim about all font metrics.

Write regression tests first for timeout and cleanup, retained diagnostics,
reload success/failure/shrink/tab isolation, and wide/combining/joined Unicode
under selection, clicks, clipping and resize. Perform scoped resource/terminal
inspection and separate source-only observation/source-blind comparison.
This user-directed follow-up uses the existing deliberate advisory exception
while review receipts are unavailable; no acceptance or receipt is implied.
Retain the earlier Esc/help interpretation findings for human arbitration.

Scoped implementer inspection: launcher capture and drain batches are bounded,
deadline checks use monotonic time, and explicit timeout cleanup owns only the
direct child. Diagnostics retain the existing inert footer boundary. Reload
validates and reads the same opened descriptor; the initial pathname-check/read
race was corrected after independent observation. No unsafe code, shell,
worker, process-group termination or new registry package is introduced.
Nonregular objects are opened before validation, so this is not isolation
against device-open effects or ancestor-path replacement. Spawn, kill/wait,
regular-file reads and rendering have no hard real-time bound; concurrent
writes are not snapshots. The Unicode harness shares production segmentation
and width libraries and implements only selected terminal controls. This
scoped inspection is not an independent security certification.

### Production viewer increment (2026-10-23)

Keep terminal ownership in the open-file loop rather than each document event
loop. Release only for editor handoff or exit. A retained painter composes
complete, padded rows, compares them to the last successful frame, and emits
changed rows in one buffered synchronized update. Do not clear before writing;
padding erases stale text. Size changes and Ctrl+L invalidate the cache.
Commit cached state only after successful output/flush.

Search stores sorted occurrence spans with rendered row and display-column
intervals. Compile literal queries by regex-escaping them; use the already
vendored Rust regex engine with explicit input/compiled/cache bounds.
Find nonempty matches on normalized plain lines, mapping byte ranges to whole
grapheme display ranges without changing the underlying text. Store direction,
mode and current index; incremental edits choose the closest occurrence at the
viewport. Errors are retained, not treated as empty successful queries.
Span overlays restore accumulated source SGR on every boundary and reassert
the overlay after source resets, preserving syntax outside the exact spans.
Link spans use the same overlay path. Selecting links clears search emphasis.
Spatial navigation compares renderer spans, not label
text; deterministic row/column/source-order ties and horizontal reveal are
shared with search reveal. Source-order traversal remains available.
Table cells retain source-bound byte marks alongside text; the existing
whole-word wrapper maps those marks to final display columns without changing
table geometry. Bare URL occurrences get distinct event-relative identities,
shared by extraction and rendering after script/control projection. Selection
and mouse hit tests no longer guess from duplicate label text.

Closing a tab removes its history entries and decrements higher indexes.
Preserve surviving positions and tab order. Help is a separate transient
rendered document with no link actions; modal cancellation never closes tabs.
Ignore release events and reserve Ctrl-modified keys before plain-character
dispatch, so Ctrl+F does not enter link mode.

Tests precede code: styled exact-span cells, Unicode clipping, multiple
occurrences, search direction/wrap/errors, spatial selection, heading jumps,
closing tabs/history, no full clears or alternate-screen churn, unchanged-row
suppression, resize/stale erasure and terminal cleanup. Scoped input/resource
inspection and independent source-only observation/source-blind comparison
follow implementation. No acceptance or review receipt is fabricated; this
human-requested increment proceeds under a deliberate advisory exception while
the planned review mechanism is unavailable.

Scoped implementer inspection: `regex` reuses the already locked/vendored
MIT/Apache-2.0 engine without adding registry packages, shells or network work.
Queries, compiled automata and DFA caches have explicit limits; matching remains
synchronous and no hard giant-file latency guarantee is claimed. Regex errors
and queries cross the inert footer boundary. Source and bare-link identities
are distinct typed variants, avoiding collisions after length-changing text
projection. Table mark mapping fails explicitly if wrapping loses a token.
All frame rows are clipped/padded, SGR overlays restore source attributes, and
writer/flush/terminal-release errors propagate. PTY evidence checks canonical
mode and echo restoration after exit and editor errors. This scoped inspection
is not an independent security certification or a compliance determination.

### Standalone/session boundary increment

Use an empty default feature set and one `sessions` feature. Compile-gate the
session module, session display helpers, session CLI subcommands/options and
session-specific tests. Keep common flags and positional `Vec<PathBuf>` inputs
in both configurations. Clap version/help need no file/theme I/O.
Session commands live behind a private CLI adapter; `session ID` replaces
`show ID`, while the old non-following `stream` and redundant `view` disappear.
Keep rendering consumers feature-minimal; Kvist's existing all-feature recipes
and CI retain sessions, with an additional no-default-feature CI check.
Remove unused direct runtime dependencies rather than feature-gating them.

Preload canonicalized initial files into the existing open-file vector, dedupe
without changing argument order, and reuse the same event loop for single and
multiple inputs. Keep tab vector order fixed and bracket the selected entry.
Reserve the same marker columns for inactive tabs so labels never shift.
The terminal clips overflow without bringing the active tab to the front.

Capture explicit `xdg-open` diagnostics with a standard-library Unix socket pair:
child stdin is null, stdout/stderr share the capture socket, and the reader is
nonblocking. Drain in bounded per-iteration batches while waiting for the
launcher, retaining at most 4,096 bytes with explicit truncation. Once the
launcher exits, drain only immediately available output; inherited browser
descriptors must not keep the viewer waiting for EOF. No shell or new daemon
is introduced. The later reliability follow-up bounds the synchronous wait.
Successful diagnostics become a retained notice; nonzero status includes the
bounded diagnostics in an error. The viewport inert-text boundary owns display.

Tests precede implementation: both feature sets' help/version/CLI surfaces,
file extension independence, duplicate/multiple-file tabs, stable placement,
active snapshots, and fake launchers producing success warnings, failure,
large/control output and null-stdin reads. Socket-level coverage exercises
bounded reads while a writer remains open. No live browser is required.
This human-approved boundary uses a deliberate advisory exception while the
planned receipt mechanism is unavailable; independent observation/comparison
remain separate from implementation and are not acceptance.

### Viewer polish increment

Use renderer-owned navigation metadata, recorded when inline link spans and
heading prefixes are actually emitted, rather than searching label text (which
confuses duplicates, prose, code and wrapped labels). Preserve metadata on reflow.
The later production increment extends that metadata to table cells and bare
URLs, removing the old text fallback. Share parser options between link
extraction and rendering. Decode UTF-8 percent escapes before path/fragment
resolution; do not invoke a shell.

Keep an indexed open-file list with per-tab viewport offsets and a history of
visited tabs. Render tabs in opening order in the clipped top row, marking
the active tab without reordering. Search/link status and errors belong in the footer.
Navigation failures retain the current document and expose a diagnostic.
Editing hands the complete file to the existing editor after terminal release.

Theme-file loading is explicit, bounded to 65,536 bytes using a limited reader,
strictly deserialized with serde_json, and applied once before CLI dispatch.
Partial palette overrides inherit the resolved base theme. Color depth and
plain-output settings remain independent.

A line-buffering quote writer decorates all quote content (including code,
tables and nested quotes), reasserts background after SGR resets, and pads only
to the available width. The parser reduces inner layout budgets by the border
width. Ordered lists keep one next-item counter per open list.
No new dependencies are required. Test malformed themes, duplicate/wrapped
links, same/cross-file fragments, tabs/back/resize, nested numbering, quote
geometry/spacing and existing showcase styling before production changes.

This increment records a deliberate per-bundle advisory exception while the
planned advisory mechanism is unavailable. This is not a human acceptance,
Kvist receipt or compliance claim.

Scoped audit: theme values are typed colors rather than terminal sequences;
JSON reads are bounded, percent decoding fails explicitly, viewport diagnostics
cross the inert-text boundary and external actions require explicit user input
with separate subprocess arguments. Malformed theme keys exposed a CLI
diagnostic-control gap; theme error output now escapes controls, with failing
root/palette-key inputs covered by regressions. This audit is not an
independent compliance finding.

The separate source-blind comparison found an ambiguity in the heading-slug
contract's punctuation rule. Explicit human arbitration selected preserving
underscores; the contract now lists that exception, matching the existing
documented slug implementation. The original finding and evidence reservations
remain in `IMPL.md`; no automated acceptance is implied.

- I/O errors are reported clearly to the user
- Unsupported math/diagram syntax retains source with diagnostics
- Terminal resize events are handled; content reflows
- Empty or tiny viewports do not panic

## Security and resource design

- Read-only access to session files (no writes without explicit edit mode)
- No network access required
- Paths validated; symlinks not followed when reading session artifacts
- Historical proposal, not an implemented guarantee: "Memory usage proportional
  to document size (streaming for large files)"

The file-input comparison D-01 identified that streaming proposal as inconsistent
with whole-file buffering and the current bounded-input design. It is retained
here as historical, unimplemented intent rather than a capability claim.
Human disposition of the conservative input policy and future streaming
direction remains open; this clarification does not claim acceptance.

## Verification strategy

- Integration tests exercise the full viewer pipeline
- Unit tests for individual rendering functions
- Manual testing of interactive navigation and search
- Compatibility testing with various Markdown dialects and extensions

## Accepted text-only rendering design (2026-10-21)

### P2 script profile

Script spans use literal, unescaped `^` or single `~` delimiters within one
parser text payload. They do not cross whitespace, code, HTML, math or Markdown
formatting boundaries. Native Markdown strikethrough retains precedence.
Surrounding emphasis, heading, link and alert styles are retained. The extension
uses parser source offsets rather than rewriting Markdown, so decoded entity
delimiters and backslash-escaped delimiters cannot acquire script semantics.
Nested script/markup syntax is not interpreted inside a span.
Touching completed spans such as `^2^^3^` are separate projections; empty or
unmatched doubled delimiters stay literal. A decoded entity or escaped token
inside a prospective span breaks its literal source run rather than being
reinterpreted as syntax.

The exact superscript map is digits `0-9`, `+ - = ( )`, `i` and `n`.
The exact subscript map is digits `0-9`, `+ - = ( )`, and
`a e h i j k l m n o p r s t u v x`. Mapping is whole-span: if any character is
unmapped, the complete content becomes `^(content)` or `_(content)`. No glyph
substitution is performed in code, escaped notation, or unmatched delimiters.
This small, stable map is also the script primitive for P3 mathematics.

### P3 mathematical text profile

Enable parser math events and retain their complete source-offset slices,
including dollar delimiters, for source mode and whole-expression fallback.
The private recursive-descent projector accepts literal Unicode, grouped
expressions, one subscript and one superscript per atom, grouped `\frac`
arguments, grouped `\sqrt`, literal `\text`, and matching `\left`/`\right`
parentheses, brackets, escaped braces, bars and angle brackets. Fractions are
always `(numerator)/(denominator)` and roots `√(radicand)`; ordinary groups keep
parentheses, while script groups use the P2 whole-span mapper. Unsupported
commands, duplicate scripts, mismatched groups/delimiters, missing arguments
and executable/macro/include syntax reject the entire projection. There is
no partial conversion, macro expansion, external resource loading or execution.
Fractions also receive whole-expression parentheses when embedded in another
expression or used as a script base: `x/\frac{a}{b}` is `x/((a)/(b))`, not
`x/(a)/(b)`. The P3 independent audit identified this binding case; a failing
regression preceded its correction.

Greek commands cover the conventional lower-case alphabet, common variants,
and conventional upper-case forms. Operator commands cover the documented
arithmetic, comparison, set/logic, arrows, calculus and named-function table in
`docs/USAGE.md`. Escaped TeX punctuation is literal. Small spacing commands
produce a space (negative spacing is omitted); sizing is not graphical.
`\text` permits plain characters, balanced literal braces and escaped
punctuation, not nested commands.

For inline currency conservatism, signed numeric amounts with decimal/thousands
separators remain source. A numeric-leading inline span whose closing dollar
immediately precedes another digit and has no TeX/script/group syntax also
remains source, preserving ambiguous prose such as `$5 and $10`. Explicit
display math is not currency. Escaped/unmatched dollars and code stay literal.

The projector rejects input above 16,384 UTF-8 bytes, structural nesting above
32 and projected output above 65,536 UTF-8 bytes. These are inclusive ceilings.
Resource fallback emits at most 16,384 source bytes at a UTF-8 boundary plus
an explicit omitted-byte count and diagnostic. Other fallback preserves the
original source and delimiters, subject to ordinary table whitespace rules and
inert-control representation. Source mode skips projection entirely.
CRLF is normalized to LF before parsing; standalone carriage returns are
escaped visibly, including decoded entities and code/fallback content.
Inline formulas and fallbacks remain atomic text spans; display formulas are
separate unwrapped blocks. Existing horizontal overflow handles long formulas.

### P4 backend admission

Admit pinned Merman 0.9.1 with defaults disabled and only `ascii` plus the
flowchart, sequence, state, class and ER selectors. Mmdflux 2.6.1 lacks the
required ER family; retaining five families justifies Merman's larger graph.
The active graph excludes graphical renderer crates. Approved `kvist vendor`
provisioning updated the canonical locked snapshot; its documented host
acquisition fallback was used when sandbox toolchain discovery failed.
This added 73 locked crates, about 47 MB unpacked. Merman/Dugong are
MIT OR Apache-2.0 with retained translated-source notices; transitive
MPL-2.0, Unicode, BSD and Zlib notices remain distribution obligations.

The adapter rejects rather than pretending success when the scene requires a
non-spatial relation summary. Five-family fixtures admit labelled spatial
diagrams, branches/cycles, groups/control frames, notes, class members and ER
attributes/keys. Browser interactions, directives/front matter, accessibility
declarations, rich HTML, endpoint-product shorthand and unsupported model
features are gated explicitly. Some composite-state crossings/cycles,
namespace-plus-external-note combinations and relation topologies remain
unsupported. Styling and elaborate shapes are text approximations. The exact
profile is documented in `docs/USAGE.md`.

Enforced rejection ceilings: 65,536 source bytes, 8,192 lexical units, 256
nodes/participants/containers, 512 edges/messages, nesting 32, 32,768 model
items, 262,144 model-text bytes, 262,144 grid/document cells, 2,097,152 output
bytes, 256 bytes per grapheme and 2,000,000 deterministic layout-work units.
Model admission checks run after parsing, not before model allocation.
Each call uses a fresh child control with a 500 ms cooperative deadline;
earlier ancestor deadlines also apply. This is checkpoint cancellation, not
hard preemption or a memory sandbox. Reaching a ceiling is an explicit
resource failure, not a promise that every smaller scene succeeds. Source
fallback is bounded to 65,536 bytes with an explicit omitted-byte count.
Source mode bypasses the engine.

The public `RenderCancellation` wrapper lets hosts retain a clone and cancel
controlled document rendering. It does not expose backend types. Cancellation
and deadlines propagate as `Interrupted`; writer errors preserve their original
kind. Unsupported/invalid/resource engine errors get distinct visible source
fallbacks. Diagram output crosses the inert-text boundary again, since labels
may have been decoded by the engine. Completed rows never viewport-wrap or
rotate; width does not alter canonical diagram geometry.

Admission measurements (optimized test profile, 100 repetitions per family):
flowchart 0.847 ms, sequence 0.234 ms, state 0.782 ms, class 0.243 ms and ER
0.450 ms average; cold flowchart 6.85 ms; corpus process peak RSS 17,944 KiB.
A 16-node chain took 8.07 ms; 64-256-node chains explicitly exhausted work in
76-81 ms. These measured fixtures are not universal latency guarantees.

The human accepted `docs/TEXT_RENDERING_PLAN.md` and requested separate milestone
commits. P0 records intent; P1-P5 cover themes/output, scripts, math, native
diagrams, and integration respectively. No graphical intermediate is allowed.
The renderer remains separate from terminal ownership and CLI policy.

Typed render settings carry appearance/palette, color depth, plain-output mode,
math mode, Mermaid mode and diagram charset. Standalone CLI policy is resolved
once from flags/environment/TTY state. Embedders pass resolved settings.
Automatic theme detection uses side-effect-free environment hints; the unsafe
blocking OSC-query approach is removed rather than moved into the render loop.
Immutable syntax/theme assets use a shared once-initialized store.

The existing styled-text pipeline and display-column viewport are retained.
Only renderer-owned SGR may be emitted, with output encoding responsible for
capability conversion and escape-free plain output. Authored controls are made
inert before they can become styling. Semantic palette roles cover headings,
links, alerts, muted text, math, code/table backgrounds, search and chrome.
Colors must not affect the text or measured width.

Scripts use a shared all-or-nothing character mapping. A parser-aware text
event extension handles adjacent delimiters without touching code, escapes or
strikethrough. Unsupported spans retain explicit notation. Math uses the same
script mapping and a small bounded grammar; preserve the complete original
source range for fallback, rather than reconstructing its delimiters.
Initial limits are 16 KiB per formula, nesting depth 32 and 64 KiB projected
text. No macro expansion, file inclusion or executable TeX is supported.

The Mermaid backend is selected from pinned Merman 0.9.1 and mmdflux 2.6.1
using the accepted corpus criteria before integration. Prefer Merman's text-only
feature set because ER is required. One backend only; record actual dependency
availability, licenses, feature graph and corpus measurements before admission.
Initial adapter ceilings are 64 KiB source, 256 nodes/participants, 512
edges/messages, nesting 32, 262,144 grid cells and 2 MiB output. The selected
engine's deterministic work budget and cancellation control must also be set;
numeric backend-specific values are recorded with its evaluation.
Do not admit an unbounded layout algorithm by checking only its final output.

Source-mode bypasses conversion. Unsupported/malformed content gets a visible
diagnostic and its unmodified source (except inert control presentation).
Resource limits have distinct diagnostics; oversized source fallback is bounded
and explicitly reports omitted content. Cancellation/writer failures propagate.
Diagram rows stay unwrapped; viewport clipping and horizontal scrolling expose
overflow. Width/charset changes invalidate diagram layout. No persistent cache
is introduced; any measured need for caching requires a bounded in-memory key
including source, width, charset, palette and engine/options identity.

Each milestone uses failing tests before production code, followed by scoped
security inspection and independent source-derived/source-blind evidence.
Finite test evidence does not establish universal compliance or human acceptance.

P0 advisory input was requested in a separate read-only intent context. Its
nonbinding findings are acknowledged: freeze exact script/math/diagram profiles
before their tests/code; source fidelity excludes inert-control escaping and
explicit bounded omission; backend admission requires measured budgets and
provisioned dependencies; asset reuse and absence of render-time effects need
explicit tests. These dispositions are not a Kvist receipt.

The installed `kvist component accept blad` cannot record revisions while the
project is invalid because of an existing Skott queue timestamp inconsistency.
The accepted human scope is retained here; existing revision digests are not
forged or silently replaced. Historical Blad multiline queue contexts use
chomped block scalars so they satisfy the installed queue validator.