<!-- kvist-requirements-version: 1 -->

# Blad Requirements

## Purpose and scope

Blad is a first-class terminal Markdown viewer that reads and displays
skott session transcripts and standalone Markdown files with rich rendering,
interactive navigation, search, and optional edit support.

## Stakeholders and concerns

- Human operators who read skott session transcripts need reliable, fast, correct rendering.
- Users of terminal tools need keyboard-driven navigation and search.
- Automation needs dump mode for non-interactive consumption.

## Functional requirements

Blad must provide the following capabilities:

### Session Viewing

1. **R-001 List sessions:** Users can list recent skott sessions with metadata
   (ID, model, message count, status).
2. **R-002 Show session:** Users can view a completed session transcript with
   full Markdown rendering and syntax highlighting.
3. **R-003 Stream session:** Users can follow a session transcript (live or
   completed) with the same rendering as show.
4. **R-004 Export session:** Users can export a session to a combined Markdown
   document for sharing or archival.
5. **R-005 Load by path:** Users can load a session by its full directory path
   as well as by ID or ID prefix.

### File Viewing

6. **R-006 View file:** Users can view any Markdown file with interactive
   navigation (up/down arrows, page up/down, scroll wheel).
7. **R-007 Dump mode:** Users can output rendered content as plain text
   (no interactive viewer) using `--dump`.
8. **R-008 Edit mode:** Users can press `e` in the viewer to open the file
   in their configured editor and re-render after saving.

### Interactive Viewer

9. **R-009 Scroll navigation:** Users can navigate with up/down arrows,
   Page Up/Down, `g`/`G` for top/bottom, `k`/`j` (vim-style), `u`/`d`
   (half-page).
10. **R-010 Mouse wheel:** Users can scroll with mouse wheel in supporting
    terminals.
11. **R-011 Search:** Users can press `/` to search, type a query, navigate
    between matches with `n`/`p`, and see match count in the footer.
12. **R-012 Status bar:** The viewer shows a footer with scroll percentage
    and position (TOP/BOTTOM/SCROLL).
13. **R-013 Scrollbar:** The viewer shows a scrollbar on the right side for
    long documents.

### Markdown Rendering

14. **R-014 CommonMark:** Support full CommonMark spec.
15. **R-015 GitHub Flavored Markdown:** Support GFM extensions including
    tables, strikethrough, task lists, autolinks, footnotes, superscript,
    subscript, and alert blockquotes ([!NOTE], [!TIP], etc.).
16. **R-016 Syntax highlighting:** Highlight fenced code blocks using
    syntect (50+ languages).
17. **R-017 Mermaid support:** Render the admitted Mermaid subset as terminal
    text boxes, connectors, arrows and labels, without external executables or
    graphical intermediates. Unsupported content remains visible as source.
18. **R-018 Math support:** Render inline and display math (LaTeX) with
    distinct styling.

### Smart Routing

19. **R-019 Smart shortcut:** `blad <target>` tries to interpret the target
    as a file path, session path, or session ID automatically.

## Quality requirements and constraints

20. **R-020 Performance:** Rendering must be fast, even for large session
    transcripts (thousands of messages).
21. **R-021 Error resilience:** I/O errors are reported clearly; failures
    to write artifacts do not crash the interactive session.
22. **R-022 Deterministic:** Session listing is deterministic (sorted by
    creation time, newest first).
23. **R-023 Filesystem safety:** All paths are validated; symlinks are not
    followed when reading session artifacts.
24. **R-024 No network:** All operations are local; no network access
    required.
25. **R-025 Platform:** Linux only (uses crossterm for terminal interaction).

### Targeted Layout Corrections

26. **R-026 Viewport retention:** The interactive viewer must retain the first
    document line at the first content row. Header, footer, content and scrollbar
    writes must be positioned and clipped so drawing never scrolls the terminal.
    Empty and tiny viewports must not panic.
27. **R-027 Prose layout:** Prose and headings wrap at the available terminal
    display-column width after reserving the scrollbar and optional number
    gutter, without hidden margins. Markdown event boundaries must not insert
    spaces, and paragraph/hard-break boundaries must reset line state. Preserve
    source-contributed soft-break spaces, inline-code whitespace and the existing
    indented list continuations. Long ordinary words may break at grapheme
    boundaries; indivisible overwide graphemes and inline code may overflow.
28. **R-028 Layout refresh:** Resize must reflow prose to the new content width.
    Code block lines remain unwrapped and horizontally accessible. Horizontal
    clipping must not cut ANSI controls or Unicode graphemes, and short lines
    must not reset the shared horizontal offset.

29. **R-029 Table geometry:** Compute each cell's aspect floor from its own
    nonempty soft-wrapped text lines, never from neighboring cells, vertical
    padding or empty explicit lines. Use the terminal model of one display
    column being half a line high: require four display columns per occupied
    text line, but never more than that cell's natural unwrapped width. Short
    and explicitly multiline values are exempt once fully unwrapped. A column
    uses the maximum of its cells' floors and must never exceed its widest
    natural text value; an entirely empty column has width zero. Horizontal
    overflow takes precedence over compressing below the text-derived floor.
30. **R-030 Table optimization:** When a feasible layout fits the viewport,
    minimize the maximum rendered row height exactly. Otherwise minimize the
    total table width satisfying R-029, breaking ties by maximum row height.
    Account for separators. Use deterministic integer arithmetic; do not claim
    that proportional space distribution minimizes total table height.
