<!-- kvist-specification-version: 1 -->
# Kvist Root Component Specification

<details open>
<summary>Layer 1: Executive summary and public contract</summary>

## Purpose

Provide a local, headless Rust CLI that creates and inspects filesystem-native
Kvist projects. The root component makes durable project state, component
contracts, task queues, and compliance documentation visible to both humans and
automation.

## Public contract

The CLI provides `init`, `doctor`, `status`, `tree`, `spec new`, and `spec validate`.
`init` creates a validated root artifact set only in an uninitialized project;
it is a no-op for a current project and refuses every other existing state.
`doctor` reports project-state diagnostics without modifying files. `status`
reports a versioned, machine-readable project and component inspection without
modifying files. `tree` renders component layout, and specification commands
create or validate component contracts. `doctor` also reports whether every
required root and discovered component artifact is tracked by the selected Git
or jj repository. Commands use project-local configuration and produce
deterministic, non-interactive output.

The root component also owns the version-2 `TODOS.yaml` contract. A queue is a
durable, version-controlled execution plan for one component, not an informal
checklist. Every task records its stable identity, actionable work, reason,
expected result, source requirement references, dependencies, lifecycle role,
state, and audit timestamps. The queue records the component and immediate
parent specification revisions that its plan was reviewed against so later
tools can identify stale work before selecting or executing it. Until the
future queue CLI exists, the YAML file itself is the human task-content
interaction surface; `doctor` reports only its root-artifact validity.

</details>

<details>
<summary>Layer 2: Architectural guarantees</summary>

## Constraints and invariants

- Root configuration is limited to 64 KiB; specifications and inspected root
  text artifacts are limited to 1 MiB.
- Discovery uses configured, hard-bounded limits for depth, directories,
  components, directory entries, and relative-path bytes.
- Persistent writes use no-clobber same-directory temporary files. Root
  initialization is not a multi-file transaction and must remain recoverable
  through explicit user action.
- The root component performs no network or LLM invocation. It does not follow
  link-like paths and requires a trusted workspace before future task execution.
- VCS inspection never stages, commits, or snapshots a working copy. Git uses
  native tracking and ignore semantics; jj inspects only its saved snapshot.
- A filesystem-loaded TODO queue is UTF-8 YAML at most 1 MiB. The root
  artifact reader enforces this bound before parsing; the public in-memory
  parser validates a caller-provided string and therefore has no independent
  byte limit. Every future filesystem queue loader must apply the same bound
  before calling it. Its schema version is independent of every other artifact
  version. Schema validation rejects unknown fields, duplicate task IDs,
  invalid references, dependency cycles, invalid lifecycle order, invalid
  state-specific metadata, non-canonical collections, and malformed timestamps
  rather than silently discarding user-authored state.
- Canonical queue serialization preserves declared task order and emits a
  stable field order, two-space indentation, LF endings, sorted dependency and
  requirement-reference lists, and normalized timestamps. Equivalent queue
  values therefore produce byte-identical output for reproducible VCS diffs.
- A task is executable only when its component revalidation state is `current`,
  its dependencies are `completed`, and its lifecycle predecessors have
  completed. Phase 2's execution implementation owns the lock, atomic write,
  and attempt-record behavior; this schema supplies the validated data it
  needs and does not itself execute commands.
- Status inspection is read-only. It validates each discovered component's
  adjacent artifacts with the same versioned parsers as root inspection,
  compares only the component `SPEC.md` and immediate parent `SPEC.md` exact
  UTF-8 bytes with recorded SHA-256 revisions, and never writes derived stale
  evidence. Component status precedence is unsupported version, invalid,
  missing, stale, blocked, then current, so an actionable artifact failure
  cannot be hidden by a lower-priority workflow state.
- `status` output has format version 1. Its default text and `--format json`
  variants contain the same information in deterministic component and
  artifact order. A completed inspection exits 0 regardless of its reported
  state; inspection I/O failures exit 1; parser help exits 0 and parser input
  errors use clap's nonzero exit status. JSON is a report, not an instruction
  to mutate project state.

