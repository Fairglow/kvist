<!-- kvist-implementation-record-version: 1 -->

# Component Implementation Record

Date: 2026-10-12

## Text-only integration observation - 2026-10-21

An independent clean-slate observer inspected final application source/tests
without intent, queue, prior implementation records, reviews, chat, Git history
or peer implementations. Table-solver and dependency internals were outside
its scope. This section retains its source-derived report, not an author-side
compliance certification:

- Typed settings and semantic roles cover content and viewer chrome.
  Color reduction/plain encoding is streaming; monochrome may retain emphasis,
  while dump removes styling. File/session/show/stream and embedding routes use
  shared rendering. Export deliberately retains raw Markdown, including any
  authored controls, rather than rendering or sanitizing source.
- Syntax/theme assets are process-wide immutable caches; highlighting state
  remains local to a code block. Repeated/cloned-renderer tests compare asset
  identities. No diagram cache or application-owned render subprocess, worker,
  temporary file, network operation or graphical intermediate was observed.
  Explicit editor execution and interactive terminal ownership are separate.
- Parser-offset scripts protect code, escapes, entity/formatting boundaries
  and use whole-span mapping/fallback. Math uses the documented finite command
  vocabulary, explicit grouping and per-formula input/depth/output checks.
  Fraction-operand binding and lone-CR fixes from the earlier audit were
  independently reconfirmed in source and passing coverage.
- Five Mermaid families use native spatial text admission, separate charset
  selection and canonical geometry rather than viewport reflow. Limits and
  cooperative cancellation remain explicit, not hard preemption or a memory
  sandbox. The earlier bidi, decoded-label, source and diagnostic boundary
  remediations were independently reconfirmed.
- Shared inert-source processing covers both authored input and decoded text.
  C0/C1 and bidi controls cannot become document-triggered terminal protocols;
  CRLF normalizes and lone CR becomes a visible escape.
- Search overlays are reapplied after embedded style transitions/resets.
  Configured appearance/projection survives reflow and repeated editor
  re-entry. An outer viewer loop releases previous documents/guards instead
  of growing recursive frames. Editor launch/status/reread errors propagate,
  and restoration precedes editor launch.
- CLI/PTY tests cover explicit policy, color depths, environment hints,
  `NO_COLOR`, `TERM=dumb`, plain precedence, absent runtime tools, untouched
  non-TTY stdin cursor, preserved exports, resize, clipping, editing and
  error restoration. PTY geometry uses ASCII fixtures; it is not exhaustive
  physical-terminal/font validation.

The observer independently executed the full Blad suite (**153 passed**,
two optional characterization tests ignored), formatter, strict all-target
Clippy and Skott consumer compilation successfully. It did not execute timing
tests, so the measurements in DESIGN.md are parent-run evidence, not the
observer's own characterization.

The audit found one actionable integration defect: code-panel padding measured
tabs differently from viewport expansion. At 20 columns, fitting `tab + X`
could acquire seven padding-only scroll columns. Parent regressions reproduced
this before correction: the two-tab unit fixture had `max_x = 13` instead
of zero, and PTY Right movement shifted fitting text.

A separate narrow clean-source observer inspected only tab measurement,
normalization, the two code-padding paths and their regressions. It confirmed
both paths now share eight-column stops with `TerminalLine`, with styling
excluded from measured width and padding restricted to remaining columns.
It independently ran the library and PTY tab regressions successfully across
text/Rust syntax, gutter and plain/color combinations. No remaining defect
was observed in that scope. Unicode equivalence was source-derived for that
observer; parent-executed helper assertions also cover CJK, combining marks
and joined emoji.

Final parent verification after that remediation: **155 passed**, two optional
characterization tests ignored; formatter, strict all-target Clippy, Skott
consumer compilation and whitespace-diff checks passed.

Residual limits and pre-existing behavior remain explicit:

- Whole-document work/memory is not capped. Sanitized source, buffered words,
  code blocks, tables and viewport documents can grow; many individually
  admitted formulas/diagrams accumulate work. Blocking writers, highlighting,
  parsing and first asset initialization are not hard interruptible.
