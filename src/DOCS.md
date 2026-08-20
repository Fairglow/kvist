<!-- kvist-documentation-version: 1 -->
# Root Component Compliance Documentation

This document records behavior observed from the Rust implementation and its
tests. It describes the current executable surface; it does not establish
requirements for behavior that is absent.

## Public command-line interface

The `kvist` executable uses a `clap` subcommand interface. It prints a
successful command result followed by a newline to standard output. Domain
failures print `error: <message>` to standard error and exit with status 1.
Parser help and parser-generated argument errors use clap's own output and
exit status (help exits successfully).

The available commands are:

* `kvist init [PROJECT_DIR]` initializes the explicit directory, or `.` when
  omitted. Success reports either `initialized Kvist project at <path>` or
  `Kvist project already initialized at <path>`.
* `kvist doctor [PROJECT_DIR]` performs a read-only root-artifact inspection
  and writes its multi-line report to standard output.
* `kvist status [PROJECT_DIR] [--format text|json]` performs a read-only
  project and component inspection. It defaults to `.` and text output.
* `kvist task next COMPONENT_DIR` selects one ready task without writing.
* `kvist task transition COMPONENT_DIR TASK_ID STATUS [--reason REASON]`
  persists one audited task-state transition.
* `kvist task run COMPONENT_DIR [TASK_ID] [--stream]` runs the configured
  external agent for an explicit or first ready task.
* `kvist task log COMPONENT_DIR TASK_ID` prints the most recent task log.
* `kvist task approve-policy [PROJECT_DIR]` stores the current test-policy
  hash in the project's `.kvist` directory.
* `kvist tree [PROJECT_DIR]` loads configuration, discovers components, and
  renders a deterministic ASCII tree.
* `kvist spec new COMPONENT_DIR` creates a new `SPEC.md`.
* `kvist spec validate SPEC_FILE` validates an existing specification. A valid
  file reports `valid specification: <path>`; an invalid file is a command
  failure with line-aware diagnostics.
* `kvist spec accept COMPONENT_DIR` updates the selected component queue's
  recorded specification revisions and clears its stale evidence.

Unknown commands are rejected by the argument parser. `main` owns output and
exit handling; `kvist::run()` parses process arguments and `cli::execute`
returns a displayable `CommandOutput` or `KvistError`.

Task commands do not create queues, revise task definitions, or migrate queues.
`status` loads queues as read-only component-inspection data. Humans otherwise
inspect queue content directly in the durable YAML file; `doctor` reports only
the root queue artifact's validity.

## Root artifacts and project state

Initialization's fixed root artifact set, in inspection order, is:

1. `kvist.toml`
2. `ROOT_CONTRACT.md`
3. `src/SPEC.md`
4. `src/TODOS.yaml`
5. `src/DOCS.md`

Each generated artifact has independently versioned content: configuration
version 1, root-contract version 1, specification version 1, TODO-queue
version 2, and documentation version 1. The generated configuration selects
`src` as component root, default discovery limits, automatic VCS selection,
and an LLM provider value of `none`.

`init` creates a missing target directory, including missing parents. It
inspects the complete root set before writing: an uninitialized directory is
written; a current project is returned unchanged; partial, invalid, and
unsupported-version projects are refused. Files are written through
same-directory temporary files, synced, and persisted without replacing an
existing destination. Artifact parent directories are created before writes.
This means a filesystem failure after a prior artifact write can leave a
partial project, which later initialization refuses rather than repairs.

Project and artifact parents must be real directories. Initialization and
inspection reject symbolic links (and Windows reparse points), regular files
where directories are required, and non-regular artifacts. A current project
is determined by validating all five artifact paths; it is not determined by
mere file presence.

`project_state::inspect` classifies a root as follows:

* `uninitialized`: the root is absent, or all required artifacts are missing.
* `current`: every required artifact is valid at the supported version.
* `partial`: at least one artifact is valid and at least one is missing, with
  neither invalid nor unsupported artifacts.
* `invalid`: the root is not a real directory, an artifact or its parent has
  an invalid filesystem type, or content validation fails.
* `unsupported-version`: at least one artifact has a syntactically valid,
  positive but unsupported version. This classification takes precedence over
  `invalid`.

