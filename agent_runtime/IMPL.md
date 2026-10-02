<!-- kvist-implementation-record-version: 1 -->

# Component Implementation Record

## Basis and limits of this record

This is a newly derived implementation observation, not an implementation
change, intent interpretation, security assurance, or compliance certification.
The observation used only:

- All 15 Rust files in `agent_runtime/src/`.
- All 11 Rust files in `agent_runtime/tests/`.
- `agent_runtime/Cargo.toml`.
- The supplied current-source run log at
  `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/runtime-final-tests.log`.

No intent documents, queues, previous implementation record, prior reviews,
research, other components' implementation, Git data, or other agent output
were consulted. The existing destination was not read or reused. Tests,
providers, and commands described below were **not personally executed** by
this observer. Test source was inspected; execution results are attributed to
the supplied log. A read-only SHA-256 calculation independently matched all
27 allowed source/test/manifest files to their identities in that log.
Matching identities associate this observation with the logged source snapshot;
they do not independently authenticate the log or its execution environment.

The supplied log records:

- Command: `cargo test --locked -p agent-runner -p agent-runtime`.
- UTC start `2026-10-02T00:08:04Z`; UTC end `2026-10-02T00:08:15Z`.
- `rustc 1.99.0 (b940084d7 2026-09-28)`.
- `cargo 1.99.0 (5f94df478 2026-08-27)`.
- Log SHA-256:
  `724b25f2e4a619435ae1aedf8278c6a9eba23ff2a32882d78a64d8ef2ef9e15b`.

Only the runtime results are used as runtime test evidence below. The presence
of another package in the command/log is not a basis for observing its
implementation, its integration behavior, or its assurance.

## Package, exported surface, and execution boundary

The manifest declares package `agent-runtime` version `0.2.0`, Rust edition
2024, minimum Rust version `1.95`, library `agent_runtime`, and binary
`agent-run`. Both library and binary contain a non-Linux `compile_error!`.
The implementation uses synchronous sockets, Unix file flags, process groups,
threads, channels, and process-global signal state; it is not a portable
non-Linux backend.

The manifest lists clap/derive, hex, nix with fs/poll/process/signal, serde,
serde_json, sha2, tempfile, thiserror, toml_edit, and tracing. Tokio is optional,
with macros/net/rt/time features; default crate features are empty. Inspected
transport code uses standard-library TCP, not Tokio or an HTTP framework.
Dependency implementations and the lockfile were not inspected.

`lib.rs` exposes `gbnf`, `loop_detection`, and `trajectory` as public modules,
and re-exports these boundaries:

- `DirectModelTransport`; `ModelTransport`; canonical request, message, turn,
  usage, tool definition/intent/choice, finish, stream-event, provider, effort,
  and cancellation types.
- `CatalogProvider`, `ProviderModel`, `ModelCatalog`,
  `ModelDiscoveryOptions`, and `discover_models`.
- `render_command`, `render_command_with_reasoning_effort`,
  `split_raw_command`, `resolve_prompt`, and `MAX_PROMPT_BYTES`.
- `ModelProfile`, profile path/load/upsert functions,
  `MAX_PROFILE_CONFIG_BYTES`, setup options/collection/wizard functions,
  `verify_profile`, and `SETUP_TEST_PROMPT`.
- `CommandSpec`, `SupervisionPolicy`, attempt/retry/report types,
  `run_supervised`, `run_supervised_capture`, action-ring decisions and
  hashing/similarity helpers/constants.
- Signal-handler installation, interrupt consumption, active process-group
  registration, unconditional clearing, and conditional clearing.
- GBNF schema/tool compilation and trajectory record/replay types/functions.
- Non-exhaustive `Error` and `Result<T>`.

There is no sandbox, filesystem-authority enforcement, tool executor, autonomous
model/tool loop, credential isolation, or network isolation in these inspected
files. Command execution inherits host authority and environment. Tool schemas
and proposals are data, not executable implementations or approval evidence.
Action-ring decisions and trajectory recording are callable mechanisms; the
standalone `run` and `model` paths do not automatically connect them into an
agent loop or journal.

## Canonical model API and wire representation

`ModelRequest` contains `model`, ordered `messages`, `tools`, `tool_choice`,
optional `reasoning_effort`, optional `output_schema`, and optional
`max_output_tokens: u32`. Those three optional fields have serde defaults and
are omitted when absent.

`ModelMessage` distinguishes system text, user text, assistant text plus prior
tool intents, and tool result `{call_id, name, content}`. `ToolDefinition`
contains `{name, description, parameters: JSON Value}`. `ToolIntent` contains
canonical `id`, optional `provider_id`, `name`, and object `arguments`.
Messages and relevant enums use serde's kebab-case variant names; messages
are ordinary externally tagged enums rather than provider-native role objects.