- Existing ordered-list bullet simplification, omitted checkbox state/URLs,
  ignored raw HTML and flattened table-cell styling do not become complete
  CommonMark/GFM support through these milestones.
- Legacy search-navigation calls discard the calculated jump result. This
  pre-existing finding is retained, not silently fixed as rendering integration.
- Raw-source exports intentionally bypass the terminal trust boundary.
  The selected dependency internals were not comprehensively audited.
- Gates ran on the active toolchain, not a separate minimum-Rust-1.95 build.

A separate source-blind P5 reviewer compared only current local requirements,
contract, design and these dated source/evidence observations, not source,
test code, manifests, peers or history. It supported R-033 through R-040 and
related R-017/R-018 consumer semantics without a demonstrated outstanding
in-scope contradiction. It distinguished independent 153-test execution and
the narrow remediation checks from parent-run 155-test verification/timings.
The residual limits above remain limitations, not certification. Human plan
acceptance is separate; project-tool revision recording remains blocked by
the unrelated Skott queue timestamp, and no receipt is claimed.

## Native text diagrams observation - 2026-10-21

An independent clean-slate observer inspected only the adapter, integration,
diagram tests and selected manifest/dependency graph. It excluded intent,
queues, implementation records, reviews/chat/history and peer code. Its
source-derived findings:

- Five model families produce tested spatial ASCII/Unicode boxes/connectors:
  flowchart, sequence, state, class and ER. Tests cover labels, directions,
  groups/frames, notes, class members and ER attributes. This establishes a
  tested subset, not universal Mermaid semantic fidelity.
- Width is deliberately ignored, preserving canonical unwrapped/unrotated rows;
  one measured ER row was 170 columns wide. Authored Unicode remains Unicode.
- Directives, rich HTML, endpoint products and selected interaction/metadata
  fields are gated. Engine reports must be diagrammatic, non-fallback and omit
  standalone `relations:` summaries. This rejects known summaries but is not a
  proof that every possible accepted detail survives. Styling is ignored.
- Preflight limits source to 65,536 bytes, lexical units to 8,192 and nesting
  to 32. **After parsing**, model admission limits nodes/participants to 256,
  edges/messages to 512, items to 32,768 and text to 262,144 bytes.
- Renderer policy requests 262,144 grid/document cells, 2,097,152 encoded-output
  bytes, 2,000,000 work units and 256 bytes/grapheme. Tests exercise lowered
  work/grid/output limits across all families and exact encoded-output
  boundaries; dependency internals were not audited.
- Each diagram uses a child 500 ms cooperative deadline. Cancellation/deadline
  errors become `Interrupted`, never source-fallback success. This is not hard
  preemption or a wall-clock/memory bound; parsing/preprocessing/writer work can
  occur between checkpoints.
- Unsupported/invalid/resource diagnostics retain source, capped at 65,536
  bytes on a UTF-8 boundary with explicit omitted bytes. Source mode bypasses
  the cap. Writer errors propagate without rolling back existing output.
- Selected Merman 0.9.1 disables defaults and enables only ASCII plus five
  family selectors. HTML/CSS/URL/ICU parsing appears in the active graph;
  URL parsing alone is not networking. Optional graphical/editor/analysis
  packages in the lockfile are inactive. Adapter code contains no process,
  file, network or graphical-intermediate effects; this is not a complete
  dependency-source effects audit.

The observer independently ran seven end-to-end and 16 backend tests, plus the
opt-in corpus characterization. Repeated family means were approximately
0.24-0.88 ms; 64/128/256-node chains failed explicitly with work limits in
75-81 ms. It inspected the selected feature graph independently.

Its audit found that rejected/source diagrams could still emit bidi override/
isolate characters. A new end-to-end regression reproduced this before the
shared inert boundary was extended to U+202A-U+202E and U+2066-U+2069.
Additional encoded-label and malformed-diagnostic probes now pass without
terminal controls. Latest targeted evidence: nine end-to-end plus 16 backend
tests pass; strict all-target Clippy passes. Final P5 observation must include
this remediation.

