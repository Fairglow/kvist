<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

> Historical evidence notice: this record predates the one-way move from the
> repository-root Rust layout to `engine/`. Its original evidence paths are
> retained rather than rewritten as a new clean-slate derivation.

## Evidence boundary

This record was derived from the Rust implementation files directly under
`src/`, the integration tests under `tests/`, and the root Cargo manifest.
`src/agent_runtime/` was not inspected and is treated as an opaque dependency.
No intent, architecture, existing implementation-record, review, guide, or
version-control evidence was used.

The root package is `kvist` version `0.1.0`, uses Rust edition 2024, declares a
minimum Rust version of 1.94, forbids unsafe code in the library, and fails
compilation on non-Linux targets. It builds a library and a CLI binary. The
workspace also names the opaque `agent-runtime` package as a path dependency.

## Observed process boundary

- `kvist::run()` parses process arguments with Clap and dispatches through
  `cli::execute`.
- The binary prints non-empty successful output to stdout and returns exit
  status 0.
- Failures are printed to stderr. Parser-controlled help and parser errors use
  Clap's exit status and rendering; all other root-defined errors return status
  1. Structured component-validation failures in JSON mode are written as the
  complete JSON error object without an added `error:` prefix.
- `--json` is a global flag. Most commands wrap successful results in JSON;
  `status` emits its own versioned JSON representation and `prompt` emits a
  JSON object containing captured provider output. Plain mode emits
  human-readable text.
- JSON generation is implemented in the root component rather than through one
  uniform response schema. Command-specific fields and command names therefore
  differ by operation.

## Observed CLI commands

### `init [PROJECT_DIR]`

- Defaults to `.` and creates a missing directory tree.
- Rejects a file or link-like project path.
- Inspects the complete root artifact set before writing. A current project is
  reported unchanged; partial, invalid, or unsupported-version state is
  refused without repair.
- An uninitialized directory containing both `Cargo.toml` and a real `src`
  directory is routed to existing-Rust-project conversion instead of normal
  initialization.
- Normal initialization creates nine deterministic files: `kvist.toml`,
  three project-level Markdown files, three root-component intent documents,
  `src/TODOS.yaml`, and `src/IMPL.md`.
- Parent directories are checked as real, non-link directories. Individual
  files are written via synchronized same-directory temporary files with
  no-clobber publication.
- Preflight validation reduces overwrite risk, but the sequence has no
  transaction spanning every generated file; an I/O failure after earlier
  writes can leave a partial artifact set.

### `convert PROJECT_DIR`

- Requires a real project directory, a parseable Cargo manifest with nonblank
  package name and version, and a real non-link `src` directory.
- Reads package description, authors, dependency names, and feature names.
- Creates `.kvist/` without changing the manifest, implementation, tests, or
  benchmarks, then writes draft `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`,
  `TODOS.yaml`, `IMPL.md`, and `COMPLIANCE_REVIEW.md`.
- Generated component documents and queue are validated before publication.
- An existing `.kvist` directory is accepted as already converted only when
  all six expected entries are real files. Other pre-existing metadata states
  are refused.
- Writes are individually no-clobber and the metadata directory is synced, but
  there is no all-files rollback after publication begins.

### `import REPO_URL [DEST_DIR]`

- Runs `git clone --branch <branch> --depth 1` directly without a shell;
  `--branch` defaults to `main` and destination defaults to `.`.
- The destination must be absent or an empty directory and must be representable
  as UTF-8 for the `git` argument.
- An optional `--component` selects a directory inside the clone.
- If the selected directory has a complete adjacent artifact set, or a
  complete `.kvist` conversion set, the three documents and queue are parsed
  and validated.
- Otherwise the selected directory is passed to initialization, which may
  initialize it or convert an existing Rust project.
- Clone or post-clone validation failures are reported, but the cloned
  destination is not removed automatically.

### `reverse-discover PATH`

- Requires a real non-link directory and refuses to overwrite intent documents
  in `.kvist` or `src`.
- Recursively scans non-link files while skipping `.git`, `.kvist`, `target`,
  and `node_modules`.
