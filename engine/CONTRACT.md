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

- `shell [PROJECT_DIR]`
- `init [PROJECT_DIR]`
- `convert PROJECT_DIR`
- `import REPO_URL [--branch BRANCH] [--component PATH] [DEST_DIR]`
- `reverse-discover PATH`
- `doctor [PROJECT_DIR]`
- `status [PROJECT_DIR] [--format text|json|overview] [--only-documents]
[--only-impls] [--unfinished]`
- `overview [PROJECT_DIR]`
- `tree [PROJECT_DIR]`
- `component new COMPONENT_DIR`
- `component validate COMPONENT_DIR`
- `component accept COMPONENT_DIR [--commit] [--message TEXT]`
- `task next COMPONENT_DIR`
- `task transition COMPONENT_DIR TASK_ID STATUS [--reason TEXT]`
- `task run COMPONENT_DIR [TASK_ID] [--stream]`
- `task log COMPONENT_DIR TASK_ID`
- `task replay SESSION_JSONL [--max-turns N]`
- `task approve-policy [PROJECT_DIR]`
- `task unlock COMPONENT_DIR [--force]`
- `task recover COMPONENT_DIR TASK_ID ATTEMPT_ID --disposition execution-did-not-start`
- `task finalize COMPONENT_DIR TASK_ID ATTEMPT_ID DISPOSITION [--commit]
[--reason TEXT]` with DISPOSITION `accept` or `block`
- `prompt` with one explicit prompt source or redirected input, optional
  `--role`, `--model`, and `--reasoning-effort`
- `agent setup [--force]`
- `agent profile add [--force]`, `agent profile list`, and
  `agent profile remove [MODEL_NAME] [--all] [--global]`
- `agent role list`, `agent role set ROLE MODEL_NAME [--effort EFFORT]
[--global]`, and `agent role clear [ROLE] [--all] [--global]`
- `agent check [--global]`
- `agent list` and `agent remove [MODEL_NAME] [--all] [--global]`
- `vcs commit-accepted ACCEPTANCE_ID`
- `completions SHELL`

`task run` without TASK_ID suggests the first ready task of the component and
executes it only after an explicit interactive confirmation (a bare ENTER
accepts; a refusal changes no state). When no task is ready, or when standard
input is not an interactive terminal, it fails with an actionable diagnostic
and changes no durable state. `vcs commit-accepted` retries or performs the
isolated index commit for an already accepted set.

Commands are non-interactive unless their contract explicitly obtains terminal
input; `prompt` (with a terminal), `shell`, the agent setup/configuration flows,
and `task run` without TASK_ID (which confirms the suggested task) are the
interactive exceptions. `prompt` obtains a terminal to open the standalone
`agent-runner` shell; with no interactive terminal it falls back to the one-shot
host path, which still requires `--allow-host-execution`.
Success is written to standard output. Domain failures are actionable, written
to standard error, and return a nonzero status. Parser help returns success and
parser input errors use the parser's nonzero status.

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

`agent check` live-verifies the model profiles configured in the selected
scope (the project-local `kvist.toml` by default, the user configuration with
`--global`). It lists the profiles and obtains an explicit interactive
acknowledgement before executing any profile command; that acknowledgement is
also a cancellation point. A refusal, cancellation input, exhausted standard
input, or interruption ends the check without executing a test command and
without changing configuration. Each profile is tested on the host with the
runtime's bounded verification and the fixed prompt `Reply with exactly: OK`.
For each failing profile, the check offers removal (the standard profile
removal semantics, including role-binding clearance) or ignore for now
(configuration unchanged), and it remains cancellable at every prompt. With
global `--json`, the transcript goes to standard error and standard output
carries exactly one result object, as in the other interactive agent flows.
`agent role list` presents only the predefined roles (developer, architect,
security-reviewer) and their assigned model profiles, and never presents model
profile names as roles.

`init` writes the complete root artifact set only for an uninitialized project,
is a no-op for a current project, converts an existing Rust package into draft
`.kvist/` metadata, and refuses every other state.

`component new` creates all three intent templates after checking every
destination and never overwrites. `component validate` validates all three
documents. `component accept` validates local intent and the immediate parent
contract, records their exact revisions in the queue, and clears attributable
stale evidence without changing task definitions or status.

The pre-spawn dogfooding recovery surface provides explicit attempt recovery
only when authenticated evidence proves the runner descriptor was not launched
and no write scope was exposed. `task finalize` records an explicit human
disposition for a completed attempt and optionally creates the acceptance
commit. Acceptance operations support an explicit `--commit` option, and
`vcs commit-accepted ACCEPTANCE_ID` retries a commit for an already accepted
set.

