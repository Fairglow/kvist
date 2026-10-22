# Blad Enhanced Navigation Feature Plan

## Features

1. **SPACE for page down** - Like common pagers (less, more) — *implemented*
2. **Tab/Shift+Tab link navigation** - Cycle through clickable links — *implemented*
3. **Enter to follow links** - Open linked files or URLs — *implemented*
4. **Back navigation** - Return to previous file after following a link — *implemented*
5. **Browser opening for URLs** - Open http(s)/mailto/tel links in default browser — *implemented*
6. **Tab bar with multiple files** - Show open files in a top bar — *implemented*
7. **Alt+Left/Right to switch tabs** - Navigate between open tabs — *implemented*

## What was implemented

Open files form reusable tabs with per-file vertical/horizontal positions.
The active tab appears first, bracketed, in the top row. `Alt+Left`/`Alt+Right`
switch tabs; `b` follows tab visit history. Search/link/error status is in the
footer. Tab/Shift+Tab select links without a special mode, Enter follows them,
and inline/named labels support left-click hit testing.

Link destinations are classified into three kinds:

- **Anchor** (`#fragment`) — resolved to rendered heading rows, including
  explicit IDs, duplicate heading slugs and percent-encoded UTF-8.
- **URL** (`http://`, `https://`, `mailto:`, `tel:`) — opened in the default
  browser via `xdg-open`; the viewer stays on the current file.
- **File** (anything else) — resolved against the current file's directory
  and opened in the viewer, including `file.md#heading`.

## Deliberately not added

An embedded/per-section editor remains deferred: `e` already supplies an
explicit whole-file editor handoff and rereads after saving. `~~~` remains a
CommonMark code fence. ER endpoint cardinalities remain textual Mermaid tags,
as described in `docs/USAGE.md`, rather than graphical crow's-foot glyphs.

## Testing Strategy

- Unit tests for link extraction, classification and path resolution
  (`src/links.rs`).
- PTY-driven integration tests that follow a file link and navigate back, and
  that verify link mode is inert without links and that `Esc` exits it
  (`tests/viewer.rs`).
- Browser errors remain visible in the viewer; rendering never launches a process.
