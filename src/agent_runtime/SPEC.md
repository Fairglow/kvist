<!-- kvist-specification-version: 1 -->
# Agent Runtime Component Specification

<details open>
<summary>Layer 1: Executive summary and public contract</summary>

## Purpose

Provide a reusable Linux-first Rust library and standalone CLI for acquiring a
prompt, rendering a provider command without a shell, supervising its process,
and retrying bounded nonfatal failures with explicit prior-attempt context.
The component is independent of Kvist task queues, roles, component discovery,
and compliance workflow.

## Public contract

The `agent_runtime` library exposes bounded prompt acquisition, shell-free
command-template rendering, typed supervision policy and attempt context, and
supervised host execution. A caller supplies the prompt, command template,
context paths, working directory, and retry policy. Each attempt is built from
fresh typed input; retries receive a deterministic notice describing the prior
failure and warning that prior side effects may remain.

The library also owns reusable named provider profiles, their bounded
formatting-preserving TOML store, provider-specific setup defaults, profile
validation, optional command verification, and terminal-neutral setup
operations over caller-provided input/output streams. A profile contains a
case-sensitive name, provider kind, and command template. It does not contain
Kvist roles, component paths, task state, or compliance policy.

The planned native runtime classifies integrations as native model, one-shot
model, external agent, or plan-only backends. Standalone-owned canonical model,
capability, and tool-intent contracts connect model transports to a bounded
agent loop and typed broker mechanism. Host authority interfaces connect that
mechanism to policy, execution, and durable evidence without importing host
types. Third-party libraries may implement a private transport adapter; their
agent, tool, persistence, and authorization types do not become this
component's public or durable contract.

`agent-run setup` interactively creates or updates a profile in the
Linux user configuration, defaulting to
`$XDG_CONFIG_HOME/agent-runtime/config.toml` or
`$HOME/.config/agent-runtime/config.toml`. When that canonical path does not
exist and the corresponding legacy `supervised-agent/config.toml` exists, the
runtime selects the legacy file so renamed installations retain their stored
profiles; it never merges two stores or falls back from an existing invalid
canonical file. Empty or relative base-directory environment values are
ignored, and resolution fails if neither base is an absolute path. `agent-run
run` accepts
prompt text, `--file`, `--editor`, or redirected standard input; exactly one of
a command template or stored profile; optional context paths and working
directory; idle/loop/retry controls; and the explicit
`--allow-host-execution` acknowledgement. It streams provider output and exits
nonzero on invalid input, unknown profiles, command failure, exhausted
supervision, or refusal.

`agent-run model` performs a text-only unary or streaming request
through the direct local HTTP adapter. It accepts the same positional, file,
editor, or redirected prompt sources; an Ollama or llama-server provider;
explicit loopback endpoint and model; deadline; and response bound. It exposes
no tools and performs no host process execution. Unary output is written after
the complete turn; streaming output is flushed as text deltas arrive.

The component supports Linux only. Other targets fail explicitly rather than
silently providing a different process or filesystem contract.

</details>

<details>
<summary>Layer 2: Architectural guarantees</summary>

## Constraints and invariants

- Rust 1.94 and edition 2024 are supported; unsafe Rust is forbidden. Rust
  1.94 is the upstream-tested compiler for the pinned Rig release and remains
  below the current stable toolchain.
- Commands are parsed and spawned directly without a shell. Quotes group
  arguments, but expansion, redirection, pipelines, and shell operators have
  no special meaning.
- Prompt input must be nonblank UTF-8 no larger than 1 MiB. Prompt files are
  regular non-link files opened without following a final symbolic link.
- Profile configuration is UTF-8 TOML no larger than 64 KiB. Existing
  configuration must be a regular non-link file with `schema_version = 1`.
  It contains at most 128 uniquely named profile tables. Profile names are
  nonblank ASCII identifiers of at most 128 bytes using letters, digits,
  `.`, `_`, `-`, and `:`; commands are nonblank and at most 16 KiB.
- Profile updates preserve unrelated TOML values, comments, ordering, and
  profiles; replace a profile with the same name or append it; validate the
  result; and use same-directory synchronized atomic persistence.
