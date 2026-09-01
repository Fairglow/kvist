<!-- kvist-requirements-version: 1 -->
# Agent Runtime Requirements

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Purpose and scope

`agent-runtime` provides a reusable Linux-first Rust library and standalone CLI
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
It MUST qualify the generated command automatically with one fixed, minimal,
nonblank test prompt rather than asking the user to supply a prompt or repeat a
host-authority acknowledgement. The setup action itself is the acknowledgement
for that exact qualification command. Failed qualification MUST prevent
persistence without offering an interactive bypass; an explicit `--force` MAY
persist the failed profile with a warning. Qualification uses current host
authority and does not claim tool or filesystem isolation. Qualification
output MUST remain bounded and MUST NOT be forwarded as setup presentation.

### AR-REQ-MODEL-TRANSPORT

Direct local Ollama and llama-server transports MUST support bounded unary and
streamed text and structured tool-intent translation without executing tools.
Endpoints, requests, responses, deadlines, cancellation, identities, usage,
finish reason, and errors MUST remain explicit.

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

- Rust 1.94 and edition 2024 are supported for the currently pinned optional
  Rig release; unsafe Rust is forbidden.
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
- Capabilities are tracked separately as advertised, conformance-tested, and
  policy-enabled.

## Acceptance and traceability

Each requirement requires deterministic unit or integration tests with fake
providers or local loopback servers. Process-group, signal, timeout, output,
configuration, filesystem, and endpoint guarantees require Linux-native tests.
Provider promotion requires targeted security audit and independent compliance
review.

The consumer boundary is defined in [`CONTRACT.md`](CONTRACT.md), private
realization in [`DESIGN.md`](DESIGN.md), supplemental authority design in
[`../../docs/agent-runtime/architecture.md`](../../docs/agent-runtime/architecture.md),
and Rig evaluation evidence in
[`../../docs/agent-runtime/rig-evaluation.md`](../../docs/agent-runtime/rig-evaluation.md).
