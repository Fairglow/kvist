# Component Implementation Record

Date: 2026-10-12

## Scope and evidence boundary

This is an independent, clean-slate record of **current layout, rendering, and
viewer-path observations**, not a whole-component inventory or compliance
determination. It does not certify safety, intent satisfaction, or terminal
behavior beyond the evidence described below.

Source evidence was limited to `src/inline.rs`, `src/terminal_text.rs`,
`src/viewport.rs`, `src/markdown.rs`, `src/markdown_tests.rs`, `src/display.rs`,
`src/lib.rs`, `tests/layout.rs`, and `tests/viewer.rs`. Package/workspace
`Cargo.toml` and root `Cargo.lock` were consulted only for build identity. No
intent documents, previous implementation record, review reports, Git
information, peer implementations, or previous documenter observations were
consulted. The root document referenced by one viewer test was left opaque:
the test used it as a fixture, but its contents were not independently read.

Build identity: `blad` 0.3.0, Rust edition 2024, manifest minimum Rust 1.95.
Relevant locked dependencies include pulldown-cmark 0.13.4, syntect 5.3.0,
crossterm 0.29.0, unicode-segmentation 1.13.3, and unicode-width 0.2.2.
The viewer test harness uses nix 0.31.3 and vte 0.14.1.

## Inline layout and Markdown rendering

`markdown::render_markdown` delegates to `render_markdown_width` with no width
limit. Both take a caller-supplied writer and return an I/O result. The
Markdown and display modules are public; inline, terminal-text, and viewport
helpers are private library modules.

The inline writer buffers a complete word across parser events and style
changes. Its text buffer carries byte-range style spans; neighboring spans
with equal styles merge. Thus emphasis boundaries do not create spaces or
independent wrap decisions: `ab**cd**ef` is measured as `abcdef`, while its
styled slices are retained when written.

Ordinary text treats ASCII space, tab, carriage return, and newline as word
separators. Each separator flushes the preceding word and sets a pending
space. Repeated separators collapse to one space between words; leading and
trailing separators do not emit spaces. Soft breaks use the same pending-space
mechanism. Hard breaks finish the word, discard pending whitespace, and emit
a newline. Other whitespace characters are not delimiters in this path.

With a supplied width, the inline budget is at least one column. Before a word
is emitted, existing text, the pending one-column separator, and the complete
word width participate in the fit decision. A word that does not fit after
existing text moves to a new line. Oversized non-atomic words are then split
at extended grapheme boundaries using Unicode display width. Span output
follows those slices without independently wrapping each style. Wrapping does
not produce an initial empty line merely because a word exceeds the budget.

Inline code is appended without whitespace splitting, uses reverse video,
and marks the entire currently buffered word atomic. This includes adjacent
ordinary text when there is no separating whitespace. Atomic words may move
to the next line but are not subdivided to satisfy the budget. Fenced and
indented code block lines also bypass prose wrapping; code whitespace and
long lines remain available to the viewport's horizontal clipping.

Heading hash prefixes, list bullet prefixes, and GFM alert labels pass through
the inline writer rather than occupying an unchecked fixed-width output
prefix. Heading hashes can themselves wrap at very small widths. Lists
emit their initial nesting spaces separately, capped at width minus one,
then buffer the colored `• ` prefix. Continuation indentation is
`2 * list_level + 2`, likewise capped at width minus one; hard breaks retain
that indentation. Paragraph-start handling uses the current prefix column to
avoid inserting a blank line between a prefix and its content. Horizontal
rules use the supplied width, clamped to one, or 80 without a supplied width.

The executed prefix test checks every width **1 through 30** for a combined
sixth-level heading, paragraph, nested list, and NOTE alert. Every resulting
line in that ASCII-text fixture is within its budget. This is not a universal
width guarantee for atomic code, tables, or wider-than-budget graphemes.

Code highlighting selects a syntax with plain-text fallback and writes ANSI
foreground/background/style sequences. Highlighting failure for a line falls
back to the original line. Tables use a separate cell-width/wrapping path,
not the inline writer. The Mermaid branch attempts an external `mmdc`
subprocess, returns a placeholder on successful execution, and otherwise
renders a labeled source fallback. That branch was not exercised here.

