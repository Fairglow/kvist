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

## 7. HTML Projection Security Audit

**Date:** 2026-01-15
**Scope:** Source-only inspection of `blad/src/html.rs` and `blad/tests/html.rs`. No runtime execution, fuzzing, or network interaction.

### 7.1 Parser Bounds

- Tag names validated: must start with ASCII alphabetic, followed by alphanumeric or hyphen
- Attribute syntax validated: name, optional `=` and quoted/unquoted value
- Malformed tags (missing `>`, invalid syntax) treated as literal text, not parsed
- Stack depth bounded to 64 nested style tags
- Raw elements (`script`, `style`, `textarea`) use case-insensitive close tag search
- Comment handling: `<!--` ... `-->` recognized; incomplete comments treated as literal text
- Tokenization handles quoted attributes with embedded `>` and `<`

### 7.2 Malformed Input Handling

- Unmatched closing tags: treated as literal text, do not pop stack
- Unbalanced nesting: content preserved, extra closers become literal text
- Missing attribute quotes: `attributes()` fails, tag becomes literal text
- Incomplete tags: `token_end()` returns None, text becomes literal
- Double `<` (`<<`) causes `token_end()` to return None, treated as literal
- Empty or whitespace-only tag content: preserved as-is

### 7.3 Active-Content Isolation

- No script execution: `<script>` content kept as literal text
- No style interpretation: `<style>` content kept as literal text
- No DOM manipulation: pure text-to-terminal transformation
- Style tags (`b`, `i`, `s`, `code`, etc.) map to terminal text attributes only
- `<br>` and `<hr>` produce terminal newlines/rules, not HTML elements
- No CSS parsing or application
- No JavaScript evaluation

### 7.4 Entity and Control Character Handling

- Named entities: `amp`, `lt`, `gt`, `quot`, `apos`, `nbsp` decoded
- Numeric entities: decimal (`&#65;`) and hex (`&#x41;`) decoded
- Long zero-padded numeric entities handled (tested with 512 leading zeros)
- Invalid entities: remain as source text
- NUL character: filtered from decoded entities
- Surrogate codepoints (U+D800–U+DFFF): filtered
- Out-of-range codepoints (> U+10FFFF): filtered
- Control characters in decoded entities: pass through (e.g., `&#27;` → ESC)

### 7.5 Resource Fetching and Execution

- No network calls: no HTTP requests, no URL fetching
- No file operations: no file reads or writes
- No process spawning: no `exec`, `spawn`, or similar
- No external tool invocation
- Pure in-memory text transformation

### 7.6 Evidence Limits

- No runtime testing, fuzzing, or network interaction performed
- No inspection of downstream navigation code
- Tests reviewed but not executed
- No verification of terminal emulator behavior for decoded control characters
- No inspection of browser rendering comparison

### 7.7 Findings Summary

| Area | Status | Notes |
|------|--------|-------|
| Parser bounds | Well-defined | Clear validation rules; malformed input becomes literal text |
| Malformed input | Conservative | Content preserved; no silent dropping or crashes |
| Active-content isolation | Strong | Script/style/textarea contents kept as literal text |
| Entity handling | Robust | Wide range of entities decoded; invalid ones preserved |
| Control characters | Permissive | Decoded entities can produce control characters (e.g., ESC) |
| Resource fetching | None | Pure text transformation; no I/O or execution |
| Stack depth | Bounded | 64-level limit prevents unbounded nesting |

## 8. Definition List Handling Audit

**Scope:** Source-only inspection of `blad/src/markdown.rs` definition list normalization and rendering. No runtime execution, fuzzing, or network interaction.

### 8.1 Normalization Bounds

The `normalize_definition_lists()` function preprocesses source before parsing:

- Converts `term\n  definition` to `term\n  : definition` (adds colon separator required by pulldown-cmark)
- Handles multiple consecutive terms before definitions
- Handles multiple definitions per term (separated by blank lines)
- Skips frontmatter blocks (between `---` delimiters)
- Leaves 4+ space indented lines unchanged (code block detection)
- Detects ordered list items via digit-dot prefix to avoid false positives

### 8.2 Input Bounds

- No explicit length limits on terms or definitions
- No nesting depth limit (relies on pulldown-cmark's parser)
- Blank lines between term and definition are consumed
- Lines with 4+ spaces of indentation are excluded (treated as code)
- Block element markers (`#`, `- `, `* `, `>`, etc.) exclude a line from being a term

### 8.3 Resource Use

- `normalize_definition_lists()` allocates a new `String` with capacity equal to input length
- Lines are processed in a single pass with lookahead
- No unbounded recursion or stack growth
- Memory use is O(input size) for the normalized string

### 8.4 Control Character Handling

- Input is passed through `inert_source()` before normalization, which:
  - Strips `\r` before `\n`
  - Escapes control characters (except `\n`, `\t`) as `\u{xxxx}`
  - Escapes bidirectional override characters (U+202A–U+202E, U+2066–U+2069)
- Normalization operates on the inert source, so control characters are already escaped
- No additional control character handling in the normalization step

### 8.5 Source Range Handling

- Parser events include byte ranges for each element
- Definition list events (`DefinitionList`, `DefinitionListTitle`, `DefinitionListDefinition`) include ranges
- Ranges are used for rule style detection but not specifically for definition list layout
- No range validation specific to definition lists

### 8.6 Rendering Behavior

- `DefinitionList` start: blank line before (unless at document start), finish inline
- `DefinitionListTitle`: bold styling, finish inline, newline after
- `DefinitionListDefinition`: 2-space indent, finish inline, newline after
- `DefinitionList` end: blank line after
- Inline writer width tracking is reset at each definition list boundary

### 8.7 Fallible Output

- All write operations return `std::io::Result` and are propagated
- No silent truncation or error suppression
- Write failures abort rendering immediately

### 8.8 Evidence Limits

- No runtime testing, fuzzing, or network interaction performed
- Tests reviewed but not executed
- No inspection of pulldown-cmark's internal definition list parsing
- No verification of edge cases in normalization (e.g., very long terms, Unicode in terms)
- No inspection of downstream navigation code

### 8.9 Findings Summary

| Area | Status | Notes |
|------|--------|-------|
| Normalization bounds | Well-defined | Clear term/definition detection; frontmatter and code blocks excluded |
| Input bounds | Open | No length limits; relies on parser for nesting depth |
| Resource use | Linear | O(input size) memory; single-pass processing |
| Control handling | Present | `inert_source()` applied before normalization |
| Source ranges | Included | Parser provides ranges; not used for DL-specific layout |
| Fallible output | Yes | All writes return Result; failures propagate |

## 9. Mermaid Terminal Adapter

**Scope:** Independent observation of `src/main.rs`, `src/mermaid.rs` and
`tests/mermaid_backend.rs`, with a supplied targeted nextest result (78 passed).

`render_bounded` applies lexical preflight, strict semantic parsing, typed
admission and bounded, width-independent output under a 500 ms cooperative
deadline. The default uses Blad projections or strict spatial output and rejects
nonspatial summaries. `--mermaid-tree merman` skips production admission gates
but retains backend budgets; its format is not stable. `ascii_error` reports
routing and port-capacity misses as unsupported, separately from invalid input,
resource failures and cancellation.

The targeted tests cover labelled transitions in composite cycles, a valid graph
with insufficient ports, native GitGraph/Gantt/Packet output confined to debug
mode, cancellation, and the exact encoded-output byte boundary. The observation
does not establish upstream behavior beyond that evidence.