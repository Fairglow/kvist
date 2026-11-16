# Table Span Composition: Independent Observation

**Date:** 2026-01-15
**Scope:** Source-only inspection of `blad/src/table_spans.rs`, `blad/src/links.rs`, and `blad/src/terminal_text.rs`. No runtime execution.

## 1. Span Bounds and Indexing

- `TableSpans::append` records spans as `bytes: start..end` where `start` is the cell string length before appending. Spans are always contiguous within the cell string; no gaps or overlaps are introduced.
- Empty text or default style is skipped, preventing zero-length spans.
- Adjacent spans with identical style are merged in `append` (end extension) and `push_span` (byte and column extension), reducing span count.
- `wrapped` projects source spans onto wrapped lines by advancing `source_cursor` and `mark_cursor` monotonically. Each word is located via `find` in the remaining source; if not found, an explicit `io::Error::InvalidData` is returned.
- `mark_cursor` never decrements or skips backwards, ensuring each span is applied exactly once.
- `write_cell` uses `text.get(cursor..span.bytes.start)` and `text.get(span.bytes.clone())` for slicing. If a span is out of bounds, `get` returns `None` and the operation fails with `io::Error::InvalidData`. No `unwrap()` or `expect()` is used.

## 2. Unicode and Grapheme Clipping

- `wrapped` uses `TerminalLine::column_ranges` to map byte ranges to column ranges, handling multi-byte UTF-8 and wide characters.
- If the number of column ranges does not match the number of overlaps, an explicit error is returned.
- Split graphemes (base character + combining diacritic) are handled by keeping the byte range for the base and the column range for the whole grapheme. The test `split_grapheme_styles_keep_safe_bytes_and_whole_link_columns` verifies this.
- No assumption that bytes map 1:1 to columns or graphemes.

## 3. Inert Controls

- `links.rs` applies `crate::terminal_text::inert_source` to bare URL text before scanning for autolinks, stripping ANSI escape sequences and bidirectional override characters. This prevents crafted escape sequences from being interpreted as part of a URL.
- `write_cell` generates SGR sequences for styling, but these are produced by the renderer, not from user input. Link destinations are passed to `mark_link_columns` for navigation metadata, not for terminal output.

## 4. Fallible Writes and Resource Behavior

- All I/O operations in `table_spans.rs` and `links.rs` return `io::Result` or `Option`. No `unwrap()` or `expect()` in production code paths.
- `write_cell` and `write_span_style` propagate I/O errors via `?`.
- `wrapped` propagates errors from `find` and `column_ranges`.
- No file handles, sockets, or other OS resources are opened or retained. The code is purely functional with no side effects beyond writing to the provided writer.

## 5. Link Destination Classification

- `classify_destination` is total: every destination is classified as `Anchor`, `Url`, or `File`.
- `#fragment` links → `Anchor`.
- `mailto:` and `tel:` → `Url`.
- Scheme URLs (`scheme://`) → `Url` only if scheme matches `[A-Za-z0-9+.-]+`, preventing filenames like `notes:2024.md` from being misclassified.
- All other destinations → `File`.

## 6. Risks and Limitations

- Span composition assumes wrapped lines preserve original tokens. If wrapping alters tokens, `wrapped` fails with an explicit error.
- Link destination classification does not validate URLs or file paths. Malicious destinations could open unexpected resources.
- Bare autolink scanning uses heuristics. Edge cases (unusual characters) may not be recognized or may be recognized incorrectly.
- No sandboxing when following links. The user's browser or file manager is invoked directly.
- No inspection of downstream navigation code (how links are actually followed after extraction).

## 7. Evidence

- Source inspection of `blad/src/table_spans.rs`, `blad/src/links.rs`, `blad/src/terminal_text.rs`.
- Unit tests in `blad/src/table_spans.rs` (normalized spaces, split graphemes, projection failures).
- Unit tests in `blad/src/links.rs` (link extraction, bare URLs, email scanning, destination classification).
- Existing IMPL.md autolink recognition record (cross-referenced).
