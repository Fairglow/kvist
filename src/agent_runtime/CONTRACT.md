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

- `setup` for profile creation and update;
- `run` for supervised provider command execution from positional, file,
  editor, or redirected prompt input; and
- `model` for text-only unary or streaming local model requests.

`run` requires exactly one command template or stored profile and explicit
`--allow-host-execution`. `model` exposes no tools and performs no host process
execution.

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
provider/model and request identities, usage, finish reason, capabilities,
runtime events, and typed errors.

Tool descriptors and intents use component-owned JSON Schema-compatible data
shapes where structured validation is required. A descriptor records stable ID
and version, input/output schema, effect class, required capability/resource
scope, timeout/output bounds, and retry/idempotency semantics. No external
schema file is currently the canonical library API.

`{prompt}`, `{prompt_json}`, `{context_files}`, and `{target_directory}` are
the documented command-template placeholders. `{prompt_json}` emits one
complete JSON string value.

## Behavioral guarantees

Quotes group command arguments, but no shell syntax is interpreted. An empty
standalone context placeholder removes its immediately preceding option.
Prompt acquisition accepts exactly one explicit source, otherwise redirected
input, otherwise an optional editor at a terminal.

The supervisor retries only configured idle timeouts and deterministic repeated
output. Nonzero exit, spawn failure, stream failure, invalid input,
cancellation, wall timeout, output exhaustion, and write failure are terminal.
Before a retry it terminates and waits for the process group and supplies a
deterministic warning that earlier side effects may remain.

Profile updates preserve unrelated content, validate complete input and output,
and atomically create or replace the selected regular non-link file. Canonical
and legacy configuration locations are never merged.

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
network authority. The standalone CLI requires explicit acknowledgment and the
library names this mode directly. Context paths do not restrict access.

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