</details>

<details>
<summary>Layer 3: Detailed strategy and algorithms</summary>

## Design and failure paths

Root inspection classifies a project as uninitialized, current, partial,
invalid, or unsupported-version. Initialization writes only uninitialized
projects, performs no writes for current projects, and refuses every other
existing state. `TODOS.yaml` ordering is a root-contract authoring rule; the
Phase 1 parser deliberately performs only structural validation. Discovery
reads a configured component root lexically, rejects resource-limit and
hierarchy violations, and reports missing or filesystem-malformed adjacent
artifacts. Content validation of a non-root component's adjacent artifacts is
owned by that artifact's future component-inspection surface, not discovery.
Specification validation uses the versioned three-layer Markdown format and
returns line-aware diagnostics. Every operation returns contextual errors
rather than silently repairing, migrating, overwriting, or following links.
VCS auto-selection refuses a checkout containing both Git and jj until the
project owner selects one in `kvist.toml`.

## TODO queue schema and validation

`TODOS.yaml` version 2 is a mapping with exactly `schema_version`, `component`,
and `tasks`. `schema_version` is the integer `2`. The `component` mapping has
these fields:

| Field | Value | Tool use |
| --- | --- | --- |
| `specification_revision` | `sha256:` followed by 64 lowercase hexadecimal digits | The digest of this component's exact UTF-8 `SPEC.md` bytes when its queue was reviewed. P2-02 inspection will compare it with the current file to detect a local specification change. |
| `parent_specification` | `null` for the root component, otherwise a mapping with `path: "../SPEC.md"` and `revision` | Limits dependency tracking to the immediate parent. Component hierarchy rules make that parent exactly one directory above the child, so the path is always `../SPEC.md`; its revision is the recorded SHA-256 digest. P2-02 inspection will compare it with the current parent file to detect the ripple effect without loading peer components. |
| `revalidation` | mapping described below | Records whether the plan is safe to select and the evidence needed to explain or resolve stale state. |

`revalidation` has `state`, `checked_at`, `stale_since`, and `causes`.
`state` is `current` or `stale`. A current queue has a valid UTC timestamp in
`checked_at`, `null` `stale_since`, and no causes. A stale queue has valid UTC
timestamps in both fields, a `stale_since` not later than `checked_at`, and
one or more causes. Each cause records `kind`, `path`, `expected_revision`,
and `observed_revision`; `kind` is
`component-specification-revision-changed` or
`parent-specification-revision-changed`; its nonblank path and distinct valid
expected and observed revisions are required. This makes staleness attributable
rather than a lossy boolean.

Every task mapping has these fields:

| Field | Value and constraint | Tool use |
| --- | --- | --- |
| `id` | 1-64 character lowercase kebab-case identifier | Stable dependency target, audit key, machine-readable selection target, and VCS-merge anchor. IDs are unique and never renumbered or reused. |
| `title` | Nonblank one-line summary, at most 120 Unicode scalar values | Human task-list label and stable concise status output. |
| `description` | Nonblank actionable work description, at most 4,096 Unicode scalar values | Gives an implementer the bounded scope of the task; it is included in future execution context. |
| `context` | Nonblank background or triggering condition, at most 4,096 Unicode scalar values | Explains why the task exists and lets reviewers judge whether the task still applies after change. |
| `purpose` | Nonblank value or risk addressed, at most 4,096 Unicode scalar values | Explains usefulness, preventing activity that has no architectural value. |
| `expected_outcome` | Nonblank, observable completion condition, at most 4,096 Unicode scalar values | Supplies the completion assertion for the executor, reviewers, and later compliance comparison. |
| `kind` | `test`, `implementation`, `security-audit`, or `compliance-review` | Enforces the mandatory lifecycle and tells execution which trust boundary applies. |
| `status` | `pending`, `in-progress`, `blocked`, or `completed` | Controls eligibility and exposes progress without inference from prose. |
| `depends_on` | Lexically sorted, duplicate-free task-ID list | Defines the local directed acyclic graph. Selection requires all listed tasks to be completed. |
| `requirements` | Lexically sorted, duplicate-free `SOURCE#LOCATOR` strings with nonblank parts | Links task intent to a precise specification, contract, or roadmap requirement for traceability. `SOURCE` identifies the durable artifact and `LOCATOR` identifies its local requirement/heading. Tools retain and display these references; review uses them to form its evidence set. |
| `timestamps` | Mapping described below | Provides attributable, comparable transition history without trusting file modification time. |
| `blocked_reason` | `null` unless status is `blocked`, then nonblank text | Makes a blocked task actionable and visible to automated status reporting rather than silently skipping it. |

