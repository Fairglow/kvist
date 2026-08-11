# Kvist Phase 1 Observed Behavior

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
`src/{SPEC.md,TODOS.yaml,DOCS.md}`. `init` writes only an uninitialized project
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
containing only normal path segments. Other keys, including `[llm]`, are not
validated or used.

## Validation and tree

Specifications are UTF-8 regular files of at most 1 MiB. Root contract, TODO,
and documentation inspection also bounds each file to 1 MiB. Specification
validation requires an exact first-line `kvist-specification-version` marker
(`1`), three exact ordered `<details>` layers, required ordered headings, and
nonblank section content. Diagnostics are deterministic, line-aware, and use
column 1.

`tree` recursively reports `SPEC.md`, `TODOS.yaml`, and `DOCS.md` as complete,
missing, or invalid. It ignores artifact contents, skips `.git`, `.hg`, `.jj`,
`node_modules`, and `target`, sorts paths lexically, and renders stable ASCII.

## Safety, limits, and limitations

Unsafe Rust is forbidden. Direct project, configuration, component, and
specification symlinks are rejected; discovery does not follow symlink entries.
Discovery permits at most 64 levels below the component root. No network or LLM
operation is implemented. Root inspection validates the current TODO mapping
and root contract/documentation version markers, but Phase 2 still owns the
complete TODO schema and specification-to-project association.