- The idle timeout is positive and at most 3,600 seconds. Automatic retries are
  bounded to at most 10. An optional per-attempt wall timeout is positive and
  at most 24 hours. Combined output is positive and bounded to at most 16 MiB,
  stream transport uses bounded memory, and loop detection retains at most
  4 KiB of UTF-8 output.
- The supervisor retries only idle timeouts and deterministic repeated-output
  loops. A nonzero exit, spawn failure, stream failure, invalid contract, or
  cancellation is terminal.
- Every retry rebuilds the provider command and may append the supplied retry
  notice to its prompt. The notice is advisory: it does not prove that an agent
  inspected prior changes, make operations idempotent, or restore state.
- On supervised termination, the Linux process group is terminated and waited
  for before another attempt starts.
- Output readers use nonblocking polling. If output pipes remain open for one
  second after process-group termination, readers stop and the attempt fails
  terminally rather than hanging; this detects but does not terminate a
  descendant that escaped the process group.
- SIGINT and SIGTERM request cancellation through the supervisor; cancellation
  terminates and reaps the process group before returning a terminal result.
- Host execution requires an explicit acknowledgement at the standalone CLI
  boundary. The library names host execution directly and does not describe it
  as contained, restricted, or sandboxed.
- Host mode inherits the caller's filesystem, credentials, network, and
  executable authority. Declared context files control command arguments only;
  they are not an access-control list.
- Provider endpoint probes are advisory. Profile verification runs the exact
  generated command only after a distinct full-host-authority acknowledgement.
  Failed verification defaults to refusing profile persistence, with an
  explicit save-without-verification choice. Probe URLs are HTTP or HTTPS,
  contain no whitespace or control characters, and are limited to 2,048 bytes.
- CLI-backed providers first probe their conventional executable name with a
  `--version` invocation bounded by ten-second idle and wall timeouts. If that
  probe fails, setup requests an explicit executable or compatible wrapper path
  and applies the same probe.
  Accessibility is distinct from model qualification: only a successful run of
  the exact rendered template proves that credentials, model selection, and
  provider arguments work together. Explicit executable and GGUF paths are
  resolved to stable absolute paths before qualification and persistence.
- Generated llama-cli commands use `--model`, `--prompt`, `--single-turn`,
  `--simple-io`, `--no-display-prompt`, and a bounded `--predict` value.
  llama-cli is inference-only: it cannot inspect context paths, edit files, or
  invoke tools unless a separate agent wrapper implements those capabilities.
- Generated Gemini commands use the installed `gemini` executable,
  noninteractive `--prompt`, text output, and explicit host-agent approval
  mode. Generated Copilot commands use noninteractive `--prompt`, silent
  output, and explicit tool approval. Both collect an optional model selector.
- The following native-runtime invariants are planned contracts. They become
  binding only as their referenced lifecycle tasks are implemented and
  independently reviewed; current supervisor and profile reviews do not treat
  absent future runtime code as a discrepancy.
- Native model transports never execute tools. They return canonical text,
  structured tool intent, usage, finish, provider identity, and error events to
  the component-owned bounded loop.
- Tool intent is untrusted input. Only the component's typed broker mechanism
  may validate and canonicalize it, obtain a host authorization decision, and
  dispatch it through the selected execution backend. The broker emits bounded
  redacted runtime events; it does not create host compliance evidence.
- Capabilities are tracked separately as advertised, conformance-tested, and
  policy-enabled. Unsupported behavior fails explicitly instead of silently
  degrading to a provider's closest feature.
- External agents are opaque processes. Provider permission controls are
  defense in depth; only the selected outer execution backend can enforce
  filesystem, process, credential, environment, and network restrictions.
- Provider libraries remain private implementation details behind
  component-owned types. Arbitrary provider parameters, hosted tools, raw
  transcripts, and third-party serialized state cannot enter host policy or
  canonical evidence.
- The embedding host is the future MCP host and policy boundary. MCP transports
  tools but does not authorize them. ACP may structure an external-agent
  adapter but does not expose or mediate that agent's internal tool loop.
- Generated llama-server commands use `{prompt_json}` for request bodies.
  Generated Ollama commands materialize the selected endpoint through
  `OLLAMA_HOST` rather than depending on ambient endpoint configuration.
