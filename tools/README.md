# tools

Small build helpers that are not part of the Rust workspace. They are invoked
by the [justfile](../justfile), not by Cargo directly.

## `nextest.toml`

Shared repository scheduling policy. `just test` explicitly selects this file;
direct nextest runs should also pass `--config-file tools/nextest.toml`.
It reserves the complete test-thread pool for the native repository-wide
sandbox trial while ordinary tests stay parallel. It also reserves the pool for
Blad's `markdown::tests::test_mermaid_block_is_processed` and the two
`mermaid::budget_tests` work/grid/output and exact-byte-boundary regressions,
including their copies in the `mermaid_backend` binary. Those five instances
retain a 500 ms cooperative wall-clock render deadline; cold engine initialization
and CPU scheduling count against it. They must not compete with other suite
work merely to test diagram semantics and deterministic resource boundaries.

The CI profile inherits both default rules. No assertion, production limit,
retry or skip is changed. Pool reservation coordinates only this nextest
invocation, not unrelated host processes or another test runner. This tracked
policy does not depend on the ignored, sandbox-local `.config/nextest.toml`.

## `cargo_install_binaries.py`

Extracts the built executables from a Cargo NDJSON build log and copies them to
an install directory. The justfile's `build` and `release` recipes run the
workspace build with `--message-format=json-diagnostic-rendered-ansi` into a
log file, then call this script to install the resulting binaries:

```bash
python3 tools/cargo_install_binaries.py <log-file> [install-dir]
```

- `<log-file>` — the Cargo NDJSON build log (e.g. `.kvist/cargo-build.log` or
  `.kvist/cargo-release.log`).
- `[install-dir]` — where to copy the binaries. Defaults to `~/.kvist/bin`.

The script parses only `compiler-artifact` messages that carry an `executable`
path (i.e. binary targets), copies each to the install directory, and reports a
warning if it saw compiler artifacts but none were binaries. It is a plain
Python 3 standard-library script with no third-party dependencies.
