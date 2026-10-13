# Blad: In-depth Review and Implementation Plan

**Date:** 2026-10-12

**Reviewed revision:** `05453dd`, including the existing uncommitted changes to
`src/markdown.rs`.

**Disposition:** Review and proposed implementation work, not accepted component
intent, an implementation record, or a compliance/security certification.

## 1. Executive assessment

Blad has a useful starting point: a small Rust library/CLI, a maintained Markdown
parser, syntax highlighting, and a basic interactive viewer. It is not yet a
first-class viewer. Passing tests currently conceal substantial rendering,
viewport, search, session-reading, and lifecycle gaps.

The central problem is the representation: parser events write directly into an
ANSI string, then the viewer treats that string as both searchable text and
terminal geometry. Source bytes, Unicode characters, terminal columns, logical
lines, and physical screen rows are consequently mixed together. Fixing these
independently with more string manipulation will continue producing regressions.

The recommended solution is a modest, component-local pipeline:

```text
validated source snapshot
  -> Markdown blocks and styled text with source anchors
  -> width-aware layout and table geometry
  -> folded/visible document
  -> clipped terminal frame
  -> terminal output
```

Search uses semantic text and source anchors, not ANSI. Navigation changes typed
viewer state, not rendering internals. Code blocks have a no-wrap layout policy;
overflow is handled by clipping and horizontal navigation, never terminal
autowrap. Streaming updates source snapshots and invalidates affected caches.

**Initial review scope:** This document replaces the old all-completed plan and
links it from the README, fixing the README's nonexistent architecture link.
No viewer bug or new runtime feature is represented as fixed. The existing
uncommitted renderer work was preserved. Requirements, contract, design, queue,
and implementation record were not silently rewritten or marked accepted.
The subsequent targeted layout correction is recorded in section 13.

**Estimate:** Approximately **29-45 focused engineering days** for the complete
plan, plus roughly 20% uncertainty: **35-54 days, or 7-11 working weeks** for one
experienced Rust engineer with timely review. A usable feature-complete
checkpoint can arrive earlier; see section 11 for the breakdown and boundaries.

## 2. Review scope, evidence, and limitations

Read all Blad production sources and tests, its five local artifacts, README,
usage guide, prior plan, workspace/crate manifests, global vision/architecture/
root contract, standards posture, and the supplied root compliance report.
Checked the provider's public `skott/CONTRACT.md` and repository call sites for
the exposed Blad rendering/viewing functions; no peer implementation was used
to infer a transcript contract.

The supplied `../COMPLIANCE_REVIEW.md` reviews the artifact-model redesign for
maerg/sav, not Blad. Its source-blind verdict must not be reused as Blad evidence.
Blad is also absent from the root architecture's component table despite being
a workspace member; that boundary needs explicit reconciliation.

### Checks performed

| Check | Observed result |
| --- | --- |
| `cargo test -p blad --offline --locked` | 22 renderer unit tests and 4 integration tests passed; no CLI/viewer unit tests or terminal integration tests exist. |
| `cargo clippy -p blad --all-targets --offline --locked --no-deps -- -D warnings` | Passed. |
| `cargo fmt -p blad -- --check` | Passed. |
| `cargo build -p blad --offline --locked` | Passed. |
| Public rendering API probes | Confirmed whitespace corruption, width errors, table overflow, missing features, tiny-width panics, and basic incomplete-Markdown behavior. |
| Linux PTY probes | Confirmed top-line loss, ANSI corruption during horizontal movement, ineffective search jumps, stale resize geometry, and tiny-viewport failure. |
| Synthetic session/CLI probes | Confirmed one-shot streaming, outside-session/link reads, silent missing bodies, ANSI in file dump, and success exit codes on missing inputs. |

PTY checks used explicit window sizes and a small local VT interpreter for the
controls this viewer emits. It is supporting reproduction evidence, not a
complete terminal emulator or a guarantee across terminal applications. ASCII
heading/code/search probes avoid ambiguous Unicode display-width assumptions.
Future committed tests should use a maintained, development-only VT oracle.

Probe programs and output were retained in this review session's `files/`
directory, outside the repository. They used temporary synthetic session files,
not private user transcripts. No dependencies were installed. Timings below
are local smoke measurements, not statistically controlled benchmarks.

### Particularly useful reproductions

| Input/action | Actual behavior |
| --- | --- |
| `COMPLIANCE_REVIEW.md`, 80 columns x 24 rows | Initial document title disappeared; physical output scrolled three rows while footer still said `L1-22 ... TOP`. |
| Long ASCII heading, 30 x 10 | Physical screen scrolled four rows during initial draw; title start disappeared. |
| `First paragraph.\n\nSecond paragraph.`, render width 40 | Plain rendered result: `"First paragraph.\n\n Second paragraph.\n"`. |
| `a**b**c and ` followed by inline code `x` and `y.`, width 40 | Result: `a b c and x y.` instead of `abc and xy.`. |
| `one **two** three`, width 40 | Extra space before the emphasized word. |
| Hard line break before `Second line.` | Second line gained a leading space. |
| Rule `---`, width 20 | An 80-column rule was emitted. |
| Two-column table, width 30 | A 31-character header line was emitted; column sizing also allowed a width-3, height-3 cell. |
| 67-column fenced code line, 30 x 10 | Renderer kept one code line, but viewer physically wrapped it over multiple rows. |
| Right-arrow ten times on highlighted code | Literal fragments such as `[38;2;192;197;206m` appeared; short lines reset the shared horizontal offset. |
| Three matching paragraphs, two `needle` occurrences each | Count was 3 rather than 6; `n`/`p` did not move the viewport; match count disappeared after Enter. |
| Resize 30 -> 50 columns | No immediate redraw; next key still used the old geometry/layout. |
| Render widths 0 and 1; PTY 1 x 1 | Arithmetic-overflow panic; PTY process exited 101. |
| `blad view <missing-file>` | Diagnostic printed, but exit status was 0. |
| `blad --dump view blad/test_wrap.md` | ANSI SGR sequences still present. |
| Active synthetic session, `stream` | Printed one snapshot and exited with “re-run to see updates”. |
| Synthetic `../outside.md` or message symlink | Loaded the outside-session fixture; missing body became an empty message. |

The missing-lines defect is conditional, not a parser that always drops the first
N source lines: a short heading in a sufficiently wide viewport survived.
Rendering-only tests cannot prove the screen retains those lines.

## 3. Findings and dispositions

Priorities: **P0** = correctness/safety blocker for promotion, **P1** = required
viewer capability or substantial reliability defect, **P2** = quality/maintenance
gap. These are engineering priorities, not independent security-audit ratings.
Locations are relative to `blad/` at the reviewed worktree revision.

