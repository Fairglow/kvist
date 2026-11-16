# Compliance Review: TOML Frontmatter Intent vs. Implementation

**Task:** blad-toml-frontmatter-compliance-review
**Date:** 2026-11-16
**Reviewer:** ComplianceReview agent (source-blind comparison)
**Status:** Complete — discrepancies retained for human arbitration

## Scope

Compare approved intent (R-075 in `REQUIREMENTS.md`) against the independent
implementation record (`IMPL_TOML_FRONTMATTER.md`) and associated tests.
This review is source-blind: it compares documented intent against documented
evidence, not against source code directly.

## Intent Summary (R-075)

Recognize a single leading YAML or TOML metadata block at document start:
- YAML: `---` opener, `---` or `...` closer
- TOML: `+++` opener and closer
- Preserve delimiters, indentation, comments, ordering, blank lines, multiline scalars
- Display as syntax-highlighted code panel
- Support empty headers, BOM, LF/CRLF, trailing delimiter whitespace
- Only unindented opener on first line qualifies
- Unclosed candidates retain normal Markdown parsing (streaming may complete later)
- No validation, deserialization, reformatting, or execution
- Invalid content remains visible as source
- Metadata contributes no headings, links, definitions, or footnotes
- Preserve body offsets, navigation, searchability, reflow, theme, inert controls

## Implementation Record Summary

The implementation record documents:
- Prefix-only recognition with delimiter matching
- TOML uses `+++`/`+++`; YAML uses `---`/`---` or `---`/`...`
- Mismatched delimiters not recognized
- BOM stripped before recognition, not in output
- CRLF normalized to LF in output
- Unclosed frontmatter falls through to Markdown parsing
- Rendered as fenced code block with language tag
- Source preserved verbatim (whitespace, layout, indentation)
- No Markdown parsing within frontmatter
- No link/definition extraction from frontmatter
- Body link offsets adjusted for frontmatter prefix length
- No TOML/YAML parser imports (no validation/deserialization)
- Terminal controls escaped via `inert_source()`
- Writer errors propagate; cancellation honored

## Discrepancy Analysis

### D-001: CRLF Normalization vs. Preservation
- **Intent:** "Support ... LF/CRLF" (does not specify normalization)
- **Evidence:** "CRLF line endings are normalized to LF in output"
- **Assessment:** Minor. Requirement says "support" not "preserve" line ending style.
  Normalization is a reasonable implementation choice. Not a contradiction.

### D-002: BOM Handling
- **Intent:** "Support ... optional initial UTF-8 BOM"
- **Evidence:** "BOM is stripped before recognition; not included in output"
- **Assessment:** Aligned. "Support" means accept, not necessarily preserve.
  Stripping BOM is standard practice.

### D-003: Unclosed Candidates and Streaming Completion
- **Intent:** "Unclosed candidates retain normal Markdown parsing, including
  provisional streaming snapshots; later input may complete the header."
- **Evidence:** "Unclosed frontmatter (no closing delimiter) is not recognized;
  falls through to Markdown parsing."
- **Assessment:** Partial alignment. The implementation record confirms unclosed
  candidates fall through to Markdown parsing, but does not explicitly document
  the streaming behavior where later input may complete the header. The
  implementation likely handles this (since it's prefix-only and re-evaluated
  on new input), but the evidence record is silent on this specific aspect.
  **Retain for arbitration.**

### D-004: Token Color Verification
- **Intent:** "Verify ... effective YAML/TOML token colors"
- **Evidence:** Mentions syntax highlighting via code block presentation but no
  explicit token color verification.
- **Assessment:** Minor gap in evidence coverage. The implementation uses the
  existing code panel presentation which provides syntax highlighting, but the
  record does not confirm specific token colors were verified. **Retain for
  arbitration.**

### D-005: CLI File/Stdin Acceptance
- **Intent:** Mentions "CLI file/stdin acceptance"
- **Evidence:** Not explicitly addressed in the implementation record.
- **Assessment:** Implied by general file/stdin handling but not explicitly
  documented in the TOML frontmatter evidence. **Retain for arbitration.**

## Alignment Summary

| Aspect | Aligned | Notes |
|--------|---------|-------|
| Prefix-only recognition | ✅ | |
| Delimiter matching (YAML/TOML) | ✅ | |
| Mismatched delimiters rejected | ✅ | |
| Unclosed falls through to Markdown | ✅ | Streaming aspect not documented |
| No validation/deserialization | ✅ | No parser imports |
| Verbatim source preservation | ✅ | |
| Code block presentation | ✅ | |
| No link/definition extraction | ✅ | |
| Body offset preservation | ✅ | |
| Terminal control inertness | ✅ | |
| Writer error propagation | ✅ | |
| Cancellation handling | ✅ | |
| CRLF handling | ⚠️ | D-001 (normalization vs. preservation) |
| BOM handling | ⚠️ | D-002 (stripped vs. preserved) |
| Streaming completion | ⚠️ | D-003 (not documented in evidence) |
| Token color verification | ⚠️ | D-004 (not explicitly verified) |
| CLI acceptance documentation | ⚠️ | D-005 (not explicitly documented) |

## Evidence Limits

1. The implementation record is based on source inspection, not runtime execution.
2. No fuzzing or property-based testing is documented.
3. Token color verification is not explicitly documented.
4. Streaming behavior for unclosed candidates is not explicitly documented.
5. CLI file/stdin acceptance is not explicitly documented in the TOML-specific evidence.

## Recommendations

1. **Human arbitration needed for:** D-003 (streaming completion documentation),
   D-004 (token color verification), D-005 (CLI acceptance documentation).
2. **Low priority:** D-001 and D-002 are minor interpretation differences that
   are unlikely to affect users.
3. **Suggest documentation update:** The implementation record could be enhanced
   to explicitly document streaming behavior for unclosed frontmatter candidates.

## Conclusion

The implementation record aligns with the approved intent for the core TOML
frontmatter features: prefix-only recognition, delimiter matching, verbatim
preservation, code block presentation, navigation exclusion, and error handling.
Five minor discrepancies are retained for human arbitration, primarily related
to documentation completeness rather than functional contradictions.