Earlier full integration evidence: 137 tests passed, one opt-in timing test
ignored, and the Skott consumer check passed. Backend provisioning/admission
measurements and license obligations are recorded in DESIGN.md, not fabricated
as independent compliance evidence.

A separate source-blind P4 comparison found no proven mismatch for the bounded
R-017/R-037/R-038/R-039/R-040/C-004 scope. It supported five-family spatial
admission, independent geometry and documented limits, while retaining
independent P5 reconfirmation of remediation evidence and explicitly not
claiming universal fidelity or a dependency-internals audit.

## Mathematical text observation - 2026-10-21

An independent clean-slate source observer inspected math, shared scripts,
parser integration, inline writing and mathematical tests without intent,
queues, prior records/reviews, project documents, history or peer/backend code.
It independently executed 46 library and 12 mathematics tests successfully.
Its bounded source observation reported:

- Markdown enables math events alongside tables and prose scripts. Unicode
  projection is a recursive parser, not TeX execution. It accepts literal
  characters, nonempty braced groups, fractions, square roots, limited literal
  `\text`, matched `\left`/`\right` delimiters, escapes and spacing commands.
- Greek mappings distinguish epsilon/phi and their variants, plus theta, pi,
  rho and sigma variants. Upper-case Greek and fixed operator/function maps
  exist. Unknown commands, root indices, macros, comments/alignment and commands
  inside `\text` fall back visibly.
- Groups become parentheses, fractions `(numerator)/(denominator)` and roots
  `√(argument)`. Fraction and text bases receive parentheses before scripts.
  At most one subscript and one superscript attach to each atom, in source
  order; shared mapping is whole-span with explicit fallback.
- Projection accepts 16,384 input bytes, nesting through 32 and checked strings
  through 65,536 bytes. Loops consume input or terminate; no non-progress loop
  or unchecked recursive depth was identified.
- Failure discards partial projection and retains delimited source with
  distinct unsupported/invalid/resource diagnostics. Resource fallback truncates
  source above 16,384 bytes at a UTF-8 boundary with an omitted-byte count.
  Source/currency modes bypass projection and its limits, not document-wide
  allocation limits.
- Currency preserves signed digit/comma/period strings, with a broader
  numeric-leading rule when the closing dollar immediately precedes another
  digit. `$1+2$3` therefore also stays literal; display math bypasses this rule.
- Original offsets refer to sanitized Markdown. Inline presentations are
  atomic buffered spans; display presentations are unwrapped blocks; table
  presentations enter cell strings. Writer errors propagate when flushed.
  No external execution appeared in inspected math paths.

The audit identified ambiguity for `x/\frac{a}{b}` and requested a lone-CR
source/fallback probe. Added regressions failed before correction: fraction
operands now test as `x/((a)/(b))` and `((a)/(b))c`, and lone CR becomes visible
`\u{d}` while CRLF normalizes to LF, including source mode, entities and code.
These are test-observed remediations; the final clean-source P5 observation
must include them. The original grouping/Greek and source/limit regressions
were also failing before their corresponding implementation.

Latest parent verification: 112 tests passed across library, math, scripts,
render-options, integration, layout, table-rendering and PTY viewer suites;
scoped strict Clippy passed. The separate 16-test backend corpus also passed
in the earlier full suite, with its opt-in measurement test ignored by default.

A separate source-blind P3 comparison reported bounded support with no
demonstrated outstanding R-036/R-038/R-039 mismatch. It explicitly retained
independent reconfirmation of the two audit remediations, cancellation
propagation and wider integration coverage for P4/P5; passing parent tests
were not substituted for independent source observation.

## Script notation observation - 2026-10-21

A separate clean-slate source observer read only script, parser, inline and
script-test code, excluding intent, queues, previous records/reviews, history
and peers. It reported:

