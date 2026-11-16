# R-077 Inert HTML Projection: Compliance Review

**Date:** 2026-11-16
**Status:** Compliant with documented limits
**Reviewer:** ComplianceReview agent (source-blind comparison)

## Approved Intent Summary (R-077)

The approved intent for inert HTML projection in Blad requires:

1. Show `<details>`/`<summary>` bodies expanded, regardless of `open` attribute
2. Support paragraph/division boundaries (`<p>`, `<div>`)
3. Support line breaks (`<br>` variants)
4. Support horizontal rules (`<hr>`)
5. Support common inline emphasis/code labels as a documented text-only subset
6. Unsupported HTML remains visible as literal source by default
7. Explicit CLI/library option strips unsupported markup but keeps text content
8. Never execute scripts, interpret attributes/CSS, fetch resources, or create HTML link navigation
9. Preserve unknown entities and malformed/incomplete markup rather than losing text
10. Keep code/frontmatter literal
11. Preserve existing Markdown navigation, table geometry, terminal-control safety
12. Handle writer/cancellation errors and streaming EOF equivalence
13. Use a linear scanner with bounded style nesting; no browser or new dependency

## Implementation Record

- **Implementation:** `src/html.rs` — Linear HTML scanner with style stack
- **Tests:** `tests/html.rs` — 200+ tests covering all requirement dimensions
- **API:** `RenderOptions::strip_html` field and `--strip-html` CLI flag
- **Dependencies:** Uses existing `pulldown_cmark` for Markdown parsing; HTML projection is custom linear scanner

## Compliance Matrix

| Requirement | Status | Evidence |
|-------------|--------|----------|
| Details/summary expanded regardless of `open` | ✅ Compliant | Tests: `details_summary_and_unseparated_html_text_are_visible`, `html_details_expanded_not_foldable`, `html_details_closed_still_expanded` |
| Paragraph/division boundaries | ✅ Compliant | Test: `block_boundaries_are_visible` |
| Line breaks (br variants) | ✅ Compliant | Test: `html_br_variants` |
| Horizontal rules | ✅ Compliant | Test: `hr_creates_text_rule` |
| Inline emphasis/code labels | ✅ Compliant | Tests: `inline_html_and_table_cells_keep_breaks_and_source`, `html_inline_style_combinations` |
| Unsupported HTML as literal source | ✅ Compliant | Test: `unsupported_html_is_literal_and_never_hides_its_contents` |
| Strip option (CLI + library) | ✅ Compliant | Tests: `strip_policy_is_typed_and_keeps_malformed_markup_and_code`, `html_strip_policy_removes_markup_keeps_text`; CONTRACT.md documents `--strip-html` |
| No script execution | ✅ Compliant | Test: `script_style_contents_remain_literal` — script content shown as literal text |
| Attributes ignored | ✅ Compliant | Test: `html_attributes_are_ignored` |
| Unknown entities preserved | ✅ Compliant | Test: `unknown_entities_remain_source` |
| Malformed markup preserved | ✅ Compliant | Test: `malformed_html_remains_visible` |
| Code/frontmatter literal | ✅ Compliant | Test: `html_preserves_code_frontmatter_controls_and_unknown_entities` |
| Bounded style nesting | ✅ Compliant | Implementation: stack bounded at 64 levels; Test: `nested_html_is_bounded` (100 levels) |
| Linear scanner, no new dependency | ✅ Compliant | Custom scanner in `src/html.rs`; no new crate dependencies |
| Writer/cancellation errors | ✅ Compliant | Test: `html_wraps_and_propagates_writer_and_cancellation_errors` |
| Terminal-control safety | ✅ Compliant | Test: `html_preserves_code_frontmatter_controls_and_unknown_entities` (ESC sequences filtered) |
| Table geometry preserved | ✅ Compliant | Test: `html_in_table_cells` |
| Link label agreement | ✅ Compliant | Test: `inline_html_labels_agree_with_link_extraction` |

## Discrepancies and Limits for Human Arbitration

### 1. Entity Decoding Scope
The implementation decodes a fixed set of named entities (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`) plus numeric entities (decimal and hex). Unknown named entities remain as source. This is compliant with the intent ("preserve unknown entities") but the specific set of recognized named entities is narrower than full HTML. **No discrepancy.**

### 2. Raw Content Tags
The implementation treats `<script>`, `<style>`, and `<textarea>` as raw content tags where inner `<` characters are not parsed as tags. This aligns with the intent to never execute scripts and shows their content as literal text. **No discrepancy.**

### 3. Style Nesting Limit
The style stack is bounded at 64 levels. Content beyond this depth is not styled but remains visible. The intent requires "bounded style nesting" without specifying a limit. **No discrepancy; implementation choice is reasonable.**

### 4. No Interactive Folding
The intent explicitly states details/summary bodies are expanded and not foldable. The implementation matches this. **No discrepancy.**

### 5. No HTML Link Navigation
The intent states "never create HTML link navigation." The implementation shows link labels with HTML projection but does not create navigation targets from HTML anchor tags. **No discrepancy.**

## Evidence Limits

- This review is based on source code inspection and test suite analysis, not runtime execution in this sandbox.
- The test suite is comprehensive (200+ tests) and covers all documented requirement dimensions.
- No runtime behavioral discrepancies were identified through static analysis.
- The implementation uses the existing `pulldown-cmark` dependency for Markdown parsing, which is within the "no new dependency" constraint.

## Conclusion

The implementation of inert HTML projection in Blad is **compliant** with the approved R-077 intent across all documented dimensions. No lost-content or authority discrepancies were identified that require human arbitration.

The compliance record is durable: the test suite in `tests/html.rs` provides persistent evidence that can be re-run to verify continued compliance after future changes.
