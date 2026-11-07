<!-- kvist-design-version: 1 -->

# Sav Design

## Native Cargo cache scripts

Remove unconditional source replacement from both wrapper variants. Retain only
offline/locked selection plus fixed optional deliberately staged export config.
Initialization creates private Cargo administration under caller-selected
`CARGO_HOME` (default sandbox scratch `/tmp/cargo-home`), then creates links to
present fixed registry/Git data mounts. The fragment is caller-trusted sandbox
initialization, not acquisition or host path selection. No source data is copied.

## Workspace offline wrapper

Render both public wrapper variants through the same private generator with a
locked-resolution boolean. Only workspace Cargo/Clippy omit forced `--locked`;
selection, literal arguments and deliberately staged export configuration remain.
Protected wrappers retain forced `--locked`, with native sources by default.
The workspace launcher path is `/rust/rustup`, supplied as a live read-only
file grant by the host; the protected launcher path remains `/usr/bin/rustup`.

## Fixed offline source configuration

Only the Cargo and Clippy wrappers conditionally add the fixed
`/rust/runtime/vendor-config.toml` argument when the host deliberately stages
portable-export configuration. Use a shell
array for the literal optional argument; no environment/model path or shell
interpolation constructs executable fragments. Preserve the offline/locked arguments and selection behavior without a default
crates.io replacement.

## Resilience realization

Deliver four independently checked phases: outcome correctness; streamed
repetition/recovery; progress-aware action/observation protection; and typed
diagnostics/resource support. Host authority remains outside these mechanisms.

Keep stream detection incremental and deterministic, with bounded normalized
word fingerprints, separate answer/reasoning state and exact/near periodic
suffix comparisons. Three substantial repeated periods are strong evidence;
short phrases need additional repetitions. Bound token length/history and
candidate work, and retain structural distinctions rather than deleting code
or numbers. Inspect before presentation and before accepting unary results. Retain at most
2,048 word fingerprints per channel and at most eight prefix characters of one word.
Inspect periods up to 512 words; substantial periods (50+) require three
copies, shorter periods require at least six and 48 total words. Near matches
require at least 94 percent token identity and sufficient token diversity.
Before enlarging a length-limited generation, two exact substantial prose
copies are enough for a conservative repetition warning and bounded correction;
this terminal-only rule does not apply to structured output or near matches.
Apply near matching only to prose, not fenced code, quoted/list/table/heading
structure; exact matching still inspects structured content. Preserve numeric
distinctions: changed numeric-bearing words are ineligible for near matching.
Keep bounded quote/inline-code, indentation and fence-delimiter state across
lines; an internal shorter fence cannot end a longer code fence, and a closing
fence line must contain only the marker and whitespace. Inspect on an eight-word
cadence plus terminal flush, independent
of chunk boundaries. Bound near candidates to 16 per inspection.
Terminal near inspection spreads disjoint anchors so cosmetic changes at the
three fast streaming anchors do not defeat the final check. Poll a host control
predicate at most every 256 Unicode scalars, including complete/unary fallback
inspection; latch interruption and recheck host stopping causes before
presentation or acceptance. Known usage increases a conservative token debit
when larger than the reservation; unknown or smaller usage cannot refund it.
Also retain at most 2,048 Unicode scalars of the current unfinished word:
long unbroken character loops require at least 1,024 scalars and strong exact
periodicity. This avoids waiting for whitespace/resource exhaustion without
retaining unbounded tokens. Character checks exclude recognized quoted/code/data
payloads, including unbroken bracketed JSON and XML-like text; arbitrary text
cannot be semantically classified without a deliberate host exception.
Bound action/outcome history to 128 proposals and cycles to 32 actions.
These are tested heuristic windows, not universal semantic detection.

Retain the prompt's two corrections outside compactable messages. Progress
clears streaks but never replenishes spent corrections; only a new human prompt
does. Record
correction before adding bounded host-authored guidance. A batch produces one
intervention, complete skipped-result pairing and one response opportunity.
Reconcile every returned attempt before control precedence; retain a completed
turn record even when guidance supersedes it. Accumulate usage once per attempt,
including length retries, and latch unavailable provenance for unknown failures.
After publishing a correction, append it before the next iteration delivers
newly arrived human guidance. Do not charge an undelivered correction merely
because guidance arrived during publication.
Track proposals even when rendering/policy rejects them. Observe typed status
and bounded content fingerprints, not claims of filesystem mutation from stdout.
State revisions and new observation ranges permit useful repeated actions;
unchanged observations and repeated failure/cycle signatures remain evidence
of stagnation. No-op success cannot clear that evidence.