- For Rust it records lines beginning with selected `pub` declarations and
  simple test-name patterns. For Python it records public-looking classes and
  functions. Other languages are not analyzed.
- Supplemental Markdown contributes only its first line. Read failures for
  supplemental Markdown are converted to empty content rather than reported.
- The scanner is textual, not a language parser; multiline declarations,
  re-exports, macros, visibility variants, nested semantics, and runtime
  behavior can be missed or misclassified.
- Writes six generated files under `.kvist`. It validates generated documents
  and queue first and uses no-clobber file creation, but has no transaction or
  rollback spanning the complete set.

### `tree [PROJECT_DIR]`

- Loads project configuration, discovers the configured component root, and
  renders a deterministic ASCII list headed by `component root:`.
- Components are lexically ordered and indented by relative path depth.
- Each component is labeled complete, incomplete with missing filenames, or
  invalid with the observed filesystem-object class.
- This command checks artifact layout, not complete artifact contents.

### `doctor [PROJECT_DIR]`

- Performs read-only root, component, and VCS inspection and renders the
  resulting `ProjectInspection`.
- Root state precedence is unsupported version, invalid, uninitialized,
  current, then partial.
- Component state precedence is unsupported version, invalid, missing, stale,
  blocked, then current.
- Inspection validates bounded UTF-8 regular files, version markers, required
  headings or document sections, queue semantics, recorded document hashes,
  nearest ancestor component-contract context, and durable-artifact tracking.
- Configuration or discovery errors encountered after a current root
  classification are retained as diagnostics rather than causing repair.

### `status [PROJECT_DIR]`

- Emits status format version 1 as stable text or compact JSON.
- Supports `--only-documents`, `--only-impls`, and `--unfinished`.
- Text output escapes control characters in paths and includes next-step
  guidance for stale, blocked, invalid, and missing components.
- JSON includes project state, component root, components, filtered artifact
  states, revalidation causes, and any discovery error.
- Rendering itself performs no filesystem I/O; command dispatch first obtains
  a `ProjectInspection`.

### `component new COMPONENT_DIR`

- Creates a missing component directory and writes deterministic version-1
  requirements, contract, and design templates.
- Refuses a link-like or non-directory component path.
- Preflights all three destinations and refuses the operation if any already
  exists.
- Checked-in template strings are structurally validated before each write.
  Publication is no-clobber but is not transactional across all three files.

### `component validate COMPONENT_DIR`

- Reads `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md` as regular non-link
  UTF-8 files no larger than 1 MiB each.
- Requires an exact line-one positive version marker, the supported version 1,
  all kind-specific headings exactly once and in order, and nonblank content
  below every required heading.
- Diagnostics use one-based line and column positions and deterministic source
  ordering. Validation does not rewrite documents.
- Plain mode returns a domain error on invalid input. JSON mode returns a
  structured object with `valid: false` and per-document diagnostics through
  the structured-failure path.

### `component accept COMPONENT_DIR`

- Requires a current project, complete VCS tracking, and a discovered component
  whose artifact conditions allow acceptance.
- Accepts component-root-relative paths, paths prefixed by the configured
  component root, and absolute paths inside the project.
- Revalidates the three local documents, computes SHA-256 revisions, resolves
  the nearest ancestor component through transparent directories, and records
  only that ancestor's `CONTRACT.md` revision for non-root components.
- Updates the queue's revision fields, sets revalidation to current, records a
  timestamp, clears staleness evidence, and atomically replaces `TODOS.yaml`.
- It does not modify the accepted Markdown documents.

### `task next COMPONENT_DIR`

- Requires a current project, a current component, and every required durable
  artifact reported as tracked by the selected VCS.
- Returns the first pending task in authored queue order whose complete
  dependency chain is completed. It returns `no ready task` in text mode or
  `ready_task_id: null` in JSON mode when none exists.
- Selection does not mutate the queue.

### `task transition COMPONENT_DIR TASK_ID STATUS`

- Supports pending, in-progress, blocked, and completed states.
- Allowed edges are pending to in-progress/blocked, in-progress to
  pending/blocked/completed, and blocked to pending/in-progress.
