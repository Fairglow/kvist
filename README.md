# Kvist

[![Rust](https://github.com/Fairglow/kvist/actions/workflows/rust.yml/badge.svg)](https://github.com/Fairglow/kvist/actions/workflows/rust.yml)
[![License: AGPL v3+](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE)

Kvist is a filesystem-native, architecture-driven tool for human-directed AI
development. Its durable hierarchy is `VISION.md` -> `ARCHITECTURE.md` ->
per-component `REQUIREMENTS.md` + `CONTRACT.md` + `DESIGN.md` ->
`TODOS.yaml` -> `IMPL.md`. Its current interface is command-line based;
graphical and editor integrations are planned without changing that model.
Kvist currently builds and runs on Linux only. macOS and Windows support is
intentionally deferred until native maintainers and test environments can
validate their execution boundaries.
The product vision is defined in [`VISION.md`](VISION.md), the approved system
structure in [`ARCHITECTURE.md`](ARCHITECTURE.md), detailed strategy in
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md),
and the standards posture in [`docs/standards.md`](docs/standards.md).

## Why Kvist

AI coding tools are increasingly capable at producing code. The harder problem
is keeping product intent, architecture, authority, and evidence coherent as
those tools act. Important decisions can otherwise remain in chat history,
context can cross component boundaries without review, and successful
execution can be mistaken for correctness.

Kvist provides a durable control layer around AI-assisted development. It does
not try to replace coding agents or model frameworks. It gives them explicit,
version-controlled work to perform; limits the context and authority they
receive; and keeps intended behavior separate from independently observed
implementation evidence.

The human remains the architect and final arbiter. Agents may propose,
implement, document, and review, but an agent does not approve product intent
or certify its own work. When evidence and intent disagree, Kvist preserves the
discrepancy for a person to resolve rather than silently choosing a side.

This makes Kvist most relevant to teams that value architectural continuity,
inspectable state, bounded execution, and reviewable evidence more than
unrestricted autonomy. See [Why Kvist](docs/why-kvist.md) for a grounded
comparison with coding agents, specification kits, agent frameworks,
sandboxes, and governance tools.

## CLI contract

| Command                                            | Contract                                                                                     |
| -------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `kvist init [PROJECT_DIR]`                         | Initialize the Kvist root artifacts in `PROJECT_DIR`, defaulting to the current directory.   |
| `kvist convert <PROJECT_DIR>`                      | Generate no-clobber draft onboarding artifacts for an existing Rust project.                 |
| `kvist doctor [PROJECT_DIR]`                       | Read-only inspection of the root artifact state and recovery guidance.                       |
| `kvist status [PROJECT_DIR] [--format text\|json] [--only-documents]` | Read-only versioned inspection, optionally limited to document state.        |
| `kvist tree [PROJECT_DIR]`                         | Render the component hierarchy rooted at `PROJECT_DIR`, defaulting to the current directory. |
| `kvist component new <COMPONENT_DIR>`              | Create no-clobber `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md` templates.               |
| `kvist component validate <COMPONENT_DIR>`         | Validate all three component intent documents without rewriting them.                        |
| `kvist component accept <COMPONENT_DIR>`           | Structurally validate and record local intent and immediate-parent contract revisions.        |
| `kvist task next <COMPONENT_DIR>`                  | Select the first ready task without changing durable state.                                  |
| `kvist task transition <COMPONENT_DIR> ...`        | Persist one legal task-state transition with append-only attempt evidence.                   |
| `kvist task run <COMPONENT_DIR> [TASK_ID]`         | Run the configured external agent for one ready task; see the execution boundary below.      |
| `kvist task log <COMPONENT_DIR> <TASK_ID>`         | Print the most recent bounded, redacted agent log for a task.                                |
| `kvist task approve-policy [PROJECT_DIR]`          | Record approval of the complete effective execution policy.                                  |
| `kvist prompt [PROMPT] --allow-host-execution`     | Run a prompt with optional role/model/reasoning selection; text output is provider content only. |
| `kvist agent setup [--force]`                      | Collect, qualify, and bind a reusable profile; force is required to retain failed qualification. |

Delivery is organized into phases. The completed, current, and planned phase
scope, context, and acceptance criteria are maintained in
[`TODO.md`](TODO.md). The Phase 1 foundation and Phase 2
queue, status, task-transition, agent-runner, and test-verification mechanics
are implemented. `task run` requires an approved external sandbox runner and
enforces the documented timeout, output, redaction, and lifecycle-lock bounds.

## Configuration and platform policy

Core project configuration is read from `kvist.toml` in the selected project
root; Kvist does not search parent directories for a project. Agent
configuration is resolved in this order: `[agent]` in `kvist.toml`,
`.kvist/config.toml` in the project, the per-user configuration path, then the
system configuration path, then a built-in default. This behavior is limited
to agent settings; it does not select a project root. The resolver records the
selected source identity and SHA-256 digest. It never creates a user
configuration as a side effect.

### Per-role model selection

Each `architect` and `developer` profile can choose a named entry from its
`models` list:

```toml
[agent.profiles.developer]
model = "local"
default_model = "default"
models = [
  { name = "default", command = "example-agent --message '{prompt}' {context_files}" },
  { name = "local", command = "local-agent --prompt '{prompt}' {context_files}", system_prompt = "Follow the component contract." },
]
```

`model` takes precedence over `default_model`. If neither names a concrete
entry, the aliases `default` and `default-model` select the first listed
model. Names are case-sensitive. An unknown selected name fails before the
agent program is invoked and lists the available names.

Agent settings come from one selected source; they are not merged across the
paths above. `[agent]` in the project `kvist.toml` wins when present. Otherwise
the first existing project-local, user, or system agent configuration is used,
then the built-in defaults. Omitted fields in the selected configuration retain
their built-in values. When present, `command_template` updates the built-in
`default` model when that model list has not been replaced.

On Linux, the user path is
`$XDG_CONFIG_HOME/kvist/config.toml`, falling back to
`$HOME/.config/kvist/config.toml`, and the system path is
`/etc/kvist/config.toml`.

Model commands use a deliberately limited shell-free argument template.
Single and double quotes group arguments and may quote executable paths, but
no shell is started. For normal models, `{prompt}` and
`{target_directory}` are substituted in an argument, `{prompt_json}` emits a
complete escaped JSON string value, and an argument containing
`{context_files}` is emitted once for each declared context path. Shell
operators, redirections, and pipelines are not supported.
`system_prompt`, when nonempty, is prefixed to the task prompt with a blank
line; it is not passed as a provider-specific system-message option.

Selecting the model whose **entry name** is `none` takes a separate raw path:
its command is only split into a program and whitespace-delimited arguments.
Kvist does not interpolate placeholders or add `system_prompt` in this mode.
It still executes an external program through the approved sandbox runner;
`none` is not a no-op, a manual-execution mode, or a way to bypass approval.

Model selection is part of the effective agent profile covered by
`kvist task approve-policy`. Changing the selected source, profile, model
list, command, or prompt requires a fresh approval before `task run`.

### Custom prompts and model setup

`kvist prompt` accepts prompt text as a positional argument, from a regular
UTF-8 file with `--file PATH`, or from standard input:

```bash
kvist prompt --allow-host-execution "Review this component contract"
kvist prompt --allow-host-execution --file review-prompt.md
printf '%s\n' "Review this component contract" |
  kvist prompt --allow-host-execution
```

Use `--file -` to select standard input explicitly. Use `--editor` to author a
multiline prompt with `$VISUAL`, `$EDITOR`, or `vi`. If
no source is supplied, redirected standard input is read automatically; at an
interactive terminal Kvist offers to open the editor. These input modes are
mutually exclusive, limited to 1 MiB, and must produce nonblank UTF-8 text.
The acknowledgement is mandatory because this custom prompt path runs the
configured provider with the invoking user's host permissions. Idle and loop
retries append a warning that an earlier attempt may already have changed
files or external systems; the warning does not roll those effects back.

Prompt acquisition, command rendering, and host-process supervision are
provided by the independently usable `agent-runtime` workspace package:

```bash
cargo run --manifest-path engine/Cargo.toml --locked -p agent-runtime --bin agent-run -- run \
  --allow-host-execution \
  --command "local-agent --prompt '{prompt}' {context_files}" \
  --file review-prompt.md
```

Create a reusable provider profile interactively, then use it by name:

```bash
cargo run --manifest-path engine/Cargo.toml --locked -p agent-runtime --bin agent-run -- setup
cargo run --manifest-path engine/Cargo.toml --locked -p agent-runtime --bin agent-run -- run \
  --allow-host-execution \
  --profile local-coder \
  "Review this change"
```

Send a text-only request through the Rig-backed local Ollama or llama-server
transport:

```bash
cargo run --manifest-path engine/Cargo.toml --locked -p agent-runtime --bin agent-run -- model \
  --provider ollama \
  --endpoint http://127.0.0.1:11434 \
  --model qwen3-coder \
  --stream \
  --file prompt.md

cargo run --manifest-path engine/Cargo.toml --locked -p agent-runtime --bin agent-run -- model \
  --provider llama-server \
  --endpoint http://127.0.0.1:9931 \
  --model Qwen3.8-9B-Q4_K_M \
  "Summarize the supplied prompt"
```

The exactly pinned Rig adapter is enabled and selected by default on the
`rig-integration` branch. It accepts loopback HTTP only, has no proxy or
credential support, and exposes no tools through this command. The library API
additionally supports canonical tool descriptors and returns tool calls as
untrusted `ToolIntent` values; it never executes them.

```bash
cargo run --manifest-path engine/Cargo.toml --locked -p agent-runtime --bin agent-run -- model \
  --provider ollama \
  --endpoint http://127.0.0.1:11434 \
  --model qwen3-coder \
  --output-schema '{"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false}' \
  --file prompt.md
```

Provider-native schema enforcement is a generation constraint; callers still
parse and validate the returned content. The direct adapter remains an
explicit conformance and fallback path via `--transport direct`. Kvist never
automatically replays a failed Rig request through it.

List provider-advertised model IDs without running inference:

```bash
cargo run --manifest-path engine/Cargo.toml --locked -p agent-runtime --bin agent-run -- models \
  --provider ollama
cargo run --manifest-path engine/Cargo.toml --locked -p agent-runtime --bin agent-run -- models \
  --provider copilot \
  --allow-host-discovery
cargo run --manifest-path engine/Cargo.toml --locked -p agent-runtime --bin agent-run -- models \
  --provider gemini \
  --allow-host-discovery \
  --json
```

Ollama and llama-server use bounded numeric-loopback HTTP catalogs at
`/api/tags` and `/v1/models`. Copilot and Gemini use their account-aware ACP
catalogs. ACP discovery starts the selected CLI with the caller's host
authority but sends no model prompt, so standalone use requires
`--allow-host-discovery`. Text output is one model ID per line; `--json`
returns the ordered descriptors and advertised current model.
`llama-cli` and custom wrappers have no provider inventory; `models` reports
that capability as unsupported and their setup paths remain manual.

During `agent-run setup`, each catalog-capable provider presents numbered
choices and uses its advertised current model, or first model, as the default.
Manual entry is available only through the final `Other model ID...` choice.
If discovery fails visibly, setup offers the provider fallback (`auto` for
Copilot and Gemini, `default` for llama-server, or `llama3.1:8b` for Ollama)
and the custom choice. The reusable profile name is selected separately from
the provider model ID.

Standalone profiles are stored at
`$XDG_CONFIG_HOME/agent-runtime/config.toml`, falling back to
`$HOME/.config/agent-runtime/config.toml`. Version 1 stores generic profile
names, provider kinds, and command templates:

```toml
schema_version = 1

[[profiles]]
name = "local-coder"
provider = "ollama"
command = "ollama run qwen3-coder '{prompt}'"
```

The library crate is named `agent_runtime`. Its outcomes and constraints,
consumer boundary, and private realization live in
[`engine/agent_runtime/REQUIREMENTS.md`](engine/agent_runtime/REQUIREMENTS.md),
[`engine/agent_runtime/CONTRACT.md`](engine/agent_runtime/CONTRACT.md), and
[`engine/agent_runtime/DESIGN.md`](engine/agent_runtime/DESIGN.md).
The layered runtime decision is documented in
[`docs/agent-runtime/architecture.md`](docs/agent-runtime/architecture.md),
the current and planned runtime choices in
[`docs/agent-runtime/runtime-selection.md`](docs/agent-runtime/runtime-selection.md),
the direct transport is available for local testing, and the Rig 0.42.0
adoption decision and contained optional adapter gates are in
[`docs/agent-runtime/rig-evaluation.md`](docs/agent-runtime/rig-evaluation.md).
The current host mode is a reliability aid, not a sandbox. `fakeroot`, retry
notices, and backups likewise do not restrict an agent's authority.

Setup probes the conventional `llama-cli`, `gemini`, or `copilot` executable
with `--version`. If that fails, it asks for a direct executable or compatible
wrapper while retaining the selected provider. It automatically qualifies the
exact generated command with the fixed prompt `Reply with exactly: OK` before
saving; `--force` is required to retain a failed qualification. Maintained
Linux templates are:

```text
llama-cli --model /path/model.gguf --prompt '{prompt}' --single-turn \
  --simple-io --no-display-prompt --predict 4096
gemini --prompt '{prompt}' --output-format text --approval-mode yolo \
  --skip-trust --model MODEL
copilot --prompt '{prompt}' --silent --allow-all-tools --no-ask-user \
  --model MODEL
```

The Gemini `yolo` and Copilot `--allow-all-tools` options make noninteractive
agent work possible; they are not sandbox controls. For a response-only Gemini
check, use `--approval-mode plan`. For a response-only Copilot check, restrict
the available tool set, for example with `--available-tools=`, while retaining
`--allow-all-tools` to satisfy noninteractive permission handling.

`llama-cli` itself is an inference process, not a coding agent: it cannot read
declared context files, edit a workspace, or invoke tools. A wrapper may prepare
its runtime environment, but must preserve argument boundaries:

```sh
#!/bin/sh
# Set provider-specific environment here.
exec /path/to/llama-cli "$@"
```

Do not forward with unquoted `$*`; that splits a prompt containing spaces into
multiple CLI arguments. The model path remains part of the generated
llama-cli profile, unless the editable wrapper intentionally owns model
selection. Explicit wrapper and GGUF paths are stored as canonical absolute
paths so later working-directory choices cannot change what is executed.

`kvist agent setup` can call the same reusable provider-profile collection or
load an existing standalone profile. It then asks which Kvist roles should use
the profile and where the Kvist binding should be stored. The exact profile
name and command are copied into Kvist configuration rather than dynamically
referenced, so execution approval remains bound to reviewed command bytes.
Existing project-local and user-global Kvist TOML is updated atomically:
unrelated settings, models, and comments are retained.

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
directory and writable component mount, denied network, allowed environment,
and read-only context mounts. `ROOT_CONTRACT.md` is materialized at
`/workspace/context/ROOT_CONTRACT.md`; a child component's nearest ancestor
component contract is materialized at
`/workspace/context/PARENT_CONTRACT.md`. The runner must enforce those values
and proxy its sandboxed child result. Missing configuration, an unavailable
runner, or a failed probe refuses `task run` before any task transition; Kvist
never falls back to host execution. A version-1 sandbox cannot run a
`test_policy` with `working_directory = "project"`.

Before `task run` probes a runner or changes a task, run
`kvist task approve-policy`. It atomically writes a versioned, deterministic,
non-secret record in
user-owned state outside the repository. A persistent cryptographically random
user secret authenticates that record and binds it to canonical project and
worktree identities, so a repository cannot forge approval by replacing its
configuration and hashes. Repository-contained and legacy approval records are
rejected. The record covers both effective agent templates, token limits,
timeouts, combined-output caps, and redaction policies,
the selected agent-config source path and digest, the exact bounded
`ROOT_CONTRACT.md` digest, parsed sandbox configuration, canonical runner path
and digest, test policy (including absence), and relevant schema/protocol
versions. Any missing, malformed, or changed input causes `task run` to refuse
without probing, host fallback, or task mutation. On Linux, each probe and
request launches a private descriptor-bound copy of freshly verified runner
bytes, so replacement after validation cannot alter what executes. Platforms
without a descriptor-bound launch mechanism fail closed.

Kvist targets current stable Rust on Linux. Non-Linux builds fail explicitly.
Portable contracts remain free of Linux-specific policy assumptions, while
platform process and isolation implementations are kept behind replaceable
boundaries for possible future restoration.

## Toolchain and quality gates

Kvist's MSRV is Rust **1.94**. Edition 2024 itself is available from Rust 1.85,
but 1.94 is the upstream-tested compiler for the exactly pinned Rig 0.42.0
transport dependency. CI tests Rust 1.94 and current stable on Linux.
`engine/Cargo.lock` is committed and every CI build/test command uses
`--locked`. Kvist's own repository dogfoods the component model: the root Rust
workspace and package live in `engine/`, with `agent_runtime/` and
`sandbox_runner/` as complete child component boundaries. The
[`sandbox_runner` intent](engine/sandbox_runner/REQUIREMENTS.md) is present, but
its package remains an explicit fail-closed scaffold; the Bubblewrap protocol
is assigned to later tasks. Newly initialized projects retain their configured
component root and currently default to `src/`.

The portable default quality gate uses only Cargo:

```bash
cargo fmt --manifest-path engine/Cargo.toml --check
cargo clippy --manifest-path engine/Cargo.toml --locked --workspace --all-targets --all-features -- -D warnings
cargo test --manifest-path engine/Cargo.toml --locked --workspace --all-features
cargo build --manifest-path engine/Cargo.toml --locked --workspace --release --all-features
```

`just` is an optional wrapper for these commands; `just all` runs the same
gate, and `just msrv` runs all-feature tests with Rust 1.94.0. Dependency updates must be
small, intentional changes with a stated purpose, lockfile update, and passing
MSRV and stable CI.

### Filesystem threat model

Read-only discovery supports ordinary local checkouts and malformed or
untrusted **static** workspaces: it bounds reads, reports malformed layouts,
and refuses link-like paths it directly inspects. It is not a sandbox, does not
establish canonical containment, and makes no guarantee if another process
changes the filesystem between metadata checks and use (TOCTOU). Do not treat
`init`, `doctor`, `status`, `tree`, or component-document validation as
authorization to run repository code. The execution boundary still requires a
separately documented trusted-workspace policy and explicit execution
authorization.

On Linux, symbolic links are link-like. Link-like project, configuration, and
component paths are refused, while a link-like required artifact is invalid.
Discovery refuses link-like non-artifact descendants rather than following or
silently traversing them. These direct checks reduce accidental traversal only;
they do not remove the TOCTOU limitation above.

## Root artifact templates

`kvist init` creates the following deterministic, UTF-8 templates.

| Path                  | Version and required defaults                                                                    | Purpose |
| --------------------- | ------------------------------------------------------------------------------------------------ | ------- |
| `VISION.md`           | `<!-- kvist-vision-version: 1 -->`                                                              | Approved product direction. |
| `ARCHITECTURE.md`     | `<!-- kvist-architecture-version: 1 -->`                                                        | Approved system decomposition, dependency direction, and cross-cutting decisions. |
| `kvist.toml`          | configuration schema `1`; `component_root = "src"`; `vcs.kind = "auto"`; `llm.provider = "none"` | Project-local configuration with VCS and opt-in external LLM settings. |
| `ROOT_CONTRACT.md`    | `<!-- kvist-root-contract-version: 1 -->`                                                       | Global architectural and compliance constraints for every component. |
| `src/REQUIREMENTS.md` | `<!-- kvist-requirements-version: 1 -->`                                                        | Root outcomes, constraints, acceptance criteria, and verification obligations. |
| `src/CONTRACT.md`     | `<!-- kvist-contract-version: 1 -->`                                                            | Consumer-facing interfaces and observable semantics. |
| `src/DESIGN.md`       | `<!-- kvist-design-version: 1 -->`                                                              | Private structure, algorithms, state transitions, and recovery design. |
| `src/TODOS.yaml`      | `schema_version: 1`                                                                             | Versioned, traceable execution plan with ordered lifecycle tasks. |
| `src/IMPL.md`         | `<!-- kvist-implementation-record-version: 1 -->`                                               | Independently observed implementation record. |

These formats have independent version markers. This pre-release artifact
split is a clean break: retired component files, commands, and queue fields
are not recognized or migrated, and the split does not bump the project
version. Kvist never silently rewrites user-authored artifacts. Initial
templates contain no credentials, configured provider, copyright notices, or
license terms.

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

`kvist init` detects an uninitialized Rust project containing `Cargo.toml` and
`src/`, then creates draft onboarding artifacts under `.kvist/` without changing
the manifest, source, tests, or benchmarks. `kvist convert <PROJECT_DIR>` exposes
that same conversion explicitly. Conversion validates the generated requirements, contract, design, and queue
before writing them, refuses link-like paths, and never overwrites an existing
`.kvist/` directory. Source-derived intent is explicitly draft rather than
inferred truth and must be reviewed before task execution.

Otherwise, `kvist init` creates a missing target directory, rejects a link-like root or
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
well-formed version this binary does not support. Kvist has no automatic
repair, backward-compatibility interpretation, or migration for the retired
artifact model. Preserve user content, use `doctor` to inspect it, and update
the project explicitly to the current model.

## Project status reports

`kvist status [PROJECT_DIR] [--format text|json] [--only-documents]` inspects
the current root project and every discovered component without writing files.
It reports
`unsupported-version`, `invalid`, `missing`, `stale`, `blocked`, and `current`
component states in precedence order. `--only-documents` filters the report to
document state. Valid queue records are compared against exact SHA-256 digests
of local `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md` and, for a child,
its immediate parent `CONTRACT.md`; each mismatch is attributable and is never
persisted by inspection.

Text output begins with `status-format-version: 1`; JSON output is a compact
object with `format_version`, `project_path`, `project_state`,
`component_root`, `components`, and `discovery_error`. Both are deterministic
and report the same configured component root, ordered components, and
adjacent `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, and
`IMPL.md` states. Dynamic text fields escape ASCII control characters. JSON
path strings are lossy display text and are not persistent file identifiers.
A completed inspection exits successfully regardless of reported project
state; I/O failures exit nonzero.

## Version-control policy

Before task execution, durable artifacts (`VISION.md`, `ARCHITECTURE.md`,
`kvist.toml`, `ROOT_CONTRACT.md`, and each component's `REQUIREMENTS.md`,
`CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, and `IMPL.md`) must be tracked in a
supported VCS. `kvist doctor` inspects root and discovered artifacts without
staging or committing.

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
component acceptance, and task execution require a complete tracking
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
component; a descendant is recognized for diagnosis when at least one of
`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, or `IMPL.md`
exists beside it. This prevents ordinary source directories from becoming
components while retaining incomplete layouts for diagnosis.

Directories without component artifacts are transparent namespaces. A
recognized descendant's immediate parent component is its nearest ancestor
with component artifacts, which may be separated by one or more transparent
directories.

Each artifact must be a regular file. Missing artifacts produce an incomplete
status; directories, symbolic links, and other filesystem objects at required
artifact paths produce an invalid status. Content validation is performed by
the component-document and task-queue validators.

Traversal skips `.git`, `.hg`, `.jj`, `node_modules`, and `target` directories,
visits paths in lexical order, and reports the exact configured limit rather
than silently truncating.

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

## Component document format

Each intent document starts with its independent version marker and uses exact,
ordered, unique, nonempty level-two sections:

| Artifact | Authority | Required sections |
| --- | --- | --- |
| `REQUIREMENTS.md` | Outcomes, constraints, acceptance, verification | Purpose and scope; stakeholders and concerns; functional requirements; quality requirements and constraints; acceptance and traceability |
| `CONTRACT.md` | Consumer-visible semantics | Boundary and ownership; provided interfaces; required interfaces; data and schemas; behavioral guarantees; errors and failure semantics; security and authority; compatibility and verification |
| `DESIGN.md` | Private realization | Design overview; internal structure; interactions and state; algorithms and decisions; failure and recovery; security and resource design; verification strategy |

`CONTRACT.md` references any useful native machine-readable schema by exact
provider-owned path and dialect/version. The schema supplements consumer
semantics; it does not move authorization, ordering, retry, or failure behavior
out of the contract.

The validator returns deterministic one-based line and column diagnostics for
version, order, duplicate, missing, and empty-section issues. It accepts only
regular UTF-8 files up to 1 MiB, rejects link-like paths, and never rewrites
human content.

Root-state inspection applies the same 1 MiB bound to root and component
Markdown and YAML artifacts before reading or parsing them.

`kvist component new <COMPONENT_DIR>` checks all three destinations before
writing, creates deterministic templates with same-directory no-clobber
persistence, and never overwrites. `kvist component validate <COMPONENT_DIR>`
validates the complete local intent set.
`kvist component accept <COMPONENT_DIR>` additionally validates the immediate
parent contract and records current revisions without changing task definitions
or task status. It does not currently enforce AI review or inspect a review
receipt.

The planned advisory workflow reviews the exact local intent documents plus
authored task-definition fields before acceptance. Findings remain nonbinding:
a review-required project accepts an acknowledged receipt or an exact-bundle
exception, while a visible project opt-out disables the gate. Review evidence,
project-level acceptance, IMPL-derived intent proposals, and contract-clause
traceability are not current CLI interfaces.

## Dependencies

The CLI uses [clap](https://crates.io/crates/clap) 4 for typed, accessible
argument parsing and help generation, and
[thiserror](https://crates.io/crates/thiserror) 2 for concise, typed domain
errors. Both are mature, widely maintained Rust ecosystem dependencies. The
project keeps its dependency graph small and adds dependencies only when their
security, licensing, maintenance, and operational benefits are justified.

Dependency policy is enforced with
[`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny):