## Terminal text normalization and clipping

`TerminalLine` separates recognized ANSI SGR sequences from text, recording
them at text byte offsets. Recognition accepts `ESC [` followed by digits,
semicolons, or colons and a terminating `m`; those sequences have no measured
text width. Other control characters, including an escape starting a
non-SGR sequence, become visible lowercase `\u{hex}` text.

Tabs expand to the next fixed **eight-column stop**, measured from the start
of the logical rendered line. Preceding visible text is measured with Unicode
width, including any textual control escapes; recognized SGR sequences do not
advance the tab position. Tab expansion occurs before horizontal clipping,
so changing the viewport offset does not change tab stops.

Clipping takes a start column and column budget, not byte or scalar offsets.
Only complete extended graphemes within the requested interval are emitted.
If a wide grapheme straddles either edge, spaces fill its overlapping cells
instead of emitting a partial grapheme. Combining marks and joined emoji
therefore remain grouped, including when SGR sequences occur inside a
grapheme. Earlier style sequences are replayed to carry style into the
selected interval, and nonzero-width clips end with an ANSI reset. A
zero-width clip returns an empty string.

## Document sizing and frame painting

`Document::render` reserves the terminal's final column before assigning
content width. Without numbering, the available budget is
`columns.saturating_sub(1)`. With numbering, a gutter starts at two columns;
content is rendered, the required gutter is computed as the decimal digit
count of the rendered line count plus one, and rendering repeats whenever
that requirement grows. The gutter only increases during this convergence.
A supplied width limit further caps the remaining content budget.

The resulting Markdown output is split into rendered lines and normalized
through `TerminalLine`. Numbers refer to these rendered lines, not source
lines. The document stores the maximum normalized line width across the
entire document. Its horizontal bound is
`maximum_line_width.saturating_sub(content_width)`, rather than a bound
derived from the currently visible rows.

Painting clears the screen and uses explicit cursor positions: header at
`(0, 0)`, each content row at `(0, row)`, footer on the final row when there
are at least two rows, and scrollbar cells in the final column. Frame output
does not rely on newline advancement. Header and footer are normalized and
clipped to the terminal columns; content is clipped at the shared horizontal
offset and content budget. Number prefixes are clipped before content is
written. A matching rendered line receives a highlight SGR prefix.

Content height is `rows.saturating_sub(2)`. A one-row frame contains only its
header; a two-row frame has header and footer without content. Zero columns
or rows cause painting to stop after clearing and flushing. When the
document exceeds a positive content height, a proportional scrollbar is
positioned explicitly. Writer failures propagate through the paint result.

## Viewer lifecycle, resize, supplied width, and editor path

The inspected display entry points enter the interactive viewer only when
both stdin and stdout are terminals. `view_markdown_width` passes its supplied
width as a persistent cap to the viewer, with numbering disabled. Its
noninteractive branch renders into a buffer at the supplied width and writes
that buffer directly to stdout, reporting rendering/output errors. The
ordinary file-view path instead delegates to the unbounded Markdown dump
when noninteractive. CLI argument parsing was outside this observation.

The viewer queries terminal size and renders its initial document before
entering raw mode. It clamps vertical offset to the rendered document length
and horizontal offset to the document-wide bound before each frame. Right
movement uses that global bound, so encountering a short line does not reset
the offset used for a longer line. Home/top commands reset both offsets.

A resize event updates columns, rows, and content height; rerenders the
original supplied source with the retained numbering and width-limit
settings; recomputes search matches against the new rendered lines; and
clamps the selected match index. Offset bounds are reapplied on the next
frame. The tests exercise a wider resize and a quittable 1-column, 1-row
viewer; they do not establish behavior for every terminal geometry.

`TerminalGuard` tracks raw mode, alternate-screen entry, and cursor hiding.
Raw mode is marked active after successful enablement; alternate-screen and
hidden-cursor flags are set before their respective output commands, allowing
cleanup attempts after a partial command failure. Restoration attempts cursor
showing, alternate-screen exit, and raw-mode disablement even if an earlier
step fails, retaining the first error and clearing each successful flag.
Normal quit explicitly restores the terminal; drop attempts remaining
restoration and logs failure. These are source-observed cleanup paths, not
fault-injection or abrupt-process-termination guarantees.