- Host-specific configuration, role selection, task transitions, approvals,
  artifact promotion, and compliance evidence remain outside this component.

</details>

<details>
<summary>Layer 3: Detailed strategy and algorithms</summary>

## Design and failure paths

### Supervision and retry algorithm

Validate policy before spawning. For attempt one, build a command with no prior
failure. Capture stdout and stderr concurrently, forwarding bytes to the
caller's terminal. Reset the idle timer whenever either stream produces bytes.
Maintain a bounded stdout suffix for cycle, repeated-line, and alternating-line
loop detection. Output is read in bounded chunks through a bounded channel and
forwarded only while the destination is writable. Exhausting the combined
output budget or blocking the output destination is terminal.

Successful exit terminates any descendants that retained the process group's
output descriptors and returns an execution report. Nonzero exit returns a
terminal error. Idle or loop detection terminates the process group, waits for
it, and either returns an exhausted-retry error or starts the next attempt. The
next attempt context identifies its one-based number, prior failure class, and
a stable warning that files or external systems may already have changed.
SIGINT and SIGTERM are converted to a cancellation flag checked by the monitor;
the same process-group cleanup runs before cancellation is returned.

### Command and prompt handling

The command renderer separates quoted arguments without invoking a shell and
substitutes `{prompt}`, `{prompt_json}`, `{context_files}`, and
`{target_directory}`. `{prompt_json}` emits the complete JSON string value,
including quotes and escaping. An empty standalone context placeholder removes
its immediately preceding option argument. Prompt acquisition selects exactly one explicit source, reads
redirected input automatically, and offers an editor only at an interactive
terminal. Editor selection is `VISUAL`, then `EDITOR`, then `vi`.

### Profile configuration and setup

Version 1 profile configuration has integer `schema_version = 1` and an array
of profile tables:

```toml
schema_version = 1

[[profiles]]
name = "local-coder"
provider = "ollama"
command = "ollama run qwen3-coder '{prompt}'"
```

The loader validates the complete document before returning profiles. The
updater parses and validates an existing document before mutation, updates only
the matching profile table or appends one, validates the edited document and
size, then atomically persists it. Invalid, oversized, link-like, duplicate, or
unsupported configuration remains unchanged.

The reusable setup interaction selects llama-cli, llama-server, Ollama,
Copilot, Gemini, or a custom executable; collects a profile name and editable
command template; and may perform an advisory endpoint probe. Optional model
verification displays the exact host-authority warning and defaults its
acknowledgement to refusal. The standalone command persists the resulting
profile. Kvist may call the same collection API or load an existing standalone
profile, but it materializes only the selected name and command into its own
role configuration so later Kvist execution approval remains bound to exact
command bytes.

For llama-cli, Gemini, and Copilot, setup first runs the provider's conventional
executable name with `--version` under the same bounded process supervisor. If
it cannot be spawned or exits unsuccessfully, setup reports the failure and
asks for a direct executable or compatible wrapper path without changing the
provider kind. Cancellation propagates instead of entering fallback selection.
The fallback must be an executable regular non-link file and must pass the same
version probe. Setup uses maintained provider templates rather than inferring
arbitrary arguments from unstable help prose. It shows the editable exact
template before an optional live qualification prompt, whose wall timeout is
five minutes. Provider children receive null standard input so a headless
command cannot consume setup answers or wait for an interactive response.

### Layered agent architecture and rationale

The component separates responsibilities by authority:

1. A run coordinator binds task contract, policy revision, workspace revision,
   backend profile, budgets, and cancellation to an explicit state machine.
2. A model transport owns only endpoint, credential-reference, request,
   streaming, usage, error, deadline, and cancellation translation.
3. A native agent loop owns bounded turns, context construction, stop
   conditions, malformed-output handling, and tool-result continuation.
4. A typed broker mechanism validates intent, resolves host-approved canonical
   resources, obtains authorization, and requests execution.
5. The host policy engine makes durable decisions over role, task, tool version,
   arguments, resources, environment, credentials, network, and policy hash.
6. The selected execution backend is the only layer allowed to cause local
   effects.
7. A host-owned versioned append-only journal records decisions and terminal
   evidence.