| ID | Priority | Evidence/location | Finding and required action |
| --- | --- | --- | --- |
| F01 | P0 | `src/display.rs:202-255` | Unbounded header/content/footer writes permit terminal autowrap; logical row counting then loses the top of the document. Explicitly position and clip every screen row. |
| F02 | P1 | `src/markdown.rs:194-226,351-359,803-838` | Column state survives paragraph/heading/hard-break boundaries. Event-local splitting invents spaces at inline boundaries and duplicates trailing whitespace. Replace with block-aware whitespace and line-breaking state. |
| F03 | P1 | `src/display.rs:144-154,224-250`; `src/markdown.rs:67-70,178-192,254-264,803-825` | Width budget has arbitrary margins, omits number gutter, excludes headings, miscounts inline code/breaks, and ignores separator spaces before testing fit. Compute one explicit content rectangle and measure all text in display columns. |
| F04 | P0 | `src/display.rs:245-255,402-413` | Horizontal slicing cuts through ANSI and counts escape bytes as content. Offset is clamped separately by each line and output is not right-clipped. Home/End currently move vertically, contrary to the requested behavior. Clip styled cells, use one document-level offset, and change key semantics explicitly. |
| F05 | P1 | `src/markdown.rs:93-121`; `src/display.rs:250-253` | Code renderer does not wrap, but the screen does. Preserve code whitespace/line structure, lay it out without wrapping, and clip it through the same horizontal viewport as tables. |
| F06 | P1 | `src/markdown.rs:381-596` | “4:3” is only a comment. Separator budget charges 2 characters but writes ` \| ` (3). Fitting is biased by sequential mutation of remaining space; minimum widths/long words can overflow inconsistently. Use measured cells, actual border overhead, and an explicit ratio-constrained solver. |
| F07 | P1 | `src/markdown.rs:141-151,178-185,266-299,517-554` | Table event chunks get invented spaces, inline styles are lost, alignment metadata is discarded, and multiline rows have no horizontal cell boundaries. Preserve cell runs/alignment and draw a row grid for multiline tables. |
| F08 | P1 | `src/display.rs:170-196,233-240,276-285,347-441,481-489` | Search stores matching lines rather than occurrences; every `scroll_to_match` return is discarded. Whole-line highlighting is overridden by Markdown resets; footer count exists only during input. Implement occurrence ranges, persistent current-hit state, and composited highlights. |
| F09 | P1 | `src/display.rs:345-469`; renderer output model | No section tree, folding state, or Tab/BackTab actions. Add heading-derived sections; do not infer headings from rendered `#` text/code. |
| F10 | P0 | `src/markdown.rs:70`; `src/display.rs:153-154,246,265,302` | Small dimensions underflow; zero-height viewports and empty documents lack defined semantics. Use validated geometry and saturating/checked transitions, with a deliberate tiny-screen mode. |
| F11 | P1 | `src/display.rs:136-154,468` | Dimensions and rendered lines are immutable; resize events are ignored. Reflow at new width and preserve source position, search selection, folds, and valid horizontal offset. |
| F12 | P0 | `src/display.rs:163-167,208-210,445-478` | Recoverable terminal/output errors panic; teardown is manual and absent on several exits. Editor launch leaves cursor restoration incomplete; event-read errors become silent success. Add an acquisition-aware RAII terminal guard and explicit error propagation. |
| F13 | P1 | `src/display.rs:458-466` | Mouse events are handled but mouse capture is never enabled. Enable it only while the guard owns the interactive terminal; disable on all teardown/suspend paths. |
| F14 | P1 | `src/display.rs:65-112` | Character-device metadata is not a TTY test (`/dev/null` qualifies). Width-taking viewer ignores its argument, tests only stdout, renders once, then parses rendered ANSI as Markdown again. Use `std::io::IsTerminal`, require interactive input/output, and remove double-rendering. |
| F15 | P1 | `src/display.rs:491-512` | Empty `$EDITOR` can panic via `args[0]`; whitespace splitting mishandles quoted arguments. Post-edit flow recursively calls the viewer with rendered text and loses options/state. Suspend/resume the same viewer around a validated shell-free editor invocation and reload original Markdown. |
| F16 | P1 | `src/display.rs:14-60`; `src/main.rs:143-175` | `show` never enters the interactive viewer; `stream` never follows updates, and `--dump` is ignored on the stream route. Route file/session views through the shared model with explicit snapshot/follow semantics. |
| F17 | P0 | `src/session.rs:25-97,112-164,189-225` | Generic JSON silently defaults required fields, ignores format/media/completion/truncation metadata, suppresses read errors, and trusts paths. Reads follow links and accept absolute/parent paths; input sizes are unbounded. Define typed schema/error/limit contracts and confined session reads. |
| F18 | P1 | `src/session.rs:102-106,168-185` | Lookup scans only the newest 1000 sessions and silently chooses an ambiguous prefix. Mixed timestamp/ID comparison is not a total ordering; equal timestamps retain filesystem ordering. Separate lookup from listing, use one total sort key, and reject ambiguity. |
| F19 | P1 | `src/markdown.rs:34-42,223-226,243-252,301-376` | Claimed “full CommonMark/GFM” is not established: ordered numbers and task markers disappear; raw HTML is discarded; ordinary quotes lack structural markers; links lose destinations; math parser option is not enabled. Define a testable supported profile, then implement/preserve omitted content. |
| F20 | P1 | `src/markdown.rs:44-46,123-139,599-621` | Boolean formatting state cannot represent nested emphasis correctly; resets also erase outer heading/quote styles. Use a style stack and serialize composed style only at the terminal boundary. |
| F21 | P1 | `src/markdown.rs:31-33,93-121,697-736`; `src/display.rs:14-28` | Every render reloads syntax/theme assets; sessions repeat that per message. Rendering buffers whole documents/code blocks and Mermaid starts a blocking, unbounded external process. Cache immutable assets and separate expensive/optional effects from pure rendering. |
| F22 | P0 | `src/markdown.rs:141-176,733-770`; `src/display.rs:220,252`; `src/session.rs:213` | Raw file/code/metadata controls can reach terminal output. ANSI is not a trustworthy text representation or sanitization boundary. Escape document controls visibly; only renderer-owned terminal controls may be emitted. |
| F23 | P1 | `src/display.rs:32-48,118-126,602-613`; `src/main.rs` | File dump still emits ANSI; session dump emits raw Markdown, giving inconsistent semantics. Missing inputs produce exit 0; output failures panic. Define separate rendered plain dump vs Markdown export, deterministic widths, and explicit exit/error policy. |
| F24 | P1 | `src/display.rs:518-558` | Export truncates an existing destination before successful publication and can report incomplete data after suppressed message-read failures. Validate first and publish through a bounded atomic/no-clobber contract. |
| F25 | P1 | `src/markdown.rs:697-736`; `tests::test_mermaid` | Successful Mermaid rendering discards SVG and returns a success-shaped placeholder, not a diagram. Tests invoke host `mmdc` nondeterministically. Default to source display; treat real rendering as an explicit bounded optional adapter. |
| F26 | P1 | `src/markdown.rs:845-1142`; `tests/integration.rs` | Tests mostly assert substring presence, not exact text/styles/geometry. “No-wrap code” test uses short code; table tests do not check width/ratio; equal-timestamp sorting fixture does not test distinct dates. Add exact/unit/property/PTY/failure-path evidence. |
| F27 | P2 | `src/markdown.rs` (1142 lines), `src/display.rs`, `src/session.rs`, `Cargo.toml` | Rendering is a long mutable-state match; display mixes session composition, state/events, subprocesses, terminal I/O and export. Manifest parsing/labels are duplicated. Several direct dependencies are unused by production source; `tempfile` is test-only. Extract actual seams and audit dependency cost before adding crates. |
| F28 | P1 | Local intent/queue/record, README/usage; root architecture | Documentation/queue declares capabilities and reviews complete without sufficient current evidence. Queue lacks versioned definitions, explicit dependency chains and qualified locators; `IMPL.md` is not current evidence of the new review. Reconcile by a fresh approved plan and independent evidence, not by rewriting intent to bless bugs. |
| F29 | P2 | `CONTRACT.md`, `docs/USAGE.md`, `README.md`, `src/main.rs` | Positional export destination is documented but rejected; `--line-numbers` is undocumented; u/d full-page behavior differs from half-page requirement. Smart routing is suffix-driven, empty invocation behavior is inconsistent, and README architecture link was broken. Establish one tested CLI/key reference. |