`ToolChoice` is `None`, `Auto`, or `Required`. `ReasoningEffort` has lowercase
`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, and `max`; its string parser
trims and accepts those spellings case-insensitively.

`ModelTurn` contains `text`, optional separately accumulated `reasoning`,
`tool_intents`, `finish_reason`, provider kind, model identity,
`response_id`, `provider_request_id`, and optional `usage`. Reasoning is
omitted when absent. `response_id` defaults to absent during deserialization;
the other optional response fields are not all omitted during serialization.
`ModelUsage` carries unsigned input/output/total counts.

`FinishReason` distinguishes `Stop`, `Length`, `ToolCalls`, `ContentFilter`,
and `Other(String)`. `ModelStreamEvent` is a tagged
`{"type": "...", "value": ...}` enum with text delta, reasoning delta, or
complete tool intent. There is no explicit terminal/finish/usage event: the
assembled `ModelTurn` is the terminal return value.

`ModelTransport` is synchronous:

- `complete(request, cancellation) -> Result<ModelTurn>`.
- `stream(request, cancellation, callback) -> Result<ModelTurn>`, where the
  callback is `FnMut(ModelStreamEvent) -> Result<()>`.
- `stream_with_deadline(..., Duration)`, whose trait default ignores the
  override and calls `stream`.
- `deadline() -> Duration`.

The direct implementation overrides the per-call streaming deadline.
`CancellationToken` is cloneable shared atomic state with `cancel`,
`is_cancelled`, and `reset`; it is cooperative, and resetting one clone
re-arms all clones sharing that state.

### Request validation

The direct transport validates before opening its socket:

- Model: nonblank ASCII, at most 256 bytes. Unlike catalog/setup IDs, direct
  request validation does not restrict the whole value to printable ASCII or
  prohibit braces.
- Between 1 and 1,024 messages; at most 128 advertised tools.
- Combined system/user/assistant/tool-result text at most 8 MiB, with checked
  size addition. Encoded provider JSON subsequently has a tighter 2 MiB limit.
- Explicit output bound from 1 through 1,048,576 tokens; absence preserves
  provider defaults.
- Tool names: 1–128 ASCII alphanumeric, underscore, hyphen, or dot bytes;
  unique advertised names; descriptions at most 16 KiB; `parameters` must be
  a JSON object. This is not full argument-schema validation.
- Prior assistant intents: nonblank non-control IDs at most 256 bytes, valid
  tool names, object arguments, and unique canonical IDs within each
  assistant message. Tool results validate their ID/name/text but are not
  correlated with prior calls.
- Auto/required choice needs at least one tool. Output schema requires
  `ToolChoice::None`; unused descriptors with choice none can remain present.

Output schemas must be object nodes, encoded size at most 256 KiB, recursive
depth at most 32, and at most 4,096 visited schema nodes. Allowed keywords
are `type`, `title`, `description`, `properties`, `required`,
`additionalProperties`, `items`, `enum`, `const`, `anyOf`, `allOf`,
`minimum`, `maximum`, `minLength`, `maxLength`, `minItems`, and `maxItems`.
The validator checks keyword value shapes, recognized type names (including
nonempty type arrays), unique required names present in properties, and
recursive schema children. It does not implement full JSON Schema semantics,
compare lower and upper bounds, or validate returned answer text against the
schema. `const` is accepted without a specific value-shape check.

## Direct HTTP transport and provider encoding

Endpoints must be ASCII `http://` numeric loopback addresses with explicit
nonzero port; bracketed IPv6 is accepted. A trailing `/` is accepted, other
paths are rejected. Hostnames including `localhost`, non-loopback IPs, HTTPS,
credentials, query, fragment, controls, and whitespace are rejected.
There is no DNS, TLS, redirect following, proxy selection, authentication
header, or provider endpoint auto-discovery in this transport.

Constructors validate a positive deadline at most 24 hours and a response
limit from 1 byte through 16 MiB. `new` supplies 15-second slot and 45-second
TTFT settings; `with_watchdogs` accepts explicit positive settings at most
24 hours without requiring their relative ordering. Chainable slot/TTFT/
cadence setters assign durations without repeating constructor validation.
Cadence is absent by default. The default watchdog constants are declared
public inside a private module, not re-exported by `lib.rs`.

Each operation performs one TCP request. A connect attempt is limited to the
remaining deadline or 100 ms, whichever is smaller. Read/write socket
timeouts are 100 ms; retryable timeout/interrupted I/O is polled while
checking cancellation and deadline. There is no transport-level retry of a
whole request or connection. JSON POST requests include content length,
content type, accept JSON, and connection close.

Provider mapping:

| Canonical input | Ollama | llama-server |
| --- | --- | --- |
| Endpoint path | `/api/chat` | `/v1/chat/completions` |
| Streaming selection | `stream` boolean | `stream` boolean |
| Output bound | `options.num_predict` | `max_tokens` |
| Reasoning effort | `think: false` for none, otherwise effort string | `reasoning_effort` string |
| Output schema | `format: schema` | `response_format` with type `json_schema`, name `kvist_output`, strict true, schema |
| Tools | Native function descriptors only when choice is not none | Function descriptors only when choice is not none |
| Tool choice | Required is rejected before I/O; none/auto do not emit a choice field | Explicit `"none"`, `"auto"`, or `"required"` |

No support probing checks whether the selected model honors a reasoning,
output-token, or schema hint. llama-server additionally receives `grammar`
when the local GBNF compiler succeeds; a compiler error silently omits that
field while retaining `response_format`.

System/user messages become native role/content objects. Prior assistant
calls use `provider_id` when available, otherwise canonical `id`; Ollama
arguments remain JSON objects, llama-server arguments are JSON text.
Tool results include content and call identity; Ollama uses `tool_name`,
llama-server `name`. There is no semantic validation that these relationships
match actual prior provider state.

### HTTP framing and bounds

The reader accepts HTTP/1.0 or HTTP/1.1 responses, requires status 200–299,
and returns a status-only error for other responses without incorporating
the body. It supports content length, chunked encoding, and close-delimited
bodies. Duplicate content length, unsupported transfer encoding, conflicting
chunked/content-length framing, short declared bodies, and observed bytes
beyond content length fail.

Header accumulation uses a 64 KiB guard before additional reads, with
4,096-byte reads; discovery shares this code. HTTP chunk-size lines are
bounded to 1,024 bytes; trailers have a 64 KiB cumulative guard. Payload
chunks are delivered in pieces up to 8,192 bytes. Body bounds are checked
before passing accepted bytes to the decoder/callback; chunked payload totals
exclude HTTP chunk framing. Content type is not used to verify protocol kind.

Streaming records are bounded to 1 MiB each; unfinished lines are buffered
until a newline or final body completion. The HTTP body is consumed to its
framing completion even after a provider terminal marker. A terminal marker
alone therefore does not stop a socket whose response framing remains open.

## Provider response parsing and finish classification

### llama-server, unary

Unary parsing selects only the first `choices` entry and requires its
`message` object. Missing/null content becomes empty text; non-string content
fails. Reasoning uses the first present non-null field from
`reasoning_content`, `reasoning`, `thinking`; a wrong type fails and an empty
selected string yields no reasoning.