- Starting requires every direct and transitive dependency to be completed.
  Completion is accepted only from in-progress. Blocking requires a nonblank
  `--reason`; reasons are rejected for other target states.
- A user-owned per-component lock serializes mutation. Lock identity is derived
  from canonical project and component paths and stored outside the project in
  the platform user-state directory with restricted Unix directory
  permissions.
- Each transition appends a synced `prepared` JSON line, atomically replaces
  the queue, then appends a synced `committed` line under
  `.kvist-attempts/<task-id>.jsonl`.
- A trailing prepared record blocks later transition attempts pending explicit
  recovery. Normal scope exit removes the lock; abrupt process termination can
  leave a lock for `task unlock`.

### `task run COMPONENT_DIR [TASK_ID]`

- Refuses execution before mutation unless sandbox configuration, test policy,
  authenticated execution approval, runner identity, capability probe, current
  project/component state, and complete VCS tracking are present.
- If no task ID is supplied, it selects the first ready task. An already
  in-progress task can also be resumed.
- One lifecycle lock covers selection, transition, external execution,
  evidence, verification, and terminal transition.
- Test and implementation tasks use the developer profile; security-audit
  tasks use the security-reviewer profile; compliance-review tasks use the
  architect profile.
- The task prompt comes from a component-local language template, otherwise a
  user-global template, otherwise a built-in Rust, Python, or generic
  template. Language detection inspects only shallow file/manifest patterns.
  Missing built-in template files are created under `.kvist/templates` on a
  best-effort basis; related filesystem errors are ignored by that helper.
- The sandbox request mounts the selected component read-write at
  `/workspace/component`, mounts the project root contract read-only, and for
  non-root components mounts the nearest ancestor component contract
  read-only. The root code passes the five local artifact paths plus these
  context paths to the configured agent command.
- Agent stdout and stderr are combined, redacted using configured literal
  values and allowed environment values, bounded, and written to a unique log
  under `.kvist/logs`. Optional streaming writes the same retained combined
  output to stdout; stream-write errors are ignored.
- Optional token counts are read from a conventionally named JSON run record
  if present and parseable; absent or malformed records do not fail the task.
- Successful test, security-audit, and compliance-review executions complete
  immediately. A successful implementation execution additionally runs the
  configured test command in the sandbox. Verification failure or inability
  blocks the task with bounded redacted evidence.
- Agent failure, timeout, or output-limit termination blocks the task and
  records the bounded log path. State-transition and execution evidence is
  appended to the task attempt JSONL file.

### `task log COMPONENT_DIR TASK_ID`

- Requires the same current component and VCS context as other non-transition
  task reads.
- Selects the lexically latest real, non-link
  `.kvist/logs/<task-id>_*.log` and returns it as UTF-8 text.
- Missing log directories or matching files are reported as queue
  unavailability.

### `task approve-policy [PROJECT_DIR]`

- Resolves and hashes the complete effective execution material: schema and
  protocol versions, root-contract bytes, agent configuration source and
  profiles, redaction and resource policies, sandbox configuration, canonical
  runner path and content, and optional test policy.
- Rejects a legacy project-contained approval record.
- Stores the approval outside the project/worktree under the platform
  user-state directory. The record is tied to canonical project and selected
  VCS worktree identities.
- Creates or reads a 32-byte user-owned secret and authenticates approval
  records with an HMAC-SHA-256 construction. Unix secret permissions must not
  expose group or other bits.
- Later execution reconstructs all material, validates record shape, digest,
  authentication tag, project/worktree identity, and equality with current
  inputs before returning the approved runner identity.

### `task unlock COMPONENT_DIR`

- Resolves the user-owned component lock. A non-regular or link-like lock is
  refused.
- `--force` removes it directly; otherwise confirmation is read from stdin and
  only `y` or `yes` proceeds.
- Missing locks and user cancellation are successful no-op outcomes.

### `prompt [PROMPT]`