The `doctor` report starts with project path and state, then reports artifacts
in the fixed order above, VCS information, and guidance. It does not repair or
migrate files. For non-current roots VCS inspection is explicitly not checked.
Text root artifacts other than configuration have a 1 MiB byte limit and must
be UTF-8. Documentation is valid only when its first line is exactly a
positive supported `kvist-documentation-version` marker and it contains this
document's title as a complete line.

## Configuration, discovery, tree, and VCS inspection

`kvist.toml` must be a regular, non-link UTF-8 file no larger than 64 KiB.
Its root value is a TOML table with positive `schema_version = 1` and a
non-empty relative `component_root` containing only normal path segments.
Optional `[discovery]` values are positive integers. Their defaults are
`max_depth = 64`, `max_directories = 10000`, `max_components = 10000`,
`max_entries_per_directory = 10000`, and
`max_relative_path_bytes = 4096`; their hard maxima are respectively 256,
100000, 100000, 100000, and 32768. Optional `vcs.kind` is `auto` (including
an omitted table or value), `git`, or `jj`.

Discovery always represents the component root and represents a descendant
only when at least one of `SPEC.md`, `TODOS.yaml`, or `DOCS.md` exists there.
A component is complete only when all three are regular files. Missing,
directory, symbolic-link, and other invalid artifact entries are distinguished.
Traversal is lexical and deterministic, ignores directories named `.git`,
`.hg`, `.jj`, `node_modules`, and `target`, and skips the three artifact
filenames as traversal entries. It refuses link-like roots and descendants,
excess depth, scanned-directory count, component count, entry count, and
encoded relative-path-byte limits. It also rejects a candidate component below
an ordinary intermediate directory; intervening directories must themselves
be component candidates.

`tree` uses configured limits and prints `component root: <configured path>`,
then lexical components indented two spaces per normal path segment. Each line
ends in `[complete]`, `[incomplete: missing ...]`, or `[invalid: ...]`.

## Project and component status inspection

`project_state::inspect` produces a root `ProjectInspection` with optional
component root, lexical `ComponentInspection` records, and an optional
discovery error. `init` uses its root state to gate writes, `doctor` displays
its root diagnostics, and `status` renders its component records. `tree` uses
the same configuration limits and `discovery::Component` layout model, while
queue parsing is shared by root and component inspection.

`status` renders `status-format-version: 1` text in this order: project path,
project state, component root or `unavailable`, optional discovery error, then
one lexical component record with adjacent artifacts and stale causes. Compact
JSON has these top-level keys in order: `format_version`, `project_path`,
`project_state`, `component_root`, `components`, and `discovery_error`. A
component object has `path`, `state`, `artifacts`, and
`revalidation_causes` keys in order; its artifact array is ordered `SPEC.md`,
`TODOS.yaml`, `DOCS.md`, and each item has `path` then `state`. A current root
uses the configured component root and lexical, bounded discovery. If
discovery fails, the report has no component records and carries the error;
the already determined root state is retained. A root that is not current has
no component root or component records. Status omits root-artifact, VCS, and
guidance details from its own output, although its root inspection invokes the
existing read-only VCS inspection when the root is current.

Every discovered component has `SPEC.md`, `TODOS.yaml`, and `DOCS.md` entries
in that order. Each must be a regular non-link UTF-8 file of at most 1 MiB.
Specification, queue, and documentation content use the corresponding
existing validators. Component state precedence is
`unsupported-version`, `invalid`, `missing`, `stale`, `blocked`, then
`current`. A topology-inconsistent queue parent, or an unreadable immediate
parent specification, makes an otherwise present child invalid.

For valid component queues, inspection retains recorded staleness causes and
computes a SHA-256 fingerprint over the exact valid component specification
bytes. It compares that fingerprint with the queue's component revision and,
for children, compares the immediate parent's specification with its recorded
parent revision. A differing digest adds a local or parent cause and makes the
component stale. It retains recorded stale causes without reconciling them to
newly derived causes. A blocked task produces `blocked` only when no higher
precedence condition applies. Status does not write its derived evidence,
timestamps, revisions, task state, or any other file.

Text dynamically escapes backslashes and ASCII control characters. JSON
escapes JSON control characters, quotes, and backslashes; all rendered paths
are lossy display text. Inspection is path-based and point-in-time: it checks
metadata and links before separate file reads and traversal, so it does not
provide a descriptor-based TOCTOU guarantee. Inspection I/O errors fail the
command; invalid artifacts and discovery errors are generally represented in
the successful report.

