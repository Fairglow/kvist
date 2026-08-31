# Kvist workflow guide

Kvist is a local, filesystem-native tool for human-directed,
architecture-driven development. Its current interface is command-line based;
future graphical and editor interfaces operate on the same durable project
artifacts. The hierarchy is `VISION.md` -> `ARCHITECTURE.md` -> per-component
`REQUIREMENTS.md` + `CONTRACT.md` + `DESIGN.md` -> `TODOS.yaml` -> `IMPL.md`.
A component works from its local artifacts, `ROOT_CONTRACT.md`, and the
immediate parent `CONTRACT.md`. That parent is the nearest ancestor component,
even across transparent namespace directories. Parent requirements or design
and peer implementations do not propagate implicitly.

Executable releases currently support Linux only. macOS and Windows remain
disabled until native test environments and independently reviewed execution
backends are available.

[`VISION.md`](VISION.md) defines product direction,
[`ARCHITECTURE.md`](ARCHITECTURE.md) defines approved system structure, and
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
retains detailed strategy. This guide describes current commands and labels
required but not-yet-automated stages explicitly.

## Intended lifecycle

1. The human architect approves `VISION.md`, `ARCHITECTURE.md`, and each
   component boundary. For that component, `REQUIREMENTS.md` owns outcomes,
   constraints, acceptance criteria, and verification obligations;
   `CONTRACT.md` owns consumer-facing interfaces and observable semantics; and
   `DESIGN.md` owns internal realization. The three documents may be drafted
   manually or with agent assistance, but the human explicitly approves them.
2. A designer derives a traceable `TODOS.yaml` plan from the approved
   requirements, contract, and design in test, implementation, security-audit,
   and compliance-review order. The architect reviews and accepts that plan.
3. The executor advances every accepted, ready task in the queue in dependency
   order with only the permitted component context. It can run uninterrupted
   and unsupervised; the human may choose to observe or run one task at a time,
   but does not need to intervene between tasks.
4. A clean-slate documenter derives `IMPL.md` from implementation and test
   evidence without reading any intent document or prior `IMPL.md`. A separate,
   source-blind reviewer compares the fresh record and test evidence with
   `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`. The architect arbitrates
   every discrepancy explicitly.

The human remains the approval authority at every stage. Kvist validates
component documents and queues, persists legal task transitions, and can
invoke a configured external agent for one task. It does **not** yet automate
the architect or designer roles, the interview, clean-slate record creation,
source-blind review, or arbitration loop. Do not claim a component is
compliant until independent review is recorded.

## Start a project

```bash
kvist init my-project
cd my-project
kvist doctor .
kvist status .
```

`init` writes the root contract and root component artifacts only into an
uninitialized directory. When the directory is an existing Rust package with
`Cargo.toml` and `src/`, it instead writes validated draft onboarding artifacts
to `.kvist/` and preserves the package implementation. `doctor` provides
read-only diagnostics, and `status` reports component state without persisting
derived stale evidence.

After the architect approves a child component boundary, create and validate
its intent documents:

```bash
kvist component new src/network
kvist component validate src/network
```

The architect reviews the three documents, then records their current
revisions:

```bash
kvist component accept src/network
```

The designer then drafts the queue from the approved intent. Each task states
its purpose, context, expected outcome, requirement references, dependencies,
lifecycle kind, and status. The queue is durable workflow state, not an agent
transcript. Its component provenance fields are `requirements_revision`,
`contract_revision`, `design_revision`, and `parent_contract`.

## Inspect and revalidate work

```bash
kvist status .
kvist status . --only-documents
kvist tree .
kvist task next .
kvist component accept .
```

`status` detects local requirements, contract, design, and immediate-parent
contract digest mismatches and reports attributable stale components.
`--only-documents` limits status output to document state. `component accept`
is an explicit revalidation write: it records the currently reviewed revisions
and clears attributable stale evidence for the selected component. It does not
approve an arbitrary implementation change or replace independent compliance
review.

`task next` selects the first ready task in declared order. `task transition`
performs one audited state change and records `prepared` and `committed`
attempt evidence. Both require a current project, current component, and
complete VCS tracking.

## External agents and verification

`task run COMPONENT_DIR [TASK_ID]` is an optional sandbox-runner integration.
It uses the `developer` profile for `test` and `implementation` tasks, the
`security-reviewer` profile for security audits, and the `architect` profile
for compliance reviews.

The sandbox exposes one writable mount at `/workspace/component`. It also
materializes `ROOT_CONTRACT.md` read-only at
`/workspace/context/ROOT_CONTRACT.md` and, for a child, its actual nearest
ancestor component contract at `/workspace/context/PARENT_CONTRACT.md`.
Transparent namespace directories do not prevent that parent lookup. General
materialization of other explicitly declared provider contracts remains
deferred.

The target executor runs the accepted queue uninterrupted and lets the final
independent review decide compliance. The current command-line surface exposes
only the one-task primitive, but it can run the full ready queue unattended on
POSIX shells:

```bash
while task_id="$(kvist task next .)" && [ "$task_id" != "no ready task" ]; do
  kvist task run . "$task_id" || exit $?
done
kvist status .
```

This loop selects and executes each ready task in order, stopping when no task
is ready or a command-level error occurs. The final `status` exposes a blocked
or stale result. It does not automate the clean-slate documenter or
source-blind reviewer in step 4; that independent validation remains manual
until Phase 3 automation is implemented.

