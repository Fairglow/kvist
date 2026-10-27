<!-- kvist-implementation-record-version: 1 -->

# Component Implementation Record

Date: 2026-10-12

## Scoped Mermaid deadline scheduling observation - 2026-10-27

Independent clean-slate `mermaid-deadline-observer` read only nextest policy,
the Mermaid adapter, the Markdown render helper/regression, and the backend
module import/cancellation assertions. It read no intent, queues, prior
records, reviews or Git history, ran no tests and made no compliance judgment.
Adjacent test-source search context was also visible, not intent context.
Its source-derived observations are:

- The default nextest override selects the Blad package and the exact
  Markdown Mermaid test plus two work/grid/output and exact-byte budget-test
  names. Each reserves `num-test-threads`. There is no binary restriction, so
  importing the adapter in `mermaid_backend` includes both budget-test copies.
- The production 500 ms child deadline starts before preflight, engine
  initialization/possible `OnceLock` waiting, parsing, model admission, layout
  and output. Checks are cooperative, not preemption; final projection/text
  checks follow the last checkpoint.
- Assertions cover five-family resource errors, successful reuse after
  rejection, exact encoded-byte acceptance versus one-byte-short rejection,
  spatial Markdown output, pre-cancelled/expired controls, repeated successful
  reuse and cancellation through a clone. The clone is cancelled and joined
  before rendering, not during an active render.
- Pool reservation does not isolate external host load, bound initialization
  waits or govern another runner/configuration. Deadline failures remain
  possible under those conditions. No whole-component certification follows.

### Attributed diagnosis and execution

The supplied failure logs show `deadline_exceeded` after 0.58-0.61 seconds,
not work/grid/output admission failures. Local repetition before this repair
did not reproduce those deadline failures. A configuration assertion failed
before the new override and passed afterward; no rendering source or test
assertions were changed.

Implementer execution used `--offline --locked`. Nextest event traces for the
original policy showed five overlapping starts among six selected instances.
With the override, 20 default-profile iterations at CPU-count concurrency
passed all 120 instances without overlapping starts. Twenty inherited-CI
iterations with four workers also passed 120 instances. Each iteration
included all five affected instances and one ordinary Markdown control.
This demonstrates scheduling, not immunity to unrelated host contention.

The full all-feature Blad nextest run passed 318 tests, with three existing
ignored characterizations. The libtest run passed 110 library, ten diagram
and sixteen backend tests, with one existing ignored characterization.
The previously repaired maerg inventory test passed again. Workspace format
and whitespace checks passed; no new whole-workspace result is claimed.

### Independent source-blind scheduling comparison

Separate `mermaid-deadline-comparator` compared the new observation and
attributed execution with local intent and the root contract, without source,
test source, nextest configuration, prior records or Git history. It identified
no concrete scoped mismatch in selecting five instances through three exact
names while retaining production cancellation/deadline/resource semantics and
all assertions. It retained that the original timing failures were not locally
reproduced and their host-contention cause was not measured. Initialization
waits, external load, alternate runners and active-render cancellation remain
outside the demonstrated scheduling evidence. This comparison does not imply
universal timing reliability, complete R-037--R-040 coverage, new workspace
verification or acceptance.

## Scoped table emphasis source observation - 2026-10-25

**Scope.** Derived from the specified Rust source and tests only. No files were changed or tests executed here. This is an implementation observation, not acceptance or whole-component compliance certification.

**Capture and normalization.** Markdown emphasis uses nesting counters. Table cells accumulate plain text separately from byte-indexed spans retaining only bold, italic, strikethrough, and link identity; foreground color, reverse, and dim are discarded. Adjacent equal styles coalesce. Text, code, projected math, and footnote markers inherit captured emphasis. Recognized table-cell `<br>` variants insert explicit newlines. Whitespace normalization collapses token separators to single spaces while preserving explicit empty lines.

Wrapping and alignment operate independently of style metadata. Cells retain whole whitespace-delimited words, including words spanning parser events. The integer width solver respects token/aspect floors and natural-width caps; tables may overflow narrow viewports. Alignment pads each wrapped fragment using display width.

**Projection.** A forward source cursor locates wrapped tokens in original cell text. Span intersections retain UTF-8 byte ranges for emission; grapheme-based projection separately calculates display-column ranges. Thus style changes inside a combining-character grapheme retain distinct byte slices while link coordinates encompass the whole grapheme. A normalized interword space receives the first source-whitespace character's captured style. Whitespace removed at wrapping boundaries is not emitted.

**Emission and navigation.** Cell emphasis emits selective SGR enable/disable pairs: 1/22, 3/23, and 9/29. It does not globally reset each span, preserving header reverse video and row backgrounds. Each visual header line is reversed; alternating body rows receive the configured table tint. Padding and separators remain outside cell emphasis.

Table inline code has neither dedicated reverse styling nor nonbreaking code-span atomicity; surrounding emphasis still applies. Table links retain navigation identity but receive no dedicated link foreground color because capture filters color out. Outside tables, inline code uses reverse and atomic wrapping, and ordinary links can receive the palette link color.

Link registration precedes cell text emission and includes the already-written prefix, alignment padding, and quote-border width. Same-identity spans on one row merge. Viewport clipping counts display columns, substitutes spaces for partially visible wide graphemes, and restores source SGR around search/selection overlays.

**Errors and boundaries.** Lost wrapping tokens, failed grapheme projection, and invalid span slices return `InvalidData`. Writer and flush errors propagate; output can already be partially written. Authored controls are made inert, including entity-decoded controls. The output adapter validates renderer controls, suppresses all SGR in plain mode, and filters/quantizes colors independently of emphasis.

Tables are buffered and normalized before row emission. Projection advances span cursors and uses grapheme-range lookups, but allocates intermediate vectors; link registration repeatedly measures buffered line prefixes. Width solving, projection, and table emission have no internal cancellation checkpoints. No hard latency or aggregate-memory bound follows from this implementation.

**Inspected assertions.** Tests check combined/nested emphasis, header/tint preservation, unstyled neighbors/separators, normalized spaces, event-split words, Unicode, wrapping and explicit breaks, unchanged plain geometry, colorless emphasis, inert controls, and propagated failures. A 2,000-span unbroken-word test checks content and bold counts, not timing.

The mixed-style PTY table test exercises centered/right alignment with/without line numbers: bold wide `界`, italic `x`, struck `y`; search overlay application/removal; exact six-column link-selection underline; narrow resize plus horizontal clipping retaining all three emphasis flags; wider resize and successful mouse activation; and zero terminal scrolls. Separate viewport tests assert wrapped Unicode link labels exclude alignment padding and quote prefixes after numbered reflow.

No concrete table-emphasis defect was established within this inspection. Parent-supplied execution reports 110 library, 29 table, 10 render-options, and 62 viewer passes across selected stable/MSRV default/sessions suites, an existing ignored characterization, and strict clippy; these are not independently verified results here.

## Table emphasis execution evidence - 2026-10-25

Implementer execution, separate from the clean-slate observation:

- Five effective-attribute rendering regressions and the bold-link PTY
  assertion failed against the previous implementation before production
  changes. Later coverage added dense spans, explicit projection/slice errors,
  mixed linked attributes and narrow horizontal clipping.
- `cargo test --offline --locked --quiet -p kvist-blad --lib
  --test table_rendering --test render_options --test viewer --
  --test-threads=4` passed on stable and Rust 1.95.0, with and without
  `--features sessions`: 110 library, 29 table, 10 render-options and
  62 viewer tests per configuration. The existing explicit sustained-input
  characterization remained ignored.
- After strengthening the PTY case to a mixed bold/italic/strike link searched
  across style boundaries, clipped at 16 columns and resized back to 50,
  `--test viewer aligned_table` passed again in all four toolchain/feature
  combinations.
- Strict `cargo clippy --offline --locked --quiet -p kvist-blad --all-targets
  --all-features -- -D warnings`, formatting and whitespace checks passed.
  The monochrome assertion was corrected to reject actual SGR color codes,
  not italic's legitimate SGR 3.

The observer read `src/{markdown,table_spans,render_writer,inline,terminal_text,
viewport,output,lib}.rs`, `src/table/{mod,cell}.rs` and
`tests/{table_rendering,render_options,viewer}.rs` only, without intent, prior
records, reviews or Git history. Execution counts above are not its independent reruns.
No acceptance, exhaustive Cartesian coverage, hard performance bound or
whole-component security certification is implied.

### Independent source-blind table emphasis comparison

**Basis.** Compared the specified requirements, contract, design, four task definitions and publication-facing table documentation against the two new leading IMPL sections. No Rust source/test inspection or independent test execution was performed. Execution results are implementer-supplied evidence.

**Supported agreement.**

- The observed nesting counters and typed bold/italic/strike spans match R-074's first subset, including combinations and supported emphasis within links.
- Plain solver text remains separate from styling. Whole-word/event-split token handling, explicit breaks, Unicode measurement and normalized spaces agree with R-031/R-073. Assigning a retained space the first represented source whitespace's attributes matches the contract.
- Selective attribute transitions, rather than whole-style resets, support preservation of header reversal, alternating row backgrounds and enclosing quote styling. Observed assertions cover effective attributes and unstyled neighbors/separators, not merely escape-sequence presence.
- Source-owned link registration accounts for alignment padding and quote prefixes. Reported viewport assertions address numbered reflow and exclusion of padding/prefixes from link marks.
- The mixed-style PTY observation addresses search overlay application/removal, exact link-selection underline, narrow clipping, resize and subsequent mouse activation. This supports the scoped R-049/R-055 preservation claims without establishing universal terminal behavior.
- Inert authored controls, checked slices/projection failures and propagated writer errors are recorded consistently with the design.

**Scope and documentation.** README, USAGE, EXAMPLES and CHANGELOG consistently describe the three-attribute subset. Dedicated table-code appearance, code-specific whitespace/nonbreaking behavior and table link colors remain deferred; enclosing emphasis on code text is explicitly compatible with that deferral. HTML tables/merged cells and full CommonMark/GFM conformance are not promised. Terminal/font attribute limitations are appropriately acknowledged.

**Evidence limits.** The supplied execution record reports 110 library, 29 table, 10 render-options and 62 viewer passes per configuration, plus strengthened aligned-table reruns, lint and formatting checks. These counts do not establish exhaustive interaction coverage. Initial fail-first evidence covers the five rendering regressions and bold-link PTY assertion; later strengthened cases are not represented as independently observed fail-first runs. An existing characterization remains ignored. Dense-span assertions establish content/attribute behavior, not performance bounds.

**Conclusion.** No concrete scoped contradiction or material unmet obligation is established by the permitted record. This is a bounded comparison of intent against independent observations and supplied execution evidence, not independent rerun verification, human acceptance, exhaustive compliance or security certification.

## Scoped table alignment source observation - 2026-10-25

### Scope and evidence status

This observation derives from executable Rust and test source in `blad/src/markdown.rs`, `blad/src/table/{mod.rs,cell.rs,tests.rs}`, `blad/src/table_navigation.rs`, `blad/src/render_writer.rs`, `blad/src/viewport.rs`, `blad/src/inline.rs`, `blad/src/output.rs`, `blad/src/terminal_text.rs`, and `blad/tests/{table_rendering.rs,render_options.rs,viewer.rs}`.

No intent documents, prior implementation records, reviews, or Git history/diffs were consulted. No repository files were edited. No tests were independently executed; test descriptions below report inspected assertions, not observed passes. Separately supplied execution results are not independent execution evidence here. This supplement is not whole-component certification or human acceptance.

### Table construction and width selection