```bash
cargo install --locked --version 0.20.2 cargo-deny
cargo deny --manifest-path engine/Cargo.toml --all-features --locked check advisories bans licenses sources
```

CI rejects known advisories, wildcard requirements, unknown registries or Git
sources, and licenses outside the reviewed allowlist. Duplicate dependency
versions remain warnings because the pinned Rig graph currently contains
legitimate duplicates that are tracked as footprint cost.

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
  requirements_revision: sha256:<64-lowercase-hex-digits>
  contract_revision: sha256:<64-lowercase-hex-digits>
  design_revision: sha256:<64-lowercase-hex-digits>
  parent_contract: null
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
      - REQUIREMENTS.md#REQ-TASK-QUEUE
    timestamps:
      created_at: 2026-08-13T20:19:53Z
      updated_at: 2026-08-13T20:19:53Z
      completed_at: null
    blocked_reason: null
```

### Component revision and revalidation fields

| Field | Allowed values | Purpose and tool use |
| --- | --- | --- |
| `schema_version` | Integer `1` | Selects the queue parser contract independently of other artifact versions. Unknown versions are refused. |
| `component.requirements_revision` | `sha256:` plus 64 lowercase hexadecimal digits | Fingerprints the exact local `REQUIREMENTS.md` used to plan the queue. |
| `component.contract_revision` | Same SHA-256 form | Fingerprints the exact local `CONTRACT.md`; a change is separately attributable and may affect declared consumers. |
| `component.design_revision` | Same SHA-256 form | Fingerprints the exact local `DESIGN.md`; a change stales local work without becoming an implicit child input. |
| `component.parent_contract` | `null` at the root, otherwise `{ path: "<relative-parent-contract>", revision: "sha256:..." }` | Records the only implicit upstream component context. |
| `parent_contract.path` | One or more `..` segments followed by `CONTRACT.md`, such as `../CONTRACT.md` or `../../../CONTRACT.md` | Is computed from the child to its actual nearest ancestor component across transparent namespace directories; arbitrary peers and project paths are rejected. |
| `parent_contract.revision` | SHA-256 revision form | Records the reviewed parent consumer contract; a later mismatch is attributable stale evidence. |
| `revalidation.state` | `current` or `stale` | `current` permits task selection; `stale` blocks it until explicit component acceptance. |
| `revalidation.checked_at` | Whole-second UTC RFC 3339 | Records when revisions were accepted or compared. |
| `revalidation.stale_since` | `null` when current; UTC timestamp when stale | Preserves how long the current stale condition has existed. |
| `revalidation.causes` | Empty when current; nonempty cause list when stale | Retains exact mismatch evidence rather than hiding it behind a boolean. |
| `causes[].kind` | Local requirements, contract, or design revision changed; or parent contract revision changed | Attributes the artifact that invalidated the queue. |
| `causes[].path` | Nonblank component-relative artifact path | Identifies the exact inspected artifact. |
| `causes[].expected_revision` | SHA-256 revision | Preserves the revision on which the queue relied. |
| `causes[].observed_revision` | Different SHA-256 revision | Preserves the revision that invalidated the queue. |

A current queue must have `stale_since: null` and `causes: []`. A stale queue
must have both timestamps, with `stale_since` no later than `checked_at`, and
at least one cause whose nonblank path has different valid expected and
observed revisions. This means staleness is inspectable evidence, not a
mutable boolean. `kvist status` derives the mismatch when local requirements,
contract, design, or the immediate parent contract changes; a
human then reviews affected tasks, updates requirement links and revisions as
necessary, and runs `kvist component accept`. No task-selection tool may
silently treat a stale plan as current.

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
| `requirements`            | Lexically sorted, duplicate-free `SOURCE#LOCATOR` strings                           | Links the task to exact requirements, contract guarantees, root constraints, roadmap items, or runbook obligations. Review and execution retain these references as evidence.                  |
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

