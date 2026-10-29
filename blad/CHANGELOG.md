# Changelog

## 0.3.0 - Unreleased

First standalone crates.io release candidate, not yet published.

- Preserve unsupported HTML literally by default, with `--strip-html` to remove
  markup without losing text. Render expanded details/summary, block/line
  breaks, rules and common inline styles without executing HTML or attributes.
- Read Bash process substitutions and inherited descriptor paths as bounded
  UTF-8 snapshots, without canonicalizing pipe pseudo-targets or enabling
  device/FIFO reads, editing or automatic reload.
- Package name `kvist-blad`; executable and Rust library remain `blad`.
- Local Markdown viewing, stable tabs, literal/regex search, precise links,
  heading navigation and external editing.
- Debounced automatic file reload with retained viewport/search and recovery
  after failed reads; inactive tabs refresh when revisited.
- **Breaking:** terminal stdout now automatically pages piped/redirected stdin
  live (`cat FILE | blad` or `blad -`), without waiting for EOF. Use `--dump`
  to retain finite noninteractive output; redirected stdout remains finite
  and plain by default. Missing controlling terminals fail with a `--dump` hint.
- Bounded 16 MiB live source, split UTF-8 handling, provisional whole-document
  Markdown, timed/coalesced refresh, follow/pause/resume, retained EOF browsing
  and persistent input failures with nonzero exit after terminal restoration.
  Input continues across tabs/help; quit never drains or kills the producer.
- Quiet success when downstream readers close finite output early.
- Simplified viewer keys: `?` opens help, `N`/`<` move to previous matches,
  links need no mode, and file reload is automatic only. Removed `p`, `f` and
  `F5` actions; normal-mode `Ctrl+R` starts regex search and search-prompt
  `Ctrl+R` still toggles regex.
- Diagnostic logging retains one descriptor, rejects blocking or
  linked destinations and reports initialization failures visibly.
- Recompute existing matches immediately when toggling literal/regex search.
- Distinguish the current search occurrence with reverse video while keeping
  other matches underlined, including with color disabled and reversed source.
  FIND/SEARCH footer hints show query entry, confirmation and safe exit.
  Esc/q leave confirmed search instead of closing a tab; Enter advances after
  confirmation. Resize retains the current match ordinal when possible.
- Dark, light and terminal-native palettes; strict JSON palette overrides;
  explicit color policy and escape-free rendered dumps.
- Footnote prose retains its theme across wrapped rows, paragraphs and inline
  links, emphasis, code and math, without leaking into following prose.
- Syntax highlighting, wrapped tables, task completion markers, bounded
  mathematical text and five-family text-only Mermaid diagrams.
- General YAML frontmatter highlighting with literal source layout, searchable
  unwrapped code panels, prefix-only detection and preserved body navigation.
  Supports empty headers, BOM/CRLF and `---`/`...` closers without YAML execution
  or skill-schema validation; streaming snapshots update when a header closes.
- Table delimiter left/center/right alignment in headers and wrapped cells,
  with Unicode-aware padding and precise aligned link selection/hitboxes.
- Bold/italic/strikethrough combinations in table cells, preserving wrapping,
  row backgrounds, header reversal and styled link/search positions. Dedicated
  inline-code cell appearance and code-specific wrapping remain deferred.
- Self-contained package fixtures, unchanged AGPL license text and standalone
  package/installation verification.
- Separate opt-in Kvist/Skott snapshot/export integration with explicit
  bounded artifact errors and descriptor-relative no-follow message reads.

Supported executable target: Linux; x86_64 is verified. Rust 1.95 or newer.
This is not full CommonMark/GFM/TeX/Mermaid conformance, an unlimited-stream pager,
or a promise of hard resource/latency bounds. See the user guides for details.
