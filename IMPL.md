# Kvist Phase 1 Implementation Record

This document was reverse-engineered from the implemented Rust code and tests
without using the architecture specification.

## CLI

`kvist 0.1.0` provides `init [PROJECT_DIR]`, `doctor [PROJECT_DIR]`,
`tree [PROJECT_DIR]` (default `.`), `spec new COMPONENT_DIR`, and `spec
validate SPEC_FILE`. Success writes
one line to stdout; failures write errors to stderr and exit nonzero. Parser
and help behavior use Clap's status handling.

## Persistent behavior

`init` creates five deterministic files: `kvist.toml`, `ROOT_CONTRACT.md`, and
`src/{SPEC.md,TODOS.yaml,IMPL.md}`. `init` writes only an uninitialized project
and leaves a fully valid current project unchanged; it refuses partial,
invalid, and unsupported-version projects. `doctor` is read-only and reports
the five-state classification plus per-artifact diagnostics. Writes use
same-directory temporary files, synchronize contents, and use no-clobber
persistence. A failed multi-file initialization can leave a partial project;
Phase 1 deliberately requires explicit user recovery rather than overwriting it.

`spec new` creates a deterministic `SPEC.md`, creating its directory if absent,
and never overwrites an existing path.

## Configuration

`kvist.toml` must be a regular UTF-8 TOML file of at most 65,536 bytes with
integer configuration `schema_version = 1` and a non-empty relative `component_root`
containing only normal path segments. `[discovery]` supports positive bounded
limits for depth, scanned directories, recognized components, entries per
directory, and encoded relative-path bytes; omitted values use deterministic
defaults. `[vcs].kind` accepts `auto`, `git`, or `jj` and selects the
read-only durable-artifact tracking diagnostics emitted by `doctor`; `auto`
requires exactly one detected supported repository. `[llm]` is not validated
or used.

## Validation and tree

Specifications are UTF-8 regular files of at most 1 MiB. Root contract, TODO,
and documentation inspection also bounds each file to 1 MiB. Specification
validation requires an exact first-line `kvist-specification-version` marker
(`1`), three exact ordered `<details>` layers, required ordered headings, and
nonblank section content. Diagnostics are deterministic, line-aware, and use
column 1.

`tree` recursively reports `SPEC.md`, `TODOS.yaml`, and `IMPL.md` as complete,
missing, or invalid. It ignores artifact contents, skips `.git`, `.hg`, `.jj`,
`node_modules`, and `target`, sorts paths lexically, and renders stable ASCII.

## Safety, limits, and limitations

Unsafe Rust is forbidden. Direct project, configuration, component, and
specification link-like paths are rejected; Windows reparse points receive the
same treatment. Discovery rejects link-like non-artifact descendants, reports
link-like required artifacts as invalid, applies configured resource bounds,
and requires every intermediate directory to be a component before recognizing
an artifact-bearing descendant. It does not provide canonical containment or
TOCTOU protection. No network or LLM operation is implemented. `doctor` uses
Git's native index and ignore semantics to report tracked, ignored, and
untracked durable artifacts without staging or committing. jj inspection uses
its saved working-copy snapshot with `--ignore-working-copy`; a non-listed
artifact may be ignored, excluded by snapshot rules, or newer than that
snapshot. VCS queries are batched below an 8 KiB argument budget; a durable
path too large for an individual query is reported as unavailable rather than
failing all tracking diagnostics. Root inspection validates the current TODO mapping and root
contract/documentation version markers, but Phase 2 still owns the complete
TODO schema and specification-to-project association.
