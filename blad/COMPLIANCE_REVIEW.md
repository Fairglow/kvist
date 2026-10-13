# Targeted Layout Correction: Source-Blind Evidence Comparison

Date: 2026-10-12

The independent comparison below describes the test snapshot at review time.
Subsequent host-authored verification improvements are distinguished in the
addendum, not represented as a reissued independent review.

## Scope and independence

This is one bounded, independent **source-blind comparison** of R-026, R-027,
R-028, Contract **Width and Viewport Semantics**, and Design **Targeted Layout
Correction Design**. It is not a Kvist review receipt, human acceptance,
whole-component compliance determination, or security certification.

Evidence read: local REQUIREMENTS.md, CONTRACT.md, DESIGN.md, TODOS.yaml, the
fresh IMPL.md, tests/layout.rs, tests/viewer.rs, and src/markdown_tests.rs
**as test source only**. Cargo manifests were read solely for build identity:
`blad` 0.3.0, edition 2024, minimum Rust 1.95.

No production Rust source, source-bearing history/diffs/build artifacts,
prior review reports, peer internals, or agent/session history was consulted.
The root COMPLIANCE_REVIEW.md remains an opaque fixture; its expected first
line is visible only in the permitted viewer test assertion. No source
inspection was used to resolve uncertainty.

Implementation observations below are attributed to the independent IMPL
record, not to direct source inspection by this reviewer. Executed results
are that record's independently obtained evidence; this comparison did not
execute additional commands.

## Executed evidence

IMPL reports successful offline, locked runs of:

| Command | Recorded result |
| --- | --- |
| `cargo test -p blad --offline --locked --test layout --test viewer` | 8 layout and 4 viewer tests passed |
| `cargo test -p blad --offline --locked --lib terminal_text::tests` | 4 passed; 24 filtered out |
| `cargo test -p blad --offline --locked --lib viewport::tests` | 2 passed; 26 filtered out |

These are **18 executed passing tests**, not the entire library or component
suite. The 22 tests in markdown_tests.rs were inspected but not executed in
those runs. Their assertions describe additional intended checks, not passing
evidence. In particular, the list-continuation test checks indentation of
selected lines without asserting that any such lines exist.

## Targeted outcome comparison

### R-026 — First-line retention and bounded frame painting

**Observed alignment:** IMPL describes explicit cursor positioning for the
header, each content row, footer, and scrollbar, without newline advancement.
Header/footer normalization and clipping, content clipping, saturated geometry,
and zero-dimension paint handling match the targeted design. The first rendered
document line is assigned to the first content row.

**Executed examples:** `tagged_document_keeps_its_first_line_without_screen_scrolling`
asserts the opaque fixture's expected heading at content row 1, a header at row
0, zero modeled screen scrolls, and successful quit at 80×24.
`heading_prose_and_gutter_share_the_terminal_width` checks first-heading
visibility and zero scrolls at 30×12 with and without numbers, using a long
filename. Other viewer tests check zero scrolls with code overflow and resize;
the 1×1 case remains quittable without modeled scrolling.

**Limit:** These examples do not prove “never scrolls” for all documents or
terminals. The PTY model counts each printed character as one cell and ignores
SGR appearance. Real-terminal Unicode widths, zero dimensions, empty interactive
documents, two-row frames, and terminal cleanup under injected failures are not
independently demonstrated by this executed evidence. IMPL's observations
support those geometry/cleanup paths but are not fault-injection proof.

### R-027 — Exact prose budget, whitespace, and grapheme wrapping

**Observed alignment:** IMPL describes a word buffer spanning parser/style
events, source-contributed pending spaces, separator-inclusive fit decisions,
block/hard-break resets, complete-grapheme emergency breaks, atomic inline code,
and retained list-continuation indentation. Heading/list/alert prefixes
participate in layout. Document sizing reserves one scrollbar column and a
monotonically converged number gutter, without an additional prose margin.

**Executed examples:** Layout assertions establish exact paragraph/hard-break
output, inline-event adjacency, source soft-break spacing, full-width separator
boundaries, cross-style words, long words without initial blank lines, heading
widths, and inline/block-code whitespace. CJK, combining-mark, and joined-emoji
examples preserve graphemes. A combined heading/paragraph/nested-list/alert
fixture fits every width 1 through 30. The recorded viewport tests cover gutter
convergence and a supplied width cap; the numbered PTY example checks frame
behavior.

