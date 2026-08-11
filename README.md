# Kvist

Kvist is a Rust CLI for a filesystem-native, spec-driven architecture workflow
for human-directed AI development. Its product architecture is defined in
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md).

## Initial CLI contract

| Command | Contract |
| --- | --- |
| `kvist init [PROJECT_DIR]` | Initialize the Kvist root artifacts in `PROJECT_DIR`, defaulting to the current directory. |
| `kvist tree [PROJECT_DIR]` | Render the component hierarchy rooted at `PROJECT_DIR`, defaulting to the current directory. |
| `kvist spec new <COMPONENT_DIR>` | Create a layered `SPEC.md` for a component directory. |
| `kvist spec validate <SPEC_FILE>` | Validate a layered `SPEC.md` file. |

All Phase 1 commands are implemented: `kvist init`, `kvist tree`, `kvist spec
new`, and `kvist spec validate`.

## Configuration and platform policy

The initial configuration is project-local only: `kvist.toml` lives directly in
the selected project root. Kvist does not read global configuration and does
not search parent directories for a project. Relative command paths are
resolved by the operating system from the current working directory.

Kvist targets current stable Rust on Linux, macOS, and Windows for x86_64 and
ARM64 systems. Filesystem behavior must be covered on every supported platform;
platform-specific differences must be explicit in the relevant command
documentation and tests.

## Root artifact templates

`kvist init` creates the following deterministic, UTF-8 templates.

| Path | Version and required defaults | Purpose |
| --- | --- | --- |
| `kvist.toml` | `schema_version = 1`; `component_root = "src"`; `llm.provider = "none"` | Project-local configuration with opt-in external LLM integration. |
| `ROOT_CONTRACT.md` | `kvist-template-version: 1` | Global architectural and compliance constraints for every component. |
| `src/SPEC.md` | `kvist-template-version: 1` | Root component contract with the three progressive-disclosure layers. |
| `src/TODOS.yaml` | `schema_version: 1` | Ordered lifecycle tasks: tests, implementation, security audit, compliance review. |
| `src/DOCS.md` | `kvist-template-version: 1` | Independently reverse-engineered implementation documentation. |

Template and schema versions are positive integers. Backward-incompatible
changes must increment the relevant version and include an explicit migration
path; Kvist must never silently rewrite user-authored artifacts. The initial
templates contain no credentials, configured external provider, copyright
notices, or license terms.

`kvist init` creates a missing target directory, rejects a symbolic-link root
or artifact parent, and writes each artifact through a same-directory temporary
file with no-clobber persistence. It never merges a partial Kvist artifact set
or overwrites existing artifacts. A complete existing set is reported as
already initialized without modification.

## Component discovery policy

The discovery model is read-only and accepts an explicit component-root
directory (the initial configuration uses `src`). The root is always a
component; a descendant is a component only when at least one of `SPEC.md`,
`TODOS.yaml`, or `DOCS.md` exists beside it. This prevents ordinary source
directories from becoming components while retaining incomplete layouts for
diagnosis.

Each artifact must be a regular file. Missing artifacts produce an incomplete
status; directories, symbolic links, and other filesystem objects at required
artifact paths produce an invalid status. Content validation is intentionally
deferred to the specification and task-queue validators.

Traversal never follows symbolic links, skips `.git`, `.hg`, `.jj`,
`node_modules`, and `target` directories, visits paths in lexical order, and
reports an error rather than silently truncating beyond 64 directory levels.

`kvist tree` reads only the selected project's `kvist.toml`, renders plain
ASCII with no terminal capability detection, and never writes project files.
Its first line identifies the configured component root; every subsequent line
reports a component's relative path and complete, incomplete, or invalid
artifact layout. Invalid output lists both malformed and missing artifacts.

## Specification format

`SPEC.md` starts with `<!-- kvist-template-version: 1 -->` on line 1, followed
by the three ordered collapsible sections below. The required summaries and
headings are exact so Kvist can validate them without rewriting user content.

| Layer | `<details>` syntax | Required headings |
| --- | --- | --- |
| Executive summary and public contract | `<details open>` / `Layer 1: Executive summary and public contract` | `## Purpose`, `## Public contract` |
| Architectural guarantees | `<details>` / `Layer 2: Architectural guarantees` | `## Constraints and invariants` |
| Detailed strategy and algorithms | `<details>` / `Layer 3: Detailed strategy and algorithms` | `## Design and failure paths` |

Every required heading needs non-whitespace content before the next heading or
closing tag. The validator returns deterministic, one-based line and column
diagnostics for version, ordering, syntax, missing-heading, and empty-section
issues. It is read-only: all Markdown outside the required structure remains
user-authored and untouched.

`kvist spec new <COMPONENT_DIR>` creates the missing directory when necessary,
validates the deterministic template before writing, and persists `SPEC.md`
through a same-directory no-clobber atomic write. It never overwrites an
existing specification. `kvist spec validate <SPEC_FILE>` reports either a
success line or line-aware validation errors without modifying the file.

## Dependencies

The CLI uses [clap](https://crates.io/crates/clap) 4 for typed, accessible
argument parsing and help generation, and
[thiserror](https://crates.io/crates/thiserror) 2 for concise, typed domain
errors. Both are mature, widely maintained Rust ecosystem dependencies. The
project keeps its dependency graph small and adds dependencies only when their
security, licensing, maintenance, and operational benefits are justified.

The runtime uses [toml](https://crates.io/crates/toml) 1 to validate the
project-local configuration before reading its component tree.
The runtime uses [tempfile](https://crates.io/crates/tempfile) 3 for
same-directory, no-clobber atomic artifact writes.