Tool calls require nonblank bounded IDs, unique supplied IDs, a function
object, valid bounded name, and arguments that are either JSON text parsing
to an object or an existing object. The `type` field is not enforced, and
returned tools need not match an advertised descriptor or its schema. The
unary OpenAI-style parser has no separate 128-call count check.

Body `model` and `id`, if present, must be nonblank, control-free strings at
most 256 bytes. Missing model falls back to the requested selector; body
`id` becomes `response_id`. `provider_request_id` is always absent; HTTP
request-ID headers are not retained.

### llama-server, streaming

The decoder is line-oriented SSE: empty lines and colon-prefixed comments
are ignored; other lines must start `data:`. It removes at most one leading
space after that prefix. It does not implement general SSE field/multiline
event assembly. `[DONE]` is required; subsequent non-marker data fails.

JSON chunks require a choices array; an empty array can carry usage metadata.
The implementation iterates all choices into one accumulated turn rather
than selecting or separating choice indices. Absent/non-object deltas are
skipped. String content is appended and emitted, including an empty
`TextDelta`; non-string content is ignored in this streaming path. Reasoning
is appended/emitted only when nonempty, using the same field precedence as
unary parsing. Within one delta, content is handled before reasoning.

Tool deltas must be arrays with unsigned indices below 128. A BTreeMap
assembles calls by index: ID must not change; function must be an object;
present name/argument fragments must be strings. Names concatenate with a
128-byte bound and arguments concatenate with a 1 MiB bound. Missing IDs,
invalid complete names/arguments, and duplicate completed IDs fail before
any complete tool-intent event is delivered. Complete intents are emitted
in index order only during decoder finish after HTTP body completion.

Model/response IDs and usage retain their latest supplied values. Every
non-null `finish_reason` replaces the remembered finish; null/absent reasons
do not erase a prior explicit one.

### llama-server finish classification

Both paths preserve `"stop"` as `Stop`, even when tool intents exist.
`"length"` is `Length`, `"tool_calls"` or `"tool_call"` is `ToolCalls`,
`"content_filter"` is `ContentFilter`, and other strings become `Other`.
Missing/null terminal reasons become `Other("unknown")`, including `[DONE]`
without any choice and turns containing tools. No inference upgrades an
explicit stop or unknown finish merely because calls exist.

Present reasons must be strings at most 128 bytes without controls;
non-string, oversized, or control-containing values fail. An empty reason
string is not explicitly rejected and maps to `Other("")`.

### Ollama, unary and streaming

Unary parsing requires `done: true` and a message object. Content handling
matches unary llama-server behavior. Reasoning selects `thinking`, then
`reasoning`. No response or request identity is retained.

Streaming uses trimmed NDJSON lines. Blank lines are ignored; each nonblank
record after a `done: true` record fails. Records without message objects can
still carry terminal state. In a message, nonempty reasoning is emitted
before nonempty text; non-string streaming content is ignored. A native
tool array contains complete calls rather than incremental argument
fragments. Calls accumulate across records, with at most 128 in the turn,
and their complete intent events wait until decoder finish.

Native calls require a function object, valid name, and arguments that are
an object or JSON text decoding to an object. Provider IDs are optional;
missing ones produce `ollama-call-N` with the accumulated call offset.
Duplicate supplied provider IDs are rejected within a record and across
stream records. This does not provide a separate collision check between a
generated canonical ID and an explicitly supplied ID of the same spelling.

Both unary and streaming require native terminal completion. Missing/null
`done_reason`, or explicit `"stop"`, means `ToolCalls` when calls exist and
`Stop` otherwise. Other explicit reasons use the same mapping/validation as
llama-server and are preserved, including length, filtering, and arbitrary
provider reasons even when tools exist.

### Usage and terminal authority

llama-server usage, when non-null, requires unsigned `prompt_tokens` and
`completion_tokens`. A valid unsigned total is retained; absent/invalid
total falls back to a **saturating** sum, not the checked sum suggested by
the `ModelUsage` field comment. Ollama returns usage only when
`prompt_eval_count` is an unsigned integer, then requires unsigned
`eval_count` and uses checked addition; overflow fails. Streaming Ollama
reads usage from the terminal record.

The transport can return a successful `Result<ModelTurn>` for length,
filtering, other/unknown, or stop-with-tools. For streaming, it can emit
complete tool-intent events for those finishes. It does not decide whether
the caller should accept an answer or dispatch a tool. Returned answer
content is neither JSON-decoded nor locally schema-checked.

## Time bounds, semantic progress, failures, and cancellation

These are distinct mechanisms, not interchangeable token timers:

1. **Overall deadline.** Direct network execution starts its deadline after
   request validation/encoding. Streaming also establishes an outer decode/
   delivery deadline before execution and checks it during decoding and
   before/after callbacks and final assembly. Its override is capped at
   24 hours. Unary parsing occurs after bounded body receipt without a
   corresponding post-parse cancellation/deadline check.
2. **Slot allocation timeout.** Despite its name/help description, this
   measures waiting for complete HTTP response headers after connect and
   request writes. It does not observe actual provider slot acceptance,
   GPU allocation, or model loading state.
3. **TTFT watchdog.** In streaming, this is initially a body-I/O deadline
   after headers, not a decoded first-token deadline. Initial body bytes
   disable that stage; processing any body fragment clears its first-body
   flag, even without a complete record or semantic progress. Headers
   already received with body bytes can bypass initial TTFT waiting.
4. **Optional cadence watchdog.** This starts/refreshes only when decoding
   accepted data reports nonempty answer/reasoning text, nonempty
   llama-server tool name/argument fragments, or parsed native Ollama calls.
   Tool-ID metadata alone, blank lines, SSE comments, usage-only records,
   role/control records, empty fragments, and terminal markers do not
   refresh it. Incomplete line fragments do not start it before the first
   decoded progress. Once armed, raw traffic without progress cannot keep
   the generation cadence alive.