When root artifacts are current, `doctor` also discovers components and asks
the configured VCS about every root artifact and every discovered component's
three adjacent artifact paths. This inspection never stages, commits, or
writes. Automatic selection rejects colocated Git and jj repositories as
ambiguous. Git distinguishes tracked, ignored, untracked, missing, and
unavailable classification. jj checks its saved `@` snapshot without
snapshotting; an absent listing is reported as not tracked by the jj snapshot
and may reflect ignore rules, `snapshot.auto-track`, or a newer file. VCS
arguments are limited to 8 KiB batches; paths that cannot be safely queried
are `unknown`.

## Specification behavior

`spec new` creates missing component directories, rejects link-like or
non-directory targets, refuses an existing `SPEC.md`, validates the built-in
template first, then atomically writes that template without overwriting.
`spec validate` and the public `validate_file` reject links, non-files, and
files larger than 1 MiB; otherwise they read UTF-8 and do not modify it.

The public `validate(&str)` returns a `SpecificationValidation` with an
optional parsed template version and deterministic, source-ordered
line/column-one diagnostics. It checks the first-line version marker,
three exact disclosure-layer summaries and opening tags, their order and
closures, and required headings, order, uniqueness, and nonblank content.
`format_diagnostics` renders one diagnostic per line as
`line:column: message`. Other Markdown is not rewritten or otherwise
interpreted by this validator.

## TODO queue library API and data model

`task_queue::parse(contents)` parses YAML into `TaskQueue` and then performs
semantic validation. It returns `TaskQueueError::Yaml` for typed-YAML shape
failures, `UnsupportedVersion` unless `schema_version` is 2, or `Invalid` for
semantic failures. Unknown fields are rejected for every typed mapping.
`validate(&TaskQueue)` applies the same semantic checks to an in-memory queue.
Neither function reads or writes the filesystem.

`TaskQueue` has `schema_version`, `component`, and authoring-order `tasks`.
`ComponentState` records a local `sha256:` revision, optional immediate parent
specification, and `Revalidation`. A parent, when present, must use exactly
`../SPEC.md` and a valid revision. Revisions have the prefix `sha256:` plus
exactly 64 lowercase hexadecimal digits. Revalidation is `current` or
`stale`, has a whole-second UTC timestamp, optional stale-since timestamp,
and zero or more causes. Current requires null stale-since and no causes;
stale requires a stale-since timestamp no later than checked-at and at least
one cause. Causes require a nonempty path, distinct valid expected and
observed revisions, and one of the two serialized kind names
`component-specification-revision-changed` or
`parent-specification-revision-changed`.

The transparent `Timestamp` type accepts only real UTC calendar instants in
the exact `YYYY-MM-DDTHH:MM:SSZ` form. It checks leap years, month lengths,
and ranges, but exposes no public constructor or clock operation.

Each `Task` has ID, title, four detail fields (`description`, `context`,
`purpose`, and `expected_outcome`), lifecycle `kind`, `status`,
`depends_on`, `requirements`, timestamps, and `blocked_reason`.

* IDs are 1--64 lowercase ASCII letters, digits, and single hyphens; they
  cannot start or end with a hyphen.
* Titles are nonblank, trimmed, one-line, and at most 120 Unicode scalar
  values. Each detail field is nonblank, trimmed, and at most 4096 scalar
  values.
* Dependency and requirement lists are lexically sorted, duplicate-free, and
  contain no blank entries. Every requirement contains a nonblank source and
  nonblank locator separated by `#`.
* Task IDs are unique. Dependencies cannot be self-references, must name
  existing earlier tasks, and must be acyclic.
* `test` tasks need no lifecycle predecessor. `implementation` needs a
  transitive preceding `test`; `security-audit` needs a transitive preceding
  `implementation`; and `compliance-review` needs a transitive preceding
  `security-audit`.
* `updated_at` cannot precede `created_at`. A completed task has a valid
  `completed_at` no earlier than `updated_at`; every other status requires it
  to be null. A blocked task requires a nonblank reason; every other status
  requires a null reason.

