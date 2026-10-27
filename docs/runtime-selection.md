# Runtime selection and ownership

The approved architecture is [ADR 0015](decisions/0015-shared-runtime-and-run-inspection.md).
Sav is the shared headless runtime library. Skott is its interactive/headless
workspace client and independent run inspector. Kvist/maerg owns protected task
authority, workflow and canonical evidence; Galla enforces approved effects.
Skott links Sav, not the `sav-run` executable.
Its native workspace tools and deliberately separate dependency-acquisition
boundary are described in [Native agent toolkit](agent-toolkit.md).

## Select a backend, not a different source of truth

| Backend | Loop owner | Suitable use | Authority |
| --- | --- | --- | --- |
| Native Ollama / llama-server | Shared Sav native runtime with host integration | Interactive/headless workspace work; protected Kvist authoring through its broker | Host-supplied tools and separately enforced grants |
| External coding agent | Selected CLI owns its internal loop; Sav supervises the complete process | Explicit opaque-agent execution | Explicit outer execution boundary; never inferred from provider flags |
| One-shot command | No first-party tool loop | Diagnostics and bounded prompt execution | Explicit acknowledged process execution |

Model transport, agent driver and effect enforcement are separate choices.
Copilot/Gemini catalog discovery through ACP does not make those agents native
model transports. An opaque external agent must not be wrapped inside the
native model/tool loop. Tool requests, provider output and permission switches
are never authorization.

Protected task authoring must reject a backend that cannot satisfy its broker
contract. Do not replace `kvist task run` with the workspace shell merely because
both can use a sandbox. Skott's workspace and maerg's protected component grants
are deliberately different.

## Shared configuration

Sav owns typed provider/profile resolution, bounded discovery, qualification and
atomic persistence. Skott and Kvist bind the same profiles rather than maintain
copied model definitions. Application bindings, UI preferences, tool policies
and task approval remain local to the application.

A provider instance identifies an endpoint or executable. A profile identifies
the provider-facing model and supported settings. Discovery is a time-bound
observation, not an approved configuration or guarantee of future availability.
Advertised, tested and enabled capabilities remain distinct.

Explicit task selections are resolved once per attempt. Loaded-model preference
is an interactive convenience and must not override a pinned task binding.
Configuration loading never implicitly starts a provider, performs billable
qualification, installs tools, or grants process/network authority.

## Interactive and batch operation

The same native mechanisms support both terminal and unattended operation.
Skott headless workspace execution retains its required private journals and
does not acquire an unconfined host fallback. Maerg retains its protected broker,
dependency phases, verification and human finalization.

`sav-run` remains a small diagnostic/setup/supervision client. Its continued
existence does not imply a second first-party native engine.

## Observation and control

Run owners append versioned live diagnostic records during execution.
`skott --attach JOURNAL` inspects those records without loading models or tools.
Without a terminal it prints a snapshot; `--follow` waits for appended records.
Closing inspection never terminates execution. EOF without a terminal
disposition is incomplete, not successful completion.

Controls are separate private local capabilities. Supply `--control SOCKET`
with `--cancel-run` or `--guide TEXT` to send an explicitly authorized request.
Queue acknowledgement is not model-context delivery. Guidance supplements the
existing task without changing approved intent, grants or accounting; executing
effects are not interrupted to steer. Cancellation cannot undo partial or
unknown effects. Neither logs nor a missing process authorize replay/resume.

## Frameworks and deferred integrations

The `rig-core` experiment was rejected and removed in
[ADR 0009](decisions/0009-brokered-model-transport-and-isolated-effects.md).
No Rig transport or agent runtime is part of the current dependency graph.
MCP, richer ACP execution and other agent frameworks require explicit adapters,
capability tests and host authority contracts before activation. They do not
replace canonical Kvist state or evidence.