This component owns the reusable provider-neutral protocol: backend classes,
capability states, model messages and turns, tool descriptors, untrusted tool
intents and results, bounded loop state, broker sequencing, execution-backend
interfaces and reusable Linux implementations, host-authority traits, and
redacted runtime events. An embedding host owns policy decisions and
authorization records, approved resource and credential bindings, execution
tier selection, artifact promotion, and canonical compliance evidence. Kvist
is one such host. The dependency is one way: hosts depend on this component,
which never imports host types.

This split is chosen over provider-specific agent implementations because
planning, tool dispatch, recovery, and evidence would otherwise be duplicated
for llama.cpp, Ollama, and each hosted API. It is chosen over a universal
external-agent interface because mature CLIs contain opaque provider-specific
loops that cannot be faithfully or securely reduced to model turns.

Native model mode is the preferred enforceable path. llama-server is the first
llama.cpp transport target because its persistent OpenAI-compatible service is
better suited to multi-turn streaming and tool calls than repeated llama-cli
processes. Ollama and an explicitly approved generic OpenAI-compatible endpoint
follow. llama-cli remains a one-shot and diagnostic backend.

Gemini CLI, Copilot CLI, and other mature coding agents remain useful as
external-agent backends. The embedding host supervises and ultimately
sandboxes the complete process and validates resulting artifacts, but does not
claim to authorize each internal tool call. Plan-only mode makes no effectful
tools available.

The canonical core represents messages, tool intent/results, provider/model
identity, distinct response-scoped and transport-request identities, usage,
finish reason, streaming terminal state, cancellation, typed unsupported
capabilities, and bounded provider extensions. Provider-specific fields are
retained only in explicitly typed, namespaced, approved, and redacted
extensions. Conformance tests, not nominal API compatibility, establish
deployment support.

OpenAI-compatible APIs are model-transport targets, function calling is a
structured intent format, MCP is a brokered external-tool transport, and ACP is
an optional external-agent protocol. None of them replaces embedding-host policy,
execution isolation, or durable evidence. For Kvist as one embedding host, the
supplemental rationale and delivery ordering are recorded in
`docs/agent-runtime/architecture.md`.

### Rig adoption decision

Rig 0.42.0 is approved for a pinned, non-default transport prototype after the
project deliberately raises its MSRV from Rust 1.85 to Rust 1.94. Rust 1.85
was the first Edition 2024 compiler and the prior project compatibility
policy, but Rig directly uses Edition 2024 let-chains stabilized in Rust 1.88.
Rig declares no package MSRV; Rust 1.94 is selected because that exact release
pins and tests its repository with 1.94, while a linked narrow prototype
already builds on current Rust 1.98.

The prototype depends exactly on `rig-core = 0.42.0`, disables default
features, and remains behind a non-default internal Cargo feature. It exposes
only component-owned canonical types and evaluates unary and streamed text
plus structured tool-intent conversion against deterministic fake providers,
local Ollama, and local llama-server. The direct adapter remains the default,
fallback, conformance oracle, and marginal dependency baseline until the Rig
security audit and compliance review approve promotion.

The prototype supplies Rig with a component-owned HTTP client that rejects
redirects, proxies, TLS, non-numeric or non-loopback endpoints, serialized
requests above 2 MiB, and unary or streaming responses above the caller's
bound. It suppresses Rig tracing within the framework call because Rig 0.42.0
contains payload-bearing trace, debug, and error events even when content
telemetry is disabled. Provider response bodies and framework diagnostics do
not enter canonical errors.

The evaluated release contains separate `rig-core` and `rig-agent` crates, but
not the later current-main `rig-run`, `rig-reqwest`, or `rig-rmcp` crate split.
The prototype excludes the `rig` facade, `rig-agent`, Rig tool execution,
provider-hosted tools, arbitrary additional parameters, Rig persistence, and
raw provider payloads in evidence. A later immutable release containing
`rig-run` may receive a separate research spike only after transport reuse
proves worthwhile.

This boundary follows from source-level findings:

- `rig-core` contains credible provider conversion, streaming normalization,
  Ollama support, and llama.cpp-specific compatibility behavior.
- Rig request and message types deliberately retain provider extensions and
  raw material that are too broad for Kvist's canonical policy/evidence model.