Before semantic progress, after TTFT is cleared by body data, the overall
deadline remains the applicable bound; there is no additional decoded
first-token timer in that interval. The HTTP framing readers consult the
watchdog deadline around payload, chunk framing, and trailer handling.
Thus a cadence can also expire while waiting for response framing to close,
not only while waiting for model tokens. Cancellation and the overall
deadline remain applicable even when heartbeat traffic continues.

Callbacks are synchronous and cannot be forcibly preempted by this API.
Deadline/cancellation checks detect an overrun after a callback returns;
already delivered events or callback side effects are not undone.
Callback errors propagate, although a cancellation/deadline detected after
the callback can take precedence over its returned error.

The error enum includes typed input/policy/profile/catalog/capability errors,
transport cancellation/deadline/watchdog/status/framing/size/duplicate-call
errors, transport I/O, process failure/exhaustion/limits/cancellation, retained
streams, and filesystem/process I/O with operation/path/source. Malformed
provider JSON errors and HTTP status errors omit provider body contents.
Tracing direct dispatch includes provider and endpoint authority; this is
not universal redaction of all metadata or all error strings.

`Error::is_retryable` classifies overall model timeout and all three
watchdog timeouts; socket reset/abort/broken pipe/timeout/refusal/unexpected
EOF; and HTTP 429, 500, 502, 503, 504, 507 as retryable. Other errors,
including cancellation, malformed data, limits, duplicate calls, request/
configuration errors, and framework errors, are not retryable. This is
classification only: neither direct transport nor the standalone `model`
command performs whole-turn retries or backoff.

## Host subprocess supervision

`SupervisionPolicy` contains idle timeout, optional attempt timeout,
stdout repetition detection, retries after the initial attempt, and a
combined stdout/stderr byte limit. Validation permits positive idle
durations through one hour, positive attempt durations through 24 hours,
0–10 retries, and output bounds from 1 byte through 16 MiB. Although its
idle error text mentions seconds, the code accepts positive subsecond
durations.

`run_supervised` forwards output; `run_supervised_capture` retains it.
Both ask a caller closure for `CommandSpec` per one-based `AttemptContext`.
Programs/arguments are passed directly to `Command`; stdin is null, stdout/
stderr piped, working directory optional, and child process group newly
created. A caller may explicitly choose a shell executable; “shell-free”
means no implicit shell evaluation by this runtime, not a shell prohibition.
Spawn retries executable-busy errors up to ten times with 10 ms sleeps,
separately from policy retries.

Two reader threads use nonblocking pipes, 50 ms poll, 4 KiB chunks, a
16-item synchronous channel, and shared atomic byte reservation. Their
combined accepted bytes cannot exceed one attempt's configured limit.
Readers retry full-channel sends in 5 ms steps until stopped/disconnected.
Cross-stream interleaving is scheduling-dependent; order within each pipe
is retained. Capture buffers are reset for each attempt, so success returns
only that attempt's stdout/stderr and total attempt count. Forwarding has
already exposed prior-attempt output.

Monitoring checks interruption, optional total attempt duration, process
status, and stream events. Any accepted bytes on either stream reset idle.
Only stdout participates in the raw repetition detector, using a lossy
UTF-8 suffix bounded to 4,096 bytes:

- Three identical consecutive suffix blocks of 10–512 bytes.
- Four identical nonblank trimmed trailing lines.
- Three repeats of a two-line nonblank trailing pattern.

Idle timeout and detected repetition are the actual retry events produced
by this monitor. The publicly exposed action-repetition/output-invariant
retry causes and their specialized rejection text are not generated here.
Retries have no backoff. A retry notice for idle/repetition warns that
prior attempts may have changed files or external systems and asks for
reconciliation; no rollback/idempotency enforcement is implemented.

Nonzero exits are terminal and not retried. Output overflow, total attempt
timeout, I/O/callback construction errors, retained pipes, and cancellation
are terminal. Retry exhaustion returns attempt count and cause text.
Attempt duration begins in monitoring, after spawn and pipe setup, and
does not bound the caller's command-construction closure.

After monitoring ends, the implementation sends SIGKILL to the child
process group, waits for the direct child, drains output for up to one
second, stops/join readers, and clears active-group registration. It does
this even after the direct child exits successfully, terminating group
descendants holding pipes. A descendant that escapes the group is not
confined or individually terminated; retained output becomes
`OutputStreamsRetained`. Cancellation/attempt-timeout results are retained
after cleanup rather than replaced by cleanup errors. Other cleanup errors
can take precedence over the monitor result.

Forwarded destinations are polled for writability for one second before
blocking `write_all`/flush. This is not preemption of the entire destination
write. Very early post-spawn error exits before normal cleanup have no
general child-owning drop guard in this supervisor.

### Process-global interrupt mechanism

`install_handler` uses `Once` and separate unsafe `sigaction` calls for
SIGINT and SIGTERM, with `SA_RESTART`. Installation failures log warnings,
not returned errors. The handler sets a process-global atomic flag and
forwards **SIGINT** to the one registered positive process group, even when
the received signal was SIGTERM. This differs from module wording about
forwarding “the signal”; the crate-level “single unsafe block” claim also
differs from the two inspected unsafe expressions.

Supervision consumes the interrupt flag and registers/clears the child
group. There is only one process-global active-group slot, not a per-run
registry; concurrent supervisors can overwrite registration and consume
each other's flag. Conditional clearing is exported, while this supervisor
uses unconditional clearing. Direct model operations poll cancellation
tokens, not this interrupt flag; the standalone model path creates a fresh
token without wiring signals into it.

## Provider catalog discovery

Catalog providers are Ollama, llama-server, Copilot, and Gemini. Default
HTTP endpoints are ports 11434 and 9931 at `127.0.0.1`; default ACP binaries
are `copilot` and `gemini`. `ModelDiscoveryOptions` defaults to five seconds,
64 KiB response, current directory `"."`, no overrides, and no host
acknowledgement. Validation permits positive duration through 30 seconds
and response size through 1 MiB.