`blad/src/markdown.rs` collects table cell strings, parser-provided column alignments, and link marks separately. Starting a table resets these collections; ending it renders the accumulated rows and clears alignment state.

In `blad/src/table/mod.rs`, `Table::new` determines the largest row length and fills missing cells with empty strings. Each column's minimum and natural widths are the maxima of its cells' respective widths.

`blad/src/table/cell.rs`:

- Splits cells on explicit newlines, preserving empty and trailing lines.
- Normalizes each line through `TerminalLine`, then tokenizes with `split_whitespace`.
- Measures words with Unicode display width and rejoins adjacent words with single spaces.
- Wraps greedily between whole words; it does not split a word to fit a supplied width.
- Computes a cell minimum by binary-searching between its longest word and natural width. Below natural width, the admissibility check permits at most `width / 4` occupied wrapped lines. Empty lines do not contribute to that count. Natural width remains the upper-bound fallback even when explicit line breaks prevent this aspect relation.

Width selection reserves three columns per inter-column separator:

1. Without a viewport, return natural widths.
2. If natural widths fit the remaining budget, return them.
3. If column minima consume or exceed that budget, return the minima, allowing horizontal overflow.
4. Otherwise, binary-search a maximum cell-height cap. For each cap, independently find each column's smallest width at which all its cells fit.
5. Distribute remaining budget proportionally to unused natural-width capacity, using integer largest remainders; equal remainders favor earlier columns.

The height objective is the maximum individual cell height, equivalently the maximum rendered row height, not the sum of all rendered row heights. Explicit empty lines count toward wrapping height, although they do not inflate the aspect minimum.

### Alignment and emitted layout

`render_table` and `cell_padding` in `blad/src/markdown.rs` apply alignment after width selection and wrapping. Alignment therefore does not alter solver inputs or word partitioning.

For each wrapped cell line, spare space is the allocated width minus Unicode display width, using saturating subtraction:

- Unspecified and left alignment: all spare space trails the text.
- Center alignment: `spare / 2` precedes the text; an odd extra space trails it.
- Right alignment: all spare space precedes the text.

Headers and body cells use the same calculation. Missing alignment entries default to unspecified alignment. Cells with fewer fragments than their row's maximum height are filled with spaces.

Columns are joined by `" | "` without outer borders or horizontal separator rows. The first logical row receives reverse video. Body rows alternate between the configured table background and no table-specific prefix; every fragment of a logical row receives that row's treatment.

### Inline cell formatting

Table content does not use the styled-word representation in `blad/src/inline.rs`.

In `blad/src/markdown.rs`, emphasis, strong, and strikethrough events update style counters, but table text is appended as strings without those styles. Inline code also appends text directly: it receives neither the non-table reverse-video treatment nor `InlineWriter`'s atomic-code behavior. Its internal whitespace consequently participates in ordinary cell whitespace normalization and word wrapping.

Links retain label text and navigation identity, but table rendering does not apply the ordinary inline link color. Link destinations are not emitted by the table branches.

Ordinary table text passes through text projection and inert-text handling; embedded newlines in text events become spaces. Table soft breaks append spaces. Hard breaks append newlines. Inline HTML contributes a newline only for trimmed, case-insensitive `<br>`, `<br/>`, or `<br />`; other inline HTML is ignored by that branch.

Footnote references and math events append their helper-produced text without retaining separate inline styles.

Thus alignment applies to normalized cell text, not to preserved rich inline formatting.

### Link mark positioning

`blad/src/table_navigation.rs` records appended linked text as byte ranges in the accumulated cell string, keyed by logical row and column.

After wrapping, `TableLinks::wrapped` finds each emitted word sequentially in the original cell string. It intersects that word with recorded link ranges and uses `TerminalLine::column_ranges` to convert intersecting byte ranges to grapheme-aware display-column ranges. Normalized inter-word spacing advances the wrapped column by one.

In `render_table`, leading alignment padding is written **before** link marks are registered. `RenderWriter::mark_link_columns` in `blad/src/render_writer.rs` then adds:

- The display width already buffered on the output line, including earlier cells, separators, and leading padding.
- The current quote-border width.

Trailing alignment padding is written after marking and is not added to the mark. Repeated marks for the same navigation identity on one output row extend its last span, so spaces between linked words can lie inside the resulting span.

`blad/src/viewport.rs` keeps navigation coordinates relative to rendered document lines. Line-number gutters are painted separately. Documents are rendered anew for their available width, producing corresponding geometry and navigation spans.

### Bounds and errors

The inspected paths use saturating arithmetic for separator budgets, width totals, padding subtraction, quote-width reductions, viewport width/gutter reservation, horizontal overflow, and clipping endpoints.

Empty tables and tables with no columns produce no table content. Empty cells can have zero natural width. Zero or narrow viewports do not force table words or geometry below their computed minima.

`TableLinks::wrapped` returns `InvalidData` with `"table link token lost during wrapping"` if sequential token correspondence fails. The inspected tests do not directly exercise that failure branch.

Rendering propagates writer and flush errors. `RenderWriter` and viewport document construction report invalid UTF-8 as `InvalidData`. `Output` rejects unexpected, malformed, overlong, or incomplete renderer controls. Plain mode suppresses renderer SGR; color-depth handling otherwise preserves non-color attributes.

`TerminalLine` excludes recognized SGR from display width and clips by grapheme boundaries, substituting spaces for partially visible wide graphemes. Authored controls are made inert before rendering, including control characters decoded into text events.

These observations do not establish arbitrary-input resource bounds. Table solving and emission contain no internal cancellation checkpoints; the enclosing Markdown event loop checks cancellation between events.

### Relevant assertions and coverage limits

**`blad/tests/table_rendering.rs`**

- `delimiter_alignment_applies_to_headers_and_body_in_styled_and_plain_output` checks exact left, center, right, unspecified, and missing-cell padding at widths `None`, `0`, `1`, and `80`; it also checks plain output lacks escapes and styled output starts with header reversal.
- `delimiter_alignment_measures_unicode_display_columns` checks exact padding for a wide character, combining sequence, and ZWJ emoji.
- `delimiter_alignment_preserves_explicit_and_empty_cell_lines` checks aligned fragments around supported HTML breaks.
- `delimiter_alignment_pads_each_wrapped_line_without_changing_geometry` compares aligned and unaligned output at six widths, asserting equal line counts and display widths with expected per-fragment padding.
- `delimiter_alignment_is_local_to_each_table` checks that right alignment does not carry into a following unspecified-alignment table.
- Whole-word overflow, short/empty columns, Unicode words, explicit empty lines, deterministic resizing, equal rendered widths, and table writer failure have additional targeted assertions.

These tests do not assert preservation of bold, italic, strike, code styling, or code atomicity inside cells.

**`blad/src/table/tests.rs`**

- `exact_solver_matches_exhaustive_small_tables` enumerates candidate widths for selected small tables and compares fitting-height or overflow results.
- `cell_floor_matches_exhaustive_text_geometry` compares cell minima with enumerated admissible widths, including generated word lengths, Unicode, and explicit blank lines.
- `wrap_measurement_matches_output_and_is_monotone` checks wrapping height, height predicates, whitespace normalization, explicit empty lines, and selected Unicode/control cases.
- Other tests cover ragged/empty tables, stable rounding, neighbor-independent floors, `usize::MAX` viewports, and a 10,000-word cell.

These are finite cases, not exhaustive coverage of all inputs or performance guarantees.

**`blad/src/viewport.rs`**

- `duplicate_table_and_bare_links_have_source_owned_spans` checks distinct source-owned navigation spans.
- `wrapped_unicode_table_links_are_precise_in_quotes_and_after_reflow` reconstructs linked labels from spans at two widths, including a wrapped label containing strong markup.
- `aligned_table_link_spans_exclude_padding_after_numbered_quoted_reflow` reconstructs header/body labels at widths `25`, `40`, and `80`, with numbering both enabled and disabled; fragments must be trimmed and exclude the quote prefix.

These assertions inspect span text and coordinates, not retained inline styling.

**`blad/tests/viewer.rs`**

`aligned_table_links_keep_selection_and_mouse_hitboxes_after_resize` exercises centered and right-aligned two-column-wide labels, with and without line numbers. It checks that preceding padding does not activate the link, Tab underlines exactly the label's occupied columns, and clicking the resized label opens its target without screen scrolling.

Its resize is from 40 to 50 terminal columns around a fixed, unwrapped table. It does not independently establish wrapped or quoted aligned-table mouse behavior.

`table_overflow_is_accessible_and_resize_preserves_the_first_row` separately checks horizontal access to an overflowing table and its presentation after widening.

**Output and terminal tests**

`color_depth_never_changes_visible_geometry` in `blad/tests/render_options.rs` compares stripped output across themes and color depths, including an aligned table. Entity-decoded control and writer/flush tests cover related error boundaries. `blad/src/terminal_text.rs` tests grapheme-safe clipping and span overlays; `blad/src/output.rs` tests bytewise encoding and malformed-control rejection.

## Table alignment execution evidence - 2026-10-25

The implementer, separately from the clean-slate source observer, executed:

- Before production changes, all five `delimiter_alignment` rendering
  regressions failed on output padding and the aligned-table PTY regression
  failed on the expected label position. The source-bound span regression
  already passed and subsequently protects the new padding/mark ordering.
- `cargo test --offline --locked -p kvist-blad --lib --test table_rendering
  --test render_options --test viewer -- --test-threads=4`: 107 library,
  23 table, 10 render-options and 62 viewer tests passed. One pre-existing
  explicitly invoked sustained-input characterization remained ignored.
- The same selected suites with `--features sessions` passed with the same
  counts and ignored characterization.
- Both selected-suite commands also passed on Rust 1.95.0 with the same
  counts and ignored characterization.
- `cargo clippy --offline --locked -p kvist-blad --all-targets --all-features
  -- -D warnings`, `cargo fmt -p kvist-blad -- --check` and
  `git diff --check` passed.
- A real piped `blad --dump` invocation now centers `b` and right-aligns
  `7` in a table using `:---:` and `---:` delimiters.

These are scoped execution results, not acceptance, independent execution or
a new universal performance/security guarantee. Unrelated pre-existing dirty
configuration, lockfile and manifest changes were preserved.

### Independent source-blind table alignment comparison

**Scope and basis.** Compared the revised Blad requirements, contract, relevant design and `blad-table-alignment-*` task definitions against only the two permitted leading IMPL sections. No source, test source, historical implementation records, other reviews or Git evidence was consulted. Execution results below are supplied implementer reports, not independently rerun tests.

**Implemented behavior.** The independent observation supports R-073 and the new alignment design:

- Parser alignment is retained per table and cleared between tables.
- Alignment is applied after solving widths and wrapping, without changing either algorithm.
- Headers and body fragments receive Unicode-display-column padding. Unspecified/left alignment trails padding; right alignment leads it; center puts an odd surplus space on the right.
- Empty fragments retain allocated width. Source column order, separators, header reversal and logical-row backgrounds remain intact.
- Leading padding precedes link-mark registration. Observed coordinate composition accounts for earlier columns and quote prefixes; number gutters remain viewport-owned.

The observed cell floors, natural-width ceilings, whole-word wrapping, explicit empty lines, supported HTML breaks and overflow policy are consistent with directly relevant R-029/R-031 and table-layout semantics. Alignment introduces no observed contradiction with those constraints.

R-015 and the terminal profile appropriately disclaim full GFM conformance. Inline cell emphasis, strike, code and ordinary link styling remain flattened. Inline code also undergoes ordinary cell whitespace normalization and wrapping; this increment does not preserve atomic code spans. These observations fit the table-specific normalization policy and deliberately separate future styling increment, rather than establishing an alignment defect.

