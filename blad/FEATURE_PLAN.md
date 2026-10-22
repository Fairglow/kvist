# Blad Enhanced Navigation Feature Plan

## Features

1. **SPACE for page down** - Like common pagers (less, more) — *implemented*
2. **Tab/Shift+Tab link navigation** - Cycle through clickable links — *implemented*
3. **Enter to follow links** - Open linked files or URLs — *implemented*
4. **Back navigation** - Return to previous file after following a link — *implemented*
5. **Browser opening for URLs** - Open http(s)/mailto/tel links in default browser — *implemented*
6. **Tab bar with multiple files** - Show open files in a top bar — *deferred*
7. **Alt+Left/Right to switch tabs** - Navigate between open tabs — *deferred*

## What was implemented

The open files form a **navigation stack**: the file you are viewing is the
top of the stack. Following a file link pushes a new file onto the stack;
pressing `b` pops back to the previous file. There is no tab bar and no
side-by-side tab switching — that is the deferred work in items 6 and 7.

Link destinations are classified into three kinds:

- **Anchor** (`#fragment`) — recognised, not yet resolved to a scroll offset.
- **URL** (`http://`, `https://`, `mailto:`, `tel:`) — opened in the default
  browser via `xdg-open`; the viewer stays on the current file.
- **File** (anything else) — resolved against the current file's directory
  and opened in the viewer.

## Deferred

A visible tab bar and `Alt+Left`/`Alt+Right` switching would replace the
stack model with a flat list of open files. That is a larger change to the
viewer's state and rendering and is intentionally out of scope for this pass.

## Testing Strategy

- Unit tests for link extraction, classification and path resolution
  (`src/links.rs`).
- PTY-driven integration tests that follow a file link and navigate back, and
  that verify link mode is inert without links and that `Esc` exits it
  (`tests/viewer.rs`).
- Browser opening is exercised by the URL-classification unit tests; the
  `xdg-open` spawn itself is not asserted in CI.