### Ordering and serialization

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

This is the only pre-release queue format recognized by the current artifact
model. Retired fields are rejected rather than aliased or migrated. Future
compatibility policy must be declared explicitly before release; it is not
implied by the current version marker.

## License and contributions

Kvist and the in-repository `agent-runtime` crate are copyright (C) 2026
Stefan Lindblad and licensed under the
[GNU Affero General Public License, version 3 or later](LICENSE).

The AGPL permits personal, open-source, and commercial use under its terms.
Organizations that need to distribute, embed, modify, or operate Kvist without
AGPL obligations may request separate commercial terms as described in
[`COMMERCIAL-LICENSE.md`](COMMERCIAL-LICENSE.md). No permission beyond the
AGPL is granted unless both parties execute a separate written agreement.

External code contributions are not accepted at this stage. Bug reports,
use-case feedback, and design discussion remain welcome. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## Task execution boundary

The intended executor advances an accepted queue from task to task without
requiring the human to select each one; the final independent review determines
whether the resulting component can be declared compliant. Human
task-by-task supervision remains an option, not a lifecycle requirement.

Today, `task run` is the one-task execution primitive that an unattended loop
can compose. It requires a current project and component, complete VCS
tracking, and a ready queue task. One user-owned, sandbox-inaccessible lock,
keyed by canonical project and component identity, is retained from selection
through agent execution, verification, durable evidence, and its terminal
transition. Kvist revalidates its ownership before durable transitions. Kvist
passes the task-appropriate local component artifacts to the configured agent.
The immediate parent `CONTRACT.md` is the only implicit propagated component
context; parent requirements, parent design, and peer implementation files are
excluded. Sandboxed execution mounts the component writable at
`/workspace/component`, the root contract read-only at
`/workspace/context/ROOT_CONTRACT.md`, and a child's actual parent contract
read-only at `/workspace/context/PARENT_CONTRACT.md`. General materialization
of explicitly declared provider contracts remains deferred. See
[`GUIDE.md`](GUIDE.md) for a current command-line loop.