`shell [PROJECT_DIR]` starts the interactive workspace shell and requires an
interactive standard input; it fails with an actionable diagnostic otherwise
and is not supported under global `--json`. Lines are parsed against the same
command surface as the CLI. Static completion is derived from that surface and
dynamic completion covers component paths, task IDs, attempt IDs, model
profile names, and the active VCS branch; dynamic sets are refreshed after
every executed command and each source degrades independently without aborting
the session. The shell builtins are `cd [COMPONENT_DIR]`,
`tasks [COMPONENT_DIR] [--status STATUS]`, `run [COMPONENT_DIR] [TASK_ID]`,
`help`, `last [COUNT]`, `history [COUNT]`, `journal`, `locks [clean]`, and
`exit`/`quit`. `cd` remembers a default component for the builtins and for
completion ordering; when `tasks` or `run` names a component different from the
current focus, the shell prints a hint to switch and leaves the focus
unchanged. `run` without a task ID suggests the first ready task of the
selected component and runs it only after an explicit confirmation (a bare
ENTER accepts; a refusal changes no state). `prompt TASK_ID` opens the external
editor seeded with
the task context and submits only after explicit confirmation.

Shell presentation degrades to plain text when `NO_COLOR` is set (any value),
`CLICOLOR=0`, `TERM=dumb`, or stdout is not a terminal; `CLICOLOR_FORCE` (any
value but `0`) forces styling even when stdout is not a terminal. Styled
rendering keeps table columns aligned on visible width. Titled boxes (welcome
banner, streaming stages) fit the terminal width: at least 40 columns, never
wider than the terminal width minus a margin or 100 columns, whichever is
smaller, and a box extends rather than truncating content that is wider than
the cap. Output is paged only when it would scroll past the terminal height
minus the prompt row (a 15-line fallback when the height is unknown), and
`KVIST_NO_PAGER` disables paging. The prompt reports the last command's exit
state with a failure marker until a command succeeds; Ctrl+L clears the screen
and redraws the prompt. Completion describes dynamic values (task status and
title, a next-ready marker in run contexts, the current component), and a
complete `--` token ends flag parsing and switches to positional completion.

The shell persists an append-only JSONL session journal at
`.kvist/session.log` and a line-based editor history at `.kvist/history`; both
are local uninspected state and MUST NOT be treated as compliance evidence.
Command failures, prompt-editor cancellations, and transient terminal read
failures do not terminate the session; repeated terminal read failures exit
with an actionable diagnostic. SIGINT during a running command requests
cancellation, terminates the supervised process group, and returns to the
prompt with durable task state left for explicit finalize or recovery.
Streaming task execution relays sandbox output to the terminal while it is
produced and retains the full bounded log as evidence. The prompt and status
line report the active VCS branch, the default component, the configured
sandbox backend, the default model, and task-lock counts that distinguish
live locks from stale ones. Destructive operations (`task unlock --force`,
`agent remove --all`, `component accept --commit`, `vcs commit-accepted`)
require an explicit in-shell confirmation.

## Required interfaces

Kvist requires:

- `agent-runtime.library/v1` for prompt acquisition, command rendering,
  profiles, setup, process supervision, and model transports;
- a selected Git or Jujutsu repository for durable-artifact tracking checks;
- an independently installed runner implementing
  `kvist-sandbox-probe-v1` and `kvist-sandbox-request-v1` for task execution,
  together with an approval-bound enforcement backend executable configured as
  an absolute `[sandbox] backend` path;
- canonical supported package-source services for explicitly approved
  dependency acquisition;
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
Any fenced task or unresolved authenticated attempt fences every queue writer
in that component until its exact recovery finishes; read-only inspection
remains available. A completed authenticated recovery chain remains terminal
historical evidence after the recovered queue digest has been verified, so
later legal queue updates do not re-fence it.

