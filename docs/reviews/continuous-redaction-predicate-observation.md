# Redaction predicate observation

Observer: GitHub Copilot, independent source-only observation limited to
`maerg/src/config.rs:2020–2085` and `maerg/src/config.rs:2425–2460`.
No commands or tests were executed. This is not assurance, certification, or
a compliance judgment.

## Current source

`maerg/src/config.rs:2020–2085` rejects whitespace-only strings:
the predicate requires `!value.trim().is_empty()`. Nonblank values are
preserved, not trimmed: storage uses `value.to_owned()` on the original
string. The 4096-byte bound and duplicate comparison also use the original
string.

## Unexecuted test assertions

`shared_role_redaction_values_are_applied_and_validated`
(`maerg/src/config.rs:2425–2460`) asserts preservation of `["first", "second"]`
and errors for:

- An empty array.
- A boolean element.
- An empty string.
- A space-only string.
- A tab-and-newline-only string.
- A space-only string in `[redaction].values`.
- Duplicate `"same"` strings.
- Simultaneous `redaction_values` and `[redaction].values` configuration.

This test source does not assert preservation of padded nonblank strings
or rejection of overlength strings; those behaviors above are observations
of the predicate and storage code, not executed results.
