# Kvist

[![Rust](https://github.com/Fairglow/kvist/actions/workflows/rust.yml/badge.svg)](https://github.com/Fairglow/kvist/actions/workflows/rust.yml)

Kvist is a filesystem-native, spec-driven architecture tool for human-directed
AI development. Its current interface is command-line based; graphical and
editor integrations are planned without changing the durable project model.
Its product architecture is defined in
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md).

## CLI contract

| Command                                            | Contract                                                                                         |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `kvist init [PROJECT_DIR]`                         | Initialize the Kvist root artifacts in `PROJECT_DIR`, defaulting to the current directory.       |
| `kvist doctor [PROJECT_DIR]`                       | Read-only inspection of the root artifact state and recovery guidance.                           |
| `kvist status [PROJECT_DIR] [--format text\|json]` | Read-only versioned inspection of project and component workflow state.                          |
| `kvist tree [PROJECT_DIR]`                         | Render the component hierarchy rooted at `PROJECT_DIR`, defaulting to the current directory.     |
| `kvist spec new <COMPONENT_DIR>`                   | Create a layered `SPEC.md` for a component directory.                                            |
| `kvist spec validate <SPEC_FILE>`                  | Validate a layered `SPEC.md` file.                                                               |
| `kvist spec accept <COMPONENT_DIR>`                | Record reviewed specification revisions and clear recorded stale evidence for one component.      |
| `kvist task next <COMPONENT_DIR>`                  | Select the first ready task without changing durable state.                                      |
| `kvist task transition <COMPONENT_DIR> ...`        | Persist one legal task-state transition with append-only attempt evidence.                       |
| `kvist task run <COMPONENT_DIR> [TASK_ID]`         | Run the configured external agent for one ready task; see the execution boundary below.          |
| `kvist task log <COMPONENT_DIR> <TASK_ID>`         | Print the most recent raw agent log for a task.                                                  |
| `kvist task approve-policy [PROJECT_DIR]`          | Record approval of the complete effective execution policy.                                       |

Delivery is organized into phases. The completed, current, and planned phase
scope is defined by the implementation roadmap in
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
and prioritized in [`TODO.md`](TODO.md). The Phase 1 foundation and Phase 2
queue, status, task-transition, agent-runner, and test-verification mechanics
are implemented. `task run` now requires an external sandbox runner; provider
approval and resource-boundary follow-up work remains.

## Configuration and platform policy

Core project configuration is read from `kvist.toml` in the selected project
root; Kvist does not search parent directories for a project. Agent
configuration is resolved in this order: `[agent]` in `kvist.toml`,
`.kvist/config.toml` in the project, the per-user configuration path, then the
system configuration path, then a built-in default. This behavior is limited
to agent settings; it does not select a project root. The resolver records the
selected source identity and SHA-256 digest. It never creates a user
configuration as a side effect.

### Sandboxed task execution

`task run` never executes an agent or verification command directly. Each
project must opt in with this versioned, project-local configuration:

```toml
[sandbox]
schema_version = 1
runner = "/absolute/path/to/separately-installed-sandbox-runner"
network = "deny"
environment_allowlist = ["PATH"]
mount = "component"
```

`runner` must be an absolute path to a regular, non-symlink executable outside
both the project root and the selected Git/jj worktree root. Repository-
controlled runners, including siblings of a nested Kvist project, are forbidden
even when they self-attest. Kvist refuses execution if it cannot resolve the
selected worktree root. The runner is spawned without a shell. It must acknowledge
`--kvist-sandbox-probe-v1` by writing exactly
`kvist-sandbox-probe-v1: network=deny; mount=component` and accept one JSON
request on stdin when passed `--kvist-sandbox-request-v1`. Request version 1
contains the target program/arguments, a `/workspace/component` working
directory, a single component mount, denied network, allowed environment, and
agent context paths. The runner must enforce those values and proxy its
sandboxed child result. Missing configuration, an unavailable runner, or a
failed probe refuses `task run` before any task transition; Kvist never falls
back to host execution. A version-1 sandbox cannot run a `test_policy` with
`working_directory = "project"`.

Before `task run` probes a runner or changes a task, run
`kvist task approve-policy`. Despite its retained compatibility name, it
atomically writes a versioned, deterministic, non-secret record in
user-owned state outside the repository. A persistent cryptographically random
user secret authenticates that record and binds it to canonical project and
worktree identities, so a repository cannot forge approval by replacing its
configuration and hashes. Repository-contained and legacy approval records are
rejected. The record covers both effective agent templates and token limits,
the selected agent-config source path and digest, parsed sandbox configuration,
canonical runner path and digest, test policy (including absence), and relevant
schema/protocol versions. Any missing, malformed, or changed input causes
`task run` to refuse without probing, host fallback, or task mutation. On
Linux, each probe and request launches a private descriptor-bound copy of
freshly verified runner bytes, so replacement after validation cannot alter
what executes. Platforms without a descriptor-bound launch mechanism fail
closed.

Kvist targets current stable Rust on Linux, macOS, and Windows for x86_64 and
ARM64 systems. Filesystem behavior must be covered on every supported platform;
platform-specific differences must be explicit in the relevant command
documentation and tests.

## Toolchain and quality gates

Kvist's MSRV is Rust **1.85**, the first stable release supporting edition 2024. CI tests the MSRV on Linux and current stable Rust on Linux, macOS, and
Windows. `Cargo.lock` is committed and every CI build/test command uses
`--locked`.

The portable default quality gate uses only Cargo:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked --release
```

`just` is an optional wrapper for these commands; `just all` runs the same
gate, and `just msrv` runs tests with Rust 1.85.0. Dependency updates must be
small, intentional changes with a stated purpose, lockfile update, and passing
MSRV and stable CI.

### Filesystem threat model

Read-only discovery supports ordinary local checkouts and malformed or
untrusted **static** workspaces: it bounds reads, reports malformed layouts,
and refuses link-like paths it directly inspects. It is not a sandbox, does not
establish canonical containment, and makes no guarantee if another process
changes the filesystem between metadata checks and use (TOCTOU). Do not treat
`init`, `doctor`, `status`, `tree`, or specification validation as
authorization to run repository code. The execution boundary still requires a
separately documented trusted-workspace policy and explicit execution
authorization.

On Unix, symbolic links are link-like; on Windows, all reparse points,
including junctions and symbolic links, are link-like. Link-like
project/configuration/component paths are refused, while a link-like required
artifact is invalid. Discovery refuses link-like non-artifact descendants
rather than following or silently traversing them. These direct checks reduce
accidental traversal only; they do not remove the TOCTOU limitation above.

## Root artifact templates

`kvist init` creates the following deterministic, UTF-8 templates.

| Path               | Version and required defaults                                                                    | Purpose                                                                     |
| ------------------ | ------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------- |
| `kvist.toml`       | configuration schema `1`; `component_root = "src"`; `vcs.kind = "auto"`; `llm.provider = "none"` | Project-local configuration with VCS and opt-in external LLM settings.      |
| `ROOT_CONTRACT.md` | `<!-- kvist-root-contract-version: 1 -->`                                                        | Global architectural and compliance constraints for every component.        |
| `src/SPEC.md`      | `<!-- kvist-specification-version: 1 -->`                                                        | Root component contract with the three progressive-disclosure layers.       |
| `src/TODOS.yaml`   | `schema_version: 1`                                                                              | Versioned, traceable component execution plan with ordered lifecycle tasks. |
| `src/IMPL.md`      | `<!-- kvist-implementation-record-version: 1 -->`                                                | Independently reverse-engineered implementation record.                     |

The configuration, root-contract, specification, TODO-queue, and
implementation-record versions are independent positive-integer domains. Backward-incompatible
changes must increment only the relevant domain and include an explicit
migration path; Kvist must never silently rewrite user-authored artifacts. The
initial templates contain no credentials, configured external provider,
copyright notices, or license terms.

`[discovery]` may configure bounded tree traversal. Omitted values use the
defaults below; values must be positive integers and may not exceed their hard
maximum, otherwise `doctor` classifies the project invalid and `init`/`tree`
refuse it.

| Key                         | Default | Hard maximum | Meaning                                                        |
| --------------------------- | ------: | -----------: | -------------------------------------------------------------- |
| `max_depth`                 |      64 |          256 | Levels below `component_root`.                                 |
| `max_directories`           |  10,000 |      100,000 | Directories whose entries are scanned, including the root.     |
| `max_components`            |  10,000 |      100,000 | Recognized components, including the root.                     |
| `max_entries_per_directory` |  10,000 |      100,000 | Entries read from one directory.                               |
| `max_relative_path_bytes`   |   4,096 |       32,768 | Platform-encoded bytes in a path relative to `component_root`. |

`kvist init` creates a missing target directory, rejects a link-like root or
artifact parent, and writes each artifact through a same-directory temporary
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
well-formed version this binary does not support. Kvist has no automatic repair or migration: preserve user content, use
`doctor` to inspect it, then repair or migrate explicitly. Any future repair
or migration command must
define every permitted rewrite and remain opt-in.

## Project status reports

`kvist status [PROJECT_DIR] [--format text|json]` inspects the current root
project and every discovered component without writing files. It reports
`unsupported-version`, `invalid`, `missing`, `stale`, `blocked`, and `current`
component states in precedence order. Valid queue records are compared against
the exact SHA-256 digest of the component `SPEC.md` and, for a child, its
immediate parent's `SPEC.md`; a mismatch appears as attributable stale
evidence but is never persisted by inspection.

Text output begins with `status-format-version: 1`; JSON output is a compact
object with `format_version`, `project_path`, `project_state`,
`component_root`, `components`, and `discovery_error`. Both are deterministic
and report the same configured component root, ordered components, and
adjacent `SPEC.md`, `TODOS.yaml`, and `IMPL.md` states. Dynamic text fields
escape ASCII control characters. JSON path strings are lossy display text and
are not persistent file identifiers. A completed inspection exits successfully
regardless of the reported project state; I/O failures exit nonzero.

## Version-control policy

Before Phase 2 task execution, durable artifacts (`kvist.toml`,
`ROOT_CONTRACT.md`, and each component's `SPEC.md`, `TODOS.yaml`, and
`IMPL.md`) must be tracked in a supported VCS. `kvist doctor` inspects every
root and discovered component artifact without staging or committing.

`[vcs].kind` defaults to `"auto"`, which selects the one detected VCS. Set it
to `"git"` or `"jj"` for a colocated checkout containing both. Git inspection
uses Git's index and native ignore rules, so an ignored required artifact is
reported as `ignored`. jj inspection uses `--ignore-working-copy` and the
saved working-copy snapshot, avoiding an automatic jj snapshot or other
mutation. A required file absent from that snapshot is reported as not tracked
and may be ignored, excluded by `snapshot.auto-track`, or newer than the saved
snapshot; Kvist does not run a mutating jj command merely to distinguish those
cases. Transient logs, locks, raw provider data, and credentials remain
untracked.

VCS tracking is advisory for read-only commands. Task selection, transitions,
specification acceptance, and task execution require a complete tracking
inspection.
Git and jj queries are batched below an 8 KiB argument budget; an individual
durable path that cannot fit in that budget is reported with unavailable
tracking status rather than causing the entire inspection to fail.
CI installs jj 0.44.0 in its dedicated VCS job; local environments without jj
continue to receive Git/no-repository diagnostics, while the jj fixture is
skipped.
Kvist intentionally does not apply VCS ignores to component discovery:
discovery remains deterministic and its own directory policy is independent of
tracked-state semantics.

## Component discovery policy

The discovery model is read-only and accepts an explicit component-root
directory (the initial configuration uses `src`). The root is always a
component; a descendant is a component only when at least one of `SPEC.md`,
`TODOS.yaml`, or `IMPL.md` exists beside it. This prevents ordinary source
directories from becoming components while retaining incomplete layouts for
diagnosis.

Every intermediate directory from the component root to a recognized
descendant must itself be a component. A recognized artifact-bearing directory
below an ordinary directory is an actionable discovery error, not a tree entry
with ambiguous indentation.

Each artifact must be a regular file. Missing artifacts produce an incomplete
status; directories, symbolic links, and other filesystem objects at required
artifact paths produce an invalid status. Content validation is intentionally
deferred to the specification and task-queue validators.

Traversal skips `.git`, `.hg`, `.jj`, `node_modules`, and `target` directories,
visits paths in lexical order, and reports the exact configured limit or
hierarchy violation rather than silently truncating.

Permission failures are reported as filesystem errors with the operation and
path. The automated suite does not alter permissions: root and privileged CI
accounts can bypass those checks, making such tests flaky. Permission behavior
is exercised in supported-platform release/manual testing with an unprivileged
account, a directory that denies enumeration, and a required artifact that
denies metadata/read access; each case must produce a nonzero command result
without writes.

`kvist tree` reads only the selected project's `kvist.toml`, renders plain
ASCII with no terminal capability detection, and never writes project files.
Its first line identifies the configured component root; every subsequent line
reports a component's relative path and complete, incomplete, or invalid
artifact layout. Invalid output lists both malformed and missing artifacts.

## Specification format

`SPEC.md` starts with `<!-- kvist-specification-version: 1 -->` on line 1, followed
by the three ordered collapsible sections below. The required summaries and
headings are exact so Kvist can validate them without rewriting user content.

| Layer                                 | `<details>` syntax                                                  | Required headings                  |
| ------------------------------------- | ------------------------------------------------------------------- | ---------------------------------- |
| Executive summary and public contract | `<details open>` / `Layer 1: Executive summary and public contract` | `## Purpose`, `## Public contract` |
| Architectural guarantees              | `<details>` / `Layer 2: Architectural guarantees`                   | `## Constraints and invariants`    |
| Detailed strategy and algorithms      | `<details>` / `Layer 3: Detailed strategy and algorithms`           | `## Design and failure paths`      |

Every required heading needs non-whitespace content before the next heading or
closing tag. The validator returns deterministic, one-based line and column
diagnostics for version, ordering, syntax, missing-heading, and empty-section
issues. It is read-only: all Markdown outside the required structure remains
user-authored and untouched.

`spec validate` accepts only regular UTF-8 files up to 1 MiB and rejects
symbolic links before parsing.

Root-state inspection applies the same 1 MiB bound to `ROOT_CONTRACT.md`,
`src/TODOS.yaml`, and `src/IMPL.md` before reading or parsing them.

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
The runtime uses [serde](https://crates.io/crates/serde) 1 and
[serde_yaml](https://crates.io/crates/serde_yaml) 0.9 for the typed,
versioned `TODOS.yaml` schema. They are used only for project-local durable
workflow data; they do not send queue contents over the network.

## TODO queue format

`TODOS.yaml` is the durable execution plan for exactly one component. It is
not a scratchpad or an agent transcript. It records the work that is authorized
to happen, why that work matters, which requirement demands it, what must
happen before it, and what outcome proves it is complete. Keeping that
information in a validated, version-controlled file lets humans, CI, the
future CLI executor, and future web/LSP views reach the same decision without
depending on chat history.

Schema version 1 is the current format. The whole document is a UTF-8 YAML
mapping with **only** these top-level fields:

```yaml
schema_version: 1
component:
  specification_revision: sha256:<64-lowercase-hex-digits>
  parent_specification: null
  revalidation:
    state: current
    checked_at: 2026-08-13T20:19:53Z
    stale_since: null
    causes: []
tasks:
  - id: write-tests
    title: Write queue tests
    description: Define executable coverage for the queue contract.
    context: The queue will control future task execution.
    purpose: Prevent execution from relying on unvalidated workflow data.
    expected_outcome: Valid and invalid queue behavior is covered by tests.
    kind: test
    status: pending
    depends_on: []
    requirements:
      - SPEC.md#TODO-queue-schema-and-validation
    timestamps:
      created_at: 2026-08-13T20:19:53Z
      updated_at: 2026-08-13T20:19:53Z
      completed_at: null
    blocked_reason: null
```

### Component revision and revalidation fields

| Field                              | Allowed values                                                                        | Purpose and tool use                                                                                                                                                                                                    |
| ---------------------------------- | ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `schema_version`                   | Integer `1`                                                                           | Selects the parser contract independently of Kvist, specification, configuration, and implementation-record versions. An unsupported version is refused rather than guessed or rewritten.                                 |
| `component.specification_revision` | `sha256:` plus 64 lowercase hexadecimal digits                                        | Fingerprints the exact component `SPEC.md` reviewed when the queue was planned. `kvist status` compares it with the current specification to discover that local work needs revalidation. |
| `component.parent_specification`   | `null` for the root, otherwise `{ path: "../SPEC.md", revision: "sha256:..." }`       | Records the only allowed upstream contract: the immediate parent. It lets tools detect an upstream change without loading peer implementations or violating the context boundary.                                       |
| `parent_specification.path`        | Exactly `../SPEC.md`                                                                  | Prevents a queue from disguising peer or arbitrary-project inputs as a parent dependency.                                                                                                                               |
| `parent_specification.revision`    | SHA-256 revision format above                                                         | Is the parent specification the component plan was reviewed against; a later mismatch produces explicit stale evidence.                                                                                                 |
| `revalidation.state`               | `current` or `stale`                                                                  | `current` permits later task selection; `stale` prevents it until human revalidation records a reviewed plan.                                                                                                           |
| `revalidation.checked_at`          | Whole-second UTC RFC 3339 (`YYYY-MM-DDTHH:MM:SSZ`)                                    | Records when revision comparison last ran, rather than relying on ambiguous filesystem modification time.                                                                                                               |
| `revalidation.stale_since`         | `null` when current; UTC timestamp when stale                                         | Preserves how long the current stale condition has existed for status views and review prioritization.                                                                                                                  |
| `revalidation.causes`              | Empty when current; nonempty cause list when stale                                    | Retains the evidence behind staleness. A tool never hides a changed contract behind an unexplained flag.                                                                                                                |
| `causes[].kind`                    | `component-specification-revision-changed` or `parent-specification-revision-changed` | Tells the revalidator whether the component's own contract or its immediate parent changed.                                                                                                                             |
| `causes[].path`                    | Nonblank component-relative specification path                                        | Identifies the exact artifact inspected.                                                                                                                                                                                |
| `causes[].expected_revision`       | SHA-256 revision                                                                      | Preserves the revision on which the old plan relied.                                                                                                                                                                    |
| `causes[].observed_revision`       | Different SHA-256 revision                                                            | Preserves the revision that invalidated the plan.                                                                                                                                                                       |

A current queue must have `stale_since: null` and `causes: []`. A stale queue
must have both timestamps, with `stale_since` no later than `checked_at`, and
at least one cause whose nonblank path has different valid expected and
observed revisions. This means staleness is inspectable evidence, not a
mutable boolean. `kvist status` derives the mismatch when a component or
immediate parent `SPEC.md` changes; a human then reviews
affected tasks, updates their requirement links and revisions as necessary,
records a fresh `checked_at`, and clears the causes. No later task-selection
tool may silently treat a stale plan as current.

### Task fields

Each task is a single bounded unit of work. Unknown task fields, duplicate IDs,
empty traceability fields, malformed timestamps, invalid state metadata,
unknown dependencies, dependency cycles, future-position dependencies, and
non-canonical dependency/reference lists are rejected.

| Field                     | Allowed values                                                                      | Purpose and tool use                                                                                                                                                                           |
| ------------------------- | ----------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                      | Unique 1-64 character lowercase kebab-case identifier                               | Stable local primary key for dependency edges, task selection, audit records, status output, and merge review. Never renumber or reuse it.                                                     |
| `title`                   | Trimmed, nonblank, one-line text up to 120 Unicode scalar values                    | Short human label for CLI, CI, and UI status lists.                                                                                                                                            |
| `description`             | Trimmed, nonblank work instruction up to 4,096 Unicode scalar values                | Bounded implementation scope supplied to the future execution context.                                                                                                                         |
| `context`                 | Trimmed, nonblank background up to 4,096 Unicode scalar values                      | Explains the triggering condition and lets an owner decide whether the task is still relevant after a change.                                                                                  |
| `purpose`                 | Trimmed, nonblank value/risk statement up to 4,096 Unicode scalar values            | Explains why the task is useful. It prevents work with no architectural or user value from being treated as required.                                                                          |
| `expected_outcome`        | Trimmed, nonblank observable completion condition up to 4,096 Unicode scalar values | Gives the executor and reviewers a concrete completion assertion and later compliance evidence.                                                                                                |
| `kind`                    | `test`, `implementation`, `security-audit`, or `compliance-review`                  | Declares the lifecycle trust boundary. Non-test tasks must transitively depend on the preceding lifecycle kind, so implementation cannot precede tests and an implementer cannot self-certify. |
| `status`                  | `pending`, `in-progress`, `blocked`, or `completed`                                 | Is the authoritative workflow state used by later ready-task selection and status views.                                                                                                       |
| `depends_on`              | Lexically sorted, duplicate-free list of earlier task IDs                           | Defines the component-local DAG. A task becomes ready only after every listed task is completed. Declared task order is the deterministic tie-breaker.                                         |
| `requirements`            | Lexically sorted, duplicate-free `SOURCE#LOCATOR` strings                           | Links the task to the exact specification, root-contract, roadmap, or runbook requirement that justifies it. Review and execution surfaces retain these references as evidence.                |
| `timestamps.created_at`   | UTC timestamp                                                                       | Records when this version-1 task record was created.                                                                                                                                           |
| `timestamps.updated_at`   | UTC timestamp not earlier than `created_at`                                         | Records the most recent durable task update.                                                                                                                                                   |
| `timestamps.completed_at` | `null`, or UTC timestamp for a completed task                                       | Proves when a terminal task completion was recorded. It is required only for `completed` and may not predate `updated_at`.                                                                     |
| `blocked_reason`          | `null`, or trimmed nonblank text for a blocked task                                 | Makes a blocked task actionable instead of allowing tools to silently skip it. It is required only for `blocked`.                                                                              |

The legal task transitions are `pending -> in-progress | blocked`,
`in-progress -> pending | blocked | completed`, and
`blocked -> pending | in-progress`. `completed` is terminal: new work uses a
new ID and a requirement link, preserving the completed task's audit trail.
`kvist task transition` sets `updated_at`, sets `completed_at` only for
completion, and retains prepared/committed attempt evidence rather than
overwriting history.

### Ordering, serialization, and migration

Dependencies are local to one queue, must refer to earlier declared tasks, and
must form a directed acyclic graph. In the first queue format, a deliverable is its explicit
transitive dependency chain; there is no implicit feature-grouping field. The
required lifecycle is expressed in that chain as test, implementation, security
audit, then compliance review. Requirement references and explicit dependency
edges make the chain's scope reviewable while still allowing several small
tasks within a component.

Kvist serializes a validated queue deterministically: fixed field order,
two-space indentation, LF line endings, preserved declared task order, and
sorted dependency and requirement lists. Strings are emitted in quoted YAML
form so punctuation, timestamps, and multiline text do not receive
parser-dependent meanings. Deterministic output gives VCS a meaningful diff
and lets automation compare semantically equal queues reproducibly.

This is the first supported queue format; there is no prior Kvist queue schema
to migrate. Any future incompatible queue format must have its own explicit,
opt-in migration and preserve user-authored provenance rather than silently
rewriting workflow state.

## Task execution boundary

The intended executor advances an accepted queue from task to task without
requiring the human to select each one; the final independent review determines
whether the resulting component can be declared compliant. Human
task-by-task supervision remains an option, not a lifecycle requirement.

Today, `task run` is the one-task execution primitive that an unattended loop
can compose. It requires a current project and component, complete VCS
tracking, and a ready queue task. The task is transitioned atomically, and
Kvist passes the component `SPEC.md`, `TODOS.yaml`, `ROOT_CONTRACT.md`, and,
when applicable, the immediate parent `SPEC.md` to the configured agent. It
does not add peer implementation files to that explicit context. See
[`GUIDE.md`](GUIDE.md) for a current command-line loop.

The current runner has two profiles: `developer` for test and implementation
tasks, and `architect` for security-audit and compliance-review tasks. Agent
templates support `{prompt}`, `{context_files}`, and `{target_directory}` and
are spawned without a shell. They are whitespace-delimited argument templates,
not shell scripts; pipelines, redirections, and shell quoting are unsupported.

An implementation task also runs the matching inherited test command after the
agent exits successfully. Test commands require a versioned `[test_policy]`
included in the full execution approval. The policy controls working directory,
inherited environment variables, timeout, output cap, and component-to-command
mapping. Agent logs are written under `.kvist/logs`; attempt and verification
records are durable JSONL files.

**Safety status:** agent and test programs require an external sandbox runner
that attests network denial and component-only mounting, and every
execution-sensitive input must match the explicit approval record. Agent
execution still has no timeout or output cap.

## Intended lifecycle and current scope

Kvist's direction remains structure before syntax. The human architect starts
with the project vision, iteratively decomposes it into hierarchical component
specifications (manually, with assistance, or through an architect agent), and
explicitly approves each result. A designer agent then iteratively drafts the
specialized, traceable queue from an approved component specification; the
architect may refine and must accept that queue. Implementation follows the
accepted plan, and a clean-slate documenter and source-blind reviewer
independently compare observed behavior with the specification. `IMPL.md` is
the component's implementation record, not user documentation; public
integration material belongs under `docs/`.

The CLI currently enforces queue ordering and durable transitions, but it does
not yet automate architect/designer agents, the interview, clean-slate
documentation, source-blind comparison, arbitration, editor integration,
daemon, LSP, or web UI flows. Those are planned capabilities, not current
commands. See
[`GUIDE.md`](GUIDE.md) for the accurate manual workflow and
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
for the enduring architecture and phased direction.
