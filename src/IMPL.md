<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Observation basis

This record describes behavior observed in the root Rust crate and its tests. It
does not certify conformance to a separate specification. The `agent_runtime`
crate is treated as a dependency; its reusable behavior is recorded separately.

## Build and runtime surface

- The package builds a `kvist` binary and exposes a library used by integration
  tests.
- The crate targets Rust edition 2024, declares Rust 1.94 as its minimum
  compiler version, forbids unsafe code, and rejects non-Linux targets at
  compile time.
- `main` parses command-line arguments, dispatches into library modules, prints
  successful command output to stdout, and renders domain errors to stderr.
- Major command families initialize and inspect projects, discover and render
  component trees, validate component documents, import or reverse-discover
  component material, manage task queues, accept observed component revisions,
  approve execution policy, run tasks, inspect task logs, and remove eligible
  orphaned locks.
- Component-document JSON validation serializes structured diagnostics with
  document, diagnostic kind, line, column, and message fields. Invalid
  documents cause a nonzero exit, JSON-only stderr, and empty stdout.

## Project and component discovery

- Project discovery walks ancestors from a supplied path and loads the nearest
  recognized project configuration.
- Configuration names a component root. Component discovery walks beneath that
  root in deterministic path order.
- A directory is recognized as a component from its component artifacts.
  Directories without component artifacts can act as transparent namespace
  directories between components.
- Discovery rejects link-like component paths rather than following them.
- Component and configuration text reads enforce code-defined size limits.
  Root component text artifacts are limited to 1 MiB and `kvist.toml` is
  limited to 64 KiB.
- Paths are canonicalized and checked against the selected project, component
  root, and VCS worktree boundaries before security-sensitive operations.

## Component documents and observed state

- The engine recognizes `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`,
  `TODOS.yaml`, and `IMPL.md` as component artifacts.
- An implementation record is accepted by the production validator only when
  its first line is
  `<!-- kvist-implementation-record-version: 1 -->` and it contains the exact
  heading `# Component Implementation Record`.
- Project inspection parses component documents and the task queue, reports
  missing or invalid artifacts, derives phase/state information, and emits
  stable human-readable or structured output where the command supports it.
- Local requirements, contract, and design revisions are hashed and compared
  independently. A change to any one is attributed to that document rather
  than collapsed into a single local revision.
- The only document revision propagated across a component boundary is the
  nearest ancestor component's contract. Changes to an ancestor's requirements
  or design can stale that ancestor without staling its child.
- The nearest parent component can be separated by transparent namespace
  directories. Inspection computes the relative path from the child component
  to that parent's `CONTRACT.md`.

## Task queue format and validation

- `TODOS.yaml` is parsed into a versioned queue with component revision
  references and an ordered list of typed tasks.
- Queue validation rejects unknown fields, duplicate task identifiers, unknown
  dependencies, self-dependencies, dependency cycles, dependencies on later
  declarations, illegal status data, and invalid task-kind sequencing.
- Required task-kind ancestry is checked transitively through the dependency
  graph.
- A recorded parent contract path consists of one or more leading `..`
  components followed directly by `CONTRACT.md`. Absolute paths, peer paths,
  alternate filenames, and other path forms are rejected.
- Inspection also compares the recorded parent path with the path computed from
  discovered component ancestry, so a syntactically safe but incorrect path is
  invalid.
- Queue serialization is deterministic and task mutations use atomic file
  replacement.

## Task state and readiness

- Tasks use pending, in-progress, blocked, and completed states with
  code-enforced transitions.
- Entering in-progress from pending or blocked requires every direct and
  transitive dependency to exist and be completed.
- `task next` selects the first pending ready task in declared queue order and
  does not mutate the queue.
- Automatic selection in `task run` uses the same readiness predicate as
  `task next`, including the full transitive dependency check.
- Explicitly selecting an already in-progress task is allowed so an interrupted
  execution can be resumed without reapplying the pending-to-in-progress
  readiness transition.
- Blocking requires a nonempty reason. Completion is accepted only from
  in-progress.
- Task transitions are protected by an exclusive component lock and use
  prepared and committed durable audit entries around the queue replacement.
- The execution lifecycle uses a separate lock spanning selection, agent
  execution, verification evidence, and terminal transition. Agent-visible
  component contents cannot release that host-held lock.

## Component acceptance

- Acceptance validates the component before mutating its queue.
- Component arguments can be `.`, component-relative paths, paths prefixed by
  the configured component root, or absolute paths within the project.
- Acceptance records current local document digests.
- For a child component it computes the nearest parent contract across
  transparent namespace directories and repairs both the recorded relative
  path and its SHA-256 revision.
- Writes use the queue serializer and atomic replacement; invalid documents or
  paths leave the queue unchanged.

## Execution approval

- Task execution requires project-local agent, sandbox, and test-policy
  configuration where applicable.
- The authenticated approval record binds normalized configuration, agent
  source, configured resource limits, selected VCS state, the canonical
  content-addressed sandbox runner identity, and the SHA-256 digest of the
  exact project-root `ROOT_CONTRACT.md`.
- Root-contract binding accepts only a regular non-link file within the 1 MiB
  root-artifact limit. Approval and execution checking both hash its bytes.
- Approval performs the runner capability probe before recording approval.
- Execution recomputes the bound inputs and refuses changed or forged approval
  data before task mutation or runner request execution.
- A changed root contract produces a specific unapproved-policy error before
  the sandbox capability probe, component lock creation, queue read, or queue
  mutation.
