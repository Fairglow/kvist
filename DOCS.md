# Kvist Phase 1 Observed Behavior

This document was reverse-engineered from the implemented Rust code and tests
without using the architecture specification.

## CLI

`kvist 0.1.0` provides `init [PROJECT_DIR]`, `tree [PROJECT_DIR]` (default
`.`), `spec new COMPONENT_DIR`, and `spec validate SPEC_FILE`. Success writes
one line to stdout; failures write errors to stderr and exit nonzero. Parser
and help behavior use Clap's status handling.

## Persistent behavior

`init` creates five deterministic files: `kvist.toml`, `ROOT_CONTRACT.md`, and
`src/{SPEC.md,TODOS.yaml,DOCS.md}`. Complete existing sets are unchanged;
partial sets or non-files are rejected. Writes use same-directory temporary
files, synchronize contents, and use no-clobber persistence. Failed multi-file
initialization is not rolled back.

`spec new` creates a deterministic `SPEC.md`, creating its directory if absent,
and never overwrites an existing path.

## Configuration

`kvist.toml` must be a regular UTF-8 TOML file of at most 65,536 bytes with
integer `schema_version = 1` and a non-empty relative `component_root`
containing only normal path segments. Other keys, including `[llm]`, are not
validated or used.

## Validation and tree

Specifications are UTF-8 regular files of at most 1 MiB. Validation requires
an exact first-line version marker (`1`), three exact ordered `<details>`
layers, required ordered headings, and nonblank section content. Diagnostics
are deterministic, line-aware, and use column 1.

`tree` recursively reports `SPEC.md`, `TODOS.yaml`, and `DOCS.md` as complete,
missing, or invalid. It ignores artifact contents, skips `.git`, `.hg`, `.jj`,
`node_modules`, and `target`, sorts paths lexically, and renders stable ASCII.

## Safety, limits, and limitations

Unsafe Rust is forbidden. Direct project, configuration, component, and
specification symlinks are rejected; discovery does not follow symlink entries.
Discovery permits at most 64 levels below the component root. No network or LLM
operation is implemented. No `TODOS.yaml` or `DOCS.md` schema validation,
artifact-content validation, or specification-to-project association is
implemented.
