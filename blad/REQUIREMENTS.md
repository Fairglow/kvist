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
3. **R-003 Active snapshots:** With session support enabled, users can read an
   active session's current snapshot. Live following is not provided.
4. **R-004 Export session:** Users can export a session to a combined Markdown
   document for sharing or archival.
5. **R-005 Load by path:** Users can load a session by its full directory path
   as well as by ID or ID prefix.

### File Viewing

6. **R-006 View file:** Users can view Markdown files within the R-056 source
   limits with interactive navigation (up/down arrows, page up/down, scroll wheel).
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
    between occurrences with `n`/`N` (`p` also goes backward), and see the
    current occurrence and total count in the footer.
12. **R-012 Status bar:** The viewer shows a footer with scroll percentage
    and position (TOP/BOTTOM/SCROLL).
13. **R-013 Scrollbar:** The viewer shows a scrollbar on the right side for
    long documents.

### Markdown Rendering

14. **R-014 CommonMark:** Parse CommonMark and document its terminal projection:
    raw HTML is omitted and images display alternative text, not graphics.
15. **R-015 GitHub Flavored Markdown:** Support documented extensions including
    tables, strikethrough, task lists, autolinks, footnotes, superscript,
    subscript, and alert blockquotes ([!NOTE], [!TIP], etc.). This is not a
    full GFM conformance claim; table alignment follows R-073.
16. **R-016 Syntax highlighting:** Highlight fenced code blocks using
    syntect (50+ languages).
17. **R-017 Mermaid support:** Render the admitted Mermaid subset as terminal
    text boxes, connectors, arrows and labels, without external executables or
    graphical intermediates. Unsupported content remains visible as source.
18. **R-018 Math support:** Render inline and display math (LaTeX) with
    distinct styling.

### Smart Routing

19. **R-019 File arguments:** `blad FILE...` opens files directly, regardless
    of extension, with distinct files in argument order as tabs. Positional
    arguments are never interpreted as session IDs.

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

Repository nextest verification of R-033 and R-037--R-039 MUST reserve the
current test pool for native-Mermaid fixture execution, including the dedicated
diagram/backend suites and mixed Markdown, color-geometry, CLI/session and
viewer regressions that invoke the engine. Unrelated tests MUST remain parallel.
A persistent repository-policy regression MUST reject missing coverage or
weakened pool reservations, including CI-profile inheritance. Preserve existing
render assertions, cancellation semantics and production limits; do not retry
or skip failures. This coordinates one configured runner, not external host
processes, concurrent runners or arbitrary configurations.
The canonical policy MUST be tracked at nextest's default discovery path,
`.config/nextest.toml`, so ordinary repository runs and inherited CI profiles
do not require an easily omitted `--config-file` argument. Regression checks
MUST reject an ignored/missing configuration and repository entrypoints that
select a different configuration. Other local `.config` contents stay ignored.

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
    Footnote prose retains its muted theme across wrapped rows, explicit breaks,
    paragraphs and inline-style boundaries. Links and math retain their semantic
    colors; emphasis and inline code compose with the surrounding footnote
    theme. Ending a footnote must not theme subsequent ordinary prose.
    Verify effective attributes, independently painted rows, resize, custom
    palettes and color/plain policies without changing text geometry.
    Blockquotes have a persistent left edge and optional palette background
    across their content rows, without duplicate surrounding blank lines.
    Theme and color changes must not alter text geometry.

`~~~` remains a CommonMark code fence, not a custom horizontal rule. `***`
uses a heavy rule. Editing remains a whole-file external-editor handoff: an
embedded or section editor is deferred rather than introducing another editing
and persistence model. ER cardinalities remain explicit textual Mermaid
notation; graphical crow's-foot endpoints are not required.

## Constraints

### Accepted standalone boundary increment

44. **R-044 Optional sessions:** Standalone builds default to no session
    integration. One opt-in Cargo `sessions` feature admits session APIs,
    `list`, `session` and `export`, and their session-only options.
    General viewer options remain available in both configurations.
    `show`, `view` and `stream` are retired, without aliases or migration.
    Kvist's all-feature build includes session support; rendering consumers do
    not need it. Blad has no dependency on Kvist/Sav to read session files.
45. **R-045 CLI usability:** Both builds provide `--version`/`-V` and help.
    Empty invocation on terminal stdin displays usage. Multiple file arguments share one viewer;
    repeated canonical files reuse a tab and render only once in dumps, in
    first-argument order. Unreadable files fail explicitly.