- Native superscript/subscript start/end events become one-byte `^`/`~` text
  delimiters using their source offsets. Adjacent text events merge only when
  their ranges are contiguous and each exactly matches the source.
- Recognition requires a nonempty, whitespace-free span with matching single
  delimiters. Escaping uses preceding-backslash parity. Scanning does not cross
  formatting, code, or decoded-text event boundaries.
- Exact mappings: super `0123456789+-=()in` to `⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁱⁿ`;
  sub `0123456789+-=()aehijklmnoprstuvx` to
  `₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓ`.
- Projection is whole-span: any unsupported character produces `^(content)` or
  `_(content)`, without partial substitution or added surrounding spaces.
- Inline/fenced code bypass projection. Source-mismatch guards prevent decoded
  escapes/entities from becoming script syntax. Separate surrounding entities
  do not disable adjacent scripts. Strikethrough remains a formatting boundary
  and can contain projected scripts.
- Ordinary projected text retains the current inline style. Table cells collect
  projected text. Wrapping measures projected Unicode text and retains style
  spans across grapheme-based splits.

The independent scoped audit found no confirmed panic, nontermination or
superlinear path. Cursors advance monotonically, map lookup is fixed-size and
additional merging/projection/word memory is linear; scripts have no separate
input-size limit. The observer independently ran six script tests and the
updated library script regression successfully.

Audit evidence gaps prompted additional cases for actual emitted bold/link
styling, interior entities/escapes, touching spans, the complete glyph maps and
a 200,000-character unmatched span. A touching-span regression failed before
the scanner was corrected: `^2^^3^ ~2~~3~` now tests as `²³ ₂₃`, while doubled
empty openings remain literal. These additions and the existing six script
tests pass. The local long-input case is a regression/progress check, not a
wall-clock complexity proof.

Parent verification after those additions: 87 tests passed across library,
scripts, render-options, layout and table rendering; scoped strict Clippy
passed. Earlier intent-pinned standalone dim/literal behavior was deliberately
replaced by the accepted whole-span projection test.

A separate source-blind P2 comparison reported bounded support and no proven
discrepancy for R-035/R-038/R-039. It retained explicit runtime evidence for
HTML/math boundaries, heading/alert styles, nesting and authored Unicode as
P5 integration gaps, rather than claiming whole-component certification.

## Appearance/output observation - 2026-10-21

Independent source-only contexts observed this milestone without intent,
queue, prior implementation records, reviews, chat history or Git history.
This is observed evidence, not implementer certification.

`theme.rs` exposes explicit themes, color depths, semantic palettes,
plain-output selection, and math/diagram settings through `RenderOptions`,
re-exported by `lib.rs`. Renderer defaults do not inspect environment
variables. `MarkdownRenderer` retains width, syntax-theme selection, and
rendering options independently; unknown explicit syntax-theme names return
`InvalidInput`.

Rendering passes through `Output`, which incrementally buffers ESC sequences,
rejects unexpected sequence syntax and sequences exceeding 64 bytes, converts
extended colors to the selected depth, and propagates writer/flush failures.
Plain output removes parsed SGR; `ColorDepth::None` removes colors but retains
other styling.

Code panels use theme/palette backgrounds and syntax-token foregrounds; table
output uses reverse-video headers and alternating backgrounds. Terminal
appearance omits built-in fixed panel backgrounds. Prose wrapping and viewport
clipping preserve grapheme boundaries.

File, session-show, and session-stream routes receive shared options. Viewer
resize and editor re-entry forward options, line-number selection, and width
limit. The CLI resolves environment hints without terminal queries.

The first independent audit found decoded-entity controls, raw listing
metadata and discarded editor errors. Regression tests reproduced the first
two failures before remediation. A narrower clean-source observer subsequently
reported:

- **Entities:** Decoded prose/table payloads cross `inert_source`, escaping
  BEL/DEL/ESC as visible `\u{…}`. Numeric entities therefore cannot inject
  SGR/cursor controls. CR/LF/tab remain. `Output` alone is not a BEL/DEL sanitizer.