- Accepts exactly one direct prompt source, `--file`, or `--editor` according
  to Clap conflicts; detailed acquisition is delegated to the opaque
  `agent-runtime` dependency.
- Requires `--allow-host-execution`; without it the opaque runtime supplies the
  error.
- Selects developer, architect, or security-reviewer configuration. An
  unrecognized role is rejected.
- Supports an optional model, typed reasoning effort, idle timeout, loop
  detection, and restart limit.
- Root code builds an opaque-runtime supervision policy, renders a direct
  command without a shell, and re-renders it for retries with the runtime's
  retry notice.
- Plain mode connects supervised provider output to the terminal and emits no
  synthetic success text. JSON mode captures stdout and converts invalid UTF-8
  lossily before JSON escaping.

### `agent setup`

- Runs an interactive wizard using stdin and stdout in plain mode; JSON mode
  sends wizard interaction to stderr so stdout remains machine-readable.
- Either collects a provider profile through the opaque runtime or loads a
  named saved runtime profile.
- Assigns the model to developer, architect, security-reviewer, or all roles
  and stores it in project-local `kvist.toml` or the user-global config.
- Existing TOML is parsed and validated before editing. The wizard updates or
  appends matching model entries while preserving the rest of the document,
  validates the resulting configuration, enforces the 64 KiB bound, and
  atomically replaces or creates the file.
- `--force` is passed only to opaque-runtime profile collection as the
  root-observed failed-qualification override.

### `completions SHELL`

- Generates completion scripts for Bash, Zsh, Fish, or PowerShell.
- Plain mode emits the script. JSON mode embeds the script as an escaped JSON
  string.

## Observed configuration model

- `kvist.toml` must be a regular non-link UTF-8 file no larger than 64 KiB with
  positive supported `schema_version = 1`.
- `component_root` is required, non-empty, relative, and limited to normal path
  segments; absolute paths, `.` and `..` segments are rejected.
- Discovery defaults are depth 64, 10,000 directories, 10,000 components,
  10,000 entries per directory, and 4,096 encoded relative-path bytes. Config
  may lower or raise them only up to hard maxima of 256, 100,000, 100,000,
  100,000, and 32,768 respectively.
- VCS selection is `auto`, `git`, or `jj`, defaulting to auto.
- Sandbox configuration, when present, requires schema 1, an absolute nonblank
  runner path, `network = "deny"`, `mount = "component"`, and unique ASCII
  environment-variable names.
- Agent configuration resolution order is project-root `kvist.toml`, project
  `.kvist/config.toml`, user config, system config, then built-in defaults.
  Candidate external configuration files must be regular non-link files within
  the same 64 KiB limit. The selected source identity and SHA-256 digest are
  retained for execution approval.
- Three agent profiles contain model command templates, optional system
  prompts, model selection, optional token limit, timeout, output bound, and
  literal redaction values. Timeout is capped at 3,600 seconds and output at
  1,048,576 bytes.
- Test policy schema 1 contains component/project working-directory selection,
  environment allowlist, timeout, output bound, and component command entries.
  Test command inheritance walks from the selected component toward `.` and
  uses the first exact component match.

## Observed component discovery and project state

- A component has five adjacent artifacts: requirements, contract, design,
  task queue, and implementation record.
- Discovery always represents the configured root and represents descendants
  when at least one required artifact entry exists. Ordinary directories may
  transparently contain deeper components.
- Traversal is deterministic, bounded, and lexical. It skips `.git`, `.hg`,
  `.jj`, `node_modules`, and `target`.
- Any link-like descendant encountered outside an artifact filename aborts
  discovery rather than being followed. Artifact links are classified as
  invalid.
- The implementation record validator requires only the exact line-one
  positive supported marker and a line equal to
  `# Component Implementation Record`; it does not validate this record's
  remaining section structure or factual completeness.
- Staleness is derived from SHA-256 comparisons of the three local documents
  and, for children, the nearest ancestor component's contract. Parent
  requirements and design are not compared.

## Observed task queue format and invariants

- `TODOS.yaml` is a strict Serde schema with unknown fields denied and
  `schema_version = 1`.
