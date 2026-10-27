# tools

Repository helpers that are not part of the Rust workspace. Scripts are invoked
by the [justfile](../justfile) or CI, not by Cargo directly.

## `nextest.toml`

Shared repository scheduling policy. `just test` explicitly selects this file;
direct nextest runs should also pass `--config-file tools/nextest.toml`.
It reserves the complete test-thread pool for the native repository-wide
sandbox trial while ordinary tests stay parallel. It also reserves the pool for
Blad's dedicated `diagrams`/`mermaid_backend` binaries, Markdown Mermaid
regression, adapter budget tests and exact mixed-suite regressions that render
Mermaid: color-depth geometry, runtime-tool independence, session dump policy
and viewer diagram resizing. Source-only fixtures in mixed suites and unrelated
tests stay parallel. This replaces the previous three-name policy, which
omitted other engine callers, notably `color_depth_never_changes_visible_geometry`.
Native Mermaid retains a 500 ms cooperative wall-clock render deadline; cold
engine initialization and CPU scheduling count against it. These tests must
not compete with other suite work merely to test diagram semantics and
deterministic resource boundaries.

The CI profile inherits both default rules. No assertion, production limit,
retry or skip is changed. Pool reservation coordinates only this nextest
invocation, not unrelated host processes or another test runner. This tracked
policy does not depend on the ignored, sandbox-local `.config/nextest.toml`.

`test_nextest_policy.py` pins the scoped selector, whole-pool allocation and
CI-profile inheritance. Repository CI runs it with Python 3.11+ (standard
library only); it stays outside Blad's self-contained package tests:

```bash
python3 -B -m unittest discover -s tools -p 'test_*.py'
```

Changing the selector requires updating the expected fixture scope, then
checking effective nextest selection and start/finish ordering. The Python
regression checks configuration, not timing or filter interpretation.

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
