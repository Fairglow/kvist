<!-- kvist-documentation-version: 1 -->
# Root Component Compliance Documentation

## Observed public contract

Kvist 0.1.0 provides:

- `kvist init [PROJECT_DIR]`
- `kvist tree [PROJECT_DIR]`
- `kvist doctor [PROJECT_DIR]`
- `kvist spec new COMPONENT_DIR`
- `kvist spec validate SPEC_FILE`

`PROJECT_DIR` defaults to `.`. Clap-provided `--help` and `--version` are
supported; help exits successfully. Unknown or malformed commands are rejected.

Successful commands write one line to stdout. Non-parser failures exit 1 and
are reported on stderr with `error:`; parser output follows Clap's output and
exit policy.

`init` creates a missing project directory and, only for an uninitialized
project, creates these deterministic root artifacts:

1. `kvist.toml`
2. `ROOT_CONTRACT.md`
3. `src/SPEC.md`
4. `src/TODOS.yaml`
5. `src/DOCS.md`

A current project reports that it is already initialized and remains unchanged.
Partial, invalid, and unsupported-version projects are refused and direct the
owner to `doctor`; Kvist does not repair or migrate them.

`doctor` is read-only. It prints project path, state, stable per-artifact
diagnostics, VCS status, optional VCS details, and guidance. States are:

- `uninitialized`: all root artifacts are absent, including a missing project
  path.
- `current`: all five artifacts are valid at supported version 1.
- `partial`: a mix of valid and missing artifacts.
- `invalid`: malformed content, invalid artifact/parent type, link-like path,
  oversized or non-UTF-8 root text, or an invalid project path.
- `unsupported-version`: at least one artifact has a well-formed but
  unsupported version; this classification takes precedence over invalid
  artifacts.

`tree` loads `kvist.toml`, discovers the configured component root, and emits
stable ASCII text beginning `component root: <path>`. Components are lexical by
relative path, rooted at `.`, indented by depth, and marked `complete`,
`incomplete`, or `invalid`.

`spec new` creates parent directories as necessary and writes a validated
`SPEC.md`; it never overwrites an existing destination. `spec validate` reports
`valid specification: <path>` only when validation has no diagnostics.

## Observed guarantees and constraints

All version domains currently support version 1 independently: configuration,
root contract, specification, TODO queue, and documentation.

`kvist.toml` is a regular, non-link-like UTF-8 file no larger than 64 KiB. It
requires a positive integer `schema_version` equal to 1 and a non-empty relative
`component_root` containing only normal path segments. Optional `[vcs].kind` is
`auto`, `git`, or `jj`, defaulting to `auto`. Optional `[discovery]` values are
positive integers:

| Limit | Default | Hard maximum |
| --- | ---: | ---: |
| `max_depth` | 64 | 256 |
| `max_directories` | 10,000 | 100,000 |
| `max_components` | 10,000 | 100,000 |
| `max_entries_per_directory` | 10,000 | 100,000 |
| `max_relative_path_bytes` | 4,096 | 32,768 |

Root contract and documentation files require their exact first-line version
markers and their respective required titles. `TODOS.yaml` must be a YAML
mapping with version 1 and a non-empty `tasks` sequence; each task needs
nonblank string `id`, `status`, and `description` fields. Root contract, root
specification, TODO queue, and documentation are each bounded to 1 MiB and must
be UTF-8.

A specification is at most 1 MiB and must be a regular non-link-like file. Its
first line must be exactly a positive supported marker:

`<!-- kvist-specification-version: 1 -->`

It requires, in order, one each of:

1. `<details open>` with Layer 1 summary, `## Purpose`, and `## Public contract`;
2. `<details>` with Layer 2 summary and `## Constraints and invariants`;
3. `<details>` with Layer 3 summary and `## Design and failure paths`.

Required layers, opening syntax, closures, headings, ordering, uniqueness, and
non-whitespace section content are validated. Diagnostics are deterministic,
one-based `line:column` entries. Other Markdown is preserved and not validated.

Every discovered component has adjacent `SPEC.md`, `TODOS.yaml`, and `DOCS.md`.
A regular file is present; absence is incomplete; directories, symbolic links,
and other objects are invalid. The root is always represented. Descendants are
represented only when at least one required artifact exists. A recognized
component may not appear beneath an ordinary intermediate directory.

Discovery reads ordinary directories deterministically, skips `.git`, `.hg`,
`.jj`, `node_modules`, and `target`, and rejects depth, directory-count,
component-count, entry-count, and encoded-relative-path limits rather than
truncating. It rejects missing/non-directory/link-like roots and link-like
non-artifact descendants.

Symbolic links are rejected rather than followed throughout project
initialization, configuration loading, specification operations, root
inspection, and discovery. On Windows, reparse points are also link-like.
Generated files use same-directory temporary files, sync their contents, and
persist with no-clobber semantics.

## Observed design and failure paths

Initialization inspects before generating artifacts. It creates artifact parents
only after finding an uninitialized state, then writes each artifact atomically
without replacement. A filesystem failure can still leave prior successful
writes, which subsequent inspection classifies rather than repairs. `spec new`
similarly may create its missing component directory before a later failure.
`tree`, `doctor`, and specification validation do not write project content.

Read-only VCS inspection runs only when root state is `current`. Required
durable paths include all five root artifacts plus all three artifacts for every
discovered component. Invalid root state, invalid configuration, or failed
discovery yields VCS `not checked`.

Git detection uses `git rev-parse --show-toplevel`. Tracking is determined from
`git ls-files -z -- <paths>`; untracked paths are checked with
`git check-ignore --quiet --no-index -- <path>`. States are tracked, ignored,
untracked, missing, or unavailable. Inspection does not stage, commit, or alter
Git state. Git and jj artifact queries are batched below an 8 KiB argument
budget. A durable path that cannot fit in an individual query is unavailable,
without preventing diagnostics for other paths.

jj detection uses `jj --ignore-working-copy root`. jj inspection queries
`jj file list` at saved revision `@` without snapshotting the working copy. A
non-listed existing file is reported as not tracked by the jj snapshot: it may
be ignored, excluded by `snapshot.auto-track`, or newer than the saved snapshot.
`auto` rejects colocated Git and jj repositories; explicit configuration selects
one. Missing tools, repositories, malformed repositories, failed commands, or
a project outside the selected repository yield diagnostic-only VCS results.

On Linux, Git path handling preserves arbitrary native path bytes through
NUL-delimited output and can track non-UTF-8 component paths. jj fileset
construction requires UTF-8 normal path segments; unrepresentable paths are
`unknown`. On non-Unix platforms, VCS command paths returned by Git or jj must
be UTF-8.

CI runs on pushes and pull requests to `main`. Stable Rust is checked on Ubuntu,
macOS, and Windows with formatting, Clippy (`--all-targets -D warnings`), tests,
and release build. MSRV is Rust 1.85.0 with `cargo check --locked` and
`cargo test --locked`. A separate Ubuntu VCS job installs jj 0.44.0 and runs the
VCS integration test.