`prompt` host execution requires `--allow-host-execution`. It is never
represented as sandboxed. `task run` now emits the redefined version-one
sandbox request: a closed, typed value with an explicit phase, argument vector,
working directory, environment, network capability, resource limits, and
approval-bound grants. Before serialization the engine resolves `argv[0]` to a
canonical, non-symlink executable (a bare name only against an explicitly
present request `PATH`, with no ambient host fallback) and replaces `argv[0]`
with that canonical path; every grant source is canonical; the request policy
identity is the authenticated execution-approval digest; the toolchain identity
is the content digest of the exact resolved `argv[0]` executable, exposed as a
narrow read-only toolchain grant for that executable (a full immutable
toolchain-set approval is later work); and resource limits use bounded defaults
and options that never exceed fixed safe maxima, failing closed on overflow
rather than saturating. An authoring request grants read-write access to the whole current component
directory minus the excluded paths — the five Kvist intent and record documents
(`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`), the
component's `.kvist` state and evidence, its `.git`, and any sub-component
directory — and mounts each excluded document as read-only context at a disjoint
destination. No writable ancestor may expose an excluded document, the
component's `.kvist` or `.git`, or a sub-component directory. The agent may read
the whole current project and approved dependency source within bounded per-file,
per-turn, and directory-depth limits. When an exclusion cannot be correctly
identified, or when no writable path remains, `task run` fails closed rather than
granting the component root. It emits none of the
retired `program`, `arguments`, `mounts`, or `context_files` fields. The
independently installed runner strictly parses and validates this request and
rejects the retired shape, but Bubblewrap enforcement is not yet integrated, so
a valid request fails closed and `task run` still cannot execute Kvist's own
root workspace safely. There is no fallback to the legacy request or host
execution. The engine builds fallible typed mediated-acquisition and offline-verification
plans with distinct canonical UTF-8 host sources and fixed sandbox
destinations. Acquisition is exactly `cargo fetch` using a real writable
attempt-local `CARGO_HOME`, an isolated writable lockfile workspace, and
separate scratch; it records lockfile before/after content identities for an
immutable-generation promotion result. Verification is exactly
`cargo test --locked` with denied network, `CARGO_NET_OFFLINE=true`, an
approved generation mounted read-only at `CARGO_HOME`, and separate target
scratch. Strict `[sandbox.acquisition]` configuration is approval-bound. The
runner validates these values but live `task run` acquisition wiring, OS mount,
process, DNS/address-pinning, redirect, and network enforcement, and final
project-cache generation selection remain later work.

The target supervised tier requires one explicit task and produces a pending
human disposition after agent and verification results are recorded. A
separate finalization action binds acceptance or blocking to the exact attempt
and scoped changes. Automatic selection, retry, and completion are not part of
that tier.

An acceptance with `--commit` creates one local commit from a canonical set of
accepted paths and engine-written state. It leaves unrelated staged, unstaged,
and untracked paths unchanged and refuses any overlap or concurrent head
change. Commit automation does not push or amend. A commit failure does not
reverse acceptance; it returns the acceptance ID and leaves a retryable
versioned journal for `vcs commit-accepted`.

Dependency acquisition permits Cargo network access only in its distinct phase
and only to exact configured supported sources. It uses a real attempt-local
writable `CARGO_HOME`, an isolated writable lockfile workspace, and separate
target scratch, and never mounts the user's Cargo home. `cargo fetch` may
update that workspace's lockfile. Authoring and verification remain
network-denied; verification uses a selected approved immutable Cargo-home
generation read-only with `cargo test --locked` and offline true.

`agent setup` is the explicit acknowledgement for its single generated
qualification command. Kvist adds no filesystem, credential, executable, or
network restriction to that command and does not represent it as sandboxed;
the selected runtime and provider determine which available host authority
they exercise. The acknowledgement does not extend to subsequent provider
runs.