The progress-aware tracker retains action and fully framed outcome fingerprints,
including status, cancellation, timeout and stdout/stderr boundaries. Three
identical same-action observations or failures qualify as stagnation; two exact
action/outcome cycles of length 2-32 qualify as recurring cycles. Nonrepeatable
proposals are checked before dispatch, repeatable observations after execution.
Only host-validated state change clears recurrence history; new observations
remain usable but do not fabricate mutations. Keep one report per batch, pair
all skipped proposals, then record and append one correction from the same
prompt recovery budget used by generation detection.
Inspect answer/reasoning continuity across accepted tool turns before accepting
their proposals, using the same bounded stream primitive. New complete
observations or validated state changes clear this continuity; no-op output
does not. Inspection and clearing never replenish spent correction opportunities.
Transactional continuity uses one same-sized bounded rollback snapshot in
addition to active detector state. Repetitive provisional text restores the
accepted snapshot instead of biasing the next corrective response.

Generation detection defaults on, with an explicit host-owned exception for
intentional repetition. This exception cannot disable tool repeat protection,
authority, recording or budgets. Operator-facing configuration follows in the
operational phase rather than interpreting model text as an opt-out.

Latch the first observed stopping cause before fallible/slow cleanup. A
deadline expiring during cancelled-run cleanup cannot relabel earlier user
cancellation. Diagnostics identify explicit wall/token/turn allowance rather
than guessing OOM or provider slot state from a signal or missing exit code.

Operational types live beside the runtime outcome types. Derive bounded
printable tool diagnostics from explicit refusal/error domains and outcome
flags, not inferred OOM or stdout success claims. Retain stderr (or bounded
stdout when no stderr exists) with an actionable typed recovery recommendation.
Pair completed/refused outcomes before diagnostic publication; observation
failure after dispatch reports possible effects and never authorizes replay.
Keep dispatch and observation errors distinct so a hard execution cause wins.

Freeze a stop snapshot at loop exit before cleanup; deadline watchers observe
existing cancellation before assigning expiry. Add a typed resource to every
explicit budget rejection and preserve it when the inner loop folds an error.
Move final dirty usage publication outside the fallible inner-loop body.
Resolve the shared stopping snapshot before that final publication as well as
cleanup, retaining hard/progress/stall causes on secondary evidence failures.
Always attempt terminal presentation after terminal-record failure, retaining
original and cleanup/recording causes. Successful inference cannot conceal
failed required evidence.

The bounded mailbox retains claimed identities and actual context-delivery
state until final notice completion. Cleanup drains outstanding dispositions;
final notice presentation is attempted even when its durable callback fails.
No subsequent inference/effect may follow a failed required callback. This
does not repair or fabricate a broken journal.

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

Capacity fixtures model llama-server's initial `/models` request separately
from the `/props?model=` fallback after HTTP 404. Ollama fixtures accept only
`/api/ps`, without a llama-server preflight. Assert exact routes before replying;
keep missing and invalid selected-model metadata distinct.

Native profile parsing and construction default request deadlines to 7200
seconds and validate 1–86400 seconds, matching direct transport's 24-hour
maximum. Preserve `min(profile_deadline, host_remaining)` construction and
unchanged cadence, retry and byte bounds; perform validation before I/O.

Direct transport reuses bounded loopback GET for llama-server router model
capacity, falling back only on HTTP 404 to model-qualified properties, and for
Ollama loaded-model metadata. Encode the model query as percent
escaped UTF-8, validate positive integer capacity within 1048576 tokens, and
return absent capacity distinctly. Limit discovery to 1 MiB and the caller's
request deadline (at most 24 hours):
the provider may have to load, or switch to, the selected model before it
answers a bare metadata probe, and a large local model can take minutes.
Represent the response-header watchdog as optional: absent for metadata and
unary generation, present for streaming inference. Bound all stages by the
overall deadline. Report missing response headers rather than claiming to
know provider slot occupancy; prefer the overall timeout when it expires first.
Classify socket EOF before headers, declared Content-Length, or HTTP chunks
complete as `ModelTransportIo(UnexpectedEof)`, preserving safe host retry
classification. Complete but invalid protocol records remain malformed.
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
Cargo invokes `rustup run` with native offline source resolution, adding
`--locked` only in the protected wrapper. Private Cargo administration links
to caller-provided read-only native registry/Git mounts. Directory-source
configuration is consumed only when deliberately staged for a portable export.
No host paths or
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