**Verified assertions and supplied execution.** Inspected assertions reported by the observer cover exact padding, odd centers, default/explicit alignment, Unicode widths, empty/broken/wrapped cells, multiple tables, geometry preservation and output policies. Navigation assertions reconstruct labels after quoted/numbered reflow. The PTY assertion checks exact selected-label columns, preceding-padding nonactivation and resized mouse activation with and without numbering, supporting the alignment-related portions of R-049/R-055.

The implementer reports five rendering regressions and the PTY regression failing before production changes; the source-bound span regression already passed. Selected suites subsequently passed in default and `sessions` configurations: 107 library, 23 table, 10 render-options and 62 viewer tests per configuration. Strict Clippy, formatting, whitespace checks and a real piped dump demonstration are also reported passing. One pre-existing sustained-input characterization remained ignored.

**Material gaps and finite boundaries.**

- **R-062 public documentation:** the task record claims README, guide, rustdoc and changelog updates, but the scoped independent observation does not examine them, and supplied execution does not validate their wording. The permitted intent documents describe alignment and flattened styling precisely; publication-facing documentation accuracy remains independently unestablished here.
- The aligned-table PTY resize uses an unwrapped table at 40 to 50 columns. Wrapped/quoted coordinates have inspected assertions, but wrapped/quoted aligned mouse behavior is not independently demonstrated by that PTY case.
- The scoped record does not establish all R-055 emulator details, including continuation-cell/delayed-wrap modeling. Unicode rendering assertions and wide-label PTY evidence are bounded, not terminal/font certification.
- Solver enumeration covers finite small cases. The token-correspondence error branch lacks a direct test. No universal performance, cancellation or arbitrary-input resource guarantee follows.

These boundaries do not create requirements for exhaustive Cartesian coverage or full Markdown conformance.

**Disposition.** No concrete scoped implementation-intent contradiction is established. The queue preserves tests, implementation, audit, then comparison ordering and leaves comparison pending. This report supplies independent comparison, not independent execution, security certification or human acceptance. Acceptance remains a human decision, with the documentation-evidence limitation retained explicitly.

### Subsequent implementer documentation cross-check

The implementer checked the README Rendering section, user-guide table sizing
and syntax profile, rustdoc and changelog wording against observed behavior.
A documentation search also exposed stale alignment disclaimers and an
"alignment ignored" showcase row in `docs/EXAMPLES.md`; these were corrected.
The showcase now demonstrates honored alignment, and all checked public
surfaces distinguish that support from flattened inline cell styling.
No stale unsupported/ignored-alignment claim remains in the searched public
Markdown guides. This cross-check is not independent documentation review;
the independent comparison's limited documentation scope remains explicit.
The comparison task is recorded complete after preserving its report, not as
human acceptance or blanket compliance.

## Automatic streaming pager evidence - 2026-10-25

### Independent source-only observations

One clean-slate observer examined CLI routing, live input, rendering,
navigation and relevant tests without intent, prior records or history.
A subsequent clean-slate observer examined the repaired descriptor lifecycle
and event wakeup with the same exclusions. The observations below combine
their still-current findings; the second observation supersedes the first
observation's descriptor findings.

- Terminal stdout and non-plain settings select paging, including redirected
  stdin. Dump/nonterminal output remains finite; color suppression does not
  disable paging. Missing controlling-terminal preflight fails with a dump
  hint before document consumption. Named files validate first and stdin
  occupies one deduplicated deferred tab.
- Live input owns one close-on-exec duplicate numbered at least 3. Entry
  replaces process fd 0 with the controlling terminal, without changing
  document open-file-description flags. Replacement failure drops the local
  duplicate. This requires exclusive, unbuffered stdin consumption; fd 0
  remains the keyboard terminal after viewer return.
- Editors receive a separately opened controlling terminal as stdin. The
  owned document descriptor does not survive exec. Synchronous editor/browser
  handoffs suspend ingestion and allow normal producer backpressure.
- Source retention is capped at 16,777,216 bytes, including pending UTF-8.
  A pump performs at most four 64-KiB readiness-driven reads, capped by the
  remaining allowance plus one overflow probe. Overflow rejects that read
  wholesale. Decoding preserves valid prefixes, carries incomplete UTF-8,
  and rejects malformed bytes or incomplete UTF-8 at EOF.
- The loop checks buffered keyboard events, waits on document and keyboard
  readiness together, and checks keyboard events again after waking.
  Interrupted polls allow a new loop iteration. A full input batch requests
  an immediate keyboard check. Source arrival need not await a keyboard event
  or the ordinary 50-ms live timeout.
- The retained source is rendered as a complete provisional document, allowing
  unfinished syntax and late definitions to revise earlier output. Refreshes
  rebuild links/navigation/search and clear link selection. Renders coalesce
  behind a 50-ms minimum interval, extended by prior refresh cost. EOF/error
  forces pending final refresh; inactive stdin accumulates without repeated
  rendering and renders when activated.
- Stdin initially follows the bottom. Upward/browsing/search actions pause
  following; bottom navigation resumes it, including downward arrival at the
  bottom. Numeric positions and following survive tabs and resize; resize
  recomputes layout/navigation/search and reapplies the follow pin.
- EOF leaves browsing open. EOF, failure, stop and destruction release the
  owned descriptor without draining. Closing stdin retains decoded source
  and any sticky failure but releases its pipe reader. Ingestion continues
  during help and on other tabs. Input failures remain visible there and
  survive closing stdin.
- Normal quit/final-tab close explicitly restores the terminal before returning
  a stored input error. Restoration attempts each enabled operation and
  retains its first failure; drop retries unfinished steps. Another earlier
  viewer or restoration error can supersede an input error.

The first observer independently reproduced two concrete defects: an editor
could consume the Markdown pipe, and closing stdin left fd 0 holding an
abandoned pipe reader. Dedicated regressions failed before repair. The final
source-only follow-up observed the ownership changes above and independently
passed the editor-input and producer-BrokenPipe regressions. It established
no remaining concrete defect within that narrower scope.

The initial observer independently ran seven input units, one follow-key unit,
14 live-input PTY regressions and five stdin CLI regressions. The follow-up
independently ran:

```text
cargo --offline --locked test --manifest-path blad/Cargo.toml --lib stream_input::tests
8 passed
cargo --offline --locked test --manifest-path blad/Cargo.toml --test viewer live_stdin
15 passed; 1 ignored characterization
cargo --offline --locked test --manifest-path blad/Cargo.toml --test cli stdin
5 passed
```

Its initial `-p blad` attempt failed because the workspace package is named
`kvist-blad`; the manifest-path commands above succeeded. Observed evidence
includes unchanged source flags, descriptor release, keyboard/editor isolation,
fast-producer quit, retained errors and restoration on tested exits. CLOEXEC
was observed through the explicit API, not an exec-time descriptor inventory.
Duplication/replacement/restoration failures were not injected. No fatal-signal
cleanup or non-Linux behavior was established.

### Implementation-session execution evidence

These are execution results, not independently executed by the observers:

- Complete default and `sessions` suites passed on stable Rust 1.99.1 and
  Rust 1.95.0: 282/303 passing tests respectively, with three ignored
  characterizations per configuration. The new streaming characterization
  was also run explicitly.
- Passing regressions cover idle/continuous input, unfinished paragraphs and
  fences, split Unicode/inert controls, help/inactive tabs, follow/resize,
  retained search, late references, empty EOF, sticky failures and the exact
  source ceiling. Whole-source fidelity units compare rendered bytes, link
  spans and anchors with one-shot rendering across chunk widths, rendering
  widths and color/plain settings.
- Explicit finite-route regressions include
  `explicit_stdin_on_a_terminal_is_rejected_without_waiting`,
  `named_files_leave_redirected_stdin_unconsumed`,
  `redirected_file_output_is_plain_by_default_and_head_closes_quietly`, and,
  with sessions, `empty_session_list_handles_a_fully_closed_stdout_without_panicking`.
  Both complete CLI configurations pass. The unused-input regression was
  added after source observation to close a comparison traceability gap.
- The README, user guide, changelog, release guidance and CLI help document
  automatic terminal-stdout paging, the dump opt-out, controlling-terminal
  failure, EOF/follow/source bounds and snapshot/tail distinctions. The user
  guide also discloses process-stdin replacement and error precedence.
  These documentation checks are implementation-session evidence, not source
  observer findings. Inspection found no new spool, worker, unbounded input
  queue or parser cache; retained source is re-rendered through existing APIs.
- Strict all-target/all-feature Clippy passed on both toolchains; strict
  all-feature rustdoc and formatting passed on stable.
- All 221 downstream Skott library tests passed.
- `python3 blad/ci/verify_package.py --offline --allow-dirty` passed actual
  extracted-archive default/session suites, both release installations and
  installed-binary smoke checks. An earlier extracted sessions suite exposed
  short-batch draining delayed by keyboard-only waiting; combined readiness
  fixed the cause. The exact-limit regression also passed with unoptimized
  workspace development/test profiles, without extending its deadline.
- The explicit sustained-input characterization used 36,419 source bytes in
  1,024-byte batches. It observed first text around 101 ms, final EOF around
  557 ms, seven frames, 17,002 terminal bytes, 18,216 KiB peak RSS and quit
  around 110 ms. Harness pacing/polling is included; these are not universal
  throughput or latency bounds.

The source cap is not an aggregate-memory guarantee. Rendering, search and
navigation allocate additional data and run synchronously; viewer refresh
does not use cooperative renderer cancellation. Finite tests establish no
hard filesystem/render/search/wakeup/quit deadline. Named-file reload remains
save-oriented and optional session input remains snapshots.

Final source SHA-256, in stream-input/display/terminal/files/navigation/main/
viewer-tests/CLI-tests order:

```text
9feef5d547710bde32ede605b4c442366ccd7821e2dec0093230328cefb6d8e7
9088838c4e4255ea003cf16abe90fba041af915fff6c4c910ba2aae1c4c2eed3
c6c296a00350ea808d71eacbeeba34eddc3129398e9596a92d07c976952f004e
d83842a63cb87a882d2c7567514cb80af77ec2fa4b934796ee83c6a4abeecc2b
c3c5f83f36c356ee347162faf848a8a2bffb3c398f5ec649fbe47c884d9c89f5
6a955f8fbdfffd5a8cb2da8540ddb15ffda6547c725bb274c48f6d80cbe0306a
8156b1845d043aad23331b8fd98c6c89ee512dc8697266f2a4b3837b4d5a8982
26fd5c2172e82c71cfa6b4b431f0bdfc5a42ffd075ad01352f400ab4fd015f33
```

These are independent observations and scoped engineering evidence, not human
acceptance, a Kvist acceptance receipt or whole-component certification.

### Separate source-blind comparison

A separate reviewer compared only R-065/R-066, R-069 through R-072 and related
contract/design intent against this increment's observations and reported
execution evidence. It did not inspect source/tests, run the application or
read historical records. It found support for routing, readiness/bounds,
fidelity/follow and lifecycle/error behavior; the repaired ownership defects
were not established as outstanding.

The comparison identified traceability gaps for documentation, individual
finite-route cases and explicit absence of architectural additions. The
implementation-session evidence above now names the documentation and finite
regressions, adds an unread-input regression, and records the architecture
inspection. This is additional implementer evidence, not another independent
comparison or an acceptance determination.