- **Metadata:** Listing escapes id/model.
- **Errors:** Editor launch/status/reread/reopened-viewer errors propagate;
  CLI reports failure and exits 1. Void convenience wrappers instead print
  errors.
- **Evidence/limits:** Entity/list regressions cover these controls.
  Id-control/editor-failure paths lack runtime coverage here. Requested suites:
  **17 passed**.

Latest parent-executed evidence: 93 scoped tests passed (`--lib`,
`render_options`, `integration`, `layout`, `table_rendering`, `viewer`);
Skott's consumer check and scoped Clippy with warnings denied passed.
Syntax assets are process-reused; rendering still buffers code blocks,
tables, pending words and viewport documents. Repeated edits recursively enter
the viewer. Streaming remains a snapshot. Export preserves source Markdown.
The audit also recorded pre-existing discarded search-navigation offsets and
search indication being overwritten by content SGR resets; these are not
evidence of completed search improvements.

The separate source-blind P1 comparison reported no proven in-scope mismatch
and a bounded supported verdict, not component certification. It retained
evidence reservations for the full CLI precedence matrix, palette/chrome and
geometry coverage across every route, editor failure/ID controls, and executable
reuse/no-render-time-effects checks. P5 must close these integration-evidence
gaps; later conversion milestones are not P1 failures.

## Scoped text-derived table bounds observation - 2026-10-21

This supplement supersedes the table-bound observations dated 2026-10-20
below. Earlier observations remain as historical evidence, not the current
table-bound description.

### Evidence boundary

Clean-slate observation used `blad/src/table/{mod.rs,cell.rs,tests.rs}`,
table-related event handling and rendering in `blad/src/markdown.rs`,
`blad/src/terminal_text.rs`, `blad/src/lib.rs`,
`blad/tests/{table_rendering.rs,viewer.rs}`, and table-related assertions in
`blad/src/markdown_tests.rs`. No intent documents, queues, existing
implementation records, prior reviews, Git state/history, or peer
implementations were consulted. The `include_str!` architecture fixture
remained opaque; its referencing test was executed without following the
fixture. No files were edited. This is source/test observation, not compliance
certification.

### Measurement and bounds

Cells split on literal newlines, preserving empty and trailing explicit lines.
Each line passes through `TerminalLine`: recognized SGR sequences are removed
from plain text, tabs expand to eight-column stops, and other control characters
become visible escape notation. Cells then tokenize with `split_whitespace`,
collapsing whitespace between words to single spaces. Word widths use
`unicode_width`.

Wrapping greedily packs whole words with one intervening space; it never splits
a word. An empty explicit line produces one empty fragment. Measurement and
emitted wrapping share this fragment iterator.

Each cell's natural width is its widest normalized explicit line, not its
column's natural width. Starting at the longest-word width, a binary search
derives the cell floor using four display columns per occupied wrapped fragment.
Empty fragments do not count toward that aspect calculation. The cell's own
natural width is always admissible, even when unavoidable explicit breaks
prevent satisfying the four-column ratio.

Column floors and natural widths are respective maxima across their cells.
Neighboring cells' heights and empty explicit lines do not enlarge a cell's
aspect floor. However, empty fragments still count toward measured height,
height-cap feasibility, and rendered row height; they can therefore influence
selected widths through the global height search.

Ragged rows receive empty cells. Zero-width columns remain zero: there is no
synthetic one-column minimum. Separators between columns still consume three
columns.

### Selection and optimization scope

Without a viewport, or when natural widths fit, selection returns natural widths
without expansion. Otherwise, the content budget is viewport width minus three
columns per separator, using saturating subtraction. If column floors exhaust
or exceed that budget, selection returns those floors, allowing horizontal
overflow.

For the remaining case, an outer binary search selects the smallest feasible
maximum row height, bounded below by maximum explicit cell height and above by
maximum cell height at column floors. Each feasibility check independently
binary-searches the smallest width per column permitting every cell in that
column to meet the height cap.