For a supplied file path, `e` restores the terminal before launching the
editor. `EDITOR` is split on whitespace into a program and arguments,
defaulting to `nvim` when unavailable; an empty value is reported. The file
path is passed as a separate process argument without shell interpolation.
Only a successful editor exit triggers a UTF-8 file reread and a new viewer
with the same numbering and width limit. Spawn, exit, reread, and subsequent
viewer errors are reported. No editor process was launched by these tests.

## Residual limitations visible in this scope

- Atomic buffered words and code blocks can exceed the rendering budget.
  A single grapheme wider than the budget is also emitted intact by inline
  wrapping. The viewport clips these cases; direct rendering is not bounded
  in the same way.
- The document can assign a zero content budget even though the inline
  writer clamps its own width to one. Tiny frames can therefore hide rendered
  content rather than eliminate it during rendering.
- Table natural widths and padding count Unicode scalars, while wrapping
  also uses byte lengths. Cell words are not split, minimum column widths can
  overflow, and allocation reserves two separator columns although output
  writes three-character ` | ` separators. Table rendering is not the same
  Unicode/budget discipline as inline prose.
- Control normalization and clipping belong to the viewport path. Direct
  Markdown output does not pass through `TerminalLine`. Recognized SGR input
  is retained rather than restricted to renderer-generated styles.
- Search operates on individual normalized rendered lines. The inspected
  calls to `scroll_to_match` discard its returned offset, so typing a query
  or using next/previous match does not apply the calculated vertical jump.
  This was observed in source, not tested by the targeted runs.
- `EDITOR` whitespace splitting does not implement shell quoting. Repeated
  editing reenters the viewer through nested calls. The unbounded dump helper
  uses `unwrap` for rendering and UTF-8 conversion rather than returning its
  errors to the caller.
- The PTY screen model counts each printed character as one cell and does
  not model SGR appearance. Its passing ASCII frame tests do not independently
  verify real-terminal Unicode widths, match-color persistence, or cleanup
  under injected terminal failures.

## Independently executed verification

The following commands were run offline with the existing lockfile. `TMPDIR`
was set to the repository working directory for test-created fixture files;
no system temporary directory was used.

| Command | Actual result |
| --- | --- |
| `cargo test -p blad --offline --locked --test layout --test viewer` | Layout: **8 passed**, 0 failed, 0 ignored, 0 measured, 0 filtered out. Viewer: **4 passed**, 0 failed, 0 ignored, 0 measured, 0 filtered out. |
| `cargo test -p blad --offline --locked --lib terminal_text::tests` | **4 passed**, 0 failed, 0 ignored, 0 measured, **24 filtered out**. |
| `cargo test -p blad --offline --locked --lib viewport::tests` | **2 passed**, 0 failed, 0 ignored, 0 measured, **26 filtered out**. |

All three commands exited successfully: **18 executed tests passed**.

The layout run covers cross-style adjacency and spacing, separator-budget
boundaries, hard/paragraph breaks, long words, Unicode graphemes, preserved
inline/block code whitespace, partial Markdown, tiny supplied widths, and
the 1..30 prefix fixture. The viewer run covers first-line placement without
screen scrolling using the opaque fixture, heading/prose/gutter sharing,
code clipping with a horizontal offset retained across short lines, resize,
and the tiny quittable frame. Terminal-text tests cover style-aware clipping,
grapheme edges, embedded styles, escaped controls, and fixed tab stops.
Viewport tests cover gutter convergence, a width cap, and writer-error
propagation.

`src/markdown_tests.rs` contains **22 additional tests inspected as source
only**. They were not executed by the filtered library commands and are not
counted as passing evidence. In particular, the ambient external Mermaid
test was not run. Editor invocation, supplied-width display routing, search
jumps, and terminal-guard failure recovery remain source observations rather
than independently executed assertions in this record.