The reviewer also reserved simultaneous-error diagnostic preservation:
an earlier viewer/restoration failure can supersede a stored input failure.
It identified no incorrect status, but did not establish preservation of each
concurrent diagnostic. Existing precedence is retained and disclosed rather
than silently changing intent; that policy/evidence reservation remains for
explicit human disposition. Failure injection, fatal-signal handling, other
platforms, aggregate memory and hard latency remain outside established evidence.
No blanket compliance certification or human/Kvist acceptance is claimed.

Compared intent SHA-256, in requirements/contract/design order:

```text
cbe7892daabcc961173571e557bdcc0dd9a3a9edf249e61b9639ee40aabf9e8f
0681951f05479492f8af5f935773381bc55d70769b47c0b6c1adfc94539a59e6
c3053a7210ffcf946b0db67224a8fcc2a86982b66923ce3defd43dd213fdbf96
```

## Simplified keybindings evidence - 2026-10-25

An independent source-only observer examined `src/display.rs`,
`src/viewer_navigation.rs`, `src/viewer_reload.rs`, `src/search.rs` and
`tests/viewer.rs`, excluding intent, prior records, history and execution
evidence. A subsequent source-only observer examined only the display,
navigation-help and PTY files after the direct Ctrl+R follow-up. Together
their observations describe:

- Normal-mode `?`/h/F1 opens help; other help keys dismiss it, except Ctrl+C
  which quits. `/` starts a fresh forward literal search; n/> and N/< traverse
  occurrences forward/backward. The private Search type still supports a
  backward flag, but viewer entry points pass false.
- Normal-mode Ctrl+R starts a fresh forward search, enables regex and clears
  link selection. In a prompt Ctrl+R toggles regex and immediately recomputes
  matches. Printable `?` remains query text. Plain p/f and F5 have no normal
  action binding.
- Link selection retains an index and span part but no modal dispatch.
  Tab/Shift+Tab, Enter and spatial navigation remain available without
  blocking scrolling, help, tab switching or closing.
- Only watcher observations trigger in-session reload. The existing 250 ms
  polling/debounce, bounded source read and candidate-before-replacement
  transaction remain. Failures retain the document and show a footer error;
  successful changes clamp offsets, rebuild navigation, clear selection and
  recompute search. Unchanged failed metadata is not retried until it changes.
- Focused PTY assertions cover help, reverse traversal, inert keys, direct
  regex entry, toggling, fresh re-entry, slash literal reset and link
  deselection. Former manual-reload scenarios now await automatic save frames.

These are source observations, not test execution or acceptance. Finite PTY
coverage is not exhaustive modifier/terminal/race coverage; render failure is
not injected. Empty Ctrl+R entry and its exact help wording are not separately
asserted. Metadata stability is not a snapshot or hard latency bound; sustained
writes can defer reload. Search occurrence indices are recalculated near the
viewport rather than preserved exactly.

Final source SHA-256, in display/navigation-help/reload/search/PTY order:

```text
53381386ba53422a363544dc54551c60d7b86a38a2deb1b932e78a34d8a8eb8b
1b54a0baeea6e3bec313d984fad4b227af8c2ca6ff95b1a1c4ba81e06066c23a
a0e4bf2cf9050f4b7180920b2dcfb51cb146fe84ce4d47b73298ad9fa9d3df76
13fd59165af523e5b298e75b92e6645e783c1a4ef3869c29c558c7236358dbb3
d188f52c81ab9104c2ad31472defc47772a78565783328751d3b2207d8314cfe
```

Implementer execution reproduced help-only ?, inert reload/f and direct-regex
regressions before their corresponding fixes. Final `cargo fmt --all --check`,
strict all-target/all-feature Blad Clippy and patch hygiene pass.
`cargo test --offline --locked -p kvist-blad` passes 256 tests and
`cargo test --offline --locked -p kvist-blad --all-features` passes 277 tests,
including 46 PTY cases in each configuration; each suite retains its two
pre-existing ignored characterizations. This is scoped engineering evidence,
not human acceptance or whole-component certification.

A separate source-blind comparator read only the relevant revised local intent
and this evidence section, without source, tests, queue or historical records.
It identified no concrete discrepancy in this increment and found support for
the scoped alteration, retaining the coverage limitations above. It did not
independently execute tests or certify broader requirements.

Compared intent SHA-256, in requirements/contract/design order:

```text
cc9eda4e1ea3ae20b215a2df39f3e9ba14761d5d146e303cfdf857b2c54ab08e
62030596a33086fe3b9eb1aa3aa9f2ec166259cca4ba842017f319f28f502593
70513ff4e79fc0dcf736746ed9a3dea4ae83db0f94f67511c1e8b7810c41a591
```

## Extracted-package PTY synchronization evidence - 2026-10-25

PR #9's stable and Rust 1.95.0 archive jobs failed only
`theme_file_settings_survive_tabs_edit_and_resize`, with an empty raw-output
assertion. The test's optional input helper could return after a single idle
100 ms poll without receiving the action's redraw. A new delayed-input test
first reproduced that early return with a stale normal footer.

An independent clean-slate `pty-source-observer` read only `tests/viewer.rs`,
without intent, prior records/reviews, history or execution evidence. It observed:

- `keys_with_frame` writes input and requires an end-of-synchronized-update
  marker within the existing five-second read deadline. `keys` retains optional
  output for other callers. Startup and resize also require frame completion.
- The delayed-input test stops the retained child, resumes it after 300 ms in
  a scoped thread, sends `/hit`, checks the rendered literal-search count and
  exits with Ctrl+C. The thread joins before Viewer destruction, even when
  the scope body panics, keeping the owned PID alive until signaling completes.
- Successful editor tests require redraws. Theme checks observe editing and
  each tab switch separately, asserting custom heading color and, on the
  target document, the light code background. Resize retains its color check.
- Existing exit helpers check terminal restoration; Drop kills and waits for
  the directly owned child. No explicit descendant-group cleanup is added.

Observed source SHA-256:
`e7e0998f92368f0fa3e87dd1c44b755cd0bf258975ac23fe5dd4bbee266cf461`.

A separate `pty-intent-comparator` read only local intent, that observation and
the two archive execution logs, without source, queues, previous records or
history. It found evidence supporting the narrow R-063 repair, not complete
component compliance or release approval. It retained these limitations:

- Frame completion detects an end marker received during the call, not a
  matched start/end transaction or action acknowledgement.
- The new design's blanket wording about redraw actions is broader than the
  actual scoped migration: unrelated ordinary-key callers remain optional.
- Pre-existing editor-failure tests can wait for exit without draining a late
  diagnostic into their raw buffer. Those exit-only routes were not changed.

These observations remain explicit review limitations for maintainer
disposition; they are not silently resolved by altering intent or claiming
all timing races are eliminated.

Compared intent SHA-256, in REQUIREMENTS/CONTRACT/DESIGN order:

```text
7137dccf6845d5d8117dd257752368baef24d9bf47fa7db791419154e6b70abc
9900a45d051037a7b6932722337add9278e860834b4bb5956835f342317fa65d
c6e6a6807e540749e763fa667fe15433f9317bc287941cc6d83c91d2aa8d0c13
```

Implementer execution: source PTY suites pass on stable and 1.95.0; formatting,
strict all-feature Clippy and patch hygiene pass. The actual archive verifier
passes on both toolchains with default and sessions tests and isolated release
installations, including stdin/redirected smoke checks: 44 PTY tests per cell,
254 total default and 275 sessions passing executions per compiler; two opt-in
characterizations remain ignored in each feature suite. Local candidate runs
used `--offline --allow-dirty`; no runtime source or dependency changed.
Explicit toolchain probes identify rustc 1.99.1 and rustc 1.95.0. The comparator
correctly notes the archive logs themselves did not print compiler identities.

Archive execution-log SHA-256, stable then MSRV:

```text
d52c7ef120656fdc0019c0ff66426c096b237298ae72c30226d3a530a3467701
a63f28db62d48bcaef65703982e6ab47c3cbd92c07983a74d70e2a77f13bab30
```

## Source-only release-readiness observation - 2026-10-25

Independent clean-slate `source-observer` inspected Blad source/tests, package
metadata, verifier and workflows, excluding intent, queues, prior records,
user docs, reviews and history. It ran no tests and received no execution
claims. A separate clean-slate `final-source-supplement` observed the final
regex-toggle source and test. Selected observations are transcribed below;
the reports are not compliance or security certification.

### Inputs, output and file reload

- `kvist-blad` 0.3.0 declares library/binary `blad`, edition 2024, Rust 1.95,
  empty default features and optional `sessions`, without peer path dependencies.
- Canonical duplicate files reuse their initial order. All initial inputs load
  before document output/terminal entry. `-` reads redirected stdin once as a
  finite UTF-8 snapshot; terminal stdin is rejected. With no files/command,
  redirected stdin supplies `-`; terminal stdin prints usage and exits 2.
- File/stdin sources have an inclusive 16 MiB bound and one overflow probe.
  Files open nonblockingly/no-follow and require regular opened descriptors.
  Initial canonicalization accepts authored symlinks; later final-path symlink
  replacement fails on reload/editor reread. Ancestors are not isolated.
- Interactive entry requires both standard streams to be terminals and
  non-plain options. Otherwise documents render sequentially without tab
  headings. Noninteractive CLI output is normally plain; explicit always-color
  retains styling, but dump overrides it. CLI render/final-flush BrokenPipe
  is quiet success; other I/O errors are escaped stderr failures with status 1.
- Only the active path is polled at 250 ms intervals. A candidate must remain
  unchanged for another interval. Observations include device/inode, size,
  mode, nanosecond mtime/ctime, or explicit inspection errors. Each stable
  observation is attempted once, including failures. New viewer sessions start
  fresh watchers, refreshing inactive tabs after activation.
- Reload reads/renders a candidate before replacing live content. Success
  preserves/clamps x/y, rebuilds links, clears selection and recomputes search.
  Unchanged automatic content skips rendering; failure retains the old usable
  document and reports a footer error. Automatic success clears prior reload
  errors, without a success notice. Metadata polling is not a content hash,
  concurrent snapshot or hard I/O/render deadline.
- Painting uses changed rows/synchronized updates, not terminal reentry.
  Editor handoff restores/reenters the terminal and uses whitespace-split
  EDITOR/default nvim without shell evaluation or a timeout.

### Keys, diagnostics and independently observed correction

- Ctrl+C precedes all modes; help consumes the next other key. Ctrl+Q quits
  outside search editing; normal Q quits. Tab switching is disabled during
  search editing/link mode. Normal q/Esc closes a tab; prompt Esc clears search.
- `/` and `?` begin forward/backward literal search. `n`/`>` repeat direction;
  `N`/`p`/`<` reverse. Query/regex caches are bounded, total retained matches
  are not. Home/End move horizontally, Ctrl+Home/End vertically. Ctrl+R/F5
  reloads in normal mode; h/F1 opens help.
- The first observer identified stale matches when toggling an existing query:
  dispatch flipped regex mode but skipped search recomputation. Its original
  source snapshot was `b66e6c5a460c2dae1d333eb0fa406296862aff08a64d57ef05fb2c04596c75ec`.
  The independent final supplement observes `changed` remaining true on Ctrl+R,
  immediate `search.update`, and a PTY assertion using `hit[1] hit1 hit1`:
  literal 1/1 → regex 1/2 → literal 1/1 before Enter. Observers executed no tests.
- Logging uses a process-global Once, BLAD_LOG then RUST_LOG, default warn.
  A log file opens append/create once, nonblockingly/no-follow, validates a
  regular descriptor, and retains it across pathname changes. New files use
  0600 subject to umask; existing modes remain. Invalid destinations emit an
  escaped warning and fall back to stderr. Footer notices are not automatically
  tracing events; fatal CLI errors remain unconditional stderr.