46. **R-046 Stable tabs:** Tabs retain their initial/opening order when
    switching, following links or returning. Selection changes do not shift
    label columns; per-tab positions remain independent.
47. **R-047 Browser diagnostics:** Explicit URL opening must not inherit
    viewer stdin/stdout/stderr. Capture bounded diagnostic text, make controls
    inert and distinguish successful launch warnings from launcher failure.
    Results remain in the footer across redraw/focus events until another
    user action replaces them. Browser launch does not reset the document.

The human selected opt-in `sessions`, direct file arguments in both builds,
and removal of the misleading snapshot-only `stream` command. Session
live-follow remains deferred. Separately approved bounded stdin paging is
specified in R-069 through R-072, without another feature toggle.

### Accepted production viewer increment

48. **R-048 Precise search:** Highlight only matching rendered text, including
    multiple occurrences on one line, through ANSI styles and horizontal
    clipping. Preserve graphemes. Literal, case-insensitive search is the
    default for `/`; normal-mode `Ctrl+R` starts a fresh forward regex search,
    and `Ctrl+R` toggles literal/regex inside either prompt.
    Invalid patterns and no matches remain visible; zero-width matches are
    excluded. `/` searches forward; `n`/`>` move forward and `N`/`<` move
    backward, wrapping at either end. Start near the viewport. `?` opens help,
    never backward search; `p` has no normal-mode search action.
49. **R-049 Precise navigation:** Highlight only the selected link's rendered
    spans. Support deterministic Ctrl+Arrow navigation by rendered position,
    reveal offscreen selections vertically and horizontally, and recompute
    positions after resize. Retain Tab/Shift+Tab link traversal and add
    next/previous heading navigation without repurposing established keys.
50. **R-050 Tab lifetime:** Normal-mode `q`/Esc closes the active tab, exiting
    only when it was the last. Preserve surviving tab order, positions and
    valid back history. Provide an explicit quit-all key. Search and help
    cancellation must not accidentally close a tab.
51. **R-051 Stable display:** Scrolling and tab switching must not clear the
    full screen or leave/re-enter the alternate screen. Buffer frame output,
    update changed rows only, erase stale content, and repaint on resize or
    explicit refresh. Ignore key-release events. Retain terminal restoration
    on success, editor handoff and error.
52. **R-052 Discoverability:** Provide in-viewer help and familiar full-page,
    half-page, reverse-match and repaint keys. Document deliberate
    differences from `less`, regex syntax/limits, and deferred capabilities.

### Viewer reliability follow-up

53. **R-053 Bounded browser launch:** An explicitly requested browser launch
    must stop waiting for its launcher after a five-second deadline following
    spawn. Terminate and reap the owned launcher on timeout, report a distinct
    timeout with bounded diagnostics, and retain the document and terminal.
    Do not wait for descendant-held output EOF or kill an independent browser.
    Test ordinary completion, failure, timeout, diagnostic floods and cleanup.
54. **R-054 Transactional reload:** Automatic reload updates the active file without
    terminal teardown, retaining rendering policy, tab order/history and
    numeric vertical/horizontal offsets, clamped to the new layout. Recompute
    search results and navigation metadata; clear obsolete link selection.
    Commit the new contents only after successful read and render. Read/render
    failure retains the previous usable document and exposes a footer error.
    Pathless documents do not reload. `F5` has no action; `Ctrl+R` starts regex
    search in normal mode and toggles regex inside a search prompt, never reloads.
    Open candidates nonblockingly without following final symlinks, then
    validate the opened descriptor as a regular file before reading, avoiding
    a metadata/open race with a FIFO replacement.
55. **R-055 Unicode terminal evidence:** PTY screen assertions must model
    display columns for wide graphemes, combining text and joined emoji,
    including continuation cells and delayed wrap. Cover exact highlights,
    mouse-link hit positions, resize and horizontal clipping with Unicode and
    a number gutter. State emulator limits rather than claiming every real
    terminal/font combination is certified.

### File-input reliability increment