### What can be fixed quickly, and why runtime fixes are deferred here

Discarded search return values are a small mechanical fix. Paragraph-boundary
resets, the separator-overhead typo, `IsTerminal`, and enabling mouse capture
are also small edits in isolation. They are good early regressions, but they do
not fulfill the requested behavior by themselves:

- Resetting paragraph columns does not preserve inline event boundaries.
- Correcting one table separator does not enforce the aspect ratio or prevent
  terminal autowrap.
- Assigning search offsets does not count/highlight individual hits or map them
  after wrapping/folding.
- Disabling terminal autowrap without Unicode-aware clipping can hide data or
  corrupt wide glyphs rather than solve geometry.

Do those fixes within the test-first stabilization packets below, not as an
unverified “viewer fixed” patch. The existing uncommitted wrap work updates the
returned column and continuation indent, but the review reproductions show
that it does not yet resolve paragraph/inline/geometry defects.

## 4. Proposed outcomes and decisions

These are proposed product decisions for later acceptance, not new current CLI
guarantees. Put normative outcomes in `REQUIREMENTS.md`, consumer semantics in
`CONTRACT.md`, and the private realization in `DESIGN.md` before implementation.
Use new stable IDs without renumbering unrelated existing requirements.

| Proposed ID | Acceptance outcome | Existing traceability |
| --- | --- | --- |
| R-026 | First logical content appears at the first content row; no draw scrolls the terminal or overwrites chrome. | R-006, R-012, R-013 |
| R-027 | Prose wraps at the measured content width, preserving inline adjacency, normal Markdown whitespace and hanging indentation. | R-014, R-015 |
| R-028 | Tables use measured column sizes, preserve alignment, satisfy the defined 4:3 cell ratio, and have multiline row borders. | R-015 |
| R-029 | Code and overwide tables are never viewport-wrapped; Left/Right and Home/End expose their complete lines. | R-009, R-016 |
| R-030 | Every search occurrence is highlighted; current occurrence, counts, forward/backward movement and reveal behavior are correct. | R-011 |
| R-031 | Heading sections can be collapsed/expanded and navigated with Tab/Shift+Tab without data loss. | R-006, R-009 |
| R-032 | Partial/streaming Markdown is safe and updates correctly; following has explicit pause/resume and completion semantics. | R-003, R-020, R-021 |
| R-033 | Tiny screens, resize, editor suspension, input/output errors and cancellation preserve terminal ownership and actionable errors. | R-008, R-021 |
| R-034 | Session reads validate format, paths, regular files, ordering, completion metadata and resource limits. | R-005, R-022, R-023 |
| R-035 | Raw controls are inert; plain dump/export have distinct consistent output and failure semantics. | R-004, R-007, R-021, R-024 |
| R-036 | Pure layout/state, CLI, PTY, failure-path and performance tests substantiate a precisely named Markdown profile. | R-014, R-015, R-020 |

### Resolve these ambiguities explicitly

| Topic | Conservative proposed decision |
| --- | --- |
| Meaning of 4:3 | Terminal display columns versus content rows, excluding border/padding; apply to the full rectangular cell height after row padding, not just its nonblank text. This can expand short-value columns next to tall descriptions. Pixel geometry is not portable and is not inferred. |
| Table cell newlines | GFM physical source newlines do not create multiline cells. Support safe `<br>`, `<br/>`, `<br />` as explicit cell breaks; other raw HTML is displayed inertly or given a documented safe text representation, never executed. |
| No-wrap code | Fenced/indented block lines never soft-wrap; atomic inline-code spans also retain internal whitespace and are not split by prose/table wrapping. Tabs are visualized at fixed documented tab stops; source/export bytes remain unchanged. |
| Search | Literal, case-insensitive occurrence search over displayed semantic text, excluding formatting syntax/decorative borders. No regex in the first version. Unicode lowercase mapping retains original offsets; normalization/full case-fold equivalence is a separately documented future decision. |
| Search boundaries | Search spans a prose soft break/layout wrap, but not a block boundary, separate table cells, or separate code source lines. All occurrences, including those in collapsed bodies, count. Non-overlapping hits in source order. |
| Folding | Heading sections only initially (H1-H6). Do not add arbitrary paragraph/code/table folds without a later requirement. Heading remains visible; fold hides its body and descendant headings. |
| Section keys | Tab/Shift+Tab select next/previous visible heading; Space toggles focused section. Nearest section governing the top content anchor is the fallback focus. No section means a visible non-error status, not accidental scrolling. |
| Horizontal keys | Home/End become beginning/end of horizontal content; retain g/G and H/L for vertical top/bottom. Left/Right move one display column. Offset is global to the document, never reset by an individual short line. |
| Page keys | PageUp/PageDown move a full content page; u/d move half a page, at least one row. Resolve existing contract/requirement mismatch intentionally. |
| Initial/follow position | Ordinary views start at the top. Follow mode starts at the live tail; scrolling/search/folding away pauses tail-follow, G resumes it. New content cannot drag a reading user back to the bottom. |
| Dump | `--dump` is rendered, ANSI-free text; plain output is deterministic and unwrapped unless an explicit width is requested. `export` is source Markdown with session framing. Dump always expands folds. |
| `stream --dump` | Finite snapshot, consistent with “output ... and exit”; live follow is interactive. State this explicitly in help/docs rather than silently ignoring the flag. |
| Export replacement | Default no-clobber; a separately approved `--force` permits atomic replacement. Do not introduce it before the contract is accepted. |
| State persistence | Search/folds/scroll are ephemeral per-viewer state. No new session storage format or durable preferences are needed for these features. |
| Limits | Explicit, typed input/query/table limits and diagnostics; no silent clipping, empty bodies, fallback acceptance, or unbounded external renderer invocation. Choose numeric defaults in Q01 using provider limits and performance measurements. |

The public provider contract does not currently specify the `session.json` +
Markdown-body format assumed by Blad with enough precision for live consistency.
Its public history guarantees do require non-linked regular paths and bounded
reads (currently 5 MiB per file and 4096 directory entries). Do not copy provider
internals or infer an atomic multi-file publication protocol. Add a
provider-owned versioned transcript contract/schema with exact paths/dialect
and declared consumer relationship before optimizing live reads around it.
This is a cross-component acceptance dependency, not permission to alter peer
source during a Blad task.

## 5. Structure and Rust implementation approach

Stay in the existing Blad component. Suggested private module seams below are
not new Kvist components, plugin systems, or a generic UI framework.