- Renderer strings/Write have explicit deterministic host options; no renderer
  terminal/process discovery. Controls are inert, styling respects plain mode,
  writer/flush errors propagate. Math/diagrams are bounded text subsets with
  visible fallbacks and cooperative cancellation, not hard preemption.
- Optional session reads are descriptor-relative and individually bounded.
  Snapshot output is not interactive. Named exports preserve raw Markdown,
  create/truncate the selected output and are not atomic or inert terminal dumps.
- The verifier defines extracted default/sessions suites, isolated release
  installs, package payload/target checks and stdin/redirected/plain smoke
  comparisons. Workflow declarations do not prove execution, registry ownership,
  protected environment approval or human acceptance. Sources use Unix APIs.

### Final observed source fingerprints

SHA-256, relative to this component. Final supplement supersedes the earlier
display/test fingerprints; all other listed sources remained unchanged.

```text
47cb324341a8a8333a756f095e4368adfb0e9c690699bdb2e002fc4ff8d717f3  src/main.rs
24812ade905665a093bdd75f502f6f3d0dea9a1d4669ea34008c2f0e985fb6b1  src/display.rs
16b463bb4cd2fbeaa8756d8866421f33a06e11bb7c86956d1fe4afb7cc823c26  src/viewer_files.rs
a0e4bf2cf9050f4b7180920b2dcfb51cb146fe84ce4d47b73298ad9fa9d3df76  src/viewer_reload.rs
ddd2b61e53fae814d6e28ace22fbb9685f939fe84e3a6e8bbc2cf8978bc92066  src/logging.rs
13fd59165af523e5b298e75b92e6645e783c1a4ef3869c29c558c7236358dbb3  src/search.rs
f08a4e9fbc4351e6bd5101b46ce109bdbd619a32ae8e06c3801769b3e6b77d59  tests/cli.rs
c1a7e86d8c9b974c8ea1d64f09b196e9278902ad93aaa725fd1efff477352c8b  tests/viewer.rs
d5566d67feb079a039e3cf4ee8951a01744af36996a3e79e9ede57d729bab4d2  ci/verify_package.py
```

### Independent source-blind comparison and final execution

Separate `intent-comparator` read only local intent, the independent source
reports and final execution logs. It did not inspect source/tests, manifests,
workflows, queues, earlier records/reviews or history. It found scoped agreement
for automatic reload, stdin/pipelines, logging and the repaired search behavior,
not wholesale compliance or publication authority. The independently found
regex-toggle defect was reproduced before repair and the final PTY regression
passes in both archive feature modes.

Two residual **intent wording discrepancies remain for human arbitration**:

1. Historical DESIGN security prose says no writes without edit mode, whereas
   current CONTRACT/source explicitly permit named export and configured logs.
2. Historical DESIGN says help/version need no file/theme I/O, but explicitly
   configured logging initializes before argument parsing. Current requirements
   instead guarantee visible fallback without blocking help/version.

These findings are retained rather than silently rewriting intent to certify
the implementation. Candidate export/nonblank-kind/help clarifications and
the conservative 16 MiB policy still require maintainer disposition; this
review does not retroactively accept earlier findings or fabricate receipts.
The comparator also retained coverage/provenance limits for exact public-doc
accuracy, styled/plain task clauses, nonblank-kind cases, matrix attribution
and Galla runtime evidence. Unverified scope is not a demonstrated test failure.

Actual final execution, separately supplied by the implementing context:

- Stable rustc 1.99.1 and MSRV 1.95.0 each pass default (253) and sessions (274)
  suites, including 43 PTY cases per mode; each suite normally ignores the two
  opt-in characterizations. Both characterizations were separately run.
- Formatter, strict default/all-feature lint, warnings-as-errors rustdoc and
  downstream Skott/Kvist compilation pass. Offline dependency/advisory/license/
  source policy passes; seven duplicate-version warnings were observed. This
  uses cached advisories, not a fresh release-time database.
- The actual extracted archive passes 527 executions across both feature modes;
  both isolated release installations and installed stdin/redirected/dump
  comparisons pass. This candidate preparation used `--allow-dirty`, not an
  accepted clean release/tag or registry publication.
- Corpus observations: 14,900-byte mixed text cold 33.34 ms, warm means
  6.24–6.55 ms across depths; admitted diagram means 0.225–0.831 ms. Larger
  chain fixtures explicitly hit their work ceiling. These are measured fixtures,
  not giant-file or hard responsiveness guarantees.
- After comparison, an additional labelled rerun identifies every compiler/
  feature command and the native Galla nesting test explicitly, all successful.
  This supplies attribution missing in the original quiet log; it is additional
  implementer execution evidence, not an independently rerun comparison.

Compared intent SHA-256:

```text
7137dccf6845d5d8117dd257752368baef24d9bf47fa7db791419154e6b70abc  REQUIREMENTS.md
9900a45d051037a7b6932722337add9278e860834b4bb5956835f342317fa65d  CONTRACT.md
b570aef7ae2027b95e1f156ff3f1755a897dac57b8f32f310a3e9a48d17d1366  DESIGN.md
```

Evidence SHA-256 (session-local files; source observations above are persisted
here so interpretation does not depend solely on the session):

```text
5e501b3c939ad72de36c51974390af9e962d6df17fa268745be2aeb5053697f2  source-observation.txt
0dc41e075697002a7d9dbb0a2b17ef40f8a8bdb7aa35e81a1af73d267110cb43  source-supplement.txt
792841f99ed0e912336afe021947c1acabc3baf74ddf5c631750ccaab5baf240  source-blind-comparison.txt
bfc8410ec9f3bf996b39b91a1a85f81a791da6472ff8d805ecc1944a27293e4d  blad-source-validation-final.log
7429d511cc49b99eaf6c8167a36ae73b980cf48521fa8c0714408b401f2f1234  blad-package-validation-final.log
ff97c832d95b635a9f6046e3146f50876ccc899e0caf8022db4b29ed62b4fd6d  blad-labelled-validation.log
6b89b71a81813c727d353b610a26f2ed94767245d118c4f2b50bf0e8bf2e4440  kvist-blad-0.3.0.crate
```

Assessment: a locally verified **Linux release candidate**, not unconditional
release approval. Maintainer arbitration/exact-revision acceptance, fresh
advisories, registry/account/legal-notice verification, protected publication
approval and real-terminal/font smoke remain external release gates.
Per-source bounds do not prove aggregate memory, latency, ancestor isolation,
atomic snapshots, all-terminal behavior or other-platform support.

## Standalone publication observation - 2026-10-24

The following record was independently derived in a source-only context.
Its heading levels are normalized here; the observer did not read prior
records, intent, queues, user documents, history or reviews.

Observed 2026-10-24 from permitted Blad production/test source, manifests, verifier, and workflows only.
No intent artifacts, queue, existing implementation record, user documentation, history, or other reports were consulted.
This is a source observation, not a review, certification, acceptance, or publication authorization.
Final display/script source was reread; no executing/overwritten log was read and no test command was run in this refresh.
Earlier observed execution results do not establish results for these fingerprints; later final execution is separate reviewer input.

### Package and API
- Registry declaration: `kvist-blad` 0.3.0; library crate and sole declared executable: `blad`; edition 2024; declared MSRV 1.95; AGPL-3.0-or-later.
- Default features are empty; optional `sessions` enables rustix plus session APIs/CLI; neither mode depends on a Kvist peer runtime.
- Skott consumes the version-pinned package through alias `blad` and a local path without enabling sessions; workspace default member is maerg.
- Public modules: display, links, markdown, theme, optional session; reexports include MarkdownRenderer, render options/palette/color enums and RenderCancellation.
- Renderer accepts caller strings and Write, propagates write/flush errors, shares syntax assets, and performs no environment/TTY/stdin/subprocess discovery.
- Options replacement retains width and syntax-theme selection; invalid explicit syntax themes error; some older void display wrappers print errors instead of returning them.

### CLI, appearance, and viewer
- Files are extension-independent and retain first-argument order after canonical deduplication; all initial files load before document output or terminal entry.
- CLI options: --dump, --line-numbers, --theme auto|dark|light|terminal, --theme-file, --color auto|always|never, --math unicode|source, --mermaid text|source, --diagram-charset unicode|ascii, help/version.
- No stdin-document route; no arguments produce non-success usage; -- disambiguates command-like filenames; session commands and files cannot be combined.
- CLI uses the viewer only with both TTYs and no dump; otherwise files render sequentially. Single-document library display selects by both TTYs even with plain options.
- Library defaults: dark/truecolor/styled, Unicode math and diagram structure, Mermaid text, unbounded width; plain suppresses all SGR, not Unicode or Markdown projection.
- CLI auto theme prefers an explicit file theme, else final COLORFGBG item 7/15 means light; explicit theme overrides file theme, retaining palette overrides.
- Auto color is off for nonempty NO_COLOR, non-TTY stdout or TERM=dumb; otherwise COLORTERM truecolor/24bit, TERM containing 256color, then ANSI16 select depth.
- Always overrides auto suppressors; never removes added colors but can retain emphasis/reset SGR; dump overrides all styling.
- Theme JSON is explicit, strict, regular-file bounded to 65,536 bytes; palette colors are 0..255 or #RRGGBB, alerts require five values, fills may be null.
- Built-in terminal theme has no fixed code/table/quote fill but retains semantic/syntax foreground styling; explicit palette overrides can add fills.
- j/k or arrows scroll lines; Space/PageDown/Ctrl+F and PageUp/Ctrl+B scroll pages; d/u or Ctrl+D/U half-pages; g/H/Home top; G/L/End bottom; Left/Right horizontal.
- Down/j has a match guard requiring offset below total_lines.saturating_sub(visible_lines); f has a !links.is_empty() guard before entering dedicated link mode.
- / and ? start forward/backward search; Ctrl+R toggles regex, Ctrl+U clears, Backspace edits, Enter commits, Esc cancels the prompt; n repeats, N/p reverse.
- Search is case-insensitive; input is bounded to 4,096 UTF-8 bytes, regex/DFA compilation to 2 MiB each; zero-width matches are omitted, total matches unbounded.
- Tab/BackTab select links; Ctrl+arrows select spatially; Enter follows selected or first link at/below viewport top; `[`/`]` jump headings; Alt+Left/Right cycle tabs; b goes back.
- h shows modal help; r reloads; e edits; Ctrl+L redraws; normal q/Esc closes a tab, Q quits all; Ctrl+C precedes all modes and always quits.
- Help consumes every non-Ctrl+C key only to dismiss; prompt q/Q are text; dedicated link mode consumes ordinary keys except Tab/BackTab/Enter/Esc, while Q quits.
- Alt-tab switching is disabled in prompt/dedicated link mode; normal Esc after search Enter closes the tab. Key-release events are ignored.
- Tabs retain both offsets but search/link state is reconstructed on viewer-session changes; reload failure retains old content/state, success rebuilds navigation/search and clamps offsets.
- Renderer-owned source identities distinguish duplicate/wrapped/table links; anchors include generated Unicode slugs, duplicate suffixes and explicit IDs; percent decoding rejects malformed UTF-8/escapes.
- Viewer reserves gutter/scrollbar width, clips whole graphemes/display columns, paints changed rows in synchronized updates, and owns/restores raw/alternate/cursor/mouse state.
- Explicit URL follow runs xdg-open without a shell or terminal stdin, captures 4,096 diagnostic bytes and kills/reaps its owned launcher after five seconds; descendants are not managed.
- Editor uses whitespace-split EDITOR or nvim, no shell interpretation/timeout; launch/status/reread errors propagate after terminal restoration.