The current runner has two profiles: `developer` for test and implementation
tasks, and `architect` for security-audit and compliance-review tasks. Agent
templates support `{prompt}`, `{prompt_json}`, `{context_files}`, and
`{target_directory}` and are spawned without a shell. `{prompt_json}` includes
the JSON quotes and escaping. Templates are whitespace-delimited arguments, not
shell scripts; pipelines, redirections, and shell quoting are unsupported.

Each resolved agent profile has a mandatory timeout and combined stdout/stderr
byte cap. Defaults are 300 seconds and 65,536 bytes; configuration may lower
or raise them only to positive values within hard maxima of 3,600 seconds and
1,048,576 bytes. A timeout or combined-output breach terminates the sandbox
runner and blocks the task with bounded attempt evidence. Optional
`[agent.profiles.<name>.redaction] values = ["..."]` lists nonblank literal
values to replace with `[REDACTED]`; values inherited through the sandbox
environment allowlist are automatically redacted too. Kvist normalizes agent
output as stdout followed by stderr and redacts that combined value before
logs, streams, attempt records, blocker reasons, and terminal output.
`--stream` writes that one redacted value to stdout, so original inter-stream
ordering is not preserved. Runtime log directories and files must be real,
non-link filesystem objects.

The current automated coverage exercises general command interpolation and
sandboxed agent execution. Focused regression coverage for explicit and
fallback model selection, `none` raw handling, and system-prompt prefixing is
still required; model availability is checked only when the selected command
is run by the sandbox runner.

