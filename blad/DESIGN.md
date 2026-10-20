<!-- kvist-design-version: 1 -->

# Blad Design

Blad is a viewer for skott agent sessions and standalone Markdown files. It
reads skott's structured session format from `.skott/runs/` and provides
interactive viewing, live streaming, session listing, transcript export,
full Markdown rendering with syntax highlighting, search/navigation, and
optional edit mode.

## Design overview

Blad is a CLI terminal application built on Rust with clap for argument
parsing, crossterm for interactive terminal I/O, pulldown-cmark for Markdown
parsing, and syntect for syntax highlighting. It reads skott session files
and renders them to ANSI-colored terminal output.

## Internal structure

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

- `show_session(session)`: Display completed session transcript
- `dump_session(session)`: Output session as plain text
- `stream_session(session)`: Stream session transcript
- `view_markdown(content)`: View rendered markdown interactively
- `view_markdown_file(path, content)`: View with edit support
- `dump_markdown(content)`: Output rendered markdown as plain text
- `export_session(session, output)`: Export to combined markdown document
- `interactive_viewer(path, content)`: Full interactive viewer with navigation and search

### Markdown rendering (`markdown.rs`)

- `render_markdown(md, writer)`: Render markdown with syntax highlighting
- `find_syntax(ss, lang)`: Find syntax definition for a language
- `render_mermaid(content)`: Attempt to render mermaid diagram via mmdc
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
- Mermaid diagrams (with mmdc integration)

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

Let `h_rc(w_c)` be a prepared cell's exact wrapped height, `H` the maximum row
height, `n` the number of columns, and `B = viewport - 3(n-1)` the saturating
content budget. The constraints are integer `w_c >= 4H` and
`h_rc(w_c) <= H` for every cell. The common floor includes blank cell padding;
checking only each cell's own text height misses tall adjacent cells.

For a candidate height cap `H`, binary-search each column for its smallest width
that satisfies both constraints. The searches start at the larger of the
longest word and `4H`; unwrapped width or the preceding cap's solution is a
valid upper bound. This independently yields the exact smallest sum `S(H)`
for that cap. Enumerate caps in ascending order; the first with `S(H) <= B`
is the exact minimax-height solution. Otherwise retain the smallest `S(H)`
(earlier cap wins ties) for minimum-width overflow. Separator overhead is fixed.
All arithmetic is integer, with saturating size arithmetic at representational
limits and `u128` proportional products.

To bound enumeration without a viewport-dependent search explosion,
binary-search the smallest feasible *uniform* column width `U`. It provides
a feasible table of width `nU`. Any cap above `floor(U/4)` already requires
more than that content width and cannot improve the overflow optimum or
precede the first feasible viewport solution. Start at the largest explicit
line count, since smaller caps are impossible. Stop earlier if the sum of
`max(4H, longest_word_c)` reaches the best overflow width: that nondecreasing
lower bound cannot improve width or the earlier cap's tie-break.
Height queries short-circuit
on exceeding the cap and allocate no rendered text. The solver is polynomial
and bounded by content, not the numeric viewport width; it does not enumerate
column-width combinations.

After solving the cap and floor, distribute surplus up to natural widths by
integer proportional flexibility and largest remainders, with column index as
the stable tie-break. Increasing a width cannot invalidate feasibility.
Without a viewport, use unwrapped widths raised to the common aspect floor.
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
4. **Mermaid fallback:** If `mmdc` is not installed, mermaid diagrams are
   displayed as plain text with a note.
5. **No state management:** Blad is stateless; it reads files and displays
   them without maintaining persistent state.

## Failure and recovery

- I/O errors are reported clearly to the user
- Missing optional tools (mmdc) fall back gracefully
- Terminal resize events are handled; content reflows
- Empty or tiny viewports do not panic

## Security and resource design

- Read-only access to session files (no writes without explicit edit mode)
- No network access required
- Paths validated; symlinks not followed when reading session artifacts
- Memory usage proportional to document size (streaming for large files)

## Verification strategy

- Integration tests exercise the full viewer pipeline
- Unit tests for individual rendering functions
- Manual testing of interactive navigation and search
- Compatibility testing with various Markdown dialects and extensions