- Revisions are `sha256:` plus exactly 64 lowercase hexadecimal digits.
- Parent contract paths consist of one or more `..` segments followed only by
  `CONTRACT.md`.
- Current revalidation requires no stale timestamp or causes. Stale
  revalidation requires a valid stale timestamp not later than `checked_at`
  and at least one cause whose expected and observed hashes differ.
- Task IDs are 1-64 lowercase kebab-case ASCII characters. Titles are trimmed,
  one line, nonblank, and at most 120 characters. Description, context,
  purpose, and expected outcome are trimmed, nonblank, and at most 4,096
  characters.
- Dependency and requirement lists must be lexically sorted and duplicate-free.
  Requirements must contain nonblank source and `#` locator parts.
- Dependencies must refer to earlier tasks, cannot self-reference, and must be
  acyclic.
- Implementation tasks require a transitive test predecessor, security-audit
  tasks an implementation predecessor, and compliance-review tasks a
  security-audit predecessor.
- Timestamps use exact whole-second UTC `YYYY-MM-DDTHH:MM:SSZ` syntax and are
  calendar-validated. Completion and blocker metadata must agree with status.
- Serialization emits deterministic quoted YAML and revalidates before
  producing output.

## Observed VCS behavior

- VCS inspection invokes Git and jj directly and requests locale-stable output.
  It does not stage or commit.
- Auto selection rejects a checkout in which both Git and jj are detected.
- Git uses `ls-files` and `check-ignore --no-index` with bounded batches and
  NUL-separated paths. On Unix it preserves non-UTF-8 path bytes.
- jj uses `--ignore-working-copy` and its saved `@` snapshot. A path absent
  from that snapshot is reported as not tracked by jj without claiming whether
  it is ignored, excluded, or newly created.
- Required paths must be non-empty normal relative paths. Paths too large for
  the bounded VCS argument batch are reported with unknown tracking state.
- Task commands require every reported durable artifact to be `Tracked`; an
  ignored, untracked, missing, unknown, or jj-unlisted artifact blocks them.

## Observed sandbox and external-process boundary

- The configured runner must be an absolute real file or directory outside
  both the canonical project and selected VCS worktree.
- Runner identity consists of canonical path and SHA-256 content. Directory
  identity concatenates readable regular-file contents in filename order.
- Before launch, identity is recalculated and must equal the approved identity.
  The runner is copied into a restricted temporary state directory; on Linux
  the copied executable is opened and launched through `/proc/self/fd/<fd>`.
- Runner processes are started directly, never through a shell, with a cleared
  environment populated only from the configured allowlist.
- Availability requires exact stdout from a version-1 capability probe
  asserting denied network and a component mount.
- Execution sends a JSON protocol-version-1 request on stdin. It declares the
  working directory `/workspace/component`, network denial, one read-write
  component mount, optional read-only mounts, arguments as an array, explicit
  environment, and context-file names.
- Stdout and stderr are captured concurrently under one combined byte budget.
  Exceeding the budget or timeout kills the runner's process group with
  `SIGKILL`; there is no host-execution fallback.
- The implementation establishes and validates the runner protocol request and
  response boundaries but cannot itself prove that an arbitrary configured
  runner enforces the requested filesystem or network isolation.

## Public Rust library surface

- Public modules expose artifact templates, configuration types/loading,
  document creation/validation, conversion, import, project inspection,
  discovery, status/tree rendering, task queue parsing/validation/serialization,
  task commands, VCS inspection, sandbox execution, agent command construction
  and execution, prompt resolution, reverse discovery, initialization, and the
  setup wizard.
- Public result structures generally expose owned paths, classifications,
  bounded output, token counts, timestamps, or diagnostics. `KvistError` is
  non-exhaustive and is the shared domain error.
- Several public root functions call `agent-runtime` for prompt acquisition,
  command rendering, supervision, profile storage/collection, and runtime
  errors. Only the arguments supplied and results consumed by the root
  component are established here.

## Failure and durability characteristics

- Filesystem, parsing, subprocess, validation, clock, approval, and sandbox
  failures are converted to explicit domain errors with an operation and path
  where implemented.
