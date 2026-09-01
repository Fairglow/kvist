<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Scope and evidence

This record describes behavior observed in the root Rust implementation and its
root integration/unit tests. The `src/agent_runtime/**` child component is
excluded; this record describes only the root-facing calls and process
boundaries around it. No compliance conclusion is made.

The root package is a Rust 2024 command-line binary/library. `main` delegates
to `kvist::run`, prints a nonempty successful `CommandOutput` to stdout with a
newline, and reports failures through the domain error presentation path.
Command parsing and dispatch are kept in the root crate so they can be tested
without spawning the binary.

## Observed command surface

The global `--json` flag selects compact structured output. The root dispatch
supports project initialization, conversion, repository import, tree rendering,
doctor/status inspection, reverse discovery, standalone prompting, task
selection/transition/execution/logging/unlocking/policy approval, component
document creation/validation/acceptance, agent setup, and completion-script
generation.

Successful JSON commands generally return one object on stdout. A failed JSON
component validation returns no stdout and a structured error object on stderr,
including `status`, `command`, `valid: false`, and diagnostics. Text commands
use stable human-readable messages; command errors are written to stderr.

## Project and filesystem behavior

`init` accepts a real directory (creating missing parents), refuses a file or
link-like project path, and writes a deterministic root artifact set only when
the project is uninitialized. Writes use same-directory temporary files,
synchronization, and no-clobber publication. A complete current project is
reported unchanged. Partial, invalid, or unsupported-version project states
are refused without repair or migration. An existing Rust project containing a
manifest and `src` is routed to the conversion path instead of normal
initialization.

Configuration is read from a bounded, regular non-link `kvist.toml`. The
component root is a nonempty relative path containing only normal segments.
Configuration has independent version checks and validates VCS selection,
discovery limits, agent profiles/models/resource policies, optional test
policy, and optional sandbox configuration. Agent configuration resolution
observed in the root code is:

1. an `agent` table in the project `kvist.toml`;
2. project-local `.kvist/config.toml`;
3. the user configuration path;
4. the system configuration path; or
5. built-in defaults.

Source identity and a SHA-256 content digest are retained with the resolved
agent configuration. User/system candidates must be regular non-link files
within the configuration size bound.

## Component discovery, documents, and status

The configured component root is represented as `.`. Descendants are included
only when at least one of `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`,
`TODOS.yaml`, or `IMPL.md` is present. Directory entries and components are
sorted lexically. `.git`, `.hg`, `.jj`, `node_modules`, and `target` are
ignored; link-like roots and descendants are rejected. Depth, directory,
component, per-directory-entry, and relative-path byte limits are enforced and
reported specifically rather than silently truncating discovery.

Component document creation writes deterministic requirements, contract, and
design templates without overwriting any existing document. Validation checks
the first-line version marker, supported version, required headings, heading
order, duplicate headings, and nonempty sections, and reports one-based
diagnostics. Files must be bounded, regular, non-link UTF-8 files.

Read-only project inspection classifies root state as uninitialized, current,
partial, invalid, or unsupported-version. For a current root it discovers
components and reports per-artifact valid/missing/invalid/unsupported states.
Component state precedence observed in the implementation is unsupported,
invalid, missing, stale, blocked, then current. Staleness is derived from
recorded versus current SHA-256 revisions of local requirements/contract/design
and, for descendants, the immediate parent contract. Parent contract paths and
revalidation causes are retained in the queue and exposed by text and JSON
status reports. Status supports document-only, implementation-only, and
unfinished filters.

Tree output is deterministic ASCII text; JSON tree output contains the
component root and component paths/states. VCS inspection is read-only. Git
uses index/ignore answers; Jujutsu uses its saved working-copy snapshot and
reports paths not present in that snapshot without mutating either VCS.

## Queue and durable task lifecycle

`TODOS.yaml` is parsed into typed version-1 task queues with unknown fields
rejected. Validation covers bounded identifiers/text, unique IDs, dependency
existence and acyclicity, legal task kinds, timestamps, revision formats,
revalidation evidence, and state-dependent fields. Serialization emits a
canonical deterministic YAML form.

Ready-task selection returns the first pending task whose complete dependency
chain is completed. Legal transitions are enforced by the finite state machine:
pending to in-progress/blocked, in-progress to pending/blocked/completed, and
blocked to pending/in-progress. Completing requires in-progress; blocking
requires a nonblank reason; reasons are rejected for other targets.

Transitions and task execution use a user-owned lock keyed by canonical project
and component identities. Lock ownership is rechecked before durable writes.
Queue replacements are atomic. Per-task JSONL attempt records contain prepared
and committed transition phases plus agent and verification evidence; a
prepared final phase prevents another transition until explicit recovery.
Component acceptance revalidates the three local documents and the immediate
parent contract, updates revisions and current revalidation state, and
atomically replaces the queue. Unlock can prompt or force-remove the
user-owned lock.

## Agent task execution

Task execution requires a current project/component, complete VCS tracking,
configured sandbox, an approved effective execution policy, and a configured
test policy. Task kinds route to profiles as follows: test and implementation
use the developer profile, security audit uses the security-reviewer profile,
and compliance review uses the architect profile.

The root constructs a bounded context containing the component's local
requirements, contract, design, queue, and implementation record plus the
root contract. A child additionally receives the nearest parent contract at a
fixed read-only context path. The root passes these paths to the external
runner and does not pass peer internals through this context assembly.