`timestamps` has `created_at`, `updated_at`, and `completed_at`. The first two
are UTC RFC 3339 instants with whole seconds (`YYYY-MM-DDTHH:MM:SSZ`);
`completed_at` is `null` unless status is `completed`, when it is a UTC instant
not earlier than `created_at` or `updated_at`. `updated_at` must not precede
`created_at`. Tools set these values on creation and every persisted state
transition. A version-1 migration records the instant it created each
version-2 task record and retains the original file in version control; it
must not invent an earlier historical instant.

Task states transition only as follows: `pending` to `in-progress` or
`blocked`; `in-progress` to `pending`, `blocked`, or `completed`; and
`blocked` to `pending` or `in-progress`. `completed` is terminal for a task
ID: renewed work needs a new task linked by a requirement reference, keeping
the completed evidence intact. Every transition updates `updated_at`; only the
transition to `completed` sets `completed_at`. The future state-update command
will reject all other transitions and write an append-only attempt record.

Dependencies may point only to a different task in the same queue. All IDs
must exist, the graph must be acyclic, and a task can depend only on an earlier
declared task. Declared task order is the deterministic human and execution
tie-breaker; it is not inferred from hash-map iteration. For version 2, a
"deliverable" is the explicit transitive dependency chain terminating at a
later lifecycle task; there is deliberately no separate deliverable-grouping
field. Lifecycle work in a chain is declared in this order: `test`,
`implementation`, `security-audit`, `compliance-review`, with each later role
having a preceding role of the required kind in that chain. The schema
validates this kind-based ordering. Requirement references and explicit
dependencies, rather than unstated grouping, are the traceability evidence
that the chain addresses the same component change.

P2-02 inspection will hash the target component's `SPEC.md` and, for a child,
only its immediate parent's `SPEC.md`. A digest mismatch will create the
corresponding revalidation cause and make the queue stale. The later atomic
state-update command will persist that derived result; it will never silently
rewrite the recorded revisions. A human will revalidate by reviewing affected
tasks, updating their requirement references or task plan as needed, recording
the new digests, and clearing causes with a new `checked_at`. This provides the
upstream-change ripple signal while preserving the component context boundary.

Version 1 is intentionally unsupported by the version-2 parser. Migration is
explicit and no-clobber: preserve the original file in VCS, map each legacy
`id`, `status`, and `description` into a new complete task record, add the
human-reviewed title/context/purpose/expected outcome/requirements, compute
the component and parent revisions, and record fresh timestamps. Kvist must
not invent missing provenance or overwrite the legacy file automatically. A
future `todo migrate` command will perform only this documented, opt-in
transformation and retain migration evidence.

## Project status inspection

`kvist status [PROJECT_DIR] [--format text|json]` is the phase-2 read-only
inspection surface. It first runs the existing root inspection. When the root
state is not `current`, it reports that state and no component records because
the configured component root cannot be trusted. When it is current, it loads
the validated project configuration, discovers components with configured
limits, and emits one record for every discovered component in lexical order,
including the configured component root as `.`. Discovery failures become a
top-level inspection failure record; they do not suppress the valid root
result or mutate files.

