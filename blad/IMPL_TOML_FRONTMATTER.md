<!-- kvist-implementation-record-version: 1 -->
# TOML Frontmatter Metadata Isolation — Implementation Record

**Date:** 2026-11-16
**Scope:** Source-only inspection of TOML frontmatter recognition, rendering isolation, and navigation exclusion. No runtime execution, fuzzing, or network interaction.

## 1. Prefix Recognition

### Delimiter Matching
- YAML frontmatter: opens with `---`, closes with `---` or `...`
- TOML frontmatter: opens with `+++`, closes with `+++` only
- Mismatched delimiters (`+++`/`---` or `---`/`+++`) are not recognized as frontmatter
- Delimiter lines may have trailing whitespace (spaces/tabs)
- BOM (`\u{feff}`) is stripped before recognition; not included in output
- CRLF line endings are normalized to LF in output

### Prefix-Only Constraint
- Frontmatter must appear at the very start of the document (after optional BOM)
- Leading blank lines or content before delimiters prevent recognition
- Unclosed frontmatter (no closing delimiter) is not recognized; falls through to Markdown parsing
- Multiple frontmatter blocks: only the first prefix block is recognized

**Evidence:** `blad/src/frontmatter.rs` functions `prefix()` and `delimiter()`; tests in `blad/tests/frontmatter.rs` (`frontmatter_is_prefix_only_and_unclosed_candidates_stay_markdown`, `toml_frontmatter_unclosed_and_mismatched_delimiters`, `toml_frontmatter_not_prefix_only`).

## 2. Rendering Isolation

### Code Block Projection
- Frontmatter content is rendered as a fenced code block (`yaml` or `toml` language tag)
- Source is preserved verbatim (whitespace, layout, indentation)
- No Markdown parsing within frontmatter: `**bold**`, `[link](url)`, `$math$`, etc. remain literal
- No link or definition extraction from frontmatter content
- Invalid/malformed YAML or TOML is displayed as-is (no validation)

### Terminal Control Inertness
- Control characters in frontmatter are escaped via `inert_source()`
- Unicode bidirectional override characters (U+202A–U+202E, U+2066–U+2069) are escaped
- Prevents terminal injection or bidirectional text confusion from metadata

**Evidence:** `blad/src/frontmatter.rs` function `events()`; tests in `blad/tests/frontmatter.rs` (`frontmatter_preserves_yaml_layout_and_literal_markdown`, `frontmatter_keeps_invalid_yaml_visible_and_terminal_controls_inert`, `toml_frontmatter_malformed_is_still_displayed`).

## 3. Navigation Exclusion

### Link Extraction
- `extract_links()` operates on the full source but frontmatter content is excluded from link discovery
- URLs in frontmatter (e.g., `url: https://example.com`) are not extracted as navigable links
- Reference-style links in frontmatter are not registered
- Body link offsets are preserved relative to the full source (accounting for frontmatter prefix length)

### Source Offset Handling
- Frontmatter events are generated with their own source ranges
- Body events have offsets adjusted by the frontmatter prefix length
- Link source offsets are always >= frontmatter length

**Evidence:** `blad/src/frontmatter.rs` function `events()`; `blad/src/links.rs`; tests in `blad/tests/frontmatter.rs` (`frontmatter_never_creates_links_or_definitions_and_body_offsets_survive`, `toml_frontmatter_never_creates_links_or_definitions`); `blad/tests/autolink_extended.rs` (`frontmatter_excluded`).

## 4. Absence of Metadata Authority

- Frontmatter is not parsed, validated, or interpreted as structured data
- No schema enforcement, type checking, or field validation
- No runtime behavior changes based on frontmatter content
- Frontmatter is purely presentational: rendered as highlighted code, excluded from navigation
- No dependency on external TOML/YAML parsing libraries

**Evidence:** Source inspection of `blad/src/frontmatter.rs` — no TOML/YAML parser imports; frontmatter content is passed through as raw text within code block events.

## 5. Error Propagation and Cancellation

- Writer errors during frontmatter rendering propagate as `io::Error`
- Render cancellation is checked and honored during frontmatter rendering
- Cancellation produces `io::ErrorKind::Interrupted` with empty output

**Evidence:** `blad/src/frontmatter.rs` function `events()`; tests in `blad/tests/frontmatter.rs` (`frontmatter_propagates_writer_errors_and_cancellation`, `toml_frontmatter_propagates_writer_errors_and_cancellation`).

## 6. Evidence Limits

- No runtime testing, fuzzing, or network interaction performed
- Tests reviewed but not executed
- No inspection of downstream navigation code (how links are followed after extraction)
- No verification of TOML/YAML syntax validation behavior (not applicable — no validation performed)

## 7. Findings Summary

| Area | Status | Notes |
|------|--------|-------|
| Prefix recognition | Well-defined | Clear delimiter matching rules; BOM/CRLF handled |
| Rendering isolation | Complete | Code block projection; no Markdown parsing within frontmatter |
| Navigation exclusion | Complete | No link/definition extraction from frontmatter |
| Metadata authority | None | No parsing, validation, or runtime behavior changes |
| Error handling | Robust | Writer errors and cancellation properly propagated |