### Rendering and input boundaries
- Bounded file reads require descriptor-verified regular files, nonblocking/no-follow final opens, inclusive size admission plus one growth-probe byte, and valid UTF-8.
- CLI documents/reload/editor rereads have a 16 MiB per-file bound; initial/file-link/theme canonicalization permits authored symlinks; later substituted final symlinks on reread are rejected.
- Authored controls except LF/tab and selected bidi embedding/isolate ranges become visible escapes; CRLF normalizes; standalone CR is escaped; decoded text is also made inert.
- Headings, lists, emphasis, quotes/alerts, rules, footnotes, code, links and selected GFM extensions are terminal projections, not full Markdown conformance.
- Task items retain bullets and explicit [x]/[ ] markers, normalizing [X] to [x]; there is no checkbox mutation/persistence command.
- Raw HTML is omitted except recognized table-cell br tags; images have text alternatives only; table alignment delimiters and independent inline cell styling are not implemented.
- Prose wraps by display-width/graphemes; inline code/math and fenced code may overflow. Tables preserve whole words/explicit breaks, natural-width caps, aspect floors and deterministic integer geometry.
- Script spans project wholly to Unicode or ^(...)/_(...) fallback; escapes, encoded delimiters and markup boundaries prevent arbitrary span reinterpretation.
- Script scanning checks bounds and ASCII whitespace before `bytes[cursor] != marker || escaped(...)`; escaped means an odd preceding backslash run.
- Math is a nonexecuting Greek/operator/script/fraction/root/group/limited-text/paired-delimiter subset; unsupported/invalid source remains visible with distinct annotations.
- Math projection limits: 16,384 input bytes, 32 nesting levels, 65,536 output bytes; oversize fallback bounds retained source; source mode bypasses projection.
- Native Merman 0.9.1 admits spatial flowchart, sequence, state, class and ER subsets; width never reflows finished rows; ASCII changes structure, not authored Unicode.
- Mermaid rejects unsupported semantics/nonspatial fallbacks and bounds source/lexical/model/layout/grid/output/graphemes; invalid/resource/unsupported failures visibly retain bounded source.
- Mermaid limits include 64 KiB source, 256 nodes, 512 edges, 32 nesting, 262,144 cells, 2,000,000 work units, 2 MiB output and a cooperative 500 ms deadline.
- Cancellation clones share state; cancellation/deadlines return Interrupted rather than successful fallback; errors may follow partial output. No hard total-time or aggregate-memory bound exists.

### Optional session consumer
- CLI: --log-dir defaults .skott/runs; list --limit defaults 20; session accepts ID/prefix or an existing directory; export accepts ID/prefix with optional output path.
- Manifest requires schema_version 1, kind skott-conversation, nonblank session_id/status and entries; model/created_at are optional strings; unknown fields are retained/ignored, not fully producer-validated.
- Entries require positive sequence, nonblank kind, normalized messages/<one filename> path and optional disposition; sequence uniqueness/order, byte declarations and timestamp syntax are not checked.
- Root/child directories and artifacts use no-follow/nonblocking descriptor opens; manifest/messages are regular UTF-8 files bounded individually to 16 MiB.
- Session retains Arc<File> for its directory; messages are opened relative to that descriptor and a held messages descriptor, not the display path; renamed/replaced root paths do not redirect reads.
- Missing runs root is empty; manifestless directories are skipped; malformed artifacts, encountered session symlinks, nonregular content and message read errors propagate.
- Listing parses all candidates before descending lexical optional-time/ID sorting and truncation; exact/unique nonblank prefix lookup has no thousand-session cutoff and rejects ambiguity.
- Message manifest is revalidated and order preserved; all bodies load before transcript output; exactly completed marks completion; other statuses print an active snapshot notice, not live follow.
- Session display applies rendering/dump settings and inert labels/metadata; export preserves raw source, adds separated headings/rules, and can overwrite through non-atomic File::create.
- Descriptor roots do not isolate ancestors or freeze contents; there is no aggregate session/message/descriptor cap, ownership check or hard-link exclusion; export is not an inert terminal dump.