Catalog IDs are 1–256 printable ASCII bytes without braces; spaces are
accepted. Names are nonempty control-free UTF-8 through 1,024 bytes;
descriptions may be empty and extend through 4,096 bytes. `ModelCatalog`
retains the first descriptor per duplicate ID in provider order, requires
1–128 unique models, drops a current ID not present in the retained set,
and defaults to current ID or the first entry. JSON has `format_version: 1`,
provider, nullable current ID, and descriptor list; absent descriptions
are omitted.

HTTP discovery GETs `/api/tags` or `/v1/models` using the direct bounded
socket helper. Ollama uses each `name` for ID/display and builds optional
descriptions from family/parameter_size/quantization_level/format strings.
llama-server uses `data[].id` for ID/display without description.
HTTP discovery errors can remain model-transport error variants rather
than being universally normalized to catalog timeout/error variants.

ACP discovery requires `allow_host_discovery` before spawn and checks
pre-cancelled tokens. It canonicalizes the working directory, starts the
selected executable with `--acp`, host environment, piped stdin/stdout,
null stderr, and a new process group. It sends only:

1. JSON-RPC 2.0 ID 0 `initialize`, protocol version 1, empty client
   capabilities; response must confirm version 1.
2. ID 1 `session/new`, absolute cwd and empty `mcpServers`; response supplies
   `models.availableModels` and optional `currentModelId`.

No inference prompt, MCP bridge, tool execution, or provider-to-client
request handling is implemented. Bounded notifications with method but no
ID are ignored. Wrong/missing numeric IDs, protocol errors, JSON-RPC
version mismatch, and client-directed requests fail.

ACP stdout is nonblocking, polled at 50 ms, cumulatively bounded, and parsed
as newline-delimited JSON with 64 KiB records. Its overall response
deadline starts after spawn/configuration; stdin writes/flush are ordinary
blocking operations outside that read-loop enforcement.

An owning `AcpProcess` terminates/reaps its group on success/failure and
also attempts termination in `Drop`. Cleanup drains nonblocking stdout
with a one-second deadline checked even during continuous incoming data.
An escaped descendant retaining/writing stdout can cause retained-stream
failure, not containment. A primary discovery error takes precedence over
a cleanup error; cleanup failure replaces a would-be successful catalog.

Discovery installs the shared signal handler, but ACP does not register
its child as the active group and its read loop checks the supplied token,
not `take_interrupted`. The global interrupt flag is checked after a
successful catalog path. Consequently programmatic token cancellation
is directly polled, while prompt signal cancellation of a stalled discovery
is not established by that same mechanism. The inspected ACP cancellation
test uses a token, not an OS signal.

## Prompt, command, profile, and setup boundaries

### Prompt acquisition

`resolve_prompt` prioritizes explicit string, then file, then editor.
The library does not reject multiple supplied sources; CLI conflicts do.
`file == "-"` reads stdin. Without explicit input, nonterminal stdin is
read directly; terminal stdin asks whether to open an editor, default yes,
or reads until EOF when declined.

All returned prompts must be valid UTF-8, nonblank after trim, and no
larger than 1 MiB, while preserving the original text/whitespace. Readers
take at most limit plus one byte to detect growth. File acquisition checks
the leaf with `symlink_metadata`, opens with `O_NOFOLLOW|O_NONBLOCK`, and
rechecks opened regular-file metadata/size. Ancestor path links are not
walked/rejected. Stdin acquisition has no wall-clock bound.

Editor acquisition creates a tempfile directory and empty `prompt.md`,
selects VISUAL then EDITOR then `vi`, treats an existing executable path as
one program or otherwise splits its command, appends the file path, and
waits for exit. Editor execution inherits stdio and is not supervised for
timeout/group/output. The interactive editor-choice line is not
size-bounded before `read_line`.

### Command rendering

The parser splits on unquoted whitespace, handles single/double quotes,
retains quoted empty arguments, and only unescapes the active quote or
backslash inside a quote. It rejects unterminated quotes. No expansion,
glob, pipe, redirection, environment interpolation, or implicit shell
evaluation is performed.

Only arguments, not the executable, receive `{prompt_json}`, `{prompt}`,
`{target_directory}`, `{context_files}`, and `{reasoning_effort}`
substitution. JSON prompt substitution includes complete string quotes
and escapes JSON controls. The implementation replaces JSON/plain prompt
first, then target, then the selected effort/context branch; it is not a
single-pass opaque-placeholder engine for every inserted value.

An argument with context placeholder expands once per supplied path;
paths are UTF-8-checked when used, not opened or authorized. An exact empty
context/effort placeholder removes a preceding argument starting with `-`;
otherwise the missing value becomes empty. The effort branch precedes
context expansion if both occur in one argument. Explicit effort requires
an effort placeholder in arguments; effort cannot select the executable.
Other unknown placeholders are not rejected.

### Profile store

`ModelProfile` is `{name, provider, command}`, with case-sensitive name
selection. Config is UTF-8 TOML requiring integer `schema_version = 1`
and optional `[[profiles]]`; unknown fields are retained, not rejected.
Maximum file size is 64 KiB, profiles 128, name/provider 128 bytes, and
command 16 KiB. Names allow ASCII alphanumeric plus dot/underscore/hyphen/
colon; providers omit colon. Commands must be nonblank and split
successfully; no required prompt placeholder or recognized-provider
allowlist is imposed.

Default location uses absolute nonempty XDG_CONFIG_HOME, otherwise
absolute HOME plus `.config`. It prefers `agent-runtime/config.toml`.
If that leaf is absent, an existing `supervised-agent/config.toml` is
selected; an invalid or inaccessible canonical store does not trigger a
legacy fallback. This fallback is actual source behavior, independent of
any compatibility claims elsewhere.

Reads reject leaf symlinks/nonregular files, use no-follow/nonblocking
open, recheck metadata, and bound reading against growth. Upsert validates
the entire existing document before changes, preserves comments/unrelated
values/order through toml_edit, reparses the output, writes a same-parent
NamedTempFile, syncs it, persists by replacement or no-clobber creation,
then syncs the parent directory. The final parent must be a real directory;
ancestor links are not traversed with a separate rejection policy. There
is no cross-process locking or conflict resolution for concurrent upserts.

