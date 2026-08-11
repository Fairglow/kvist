# Kvist

Kvist is a Rust CLI for a filesystem-native, spec-driven architecture workflow
for human-directed AI development. Its product architecture is defined in
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md).

## Initial CLI contract

| Command | Contract |
| --- | --- |
| `kvist init [PROJECT_DIR]` | Initialize the Kvist root artifacts in `PROJECT_DIR`, defaulting to the current directory. |
| `kvist doctor [PROJECT_DIR]` | Read-only inspection of the root artifact state and recovery guidance. |
| `kvist tree [PROJECT_DIR]` | Render the component hierarchy rooted at `PROJECT_DIR`, defaulting to the current directory. |
| `kvist spec new <COMPONENT_DIR>` | Create a layered `SPEC.md` for a component directory. |
| `kvist spec validate <SPEC_FILE>` | Validate a layered `SPEC.md` file. |

All Phase 1 commands are implemented: `kvist init`, `kvist doctor`, `kvist tree`, `kvist spec
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
| `kvist.toml` | configuration schema `1`; `component_root = "src"`; `llm.provider = "none"` | Project-local configuration with opt-in external LLM integration. |
| `ROOT_CONTRACT.md` | `<!-- kvist-root-contract-version: 1 -->` | Global architectural and compliance constraints for every component. |
| `src/SPEC.md` | `<!-- kvist-specification-version: 1 -->` | Root component contract with the three progressive-disclosure layers. |
| `src/TODOS.yaml` | `schema_version: 1` | Ordered lifecycle tasks: tests, implementation, security audit, compliance review. |
| `src/DOCS.md` | `<!-- kvist-documentation-version: 1 -->` | Independently reverse-engineered implementation documentation. |

The configuration, root-contract, specification, TODO-queue, and documentation
versions are independent positive-integer domains. Backward-incompatible
changes must increment only the relevant domain and include an explicit
migration path; Kvist must never silently rewrite user-authored artifacts. The
initial templates contain no credentials, configured external provider,
copyright notices, or license terms.

`kvist init` creates a missing target directory, rejects a symbolic-link root
or artifact parent, and writes each artifact through a same-directory temporary
file with no-clobber persistence. It writes only an **uninitialized** project
and reports **already initialized** only after every required artifact validates
as current. It refuses partial, invalid, and unsupported-version projects
without overwriting them.

`kvist doctor [PROJECT_DIR]` is the read-only recovery guidance surface. It
classifies a project as `uninitialized`, `current`, `partial`, `invalid`, or
`unsupported-version`, listing each required artifact and an actionable
diagnostic. `partial` means one or more, but not all, valid root artifacts are
present. `invalid` covers malformed content, incorrect filesystem types, and
symbolic links; `unsupported-version` has precedence when any artifact has a
well-formed version this binary does not support. Phase 1 has no automatic
repair or migration: preserve user content, use `doctor` to inspect it, then
repair or migrate explicitly. Any future repair or migration command must
define every permitted rewrite and remain opt-in.

## Version-control policy

Before Phase 2 task execution, durable artifacts (`kvist.toml`,
`ROOT_CONTRACT.md`, and each component's `SPEC.md`, `TODOS.yaml`, and
`DOCS.md`) are expected to be tracked in a supported VCS. Kvist is VCS-aware,
not Git-only: Git and jj are the initial supported systems. Kvist must never
auto-stage or commit. Required artifacts ignored by the selected VCS must be
reported rather than hidden. Transient logs, locks, raw provider data, and
credentials are untracked. Phase 1 does not yet implement VCS inspection; its
dedicated remediation task defines that work before Phase 2.

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

`SPEC.md` starts with `<!-- kvist-specification-version: 1 -->` on line 1, followed
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

`spec validate` accepts only regular UTF-8 files up to 1 MiB and rejects
symbolic links before parsing.

Root-state inspection applies the same 1 MiB bound to `ROOT_CONTRACT.md`,
`src/TODOS.yaml`, and `src/DOCS.md` before reading or parsing them.

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
The runtime uses [serde_yaml](https://crates.io/crates/serde_yaml) 0.9 only to
parse the current root `TODOS.yaml` mapping and distinguish its schema version
from malformed content; Phase 2 will define its full queue schema.
