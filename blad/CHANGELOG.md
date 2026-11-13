# Changelog

## 0.3.4 (2026-11-13)

- Migrated the Mermaid backend to **Merman 0.10** (facade `Renderer` /
  `parse_semantic` / `artifact.render`; the semantic model and resource types
  moved to the `merman::model` crate). The five spatial families, all budgets,
  the 500 ms cooperative deadline and the four error categories are unchanged.
- Added **`treeView`** and **`mindmap`** through a Blad-owned bounded tree
  projection (4,096 nodes, depth 64) rendered from the typed model, so Merman's
  internal model-evidence framing (`name(bytes=N)="…"`, `[id=.., level=..]`, …)
  never reaches the default output.
- New **`--mermaid-tree text|merman`** flag / `RenderOptions.mermaid_tree`:
  `text` (default) uses Blad's own clean tree; `merman` is a debug pass-through
  of Merman's raw `structured_text` projection (best-effort; that framing is
  Merman-internal, not a stable documented format).
- Relaxed the `structured_text` restriction to **staged support**: the remaining
  structured families (`gitGraph`, `gantt`, `timeline`, `journey`, `kanban`,
  `packet`, `xychart`, and the rest) are rejected in text mode with an
  explanation and can be inspected via `--mermaid-tree merman`. Their internal
  Merman features are enabled for that inspection without adding external crates.
- Documented the new families (USAGE/CONTRACT/DESIGN/EXAMPLES) and added backlog
  tasks for the remaining `structured_text` families.

## 0.3.3 (2026-10-30)

- Preserve unsupported HTML literally by default, with `--strip-html` to remove
  markup without losing text. Render expanded details/summary, block/line
  breaks, rules and common inline styles without executing HTML or attributes.
- Read Bash process substitutions and inherited descriptor paths as bounded
  UTF-8 snapshots, without canonicalizing pipe pseudo-targets or enabling
  device/FIFO reads, editing or automatic reload.
- Upgraded dependencies (`pulldown-cmark` and others).

## 0.3.2 (2026-10-27)

- Render general **YAML frontmatter** as literal source in a searchable
  code panel: prefix-only detection, preserved body navigation, empty headers,
  BOM/CRLF and `---`/`...` closers, and streaming snapshot updates when a header
  closes. No YAML execution, validation or schema enforcement.
- Footnote prose retains its theme across wrapped rows, paragraphs and inline
  links, emphasis, code and math, without leaking into following prose.
- Clarified search modes and current-match highlighting: the current match uses
  reverse video, other matches stay underlined, with FIND/SEARCH footer hints.
- Isolated the native Mermaid regression and deadline test fixtures.
- Upgraded dependencies; unpinned the other workspace components.

## 0.3.1 (2026-10-26)

- **Breaking:** terminal stdout now automatically pages piped/redirected stdin
  live (`cat FILE | blad` or `blad -`), without waiting for EOF. Use `--dump`
  to retain finite noninteractive output; redirected stdout remains finite and
  plain by default. Missing controlling terminals fail with a `--dump` hint.
- Bounded 16 MiB live source, split UTF-8 handling, provisional whole-document
  Markdown, timed/coalesced refresh, follow/pause/resume, retained EOF browsing
  and persistent input failures with nonzero exit after terminal restoration.
  Input continues across tabs/help; quit never drains or kills the producer.
- Quiet success when downstream readers close finite output early.
- Simplified viewer keys: `?` opens help, `N`/`<` move to previous matches,
  links need no mode, and file reload is automatic only. Removed `p`, `f` and
  `F5` actions; normal-mode `Ctrl+R` starts regex search and search-prompt
  `Ctrl+R` still toggles regex.
- Table delimiter left/center/right alignment in headers and wrapped cells,
  with Unicode-aware padding and precise aligned link selection/hitboxes.
- Bold/italic/strikethrough combinations in table cells, preserving wrapping,
  row backgrounds, header reversal and styled link/search positions. Dedicated
  inline-code cell appearance and code-specific wrapping remain deferred.
- Upgraded Merman to 0.9 and other dependencies.

## 0.3.0 (2026-10-25)

First release of the standalone `kvist-blad` crate.

- Package name `kvist-blad`; executable and Rust library remain `blad`.
- Local Markdown viewing, stable tabs, literal/regex search, precise links,
  heading navigation, explicit reload and external editing.
- Debounced automatic file reload with retained viewport/search and recovery
  after failed reads; inactive tabs refresh when revisited.
- Bounded stdin snapshots (`cat FILE | blad` or `blad -`), plain pipeline
  output by default, and quiet success when downstream readers close early.
- Restored documented backward-search/reverse-match bindings and aligned key
  references; diagnostic logging retains one descriptor, rejects blocking or
  linked destinations and reports initialization failures visibly.
- Recompute existing matches immediately when toggling literal/regex search.
- Dark, light and terminal-native palettes; strict JSON palette overrides;
  explicit color policy and escape-free rendered dumps.
- Syntax highlighting, wrapped tables, task completion markers, bounded
  mathematical text and five-family text-only Mermaid diagrams (Merman 0.9.1).
- Self-contained package fixtures, unchanged AGPL license text and standalone
  package/installation verification.
- Separate opt-in Kvist/Skott snapshot/export integration with explicit
  bounded artifact errors and descriptor-relative no-follow message reads.

Supported executable target: Linux; x86_64 is verified. Rust 1.95 or newer.
This is not full CommonMark/GFM/TeX/Mermaid conformance, an unlimited-stream
pager, or a promise of hard resource/latency bounds. See the user guides for
details.