### Setup and live qualification

Setup offers llama-cli, llama-server, Ollama, Copilot, Gemini CLI, and
custom script. Blank/unrecognized provider selection defaults to Ollama.
Setup reads unbounded lines, trims values, and treats EOF, ESC, `cancel`,
or `q` as cancellation.

llama-cli/Copilot/Gemini first run a supervised `--version` probe; a failed
probe offers a direct executable/wrapper path, validates regular/executable
mode and canonicalization, then probes that path too. Cancellation aborts
instead of offering fallback. Probe limits are ten-second idle/attempt,
zero retries, and 64 KiB output.

HTTP/ACP providers attempt five-second/64 KiB catalog discovery and use
numbered selection defaulting to provider current or first model. Other
model ID is explicit. Ordinary discovery failures offer fallback choices:
llama-server `default`, Ollama `llama3.1:8b`, ACP `auto`.
Transport endpoint-validation or model-token cancellation errors abort
instead. Model/profile names remain separate.

Generated command templates use:

- llama-cli: model path, prompt, `--single-turn --simple-io`,
  `--no-display-prompt --predict 4096`, no generic context/tools.
- llama-server: curl with `--disable --silent --show-error --fail-with-body`,
  POST `--json` containing model/user prompt, and the selected endpoint.
  This is a supervised raw command, not canonical turn parsing.
- Ollama: `env OLLAMA_HOST=... ollama run MODEL PROMPT`.
- Copilot: prompt, silent, allow-all-tools, no-ask-user, optional rendered
  effort placeholder, selected model.
- Gemini: prompt, output-format text, approval-mode yolo, skip-trust,
  selected model.
- Custom executable: prompt and optional repeated context paths.

The wizard announces host execution and always calls `verify_profile`
with the fixed prompt `Reply with exactly: OK` and acknowledged host
execution. Verification itself accepts a caller-supplied prompt and
requires its acknowledgement boolean before spawning. Its policy is
30-second idle, 300-second attempt, repetition enabled, zero retries,
64 KiB captured output.

Qualification checks supervised successful exit only. It does **not**
inspect whether the answer is `OK`, nonblank, or structurally correct.
Its “Model test succeeded” message is not evidence of answer semantics,
tool support, schema fidelity, or isolation. A failed qualification
prevents persistence unless `force`; cancellation is not overridden by
force. Profile validity/version probing/discovery input errors still
precede that qualification override.

## Trajectory recording and replay

`TrajectoryEvent` is serde-tagged by `event`, snake_case. Variants carry
session start, turn start, prompt evaluation counts/duration, model
reasoning, tool dispatch with raw args/action hash, tool result with raw
stdout/stderr/exit/bytes/state_mutated, turn finish, and session finish.
These values are caller-supplied. Comments referring to “approved” tools
and “sandbox” observations do not enforce approval or sandbox provenance.

`TrajectoryRecorder` stores a path, attempts parent creation while
ignoring that creation result, then opens append/create and writes one
serialized JSON line per call. It does not provide version headers,
redaction, privacy checks, no-follow opens, schema/state-transition
validation, locking, size limits, atomic replacement, or durability sync.

Replay reads UTF-8 lines, skips blank lines, deserializes events, and
returns identifiers, highest retained turn index, event/tool counts,
optional final success, and retained events. It does not execute tools,
reconstruct live state, or attest recorded mutation/success. There is no
file/line/event-count bound or leaf-symlink rejection in this reader.

`max_turns` stops when the current turn, updated by `TurnStart`, exceeds
the ceiling. Event metadata/count effects occur before that limit check;
other turn-bearing variants do not advance current turn for stopping.
Thus it is a display truncation over expected event ordering, not
validated chronological execution replay. Text CLI replay prints raw
stored strings/JSON without terminal-control escaping.

## Caller-managed action and reasoning loop mechanisms

Canonical action hash is SHA-256 of tool-name bytes followed directly by
recursively sorted compact JSON arguments; observation hash is stdout,
NUL separator, then stderr. Both have `sha256:` plus hex output.
The hash does not inspect filesystem state, exit status, or actual mutation.

`ActionHashRing` defaults to capacity eight, minimum custom capacity one,
base temperature 0.2, and zero stalls. Proposed action equal to the last
recorded action, or a third occurrence counting the proposal in the
retained window, increments stalls. This check uses action hashes alone,
not identical observations despite some surrounding wording.

`record_observation` sets the latest action's observation hash; three
trailing equal present observation hashes increment stalls, otherwise
it resets stalls. It neither requires different action hashes nor accepts
a mutation indicator. A reset is therefore an inference from recorded
output pattern, not measured environment progress.

Reasoning similarity tokenizes on whitespace/ASCII punctuation,
lowercases tokens, and compares n-gram sets by Jaccard; zero n becomes one,
short text falls back to word sets, and two empty texts yield similarity
one. Ring checking compares only the last retained reasoning with eight
token n-grams and threshold 0.88. Similar reasoning increments stalls and
is not added to history; novel reasoning is retained without independently
resetting stalls.

The first stall returns soft correction; stalls two/three return
temperature-jitter data (`min(base + 0.5, 2.0)`); four or more returns
circuit-breaker data. These decisions do not change an actual model
temperature, terminate execution, or guarantee changed state unless a
caller acts on them. Custom capacity and retained string sizes have no
hard maximum here; repeated rejected checks can continue incrementing
the unsigned stall counter. `clear` removes action/reasoning history and
stalls while retaining capacity/base temperature.

## Standalone CLI behavior

`agent-run` parses five subcommands with clap. Application errors print
`error: ...` to stderr and return failure; success returns success.

