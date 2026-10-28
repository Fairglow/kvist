<!-- kvist-design-version: 1 -->

# Sav Design

## Long-running conversations

Represent absent run caps with `Option`, not magic zero values. A prompt
deadline is optional; the owned watcher still polls interrupts without one.
Keep whole-task token charges optional and monotonic. Track unattended turns
separately from total turns; delivery of actual human guidance renews that
allowance and starts fresh generation attempts, but not wall/token accounting
or action history. Count successful compactions in the context manager only
after an atomic preflight/compaction commits.
Account for the transport's 1024-message bound as well as tokens; capacity-based
retention targets 65% of both ceilings. Exact protected messages cannot be
discarded to fit; fail explicitly if they alone exceed the message bound.
Keep duplicate-read advisory notices bounded to the most recent 1024 distinct
observation hashes. Eviction only forgets a notice, never permits a blocked
effect or changes the independent repeated-action circuit breaker.

Journal segments keep record and poll bounds independent of total lifetime.
Reserve space for a segment-link record, create the next private no-clobber
regular file relative to the held directory, synchronize it and its directory,
then synchronize the link in the old file before switching. Followers derive
the next filename from the original name and consecutive index rather than
trusting an arbitrary path in a record. Sequence and run identity span segments.
An interrupted rotation remains incomplete; it never invents success or replay.
Validate any currently available terminal tail before returning a batch even
when `run_end` occupies its final slot; failed followers cannot report a clean
end of input.

## Shared native runtime extraction (ADR 0015)

`runtime::session` owns conversation, shared events and host traits;
`runtime::context` owns atomic preflight/compaction; `runtime::retry` owns
deterministic backoff; `runtime::driver` owns the bounded loop and owned deadline
and operation watchers. `runtime::outcome` is data only, and `runtime::error`
retains typed mechanism failures without application configuration/tool errors.
There are no Skott, maerg, Galla or terminal imports.

One mutex linearizes mailbox submission, claim, dispatch readiness, short
in-memory delivery, and completion. Claimed text is owned by the run; edits and
withdrawals cannot race it back into the queue. Terminal runs discard remaining
guidance rather than enqueueing it as another objective. A separate bounded
withdrawal-notice queue avoids retaining an unbounded history.

Each provider attempt uses a fresh operation token and joined 10-ms watcher.
Only the whole-prompt token reaches the executor. A steering interruption
rebuilds context, retaining consumed tokens and the output reserve. Actual
guidance delivery renews generation attempts. Backoff polls at most every 25 ms. Before the next tool the
driver records paired skipped results for remaining proposals. Recording is
synchronous and delivery is persisted before the next request; failure is
terminal. Shutdown and delivery mutation share a lock, while blocking provider,
recorder and effect operations never run under that lock.

## Shared registry realization (ADR 0015)

The registry uses ordered maps and tagged backend variants. TOML is decoded into
the same serde types used by frontend explicit native overrides. Validation
runs after decoding and before saving; bounded no-follow reads and atomic
same-directory writes reuse profile filesystem helpers. Higher-priority named
profiles replace whole lower-priority definitions, never merge fields.
Resolution retains immutable profile data and hashes exact source bytes; no
discovery is performed. Setup alone performs explicit discovery/qualification.
External executable/argv identity is collected structurally, never recovered
from a command string. Empty capability lists are conservative defaults.

## Loaded-model metadata

Detect llama-server router mode explicitly from `/props.role` and query
`/models` with only the remaining discovery budget. Parse `data` as a bounded
array and consider only `status.value == "loaded"`. Standalone identity uses
the first valid `model`, `model_alias`, or `model_path`; validate identifiers
before returning them. No model-qualified GET or inference is used.

## Selected-model capacity and usage