Spare budget is distributed proportionally to remaining flexibility toward
column natural widths, using integer shares and largest fractional remainders;
ties favor earlier columns. The optimization is maximum row height when fitting,
otherwise minimum total width/overflow, not total table height, individual
row-height balance, or a separate aesthetic objective.

### Rendering, verification, and limits

Rendering right-pads cells to display width, pads shorter cells to their row's
maximum height, and joins columns with ` | `. It draws no borders, reverses the
header, alternates data-row tint, and propagates writer errors. Recognized
case-insensitive `<br>` variants and hard breaks preserve cell newlines; soft
breaks become spaces. Overflow is not clipped by `render_table`.

Commands ran sequentially with `TMPDIR="$PWD"` exported:

- `cargo test --offline --locked -p blad --lib table::` - **7 passed**,
  0 failed; 38 filtered out.
- `cargo test --offline --locked -p blad --test table_rendering --test viewer` -
  **18 table-rendering and 5 viewer tests passed**, 0 failed.

Evidence includes finite exhaustive width enumeration, Unicode and explicit-break
cases, deterministic spare allocation, and PTY overflow/resize behavior. It does
not establish exhaustive correctness for all text or physical-terminal Unicode
behavior. Full-library/Mermaid tests were not run by this observer.

## Scoped table solver observation - 2026-10-20

This supplement supersedes the earlier table-path observations below; other
historical observations retain their original scope and date.

### Evidence boundary

This independent observation uses `blad/src/table/{mod.rs,cell.rs,tests.rs}`,
table-related Markdown event handling and rendering, `terminal_text.rs`,
`lib.rs`, and table-related evidence in `table_rendering.rs`, `viewer.rs`, and
`markdown_tests.rs`. No intent documents, queues, existing implementation
records, prior reviews, Git history, or peer implementations informed the
observations. No source files were edited. This is not safety or compliance
certification.

### Measurement and wrapping

`Table::new` normalizes ragged rows to the maximum column count by inserting
empty cells. Each cell splits on literal newlines, preserving empty and trailing
lines. `TerminalLine` removes recognized SGR sequences from its plain
representation, expands tabs to eight-column stops, and converts other control
characters into visible `\u{...}` text.

Cells then split each explicit line using Unicode whitespace and retain owned
words measured with `unicode-width`. Repeated whitespace collapses to single
spaces; original tab spacing is not retained. Minimum width is the widest word,
and natural width is the widest reconstructed explicit line.

Wrapping greedily packs whole words without splitting them. Empty explicit
lines emit one fragment. `fits_height` uses the same fragment iterator as
output, stopping after at most `cap + 1` fragments; measurement and rendering
therefore share the wrapping mechanism.

Markdown table text replaces embedded newlines with spaces and removes
carriage returns. Soft breaks become spaces; hard breaks and recognized,
case-insensitive `<br>` forms become explicit newlines. Other inline HTML is
discarded. Inline-code events append their text without code-specific styling.

### Width allocation

The solver uses a common height cap across all cells, with every column
constrained to at least four display columns per capped line, including empty
or short columns. It reserves three columns per inter-column `" | "` separator.

Without a viewport, widths are the greater of each column's natural width and
four times the largest explicit cell height. A viewport that already
accommodates these widths returns them unchanged.

Otherwise, a binary search finds a feasible uniform width, establishing the
height-search upper bound. Increasing integer height caps are evaluated using
binary searches for each column's smallest feasible width. The first fitting
cap determines the result. Remaining budget is distributed toward natural
widths proportionally, using integer largest remainders and column-index
tie-breaking.

If no candidate fits, the solver retains the smallest total width encountered;
equal-width later candidates do not replace earlier ones. A rising
minimum-width lower bound stops further search. Narrow viewports may therefore
produce horizontal overflow rather than split words or relax the common aspect
floor.

### Rendering and resource characteristics