| Area | Owns | Must not own |
| --- | --- | --- |
| `main.rs` | Clap setup and dispatch to a fallible command entry point; exit-code presentation | Markdown parsing, duplicated session resolution, terminal lifecycle |
| `session.rs` | Typed manifest/messages, confined bounded reads, total listing order, unambiguous resolution | ANSI, screen state, swallowed I/O failures |
| `markdown.rs` / parser module | Parser options, block/style stack, heading structure, searchable text and source anchors | Terminal commands, viewport state, subprocess invocation |
| `layout.rs` | Grapheme/display-column metrics, prose breaks, code line preservation, block-to-visual-line mapping | File reads, key handling, raw ANSI slicing |
| `table.rs` | Cell wrapping, alignment, ratio-constrained widths/row heights, borders | Global terminal width wrapping after table sizing |
| `viewer.rs` | Pure state transitions: x/y, focus, folds, search selection, follow mode and resize anchors | Direct stdout writes, source schema validation |
| `search.rs` | Original-offset-preserving query matching and occurrence/source-anchor mapping | ANSI stripping, terminal painting, document mutation |
| `terminal.rs` | Terminal guard, events, clipped frame painting, style serialization and cleanup | Markdown parsing or session schema |
| `display.rs` | Compose file/session documents and coordinate the pipeline, dump, editor suspension and export | A second independent renderer or search implementation |
| `follow.rs` (if needed) | Bounded snapshot polling, update classification and follow invalidation | New persistent session format or execution authority |

Start with a few real modules, extracting additional files when a seam earns
its existence. Keep production files below roughly 1000 source lines; move tests
to focused modules/integration files rather than splitting arbitrary chunks.

### Core data and invariants

Use component-owned types along these lines, with small documented interfaces:

- `DocumentRevision`, `BlockId`, `SectionId`, `SourceAnchor`, `SourceSpan`.
- `Block` enum: heading, paragraph, list, quote/alert, code, table, rule,
  footnote, inert HTML/math representation.
- `StyledRun { text, style, source }`; styles compose by stack, never embedded
  ANSI. Table cells contain runs, alignment, and explicit breaks.
- `VisualLine` contains styled segments, measured width, source mappings,
  block identity, and `WrapPolicy`.
- `Viewport { columns, rows, gutter, content_rect }`, with a defined valid
  zero-content case, and bounded `ScrollPosition`.
- `SearchHit` contains original semantic range(s) and current layout positions;
  `SearchState` distinguishes inactive/input/committed/no-results.
- `ViewerState` distinguishes tail-follow from reading, tracks focus/folds, and
  owns navigation indices without borrowing a mutable terminal.
- `BladError` distinguishes malformed/unsupported schema, ambiguous session,
  incomplete snapshot, path/size violation, read/write/editor/terminal errors.
  Use typed status/diagnostic results where a live transient is recoverable.

Use the parser's source-offset iterator where applicable. Text transformed by
entities, Markdown syntax removal, whitespace normalization and Unicode
lowercasing needs an explicit semantic-to-source map; parser event offsets
alone are not sufficient.

Cache immutable syntax and theme assets in a reusable renderer context or a
fallible once-initialized cache. Cache highlighting by code content/language;
width changes do not change code tokenization. Avoid per-word `Vec` allocations,
repeated string lowercasing/ANSI stripping, and linear match-list membership on
every painted row. Use iterators and indexed ranges where the representation
allows them; optimize against measurements, not speculative data structures.

Prefer `Result` to `unwrap`/`expect` for all recoverable failures. Unwraps in
assertion/setup code are not equivalent to panics on user input. Represent
geometry and transitions explicitly rather than adding unchecked casts.
No async runtime or background worker is needed initially: event polling and
bounded file refresh can share one owner. Introduce workers only if measured
latency warrants them, with revision-tagged immutable results and documented
shutdown/cancellation ownership.

Reuse `tabled` only if it can expose the required measured per-column wrapping,
alignment and row borders without a second global width transform. Otherwise
the small cell/border renderer belongs in Blad. Audit unused direct dependencies
(`sav`, `serde`, `thiserror`, `tracing`, `chrono`, and `tabled` are not all used by
current production source); some become justified by this design. Move
test-only `tempfile` to development dependencies. Any new Unicode/VT support
crate needs capability, maintenance, license and transitive-cost review.

## 6. Layout, viewport, tables, and code

### One measured viewport

Header is row 0; footer is the last row; content is between them. Reserve one
scrollbar column when geometry permits and account for the optional line-number
gutter before laying out prose/tables. With terminal width `C`, gutter `G`, and
scrollbar reservation `S`, content width is `C - G - S`, checked/saturated at
zero. There is no hidden extra two-column margin.

Tiny terminals show a clipped status or as much content as fits without
underflow. Empty documents remain navigable and quittable, with no `L1-0`
position or misleading bottom state. Define zero-width layout behavior before
calling line breakers.

Every row is painted at an explicit coordinate with at most its rectangle's
display columns. Header paths, queries and footers are sanitized and clipped
too. Draw the scrollbar at its own coordinate before each glyph; never rely on
the cursor position left by text/footer output. A nonempty scrollable document
gets at least a one-cell thumb; top and bottom map to the ends of the track.

Clip styled segments by display-column interval `[x, x + width)`. Never cut an
escape sequence or grapheme. When a slice intersects half of a wide grapheme,
paint padding for the intersected columns rather than a partial glyph.
Zero-width combining marks remain attached to their base grapheme. Tabs use
documented display tab stops. After each row, renderer-owned attributes reset.
Terminal autowrap can be disabled as a guarded secondary defense, but clipping
and explicit row positioning are the correctness boundary.

The maximum horizontal offset derives from the full laid-out document width
minus content width, not from each visible row. Short rows just paint blanks
at large offsets. Home sets x=0; End sets x=max; vertical navigation retains x.
Resize clamps x once against the new document width and reflows only blocks
whose wrap policy depends on width.

### Prose and inline whitespace

Normalize whitespace according to Markdown block semantics, preserving the
distinction between soft/hard breaks and literal code whitespace. Emit a pending
space only when the source contributes one, not because a new `Text` event
starts. Maintain one line-break state across emphasis/link/code events.

Fit checks include separators, prefixes and continuation indentation. Each
paragraph/block starts with its own layout state; hard breaks reset its column.
Lists retain ordered starts/task markers and hang continuations at the actual
text start; nested list and quote prefixes count in the width budget. Headings
wrap as styled prose so a long heading does not steal physical screen rows.
Normal prose can emergency-break an overlong non-code token at grapheme
boundaries, never adding an initial empty line. Atomic inline code may overflow
and is exposed horizontally rather than reformatted.

### Table sizing: enforce the ratio, not just a comment

Measure widths in terminal display columns; preserve cell styles, alignment and
explicit breaks. Let `w_j` be column j's content width and
`h(r,j,w_j)` the number of wrapped content lines in that cell.

Rows are rectangular, so:

```text
H_r = max_j h(r,j,w_j)
For every cell (r,j): 3 * w_j >= 4 * H_r
```

Use checked integer arithmetic, not float approximations. This is deliberately
stricter than checking each cell's nonblank lines: a short cell padded to the
height of a long neighbor still has that row's height.