- `rig-agent` combines model, memory, hooks, and tool execution across the
  authority boundaries Kvist must keep separate.
- Current-main `rig-run` is promising directional sans-I/O machinery but is not
  part of the evaluated release; its foreign conversational policy and
  explicitly unstable serialized state would still need separate evaluation.
- Rig assumes the host supplies authorization, sandboxing, persistence policy,
  and operational evidence. That philosophy complements a narrow adapter but
  cannot replace the Kvist engine.
- The evaluated Rig 0.42.0 source uses a Rust 1.94 repository toolchain without
  a declared package MSRV; this component adopts that tested compiler as its
  own explicit floor rather than claiming the unverified 1.88 syntax floor.

Promotion beyond the optional prototype requires all gates to pass: locked
Rust 1.94 and current stable builds; private type
containment; approved endpoint and credential routing; bounded cancellation,
deadlines, malformed/truncated stream handling, and redaction; local-provider
text, streaming, and structured tool-intent conformance; TRACE leakage tests;
the objective dependency, license, advisory, TLS, and binary-size gates; and
upgrade tests that detect semantic change. Failure records a no-go decision and
leaves the direct adapter and canonical component transport interface
unchanged. The full evidence and source references are in
`docs/agent-runtime/rig-evaluation.md`.

### Transport dependency gates

Every direct or framework-backed model transport is evaluated on locked Rust
1.94 for `x86_64-unknown-linux-gnu` with explicit features. Candidate and
baseline use the same release profile and toolchain. Target-specific
normal/build packages are counted from `cargo tree --locked --target
x86_64-unknown-linux-gnu -p agent-runtime -e normal,build --prefix none`
after removing repeated markers and duplicate package/version lines. The
unstripped release executable is measured from a fresh target directory.

The first direct adapter is compared with this component before native
transport dependencies. A later framework adapter is compared with the
reviewed direct adapter. The default gates are at most 75 additional packages
and 15 MiB additional executable size. An exception requires explicit human
approval in the decision matrix.

Allowed transitive licenses are MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause,
ISC, Unicode-3.0, Zlib, and CDLA-Permissive-2.0. Any other, unknown, or
unlicensed dependency requires explicit legal approval. `cargo deny check
advisories` must report no unwaived advisory; a waiver records its ID,
reachability, mitigation, owner, expiry, and review evidence.

Local-only builds select no TLS backend and make no HTTPS claim. Hosted
transport requires an explicit Rustls profile and tests for roots, proxies,
redirects, DNS, and destination policy. TRACE sentinel tests must demonstrate
that prompt, tool, credential, endpoint, error, and transcript values do not
enter logs or default events. The decision matrix records exact commands,
lockfile digests, tool versions, features, counts, sizes, license and advisory
results, TLS scope, and approved exceptions.

### Direct local model transport

The first native transport is a synchronous, bounded library API intended for
an execution worker. It performs no blocking I/O in an async task. This avoids
introducing an async runtime before the native loop establishes its concurrency
boundary. A future async implementation may replace it behind the same
canonical request, event, and result types.

The public provider-neutral contract contains:

- `ModelRequest`, ordered role messages, tool descriptors, and tool choice;
- `ModelTurn`, text, untrusted tool intents, normalized finish reason, usage,
  provider kind, model identity, and optional provider request identity;
- `ModelStreamEvent` for ordered text deltas and complete tool intents;
- `CancellationToken` for cooperative cancellation;
- `ModelTransport`, whose unary and streaming methods return typed failures.

The direct adapter supports Ollama `/api/chat` and llama-server
`/v1/chat/completions`. It accepts only `http` endpoints whose host resolves
exclusively to loopback addresses. URLs containing user information, query,
fragment, control characters, non-ASCII text, or an unsupported path are
rejected. The provider path is selected by the typed provider kind rather than
accepted from model output. Redirects are rejected. No credential or ambient
proxy support exists in this local-only adapter.

Requests contain at most 1,024 messages, 128 tools, 8 MiB of canonical message
text, and 2 MiB of serialized JSON. Tool names use nonblank ASCII letters,
digits, `_`, `-`, or `.` and are at most 128 bytes. Tool schemas and tool-call
arguments must be JSON objects. Tool-call identifiers must be nonblank and
unique within a turn. Ollama responses that omit identifiers receive
deterministic turn-local identifiers; they are not represented as provider
identifiers.