31. **R-031 Table wrapping:** Prefer whole whitespace-delimited words, preserve
    explicit cell line breaks (`<br>` variants included) and empty lines,
    measure Unicode display width,
    and never split a grapheme. An unbroken token may force horizontal overflow
    rather than be split. Normalize intra-line whitespace to a single space;
    normalize terminal controls through the existing inert-text boundary.
32. **R-032 Solver verification:** Exhaustive small-case width enumeration must
    independently check feasibility and the exact objectives in R-030. Cover
    tall adjacent cells without widening short columns, the Component Model
    table from `ARCHITECTURE.md`, short/empty cells, long tokens, Unicode, explicit
    breaks, ragged rows, zero/tiny widths, deterministic rounding, resize
    reflow, I/O failure propagation and large inputs. Measurement and wrapping
    must share line-breaking semantics without allocating strings during
    optimization.

These corrections do not implement the remaining border, search, folding or
live-follow features proposed in the blad implementation plan.

### Accepted Text-Only Rendering Increment

33. **R-033 Appearance:** Provide coherent dark, light and terminal-native
    palettes, explicit host-supplied palette settings, and monochrome output.
    Color depth and appearance are independent; color must not carry meaning
    alone. Theme changes must not alter rendered text or geometry.
34. **R-034 Output policy:** Apply one resolved rendering configuration to
    standalone files, existing session routes and embedded rendering. Honor
    explicit color overrides and `NO_COLOR` in automatic CLI mode. Dumps must
    contain rendered text without terminal escapes; exports retain Markdown.
    Library rendering must neither query terminal input nor launch processes.
35. **R-035 Script notation:** Render recognized standalone and adjacent
    superscript/subscript spans with curated Unicode mappings. Unsupported
    complete spans use explicit `^(...)`/`_(...)` notation. Preserve ordinary
    prose, escaped delimiters, code, nesting and already-authored Unicode.
36. **R-036 Math projection:** Render the documented bounded TeX subset,
    including grouping, scripts, Greek/operators, fractions, roots and text.
    Preserve the exact original expression and delimiters on unsupported or
    malformed input; grouping and scope must never be silently changed.
37. **R-037 Text diagrams:** Admit flowchart, sequence, state, class and ER
    diagrams only with evidence of spatial text rendering and preservation of
    labels, endpoints and directions. Keep authored direction and unwrapped
    diagram rows; horizontal scrolling takes precedence over truncation.
    Provide Unicode and ASCII structure without transliterating authored labels.
38. **R-038 Rendering boundaries:** Make document controls inert. Bound math
    nesting/input and diagram input, semantic complexity, layout work, grid
    extent and output. Distinguish unsupported content from resource failures,
    cancellation and writer errors; retain actionable diagnostics.
39. **R-039 Integration evidence:** Cover configuration precedence, scripts,
    math, admitted diagram families, exact fallbacks, Unicode, tiny viewports,
    resize, writer failure and existing embedding consumers. Test semantic and
    geometric invariants, not success markers alone.
40. **R-040 Rendering efficiency:** Reuse immutable highlighting assets; avoid
    unbounded persistent caches and per-render terminal/process effects. Record
    measured diagram corpus cost and numeric limits before backend admission.

### Accepted viewer polish

41. **R-041 Navigation:** Follow inline/reference links without entering a
    separate mode, including left-clicked prose/heading labels; resolve local
    heading fragments (including duplicate headings,
    explicit IDs and file fragments), retain open-file tabs and per-tab scroll
    positions, and make missing destinations visible without losing the document.
42. **R-042 Theme files:** Load an explicitly selected, bounded JSON theme file
    with strict validation, semantic palette overrides and no implicit discovery.
    Explicit CLI appearance overrides file appearance; color/dump policy remains
    independent and survives navigation, editing and resize.
43. **R-043 Rendering polish:** Preserve parent ordered-list counters across
    nested lists; retain alternating bullets, matching superscript footnote tags,
    visible alert colors, syntax highlighting and code-panel cap rows.
    Blockquotes have a persistent left edge and optional palette background
    across their content rows, without duplicate surrounding blank lines.
    Theme and color changes must not alter text geometry.

`~~~` remains a CommonMark code fence, not a custom horizontal rule. `***`
uses a heavy rule. Editing remains a whole-file external-editor handoff: an
embedded or section editor is deferred rather than introducing another editing
and persistence model. ER cardinalities remain explicit textual Mermaid
notation; graphical crow's-foot endpoints are not required.

## Constraints

- **C-001 Dependencies:** Use existing workspace dependencies where
    possible (sav, clap, serde, pulldown-cmark, syntect, crossterm).
- **C-002 No new state format:** Blad reads skott's structured session
    format; it does not define its own session storage format.
- **C-003 Headless:** No cloud services, telemetry, or runtime daemon
    required for core commands.
- **C-004 Text only:** Do not generate or display PNG/SVG, including intermediate
    graphics. Do not require image protocols, Chromium, Node or Graphviz.

## Out of scope

- Creating or continuing sessions (that is skott's responsibility)
- Multi-user concurrent editing of sessions
- Encryption of session files
- Automatic cleanup or retention policies
- Backward compatibility with legacy log formats

## Acceptance and traceability

Each requirement carries an identifier (R-xxx, C-xxx) for traceability.
Tests and implementation records must reference these identifiers to
demonstrate fulfillment.