With one space of left/right padding and a shared border between columns,
border/padding overhead for n columns is `B = 3*n + 1`. Table width is
`B + sum(w_j)`. Compute it from the actual chosen border format; do not
charge a different separator width than is drawn.

**Deterministic feasibility algorithm:**

1. Measure hard lines, graphemes, natural widths and atomic-span minimum widths.
   Empty text has one line; empty columns still get a ratio-valid width.
2. Consider candidate maximum row heights `H`, starting at the greatest
   unavoidable explicit-break height. For each column, find the least width
   allowing all its cells to wrap to at most H lines, also enforcing atomic
   minima and `w_j >= ceil(4*H/3)`.
3. For the fixed line breaker, prove/test that its line count is nonincreasing
   with width before using binary search for those minima. Do not assume this
   for an arbitrary third-party wrapping algorithm.
4. A candidate with `B + sum(w_j) <= content_width` is feasible. Prefer the
   smallest feasible H (lowest worst row height); use remaining space to prevent
   additional wraps, with deterministic column-index tie-breaking.
5. If no feasible candidate fits, choose the lowest-total-width valid candidate,
   then lower actual maximum height, then deterministic width-vector order.
   The whole table is now a no-viewport-wrap block and horizontally scrollable.

This formulation tests fitting across all columns together. Given any valid
width assignment with maximum row height H, its widths are at least the
candidate minima for H; thus exhaustive candidate evaluation does not claim
overflow when a fitting assignment exists.

Bound candidate evaluation: a valid natural-width baseline supplies an upper
bound on total width; any better candidate must satisfy
`n * ceil(4*H/3) <= best_total_width - B`. Fitting candidates can additionally
use the terminal budget bound. Cache wrapping measurements and consider line
count breakpoints rather than repeatedly laying out every cell at every width.
Large-table limits and solver work budgets must be explicit. If a solver budget
is exceeded, surface the condition and choose a verified ratio-valid,
conservative overflow layout; never falsely claim optimal fit or discard cells.

Use a grid with top/header/bottom edges; add horizontal separators between every
body row when any row is multiline. Shared edges/junctions must remain aligned
after clipping. A compact single-line-row presentation may omit body separators
but uses the same measured overhead. Unicode borders match current project use;
an ASCII fallback must have identical geometry if provided.

**Concrete ratio case:** If one row requires 6 lines, every column's content
width is at least 8. Three such columns require at least `24 + 10 = 34`
terminal columns with this grid. A 30-column content viewport must overflow,
not squeeze any column below 8 or rewrap the assembled table.

### Code

Fenced/indented code is represented as original logical code lines. Preserve
spaces, tabs, blank lines and unknown-language content; highlighting failure
is a visible, bounded plain-code fallback, not data loss. The final code line
must not disappear when a fence or input ends mid-line.

No viewport-dependent prose breaker or table-wide transform runs on code.
Highlight once; clip to the horizontal rectangle at paint time. Dump without an
explicit width preserves the full line. Escaping unsafe control characters is
an intentional safety transformation, not whitespace reformatting.

## 7. Search, section navigation, and folding

### Search state and hit mapping

Build a searchable semantic text index with block/cell/code-line boundaries.
Compute lowercase query once and maintain a normalized-text-to-original-range
map for Unicode expansion. No ANSI stripping is necessary because ANSI is not
present in the document model. Keep hit identities in source order, not visual
line numbers, so reflow does not redefine a “hit”.

`/` enters input; typed changes update hits and show a preview selection.
Enter commits query/highlights; Esc cancels the preview and restores the prior
committed search. A committed zero-result query stays visible as `0 matches`.
An empty query clears matches deliberately. Bound query length with a clear
status, and edit by grapheme rather than leaving a detached combining mark.

`n` selects next occurrence; `p` and `N` select previous. Navigation cycles at
ends and indicates wraparound; a single hit remains selected. Footer retains
`current/total` after Enter and during normal scrolling. Match color is applied
as a style overlay after Markdown/syntax styling, with the current hit distinct
from other hits. Highlight only matched ranges, including split spans across
wrapped lines; ANSI resets in source styling cannot defeat it.

Selection reveals hidden ancestors and adjusts both x and y enough to show the
hit, with a bounded/clamped center preference. Incremental layout, resize,
editing and appends remap source anchors; a deleted match chooses the nearest
remaining hit or reports no results. Never leave a stale index into old lines.

### Heading-derived section tree

A section starts at a real Markdown heading and ends immediately before the
next heading of equal or shallower level, or EOF. Its parent is the nearest
preceding shallower heading; skipped levels are legal. Preamble text has no
section and always remains visible. Heading-like code/literal HTML is not a
section. Duplicate heading text still gets distinct IDs.

A collapsed section retains its heading/fold indicator and hides all body and
descendant content. Nested collapse choices remain recorded when an ancestor is
expanded again. Tab/Shift+Tab cycle through visible headings and move focus/
viewport there; descendants of a collapsed section are skipped. Space toggles
the focused or nearest governing section. The focused heading is visibly
distinguished from search highlighting.

Search counts hidden hits and selecting one expands its ancestors. Those
expansions remain until the user folds them again; ordinary query cancellation
does not silently re-hide content the user reached. Folding changes visible
line mappings but not source identity or dump/export. Clamp scroll to a valid
anchor if it lies in a newly hidden body.

Appending source generally preserves heading identities via source anchors.
For editing/replacement, remap unchanged headings using source-change mappings
and heading/parent identity, not title alone; if remapping is ambiguous, reset
only affected fold state visibly rather than transferring it to another
identically titled section.

## 8. Streaming and incomplete Markdown

Separate two concerns: **rendering an incomplete snapshot** and **following
changing session artifacts**. Today's parser can produce output for an open
fence/table/emphasis, but there is no live refresh mechanism or verified state
preservation.

### Correct snapshot rendering first

Feed the actual prefix to the parser; do not append fake closing fences,
emphasis, list items, table cells, or links. Unresolved constructs should remain
readable literal/provisional content where the parser dictates. Parsed
end-of-block events at EOF are not proof that the source supplied a closing
fence. Track source completeness separately.

Appending text can change earlier structure: setext headings, a table delimiter
following a paragraph, list continuation, later reference-link definitions and
footnotes. A “reparse only the last line” policy is incorrect. Begin with
bounded full-document reparsing on debounced updates and cached syntax/layout
resources. Adopt incremental parsing only with dependency-aware invalidation
and equivalence tests against full parsing. Do not promise constant-time append
handling for general CommonMark.

At a byte-oriented input boundary, retain a split UTF-8 suffix until the next
chunk. Distinguish incomplete encoding from invalid stable bytes; use an explicit
diagnostic and approved escaped/lossy display policy for the latter. At clean
EOF, incomplete bytes require a final visible disposition, not silent removal.

### Follow loop and snapshot consistency

Use a bounded event poll/file refresh cadence (initial target 50 ms) and coalesce
updates, with input/quit priority. Detect append, rewrite, truncation, replacement,
message completion, session completion and manifest growth. Deletion/read
permission loss/unsupported schema must become explicit status or terminal
errors; retain the last good view only with a visible stale/incomplete indicator.

