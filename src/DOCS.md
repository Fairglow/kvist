<!-- kvist-documentation-version: 1 -->
# Root Component Compliance Documentation

## Observed public contract

Commands: `init [PROJECT_DIR]`, `tree [PROJECT_DIR]`, `doctor [PROJECT_DIR]`,
`spec new COMPONENT_DIR`, and `spec validate SPEC_FILE`. Project arguments
default to `.`.

`init` creates missing project directories and, only from an uninitialized
state, writes `kvist.toml`, `ROOT_CONTRACT.md`, `src/SPEC.md`,
`src/TODOS.yaml`, and `src/DOCS.md`. `doctor` reports root-artifact status
without writing. `tree` renders discovered components as deterministic ASCII.
Specification creation and validation are independent of project configuration.

Success is written to stdout; non-parser failures are written to stderr and
exit 1.

## Observed guarantees and constraints

Root states are `uninitialized`, `current`, `partial`, `invalid`, and
`unsupported-version`; unsupported takes precedence over invalid. `init` is
idempotent only for `current`; it refuses all other existing states except
`uninitialized`.

Configuration is limited to 64 KiB; specifications and inspected root text
artifacts to 1 MiB. Discovery defaults are depth 64, 10,000
directories/components/entries per directory, and 4,096 relative-path bytes;
configured limits must be positive and no greater than 256, 100,000, 100,000,
100,000, and 32,768 respectively.

Configured component roots must be non-empty relative paths containing only
normal segments. Filesystem targets, root artifacts, configuration,
specifications, component roots, and relevant parents must be regular
directories/files rather than links. On Windows, reparse points are treated as
links.

## Observed design and failure paths

Writes use same-directory temporary files with sync and no-clobber persistence;
each file write is atomic, but initialization is not a multi-file transaction.
Existing specifications are never overwritten.

Discovery always includes its root, recognizes descendants only when at least
one adjacent `SPEC.md`, `TODOS.yaml`, or `DOCS.md` exists, sorts paths
lexically, skips normal `.git`, `.hg`, `.jj`, `node_modules`, and `target`
directories, and rejects link-like descendants, limit excess, and components
below ordinary intermediate directories. It reports missing or non-regular
artifacts but does not validate descendant artifact contents.

Root inspection validates only the five root artifacts. The TODO validator
requires non-empty string `id`, `status`, and `description` fields but does not
constrain their values or uniqueness. Specification validation is structural
and exact-tag based, not a general Markdown parser.