This section describes the target multi-turn agent tier. The current tier
performs a single, write-only authoring turn; the target tier runs a multi-turn
loop that persists the agent's intermediate reasoning in a per-run trajectory
and persists only the final brokered effect, preserving Kvist's durable,
inspectable state rather than in-chat context. When `task run` executes an
external agent, the engine performs each model turn on the host, outside the
effect sandbox. The selected model command must target a numeric loopback model
gateway; any other command is refused before any transport work with
`AgentCommandNotModelGateway`, so no agent command ever runs on the host outside
the effect sandbox. The engine liveness-probes the gateway with a bounded TCP
connect to the resolved endpoint before issuing the model turn. The probe issues
no HTTP request, so it never loads, selects, or shifts a model slot; a gateway
that does not accept a connection fails fast with `LocalModelGatewayUnreachable`
and an actionable "ensure the model server is running and listening on
`{endpoint}" hint.

The loop advertises the closed authoring tool set (`read_file`, `write_file`,
`edit_file`, `request_dependency`, and `propose_decision`). The broker reduces
each turn's untrusted tool intents to capability-bound effects under a
deny-by-default policy; a dropped intent fails that turn without ending the run.
`read_file` is honored within the read scope and logged to the trajectory but
produces no persisted effect. `write_file` and `edit_file` are reduced to the
writable component scope and applied by the engine itself inside the effect
sandbox against a read-only staged-intent mount; the host never writes component
state for an effect, and only the final brokered effect of a run is persisted.
`request_dependency` is evaluated by the broker's dependency-origin policy rather
than executed inline: a request whose origin is an exact, pinned registry
revision or a public VCS origin with an exact pinned revision (never a private,
link-local, loopback, or unverified production address, and never a wildcard or
unpinned range) is recorded and accepted so the agent continues without
interruption and acquires the revision through the build/verification step; a
request outside that policy is surfaced as a decision. `propose_decision`
records an impactful, uncovered decision for the user as a redacted proposal under
the component state directory and ends the run by placing the component in an
awaiting-decision state; the run harness transitions the task to that state and
never runs verification or jumps to completed. A decision never writes a protected
intent document, and it never blocks on a trivial matter. A run succeeds only when
the agent reports completion, no decision worthy of intervention remains surfaced,
and every authorized effect applied. Once such a decision is accepted, `TODOS.yaml` MUST gain the tasks needed
to implement it and `IMPL.md` MUST become stale; the component then remains in the
awaiting-decision state until an updated advisory review is performed and accepted,
after which `IMPL.md` is rederived from the code.

The model phase runs under one shared wall-clock budget equal to the configured
profile timeout, covering the liveness probe, every model and brokered turn, every read and effect, and every
retry backoff; the per-attempt transport deadline is the remaining budget. When
the gateway accepts but a turn still hits a transient availability failure — a
socket connection refused, timed out, interrupted, or reset error, a
slot-allocation timeout, or an overall transport timeout — the engine retries it
up to three attempts with a short fixed backoff, then surfaces the error if it
never succeeds. Response-level failures (a non-success HTTP status, a malformed
or oversized response, or cancellation) are never retried, because they
indicate a real answer rather than an unavailable gateway. When streaming output
is requested, text deltas are relayed to standard output with the run's
redaction values applied; a streamed attempt that has already emitted text is
never retried. None of this retries, weakens, or changes sandbox authorization,
effect grants, policy approval, or output redaction.

## Errors and failure semantics

Invalid, oversized, unsupported, non-UTF-8, missing, non-regular, or link-like
artifacts fail or are reported according to the read-only command contract.
Kvist does not silently migrate, truncate, repair, overwrite, stage, commit, or
approve user state.

Initialization and multi-artifact creation check conflicts first but are not
multi-file filesystem transactions. A failed operation reports the durable
state left on disk for explicit recovery.

Task execution distinguishes spawn, timeout, output-limit, policy, runner,
agent, verification, gateway-unreachable, and lifecycle failures. A selected
model command that does not target a numeric loopback gateway fails the turn
with `AgentCommandNotModelGateway` before any transport work. An external model
turn to a loopback gateway is liveness-probed first and runs under one shared
wall-clock budget; only its transient availability failures are retried a
bounded number of times within that budget before the gateway-unreachable
failure is surfaced, and response-level failures are never retried. A turn whose
intents were dropped, or whose authorized effects did not all apply, fails
closed with the bounded redacted reason recorded in the log. Failed
verification blocks the task with bounded redacted evidence. Retained locks or
a trailing prepared
record fence further writes until explicit recovery. Target recovery can
reconcile only digest-proven state; uncertain source effects remain fenced for
human disposition.
`task unlock --force` bypasses only the confirmation prompt: it refuses a
demonstrably live, changed, replaced, malformed, or fenced lock state.

Commit recovery operates only on an already accepted canonical set. A changed
accepted path, expected head, signing policy, backend identity, or repository
selection prevents commit creation without altering the accepted files or
acceptance record.

## Security and authority

Repository files, schemas, prompts, configuration, paths, environment values,
external commands, and subprocess output are untrusted. Commands are split and
spawned directly by Kvist without shell expansion. A selected provider remains
an external trust boundary and may implement its own subprocess behavior. Host
prompt execution inherits the user's full authority only after explicit
acknowledgment.

Sandbox approval is bound to canonical project/worktree identity, the exact
bounded `ROOT_CONTRACT.md` digest, runner identity, and the canonical path and
content digest of the approval-bound Bubblewrap enforcement backend, exact
policy bytes, typed grants, supported package sources, command and toolchain
identity, and a user-owned authentication secret outside the repository. The
availability probe must report the approved runner digest and the exact backend
kind, path, and digest, and the backend bytes are rehashed and revalidated
immediately before execution. Runners and enforcement backends inside the
selected worktree or project root are rejected. Every execution phase receives
only its
approved paths, network capability, environment, and bounded resources.

Model transport and credentials are not dependency-acquisition capabilities.
Initial task agents are local. A future remote agent uses host-owned model
transport and credential references plus typed tool requests; mounting ambient
provider state into the effect sandbox is outside this contract.

Git commit creation uses an isolated index and verifies the resulting tree
before an atomic expected-head branch update. Repository-controlled hooks are
not invoked by default. Signing is explicit and a required signature cannot
degrade to unsigned output. Unsupported write-capable VCS backends, including
initial Jujutsu support, fail before changing repository state.

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