Validate the provider's published snapshot protocol. If publication is not
atomic across manifest and bodies, independently reading a manifest twice does
not prove coherence. Respect declared generation/completion/byte-count rules,
retry bounded transient inconsistencies, and surface persistent violations.
Do not assume `complete: true` means immutable until the provider guarantees it.

A document revision invalidates search/layout mappings, not the entire viewer
lifecycle. Tail-follow repositions only if explicitly active; reading mode keeps
the anchored text stable. Completion stops source polling after a final
consistent snapshot but leaves the interactive view open. No duplicated message
headers/content on repeated refresh. Structured message completion/truncation
markers remain visible and never mean canonical evidence.

Optional Mermaid/image/math rendering must not block initial display or every
partial update. Default Mermaid behavior should preserve source. A real external
renderer needs explicit opt-in, bounded input/output/deadline, non-network
authority, injected testing, cancellation/reaping, and no success placeholder.
That adapter is outside the feature-complete viewer milestone unless separately
approved.

## 9. Test and quality strategy

“Tests for everything” should mean coverage of specified behavior, transitions,
error paths and cross-feature invariants, not a percentage or a substring test
for every feature name. Write failing cases from accepted intent before code.

| Layer | Required cases/assertions |
| --- | --- |
| Markdown model | Exact text, nested style stack, links/images, inert HTML, quotes/alerts, ordered/task lists, tables/alignments, footnotes, math profile, fences/unknown languages, parser-offset mappings. |
| Whitespace/wrapping | Adjacent styled spans, punctuation, entities, inline code, soft/hard breaks, block resets, hanging indentation, long tokens, trailing spaces, widths 0/1/2/exact boundary, gutter/scrollbar combinations. |
| Unicode/control safety | CJK, combining accents, emoji/ZWJ, tabs, CR/LF, ESC/CSI/OSC/BEL/backspace, invalid and split UTF-8; no control injection and no cut graphemes. |
| Table geometry | Single/multiline cells, `<br>`, empty/ragged inputs, many columns, mixed row heights, alignment, long/atomic tokens, integer overflow bounds, all border junctions. Assert every padded cell `3*w >= 4*row_height`; assert fitting where mathematically feasible. |
| Code geometry | A source line substantially wider than 20/40/80 columns; exact preserved whitespace/blank lines; horizontal first/middle/end slices; no added newline through layout; styled/unstyled equivalence. |
| Pure viewer state | x/y clamps, page/half-page moves, empty/tiny screens, resize, scrollbars, all key modes, key press/repeat/release handling, mouse/focus transitions, fold/section boundaries and nested state. |
| Search | Multiple hits per line, queries across style/wrap boundaries, Unicode lowercase offset mapping, no-hit/empty input, cancel vs commit, cyclic forward/backward, hidden hits, x/y reveal, resize/edit/append/delete mappings. |
| Session/CLI | Valid/invalid/unknown schema, ambiguous prefixes, >1000-session lookup, distinct/equal/missing dates, complete/truncated markers, no-link/path confinement, growth limits, missing/invalid bodies, exact help/exit codes, dump/export semantics. |
| Terminal/PTY | Tagged review fixture, long heading/path/footer, code/table overflow, line-number gutter, resize, Tab/BackTab, search overlays, mouse capture, editor suspension, Ctrl-C/quit, injected setup/output/event errors; terminal state restored. |
| Streaming | Every legal Unicode prefix and byte-chunk boundary, open constructs, later references/table recognition, completion, partial JSON/body publication, rewrite/truncate/delete, paused vs tail-follow, rapid bursts and cancellation. |
| Publication/subprocess | Export failure before/after temporary write, existing destination and links, broken pipe, empty/quoted editor settings, missing editor/nonzero exit; optional renderer absent/failure/timeout/output bound with injected fake process. |

Use a deterministic in-memory screen backend for state/layout snapshots and
Linux PTYs with a VT oracle for actual terminal protocol. PTY assertions must
check visible screen rows and terminal state, not merely bytes containing the
title. Isolate environment/editor/renderer dependencies and avoid global
environment mutation in parallel tests.

Property/invariant tests should verify:

- No fitted prose row exceeds its content budget; intentional overflow blocks
  remain no-wrap and are clipped at paint time.
- No frame write escapes a rectangle or emits document-owned terminal controls.
- Table feasibility/ratio/border invariants agree with an exhaustive
  small-input reference solver.
- Every partial-prefix parse is panic-free within declared limits; optimized
  updates equal full parse/layout for the same revision.
- Random event sequences keep all scroll/focus/search indices valid.
- Fold/unfold preserves text and source identity; highlighted/unhighlighted
  layout has identical geometry.

### Performance evidence and targets

A local smoke probe rendered 100 short messages in about **80 ms**, versus
about **0.8 ms** for the same text as 100 paragraphs in one call. Repeated asset
loading is therefore worth removing even before sophisticated incremental work.
Rendering all 104 prefixes of a small mixed Markdown example took about
**0.95 seconds**. These are directional measurements, not release thresholds.

Proposed acceptance targets, to calibrate and bind to a recorded host/build:

| Workload | Initial target |
| --- | --- |
| 1 MiB ordinary Markdown at 80/120 columns | Cold model/layout under 500 ms; cached resize under 150 ms. |
| 10 MiB bounded stress document | Under 3 seconds to initial view; no quadratic event/run growth; input responsive while expensive work is pending. |
| 1000-row x 8-column table with bounded cell sizes | Solver under 250 ms, with explicit work bound and correct overflow fallback. |
| 1000-message session | Compare composed vs per-message path, cache assets, report peak memory and avoid one renderer initialization per message. |
| Key -> painted frame in a ready document | p95 under 50 ms; no per-key full-document parse. |
| 20 source updates/second | Coalesce to at most 20 layout refreshes/second; quit response within 100 ms outside an explicitly bounded I/O operation. |

Record corpus bytes/rows/code languages, CPU/RAM, optimization profile, cold/
warm runs, median/p95 and peak RSS. A 10 MiB stress test must either be an
accepted supported limit or an explicit over-limit test; it cannot silently
override the provider's stricter per-file bound.

Start with straightforward full layout plus caches. Only add lazy layout,
background work or a benchmark dependency when these measurements require it.
Run scoped format/lint/tests per packet, then the complete Blad suite and
declared consumer checks at integration gates. No whole-workspace cleanup.

## 10. Actionable execution packets

Each packet has a bounded responsibility, dependencies, inputs, exact work and
acceptance evidence. Do not hand a future implementer just a feature title or
the entire repository. Local intent and immediate-parent/global contracts are
binding; required provider material must be explicitly declared.

Before each implementation packet, approve the applicable intent changes and
materialize four **separate atomic queue tasks** in this order:

```text
blad-qNN-write-tests       kind: write_tests
blad-qNN-implement-code    kind: implement_code   depends on write-tests
blad-qNN-security-audit    kind: security_audit   depends on implement-code
blad-qNN-compliance-review kind: compliance_review depends on security-audit
```

Use the actual current Kvist queue schema, qualified requirement locators,
explicit dependencies, and exact local revisions; these names are a planning
template, not executable YAML or recorded completions. Split a packet further
when its implementation cannot be reviewed atomically. Keep implementation
tasks independent of prior compliance tasks only when their true prerequisites
allow it; integration/promotion requires all applicable review chains.

