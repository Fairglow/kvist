# Blad Production Quality Plan

## Current State

Blad is a functional viewer for skott session transcripts with:

- Session listing, viewing, streaming, exporting
- Markdown rendering with syntax-highlighted code blocks
- Interactive viewer with scroll navigation
- Basic test coverage (6 unit, 4 integration tests)

## Goals

Make Blad best-in-class as a terminal Markdown viewer. Production quality,
robust, reliable, resilient, fully tested, idiomatic Rust.

## Work Items

### 1. Full GitHub Flavored Markdown Support

**Requirements:**

- Tables (already supported)
- Strikethrough (already supported)
- Task lists (already supported)
- Footnotes
- Autolinks
- Blockquote tags ([!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION])
- Superscript/subscript
- Emoji (pass through)

**Tasks:**

- [ ] Enable additional pulldown-cmark options (ENABLE_GFM, ENABLE_FOOTNOTES, ENABLE_SUPERSCRIPT, ENABLE_SUBSCRIPT)
- [ ] Handle BlockQuoteKind event for alert styles
- [ ] Handle FootnoteReference event
- [ ] Handle InlineMath and DisplayMath events
- [ ] Handle Superscript/Subscript events
- [ ] Write tests for each new feature

### 2. Search Functionality

**Requirements:**

- Type `/` to enter search mode
- Highlight all matches
- Navigate between matches with `n` (next) and `N` (previous)
- Show match count (3/12)
- Escape to exit search
- Search should work on the full rendered content

**Tasks:**

- [ ] Add search state to viewer
- [ ] Implement `/` key to trigger search input
- [ ] Highlight matches with ANSI background color
- [ ] Implement `n`/`N` navigation between matches
- [ ] Show match position in footer
- [ ] Write tests for search

### 3. Edit Mode

**Requirements:**

- Optional `--edit` flag or `e` key to enter edit mode
- Opens file in $EDITOR (or nvim/vim as fallback)
- Re-renders after edit completes
- Works on standalone markdown files

**Tasks:**

- [ ] Add edit mode support
- [ ] Detect and use $EDITOR
- [ ] Handle re-render after edit
- [ ] Add `--edit` CLI flag
- [ ] Write tests for edit mode

### 4. Mermaid Diagram Support

**Requirements:**

- Detect ` ```mermaid ` fenced code blocks
- Try to render using available tools
- Fallback: display as plain text with a note

**Approach:**

- Use subprocess to call `mmdc` (mermaid-cli) if available
- Render to temporary SVG, convert to ASCII or display as image path
- Fallback: show diagram source with "Mermaid diagram (install mmdc to render)"

**Tasks:**

- [ ] Detect mermaid code blocks
- [ ] Try `mmdc` subprocess rendering
- [ ] Handle fallback display
- [ ] Write tests for mermaid support

### 5. Documentation

**Requirements:**

- User's Guide with examples
- Command reference
- Keyboard shortcuts reference
- Feature documentation

**Tasks:**

- [ ] Write comprehensive docs/USAGE.md
- [ ] Update README.md with features and examples
- [ ] Document all keyboard shortcuts
- [ ] Add examples for each command

### 6. Kvist Component Compliance

**Requirements:**

- REQUIREMENTS.md matches implementation
- CONTRACT.md matches implementation
- DESIGN.md matches implementation
- IMPL.md derived from code
- TODOS.yaml up to date

**Tasks:**

- [ ] Rewrite REQUIREMENTS.md to reflect actual implementation
- [ ] Update CONTRACT.md to match CLI interface
- [ ] Update DESIGN.md to match code architecture
- [ ] Regenerate IMPL.md from code
- [ ] Update TODOS.yaml with remaining work

### 7. Code Quality and Tests

**Requirements:**

- Comprehensive test coverage
- Error handling review
- Performance review
- Idiomatic Rust patterns

**Tasks:**

- [ ] Add tests for all new features
- [ ] Review error handling (no unwrap for recoverable errors)
- [ ] Add benchmarks for rendering performance
- [ ] Code review pass

## Execution Order

1. Full GFM support (foundational rendering improvements)
2. Search functionality (high-value user feature)
3. Edit mode (optional but requested)
4. Mermaid support (optional, complex)
5. Documentation
6. Kvist compliance
7. Code quality pass

## Status Tracking

- [x] Full GFM support
- [x] Search functionality
- [x] Edit mode
- [x] Mermaid support
- [x] Documentation
- [x] Kvist component compliance
- [x] Code quality and tests