Agent configuration is selected from `[agent]` in `kvist.toml`,
`.kvist/config.toml`, the user configuration path, then the system
configuration path. Template arguments are passed without a shell and may use
`{prompt}`, `{prompt_json}`, `{context_files}`, and `{target_directory}`;
`{prompt_json}` emits a complete escaped JSON string. Keep every intended
argument whitespace-free or use a wrapper executable; shell pipelines,
redirection, and shell quoting are not supported.

Generic provider profiles can be configured independently of Kvist:

```bash
agent-run setup
agent-run run --allow-host-execution \
  --profile local-coder \
  --file prompt.md
```

Test a local inference endpoint without granting a provider process host
execution:

```bash
agent-run model \
  --provider ollama \
  --endpoint http://127.0.0.1:11434 \
  --model qwen3-coder \
  --stream \
  --file prompt.md
```

Use `--provider llama-server --endpoint http://127.0.0.1:9931` for
llama-server. This command is text-only and exposes no tools. The underlying
library decodes canonical tool intents for future brokered use, but does not
authorize or execute them.

llama-server setup probes `/health` rather than the router UI and reads the
bounded OpenAI-compatible `/v1/models` list. A listed model can be selected by
number; an exact model ID or the literal `default` can be entered instead.
The profile name is a separate value.

`kvist agent setup` either collects a profile through that same library setup
flow or loads a saved standalone profile. Kvist then materializes the selected
name and command into its role configuration. It does not resolve a mutable
standalone profile during task execution, so `task approve-policy` continues
to cover the exact effective command.

For llama-cli, Gemini, and Copilot, setup first verifies the conventional
executable with `--version`; an inaccessible command triggers a compatible
executable/wrapper path prompt without changing the provider kind. The
subsequent live model test is what qualifies the complete command, model,
credentials, and arguments. A llama wrapper must forward with `"$@"`, never
unquoted `$*`. llama-cli is inference-only, whereas the generated Gemini and
Copilot templates enable noninteractive agent tools under the explicit host
execution warning.

The planned agent runtime separates model transport, bounded native loop, typed
tool broker, policy, execution backend, and durable evidence. Local
llama-server and Ollama integrations will use the standalone-owned loop under
Kvist authority; Gemini and Copilot remain opaque external agents constrained
as complete processes. The direct HTTP transport remains the default and
fallback. An exactly pinned Rig 0.42.0 adapter is available behind the
`rig-transport` Cargo feature after raising the project MSRV to Rust 1.94. See
[`docs/agent-runtime/architecture.md`](docs/agent-runtime/architecture.md)
and the versioned
[`Rig transport evaluation`](docs/agent-runtime/rig-evaluation.md).

Build the optional adapter and select it explicitly:

```bash
cargo run -p agent-runtime --bin agent-run --features rig-transport -- \
  model --transport rig --provider ollama \
  --endpoint http://127.0.0.1:11434 --model qwen3-coder \
  --file prompt.md
```

For implementation tasks, configure and approve the repository test policy
before running:

```bash
kvist task approve-policy
kvist task run . implement-code
kvist task log . implement-code
```

The approval record covers the exact `ROOT_CONTRACT.md` digest, agent
configuration, sandbox runner identity, resource limits, redaction policy, and
`[test_policy]`. Agent and test programs are sent to the separately installed
sandbox runner; Kvist never falls back to executing them directly on the host.

`kvist prompt`, in contrast, is an explicitly acknowledged host operation:

```bash
kvist prompt --allow-host-execution --file prompt.md
```

Its bounded prompt input, command rendering, idle supervision, loop detection,
and retry notices come from the standalone `agent-runtime` library. The
standalone CLI can be invoked with
`cargo run -p agent-runtime --bin agent-run -- run ...`.
Host-mode retry notices warn that an earlier attempt may have left side effects;
they do not provide rollback or isolation. Snapshot workspaces, restricted
identities, Linux sandboxing, brokered tools, and future platforms are planned
in
`src/agent_runtime/REQUIREMENTS.md`, `src/agent_runtime/CONTRACT.md`,
`src/agent_runtime/DESIGN.md`, and `src/agent_runtime/TODOS.yaml`.

## Documentation and review discipline

`IMPL.md` is an observed implementation record, not user-facing documentation.
Its documenter examines source, tests, manifests, and necessary non-intent
build configuration without reading `REQUIREMENTS.md`, `CONTRACT.md`,
`DESIGN.md`, `TODOS.yaml`, any prior `IMPL.md`, architecture or root intent,
prior reviews, chat history, or Git history. The source-blind reviewer examines
the approved requirements, contract, design, fresh implementation record, test
evidence, immediate parent contract, and root contract without source access.
Record compliance, mismatches, approved deferrals, and arbitration in version
control. The implementer may not certify its own work, and no participant may
edit intent or observed records merely to hide a disagreement.

The component artifact split is pre-release. Retired component documents,
commands, and queue fields are not accepted or migrated, and there is no
version bump for this clean break.

The reusable procedure is in [`REVIEW_RUNBOOK.md`](REVIEW_RUNBOOK.md).
[`COMPLIANCE_REVIEW.md`](COMPLIANCE_REVIEW.md) records completed reviews and
the current unreviewed execution-policy discrepancy.