Each component record has its path relative to `component_root`, the states of
`SPEC.md`, `TODOS.yaml`, and `DOCS.md`, a component state, and optional
revalidation causes. Artifact states are `valid`, `missing`, `invalid`, or
`unsupported-version`. A complete component validates all three contents:
specifications use the specification validator; queues use the version-2
queue parser after the 1 MiB UTF-8 bound; and documentation uses its existing
version marker and heading validation. An incomplete or filesystem-invalid
component retains its discovery artifact state without attempting content
reads.

For a valid queue, inspection computes SHA-256 over the exact bytes of that
component's valid `SPEC.md` and, for a non-root component, its immediate
parent's valid `SPEC.md`. It compares those values only with
`component.specification_revision` and
`component.parent_specification.revision`, respectively. A mismatch produces
the corresponding attributable cause with paths `SPEC.md` or `../SPEC.md`;
the in-memory revalidation result is stale when either the queue records
`stale` or inspection finds a mismatch. Inspection never changes queue
timestamps, task state, recorded revisions, or files. A `blocked` component
has a valid, current queue with one or more blocked tasks and no higher
precedence condition. A `current` component has valid artifacts, current
revalidation, and no blocked tasks.

The text format begins with `status-format-version: 1`, followed by project
and root states, then component records in lexical order. The JSON format is a
single UTF-8 object with `format_version`, `project_path`, `project_state`,
`component_root`, `components`, and `discovery_error` keys in that order.
Components contain
`path`, `state`, `artifacts`, and `revalidation_causes` keys in that order;
artifacts are ordered `SPEC.md`, `TODOS.yaml`, `DOCS.md`. JSON path strings
are lossy display representations and must not be used as persistent file
identifiers. Text-format dynamic values escape backslashes, carriage returns,
line feeds, tabs, and other ASCII control characters, so untrusted paths,
diagnostics, or queue evidence cannot forge report records. This versioned
report is designed for scripts and future web/LSP clients; consumers must
treat unknown future versions as unsupported rather than guessing semantics.

## Task selection and state updates

`kvist task next COMPONENT_DIR` selects but does not modify the first ready
task in declared queue order. `kvist task transition COMPONENT_DIR TASK_ID
STATUS` is the only phase-2 writer; it changes one task status and records the
attempt. Both commands require a current root project, a discovered complete
component, a valid version-2 queue, current component revalidation, and
complete VCS tracking. `COMPONENT_DIR` is a component-root-relative normal
path; `.` names the configured component root. These commands do not invoke
providers, tests, shells, or network services.

A task is ready only when it is `pending`, every explicit dependency is
`completed`, and every preceding lifecycle role in its transitive dependency
chain is `completed`. `next` prints the selected task ID or `no ready task`.
Stale, blocked, incomplete, invalid, unsupported, untracked, or otherwise
non-current components are rejected rather than treated as empty queues.

Transitions use the version-2 state machine. Moving to `in-progress` requires
that the task is ready; moving to `completed` requires `in-progress`; and
moving to `pending` or `blocked` requires an active or blocked task as allowed
by the state machine. The command supplies a nonblank `--reason` only for
`blocked`; every other status stores `blocked_reason: null`. It sets
`updated_at` to the current whole-second UTC time, and sets `completed_at`
only when entering `completed`. It never changes queue revisions,
revalidation evidence, task definitions, dependencies, or requirements.

Before reading the queue, a transition atomically creates
`COMPONENT_DIR/.kvist-task.lock` with no replacement. Its contents identify
the command start time and task ID; another transition fails while the lock
exists. Stale locks are never removed automatically: the owner must inspect
the component, confirm no writer is active, and remove the named lock
explicitly. The lock is removed only after the queue and audit sequence
finishes or an in-process failure is handled.

Each transition appends an attempt record to
`COMPONENT_DIR/.kvist-attempts/TASK_ID.jsonl`. The record sequence is
`prepared` before the atomic queue replacement and `committed` after it. Queue
replacement is a same-directory no-clobber temporary write, file sync, rename,
and parent-directory sync. A cancellation or crash can leave a prepared record
and either old or new queue; it never reports success unless the committed
record is written. Future recovery tooling must retain and reconcile prepared
records explicitly rather than guessing or silently repairing them.

</details>
