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
17. **R-017 Mermaid support:** Detect mermaid fenced code blocks and attempt
    to render using mmdc, with fallback to plain text display.
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

These corrections do not implement the remaining table-ratio/border, search,
folding or live-follow features proposed in `PLAN.md`.

## Constraints

- **C-001 Dependencies:** Use existing workspace dependencies where
    possible (sav, clap, serde, pulldown-cmark, syntect, crossterm).
- **C-002 No new state format:** Blad reads skott's structured session
    format; it does not define its own session storage format.
- **C-003 Headless:** No cloud services, telemetry, or runtime daemon
    required for core commands.

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