Cells are left-aligned, right-padded, and padded vertically to their row's
tallest cell. Markdown alignment metadata is ignored. There are no outer or
horizontal borders. Every header fragment receives reverse video; alternating
body rows receive background tint. Styling wraps the complete rendered line
and does not enter width calculations. Writer failures propagate as I/O errors.

Searches and fragment iteration have finite, input-derived bounds, not
viewport-sized iteration. However, repeated feasibility scans can revisit words
across columns and height caps. The path buffers table strings, normalized word
storage, and wrapped output for each row, and allocates padding strings. No
explicit input, allocation, runtime, or cancellation limit appears in this
scope.

### Executed verification

- `cargo test --offline --locked -p blad --lib table::` - exit 0;
  **5 passed**, 38 filtered out.
- `cargo test --offline --locked -p blad --test table_rendering --test viewer` -
  exit 0; **17 table-rendering tests and 5 viewer tests passed**.

These cover a small exhaustive allocation oracle, wrapping consistency, Unicode
examples, explicit breaks, overflow, large input, writer errors, and PTY
horizontal navigation/resizing. Full library/Mermaid tests were not run by this
observer.

### Concrete concerns and uncertainty

Source SGR styling is discarded during cell normalization. The inspected
inline-code styling test checks for reverse video anywhere, which the header
already supplies; it does not establish code-cell styling. Unicode rendering
ultimately depends on terminal behavior matching library width estimates. The
viewer's test screen models characters rather than display-width-aware
graphemes. Finite loops and passing examples do not establish practical resource
bounds or exhaustive correctness.

## Scoped CI repair observation - 2026-10-14

Independent clean-slate `repair-observer` read only the enumerated source/test
files and build manifests, excluding intent, queues, prior records/reviews and
history. It executed no tests and made no compliance or acceptance claim.

`tests/viewer.rs` launches the built viewer through `setsid --ctty --wait`,
without a forced fork, attaching the PTY slave to stdin/stdout/stderr. Resize
signals address the retained child PID; quit checks successful termination,
and `Drop` kills/waits that child. Source-defined assertions cover terminal
width, scrolling, horizontal offsets, resize, tiny viewports and empty
documents. These tests require Linux PTYs, `setsid` and `stty`.

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

## Observed viewer polish increment (2026-10-22)

This supplement was derived by a separate clean-slate source documenter from
current Rust sources and test assertions, excluding intent, queues, prior
implementation records, reviews and Git history. That observer ran no tests.
It supersedes earlier navigation/search observations only within this scope;
it is not a compliance certification.

### Navigation and tabs

Normal Tab/Shift+Tab selects links cyclically and brings their recorded row into
view. Enter follows the selection or the first eligible link at/below the
viewport top; it is not a freely positioned text cursor. Optional `f` link mode
remains. Search applies the calculated case-insensitive match scroll offset.
Explicit Markdown link source offsets are carried through inline styles into
rendered row/column spans, including wrapped slices. Mouse hit testing accounts
for header, gutter and horizontal/vertical offsets and checks half-open ranges.
Selected links highlight their containing rows. Tests distinguish duplicate
linked labels from each other and from identical unlinked prose.

Heading IDs lowercase text/code labels, replace whitespace with hyphens, retain
Unicode alphanumerics plus hyphens/underscores, and discard other punctuation.
Duplicate generated IDs gain suffixes; explicit IDs are additional aliases.
Heading positions are recorded by the renderer. Empty fragments target row zero;
missing fragments produce visible status text.

File destinations split path and fragment before UTF-8 percent decoding.
Relative paths resolve against the current document. Canonical paths reuse
existing tabs, each retaining vertical/horizontal offsets. Alt+Left/Right cycles
tabs and pushes visit history; `b` pops it. Same-document anchor jumps do not
create history entries. URLs invoke `xdg-open` and expose failures as status.

Evidence: `src/display.rs:243-387,459-469,577-824,841-868`,
`src/inline.rs:19-23,185-235`, `src/render_writer.rs:105-120`,
`src/links.rs:246-294,332-344`, `src/markdown.rs:454-467`,
`tests/viewer.rs:538-780`.