HTTP response headers are bounded to 64 KiB, response bodies to the configured
positive limit no greater than 16 MiB, and individual SSE or NDJSON records to
1 MiB. Content-Length, chunked transfer encoding, and connection-close bodies
are supported. Conflicting framing, malformed chunks, malformed JSON,
non-success responses, redirects, duplicate tool identifiers, invalid tool
arguments, truncated streams, a missing terminal record, and unsupported
capabilities fail explicitly. Provider error bodies are never included in
displayed errors.

Unary requests set provider streaming to false. Streaming requests parse
OpenAI-compatible SSE for llama-server and NDJSON for Ollama, emit ordered text
deltas, assemble tool calls before emitting complete untrusted intents, and
require an explicit terminal record. Callbacks cannot authorize or execute a
tool. Ollama rejects required tool choice; llama-server receives explicit
`none`, `auto`, or `required` choice. Parallel tool choice is not exposed.

Deadlines are positive and at most 24 hours. Socket operations use short
timeouts so cancellation and the overall deadline are rechecked. Cancellation,
timeout, endpoint, protocol, response-limit, provider-status, malformed-data,
duplicate-call, and unsupported-capability failures remain distinguishable
without retaining raw prompts, responses, tool arguments, or credentials in
their display text.

### Transport stream callback boundary

The current `ModelTransport` stream callback is synchronous caller code. A
transport checks cancellation and its deadline immediately before and after
each callback, but cannot preempt a callback while it is executing. Callers
must therefore keep callbacks bounded and nonblocking, use a dedicated
execution worker, and treat output backpressure as a terminal error. The
transport must report an elapsed deadline or cancellation as soon as the
callback returns control.

An async or pull-based stream interface that can isolate delivery from caller
work is deferred. It must preserve event ordering, bounded buffering,
backpressure, cancellation, errors, and terminal-turn assembly without
detaching work or allowing callbacks to outlive borrowed state.

The standalone `model` command constructs one user message with no tools and
`ToolChoice::None`. Its deadline is 1 through 86,400 seconds and response limit
is 1 through 16 MiB. Output stream failures are terminal. The command does not
claim that a successful text response qualifies tool calling, a model/template
combination, or the future native agent loop.

### Deferred workspace recovery

A retry warning is viable only as cooperative context. It cannot prevent or
undo duplicated side effects. Copying or archiving files before execution can
aid recovery but must not overwrite concurrent user changes, follow links,
lose metadata, or invoke destructive source-control reset. The preferred
future design is a bounded immutable input snapshot plus a private writable
overlay. A successful attempt yields a reviewed patch or artifact set; a failed
attempt discards the overlay. Archive-and-restore is a fallback only after
conflict detection and explicit user approval.

### Deferred Linux isolation

`fakeroot` changes apparent ownership results for cooperating processes; it
does not remove filesystem, process, credential, or network authority and is
not a sandbox. A restricted dedicated user can reduce discretionary access,
but only when ownership, groups, inherited descriptors, credentials, runtime
files, and network access are also controlled.

Future strict Linux execution should place the provider or its tools behind a
separate backend interface. Candidate tiers are:

1. Snapshot workspace plus explicit host acknowledgement for recovery only.
2. Dedicated uid/gid, cleared environment, resource limits, and process-group
   lifecycle management.
3. Bubblewrap namespaces and read-only mounts, optionally reinforced by
   Landlock and seccomp.
4. OCI with gVisor or a microVM for hostile or multi-tenant workloads.

Tool permission should use a trusted typed broker, optionally exposed through
MCP, rather than trusting provider approval prompts. Hosted-provider access
should be mediated separately from sandboxed tool execution so credentials and
general network authority are not placed inside the agent workspace.

### Deferred platforms

Platform-independent policy and protocol types must not contain Linux syscalls
or path assumptions. Linux process control belongs behind an execution backend.
macOS support requires an independently tested Seatbelt or VM design. Windows
support requires an independently tested AppContainer/restricted-token and Job
Object design, or a VM backend. Unsupported platforms remain disabled until
their guarantees and native CI are approved.

</details>
