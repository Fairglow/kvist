<!-- kvist-requirements-version: 1 -->

# Sav Requirements

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Purpose and scope

`sav` provides a reusable Linux-first Rust library and standalone CLI
for bounded prompt acquisition, provider profile management, shell-free command
rendering, process supervision, local model transport, and the planned
provider-neutral native agent runtime.

It is independent of Kvist component discovery, task queues, roles, project
policy, artifact promotion, and compliance evidence. An embedding host supplies
authority through narrow interfaces.

## Stakeholders and concerns

- Embedding hosts need stable provider-neutral mechanisms without reverse
  dependencies.
- CLI users need explicit host-authority warnings and actionable failures.
- Provider integrations need faithful request, streaming, usage, identity,
  deadline, and cancellation translation.
- Security reviewers need bounded untrusted input, private adapters, no shell,
  safe endpoint policy, and complete process cleanup.
- Runtime implementers need explicit ownership of loop, broker, backend, and
  event responsibilities.

## Functional requirements

The following stable requirement identifiers define the mandatory behavior of
the reusable runtime component.

### AR-REQ-PROMPT-COMMAND

The component MUST acquire one bounded prompt source, render documented command
placeholders without a shell, and build every retry from fresh typed input with
explicit prior-attempt context. Named profiles MUST allow multiple providers,
models, and command configurations to coexist and MUST be selectable per
prompt. A requested reasoning effort MUST be applied only through a declared
template placeholder or a transport that explicitly supports it; it MUST NOT be
silently ignored or converted into shell text.

### AR-REQ-OUTPUT-PRESENTATION

Plain-text prompt commands MUST write only provider answer content to standard
output. They MAY show the initial prompt and live provider progress on an
interactive standard error stream, but MUST NOT append success banners,
statistics, or wrapper metadata. JSON mode MUST suppress live provider streams
and emit exactly one valid JSON object whose `content` field contains the
provider's standard output as text. Invalid UTF-8 byte sequences MUST be
replaced with U+FFFD rather than making the JSON invalid or exposing a partial
object.

Direct model mode MUST optionally expose bounded provider-supplied reasoning
text or summaries while streaming and in its canonical JSON result. Such data
MUST be identified as provider-supplied reasoning, MUST remain separate from
answer content, and MUST NOT be described as hidden chain-of-thought. Providers
that do not emit reasoning remain fully valid.

### AR-REQ-SUPERVISION

Supervision MUST stream bounded output, detect configured idle and loop
conditions, enforce cancellation and wall/output limits, terminate and reap
the Linux process group before return or retry, and treat unknown side effects
as potentially retained.

### AR-REQ-PROFILES

Named provider profiles MUST use strict bounded TOML, preserve unrelated
formatting and values, validate before and after mutation, resolve safe
configuration locations, and persist through synchronized atomic replacement.

### AR-REQ-SETUP

Setup MUST use caller-provided terminal-neutral I/O, maintained provider
templates, bounded advisory probes, and explicit executable/model selection.
Before accepting a provider model identifier, setup MUST obtain the provider's
bounded model catalog when a machine-readable catalog is available, present
numbered choices, and place manual entry behind a final explicit custom-model
choice. Ollama and llama-server MUST use their HTTP model-list interfaces;
Copilot and Gemini CLI MUST use their account-aware ACP session model lists.
Discovery failure MUST be visible and MAY fall back only to a documented
provider default plus the explicit custom-model choice. File-backed and custom
wrapper providers, which expose no model catalog, MUST identify their path or
wrapper selection as manual.

The standalone runtime MUST provide a non-interactive command that lists the
same discovered provider models in deterministic text or JSON form. Discovery
MUST be bounded by time, output bytes, model count, and model-identifier size,
must not invoke a model prompt, and must clean up every helper process.
ACP-backed standalone discovery MUST require an explicit
`--allow-host-discovery` acknowledgement for that one provider session.

It MUST qualify the generated command automatically with one fixed, minimal,
nonblank test prompt rather than asking the user to supply a prompt or repeat a
host-authority acknowledgement. The setup action itself is the acknowledgement
for the bounded provider discovery and exact qualification commands. Failed
qualification MUST prevent persistence without offering an interactive bypass;
an explicit `--force` MAY persist the failed profile with a warning.
Qualification and discovery use current host authority and do not claim tool
or filesystem isolation. Qualification output MUST remain bounded and MUST NOT
be forwarded as setup presentation.

### AR-REQ-MODEL-TRANSPORT

