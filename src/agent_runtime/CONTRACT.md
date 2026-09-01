<!-- kvist-contract-version: 1 -->
# Agent Runtime Contract

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Boundary and ownership

- **Component ID:** `agent-runtime`
- **Contract IDs:** `agent-runtime.library/v1`, `agent-runtime.cli/v1`
- **Owner:** standalone `agent-runtime` crate
- **Consumers:** Kvist and other embedding Rust applications; standalone CLI
  users

The component owns reusable prompt, command, profile, supervision, model
transport, capability, loop, broker, execution-backend interface, and runtime
event mechanisms. It does not own embedding-host roles, task state, policy
decisions, grants, approved resources or credentials, promotion, or canonical
compliance evidence.

## Provided interfaces

The Rust library exposes bounded prompt acquisition, command-template
rendering, typed supervision policy and attempt context, supervised host
execution, named profile storage and setup operations, provider-neutral model
messages/turns, local direct transports, and optional Rig transport adapters.

The standalone `agent-run` CLI provides:

- `setup [--force]` for profile creation, automatic qualification, and update;
- `run` for supervised provider command execution from positional, file,
  editor, or redirected prompt input, with named profile selection,
  `--reasoning-effort`, and optional `--json` output; and
- `model` for text-only unary or streaming local model requests, with explicit
  provider/model selection, `--reasoning-effort`, `--show-reasoning`, and
  optional `--json` output. Provider reasoning and reasoning effort are
  direct-transport capabilities; the optional Rig adapter rejects those
  requests because it cannot preserve them through its current canonical
  conversion.

`run` requires exactly one command template or stored profile and explicit
`--allow-host-execution`. `model` exposes no tools and performs no host process
execution. Multiple case-sensitive profiles from different providers and
models MAY coexist in one configuration and are selected per `run` invocation.

## Required interfaces

The library requires caller-supplied working directory, context paths,
supervision policy, cancellation, profile storage location or I/O streams, and
provider configuration appropriate to the operation.

The planned native runtime requires embedding-host authority interfaces that
decide over canonical tool, arguments, resources, credentials, policy
revision, and budget. Another application may implement those traits without
depending on Kvist.

## Data and schemas

Profile TOML has `schema_version = 1` and uniquely named profile tables
containing a case-sensitive name, provider kind, and command template.
Provider-neutral model types represent messages, text, tool intent/results,
provider-supplied reasoning, provider/model and request identities, usage,
finish reason, capabilities, runtime events, and typed errors.

Tool descriptors and intents use component-owned JSON Schema-compatible data
shapes where structured validation is required. A descriptor records stable ID
and version, input/output schema, effect class, required capability/resource
scope, timeout/output bounds, and retry/idempotency semantics. No external
schema file is currently the canonical library API.

`{prompt}`, `{prompt_json}`, `{context_files}`, `{target_directory}`, and
`{reasoning_effort}` are the documented command-template placeholders.
`{prompt_json}` emits one complete JSON string value. A requested reasoning
effort fails if the selected command does not declare
`{reasoning_effort}`. An absent effort removes a standalone
`{reasoning_effort}` argument and its immediately preceding option.

## Behavioral guarantees

Quotes group command arguments, but no shell syntax is interpreted. An empty
standalone context placeholder removes its immediately preceding option.
Prompt acquisition accepts exactly one explicit source, otherwise redirected
input, otherwise an optional editor at a terminal.

In plain-text mode, prompt execution forwards bounded provider output live and
does not append a completion message. When standard error is an interactive
terminal, the CLI identifies the initial prompt before provider output. JSON
mode captures bounded provider streams and emits exactly one object containing
the provider standard output as `content`. Captured bytes are decoded as UTF-8
with each invalid sequence replaced by U+FFFD.

Direct model streaming emits answer-text deltas and, when requested and
supplied by the provider, separate reasoning deltas. Plain text writes only
answer content to standard output; `--show-reasoning` writes provider-supplied
reasoning to standard error. `--json` emits one canonical terminal turn and is
mutually exclusive with `--show-reasoning`.
Plain text does not add line termination that was absent from provider answer
content.

The supervisor retries only configured idle timeouts and deterministic repeated
output. Nonzero exit, spawn failure, stream failure, invalid input,
cancellation, wall timeout, output exhaustion, and write failure are terminal.
Before a retry it terminates and waits for the process group and supplies a
deterministic warning that earlier side effects may remain.

Profile updates preserve unrelated content, validate complete input and output,
and atomically create or replace the selected regular non-link file. Canonical
and legacy configuration locations are never merged.

Setup always tests the generated profile with the fixed prompt
`Reply with exactly: OK`. It neither asks for a test prompt nor asks for a
second host-execution confirmation. A failed test does not persist the profile
unless setup was invoked with `--force`; no interactive save-after-failure
prompt is offered. Qualification command output is captured within the
supervision bound rather than forwarded; setup reports only qualification
status.

Model transports return canonical output or explicit typed failure and never
authorize or execute tools. Unsupported capability fails rather than silently
degrading. External agents remain opaque whole processes.

## Errors and failure semantics

Blank, conflicting, oversized, non-UTF-8, non-regular, link-like, malformed,
unsupported, or unknown inputs fail before the relevant provider effect.
Provider and HTTP failures preserve bounded actionable context without
including raw payloads or secrets.

Cancellation, timeout, output overflow, loop detection, destination closure,
and descendants retaining output descriptors run explicit cleanup. A
descendant that escapes the process group is detected as terminal failure; the
component does not claim authority it cannot enforce.

## Security and authority

Host execution inherits the caller's filesystem, credential, executable, and
network authority. The standalone `run` CLI requires explicit acknowledgment
and the library names this mode directly. Setup qualification treats the
explicit setup action as acknowledgement for the exact generated test command;
this convenience is not isolation and does not authorize any later `run`.
Context paths do not restrict access.

Endpoint probes are advisory and bounded. Local transports disable proxies and
redirects, allow only numeric loopback HTTP endpoints, bound request/response
data, and suppress payload-bearing third-party tracing from canonical output.

Tool calls are untrusted intent. Only a typed broker may validate and
canonicalize arguments, request a host decision, dispatch an allowed request
through the selected backend, validate/redact the result, and emit a bounded
runtime event.

## Compatibility and verification

The current public contract is pre-release and intentionally carries no
backward-compatibility commitment. Profile schema and provider-neutral types
remain explicitly versioned before a compatibility promise is made.

Conformance uses deterministic command, profile, setup, supervisor, direct
transport, stream, cancellation, and optional Rig adapter tests. Real provider
promotion requires comparison with the direct adapter, dependency review,
security audit, and independent compliance review.
