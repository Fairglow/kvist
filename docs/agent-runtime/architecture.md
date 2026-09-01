# Agent Runtime Architecture

## Decision

Kvist will follow a layered agent architecture organized by authority. The
standalone component owns reusable runtime mechanisms; Kvist owns the task
policy, grants, approved bindings, promotion, and durable evidence that decide
what may happen. Provider libraries and external coding agents may be reused
behind replaceable adapters, but they are not trusted as an authorization or
isolation boundary.

The supported backend classes are:

| Backend class | Examples | Agent loop | Enforcement boundary |
| --- | --- | --- | --- |
| Native model | llama-server, Ollama, approved OpenAI-compatible endpoint | Standalone bounded loop | Host policy and grants; standalone broker and execution mechanism |
| One-shot model | llama-cli | None initially; prompt/diagnostic only | No tool access |
| External agent | Gemini CLI, Copilot CLI, Aider, Goose, Cline | External process | Selected sandbox around the complete process |
| Plan-only | Any qualifying backend with effects disabled | Standalone or external process | No effectful tool is available |

Native model mode is the preferred long-term path because Kvist can observe,
authorize, execute, and journal every tool request. External-agent mode remains
useful for provider-specific coding quality and compatibility, but its internal
tool loop is opaque. Provider approval flags are defense in depth, not proof
that Kvist's contract was enforced.

## Why not normalize everything to one category?

Treating every backend as a text model discards real functionality from mature
coding agents: proprietary context selection, edit protocols, sessions, and
provider-specific tools. Treating every model as an agent delegates too much:
an inference server proposes text or tool calls but does not supply a trusted
authorization, execution, or evidence system.

The stable abstraction is therefore not "provider." It is a small set of
backend roles connected to host-owned authority layers. This avoids both the
lowest-common-denominator trap and a universal interface filled with
provider-specific switches.

## Ownership topology

The standalone `agent-runtime` crate owns the reusable runtime mechanism:
backend classification, capability state, canonical model messages and turns,
tool descriptors, untrusted tool intents and results, bounded loop state,
broker sequencing, execution-backend interfaces and reusable Linux
implementations, redacted runtime events, and private provider adapters. These
types and mechanisms cannot invent authorization or broaden a host grant.

The embedding host owns authority. In Kvist, the root engine owns task
contracts, policy decisions, authorization records, approved resource and
credential bindings, execution-tier selection, artifact promotion, and
canonical compliance evidence. The standalone interfaces accept that authority
through narrow traits; another application may provide its own policy and
evidence services without depending on Kvist.

The dependency direction is one way: Kvist depends on `agent-runtime`.
`agent-runtime` never imports Kvist types. Third-party framework types remain
behind private adapters, so neither side depends on Rig as a public contract.

## Layers and authority

### 1. Run coordinator

The coordinator binds a run to the complete task contract, policy revision,
workspace revision, backend profile, budgets, and cancellation source. It
advances an explicit state machine and never executes a tool directly.

### 2. Model transport

A transport owns provider-wire behavior:

- endpoint and approved credential-reference resolution;
- request encoding and streaming response decoding;
- provider error normalization, deadlines, and cancellation;
- model identity, token usage, finish reason, and request identifiers;
- translation between standalone canonical messages and provider messages.

It returns a canonical model turn. It does not authorize or execute tools,
select filesystem scope, or certify task completion.

OpenAI-compatible chat is an adapter target, not Kvist's internal truth.
Implementations differ in system messages, tool choice, JSON-schema
enforcement, streaming termination, usage, reasoning blocks, and cancellation.

### 3. Native agent loop

The native loop owns:

- bounded turns, wall time, tokens, output, and cost;
- transcript construction and deterministic context compaction;
- classification of final text, tool intent, malformed output, and refusal;
- sequential tool-result continuation;
- retry rules that account for uncertain side effects;
- auditable state transitions.

The first implementation will use sequential tool calls only. Multi-agent
planning, parallel effectful calls, browser control, and autonomous general
network access are out of scope until the basic loop is independently reviewed.

### 4. Tool catalog and broker

Every imported or first-party tool has a versioned descriptor containing:

- stable ID and version;
- input and output JSON schemas;
- effect class;
- required capabilities and resource scope;
- timeout and output bounds;
- idempotency and retry semantics.

The broker validates model-supplied arguments, canonicalizes resources, obtains
an authorization decision, invokes the execution backend, validates and
redacts the result, and emits a bounded terminal runtime event from which the
host may derive a durable journal record. A model tool call is an untrusted
proposal, not an authorization.

Initial first-party tools should be limited to:

- `workspace.list`
- `workspace.read`
- `workspace.patch`
- `search.code`
- `vcs.status`
- `vcs.diff`
- `test.run` through a pre-approved command profile

### 5. Policy and authorization

Policy determines whether a capability is available and whether a particular
intent may execute. Decisions bind role, task, tool version, canonical
arguments, resource scope, environment, credentials, network destination, and
the applicable policy hash.

Capabilities have three distinct states:

1. `advertised`: reported by an adapter, server, model, or external agent;
2. `tested`: demonstrated by the conformance suite for the exact deployment;
3. `policy-enabled`: approved for the current run.