56. **R-056 Bounded document reads:** Standalone file reads must accept at
    most 16 MiB (16,777,216 bytes) per file, inclusively, across initial
    interactive/dump inputs, newly followed file links, automatic reload and
    post-editor rereads. Reject oversized input explicitly, never truncate it
    into a successful document. Check both opened-file size and actual bytes
    read, consuming at most the limit plus one probe byte if size information
    understates the stream. Apply the limit before UTF-8 decoding and rendering.
    All initial named files must validate before terminal entry; all inputs
    must validate before dump/noninteractive output. Live stdin is the
    explicitly incremental exception described in R-069;
    link/reload failure retains the previous usable document and tab/history
    structure. Existing editor-error propagation/restoration remains unchanged.
57. **R-057 Regular-file inputs:** All standalone document and explicit theme
    reads must open nonblockingly, reject final symlinks at the opened path,
    validate the opened descriptor as regular and read that same descriptor.
    Preserve canonical resolution of explicitly selected initial files,
    file-link destinations and theme paths, including authored symlinks.
    Reload/editor rereads use the opening canonical path without resolving
    a subsequently substituted final symlink. Keep the inclusive 64 KiB theme
    limit. Report nonregular, size, UTF-8/JSON and I/O failures explicitly.
58. **R-058 Input-boundary evidence:** Cover inclusive limits and one byte
    over, understated/growing or endless streams, read-error propagation,
    nonregular/FIFO input, symlink policy, initial no-partial-output behavior,
    retained link/reload usability and post-editor terminal restoration.
    Document that per-file byte bounds do not establish render/search latency,
    aggregate tab memory, ancestor isolation or hard kernel-I/O deadlines.

### Standalone publication preparation

59. **R-059 Independent package:** Publishable package identity is `kvist-blad`;
    executable and Rust library remain `blad`. Its archive must contain license
    text, user documentation and self-contained tests/fixtures, without peer
    component source or root-artifact build dependencies. Declare Rust 1.95 as
    the supported minimum and verify it and stable in both feature modes.
60. **R-060 Task semantics:** Preserve checked/unchecked task state as `[x]`
    and `[ ]` in styled and plain rendering without depending on color.
61. **R-061 Session input reliability:** Optional Kvist/Skott integration must
    propagate malformed, unreadable, nonregular and oversized artifact errors.
    Bound each manifest/message to an inclusive 16 MiB. Reject symlink session
    roots/candidates, manifests, message directories and message files; open
    message entries relative to the opened session descriptor. Admit only
    `messages/<filename>` entry paths. Missing runs roots mean no sessions;
    directories without a manifest are not sessions. Reject empty/ambiguous
    ID prefixes and do not limit lookup to the first 1,000 sessions.
62. **R-062 Accurate documentation:** Public standalone documentation must cover
    implemented options, key/modal precedence, themes, every palette role,
    color capability/policy precedence and resource/platform limits. Session
    instructions belong in a separate Kvist/Skott integration guide.
63. **R-063 Release evidence:** Verify the extracted archive's complete tests,
    isolated release installation, dependency/license policy, and downstream
    compilation. Retain independent source observation and source-blind
    comparison without claiming automatic acceptance or security certification.

The session bounds above are a conservative release-hardening policy for the
retained optional integration. Selected-root ancestors, aggregate retained
memory, concurrent snapshots and hard I/O/rendering deadlines are not isolated.
This increment preserves existing search-Esc/help-Ctrl+C precedence and the
standalone source ceiling; earlier explicit-acceptance findings remain retained.

### Seamless file reload

64. **R-064 Automatic reload:** Interactive file tabs must detect in-place
    writes and atomic replacements automatically, without a flag. Refresh the
    active tab after a short stable-metadata debounce; inactive tabs refresh
    when revisited. Retain numeric vertical/horizontal offsets (clamping only
    to the new layout), rendering options, tab order/history and search prompt,
    query/mode/direction. Recompute search and link/heading metadata; cancel
    obsolete link selection. Read/render errors retain the old document
    and report a footer error without repeated notices for unchanged failures.
    Recover on the next change. Preserve terminal ownership
    and changed-row painting. Pathless documents and noninteractive output
    must not watch. Polling is local and bounded in frequency, not a hard
    latency guarantee or snapshot of concurrent writes.

### Unix pipeline behavior

65. **R-065 Standard input:** With no file arguments and redirected stdin,
    read one bounded UTF-8 document. `-` explicitly selects stdin,
    may appear among files in argument order and is read/rendered once.
    Named files do not otherwise consume stdin. Reject explicit `-` on
    terminal stdin rather than waiting for document input. Apply the inclusive
    16 MiB source limit. Noninteractive/dump sources validate through EOF
    before output; interactive stdin follows R-069. Empty input succeeds
    without document text; malformed/oversized input fails.