The direct transport MUST expose bounded selected-model serving-context
discovery without inference or tool execution, preserve local-only endpoint
policy, distinguish unavailable capacity from a numeric limit, and reject
malformed/nonpositive/out-of-bound metadata. llama-server requests MUST request
streaming usage. Unknown usage MUST remain unknown. Capacity discovery and
larger-context requests MUST retain explicit response/request byte ceilings.

The October remediation has an explicit human-requested advisory-review
exception because acceptance receipts are not implemented; separate post-code
security and compliance review remain required.

Direct local Ollama and llama-server transports MUST support bounded unary and
streamed text and structured tool-intent translation without executing tools.
Endpoints, requests, responses, deadlines, cancellation, identities, usage,
finish reason, and errors MUST remain explicit.

llama-server finish reasons MUST NOT be inferred from tool-call presence or
the streaming terminal marker. An explicit `stop` MUST remain `Stop` even
when tool intents accompany it; a missing or null terminal reason MUST remain
unknown rather than being promoted to successful text or tool completion.
Malformed non-null finish reasons MUST fail explicitly. Native Ollama terminal
tool calls MAY normalize a normal stop or absent reason to `ToolCalls`, but
explicit length, content-filter, and unknown reasons MUST remain distinct.

An enabled streaming cadence watchdog MUST start only after meaningful decoded
generation progress and MUST reset only on nonempty text, provider-supplied
reasoning, or native tool name/argument progress. HTTP framing, SSE comments,
empty deltas, control fields, and usage records MUST NOT extend that watchdog.
Tool fragments MUST count before complete tool intents are emitted. Header/slot
allocation and first-body watchdogs MUST remain separate I/O stages; caller
deadlines and cancellation MUST continue to bound every stage.

An optional `ModelRequest.max_output_tokens` MUST be positive and bounded when
present, MUST be honored as llama-server `max_tokens` and Ollama
`options.num_predict`, and MUST not execute tools. Missing output bounds retain
provider defaults. Qualifying fixtures MUST cover both protocols and reject
invalid bounds before provider I/O.

The direct transport MUST accept a bounded host-owned common JSON Schema
2020-12 provider subset for provider-native structured output, MUST reject
incompatible schema-and-tool requests before provider I/O rather than silently
dropping either constraint, and MUST NOT represent provider enforcement as
output validation.

### AR-REQ-NATIVE-RUNTIME

The planned native runtime MUST separate model transport, bounded agent loop,
typed tool broker, host authorization, execution backend, and redacted runtime
events. Tool intent is untrusted and cannot cause effects without an explicit
host decision and selected backend.

### AR-REQ-ADAPTER-BOUNDARY

Third-party provider libraries MAY implement private adapters but MUST NOT
replace canonical component types, host policy, authorization, tool execution,
evidence, or public serialized state.

## Quality requirements and constraints

- Rust 1.95 (MSRV) and edition 2024 are supported. Safe Rust is preferred;
  unsafe MAY be used only when necessary and minimally scoped, with documented
  justification and safety invariants, targeted verification, and independent
  review. This is not a promise of an unsafe-free implementation.
- Linux is the only executable target.
- Prompt input is nonblank UTF-8 at most 1 MiB.
- Profile configuration is UTF-8 TOML at most 64 KiB with at most 128 unique
  profiles; names and commands have explicit bounds.
- Idle timeout, retries, wall time, output, retained loop text, HTTP request,
  and response sizes have explicit hard maxima.
- Commands are spawned directly. Shell expansion, pipelines, redirection, and
  operators have no special meaning.
- Host execution is never described as isolated. Context paths are command
  arguments, not an access-control boundary.
- Local HTTP adapters reject proxies, redirects, TLS, non-loopback or
  non-numeric endpoints, oversized payloads, and unbounded responses.
- Setup and standalone HTTP model discovery use the same numeric-loopback,
  explicit-port endpoint restriction as local model transports.
- Capabilities are tracked separately as advertised, conformance-tested, and
  policy-enabled.
- Provider-native structured-output schemas are bounded untrusted input;
  returned content is independently validated by the embedding host.

## Acceptance and traceability

Each requirement requires deterministic unit or integration tests with fake
providers or local loopback servers. Process-group, signal, timeout, output,
configuration, filesystem, and endpoint guarantees require Linux-native tests.
Provider promotion requires targeted security audit and independent compliance
review.

The consumer boundary is defined in [`CONTRACT.md`](CONTRACT.md), private
realization in [`DESIGN.md`](DESIGN.md), supplemental authority design in
[`../../docs/sav/architecture.md`](../../docs/sav/architecture.md),
and Rig evaluation evidence in
[`../../docs/sav/rig-evaluation.md`](../../docs/sav/rig-evaluation.md).