`TaskStatus::can_transition_to` is the state-transition query API. It permits
`pending -> in-progress|blocked`, `in-progress -> pending|blocked|completed`,
and `blocked -> pending|in-progress`. Completed has no permitted outbound
transition. The API reports permission only: no public operation changes a
task, sets timestamps, checks dependencies' completion, changes revalidation,
or persists a transition.

## Queue serialization

`serialize(&TaskQueue)` validates first, then returns canonical deterministic
YAML or the same validation error classes. It preserves task and list order;
it does not sort tasks or list values. It emits mappings and fields in a fixed
order, two-space indentation, block lists, `null` for absent optional values,
and `[]` for empty lists (including `tasks: []`). All string-valued scalar
fields are double quoted. Within quoted strings it escapes double quotes,
backslashes, newline, carriage return, tab, and control characters through
U+001F (the latter as four-digit lowercase `\u` escapes). Enum values are
unquoted kebab-case names. Serialization ends with a newline and a
parse/serialize cycle is stable for valid data.

## Task selection and state updates

`kvist task next COMPONENT_DIR` validates a component-root-relative path, the
current project/component state, and complete VCS tracking, then prints the
first declared pending task whose dependency chain is completed or `no ready
task`. It does not write files. `kvist task transition COMPONENT_DIR TASK_ID
STATUS [--reason REASON]` applies only legal queue state transitions, requires
readiness for `in-progress`, requires a nonblank reason only for `blocked`,
and updates UTC task timestamps.

Transitions create a no-clobber component lock, append a `prepared` JSONL
attempt, atomically replace `TODOS.yaml`, then append `committed`. A trailing
prepared record fences another transition for that task until explicit future
recovery. The writer rejects invalid queues, stale or non-current components,
incomplete VCS tracking, unknown task IDs, illegal transitions, locks, clock,
and filesystem failures. Attempt directories and new attempt files are
directory-synced on Unix; other platforms retain the records without a
directory-entry durability guarantee. The commands do not invoke providers,
tests, shells, networks, or task execution.

## Agent execution and test verification

`task run` validates the same current project, component, and complete-VCS
tracking gates as task transitions. It runs `test` and `implementation` tasks
through the `developer` profile, and `security-audit` and `compliance-review`
tasks through the `architect` profile. The selected agent receives the local
`SPEC.md`, `TODOS.yaml`, `ROOT_CONTRACT.md`, and, for a child component, its
immediate parent's `SPEC.md`; peer implementation files are not passed as
explicit context.

Agent templates are split into a program and arguments without a shell. The
supported substitutions are `{prompt}`, `{context_files}`, and
`{target_directory}`. Standard output and error are copied to
`.kvist/logs/TASK_ID_TIMESTAMP.log`; `--stream` also writes them to the
console. A zero exit status completes the task except that an implementation
also requires its configured test command to succeed. Agent failures,
verification failures, and verification-policy failures block the task.

The test policy selects inherited commands by component path, clears the
environment except for the configured allowlist, applies the configured
timeout, and captures each output stream up to the configured byte limit.
`approve-policy` writes the SHA-256 hash of that policy to
`.kvist/approved_policy.sha256`; an implementation task blocks if the current
policy is absent or differs from that approval. Verification result records
are appended to the task's attempt JSONL file.

Agent execution is not sandboxed, has no agent timeout or agent-output cap,
and does not require approval of the resolved agent template. The implementation
therefore executes configured programs on the host and is not a safe boundary
for untrusted repositories.

## Observable omissions and failure behavior

The implementation contains no general-purpose queue-file loader or reusable
queue-loader size-bound API; root inspection is the only queue-file reader and
applies its 1 MiB root-artifact limit before parsing. Status separately reads
component queues only for inspection. It contains no queue creation, task-queue migration, or general-purpose queue
persistence API. Queue validation verifies the shape and
relationships described above;
it does not compare recorded revisions with files, resolve requirement
locators, verify that status history actually followed
`can_transition_to`, require unique or canonical revalidation causes, or
require a cause kind and path to correspond.

Failures are reported as typed `KvistError` or `TaskQueueError` values with
path and operation context where applicable. Filesystem, decoding, parser, configuration, traversal, validation, VCS-tool,
and safe-path failures do not
fall back to destructive recovery. Link-like filesystem objects are rejected
rather than followed. The current executable has no network behavior or
daemon, but it does invoke configured local agent and approved test programs.
