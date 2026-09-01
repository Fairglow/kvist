<!-- kvist-contract-version: 1 -->
# Kvist Engine Contract

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Boundary and ownership

- **Component ID:** `kvist.engine`
- **Contract IDs:** `kvist.cli/v1`, `kvist.artifacts/v1`
- **Owner:** Kvist root component
- **Consumers:** human CLI users, scripts, future editor/web integrations, and
  child component workflows

The component owns Kvist project and component artifacts, CLI dispatch,
validation, discovery, task policy and lifecycle, context selection, sandbox
approval, and durable evidence. It does not own model-provider wire protocols,
generic command supervision, or reusable provider profiles.

## Provided interfaces

The `kvist` executable provides:

- `init [PROJECT_DIR]`
- `convert PROJECT_DIR`
- `import REPO_URL [--branch BRANCH] [--component PATH] [DEST_DIR]`
- `reverse-discover PATH`
- `doctor [PROJECT_DIR]`
- `status [PROJECT_DIR] [--format text|json] [--only-documents]
  [--only-impls] [--unfinished]`
- `tree [PROJECT_DIR]`
- `component new COMPONENT_DIR`
- `component validate COMPONENT_DIR`
- `component accept COMPONENT_DIR`
- `task next COMPONENT_DIR`
- `task transition COMPONENT_DIR TASK_ID STATUS [--reason TEXT]`
- `task run COMPONENT_DIR [TASK_ID] [--stream]`
- `task log COMPONENT_DIR TASK_ID`
- `task approve-policy [PROJECT_DIR]`
- `task unlock COMPONENT_DIR [--force]`
- `prompt` with one explicit prompt source or redirected input, optional
  `--role`, `--model`, and `--reasoning-effort`
- `agent setup [--force]`
- `completions SHELL`

Commands are non-interactive unless their contract explicitly obtains terminal
input. Success is written to standard output. Domain failures are actionable,
written to standard error, and return a nonzero status. Parser help returns
success and parser input errors use the parser's nonzero status.

Plain `prompt` output is the bounded provider content and has no synthetic
completion trailer. Global `--json` suppresses live provider streams and emits
exactly one JSON object with `content`; invalid UTF-8 byte sequences in captured
output are replaced with U+FFFD. Model selection is limited to the configured
models for the selected role. Reasoning effort is a typed
per-invocation value and fails if the selected command lacks an explicit
`{reasoning_effort}` placeholder.
On an interactive terminal, the initial prompt and response label are written
only to standard error; standard output remains provider content.

`agent setup` always runs the generated provider command with the fixed prompt
`Reply with exactly: OK` before role configuration is persisted. The explicit
setup invocation first presents the runtime's bounded provider model catalog
when available and places manual entry behind a final custom choice. It
acknowledges current host authority for those discovery commands and the
qualification command only; it does not authorize a later `prompt`.
Kvist does not parse or reinterpret provider catalog descriptors; it receives
the runtime's validated rendered profile and stores that exact selected command
for the requested roles.
Qualification failure returns without persisting the new model. `--force`
still runs qualification but permits a non-cancellation failure to be persisted
after a visible warning; setup never offers an interactive save-after-failure
bypass.
With global `--json`, setup writes its interactive transcript and status to
standard error, suppresses qualification-command output, and writes exactly one
result object to standard output.
A setup invocation that binds an already saved reusable runtime profile does
not generate or execute a new qualification command.

`init` writes the complete root artifact set only for an uninitialized project,
is a no-op for a current project, converts an existing Rust package into draft
`.kvist/` metadata, and refuses every other state.

`component new` creates all three intent templates after checking every
destination and never overwrites. `component validate` validates all three
documents. `component accept` validates local intent and the immediate parent
contract, records their exact revisions in the queue, and clears attributable
stale evidence without changing task definitions or status.

## Required interfaces

Kvist requires:

- `agent-runtime.library/v1` for prompt acquisition, command rendering,
  profiles, setup, process supervision, and model transports;
- a selected Git or Jujutsu repository for durable-artifact tracking checks;
- an independently installed runner implementing
  `kvist-sandbox-probe-v1` and `kvist-sandbox-request-v1` for task execution;
- operating-system filesystem, process, terminal, and user-state services.

Core inspection does not require any external agent or network interface.

## Data and schemas

All current formats are version 1 and independently versioned:

- `kvist.toml`: strict TOML configuration;
- `VISION.md`: `kvist-vision-version` marker;
- `ARCHITECTURE.md`: `kvist-architecture-version` marker;
- `ROOT_CONTRACT.md`: `kvist-root-contract-version` marker;
- component intent documents: version markers and exact required Markdown
  headings;
- `TODOS.yaml`: strict YAML schema implemented by typed Rust parsing with
  unknown fields rejected;
- `IMPL.md`: version marker and required heading;
- status JSON: `format_version: 1`;
- attempt logs: bounded JSON Lines evidence;
- sandbox request: `protocol_version: 1`.

No external schema file is currently normative. JSON Schema export for
machine-consumed formats is permitted later; the contract must then state the
exact dialect and whether the schema or Rust parser is authoritative.

The queue `component` mapping contains:

- `requirements_revision`
- `contract_revision`
- `design_revision`
- `parent_contract`, null at the root or `{ path: "../CONTRACT.md", revision }`
- `revalidation` with state, timestamps, and attributable causes

Every revision is `sha256:` followed by 64 lowercase hexadecimal digits.

## Behavioral guarantees

Discovery is lexical, bounded, and never follows link-like paths. A directory
is a child component when at least one required adjacent artifact exists; all
intermediate component hierarchy rules apply. Artifact reporting order is
`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`.

Status precedence is unsupported version, invalid, missing, stale, blocked,
then current. It computes exact UTF-8 byte digests without persisting derived
staleness. Local mismatch causes are:

- `component-requirements-revision-changed`
- `component-contract-revision-changed`
- `component-design-revision-changed`
- `parent-contract-revision-changed`

Canonical queue serialization preserves task declaration order, stable field
order, two-space indentation, LF endings, sorted set-like lists, and normalized
timestamps.

A task is ready only when pending, current, and all explicit and transitive
predecessors are completed. Completed task IDs are terminal. Queue writes,
policy approvals, attempt records, and test verification follow the atomicity,
locking, and evidence guarantees stated in the requirements.

`prompt` host execution requires `--allow-host-execution`. It is never
represented as sandboxed. `task run` instead requires the approved runner and
a writable component-only implementation mount plus read-only root and
immediate-parent contract mounts; runner failure cannot fall back to host
execution.

`agent setup` is the explicit acknowledgement for its single generated
qualification command. Kvist adds no filesystem, credential, executable, or
network restriction to that command and does not represent it as sandboxed;
the selected runtime and provider determine which available host authority
they exercise. The acknowledgement does not extend to subsequent provider
runs.

## Errors and failure semantics

Invalid, oversized, unsupported, non-UTF-8, missing, non-regular, or link-like
artifacts fail or are reported according to the read-only command contract.
Kvist does not silently migrate, truncate, repair, overwrite, stage, commit, or
approve user state.

Initialization and multi-artifact creation check conflicts first but are not
multi-file filesystem transactions. A failed operation reports the durable
state left on disk for explicit recovery.

Task execution distinguishes spawn, timeout, output-limit, policy, runner,
agent, verification, and lifecycle failures. Failed verification blocks the
task with bounded redacted evidence. Retained locks or a trailing prepared
record fence further writes until explicit recovery.

## Security and authority

Repository files, schemas, prompts, configuration, paths, environment values,
external commands, and subprocess output are untrusted. Commands are split and
spawned directly by Kvist without shell expansion. A selected provider remains
an external trust boundary and may implement its own subprocess behavior. Host
prompt execution inherits the user's full authority only after explicit
acknowledgment.

Sandbox approval is bound to canonical project/worktree identity, the exact
bounded `ROOT_CONTRACT.md` digest, runner identity, exact policy bytes, and a
user-owned authentication secret outside the repository. Runners inside the
selected worktree are rejected. Task execution allows only the approved
component mount, denied network, explicit environment, and bounded resources.

Secrets must not be persisted in project configuration, task queues, prompts,
logs, schemas, or evidence. Configured literal redactions and output bounds
apply before durable attempt evidence is written.

## Compatibility and verification

This pre-release contract intentionally has no compatibility or migration
commitment. Each artifact keeps an independent version marker so compatibility
can be declared precisely before launch.

Contract behavior is verified by CLI, filesystem, schema, VCS, task lifecycle,
process, and Linux sandbox integration tests. `IMPL.md` must be derived
independently from observed code. Compliance requires a separate source-blind
comparison with `REQUIREMENTS.md`, this contract, and `DESIGN.md`.
