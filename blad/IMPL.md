<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

**Date:** 2026-01-15
**Scope:** Source-only inspection of Blad's autolink recognition, navigation, and control-escape behavior. No runtime execution, fuzzing, or network interaction.

## 1. Recognition Bounds

### Recognized as bare autolinks
- `http://` and `https://` URLs (any path, query, fragment, port)
- Bare `www.` URLs where the host label after the prefix begins with an alphanumeric character
- Email-like tokens: alphanumeric local part, `@`, nonempty domain; plus-sign and dot variants allowed
- URLs containing balanced parentheses, Unicode characters, query strings, fragments, port numbers

### Explicitly excluded
- URLs inside fenced or indented code blocks
- URLs inside inline code spans
- URLs already inside an existing link destination (no double-extraction)
- `mailto:` and `tel:` scheme strings (classified separately, not as bare autolinks)
- Filenames containing colons (e.g., `notes:2024.md`, `C:foo`)
- `www..example.com` (empty host label after prefix)
- `www-example.com` (no host part after prefix)
- Tokens where `@` appears without a preceding alphanumeric local part or following domain

### Boundary handling
- Trailing sentence punctuation (`.`, `,`, `;`, `!`, `?`, `:`) is trimmed from URL matches
- Unbalanced closing `)` terminates the URL; balanced `()` pairs are included
- URLs at line start/end are recognized
- Multiple bare URLs on one line are each captured

**Evidence:** `blad/src/links.rs` (functions `bare_urls`, `scan_url`, `scan_email`, `is_possible_email_start`); test coverage in `blad/tests/autolink_extended.rs` (44 tests) and `blad/src/links.rs` inline tests.

## 2. Untrusted Destinations

- No allowlist or domain validation: any recognized URL is navigable
- Bare `www.` URLs are automatically prefixed with `https://` for navigation (label remains unchanged)
- Email addresses are prefixed with `mailto:` for navigation (label remains unchanged)
- `http://` and `https://` URLs retain their original scheme
- No HTTPS enforcement for `http://` URLs
- No length limits on URL or email tokens (except 64-char local part heuristic in `is_possible_email_start`)

**Evidence:** `blad/src/links.rs` function `normalize_bare_destination`.

## 3. Inert Control Escaping

The `inert_source()` function in `blad/src/terminal_text.rs` is applied to all text before bare URL scanning:

- Strips `\r` when followed by `\n`
- Escapes all control characters (except `\n` and `\t`) as `\u{xxxx}`
- Escapes Unicode bidirectional override characters: U+202A–U+202E and U+2066–U+2069
- Returns borrowed string unchanged if no control characters are present (optimization)

This prevents injection of terminal control sequences or bidirectional text confusion into extracted link labels.

**Evidence:** `blad/src/terminal_text.rs` function `inert_source`; called in `blad/src/links.rs` during bare URL scanning.

## 4. Absence of Implicit Launches

- No `open_url`, `exec`, or similar process-launching function found in the autolink code path
- Link extraction is passive: `extract_links()` returns a `Vec<Link>`; navigation is caller-initiated
- Module documentation states Tab/Shift+Tab/Enter navigation is user-driven
- No automatic browser invocation or file open on extraction

**Evidence:** Source inspection of `blad/src/links.rs`; grep for `open_url` returned no matches in the blad component.

## 5. Evidence Limits

- No runtime testing, fuzzing, or network interaction performed
- No inspection of downstream navigation code (how links are actually followed after extraction)
- No verification of URL encoding/decoding behavior for special characters
- Tests reviewed but not executed
- No inspection of HTML link parsing (only bare autolink scanning)

## 6. Findings Summary

| Area | Status | Notes |
|------|--------|-------|
| Recognition bounds | Well-defined | Clear inclusion/exclusion criteria; extensive test coverage |
| Untrusted destinations | Open | No allowlist; any recognized URL is navigable |
| Control escaping | Present | `inert_source()` escapes controls and bidi overrides |
| Implicit launches | None found | Extraction is passive; navigation is caller-initiated |
| Email recognition | Lenient | 64-char local part heuristic; no domain validation |
| Scheme handling | Conservative | `www.` → `https://`; `http://` preserved; no enforcement |