The implementation record is generated after code/test evidence in a clean-slate
context excluding all intent, prior record/queue/reviews, chat and Git history.
A separate source-blind reviewer compares approved requirements/contract/design
with that record and test evidence. Keep discrepancies for human arbitration.
Do not let the implementer mark its own work compliant, or invent advisory
receipts for the not-yet-enforced review mechanism.

### Q01 — Establish authoritative viewer intent and boundaries

**Depends on:** This review; no implementation prerequisite.

**Inputs:** Local artifacts, root constraints, provider's public contract,
F16-F19/F28-F29 and the decisions in section 4.

**Work:** Add proposed R-026..R-036 appropriately; reconcile current CLI/key/
dump/follow/profile semantics; specify concrete limits and editor parsing.
Update private design and fresh traceable queue definitions. Record the
provider-owned transcript-schema dependency and root architecture's missing
Blad boundary for separate authorized acceptance; do not edit peer internals.
Do not downgrade “full CommonMark” merely to bless accidental data loss.
**Done when:** Every behavior decision has an owner, acceptance case and
qualified locator; planned versus current interfaces are explicit; review
opportunity/exception is recorded honestly, not as a fabricated receipt.

**Estimate:** 2-3 days.

### Q02 — Build deterministic regression and terminal test seams

**Depends on:** Q01's accepted outcomes.

**Inputs:** F01-F11/F26 and the exact reproduction fixtures in section 2.

**Work:** Add render helpers asserting exact semantic text; pure geometry/state
test seams; injected writer/events; Linux PTY fixture runner with development-only
VT decoding. Add a minimal tagged-document-equivalent fixture with long styled
list lines and a long heading. Write failing tests for first-row retention,
paragraph spaces, all content/chrome clipping, search jump/count, ANSI slicing,
code overflow, tiny dimensions and resize.
**Done when:** Current defects fail the intended assertions reproducibly; tests
do not invoke host Mermaid/editor or hang waiting for terminal input.

**Estimate:** 2-3 days.

### Q03 — Make command errors and terminal/editor lifecycle fallible

**Depends on:** Q01, Q02.

**Inputs:** `src/main.rs`, `src/display.rs`, F10/F12-F15/F23, R-021/R-033/R-035.

**Work:** Introduce meaningful command/display `Result` APIs and a thin exit-code
entry point. Use `IsTerminal`; validate dimensions/editor arguments; add a
terminal guard tracking each acquired capability (raw/alternate/cursor/mouse/
autowrap). Suspend/resume editor in one nonrecursive loop; reload source rather
than ANSI. Honor provided width, retain options, and surface failed I/O/events.
Handle press/repeat/release deliberately and Ctrl-C cleanly.
**Done when:** Injected failures at each acquisition/paint/suspend stage restore
owned terminal state; missing inputs exit nonzero; empty editor never panics;
editor return preserves line-number option and view state. Teardown failures
are reported; Drop is a best-effort safety net, not the sole error-report path.
**Estimate:** 2-3 days.

### Q04 — Introduce styled/source-anchored Markdown model and asset cache

**Depends on:** Q01, Q02; coordinate public API changes with Q03.

**Inputs:** `src/markdown.rs`, F02/F07/F19-F22/F27, sections 5 and 8.

**Work:** Convert parser events into typed blocks/cell runs and a style stack.
Retain heading/list/quote/table alignment and source/semantic maps. Keep existing
library consumers compiling through intentional documented API updates.
Cache syntax/theme assets; make rendering effect-free. Sanitize raw controls;
preserve inert unsupported content explicitly instead of discarding it.
**Done when:** Exact semantic/style tests pass for nested and adjacent spans,
ordered/task lists, links/HTML/math profile, code and cells; no external process
runs during parse/layout; unsafe input controls never enter output styles.
**Estimate:** 3-5 days.

### Q05 — Implement measured Unicode prose layout and whitespace

**Depends on:** Q04.

**Inputs:** F02/F03/F20, R-027, section 6 prose rules.

**Work:** Add grapheme/display-column metrics and one cross-run whitespace/
break state. Correct paragraph/hard-break resets; include prefixes/separators
in fit tests; wrap headings/rules/lists/quotes to actual content width.
Keep atomic code spans and source mappings through emergency token breaks.
**Done when:** Width-boundary/property tests pass at 0/1/2 and normal widths;
no leading/event-invented spaces; styled/un-styled text has identical geometry;
CJK/combining/emoji and gutter cases are measured correctly.
**Estimate:** 2-3 days.

### Q06 — Implement clipped viewport, horizontal navigation and resize

**Depends on:** Q03, Q04, Q05.

**Inputs:** F01/F03-F05/F10-F13, R-026/R-029/R-033, section 6 viewport rules.

**Work:** Extract pure state/event transitions and explicit content rectangles.
Paint bounded positioned rows with style-safe horizontal clipping. Correct
scrollbar placement/thumb and footer/header clipping. Add global x, Left/Right,
horizontal Home/End, independent vertical g/G, half-page u/d, and resize
anchoring. Code never uses the prose breaker.
**Done when:** Actual PTY screen retains tagged first lines; long code is one
logical line and reachable edge-to-edge; no ANSI fragments or per-line x reset;
resize redraws immediately; chrome/content never physically wrap.
**Estimate:** 3-4 days.

### Q07 — Implement ratio-constrained tables and multiline borders

**Depends on:** Q04-Q06.

**Inputs:** F06/F07, R-028/R-029, table equations/solver in section 6.

**Work:** Preserve runs/alignment/breaks, compute actual border overhead, implement
bounded deterministic joint-column feasibility, and render row grids. Keep
overflow tables as no-viewport-wrap blocks sharing Q06's horizontal navigation.
Evaluate existing `tabled` against the invariants before choosing reuse.
**Done when:** Every physical content box passes `3*w >= 4*row_height`; small
reference-solver cases fit whenever feasible; otherwise complete overflow is
scrollable; multiline row boundaries and clipped junctions remain aligned.
Include the 6-line/3-column/30-column forced-overflow example.
**Estimate:** 3-5 days.

### Q08 — Implement occurrence search and reliable reveal/highlights

**Depends on:** Q04-Q06; Q07 for cell-specific integration.

**Inputs:** F08, R-030, section 7 search decisions.

**Work:** Index semantic text with explicit boundaries/original offset mapping.
Replace matching-line vectors with occurrences; implement input/commit/cancel/
no-hit states, cyclic n/p/N, persistent counts, x/y reveal and composited
all/current-hit overlays. Remap on every document/layout revision.
**Done when:** Three lines with two occurrences report six hits; n/p reaches
every occurrence including another on the same line; code/Markdown resets
cannot erase highlights; search across style/wrap boundaries works.
**Estimate:** 2-3 days.

### Q09 — Implement heading sections, folds and Tab traversal

**Depends on:** Q04, Q06, Q08.

**Inputs:** F09, R-031, section 7 section/fold rules.

**Work:** Derive heading hierarchy/end ranges; add section identity/focus/folds
and visible-layout mapping. Implement Space and Tab/BackTab; preserve nested
fold choices and preamble; integrate hidden-hit expansion and anchor remapping.
**Done when:** Duplicate/skipped/nested headings behave correctly, code headings
never fold, collapsed descendants are skipped in traversal, hidden search hits
reveal, and dump/export preserve all text. Test edit/resize/appends.
**Estimate:** 2-3 days.

