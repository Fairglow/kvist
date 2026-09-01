<!-- kvist-design-version: 1 -->
# Agent Runtime Design

## Design overview

The crate separates command rendering, prompt acquisition, profile storage and
setup, process supervision, direct model transport, optional Rig transport, and
canonical provider-neutral types. Reusable mechanisms remain independent of
host policy and durable evidence.

Native model mode is the preferred long-term path because the embedding host
can observe and authorize every proposed effect. External coding agents remain
supported as opaque processes constrained by an outer execution backend.

## Internal structure

| Module | Responsibility |
| --- | --- |
| `model` | Canonical messages, turns, tool intent, capability and error types |
| `command` | Shell-free parsing, placeholders, and per-attempt command specs |
| `prompt` | Positional, file, standard-input, terminal, and editor acquisition |
| `profile` | Strict bounded profile parsing and formatting-preserving persistence |
| `setup` | Provider selection, probes, model/executable collection, qualification |
| `supervisor` | Process groups, forward-or-capture streams, idle/loop/wall/output limits, retry, cancellation |
| `direct_transport` | Bounded loopback Ollama and llama-server HTTP adapters |
| `rig_transport` | Pinned non-default private Rig adapter to canonical types |
| `lib` / `main` | Public library surface and standalone CLI boundary |

The future runtime layers are run coordinator, model transport, native loop,
typed catalog and broker, host policy interface, selected execution backend,
and host-owned durable journal.

## Interactions and state

Supervision validates policy, builds an attempt command, spawns a new process
group, reads stdout/stderr through bounded nonblocking channels, forwards or
captures output according to the caller's selected presentation, and tracks
idle, wall, output, loop, and cancellation conditions. Terminal cleanup ends
and reaps the process group before the result returns. Only idle or loop
outcomes may create a new attempt with prior-failure context.

A model transport translates provider wire data into a canonical model turn.
The native loop classifies final text, tool intent, malformed output, or
refusal. Tool intent proceeds to the broker, which validates schema and
resources, requests host authorization, invokes the selected backend only
after allow, validates/redacts results, and returns them to the loop.

## Algorithms and decisions

Command parsing supports quoting and literal placeholders without shell
evaluation. `{prompt_json}` uses complete JSON string encoding. Prompt source
selection rejects conflicts before any provider starts. Reasoning effort uses a
bounded enum and `{reasoning_effort}` placeholder; selection fails rather than
silently dropping a requested setting. Multiple named profiles remain the
portable mechanism for provider-specific model, context, temperature, and
other command options.

Output handling retains bounded UTF-8 suffix state for repeated cycle, line,
and alternating-line detection while streaming complete bounded output to the
caller or a bounded capture. Plain output forwards provider streams without a
success trailer. JSON presentation suppresses forwarding and serializes one
`content` result after loss-tolerant UTF-8 decoding that replaces invalid byte
sequences with U+FFFD. Retry notices include attempt number, prior failure
class, and uncertain-side-effect warning.

Direct transport parsing keeps provider-supplied reasoning separate from final
answer text. Streaming emits distinct reasoning events; text presentation sends
them only to standard error when requested, while canonical JSON retains them.
No adapter invents or claims access to hidden reasoning.

Profile mutation parses the complete TOML document, locates or appends one
profile table while preserving formatting, validates the edited document and
bound, then performs synchronized atomic persistence.

Setup qualification is mandatory and uses `Reply with exactly: OK` to minimize
tokens while testing the complete rendered command. Starting setup acknowledges
host execution for only this qualification attempt. Failure returns before
persistence unless the invocation supplied `--force`; the override is
non-interactive and remains visible in setup output. Qualification uses bounded
capture instead of forwarding provider streams, so callers retain ownership of
their presentation boundary. Restricting qualification to a narrower
tool/filesystem authority is deferred with the broader execution backend work.

Direct transports implement the minimal provider wire surface explicitly. Rig
0.42.0 is exact-pinned, default-disabled, supplied with a constrained
component-owned HTTP client, and translated immediately into canonical types.
It cannot own tools, persistence, policy, evidence, or public provider types.
The current Rig conversion cannot preserve requested reasoning effort or
provider reasoning content, so those requests fail before provider I/O rather
than being silently dropped.

Detailed authority rationale and delivery ordering live in
`../../docs/agent-runtime/architecture.md`; dependency and transport evaluation
evidence lives in `../../docs/agent-runtime/rig-evaluation.md`.

## Failure and recovery

Every recoverable input, filesystem, HTTP, stream, subprocess, and provider
failure returns a typed error. Automatic retry is deliberately narrow because
provider processes may have caused non-idempotent side effects.

Linux cleanup signals the process group, waits, drains bounded output, and
fails if escaped descendants retain descriptors beyond the grace period.
Cancellation uses the same cleanup path. Configuration writes preserve the
previous file unless validated replacement completes.

Workspace checkpointing, rollback, diff capture, and promotion are embedding
host responsibilities and remain deferred from this reusable component.

## Security and resource design

Unsafe Rust is forbidden. Commands never use a shell. Final input paths are
checked as regular non-link files and all parsed data has explicit size/count
bounds. Provider children receive null standard input where interaction would
conflict with the controlling setup process.

Direct HTTP rejects ambient proxies, redirects, TLS, DNS names, non-loopback
addresses, oversized request/response bodies, and unbounded streaming. Rig
payload tracing is suppressed and raw provider bodies do not enter canonical
errors.

The broker cannot broaden host grants. Capability states distinguish
advertised, conformance-tested, and policy-enabled behavior. Provider
permission flags are defense in depth rather than proof of isolation.

## Verification strategy

Unit tests cover parsing, placeholder encoding, canonical types, limits,
profile validation, model conversion, and loop detection. Integration tests
cover CLI prompt sources, profile persistence, provider setup, fake
subprocesses, process groups, signal cancellation, output/backpressure,
loopback HTTP, streaming, malformed provider data, and optional Rig parity.
Focused output tests distinguish live text forwarding from captured JSON,
verify that no success trailer contaminates content, and cover reasoning-event
separation, per-prompt profile/model/effort selection, the fixed setup prompt,
failed qualification, and forced persistence.

Conformance tests compare canonical behavior across direct Ollama,
llama-server, and optional Rig adapters. Platform support requires native
process, filesystem, and transport tests. Security audit and independent
compliance review gate promotion of each provider or execution capability.