An implementation task also runs the matching inherited test command after the
agent exits successfully. Test commands require a versioned `[test_policy]`
included in the full execution approval. The policy controls working directory,
inherited environment variables, timeout, output cap, and component-to-command
mapping. Verification output, command evidence, and blocked reasons use the
same approval-bound redaction set (both profiles' explicit values plus sandbox
allowlisted environment values) and are capped at 65,536 bytes per retained
field. Agent logs are written under `.kvist/logs`; attempt and verification
records are durable JSONL files.

**Safety status:** agent and test programs require an external sandbox runner
that attests network denial, one writable component mount, and the required
read-only root/parent context mounts. Every execution-sensitive input must
match the explicit approval record. Agent profiles enforce approved timeouts
and combined-output caps; bounded, redacted evidence is retained when a limit
breach blocks a task.

## Intended lifecycle and current scope

Kvist's direction remains structure before syntax. The human architect starts
with `VISION.md`, approves `ARCHITECTURE.md`, decomposes the system into
components, and approves each component's requirements, consumer contract, and
private design. A designer derives the traceable queue from those documents.
Tests precede implementation, followed by security audit.

A clean-slate documenter then derives `IMPL.md` from source and test evidence
without reading any intent artifact, queue, prior record, architecture/root
intent, prior review, chat history, or Git history. A different source-blind
reviewer compares the fresh record and test evidence with `REQUIREMENTS.md`,
`CONTRACT.md`, and `DESIGN.md`. The implementer cannot self-certify.
`IMPL.md` is not user documentation; public integration material belongs under
`docs/`.

The CLI currently enforces queue ordering and durable transitions, but it does
not yet automate architect/designer agents, the interview, clean-slate
documentation, source-blind comparison, arbitration, editor integration,
daemon, LSP, or web UI flows. Those are planned capabilities, not current
commands. See
[`GUIDE.md`](GUIDE.md) for the accurate manual workflow and
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
for the enduring architecture and phased direction.