- Root artifact and component-document readers reject links, non-files,
  oversize content, invalid UTF-8, malformed schemas, and unsupported versions.
- Atomic replacement uses a same-directory synchronized temporary file and
  syncs the parent directory. No-clobber creation synchronizes the temporary
  file before publication but does not explicitly sync the parent directory in
  the shared helper.
- Multi-file generation and repository import are not transactional.
- Task mutation has a prepared/committed audit protocol and external lock, but
  crash recovery is refusal-oriented: a detected incomplete prepared attempt
  must be recovered explicitly rather than automatically rolled forward or
  back.
- Evidence redaction is literal string replacement. It does not detect encoded,
  transformed, partially matching, or otherwise unknown secrets.
- Output limits retain a prefix at a valid UTF-8 boundary. Invalid external
  bytes are converted lossily where byte output becomes text.

## Test evidence observed

The repository contains unit tests in root modules and integration suites for
agent command rendering/execution, component commands and documents,
completions, conversion, discovery and configured limits, import,
initialization, JSON output, project state, prompt acquisition/execution,
reverse discovery, status, task commands, task queues, tree rendering, VCS
inspection, and the setup wizard.

Observed test cases cover:

- initialization completeness, idempotence, no-clobber behavior, invalid
  parents, files, and symbolic links;
- all root project-state classes, independent version domains, oversized and
  invalid-UTF-8 artifacts;
- deterministic discovery/tree order, ignored directories, link refusal,
  malformed layouts, transparent namespace directories, and every configured
  traversal bound;
- document templates, bounded regular-file validation, diagnostics, and
  symbolic-link rejection;
- canonical queue round trips, transition edges, malformed YAML, unknown
  fields, hash/path/timestamp metadata, dependency errors, cycles, and
  lifecycle ordering;
- status text/JSON, filters, escaping, blocked/stale/missing/invalid states,
  nearest-parent contract behavior, and independent local revision causes;
- Git tracked/ignored/untracked behavior, missing repositories, malformed
  repositories, non-UTF-8 Unix paths, and conditional jj snapshot behavior;
- task selection without queue writes, VCS prerequisites, prepared/committed
  transitions, block reasons, acceptance revision updates, parent-contract
  resolution, lock behavior, execution role routing, prompt templates, log
  selection, test-command success/failure/timeout/output bounds, failure
  blocking, redaction across streams and outputs, and attempt evidence;
- approval changes to root-contract bytes, agent source/templates/resource
  limits, runner content, sandbox configuration, and test policy, including
  rejection of project-forged approval records and runner changes after probe;
- sandbox absence and project/worktree-local runner refusal before task
  mutation;
- prompt files, redirected stdin, editor use, conflicts, size/link rejection,
  host acknowledgement, typed reasoning efforts, literal shell metacharacters,
  output bounds, JSON capture, and invalid provider UTF-8;
- conversion/import/reverse-discovery outcomes and refusal to overwrite;
- setup wizard merging, pre-save qualification, force/cancellation paths,
  project/global routing, JSON stdout separation, invalid existing
  configuration, and standalone-profile materialization.

These tests were inspected as source evidence only. They were not executed
during this documentation pass, so this record does not assert their current
pass/fail result.

## Uncertainty and limits of this record

- The opaque `agent-runtime` implementation was not read. Its internal prompt
  limits, editor behavior, command grammar, provider qualification,
  supervision, retry/loop detection, profile persistence, and error semantics
  are unknown except where root code supplies inputs or consumes outputs.
- No build, test, external Git/jj operation, sandbox probe, provider command,
  or wizard session was executed for this record.
- Behavior dependent on operating-system facilities, filesystem race timing,
  environment variables, user-state permissions, external tools, configured
  runner correctness, and provider behavior remains observationally
  unverified.
- The source contains conditional non-Linux branches, but the library's
  compile-time Linux restriction prevents treating those branches as a
  supported runtime surface.
- This record describes observed implementation and test source. It makes no
  statement about intended behavior, sufficiency, or compliance.