**Limit:** These fixtures are not universal proof across Markdown constructs,
Unicode sequences, or gutter thresholds. Tiny-width tests establish successful
rendering for their inputs, not exact one-column output at width zero.
Continuation indentation is source-observed in IMPL, with no independently
executed exact indentation assertion identified here. Atomic code and
overwide graphemes may overflow by the explicit contract; this is not a
discrepancy. IMPL also observes that adjoining ordinary text can share an atomic
buffer with inline code; R-027 permits rather than mandates ordinary-word
emergency breaking, so this is not a demonstrated contradiction.

### R-028 — Resize, unwrapped code, and shared horizontal clipping

**Observed alignment:** IMPL describes resize rendering from original Markdown
at the new budget, retained numbering/width limits, recomputed line-index search
matches, and offset clamping. Code lines bypass prose wrapping. The widest
document line determines the shared horizontal bound. Terminal normalization
separates SGR from text, expands tabs at eight-column stops, and pads intersecting
wide graphemes instead of emitting fragments.

**Executed examples:** The code PTY test asserts exact initial clipping and the
shifted long-code slice after ten Right keys, while a short line becomes empty
without resetting the shared offset. Layout tests preserve overwide code lines.
IMPL records terminal-text tests for style-aware clipping, grapheme edges,
embedded styles, visible control escapes, and fixed tab stops.

**Limit:** Despite its name, `resize_reflows_content_and_tiny_viewport_remains_quittable`
does **not** assert changed prose wrapping: after widening 30×10 to 50×12 it
checks header/footer placement, zero scrolls, and quit. Actual resize reflow
therefore rests on IMPL's source observation, not an executed content assertion.
Narrowing, resize with numbering or horizontal overflow, and real-terminal
Unicode clipping are not demonstrated by that PTY test.

## Additional contract/design evidence and boundaries

IMPL observes that a supplied rendering width is the complete prose budget,
zero clamps to one, and absent width leaves prose unwrapped. Layout tests cover
exact supplied-width boundaries and selected unbounded output. It also observes
that `view_markdown_width` retains an interactive cap and directly renders
noninteractive output without reparsing ANSI; width-aware display routing and
post-editor reentry were not exercised. Home/top offset behavior and guard
restoration are observations, not independently executed targeted assertions.
The recorded viewport writer-error test supplies limited failure-propagation
evidence.

IMPL's table-width limitations, search jumps whose calculated offsets are not
applied, and editor/dump limitations remain residual observations. They are not
silently resolved by this report. Full GFM, sessions, search navigation,
live-follow, and whole-component compliance are outside this correction;
table-ratio/border work is explicitly deferred. No older broad claim is promoted
to verified status.

## Discrepancies, arbitration, and conclusion

No demonstrated implementation-versus-targeted-intent discrepancy blocks any
of the three requested fixes. The observed implementation and passing examples
support first-line retention, corrected prose spacing/budgets, and accessible
code overflow with a document-wide horizontal offset.

The substantive verification gap is resize reflow: the executed resize test
does not directly prove its named content outcome. Exact list-continuation
preservation and width-aware/editor routing likewise have weaker evidence than
the directly asserted examples. These are verification limitations, not proof
of implementation failure. Human arbitration should decide whether the
source-observed evidence suffices or stronger targeted assertions are required;
this report does not manufacture acceptance or universal guarantees.

TODOS.yaml statuses describe orchestration only. In particular, `done` entries
and an `in_progress` comparison entry do not establish human acceptance,
independent certification, or whole-component safety.

## Host follow-up to verification limitations

The implementer strengthened `tests/viewer.rs` to assert the exact paragraph
rows before and after widening the terminal: three words plus a continuation
at 30 columns become all four words on one content row at 50 columns, with
the former continuation row empty. The same test now verifies an empty
interactive document's content/footer and successful quit without scrolling.

The preserved list-continuation test in `src/markdown_tests.rs` now asserts that
wrapped continuation lines actually exist before checking their indentation.
These changes strengthen tests without changing production behavior or intent.
The host subsequently ran `cargo test -p blad --offline --locked`: all **44**
tests passed, including the stronger assertions. Strict scoped Clippy,
formatting checks, and the Blad build also passed. The earlier independent
18-test run and this source-blind comparison are not retroactively claimed to
have executed the stronger assertions. Other verification limitations and
whole-component exclusions remain.
