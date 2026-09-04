# Runtime Selection for Kvist

Status: architecture guidance and roadmap, 2026-09-01.

## Decision summary

Kvist will support several agent-loop implementations behind Kvist-owned
canonical requests, events, policy decisions, and evidence. Users may
eventually choose a loop driver per profile or task purpose. Provider
transport, loop driver, and execution backend are independent choices:

```text
provider transport: direct | rig-core
loop driver: external-command | kvist-native | rig-run | rig-agent-container
execution backend: acknowledged-host | approved-sandbox | disposable-container
```

Only combinations whose exact capabilities have passed conformance and policy
checks may run. Selecting a framework never grants authority, broadens context,
or changes which durable records are canonical.

## Current support

| Choice | Current status | Appropriate use |
| --- | --- | --- |
| External command | Implemented | Copilot, Gemini, Claude, Aider, Goose, or another complete coding agent executed as an opaque process through the approved sandbox runner. |
| Kvist model transport with `rig-core` | Implemented on `rig-integration` | Unary or streamed local Ollama and llama-server model calls, structured generation, and untrusted tool-intent decoding. It is not yet a complete coding-agent loop. |
| Kvist direct model transport | Implemented fallback and conformance oracle | Explicit fallback, provider comparison, reasoning features not preserved by Rig 0.42, and diagnosing framework conversion differences. |
| Kvist-native loop | Planned | Compliance-sensitive component work where Kvist must own every context, policy, tool, retry, and evidence transition. |
| `rig-agent` loop | Not integrated | Candidate for an opaque disposable-container backend and effect-free advisory work; not canonical workflow state. |
| `rig-run` loop | Unavailable in the pinned release | Candidate for a future Kvist-driven sans-I/O loop after it is published in an immutable Rig release. |

Current `kvist.toml` agent profiles contain shell-free external command
templates. They do not select `RigModelTransport`, `rig-agent`, or `rig-run`
as typed runtimes. A command can invoke `agent-run model`, but that command
returns model output and cannot yet perform a complete component task. A future
profile schema version must model the three axes above explicitly rather than
overload command strings.

## Selection by purpose

### External coding agent

Use an external command when coding quality and mature agent-specific tools are
more important than observing its internal loop. This is the current choice for
implementation work with Copilot, Gemini, Claude, Aider, Goose, or OpenHands.
Kvist constrains the complete process externally and evaluates its resulting
artifacts and evidence. Provider permission switches are defense in depth, not
Kvist authorization.

### Kvist-native loop

Use the native loop for high-assurance work:

- contract-sensitive implementation;
- security or compliance workflows;
- tasks needing exact component context;
- effects requiring per-tool authorization;
- durable retry and uncertain-effect handling; and
- runs whose evidence must be reconstructed independently.

This is the intended long-term reference behavior. It uses either the reviewed
Rig transport or a direct transport but retains Kvist's broker, sequential
effect handling, cancellation, budgets, event journal, and promotion decision.

### Containerized `rig-agent`

Use `rig-agent` only when the whole run can be treated as an opaque,
disposable job:

- advisory review or brainstorming;
- bounded structured extraction;
- summarization and classification;
- prototypes and low-assurance implementation attempts; or
- a containerized coding agent whose final workspace diff is accepted or
  rejected as one result.

The container limits environmental effects, but it does not solve duplicate
effects, crash recovery, retry safety, context provenance, or evidence
ordering. Rig tools must either be effect-free or call a Kvist broker. Rig
memory and serialized runs are non-authoritative attachments and must not
replace project artifacts or the Kvist journal.

Adopting `rig-agent` directly inside the compliance-sensitive native path would
move transcript construction, loop sequencing, tool dispatch, and retry policy
across the wrong authority boundary. Kvist therefore treats it as an optional
backend, not the default internal runtime.

### Future `rig-run` driver

`rig-run` is the most promising framework reuse because its current design is
sans-I/O and exposes explicit model-call, tool-call, and done steps. A future
Kvist driver could:

1. compose a request from approved Kvist context and capabilities;
2. execute `CallModel` through a selected Kvist transport;
3. process `CallTools` sequentially through the Kvist broker and container
   backend;
4. journal every intent, decision, effect, and result before advancing; and
5. translate `Done` into a Kvist terminal event and promotion decision.

As of 2026-09-01, `rig-run` is post-0.42 unreleased work. Kvist must not depend
on Rig `main` or persist Rig run state as authoritative data. Evaluation starts
only after an immutable published release can be exact-pinned and tested for
crash, cancellation, replay, schema, and migration behavior.

## Expected configuration

The following illustrates a possible future schema and is not accepted by the
current parser:

```toml
[[agent.profiles]]
name = "controlled-local"
driver = "kvist-native"
transport = "rig-core"
provider = "ollama"
endpoint = "http://127.0.0.1:11434"
model = "qwen3-coder"
execution = "approved-sandbox"

[[agent.profiles]]
name = "advisory-container"
driver = "rig-agent-container"
image = "registry.example/kvist-rig-agent@sha256:<digest>"
execution = "disposable-container"

[[agent.profiles]]
name = "copilot"
driver = "external-command"
command = "copilot ... '{prompt}'"
execution = "approved-sandbox"
```

The eventual schema must use a new explicit version, reject unknown
combinations, bind container images by digest, record exact Rig and adapter
versions, and distinguish advertised, conformance-tested, and policy-enabled
capabilities. No automatic fallback may replay an uncertain model or tool
request through another driver.

## Promotion criteria

`rig-agent-container` may become supported after container lifecycle,
cancellation, output, diff capture, and opaque-result evidence are tested.
It remains unsuitable for canonical compliance-sensitive loop state.

`rig-run` may become a selectable native driver only after:

- an official release is exact-pinned;
- Kvist can reconstruct runs from its own canonical history;
- pending effects cannot be duplicated after a crash;
- multi-call turns are rejected or processed sequentially;
- automatic framework retries cannot bypass Kvist budgets or policy;
- Rig state and payloads remain bounded, redacted, and non-authoritative; and
- the driver passes the same conformance suite as the Kvist-native loop.

The default should remain purpose-based rather than universal: external agents
for mature coding capability today, Kvist-native for controlled work, optional
`rig-agent` containers for disposable convenience, and a future `rig-run`
driver only if it demonstrably removes generic loop code without weakening
Kvist's differentiating authority and evidence model.