### Appearance and rendering

Explicit JSON theme files are bounded to 65,536 bytes. Unknown fields/palette
keys and malformed colors fail. Colors accept ANSI indices 0-255 or six-digit
hex strings; alerts require five colors. Only backgrounds accept null.
Explicit dark/light/terminal choices override file appearance; auto uses file
appearance, then `COLORFGBG`. Palette overrides follow built-in defaults.
Color policy remains independent and dumps escape-free. Tests assert theme
retention through tabs, editing and resizing.

A writer-level quote decorator applies nested borders, fill and padding across
prose, code, tables and internal blanks; resets reassert quote fill. Narrow
widths reduce borders. Tests assert spacing, alert color and width bounds.

Syntax aliases include Bash/Python and unknown syntax falls back to plain text.
Default dark Ansi16 highlighting prefers Solarized dark; light prefers
InspiredGitHub. Width-bounded code blocks retain cap rows, padded backgrounds
and unwrapped content. Tests require multiple
foreground colors for Bash and Python at Ansi16.

Per-list counter stacks retain parent numbering after nested lists. Unordered
bullets cycle through three glyphs. Numeric footnote IDs become superscript
digits; other IDs use bracketed names consistently in references, definitions
and table cells.

Evidence: `src/theme_file.rs:8-88`, `src/main.rs:85-128`,
`src/theme.rs:147-165`, `src/render_writer.rs:45-99`,
`src/markdown.rs:284-363,514-581,672-714,927-1098`,
`tests/polish.rs`, `tests/viewer.rs:743-780`.

The Skott consumer test asserts three rows for a one-line fence: top cap,
source and bottom cap, each with a background-bearing span. It does not prove
token-color diversity or full-width fill.
Evidence: `../skott/src/tui/app.rs:4082-4106`.

### Evidence limits

The source observer obtained no runtime, real-browser or platform evidence.
Table cells do not carry recorded inline link spans; bare-URL extraction also
does not establish mouse hit-test spans. Keyboard row lookup retains a text
fallback for these cases. The pointer test does not combine gutters,
horizontal scrolling, wrapped labels and Unicode in one fixture. Tab history
is not same-document anchor history.

Implementation-session verification (not independently executed by the source
observer): the full workspace test command, workspace build, strict Blad/Skott
all-target Clippy and targeted polish/viewer tests completed successfully.
The scoped input audit additionally found that malformed theme keys could
reach CLI error output as controls; error diagnostics now escape controls,
with root/palette-key regressions and inclusive-size-bound coverage.

### Separate source-blind comparison and human disposition

A separate reviewer compared only R-041 through R-043 and related local intent
against the observed supplement and reported execution evidence, without
reading source or test implementations. Navigation, tab state, theme-file
semantics and rendering polish otherwise aligned with the increment.

The reviewer identified a concrete wording mismatch: observed heading slugs
preserved underscores, while the contract's punctuation rule did not explicitly
exempt them. The human chose **preserve underscores** when explicitly asked to
arbitrate; the contract now names that exception. This is a human disposition,
not an automatic change to hide a comparison finding.

Reservations remain: real-browser behavior and a combined Unicode, wrapping,
gutter and horizontal-scroll mouse fixture are not established by the cited
evidence. Table-cell/bare-URL mouse spans remain outside the documented
prose/heading pointer scope. The final post-diagnostic-fix workspace rerun was
still pending at comparison time. No historical whole-component compliance or
Kvist acceptance is claimed.

Final implementation-session rerun completed successfully after the diagnostic
correction: `cargo fmt --all -- --check`, `cargo test --workspace --offline
--locked --quiet` (1,588 passed, 14 pre-existing ignored across 89 successful
suites), `cargo build --workspace --offline --locked --quiet`, and `cargo clippy
-p blad -p skott --all-targets --offline --locked --quiet -- -D warnings`.
`git diff --check` also passed. These are execution results from the
implementation session, not tests independently executed by either reviewer.
