# tools

Small build helpers that are not part of the Rust workspace. They are invoked
by the [justfile](../justfile), not by Cargo directly.

## `nextest.toml`

Shared repository scheduling policy. `just test` explicitly selects this file;
direct nextest runs should also pass `--config-file tools/nextest.toml`.
It reserves the complete test-thread pool for the native repository-wide
sandbox trial while ordinary tests stay parallel. The CI profile inherits
the default rule. This tracked policy does not depend on the ignored,
sandbox-local `.config/nextest.toml`.

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