66. **R-066 Pipeline output:** Automatically enter the interactive viewer
    when stdout is a terminal and `--dump` is absent, including redirected
    stdin. A controlling terminal must be available for redirected-stdin
    paging; otherwise fail promptly and recommend `--dump`. Otherwise output one
    finite rendered snapshot, defaulting to escape-free text; explicit
    `--color always` may retain styles, but `--dump` always removes them.
    A downstream closed pipe must terminate quietly and successfully, without
    a panic, diagnostic or waiting for viewer input. Other I/O errors must
    remain explicit failures. Optional session stdout routes follow the same
    error policy; exports remain source Markdown.

### Release-readiness follow-up

67. **R-067 Diagnostic initialization:** `BLAD_LOG_FILE` must open at most
    once during logging initialization, nonblockingly, without following a
    final symlink, and require a regular file. Retain the opened handle for
    later events. Failure must visibly report escaped context and fall back
    to stderr without recursion, deadlock or suppressing help/version.
    Existing files append; newly created logs use private permissions.
    Logging must not claim that normal footer actions emit diagnostic events.
    Public docs/help must match implemented search, reload and navigation keys.

### Simplified viewer keybindings

68. **R-068 Modeless links:** Remove the `f` link mode and its modal dispatch.
    Plain `f` has no action, with or without links or retained search results.
    Preserve direct Tab/Shift+Tab, Enter, Ctrl+Arrow and mouse link navigation
    without blocking normal scrolling, tab switching or quit keys.

### Automatic bounded streaming pager

69. **R-069 Live stdin:** Interactive stdin must display valid incoming text
    before EOF, without requiring a newline, paragraph boundary or closed fence.
    Keyboard input comes from the controlling terminal, never the Markdown pipe.
    Keep navigation, help, search, links, tab switching and quit responsive while
    the producer is idle or continuously writing. Editor handoffs must use
    terminal keyboard input, never consume the document pipe. Continue bounded
    ingestion while the stdin tab is inactive. EOF finalizes the document without
    exiting the viewer, including empty input. Closing the stdin tab releases its owned
    document descriptor so an upstream writer is not stranded on an unread pipe;
    quitting must neither drain stdin nor kill the producer.
70. **R-070 Streaming fidelity and follow:** Preserve split UTF-8 and authored
    newlines across arbitrary read boundaries. Re-render retained source so
    pending syntax and late reference/footnote definitions can revise earlier
    output. At EOF the document and navigation metadata must match one-shot
    rendering at the same width/options. Initially follow the newest content;
    upward navigation, search and link/heading browsing pause following.
    Bottom navigation resumes following. Keep numeric offsets while paused,
    clamping only to layout bounds; preserve the follow state through resize
    and tab switches. Refresh retained searches and clear obsolete selections.
71. **R-071 Streaming bounds and failures:** Retain at most 16 MiB source
    bytes, with at most one overflow probe and a bounded UTF-8 carry buffer.
    Wait on document and keyboard readiness rather than imposing a polling
    delay on every short input batch. Bound work per input pump and coalesce
    whole-document renders without waiting for a quiet producer.
    Do not add an unbounded queue, worker, daemon,
    temporary spool or parser cache. Malformed/incomplete-at-EOF UTF-8,
    overflow and read failures stop ingestion, retain the last valid display,
    remain visible and return status 1 when the viewer closes. Rendering/writer
    failures propagate after terminal cleanup. Source controls remain inert.
    These are source/input-work bounds, not aggregate-memory or hard
    filesystem/render/search latency guarantees.
72. **R-072 Pager evidence and documentation:** Cover real pipes and controlling
    PTYs, early rendering, idle/continuous producers, follow/pause/resume,
    inactive/closed stdin tabs, resize, search/links, EOF, errors, exact bounds,
    split Unicode/control/syntax and final-render equivalence. Preserve finite
    dump/redirected-output behavior and optional session snapshots. Document
    the breaking routing change, `--dump` opt-out, missing-terminal errors,
    resource limits and distinction from session/file tail-following. Record
    a measured representative sustained-input workload without claiming a
    universal performance guarantee.

### Markdown table alignment