No advertised capability is enabled automatically.

### 6. Execution backend

Only this layer causes local effects. It receives an explicit request
containing mounted resources, writable scope, executable and arguments,
environment, credentials, identity, network policy, resource limits, deadline,
and artifact destinations.

Tool allowlists, MCP prompts, Gemini approval modes, and Copilot permission
flags are not operating-system isolation. Linux enforcement remains a separate
backend using transactional workspaces and, in stronger tiers, restricted
identities, Bubblewrap, Landlock, seccomp, cgroups or rlimits, or stronger
container/VM isolation.

### 7. Evidence and checkpoints

Kvist owns a versioned append-only compliance-event format. It derives durable
records from bounded standalone runtime events and host authority decisions. It
records:

- run, task, attempt, and parent identifiers;
- contract, policy, profile, model, adapter, and workspace revisions;
- each model request/result in bounded redacted form;
- tool intent, authorization decision, execution request, and result;
- stdout/stderr and artifact digests;
- cancellation, timeout, uncertainty, and promotion decisions.

Provider transcripts, KV caches, or serialized third-party agent states may be
stored only as non-authoritative opaque attachments pinned to an exact adapter
version. They are not Kvist's durable workflow state.

## Protocol boundaries

| Protocol | Appropriate use | Not provided |
| --- | --- | --- |
| OpenAI-compatible chat or Responses | Model transport adapter | Universal semantics, policy, or isolation |
| Function/tool calling | Structured model intent | Authorization or execution |
| MCP | External tool/resource transport behind the broker | Model abstraction or sandbox |
| ACP | Structured external-agent/editor adapter | Model transport or internal tool mediation |
| A2A | Later remote opaque-agent delegation | Local tool broker or host enforcement |

Kvist should become an MCP host only after its broker exists. MCP server
identity, launch, schemas, credentials, and every `tools/call` remain subject
to Kvist policy. ACP can improve external-agent event handling but does not
make an external agent's internal actions visible or authorized.

## Provider strategy

### Local inference

Use llama-server as the primary llama.cpp transport. It provides a persistent
HTTP process, OpenAI-compatible chat, streaming, function calling, and
structured-output support. Tool quality remains model- and chat-template
dependent, so support is established by conformance tests rather than endpoint
claims.

Use Ollama through a native or OpenAI-compatible adapter after testing the
specific server and model. Keep llama-cli as a one-shot diagnostic and offline
fallback. llama-cli itself cannot read files or run tools; the native Kvist
loop must place required content in model messages and mediate all effects.

### External agents

Retain Gemini CLI and Copilot CLI as external-agent adapters. Run them as whole
processes under the selected execution backend, capture structured events where
available, and compare resulting artifacts and VCS changes with the contract.
Their internal permission controls may reduce accidental actions but are not
the trusted policy boundary.

## Reuse strategy

Existing projects are useful at different boundaries:

- Rig is a narrow transport implementation. On the `rig-integration`
  experiment branch, exactly pinned `rig-core` 0.42.0 is enabled by default
  behind the `rig-transport` Cargo feature and component-owned canonical
  types. The direct adapter remains an explicit fallback and conformance
  baseline; failures never trigger automatic cross-transport replay.
- Goose is a valuable Rust agent/runtime reference and possible external agent.
- Aider is a useful patch-oriented external agent and conformance benchmark.
- OpenHands is a reference for separating control and execution environments.
- Cline is a possible structured external-agent adapter.
- Continue is a useful source of local-model compatibility experience.

Kvist imports only Rig's core provider conversion, not its agent runtime.
Dependencies must preserve the authority split above, pass the project MSRV
and supply-chain gates, and remain replaceable behind standalone canonical
types.

## Compatibility lessons

Historically durable portability layers use a small mandatory core, explicit
capability discovery, conformance tests, and controlled extensions. ODBC,
POSIX, browser automation, and cloud/provider adapters all demonstrate that
nominal compatibility is insufficient.

Kvist therefore requires:

- typed unsupported-capability errors instead of silent degradation;
- deterministic fake-provider and recorded-fixture tests;
- opt-in live tests against pinned deployment versions;
- provider extensions that are namespaced, typed, approved, and redacted;
- no extension capable of bypassing policy, credentials, network, or sandbox
  decisions.

## Delivery order

1. Complete independent reviews of the current supervisor and profile setup.
2. Implement transactional workspaces and strict Linux execution.
3. Define standalone-owned canonical model, capability, tool-intent, and
   runtime-event contracts, plus the host authority traits they consume.
4. Complete independent security and compliance review of the direct local
   HTTP transport and exactly pinned Rig transport before either
   becomes a dependency of the native loop.
5. Implement the typed broker mechanism and Kvist policy, grant, binding, and
   compliance-evidence services against those traits.
6. Implement a bounded native loop using the selected transport.
7. Harden external-agent mode under the same outer execution boundary.
8. Add MCP and ACP adapters only after the trusted boundaries exist.

The durable task chains are maintained in
`engine/agent_runtime/TODOS.yaml`.