### Q10 — Implement validated session I/O and deterministic resolution

**Depends on:** Q01's provider contract/schema acceptance; Q03's error APIs.
Can run independently of Q05-Q09 after those prerequisites.

**Inputs:** `src/session.rs`, F17/F18/F22/F24, R-005/R-022/R-023/R-034.

**Work:** Replace JSON defaults/duplicated parsing with typed validation; enforce
schema/kind/media/path/size/ordering/completeness semantics. Confine regular
no-link reads, including intermediate components and concurrent growth; do not
mistake a lexical precheck for race-safe confinement. Use a reviewed existing
no-follow primitive or documented OS strategy. Separate lookup from capped
listing; reject ambiguity; total-sort dates with explicit missing-date/ID ties.
**Done when:** Synthetic escapes/links never read outside artifacts; failures
are observable; >1000 and ambiguous prefix tests pass; mixed dates do not
violate ordering; truncation/completion are retained. No peer source coupling.
**Estimate:** 2-4 days.

### Q11 — Unify session/file display and implement live snapshots

**Depends on:** Q04-Q10, accepted provider publication guarantees.

**Inputs:** F16/F17/F21, R-002/R-003/R-032, section 8.

**Work:** Compose session messages with stable message/source identities using
the common pipeline; route show/view/smart targets consistently. Add bounded,
debounced snapshot polling, UTF-8 suffix handling, revision invalidation and
follow/reading modes. Preserve search/folds/x/y through updates and mark
transient stale/incomplete data visibly. Final completion leaves viewer open.
**Done when:** Every prefix/boundary and nonlocal-Markdown regression passes;
append/rewrite/truncate/delete/completion scenarios are correct; no duplicated
messages; pause/resume does not yank a reader; quit stays responsive.
**Estimate:** 4-6 days.

### Q12 — Finish dump/export/docs, performance and independent promotion

**Depends on:** All applicable preceding packets and their review chains.

**Inputs:** F21/F23-F29, R-020/R-035/R-036, sections 9 and 10.

**Work:** Wire ANSI-free rendered dumps and source exports consistently; implement
approved atomic/no-clobber publication and broken-pipe policy. Test exact help/
keys/global flags/smart routing; remove stale user promises and unnecessary
dependencies. Measure benchmark corpus, limits and memory; optimize only proven
bottlenecks. Generate clean-slate implementation evidence, independent security
audit, then source-blind compliance comparison. Retain unresolved discrepancies.
**Done when:** Full Blad suite and declared-consumer gates pass; usage, intent,
record and command behavior agree; measurements meet accepted budgets; human
promotion/arbitration is distinct from test or agent success.
**Estimate:** 2-3 days, excluding unplanned audit remediation.

## 11. Sequencing, effort, and delivery checkpoints

The estimate includes test authoring, implementation and normal scoped review
overhead in each packet, not just writing Rust. Security/compliance contexts must
remain independent; provider/schema decisions and reviewer availability can
lengthen elapsed time. Unplanned remediation, a real Mermaid adapter, terminal
graphics, Windows/macOS support, persistent UI preferences, regex/fuzzy search
and a replacement Markdown parser are excluded.

| Milestone | Packets/outcome | Exit boundary |
| --- | --- | --- |
| M0: approved plan and regression evidence | Q01-Q02 | Behavior is specified; current defects have meaningful failing tests. |
| M1: trustworthy display core | Q03-Q06 | No top-line loss, whitespace corruption, accidental code wrap, ANSI slicing, terminal leaks, tiny-screen crashes or stale resize geometry. |
| M2: requested interactive features | Q07-Q09 | Ratio-sized/bordered tables, real search, horizontal controls, folds and section traversal work together. |
| M3: robust transcripts/streaming | Q10-Q11 | Validated provider boundary, common file/session UX and correct live partial updates. |
| M4: production-quality promotion | Q12 and independent packet chains | Dump/export/docs, performance evidence, safety and source-blind compliance gaps are resolved or explicitly arbitrated. |

The packet estimates sum to **29-45 engineering days** before contingency.
M1 plus tables/search/folds is roughly **21-32 days** once intent/test setup is
included. A small mechanical stabilization patch can take 1-3 days, but it is
not equivalent to M1 and should not be advertised as the requested full fix.
With a second engineer, validated session I/O and terminal harness work can
overlap the layout track; the core parse -> layout -> viewport -> interactive
features dependency chain still limits parallel speedup.

Prioritize M1 before new features: otherwise search/folding/table work inherits
broken coordinates and will be implemented twice. Treat the provider
publication contract as an early decision, not a late streaming surprise.

## 12. Promotion checklist

- [ ] Approved outcomes/contract/design, explicit ambiguities resolved, traceable
      atomic queues and honest advisory-review opportunity/exception.
- [ ] No raw ANSI document model; one tested Unicode/display-width model.
- [ ] First content retained, exact width accounting, all screen rows clipped.
- [ ] Code/table overflow never viewport-wrapped; complete horizontal access.
- [ ] Every padded table cell satisfies 4:3; borders and fitting verified.
- [ ] Occurrence search, persistent counts, forward/backward and hidden-hit reveal.
- [ ] Heading folding and Tab/Shift+Tab work through resize/edit/live updates.
- [ ] Partial input/nonlocal Markdown changes and live snapshot lifecycle tested.
- [ ] Typed bounded no-link session reads, deterministic order, explicit failures.
- [ ] Terminal/editor/subprocess ownership, cancellation and teardown verified.
- [ ] Plain dump/source export/exit/help/docs agree with accepted contract.
- [ ] Reproducible performance/memory evidence and full scoped test gates.
- [ ] Fresh independent implementation record, security audit and separate
      source-blind compliance review; no self-certification or fabricated receipt.

Until those gates are evidenced, Blad should be described as an improving
pre-release viewer, not as fully CommonMark/GFM-complete, live-streaming,
security-audited or compliant merely because historical tasks say `done`.

## 13. Follow-up: targeted first-line, width and whitespace correction

The follow-up request authorizes a bounded runtime correction, not execution of
all twelve packets. Local requirements R-026/R-027 cover viewport retention and
prose layout; R-028 now covers resize/no-wrap overflow. The proposed requirement
IDs in the original review above are provisional: remap the table and later
feature IDs before accepting those future bundles rather than reusing R-028.

The correction adds word/style buffering across inline events, display-column
measurement, grapheme-safe long-word breaking, and paragraph/hard-break resets.
An independent row painter accounts for the gutter/scrollbar and clips content
and chrome at explicit coordinates. Code remains unwrapped with shared
horizontal clipping. Resize re-renders original source. Width-aware and editor
reload paths no longer parse rendered ANSI as Markdown again.

Regression evidence includes exact text/width cases and Linux PTYs decoding the
visible screen. The tagged 80x24 document reproduction must show its first
heading at content row one with zero unintended screen scrolls. Existing
continuation-indentation work is retained in the inline writer and its test.

The new scoped queue chain records tests, implementation, independent security
audit and source-blind compliance work separately. This is not completion of
M1 or the whole production-quality plan: general search, table ratio/borders,
folding, live streaming, session I/O and other historical findings remain.