73. **R-073 Table alignment:** Honor left, center and right delimiter alignment
    in headers and every wrapped body-cell line; unspecified alignment remains
    left. Use Unicode display columns for padding, placing an odd extra center
    space on the right. Alignment must not change width solving, wrapping,
    explicit breaks, source column order, row styling or horizontal-overflow
    policy. Keep link selection and mouse hitboxes on the rendered label,
    excluding padding, after reflow and with quotes/line-number gutters.
    Verify exact styled/plain geometry, default/explicit alignments, empty
    cells, odd padding, Unicode, wrapping, resize, overflow and multiple tables.
    Document supported alignment, the R-074 emphasis subset and deferred
    dedicated inline-code styling; HTML tables and merged cells remain outside
    the supported Markdown table profile.

### First inline table styling subset

74. **R-074 Table emphasis:** Preserve bold, italic and strikethrough attributes,
    including nesting/combinations and supported emphasis within links, across
    cell whitespace normalization, explicit breaks and whole-word wrapping.
    Attributes apply to the corresponding displayed text, not alignment padding,
    separators or neighboring cells/rows. Keep header reversal, alternating row
    backgrounds and enclosing quote styling intact, with no style leakage.
    Preserve plain text, solved geometry, alignment, source-owned link hitboxes,
    precise search/selection overlays, grapheme-safe clipping and resize behavior.
    Keep authored controls inert and propagate writer errors. Verify effective
    attributes, not escape-sequence presence alone, including Unicode,
    normalized spaces, nested/event-split words, plain/color policies and PTYs.
    Dedicated code appearance, code-specific whitespace/nonbreaking semantics,
    table link colors and other new inline styles remain deferred. Existing
    enclosing emphasis may style their displayed text without adding those
    separate semantics. Terminal/font attribute support is not guaranteed.

### General YAML frontmatter

75. **R-075 YAML frontmatter:** Recognize a single YAML header at the start of
    any Markdown document, independent of filename or Agent Skills fields.
    Preserve its delimiters, indentation, comments, ordering, blank lines,
    multiline scalars and literal Markdown as syntax-highlighted YAML in the
    existing code-panel presentation. Support empty headers, optional initial
    UTF-8 BOM, LF/CRLF, trailing horizontal delimiter whitespace and `---` or
    `...` closing lines. Only an unindented opening `---` on the first line
    qualifies. Unclosed candidates retain normal Markdown parsing, including
    provisional streaming snapshots; later input may complete the header.
    Do not validate, deserialize, reformat or execute YAML, expand aliases or
    interpret fields as configuration/permissions. Invalid YAML within a closed
    header remains visible as source. Later separators and fenced/quoted
    examples keep their normal Markdown meaning. Metadata contributes no
    Markdown headings, links, reference definitions or footnotes.
    Preserve body source offsets and navigation, searchability, unwrapped
    horizontal overflow, reflow, theme/color/plain policy, inert controls,
    cancellation and writer-error propagation. Verify exact source/geometry,
    effective YAML token colors, CLI file/stdin and real streaming/viewer paths,
    including navigation after the header and EOF equivalence.

### Search-mode feedback and safe exit

76. **R-076 Search feedback:** Visually distinguish the current occurrence
    from all other matches during query editing and confirmed-result navigation,
    without relying on color alone. Preserve exact Unicode/grapheme spans,
    source styling outside matches, themes, no-color output, horizontal clipping,
    resize, reload and live-input refresh. Resize retains the current occurrence
    ordinal when available, otherwise clamps it to the refreshed match list.
    Keep an already-visible current occurrence in view after reflow; do not
    pull a manually scrolled-away viewport back to an offscreen occurrence.
    Confirmed Enter navigation must pause resumed stdin following just like `n`.
    Selected links retain their existing
    selection presentation. Show distinct query-entry and confirmed-search
    indicators with the relevant confirmation/navigation/exit hints.
    Enter confirms a nonempty query into result navigation, not normal viewing;
    subsequent Enter moves forward like `n`. Esc cancels either search phase.
    In confirmed search, lowercase `q` clears search rather than closing a tab;
    in the query prompt it remains literal query text. Exiting clears query,
    results and highlights while retaining the viewport and live-follow pause.
    A later normal-mode `q`/Esc still closes the tab. Empty confirmation returns
    to normal viewing. No-match and invalid-regex results remain safely
    dismissible. Preserve global quit/help/tab-switch precedence and existing
    link/heading navigation exits from search. Cover effective current/passive
    attributes, same-row and wrapped navigation, styles/Unicode, search phases,
    errors, multi-tab safety, refresh and quit behavior through persistent tests.

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