Direct transport reuses bounded loopback GET for llama-server model-qualified
properties and Ollama loaded-model metadata. Encode the model query as percent
escaped UTF-8, validate positive integer capacity within 1048576 tokens, and
return absent capacity distinctly. Limit discovery to 1 MiB and 60 seconds:
the provider may have to load, or switch to, the selected model before it
answers a bare metadata probe, and a large local model can take minutes.
Streaming OpenAI-compatible requests ask for include_usage, preserving nullable
usage and finish classification. Raise only the complete request byte ceiling
to 8 MiB so larger validated context is not constrained by the old 2-MiB wire
limit; per-field, message/tool count and response bounds remain independent.

## Design overview

ADR 0015 consolidates mechanisms below the applications. Keep registry parsing
and atomic persistence separate from discovery/qualification and from runtime
execution. Extract native conversation, context and retry mechanisms without
moving workspace tool implementations or maerg policy into Sav. A correlated,
bounded control queue owns steering handoff; operation interruption cannot
cancel the whole prompt or reset its accounting. Live event persistence is
run-owned; observation and optional private control are separate interfaces.
External agents use process supervision, not the native model/tool loop.

`offline_rust` contains fixed sandbox-only initialization and runtime script
generation shared by Skott and maerg. Initialization copies generated settings
into tmpfs and links the read-only registration directory. Wrappers resolve
the active toolchain through system rustup or consume an explicit `+name`;
Cargo invokes `rustup run` with offline/locked vendor flags targeting the
caller-provided read-only `/rust/vendor` mount. No host paths or
untrusted script fragments are interpolated.

The crate separates command rendering, prompt acquisition, profile storage and
setup, process supervision, direct model transport, and canonical
provider-neutral types. Reusable mechanisms remain independent of
host policy and durable evidence.

Native model mode is the preferred long-term path because the embedding host
can observe and authorize every proposed effect. External coding agents remain
supported as opaque processes constrained by an outer execution backend.

## Internal structure

`observation` separates a held-descriptor journal/follower from private local
control IPC. Validate full bounded serialization before appending. A writer is
poisoned on I/O failure, synchronizes dispatch/terminal checkpoints and cannot
append after finish. Follow the original descriptor without trusting pathname
replacement; buffer only the bounded incomplete line. The listener owns its
temporary private socket directory, uses bounded per-client waits and an owned
joinable thread, validates requests before invoking the host callback, and has
no tool/model authority. Host callbacks enqueue controls rather than perform
effects. Shutdown stops acceptance and removes the ephemeral capability.

| Module             | Responsibility                                                                                |
| ------------------ | --------------------------------------------------------------------------------------------- |
| `model`            | Canonical messages, turns, tool intent, capability and error types                            |
| `command`          | Shell-free parsing, placeholders, and per-attempt command specs                               |
| `prompt`           | Positional, file, standard-input, terminal, and editor acquisition                            |
| `profile`          | Strict bounded profile parsing and formatting-preserving persistence                          |
| `setup`            | Provider selection, probes, model/executable collection, qualification                        |
| `catalog`          | Bounded HTTP, ACP, and fallback provider-model discovery                                      |
| `supervisor`       | Process groups, forward-or-capture streams, idle/loop/wall/output limits, retry, cancellation |
| `direct_transport` | Bounded loopback Ollama and llama-server HTTP adapters                                        |
| `trajectory`       | Bounded recorded event stream for agent execution trajectories                                |
| `gbnf`             | GBNF grammar escaping for provider tool schemas                                               |
| `interrupt`        | Process-level interrupt registry shared with the maerg supervisor                            |
| `lib` / `main`     | Public library surface and standalone CLI boundary                                            |

The future runtime layers are run coordinator, model transport, native loop,
typed catalog and broker, host policy interface, selected execution backend,
and host-owned durable journal.

## Interactions and state

Opaque external execution reuses the existing supervisor instead of introducing
another process pump. An optional fallible output callback observes accepted
bounded stdout/stderr bytes before presentation; an optional cancellation token
augments process signals. The new single-attempt entry point rejects retries
before spawning. Cleanup is identical on recording failure, cancellation,
timeout, output overflow and child failure.

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

