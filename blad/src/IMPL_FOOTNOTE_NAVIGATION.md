# Footnote Navigation Security Audit

## Target Identity
Footnote links are identified by the `#fn-` prefix in `classify_destination()` (`links.rs:16`). The prefix constant `FOOTNOTE_ANCHOR_PREFIX` is `"fn-"`. Any link destination starting with `#fn-` is classified as `LinkKind::Footnote`.

## Navigation Flow
1. `extract_links()` (`links.rs`) converts `Event::FootnoteReference` into `Link` objects with `kind: LinkKind::Footnote` and destination `#fn-{id}`.
2. In `display.rs`, pressing Enter on a link calls `follow_link()`. For footnotes, the current scroll position is saved to `history` before jumping.
3. The jump uses `anchor_row()` which looks up the anchor in `document.navigation.anchors`. If not found, a status message is shown ("footnote definition not found: #fn-X").
4. Pressing 'b' triggers `ViewerAction::FootnoteBack(row)` which restores the saved position.

## Back Navigation
- History is a simple `Vec` of scroll offsets (`display.rs:212`).
- Unbounded in size — grows with each footnote jump.
- No eviction or cap.

## Error Handling
- Missing footnote definitions: handled gracefully with status message; no crash.
- Malformed footnote IDs: any string is accepted; no validation.
- Links inside footnote definitions: still extracted and navigable.

## Potential Issues
1. **Unbounded history**: The history stack grows without limit; a user could exhaust memory by repeatedly jumping to footnotes.
2. **No ID validation**: Footnote IDs are arbitrary strings; no check for well-formedness.
3. **Prefix collision**: A manually defined anchor named `fn-something` would be treated as a footnote.
4. **No circular reference protection**: Deep or circular footnote chains are not detected.

## Test Coverage
- `tests/footnotes.rs` covers link extraction, ID formatting, and definitions not being treated as links.
- No integration tests for actual navigation behavior (jumping, back navigation).
- No tests for error cases (missing definitions, malformed IDs).

## Evidence Limits
- Source inspection only; no runtime observation.
- No integration tests executed.
- No stress tests for unbounded history.