- **`model`**: optional positional prompt, `-f/--file`, or `--editor`
  (mutually conflicting); required provider ollama/llama-server, endpoint,
  model; stream, timeout, response size, effort, output schema, JSON, and
  show-reasoning switches. Defaults are overall 300 s, slot 60 s, TTFT
  120 s, response 1 MiB. CLI requires slot < TTFT < overall, unlike the
  library constructor. There is no CLI cadence option, output-token
  option, tool descriptor option, or whole-turn retry option.
  It constructs one user message with choice none, no tools, no output
  bound. Plain output preserves provider answer text without appending a
  newline; streaming flushes text deltas, suppresses tool intents, and
  optionally sends reasoning to stderr. Unary reasoning adds a stderr
  newline. JSON emits one complete canonical turn plus newline and
  conflicts with show-reasoning; JSON streaming ignores interim events.
  TTY stderr can display the initial prompt; JSON paths do not do so.
  Finish classification is returned/presented, not converted into CLI
  failure merely for length/filter/unknown/empty text.
- **`models`**: required provider; optional endpoint/executable,
  allow-host-discovery, timeout (5 s), response size (64 KiB), JSON.
  HTTP providers reject executable overrides; ACP providers reject
  endpoint overrides. llama-cli/custom-script spellings are parsed but
  return unsupported catalog capability without spawning. Text prints
  IDs in retained provider order, JSON one catalog plus newline.
- **`run`**: prompt-source options; exactly one command/profile; optional
  config/context paths/working directory; idle timeout (900 s), loop
  switch, retries (3), output (1 MiB), effort, JSON. Host acknowledgement
  is required before prompt acquisition. Attempt timeout is `None` and
  has no CLI flag. Each retry appends the side-effect warning to the
  prompt before rendering. Plain mode forwards both provider streams;
  JSON captures them and emits only `{"content": ...}` from successful
  stdout, replacing invalid UTF-8 lossily. Captured stderr is not put in
  that JSON; supervisor notices may still appear on stderr.
- **`setup`**: optional config and force flag; resolves current directory,
  runs the interactive wizard with locked stdin/stdout, and persists.
  It does not require the run/discovery acknowledgement flags, instead
  announcing and internally acknowledging its qualification/discovery.
- **`replay`**: journal path, optional max-turns, JSON. JSON emits the
  replay report; text describes session, counts, and selected event
  fields including reasoning/tool args but not full tool stdout/stderr.

## Inspected tests versus supplied execution evidence

All test files and the three source unit-test modules were inspected.
Network tests primarily use synthetic loopback listeners, not installed
model servers. ACP/setup/CLI supervision tests use generated shell
providers or ordinary host commands. Some setup tests invoke curl against
fake endpoints. Provider flag strings in fixture assertions do not show
real installed Copilot/Gemini/llama model compatibility.

The supplied runtime run names the following suites and results, all with
zero failed, ignored, measured, or filtered tests:

| Runtime target | Logged passed tests |
| --- | ---: |
| Library unit tests | 7 |
| Binary unit tests | 0 |
| `tests/catalog.rs` | 11 |
| `tests/cli.rs` | 25 |
| `tests/command.rs` | 6 |
| `tests/gbnf.rs` | 9 |
| `tests/loop_detection.rs` | 11 |
| `tests/model_cli.rs` | 2 |
| `tests/model_transport.rs` | 44 |
| `tests/profiles.rs` | 4 |
| `tests/setup.rs` | 9 |
| `tests/supervisor.rs` | 9 |
| `tests/trajectory.rs` | 3 |
| Runtime doc tests | 0 |
| **Runtime total** | **140** |

Named tests and inspected assertions provide these specific evidence groups:

- **Finish semantics**: unary/stream explicit stop-with-tools remains stop;
  missing/null llama finishes remain unknown with/without tools;
  terminal-marker-only unknown; later null/absent deltas preserve explicit
  finish; malformed reason types/controls/oversize fail; native Ollama
  stop/absence follows its call convention; both providers preserve
  length/filter/other finishes alongside intents.
- **Request mapping**:
  `model_request_output_bound_has_optional_canonical_wire_shape`,
  `direct_transports_encode_output_bounds_and_preserve_absent_defaults`,
  `invalid_output_bounds_fail_before_connect_for_both_providers`,
  `direct_transports_map_every_reasoning_effort_value`,
  `direct_transports_encode_provider_native_output_schemas`,
  and `output_schema_rejects_non_objects_and_callable_tools_before_io`.
  Assertions cover boundary output limits, no-connect rejection,
  every effort spelling, schema encoding, selected malformed/unsupported
  schema forms, and callable-tool conflict.
- **Streaming/parsing**: named tests cover early delivery before terminal,
  chunk-split records, separate reasoning, fragmented tool arguments,
  required native terminal, truncated/error SSE redaction, duplicate
  supplied call IDs before intent delivery, malformed tool deltas without
  repair, output limits before events, and cancellation after a delta.
- **Watchdog remediation fixtures**:
  `cadence_ignores_heartbeats_empty_control_and_usage_frames_in_every_framing`,
  `reasoning_and_native_tool_progress_start_cadence_before_intent_delivery`,
  `cadence_does_not_start_on_slow_fragmented_initial_records`,
  `cadence_tracks_tool_fragments_and_reasoning_before_late_tool_events`,
  and `caller_deadline_and_cancellation_still_bound_heartbeat_streams`.
  These inspect both providers and content-length, multiple chunk, one
  large chunk, and close-delimited framing where applicable. They assert
  cadence expiry despite metadata traffic, successful delayed fragmented
  initial records before semantic progress, cadence refreshed by actual
  reasoning/tool generation, and overall/cancellation authority.
  Separate named tests cover stalled headers, stalled initial stream,
  hung cadence, caller deadline override, and deadline after finish callback.
- **Catalog/ACP**: descriptor bounds/dedup/version, invalid/empty models,
  HTTP paths/details, response correlation/absolute cwd/no MCP bridges,
  ignored notifications, acknowledgement before spawn, wrong IDs/client
  requests, malformed/oversized state, inconsistent current model,
  token cancellation/timeout reaping, retained stdout, and
  `acp_cleanup_deadline_applies_while_a_descendant_keeps_writing`.
