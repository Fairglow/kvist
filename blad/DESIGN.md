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

- I/O errors are reported clearly to the user
- Unsupported math/diagram syntax retains source with diagnostics
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