Task prompts are selected by detected language (Rust, Python, or generic),
loaded from component-local templates when present, then interpolated with task
fields. The implementation task runs the external agent, records bounded and
redacted combined stdout/stderr evidence and optional token counts, and then
runs the configured verification command. Successful verified implementation
tasks become completed. Agent failure, timeout, output-limit termination, or
failed/blocked verification records evidence and transitions the task to
blocked. Non-implementation tasks become completed after a successful agent
run. `--stream` writes the normalized agent evidence to stdout while it is
captured and logged.

The sandbox request is JSON over a direct subprocess boundary. The component
directory is read-write; declared parent/root context mounts are read-only;
network is denied; the environment is restricted to an allowlist; and stdout
and stderr are captured under a combined byte limit. The runner is identified
by canonical path and content digest, probed for the expected capability, and
terminated as a process group on timeout or output overflow. Host fallback is
not used.

## Standalone prompt behavior

`prompt` requires the explicit `--allow-host-execution` acknowledgement. A
prompt is obtained from one positional value, a bounded regular UTF-8 file
(with `-` meaning stdin), or an editor selected from `VISUAL`, `EDITOR`, or
`vi`; explicit sources conflict with one another. Oversized, linked, malformed,
or otherwise unusable sources fail before provider execution.

The selected role is developer by default, with architect and
security-reviewer aliases also accepted. A per-invocation `--model` overrides
the profile selection; otherwise the profile model/default model is resolved
against its declared model list. A system prompt, when configured for the
selected model, is prepended to the prompt. Command templates are rendered
without a shell, preserving prompt metacharacters as literal argument content.

`--reasoning-effort` is per invocation and accepts `none`, `minimal`, `low`,
`medium`, `high`, `xhigh`, and `max`. When supplied, the selected command
template must expose the `{reasoning_effort}` placeholder; otherwise the
invocation fails. Model and effort selection are performed for each
supervised attempt, including retries.

The prompt supervisor applies the requested idle timeout, optional loop
detection, maximum automatic restarts, and the selected profile's output
bound. Text mode does not add a synthetic completion trailer; observed provider
output is emitted as-is (for example, `/bin/echo` produces exactly `answer\n`).
When stderr is a terminal, a `Prompt: ... Response:` status preface is printed
to stderr before streaming. JSON mode captures one selected result and emits
exactly one stdout object of the form `{"content":"..."}` followed by the
binary's newline. Captured provider bytes are converted with lossy UTF-8
decoding, so invalid bytes are represented by U+FFFD. JSON prompt success
leaves stderr empty in the observed tests.

## Global JSON `agent setup`

`agent setup` is interactive. In non-JSON mode the wizard transcript and
qualification/status text use stdout. Under the global JSON flag, dispatch
constructs a buffered stdin reader and a buffered stderr writer for the entire
wizard. Consequently, prompts, transcript, warnings, and success status are
written to stderr, while stdout is reserved for the single final result object:

`{"status":"success","command":"agent-setup","message":"agent setup wizard complete"}`

The qualification provider is invoked behind the observed child boundary. Its
stdout and stderr are captured by that boundary and are not forwarded into the
JSON setup transcript; the tested JSON run therefore contains neither
`qualification stdout` nor `qualification stderr` in stderr. The final object
is the only setup result written to stdout.

The wizard first chooses either a provider profile collected and qualified now
or a saved standalone agent-runtime profile. It then selects developer,
architect, security-reviewer, or all roles, and saves to project-local
`kvist.toml` or the global user configuration. Existing TOML is parsed and
validated before editing, unrelated values/comments and existing models are
preserved, and the resulting configuration is validated before an atomic
replacement or no-clobber creation.

Normal qualification failure stops setup without persisting the model. The
`--force` path records a warning and permits persistence after a failed
qualification. Cancellation during qualification (including SIGINT in the
observed process test) fails with a cancellation diagnostic and leaves no
configuration written; force does not override cancellation. Qualification is
performed by the wizard itself, so successful setup implicitly acknowledges
the provider's host execution. Selecting a saved profile binds the selected
name and command into each chosen Kvist role; it does not merely record an
unresolved profile reference.

## Authority and approval separation

The root keeps role/profile selection, task lifecycle, context assembly,
verification policy, and sandbox policy in separate root modules. Architect,
developer, and security-reviewer task roles are selected from task kind rather
than from peer implementation details. Root task context exposes local
component artifacts and the root contract, with only the nearest parent
contract added for a child.

Execution approval is user-owned state outside the project/worktree. The
approval material binds configuration and source identity, all role profile
templates and resource/redaction limits, sandbox configuration and runner
digest, root-contract digest, test-policy schema/digest, and canonical project
and worktree identities. A generated user-private secret authenticates the
record. Changed inputs, changed runner content, changed identity, malformed
records, or repository-contained legacy approval records are rejected before
probing or executing the runner. Test evidence and agent evidence are
redacted and bounded before durable logs, blocker text, or optional streaming.

## Conversion, import, and reverse discovery

Conversion reads an existing Rust manifest and implementation root without
rewriting them, then creates validated draft documents, queue, implementation
record, and compliance-review material below `.kvist`; existing metadata is
handled without clobbering. Reverse discovery walks source while skipping
known metadata/build directories, extracts observed symbols/tests/documents, and
generates validated draft artifacts below `.kvist`, also refusing to overwrite
existing intent documents. Import performs a shallow Git clone into an empty
destination, selects an optional component directory, validates existing
standard or converted artifacts, or initializes/converts when artifacts are
absent.

## Uncertainties

The child agent-runtime implementation and provider-specific protocols were
not inspected, so only the root call signatures, captured outputs, and tested
subprocess boundary behavior are recorded here. Provider semantics beyond
command-template rendering, capture, cancellation, and the observed setup
qualification boundary cannot be inferred. This document records observed
implementation behavior and test evidence only; it does not establish that
any external intent or compliance obligation has been satisfied.