- **CLI/profile/setup**: host acknowledgement, deterministic text/JSON
  catalogs, manual unsupported providers, prompt files, JSON capture/lossy
  UTF-8, null child stdin, legacy/default path precedence, per-prompt
  effort, JSON/reasoning conflict, fixed qualification prompt/refusal,
  forced persistence, setup-to-run profile selection, llama-wrapper
  fallback flags, synthetic Gemini/Copilot templates, ACP current model,
  failed version fallback, cancelled probe, group interrupt, and replay
  text/JSON. Profile tests cover create/load, preserving comments and
  unrelated values/profiles, invalid-name rejection, and unchanged invalid
  existing config. Setup tests cover URL rejection, qualification
  acknowledgement before spawn, JSON prompt escaping, numbered discovery,
  fallback, numeric ID, invalid selection, brace rejection, selected
  Ollama endpoint.
- **Supervisor**: success attempt count; retry context side-effect warning;
  no retry for exit/output overflow; bounded retries; idle/attempt timeout;
  same-group descendants after parent exit; escaped descendant retained
  pipes without hanging. Inspected total-attempt test continues emitting
  bytes, distinguishing its bound from idle timeout.
- **Command/GBNF/action/trajectory**: command substitutions/empty option
  removal/quote errors/JSON escaping/explicit effort; GBNF string/enum/
  primitive/nested/object/tool grammar text and rejection cases; hash
  normalization, action/window repeats, output invariants, reasoning
  similarity, decision escalation/reset; record/replay/full/capped events
  and missing/malformed journal errors.

The GBNF tests assert generated string contents, not grammar acceptance
by a provider or enforcement of full schema validity. The compiler emits
fixed sorted all-required object sequences, but its mixed-optional object
rule allows zero/repeated declared properties and does not guarantee
required-property presence. It does not enforce numeric/string/array
bounds, const, composition semantics, or general additional-properties
policy. Public compilation has no independent depth/size guard despite
its “bounded JSON Schema” comment. Direct output-schema validation and
GBNF compilation are separate mechanisms with different supported shapes.

One inspected source unit test,
`streaming_without_terminal_record_fails`, obtains `expect_err` but leaves
its subsequent `matches!` expression unasserted; its logged pass proves an
error occurred, not that particular error variant. Integration tests have
additional asserted malformed/terminal checks.

No separate formatting, lint, release build, live provider qualification,
non-Linux run, stress/concurrency run, complete schema-validation run, or
security/compliance result is established by the allowed log. The logged
passing fixtures do not eliminate the source limits above or turn caller-
supplied provenance strings into evidence.

## Snapshot identities

All entries below matched independently computed current-file SHA-256
against the supplied log. Paths are relative to this component.

```text
78cae51d7ecda008561d74914abff7c73ee4a97075559755d5a438520be32b00  src/catalog.rs
3fcec7dd7f7c6b8d367a9b8c98b89fb6da179d702e40ce232d1eb79224c5b6d8  src/command.rs
f516115f3ad8cab902926ce2733c15a6116565abfb684f9cf4bfd9c471ddb830  src/direct_transport.rs
d45c94fc4615f618f3e353eae37504d0b69aa659c43ef7418e5f7634ade2c5c6  src/error.rs
359631d6fa489491b8dce8b48d870376fda3649be52544c697fdab3c49518fb4  src/gbnf.rs
c93795c89104a70a2d1ae7d48780352d0452bb4b53a29692e89b9f2cd5c2a75b  src/interrupt.rs
98131869e531e15cb8574d7588d3e32db77439b1ebc5e0e67b163b1868f1a1ba  src/lib.rs
23fc3a0f0e8e5519d1b2243a2767c796a4bd532824b907f21d37da2576f69284  src/loop_detection.rs
a99a224c00e25d4bdd8f2b5befb00ee19e8e54603167fee166b785d19ac518ce  src/main.rs
ba968d12a35da6849541795acefced35ac896ed0adb4e5bb4d768235f7265bfd  src/model.rs
189e742f1c5c429dcce56efbce3910f4067e7c12b2b0bad523d63fbf5d199b21  src/profile.rs
f66da509d224409f879ac64b801c2da1bb9e0a2e26362835163a31a14f5b8ad1  src/prompt.rs
4a12d4c0102129b5841b7ef3a5a3deff204fdebc7d2bc6d5db91a51d96ac117c  src/setup.rs
2859a30465af6188ed7bbf12e4029dd9d6d08ba1e0a1cde17fffd889ff9b273a  src/supervisor.rs
8b25ddc84777f11d7bcce0ecbc2eb6c132e890429deacab785979d64a89f1d1b  src/trajectory.rs
0a764dcbcb95c7688df01dc10b2b781da78cc395ce6e67f5395485c406afc93d  tests/catalog.rs
be5e3ec2c87bc1db1534ff2c82066ab7e870a63f2996e1d91dc541f7fc2f79f8  tests/cli.rs
ba1db40df1316134d5181dde20cc88771c169ff4908d67dfbd639abf6e2761ba  tests/command.rs
85d0c1fb24b2a86853fc32abc6a9cf08d43dee977b851d8a4edb275ffe12f2b5  tests/gbnf.rs
090ef9c7a36836f3b2e56ada040aca2f5194dffe151f2ad93b35b1df54f39348  tests/loop_detection.rs
5817235d1e6993e29ed035123f7aa4b4d6963024a34f63fce4135d7da79fc1ca  tests/model_cli.rs
0f3db42035da34bad8174b59f8361d381ac99a11940708b0b54d8ddb1b1d6419  tests/model_transport.rs
0b1a43fe3ec48733f448bc41167af3512cd85a63657dbf83bf3ec3fb954f5046  tests/profiles.rs
1d0dfcc042651a6ae9aa40861b72b010e872452e3bdb191430cbafec489c547e  tests/setup.rs
a4ae036753b4763f0118838f1342a02dcf4bb44029e4e527af40b454505ff2ba  tests/supervisor.rs
096e5beae105dd6007302d9e20f35306c990cbebcfbef005c4693b654ff45aad  tests/trajectory.rs
dc7f053c7f3c60f2671e48e90276c5c66402666bc0232605c27785eabf9c9da1  Cargo.toml
```