Native registry fields and frontend overrides reuse the direct request's
model-selector validator, so configured native identities cannot exceed the
transport's 256-byte, nonblank, control-free ASCII boundary. External model
identities retain their separate literal-argument bounds.

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

Provider-model selection normalizes all discoverable providers into an ordered
`ModelCatalog` of bounded descriptors and an optional current model. Ollama and
llama-server parse their bounded JSON list endpoints. Copilot and Gemini use a
minimal ACP v1 client: spawn one process group, exchange only `initialize` and
`session/new`, advertise no client filesystem, terminal, or MCP capabilities,
send an empty MCP-server list, correlate numeric response IDs, reject
provider-to-client requests, parse newline-delimited JSON within one deadline
and byte budget, then terminate and reap the complete process group on every
outcome. The external CLI may still initialize its own configured processes,
credentials, filesystem access, or network activity and remains opaque.

`ModelCatalog` contains a provider identifier, nullable validated current model,
and at most 128 `ProviderModel` values with bounded ID, display name, and
optional description. Duplicate IDs retain the first occurrence. Invalid
descriptors fail discovery rather than being omitted silently. Setup appends a
synthetic custom choice only at presentation time, so unadvertised text cannot
be mistaken for a catalog entry. Provider defaults are adapter-owned fallbacks
used only when discovery cannot complete.

Setup qualification is mandatory and uses `Reply with exactly: OK` to minimize
tokens while testing the complete rendered command. Starting setup acknowledges
host execution for only this qualification attempt. Failure returns before
persistence unless the invocation supplied `--force`; the override is
non-interactive and remains visible in setup output. Qualification uses bounded
capture instead of forwarding provider streams, so callers retain ownership of
their presentation boundary. Restricting qualification to a narrower
tool/filesystem authority is deferred with the broader execution backend work.

Direct transports implement the minimal provider wire surface explicitly.
They cannot own tools, persistence, policy, evidence, or public provider
types. A requested reasoning effort fails before provider I/O when the
selected command does not declare the placeholder; transport failure never
triggers automatic replay.

Finish parsing separates the providers' completion conventions. A common
bounded parser preserves explicit reason spellings and maps missing or null
input to the stable unknown sentinel. llama-server uses that parser without
tool-presence inference; its SSE decoder retains the last explicit non-null
reason, and `[DONE]` only satisfies stream framing. A stream without such a
reason returns the same unknown sentinel as unary input. The Ollama-specific
wrapper alone infers `Stop` or `ToolCalls` for absent/null native reasons and
normal native stop. Other explicit reasons and malformed field failures pass
through unchanged. Neither parsing path authorizes tools.

The private streaming body callback reports whether decoding advanced
generation, independently of public event delivery. Decoder progress includes
nonempty text/reasoning and validated tool name/argument fragments; native
Ollama tool records advance it without waiting for terminal intent events.
The body watchdog shares this signal across Content-Length, close-delimited,
and chunked framing. It disables the first-body I/O timer on body acquisition,
starts cadence only on decoded progress, and leaves cadence unchanged for
metadata, comments, empty deltas, and incomplete records. Chunk reads expose
available bounded payload slices rather than waiting for a whole HTTP chunk,
so generation records inside a slowly delivered chunk can advance the timer.
Every read uses the earliest active caller, first-body, or cadence deadline;
framing bytes and trailers are not generation progress.

The optional canonical output-token bound is validated before connecting and
translated into `max_tokens` (llama-server) or `options.num_predict` (Ollama).
No provider capability fallback silently drops a supplied bound.

An optional canonical JSON object schema is validated against a bounded common
provider subset. Ollama sends it unchanged as `format`; for llama-server the
adapter supplies a private typed `response_format.json_schema` extension with
the stable `kvist_output` name. Schema requests are rejected when tools are
enabled because llama.cpp may otherwise defer or ignore one constraint.
Provider-native enforcement is only a generation aid; returned JSON parsing
and validation remain a host responsibility.