- Runner paths must be absolute and outside both the project and selected VCS
  worktree. Link-like, incorrectly owned, or insecurely permissioned runner
  paths are rejected.
- The runner is copied into a restricted temporary state directory and its
  digest is checked again before launch.

## Sandbox request and runner lifecycle

- Sandbox requests are encoded as JSON with a versioned protocol, direct
  program and argument fields, a fixed component working directory, denied
  network access, explicit environment data, context files, and mounts.
- The target component is mounted read-write at `/workspace/component`.
- Additional context mounts are read-only. Agent task execution supplies the
  root contract at `/workspace/context/ROOT_CONTRACT.md` and, for a child, the
  nearest parent contract at `/workspace/context/PARENT_CONTRACT.md`.
- Parent selection for that mount follows discovered component ancestry across
  transparent namespace directories.
- The runner is launched directly without shell interpolation.
- The launch command clears the host environment before adding only values
  selected by the configured allowlist. Ambient caller variables therefore do
  not reach the runner unless explicitly allowed.
- The runner starts as leader of a dedicated process group. Kvist captures
  stdout and stderr concurrently under one combined output budget.
- On deadline expiration or combined output exhaustion, Kvist sends `SIGKILL`
  to the runner process group and waits for the group leader. This terminates
  runner descendants holding inherited output pipes and allows capture threads
  to finish without waiting for a sleeping runner tree.
- The bounded result separately reports exit status, timeout, and output-limit
  exhaustion. There is no host-side fallback execution of the requested
  program.

## Agent task execution and verification

- The root engine builds bounded context lists and invokes `agent_runtime`
  through the approved sandbox request.
- Execution output is streamed and persisted only after configured redaction
  and combined-size limiting. Redaction handles configured values split across
  stdout and stderr boundaries.
- Agent failure, timeout, or output exhaustion produces durable attempt
  evidence and transitions the task to blocked with a bounded reason.
- After successful agent execution, configured verification commands are run
  through the sandbox under their own timeout, environment, working-directory,
  and output limits.
- Verification failure, timeout, or excessive output blocks the task and
  records bounded evidence. Successful execution and verification transition
  the task to completed.
- Per-task JSON-lines attempt history and bounded log files are stored below
  the component. `task log` returns the latest matching execution log.

## Reverse discovery

- Reverse discovery requires its target to be a real directory and recursively
  inspects regular Rust, Python, and selected Markdown files.
- Link-like entries are skipped during traversal. `.git`, `target`, `.kvist`,
  and `node_modules` directories are not descended into.
- Generated documents and the generated queue are validated in memory before
  artifact publication.
- The destination `.kvist` metadata path may initially be absent, but an
  existing path must be a real, non-link directory. A regular file or
  link-like path is rejected.
- The metadata directory is revalidated after creation and again immediately
  before every artifact publication. Each artifact uses an atomic create-new
  write, so existing targets are not overwritten.

## VCS and filesystem boundaries

- Security-sensitive commands require the configured VCS selection and inspect
  tracking or worktree state before mutation.
- Durable writes use create-new files or same-directory atomic replacement,
  directory synchronization where implemented, and refusal of link-like state
  paths.
- Stable ordering is used for discovered components, directory-runner content
  hashing, diagnostics, and serialized queue data.
- Filesystem paths, YAML, TOML, Markdown, JSON, subprocess output, and
  environment values are treated as fallible inputs and mapped to contextual
  domain errors.

## Errors and recovery behavior

- Recoverable filesystem, parse, validation, VCS, approval, sandbox, task, and
  subprocess failures return domain errors rather than intentionally panicking.
- Validation errors identify the affected path or document and preserve
  structured diagnostics where supported.
- Mutating commands validate before replacement and release locks after either
  success or failure.
- Failed execution is represented durably by blocked task state, attempt
  records, and bounded logs. An explicit unlock command handles only locks that
  pass the engine's orphan checks.

## Incomplete or weakly evidenced surfaces

- Reverse discovery uses line-oriented source analysis and recursive traversal.
  No explicit traversal-depth, entry-count, or source-file-size bound was
  observed; regular Rust and Python files are read completely.
- Directory-form sandbox runners are hashed and copied, but the integration
  evidence is concentrated on executable-file runners.
- Prepared transition journal entries are durable, but no command dedicated to
  replaying or reconciling an interrupted prepared entry was observed.

## Verification evidence

- Component-state tests directly exercise independent local contract and design
  staleness and show that parent requirements/design changes do not propagate
  to a child.
- Task-queue tests cover dependency validation and safe ancestor contract path
  forms.
- Task-command tests cover component-root-prefixed acceptance, repair of parent
  paths through namespaces, and read-only nearest-parent contract mounting.
- JSON-output tests cover structured invalid-document diagnostics on stderr
  with a failing exit status.
- Task execution tests set an unapproved ambient variable and observe it as
  absent inside the runner.
- Approval tests change `ROOT_CONTRACT.md` after approval and observe rejection
  before sandbox request creation with the task queue unchanged.
- Reverse-discovery tests reject a symlinked `.kvist` directory without writing
  through it; production code also rejects non-directory metadata paths and
  revalidates the directory before each no-clobber write.
- Timeout tests exercise a shell runner with a sleeping descendant and assert
  return within the configured bound; output-limit tests exercise cancellation
  and bounded, redacted durable evidence.
- Task selection and transition code share the same transitive dependency
  traversal used by `task next`, transition-to-in-progress checks, and
  `task run` automatic readiness.