### Test and packaging source evidence; limits
- Inspected assertions cover checked/unchecked tasks, input limits/UTF-8/FIFO/symlinks, session path/error/retained-directory behavior, export source preservation and lookup beyond 1,000 sessions.
- Linux PTY tests assert search/link/modal/tab/reload/editor/resize/clipping/terminal restoration/browser behavior; rendering tests cover colors/controls/math/diagram limits and a small-table exhaustive oracle.
- Ignored text/diagram timing characterizations are not execution evidence. Source assertions alone do not demonstrate current tests passing, conformance, platform portability, race freedom or publication.
- Package include selects ci/verify_package.py rather than arbitrary ci/**; verifier requires Python 3.12+, hashes the archive into its build target, extracts it and excludes peer packages from its lock.
- Verifier source runs extracted full tests and release installations in default/sessions modes, asserts one blad binary/version/feature help, then empty-PATH plain task-state rendering; it is not a hermetic sandbox.
- Quality workflow declares stable/1.95 default/sessions lint/test/docs and extracted verification, plus dependency policy; path filters include both Blad workflows.
- Both final cargo-deny steps set manifest-path: blad/Cargo.toml separately; arguments are only --config deny.toml --all-features --locked. This is source configuration, not execution evidence.
- Publish workflow checks tag/version/supplied revision/clean tree, declares source/archive/policy gates, requires a publishing secret and dry-run, then publishes and registry-smokes default mode.
- Workflow identity inputs are not independent-acceptance receipts; actual runs, environment approvals, credentials, compiler identity and registry results need separate evidence.

### SHA-256 provenance (paths relative to repository root; source snapshots only)
```text
22c23184815ba564b976f140fd8222c2a0c3b4288300b8c6d5bc29bd6c8a4a45  blad/src/display.rs
327285a32e4e7a4019b0457613f27566e72a87dae00a786b9398338f973d3882  blad/src/scripts.rs
6eb1edea2d59c3235ccdc28f95034f437126cfeb409e1628707508b9d4e38d31  blad/src/session.rs
8bfb0a0b8f59b23e86a8609964c05cce0cd4fdefb6f82bcd02f3bc9325667bd3  blad/src/file_input.rs
195615776350a4ff4c18100eb458d1aa6cb710c234e603d04bab231a4b91d712  blad/src/markdown.rs
9fe2611cc99a5fc99d4de32af78b2955b4d4546bb38991cd50405f8c09a7b045  blad/src/main.rs
a60c3c5fbac5fece061d53128d731a90cfb013bd1c58a621a5fa6e384f8f9c32  blad/src/viewer_navigation.rs
fa03798602ad779f282590c01ce606d8520a72fc9b8439211191789dcd73e859  blad/src/lib.rs
e32da87846d3501a86cb7593a39c94c7f95bb2a0146455596c87eda0822865db  blad/Cargo.toml
e353783baae15d4f6389751848f48cad3afdc870f5b218716192d9c1cb5f4685  Cargo.toml
f09a6e8865223a09e2349228cacbf9d14785f0098d705f7fb371546c0f233684  Cargo.lock
68637c8bfd330dd947e9a8f12e8ada2df3cfeb0493f720097bed60c4a7b12e12  skott/Cargo.toml
cf2e216b9d16475afbf65ec4cc622ff001b868bc2cddd0dc3e45785c334eee96  blad/ci/verify_package.py
6241e8740f91deeba7eed7b47620a6fc32773d53339bed21beed9b7abba60775  .github/workflows/blad.yml
2a944c34a8cb2dbde321821140d501308144f75d41ed41c040db51d6fe7438b3  .github/workflows/blad-publish.yml
```

### Independent source-blind publication comparison - 2026-10-24

Comparator read only local Blad intent, allowed consumer docs, independent
source observations and final supplied execution evidence; no implementation,
tests, manifests, workflows, queue, prior IMPL/reviews, history or root intent.
No acceptance or security certification is given.

Exact intent SHA-256 (computed directly and rechecked):
- REQUIREMENTS.md: eb3866b223a0ff144420d7df860d55495d53649349cbef13e1feb6da31191d64
- CONTRACT.md: c91b30a644c82ccb385515a8cd0ef6436ffa4a3ce4db681441b021336b629700
- DESIGN.md: a48a26838d5ea55a4b1280b0eca9e46f506d3139fda54270a4a4bd0c449d4e7c

Evidence: BLAD_IMPL_SUPPLEMENT.md (independent source observation), permitted
BLAD_SOURCE_OBSERVATION.md precision sections, BLAD_VALIDATION.md and final
allowed logs. Historical execution sections were not used for final results.
Refreshed observation/evidence SHA-256:
- BLAD_IMPL_SUPPLEMENT.md: face5bfb77066325d99ef3ae3c1e91694158fd77ac4111bb9ab7ca63dc968df1
- BLAD_SOURCE_OBSERVATION.md: f0d9dfc0acf1febe25a33df23a72a615f40f67367d80a8f4236a8707266eca01
- BLAD_VALIDATION.md: 75cfdc7b6d970c21d63cffcbbe1ac6eb6f82cf835c890f1ab40ec89ff626762e
Consumer SESSIONS.md: 2376447b2b603a4e0315c3282283fc1ed18b860065101c81a0ab4e6bbd6f1190
The provider identifies rustc 1.99.1/stable and 1.95.0/MSRV.
Source suites: 237 default / 257 sessions passes per toolchain, zero failures.
Each mode includes 38 Linux PTY cases and two ignored characterizations.
Final archive suites: 494 passes per toolchain across both modes, zero failures;
both isolated release installations pass. Both logs identify the same archive:
4e0c437b4888f60a617558ab02fc08d6814c55906674fe10d2b8cf0e29c7975a
(kvist-blad-0.3.0.crate). Archive preparation used --allow-dirty.
The two ignored characterization cases were separately executed successfully.
Offline policy reports advisories/bans/licenses/sources ok with seven duplicate
warnings. New raw quality/guard logs show fmt/strict lint/docs, downstream
compilation, license comparison and wrong revision/version/dirty-source
rejections with no untrusted Python import. Local action-policy output passes;
its corrected-invocation mapping is provider-attested, not a remote action run.
Source-observed workflow-input correction alone is not execution evidence.

R-059..R-063 engineering substance is supported by the bounded evidence:
independent kvist-blad identity with blad targets and optional sessions, local
package fixtures/docs/license, exact task state, bounded descriptor-relative
session reads, accurate standalone/integration guides, archive tests/installs.
No new publication-feature functional failure is demonstrated.
Markdown projection, full palette/color policy and documented modal dispatch
agree with observation; they are not full conformance or platform certification.

Unresolved findings:
- F-01: CONTRACT 378 forbids writes outside edit mode, but R-004,
  CONTRACT 225-226, SESSIONS 84-89 and observation permit named export overwrite.
  This is a concrete intent-authority conflict, not unexpected export behavior.
- F-02: Originally both contract/guide said nonempty kind, versus observed
  nonblank validation. SESSIONS 135-141 now explicitly rejects empty/whitespace
  kinds, correcting the consumer omission. Unchanged CONTRACT 217-219 still
  says nonempty; contract/validation-strength disposition remains human.
  No execution of that exact whitespace-only case is established.
- I-01: CONTRACT 140-142's help/global-quit wording is ambiguous about Q.
  Actual/documented Q dismisses help; Ctrl+C always quits. Current CONTRACT
  110-112 expressly makes post-search-Enter Esc close the tab, matching behavior.
  Do not infer human disposition of retained input-policy/key findings.
- G-01: All routes, permission errors and mutation schedules are not exhaustively
  executed. CONTRACT 368's available-IDs error payload is not established.
- G-02/G-03: Raw gate logs address the earlier summary-only granularity gap.
  No real-terminal manual smoke, near-limit interactive search, fresh registry
  resolution or hermetic sandbox is established.

Limits: byte bounds are per artifact, not aggregate memory/descriptor/search
caps, ancestor isolation, snapshots or hard I/O/rendering deadlines.
Math/diagram limits are bounded subsets and cooperative, not hard preemption.
Export is raw source and non-atomic; input no-follow does not protect output.

Human/external prerequisites remain: exact-revision acceptance and retained
input/key arbitration, findings disposition, accepted clean commit/tag, registry
ownership/account and legal/notice checks, fresh advisories, protected publishing
approval/credentials, actual remote gates/publish, and registry installs in both
modes plus unlocked/docs.rs/public-link/terminal checks. Local passes and workflow
identity inputs are not acceptance receipts, publication or security certification.

Detailed external report: BLAD_COMPLIANCE_COMPARISON.md in the supplied session
files directory. Intent was not modified to fit implementation.

## File-input reliability observation - 2026-10-23

This supplement maps observed file-input behavior to R-056 through R-058.
A clean-slate documenter inspected only relevant source, assertions, manifests
and parent-run execution logs, excluding intent, queue, this record, user/project
documents, earlier reviews, chat, Git history/diffs and peer implementations.
It did not execute tests or decide compliance.

### Independent observed behavior

Both `lib.rs` and `main.rs` declare private `file_input` modules using the same
source. The display library uses its instance; the binary's theme parser uses
the binary instance. `read_regular_file` opens read-only with
`O_NONBLOCK | O_NOFOLLOW`, obtains that descriptor's metadata and rejects
nonregular inputs before reading. A reported size over the caller's limit fails
early. Otherwise `Read::take(limit + 1)` bounds actual bytes before returning
`FileTooLarge` or a complete accepted buffer. Errors name the allowed byte
count; read errors retain their kinds rather than returning successful prefixes.

Standalone document admission is inclusive at 16,777,216 bytes and precedes
whole-buffer UTF-8 conversion. Theme admission remains inclusive at 65,536
bytes and precedes JSON/schema/palette validation. Input is byte-counted,
not character-counted.

Initial files canonicalize/deduplicate in argument order and all load before
terminal entry or dump output. Newly followed files canonicalize and load before
tab/history mutation; already-open canonical targets reuse cached content.
Explicit theme paths canonicalize before loading, including in dump/session
routes. Authored symlinks therefore remain usable. Reload/editor rereads use
the stored opening canonical path without recanonicalizing a substituted final
symlink. Caller-supplied rendering strings and session artifact loading bypass
these standalone document reads.

Reload retains its read/render candidate transaction: failure preserves the
old document, viewport and search, changing only status; success rebuilds
navigation/search and clamps offsets. Failed new file reads do not append or
switch tabs/history. Editor failures intentionally propagate after terminal
restoration to CLI exit 1, not interactive recovery. Successful editing reenters
with retained options; subsequent rendering is not reload's transaction.
Initial source validation likewise does not roll back later rendering/output
failures. CLI errors retain escaped stderr and viewer errors the inert footer.

Observed assertions cover real document minus/exact/plus-one boundaries,
exact theme JSON boundaries, multibyte bytes, zero limits, probe consumption,
an endless reader stopped after exactly 32 bytes at limit 31, chained-prefix
read errors, missing/nonregular inputs and final-symlink rejection. CLI/PTY
assertions cover initial empty output on failure, FIFO/directory rejection,
authored symlinks, retained link/reload usability and recovery, and restored
canonical/echo flags on post-editor input failure.

### Independent comparison and retained findings

A separate source-blind reviewer read local requirements, contract, design,
user documentation, the independent observation and execution logs, excluding
source, tests, manifests, queue, this record, earlier reviews and Git material.
It supported the scoped obligations without establishing a needed code change.
It retained two directly related findings:

- **D-01, historical streaming claim:** The older design said "Memory usage
  proportional to document size (streaming for large files)", contradicting
  whole-file buffering and the new non-streaming boundary. After comparison,
  the parent marked that wording historical/unimplemented rather than an
  active capability claim. The original proposal and finding remain visible;
  no human decision to accept whole-file versus streaming policy is implied.
  The comparison preceded that documentation clarification.
- **D-02, input-policy disposition:** The fixed 16 MiB ceiling is explicitly
  a conservative assumption for this increment, not a human-accepted product
  baseline. Explicit human disposition remains required before claiming
  acceptance. The advisory exception is not a receipt or approval.

Earlier A-01/A-02 findings remain untouched for human arbitration. No blanket
compliance, acceptance or independent security certification is claimed.

The final parent-run logs report 247 passed, 2 ignored with all features and
235 passed, 2 ignored standalone. These are 482 successful executions, not
unique tests; the five helper tests compile into both library and binary.
Both modes include 93 library, five binary and 38 PTY viewer tests. The red
log records seven failures across the earlier library/CLI/viewer runs, including
oversized reload acceptance and FIFO waits. These are historical regression
executions, not current failures or independent test execution.

The observer matched all seven preexecution SHA-256 header entries in each
fresh log to current permitted files. This supports byte identity, not
authenticated provenance, full build-input identity or unchanged files throughout
execution. Theme-boundary assertions in `tests/polish.rs` lack a logged hash.
The red log lacks command/timestamp/hash headers, so it alone does not prove
tests-before-code chronology. Parent tool execution supplies that chronology,
not an independent certification.

Remaining limits include Linux-only operation, ancestor/path races, nonsnapshot
concurrent writes, device-open effects before descriptor rejection, kernel/FUSE
stalls, allocator capacity, aggregate tab memory and rendered expansion.
Rendering/search/editor duration remains synchronous and unbounded by this
per-file source policy. Synthetic endless-reader assertions are not exhaustive
concurrent-growth or environment tests.

Parent verification also passed strict all-target/all-feature Blad Clippy,
formatter/whitespace checks, all-feature workspace checking and structural local
intent validation. No dependency or lockfile change was needed. Scoped
implementer inspection and explicit resource/authority limits are in `DESIGN.md`;
the queue records completed engineering stages, not human acceptance revisions.

## Viewer reliability observation - 2026-10-23

This supplement covers R-053 through R-055. A clean-slate observer inspected
source, tests, manifests and parent-run execution logs without intent, queue,
this record, project documents, previous reviews, chat or Git history/diffs.
Its intermediate reload observation identified a pathname metadata/read race.
A separate clean-slate observer inspected the final reload boundary and
execution evidence after hardening, with the same exclusions. Neither ran
tests or made a compliance determination.

### Independent observed behavior

For R-053, `browser::open` invokes `xdg-open` directly with null stdin and
shared captured stdout/stderr. Capture retains at most 4,096 bytes; each drain
performs at most sixteen 4,096-byte reads. The synchronous wait polls completion
and a monotonic five-second deadline after spawn, with sleeps capped at ten
milliseconds or the remaining interval. Timeout explicitly kills/reaps only
the owned child and returns `TimedOut` with available bounded diagnostics.
Completion performs a final bounded drain rather than awaiting descendant-held
EOF. Drop attempts cleanup on other unfinished paths. Viewer notices cross
the existing inert footer boundary; browser errors retain the document.

For R-054, normal-mode `r` reads and renders a candidate before replacing live
content/document. Success retains rendering options and tab/history structure,
clamps both offsets, rebuilds links/spans, clears link selection and recomputes
the retained search near the viewport. Failure changes the footer without
replacing the previous usable state; missing paths explicitly report reload
unavailable. No watcher or terminal teardown is added.

The first reload used separate pathname metadata and read operations. After
the independent race finding, final code opens with Linux
`O_NONBLOCK | O_NOFOLLOW`, requires the opened descriptor's metadata to identify
a regular file, and reads UTF-8 through that same descriptor. Final FIFO and
symlink replacements therefore fail before content reading. The final observer
confirmed the named flags and direct `libc` dependency. Its test-name criticism
was subsequently corrected to `reload_rejects_fifo_replacements_without_blocking`:
the FIFO is opened nonblockingly, not avoided entirely.

For R-055, the PTY screen stores grapheme strings in leading cells and empty
wide-cell continuations. Combining/ZWJ extensions join the preceding grapheme,
including across SGR; display width determines cursor movement and delayed
wrap. Observed assertions cover CJK, combining accents, joined emoji, exact
emphasis cells, gutter-adjusted mouse links, clipping and resize. The extracted
private terminal guard retains raw/alternate/cursor/mouse ownership and cleanup
across reloads/tabs, with release/re-entry for editing.

### Independent comparison and evidence limits

A separate source-blind reviewer compared current requirements, contract,
design and user documentation with both observations and refreshed execution
logs. It excluded source, tests, manifests, queue, this record, earlier reviews
and Git data. It distinguished the intermediate race/counts from final
descriptor-based behavior and the test-only naming correction. It supported
the scoped R-053/R-054/R-055 obligations without identifying a new concrete
contradiction requiring implementation changes or human arbitration.

The supplied all-feature run reports 227 passed, 2 ignored; standalone reports
215 passed, 2 ignored, including 86 library and 35 PTY viewer tests in each.
These are parent-run results read by the reviewers, not independent execution.
The ignored cases remain optional characterization tests. Reload/Unicode and
final-symlink regressions failed before their corresponding corrections.
Timeout assertions include typing, reaping, flood resistance and usable input
after the production five-second deadline.

Reservations remain explicit: no candidate-render-failure injection or
exhaustive retained-state assertion; replacement tests precede reload rather
than stress concurrent races; no ancestor isolation, inode authentication or
concurrent-write snapshot. Nonregular objects are opened before validation,
so device-open effects are not isolated. Spawn, kill/wait, regular-file reads
and rendering have no hard real-time bound. Browser input remains synchronous,
independent descendants are not managed, and later descendant diagnostics may
be missed. The PTY model implements selected controls and shares production
Unicode libraries; it does not certify every terminal/font or fully emulate
alternate-screen/synchronized-update behavior. Logs alone do not bind results
cryptographically to a source revision.

Earlier A-01/A-02 interpretation findings remain for explicit human arbitration.
Historical unbounded-launcher and scalar-cell descriptions below are superseded
only by this increment. No human acceptance, advisory receipt, blanket
compliance or whole-component security certification is claimed.

Parent verification used both complete Blad feature configurations, strict
all-target/all-feature Blad Clippy, formatting/whitespace checks, an all-feature
workspace check and the root language-vendoring integration test. The direct
edge to already locked/vendored `libc` adds no registry package; local vendoring
metadata was refreshed. Separately appearing lockfile version updates were
preserved, not included as part of this increment's dependency decision.
Scoped implementer inspection and resource/authority limits are retained in
`DESIGN.md`; the queue records the four completed stages without changing
historical acceptance revisions.

## Production viewer observation - 2026-10-23

A separate clean-slate documenter inspected only Blad source, tests, manifests
and fresh execution summaries. It excluded intent, queue, this prior record,
project documents, previous reviews, chat and Git history/diffs. It made no code
changes, ran no additional tests and made no compliance determination. The
following retains its source-derived observations.

### Independent observed behavior

`display.rs::viewer_session` ignores key-release events. Ctrl+C quits even
while help or search is active. Otherwise, help consumes the next key without
applying its document action. Normal navigation includes j/k or arrows,
full-page Space/PageDown/Ctrl+F and PageUp/Ctrl+B, half-page d/u or Ctrl+D/U,
and g/H/Home versus G/L/End. Top commands reset horizontal offset; bottom
commands do not. Left/Right move one display column. `[`/`]` select the
previous/next distinct heading row and clear search/link selection.

`h` displays a rendered help document; Ctrl+L invalidates the paint cache.
Normal q/Esc closes the active tab; Q quits all. Search editing takes
precedence: Q becomes query text, Enter commits, and Esc clears search without
closing. After committing, Esc instead follows normal tab-close handling.
Legacy `f` starts a separate modal link selector: Tab/BackTab cycle, Enter
follows, Esc leaves; ordinary movement and q are ignored there, while Q and
Ctrl+C still quit.

`search.rs` searches each rendered `TerminalLine::plain()`, not Markdown
source. `/` and `?` reset state for forward/backward literal search. Ctrl+R
toggles regex; Ctrl+U clears input; Backspace removes one Unicode scalar.
Queries are limited to 4096 UTF-8 bytes; regex compilation and DFA budgets are
each 2 MiB. Matching is case-insensitive; literal mode escapes regex syntax.
Empty matches are excluded. Regex byte ranges expand to intersecting
graphemes and become display-column spans through
`terminal_text.rs::column_ranges`.

Initial selection uses the first occurrence at/below the viewport top, or
the last at/above it for backward search, wrapping if necessary. n advances
in the original direction; N/p reverse it. Every visible occurrence is
highlighted, not just the selected occurrence. `reveal` adjusts vertical and
horizontal offsets; resize recomputes matches against reflowed lines. Source
unit tests cover Unicode columns, invalid/oversized patterns, and zero-width
exclusion; PTY assertions cover exact highlighted cells and monochrome
underlining.

`links.rs` extracts inline/reference/autolinks and bare HTTP(S) URLs outside
code; formatted labels contribute text. Renderer navigation identities use
parser offsets, with an additional event-local offset for bare URLs. Duplicate
visible labels therefore remain distinguishable. Normal Tab/BackTab initially
choose relative to the viewport top, then cycle source-order links. Enter
follows the selection, otherwise the first source-order link having a span
at/below the top; this is not restricted to currently visible rows.

`viewer_navigation.rs::spatial_link` excludes the selected link entirely.
Left/Right require another span on the same row; Up/Down require a different
row. Candidates rank by row distance, column distance, link index, then span
index. Mouse clicks use rendered half-open spans after subtracting the gutter
and adding scroll offsets.

Fragments resolve through renderer-owned heading rows, including generated
Unicode slugs, duplicate suffixes, and explicit IDs. Empty fragments mean top.
File links split their fragment before percent-decoding and resolve against
the current file's directory. Opening canonicalizes paths, reuses existing
tabs, and reads new files as UTF-8. Missing targets/anchors become footer
messages. Initial files are deduplicated canonically in argument order before
terminal entry/output. Alt+Left/Right wraps tabs outside search editing/modal
link mode; b pops navigation history. Closing repairs history indices. Each
tab retains numeric x/y offsets, but search and selection are recreated on
re-entry. Existing tabs reuse stored content; switching does not reread disk.

`markdown.rs` passes link identities through inline styles and table marks.
`inline.rs` preserves adjacent styled runs, collapses ordinary whitespace into
separators, wraps prose by words/graphemes, and keeps words containing inline
code atomic. `render_writer.rs` measures columns already emitted plus quote
borders; same-link pieces on one row merge into an enclosing span.
Consequently intervening label whitespace is included. Wrapped rows retain
separate spans. `table_navigation.rs` projects marked cell byte intervals
through preserved tokens into display columns; table rendering adds preceding
cell/separator offsets. `viewport.rs` unit tests explicitly check duplicate,
quoted, wrapped, Unicode, projected-text, and math-label identities.

Search and selected-link spans share one overlay. They are sorted and
overlapping or touching intervals merge per row. `TerminalLine` preserves
SGR separately, expands tabs at eight-column stops, and clips whole graphemes;
partial wide graphemes become spaces. Overlay exit restores tracked source
styling. Document layout reserves a scrollbar column and a rendered-line-number
gutter. Code/table overflow remains horizontally accessible.

`TerminalGuard` owns raw mode, alternate screen, hidden cursor, and mouse
capture, with explicit restoration and drop cleanup. `Painter` uses absolute
positioning, synchronized updates, padded changed rows, and no output for
identical frames. Editing restores the terminal before invoking
whitespace-split `$EDITOR` (default nvim), rereads after success, then re-enters;
failures propagate. Browser launching captures bounded diagnostics with null
stdin, but waits synchronously without a timeout.

### Independent evidence and reservations

The supplied all-feature summaries report 215 passed, 2 ignored; standalone
reports 203 passed, 2 ignored, including 27 PTY viewer tests in each. The
observer read assertions and summaries rather than independently executing
tests. It found no contradictory executable source/test assertion. The
ignored tests are existing optional native-diagram and repeated-text
performance characterizations.

The PTY emulator counts printed characters as individual cells; Unicode
geometry additionally relies on source/unit assertions, not realistic PTY
grapheme emulation. A synchronous browser launcher can block indefinitely.
Passing summaries do not establish all untested edge-case behavior.

A separate source-blind comparator read only current requirements, contract,
design, that observation and execution summaries, excluding source, tests,
queue, prior records and Git data. It supported the scoped occurrence search,
localized link spans, spatial/source-order navigation, tabs, retained painting
and pager keys, without universal compliance certification.

It retained two interpretation findings for **explicit human arbitration**:

- **A-01, committed search Esc:** The implementation closes a tab after Enter
  commits search, whereas the contract's "Esc clears/cancels search" can also
  be read as clearing retained results first. Prompt cancellation is separate.
- **A-02, help and Ctrl+C:** The implementation gives global quit precedence
  to Ctrl+C; "any key dismisses" help can be read as requiring dismissal only.
  The competing global-quit clause makes precedence ambiguous.

Neither finding is treated as human-resolved or silently repaired by changing
intent or code. Current user documentation discloses actual mode precedence.
No acceptance, advisory receipt or blanket compliance is claimed.

The comparison also retained evidence reservations for exhaustive mode/key
combinations, inline regex flags, every spatial boundary and resize path,
exact tab-close selection, every write/flush recovery, help contents and
user-documentation obligations. Some are directly covered by parent-run
assertions, but the source-blind reviewer did not inspect source/tests or
user docs and did not convert absent observations into presumed defects.

Parent verification used `cargo test -p blad --offline --locked --all-features`
and `--no-default-features`, strict all-target/all-feature Blad Clippy, formatter,
whitespace checks and an all-feature workspace check. The occurrence-search
and duplicate-table/bare-URL metadata regressions failed before their
corresponding fixes. `kvist vendor . --json` successfully refreshed the local
vendoring metadata after adding the direct edge to already locked `regex`;
no registry packages were added. The scoped implementer input/terminal
inspection and limitations are retained separately in `DESIGN.md`.

## Standalone/session boundary observation - 2026-10-23

An independent clean-slate observer inspected current application source,
manifests and tests without intent, queues, prior records/reviews, project
documents, chat, Git history/diffs or peer implementations. It ran no Cargo
commands and made no compliance judgment. Its source-derived observations:

- `Cargo.toml` declares empty defaults and opt-in `sessions`. `lib.rs`,
  `display.rs` and `main.rs` compile-gate session APIs, display helpers,
  commands and `--log-dir`; general rendering dependencies/options remain.
- The CLI accepts positional files, with `list`, `session` and `export` only
  in session builds. General options can precede commands; mixed file/command
  invocations fail. `--` disambiguates command-like filenames. Help/version
  terminate before theme/file loading; empty invocation requests help.
- `display.rs` canonicalizes and preloads initial UTF-8 file contents before
  output/terminal entry, preserves first-argument order and reuses canonical
  duplicates. Interactive mode requires terminal input/output and non-plain
  settings; other routes render each unique file sequentially. Tabs retain
  opening order, start with the first file, and wrap during Alt-arrow switching.
- `session_display.rs` prints noninteractive snapshots with metadata, message
  labels/dispositions, and an active-session snapshot notice. Export writes
  source Markdown rather than applying render projections. Session directory
  lookup belongs to `session`; export still resolves ID/prefix.
- `browser.rs` invokes `xdg-open` directly with one URL argument and null
  stdin. Nonblocking combined stdout/stderr capture retains 4,096 bytes with
  explicit truncation, bounded drain batches and status-based success/error
  reporting. Collection does not await descendant-held EOF. Viewer footer
  notices are sanitized/clipped and survive repaint/resize until replaced.
- Inspected assertions cover CLI feature surfaces, version, argument ordering,
  unreadable initial files without partial output, non-UTF-8 filenames, stable
  tabs, snapshot routes and fake browser diagnostics. Browser assertions
  include success warnings, nonzero status, null stdin, control-containing
  output, truncation and resize persistence. Socket-level coverage retains an
  open writer while testing bounded nonblocking reads.

The observer also identified limitations: a launcher that never exits blocks
the synchronous viewer indefinitely; the existing session reader silently
substitutes empty bodies for message-read failures, suppresses discovery
errors, and bounds ID lookup to 1,000 listed sessions with first-prefix
selection. Browser code uses Unix APIs and `xdg-open`. The synchronous wait
and existing session-reader behavior are retained, not newly corrected or
certified by this increment; no timeout/live-follow promise is made.

Parent-executed checks cover both feature configurations: 200 tests passed
with sessions and 188 without, with two existing optional characterization
tests ignored in each. Strict all-target standalone and all-feature workspace
Clippy and an all-feature workspace build passed. These are parent-run
results, not execution by the observer.

Scoped implementation audit: compile gates and removed unused dependencies
were inspected; browser execution uses no shell or inherited viewer stdio,
retained output is bounded, diagnostics cross the inert footer boundary,
and initial file errors precede output. An empty-session listing path now
escapes controls, covered by a CLI regression. This is not a whole-component
security certification or a dependency-internals audit.

A separate source-blind comparator read only current requirements, contract,
design and this observation. It supported the scoped feature, CLI, tab and
browser semantics, distinguishing parent-run verification from independent
execution. It raised one contract ambiguity: "each input" could require dump
duplicates while the observed implementation renders unique canonical files.
The human explicitly selected `unique`; requirements/contract now specify
one rendering per distinct canonical file in first-argument order. No code
or tests were silently changed to conceal the discrepancy.

The comparator retained coverage reservations for exhaustive option placement,
every retired alias, per-tab position/back-link combinations, focus events
and document preservation. Existing passing assertions and source evidence
support bounded behavior, not exhaustive terminal/platform certification.
An obsolete `stream_session` design inventory entry was removed. No Kvist
acceptance, advisory receipt or whole-component certification is claimed.

An additional parent regression measured actual filename columns, not just
tab order, and failed because brackets shifted labels by one/two columns.
Inactive tabs now reserve both marker columns. A narrow clean-source observer
inspected only the title builder and regression, confirming equal-width
wrappers and unchanged label starts for the same file list. A separate
source-blind comparator supported fixed-column behavior from that evidence,
without claiming exhaustive link/history, scroll-position or duplicate-tab
coverage. Neither narrow reviewer executed tests.

Final parent workspace verification passed 1,597 tests across 90 suites,
with 14 existing ignored tests; formatter and whitespace checks passed.
The first workspace run exposed the local ignored vendor manifest's stale
lock digest after unused dependencies were removed. `kvist vendor .` refreshed
it offline, then the targeted readiness test and full workspace run passed.

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