Detailed authority rationale and delivery ordering live in
`../../docs/architecture.md`; the historical evaluation evidence
for the rejected Rig experiment is recorded in ADR 0009.

## Failure and recovery

Every recoverable input, filesystem, HTTP, stream, subprocess, and provider
failure returns a typed error. Automatic retry is deliberately narrow because
provider processes may have caused non-idempotent side effects.

The direct transport honors a caller-supplied per-call streaming deadline when
one is provided, clamped to a hard maximum, instead of its configured deadline;
the plain streaming call uses the configured deadline. This lets a retrying loop
extend an attempt's budget across retries — so a turn that merely ran past one
deadline can complete once an attempt has room for the whole generation — while
a plain call keeps its fixed bound.

Linux cleanup signals the process group, waits, drains bounded output, and
fails if escaped descendants retain descriptors beyond the grace period.
Cancellation uses the same cleanup path. Configuration writes preserve the
previous file unless validated replacement completes.

Workspace checkpointing, rollback, diff capture, and promotion are embedding
host responsibilities and remain deferred from this reusable component.

## Security and resource design

Unsafe Rust is limited to necessary, minimally scoped operations with documented
safety invariants, targeted verification, and independent review. The current
interrupt module uses two unsafe `nix::sigaction` calls because that chosen API
requires the caller to establish signal-handler safety. The handler has static
lifetime, performs atomic operations and signal forwarding without allocation
or locks, and installation is attempted once through `Once`. Errors are logged;
successful installation is not guaranteed. Native tests cover idempotence,
interrupt-flag handling, and forwarding to an active process group, not a
general proof of signal safety.

Commands never use a shell. Final input paths are
checked as regular non-link files and all parsed data has explicit size/count
bounds. Provider children receive null standard input where interaction would
conflict with the controlling setup process.

Direct HTTP rejects ambient proxies, redirects, TLS, DNS names, non-loopback
addresses, oversized request/response bodies, and unbounded streaming. Raw
provider bodies do not enter canonical errors.

Catalog HTTP requests reuse the numeric-loopback endpoint parser and
component-owned direct HTTP restrictions rather than curl, while ACP discovery
inherits the explicitly selected provider CLI's host authority. ACP output,
identifiers, display names, descriptions, counts, and elapsed time are bounded;
stderr is discarded, no prompt is sent, and cleanup is unconditional.

The broker cannot broaden host grants. Capability states distinguish
advertised, conformance-tested, and policy-enabled behavior. Provider
permission flags are defense in depth rather than proof of isolation.

Under ADR 0015, the native runtime delegates effects only to the executor
injected by its embedding host; Sav neither selects a runner nor owns grants.
Protected hosts must bind the installed `galla-runner` and its verified
capabilities before dispatch, failing closed when prerequisites are absent.
An explicitly acknowledged opaque host command is a distinct authority mode,
not a protected-runtime fallback. Sav constructs no isolation namespaces and
never substitutes an unconstrained executor when a host executor fails.

## Verification strategy

Unit tests cover parsing, placeholder encoding, canonical types, limits,
profile validation, model conversion, and loop detection. Integration tests
cover CLI prompt sources, profile persistence, provider setup, fake
subprocesses, process groups, signal cancellation, output/backpressure,
loopback HTTP, streaming, and malformed provider data.
Focused output tests distinguish live text forwarding from captured JSON,
verify that no success trailer contaminates content, and cover reasoning-event
separation, per-prompt profile/model/effort selection, the fixed setup prompt,
failed qualification, forced persistence, HTTP and ACP model catalogs,
current-model defaults, custom fallback gating, process cleanup, and
deterministic list output. Negative ACP cases cover wrong response IDs,
unsolicited requests, incomplete initialization, empty or inconsistent model
state, oversized records, timeout, cancellation, and retained descendants.

Conformance tests compare canonical behavior across the direct Ollama and
llama-server transports. Platform support requires native
process, filesystem, and transport tests. Security audit and independent
compliance review gate promotion of each provider or execution capability.
