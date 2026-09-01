<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Observation basis

This record was derived from the component's Rust sources, integration tests,
component `Cargo.toml`, and the workspace `Cargo.toml`. It describes observed
implementation behavior and static test evidence. No test command was run while
producing this record.

## Build and platform shape

- The package is `agent-runtime` version `0.1.0`, Rust edition 2024, with
  `rust-version = 1.94`.
- It builds the `agent_runtime` library and the `agent-run` binary.
- Both library and binary reject non-Linux targets at compile time.
- The crate forbids unsafe code.
- Default features are empty. Optional feature `rig-transport` adds the
  Rig/Reqwest/Tokio transport and exposes `RigModelTransport`; it also adds the
  CLI `--transport rig` value.
- The library exports prompt resolution, shell-free command rendering, profile
  storage/setup, model catalog discovery, direct and optional Rig transports,
  canonical model types, cancellation, and host-process supervision.
- The crate describes supervision as host execution, not as a filesystem,
  credential, network, or general sandbox boundary.

## `agent-run` command surface

The binary uses Clap and has four subcommands: `model`, `models`, `run`, and
`setup`. Successful domain execution returns exit status 0. Domain failures are
printed to stderr as `error: <displayed error>` and return failure status.
Argument conflicts and missing required arguments are handled by Clap.

### `agent-run models`

Observed arguments:

- Required `--provider`: `ollama`, `llama-server`, `copilot`, `gemini`,
  `llama-cli`, or `custom-script`.
- Optional `--endpoint HTTP_LOOPBACK_URL`.
- Optional `--executable PATH`.
- Optional `--allow-host-discovery`.
- `--timeout`, default 5 seconds.
- `--max-response-bytes`, default 65,536.
- Optional `--json`.

Provider behavior:

- Ollama performs bounded HTTP GET discovery at `/api/tags`, defaulting to
  `http://127.0.0.1:11434`.
- llama-server performs bounded HTTP GET discovery at `/v1/models`, defaulting
  to `http://127.0.0.1:9931`.
- Copilot and Gemini perform one ACP subprocess session, using `copilot` or
  `gemini` by default, or the supplied executable.
- `--executable` is rejected for the HTTP providers.
- `--endpoint` is rejected for the ACP providers.
- Copilot and Gemini require `--allow-host-discovery` before any subprocess is
  started.
- `llama-cli` and `custom-script` are accepted CLI provider spellings but have
  no model-catalog implementation. They fail immediately with
  `UnsupportedCapability`, respectively displayed as:
  ``unsupported capability `provider model catalog` for provider `llama-cli```
  and
  ``unsupported capability `provider model catalog` for provider `custom-script```.
  Provider conversion happens before current-directory lookup or discovery, so
  an executable supplied with either unsupported provider is not spawned.
- Text output writes one model ID per line in retained provider order. JSON
  output writes one newline-terminated catalog object.

Static CLI evidence includes
`models_http_text_and_json_outputs_are_deterministic`,
`models_reports_manual_providers_as_unsupported_without_spawning`,
`models_acp_requires_host_discovery_acknowledgement_before_spawn`, and
`models_acp_lists_correlated_provider_ids` in `tests/cli.rs`.

### `agent-run model`

Observed arguments:

- Prompt may be positional, `--file/-f`, or `--editor`; these conflict.
- Required `--provider`: `ollama` or `llama-server`.
- `--transport`, default `direct`; `rig` exists only with `rig-transport`.
- Required `--endpoint` and `--model`.
- Optional `--stream`.
- `--timeout`, default 300 seconds.
- `--max-response-bytes`, default 1,048,576.
- Optional `--reasoning-effort`: `none`, `minimal`, `low`, `medium`, `high`,
  `xhigh`, or `max`.
- Optional `--show-reasoning`, conflicting with `--json`.
- Optional `--json`.

The command constructs a single user-message request with no tools and
`ToolChoice::None`.

- Direct non-streaming mode returns provider answer text exactly on stdout.
- Direct streaming mode forwards text deltas to stdout and flushes each delta.
- Provider reasoning remains separate from answer text. With
  `--show-reasoning`, direct transport reasoning is written to stderr; otherwise
  it is suppressed in text mode.
- JSON mode emits one newline-terminated canonical `ModelTurn`. Streaming JSON
  consumes stream events silently and emits only the assembled terminal turn.
- When stderr is a terminal, non-JSON model execution prints the initial prompt
  and `Response:` heading to stderr. It does not do so for redirected stderr.
- Rig mode rejects `--show-reasoning` before transport execution because the
  Rig adapter does not expose provider reasoning.

Static CLI evidence includes both tests in `tests/model_cli.rs`,
`model_json_rejects_separate_reasoning_presentation` in `tests/cli.rs`, and the
Rig CLI tests in `tests/rig_transport.rs`.

### `agent-run run`

Observed arguments:

- Prompt acquisition is positional, `--file/-f`, or `--editor`.
- Exactly one of `--command TEMPLATE` and `--profile NAME` is required.
- Optional `--config PATH`.
- Repeatable `--context PATH`.
- `--working-directory`, default `.`.
- `--idle-timeout`, default 900 seconds.
- Optional `--detect-loops`, disabled by default.
- `--max-retries`, default 3.
- `--max-output-bytes`, default 1,048,576.
- Required authority acknowledgement `--allow-host-execution`.
- Optional typed `--reasoning-effort`.
- Optional `--json`.

The host-execution acknowledgement is checked before prompt acquisition or
provider command construction. A named profile supplies its stored command;
otherwise the literal template is used. Each attempt renders the prompt,
context paths, target directory, and optional reasoning effort without a shell,
then executes the resulting program and argument vector in the requested
working directory.

On retry, a supervisor notice is appended to the original prompt. It identifies
the attempt and retry cause, warns that the prior attempt may have changed files
or external systems, and asks the provider to inspect and reconcile current
state before repeating non-idempotent actions.

- Non-JSON mode forwards supervised stdout and stderr.
- JSON mode captures both streams, discards captured stderr at the CLI layer,
  converts stdout with lossy UTF-8 replacement, and emits one object
  `{"content":"..."}` followed by a newline.
- A supervised provider receives null stdin and cannot consume caller stdin.
- Terminal stderr receives an initial prompt display before non-JSON execution.

Static evidence includes the standalone run, JSON, reasoning-effort, stdin, and
interrupt tests in `tests/cli.rs`.

### `agent-run setup`

Observed arguments are optional `--config PATH` and `--force`. Setup reads
answers from stdin, writes and flushes prompts to stdout, resolves the current
directory, collects a profile, performs mandatory live qualification, and then
upserts the profile.

The fixed qualification prompt is `Reply with exactly: OK`. Qualification uses
host execution with a 30-second idle timeout, 300-second attempt timeout, loop
detection, no retries, and a 65,536-byte output limit. A failed qualification
does not persist the profile unless `--force` was supplied. Cancellation is
never converted into a force-save path.

Provider choices and generated defaults:

- `llama-cli`: probes `llama-cli --version`, or validates and probes a supplied
  regular executable; requires a regular GGUF model file; default template uses
  `--model`, `--prompt`, `--single-turn`, `--simple-io`,
  `--no-display-prompt`, and `--predict 4096`. It does not add context or format
  flags.
- `llama-server`: discovers models from the selected loopback endpoint and
  generates a `curl --disable --silent --show-error --fail-with-body --request
  POST --json ... -- <endpoint>/v1/chat/completions` template. The prompt is
  inserted as a complete JSON string. Discovery fallback model is `default`.
- Ollama: discovers models and generates
  `env OLLAMA_HOST=<url> ollama run <model> '{prompt}'`. Discovery fallback is
  `llama3.1:8b`.
- Copilot: probes the CLI, performs acknowledged no-prompt ACP discovery, and
  generates a noninteractive template using `--prompt`, `--silent`,
  `--allow-all-tools`, `--no-ask-user`, `--reasoning-effort
  '{reasoning_effort}'`, and `--model`.
- Gemini: probes the CLI, performs acknowledged no-prompt ACP discovery, and
  generates a template using `--prompt`, `--output-format text`,
  `--approval-mode yolo`, `--skip-trust`, and `--model`.
- Custom wrapper: requires a regular non-link executable and generates a
  template with `--prompt '{prompt}' --context '{context_files}'`.

Discovery results are numbered from 1 and followed by `Other model ID...`.
Provider `currentModelId`, when valid and present in the catalog, selects the
default number. Discovery failures other than cancellation or an invalid
transport endpoint produce a warning and offer the documented fallback plus a
custom choice. Manual model IDs use 1–256 printable ASCII bytes and cannot
contain braces.

## Prompt acquisition

`resolve_prompt` applies this source order: supplied string, supplied file,
editor, non-terminal stdin, or terminal interaction.

- Every prompt is valid UTF-8, nonblank after trimming, and at most 1,048,576
  bytes. Returned content is otherwise not trimmed or normalized.
- File path `-` means stdin.
- Prompt files must be regular non-link files. The implementation checks with
  `symlink_metadata`, opens with `O_NOFOLLOW | O_NONBLOCK`, rechecks the opened
  file, rejects sockets and other non-regular files, checks metadata length,
  and performs a bounded read.
- Terminal fallback asks whether to open an editor, defaulting to yes. A `n` or
  `no` response instead requests multiline stdin until EOF.
- Editor selection uses `VISUAL`, then `EDITOR`, then `vi`. A direct file path
  is used as the executable; otherwise the same shell-free command parser is
  used. The editor receives a temporary `prompt.md`; nonzero editor status is
  an invalid-prompt failure.

## Shell-free command rendering

Command templates are parsed internally and are never passed to a shell.
Whitespace separates arguments outside quotes. Single and double quotes group
arguments and are removed. Within an active quote, backslash escapes only the
active quote or another backslash; other backslashes are preserved. Empty
quoted arguments are retained. Unterminated quotes fail.

Supported substitutions in arguments are:

- `{prompt}`: exact prompt text.
- `{prompt_json}`: one complete JSON string literal with quotes, slashes,
  controls, and line breaks escaped.
- `{target_directory}`: UTF-8 target path.
- `{context_files}`: repeats the containing argument once per context path.
- `{reasoning_effort}`: stable lowercase typed effort.

The executable is the first parsed argument. `{reasoning_effort}` cannot select
the executable. Requested reasoning fails if the template lacks its
placeholder. If a standalone `{context_files}` or `{reasoning_effort}` is empty
and the preceding rendered argument starts with `-`, that preceding option is
also removed. Non-UTF-8 context or target paths fail.

## Profile storage

Profiles contain case-sensitive `name`, `provider`, and shell-free `command`.
The TOML store requires integer `schema_version = 1`; absent `profiles` means
an empty list.

Bounds and validation:

- Configuration: at most 65,536 encoded bytes.
- Profiles: at most 128.
- Name: 1–128 ASCII letters, digits, `.`, `_`, `-`, or `:`.
- Provider: 1–128 ASCII letters, digits, `.`, `_`, or `-`.
- Command: 1–16,384 bytes, nonblank, and accepted by the command parser.
- Duplicate profile names are rejected.
- Missing/non-string required fields, malformed TOML, unsupported schema, or an
  absent requested profile are typed failures.

Configuration reads reject symlinks and non-regular files, use
`O_NOFOLLOW | O_NONBLOCK`, recheck the opened file and bound, and require UTF-8.
Upsert validates the entire existing document before changing it. Existing
comments, unrelated top-level values, and other profiles are retained by
`toml_edit`; matching profiles have provider and command replaced. Persistence
uses a temporary file in the destination directory, flushes and syncs it,
atomically replaces an existing path or uses no-clobber creation, then syncs the
parent directory. The parent must be a real directory rather than a symlink.

Default configuration resolution accepts only absolute, nonempty
`XDG_CONFIG_HOME`, otherwise absolute `HOME/.config`. The canonical path is
`agent-runtime/config.toml`. If it does not exist but
`supervised-agent/config.toml` exists, that legacy location is selected; an
existing canonical path takes precedence.

## Model catalog

The canonical serialized catalog has `format_version: 1`, provider,
`current_model_id`, and ordered `models`. A model has `id`, `name`, and optional
`description`.

- At least one model and at most 128 unique model IDs are required.
- Duplicate IDs retain the first descriptor.
- IDs are 1–256 printable ASCII bytes without `{` or `}`.
- Names are nonempty UTF-8 without control characters, at most 1,024 bytes.
- Descriptions may be empty but contain no control characters and are at most
  4,096 bytes.
- An advertised current ID not retained in the catalog is discarded.
- Default selection is the valid current ID, otherwise the first model.
- Discovery timeout must be positive and at most 30 seconds.
- Discovery response bound must be 1–1,048,576 bytes.

Ollama expects a `models` array and uses each string `name` as ID and display
name. If present, string detail fields are formatted in order as family,
parameters, quantization, and format. llama-server expects a `data` array with
string `id` values.

ACP discovery canonicalizes the working directory, starts `<executable> --acp`
in a new process group with null stderr, and uses newline-delimited JSON-RPC
2.0. It sends correlated `initialize` ID 0 with protocol version 1, then
`session/new` ID 1 with the absolute `cwd` and empty `mcpServers`. It requires
the initialize result to confirm protocol version 1. Notifications may occur
between responses and are ignored; provider-to-client requests, wrong or
missing numeric IDs, protocol errors, malformed JSON, and premature EOF fail.
ACP records are capped at 65,536 bytes, total stdout uses the configured
discovery bound, and polling observes cancellation and the overall deadline.
The process group is killed and reaped after discovery, then stdout is drained
for at most one second; retained output pipes fail as `OutputStreamsRetained`.

## Canonical model API

The public model types represent ordered system, user, assistant, and tool
result messages; model-facing tool definitions; `none`, `auto`, and `required`
tool choice; optional reasoning effort; normalized turns, usage, finish reason,
tool intents, and streaming events.

`CancellationToken` is cloneable and backed by a shared atomic boolean.
Transports check it before and during work and return
`ModelTransportCancelled`.

Canonical request validation shared by transports enforces:

- Model: nonblank ASCII, at most 256 bytes.
- Messages: 1–1,024.
- Combined message text/content: at most 8,388,608 bytes.
- Tools: at most 128 with unique names.
- Tool names: 1–128 ASCII letters, digits, `_`, `-`, or `.`.
- Tool description: at most 16,384 bytes.
- Tool parameters and tool-call arguments: JSON objects.
- Tool/call identities: nonblank, at most 256 bytes, no control characters.
- Non-`none` tool choice requires at least one tool.
- Repeated call identities in one validated intent collection fail as
  `DuplicateToolCall`.

Finish reasons normalize stop, length, tool calls, content filter, and bounded
provider-specific text. A stop or absent reason with tool intents becomes
`tool-calls`. Usage is optional.

## Direct local HTTP transport

`DirectModelTransport` supports Ollama `/api/chat` and llama-server
`/v1/chat/completions`.

Transport construction and protocol bounds:

- Deadline: greater than zero and no more than 24 hours.
- Configured response limit: 1–16,777,216 bytes.
- Serialized request: at most 2,097,152 bytes.
- HTTP headers/trailers: at most 65,536 bytes.
- One streaming record or accumulated streamed tool-argument record: at most
  1,048,576 bytes.
- Only ASCII `http://` endpoints with a numeric loopback IPv4 or bracketed IPv6
  address and explicit nonzero port are accepted.
- Endpoint paths, user information, queries, fragments, whitespace, HTTPS, DNS
  names, and non-loopback addresses are rejected.

The transport opens TCP directly, uses 100 ms read/write polling intervals, and
checks the shared deadline and cancellation during connection, writes, reads,
stream decoding, and before and after each stream callback. Requests use
HTTP/1.1, JSON, a fixed provider path, explicit content length, and connection
close.

Responses accept HTTP/1.0 or HTTP/1.1 with chunked, single Content-Length, or
close-delimited framing. Duplicate or invalid lengths, unsupported transfer
encoding, conflicting framing, premature termination, excess bytes, malformed
chunks, and oversized bodies fail. Non-2xx status becomes
`ModelProviderStatus` and intentionally omits the response body from the error.
Response Content-Type is not inspected by this direct implementation.

Provider mapping:

- llama-server uses OpenAI-compatible messages, stringified JSON tool
  arguments, explicit `tool_choice`, `reasoning_effort`, unary choices, and SSE
  streaming terminated by `[DONE]`.
- Ollama uses native messages, object tool arguments, `think` (`false` for
  effort `none`, otherwise the effort spelling), unary records requiring
  `done: true`, and NDJSON streaming requiring a terminal `done: true` record.
- Ollama rejects required tool choice before network access.
- Provider reasoning keys are collected separately from answer text.
- OpenAI tool-call fragments are accumulated by numeric index and emitted only
  after complete object arguments and identities validate.
- Ollama calls lacking provider IDs receive deterministic
  `ollama-call-<index>` IDs. Duplicate supplied IDs fail.
- OpenAI response/model IDs and model identity are bounded to 256 bytes.
- OpenAI usage reads prompt/completion counts and accepts provider total or a
  saturating derived total. Ollama requires paired prompt/evaluation counts and
  rejects checked-add overflow.
- Streaming rejects data after a terminal marker and rejects streams without a
  terminal record.

## Optional Rig transport

`RigModelTransport` is compiled only with `rig-transport`. It uses the same
canonical request validation and the same numeric-loopback, explicit-port,
pathless HTTP restriction, 24-hour maximum deadline, 2,097,152-byte request
bound, and 16,777,216-byte maximum response configuration.

- Reqwest redirects and proxy use are disabled.
- Calls are synchronous at the public boundary and create a current-thread
  Tokio runtime. Invocation from an existing Tokio runtime fails as a typed
  framework error rather than nesting runtimes.
- Cancellation is polled every 10 ms and races the overall timeout.
- A no-subscriber tracing dispatch is installed around Rig execution so
  provider payload/error sentinels are not forwarded to the caller's tracing
  subscriber.
- Reasoning effort is unsupported for every Rig request.
- Rig reasoning, images, unknown streamed content, and other unsupported
  non-text provider content are malformed-response failures; returned
  `ModelTurn.reasoning` is always absent.
- Ollama required tool choice is rejected. For accepted Ollama tool choices,
  the adapter omits Rig's tool-choice field rather than silently mapping
  `required`; llama-server preserves none/auto/required.
- Tool implementations are never registered with Rig. Returned tool calls are
  converted to untrusted intents. Provider IDs are retained and reused when a
  later canonical tool result is converted.
- Response text plus serialized tool intent data is bounded by the configured
  response maximum. Metadata and identities are validated and duplicate calls
  fail.
- The bounded HTTP adapter rejects declared or streamed oversize bodies,
  serialized oversize requests, non-success status, and multipart requests.
- Ollama unary responses must be terminal. Ollama token-count overflow is
  detected for unary and streaming records before Rig can normalize it.
- Framework/provider errors are mapped to redacted domain errors rather than
  returning provider payloads.

## Host-process supervision

`SupervisionPolicy` bounds:

- Idle timeout: greater than zero and at most 3,600 seconds.
- Optional per-attempt timeout: greater than zero and at most 86,400 seconds.
- Retries after the initial attempt: at most 10.
- Combined stdout and stderr per attempt: 1–16,777,216 bytes.

Each attempt:

- Builds a fresh `CommandSpec` from the caller's `AttemptContext`.
- Rejects a blank program.
- Spawns directly without a shell, with null stdin, piped stdout/stderr, an
  optional working directory, and a new process group.
- Reads both streams on dedicated nonblocking threads in 4,096-byte chunks
  through a bounded channel.
- Applies one atomic combined-stream output budget.
- Either forwards and flushes output or captures streams separately.
- Kills and reaps the complete process group at attempt completion or failure.
- Allows one second for output streams to drain; escaped descendants retaining
  pipes produce `OutputStreamsRetained`.

Only idle timeout and detected stdout repetition are retryable. Nonzero exit,
attempt timeout, cancellation, output limit, stream I/O failure, and command
construction failure are terminal. A successful captured run returns output
only from the successful attempt because captured buffers are reset before each
retry.

Loop detection retains the latest 4,096 lossy-UTF-8 stdout bytes. It detects
three identical adjacent byte sequences of length 10–512, four identical
nonblank trimmed lines, or a repeated two-line sequence appearing three times.

SIGINT and SIGTERM set cooperative cancellation. Cancellation terminates the
current process group. Forwarding polls stdout/stderr for writability for up to
one second per write and reports an I/O timeout if the destination remains
blocked.

## Error model

The non-exhaustive public `Error` enum distinguishes invalid command templates,
prompt input, supervision policy, profile setup/configuration, missing
profiles, missing host acknowledgements, invalid catalogs, discovery timeout,
invalid requests/transports, unsupported capabilities, cancellation, transport
timeout, redacted HTTP status, response bounds, malformed responses, duplicate
tool calls, transport I/O/framework failures, nonzero processes, exhausted
retries, output/attempt/drain limits, cancellation, and contextual filesystem or
stream I/O.

Paths and operation labels are retained for local filesystem/process errors.
Provider error response bodies are intentionally not included in HTTP-status
errors.

## Static evidence inventory

Observed integration tests are organized as follows:

- `tests/command.rs`: quoting, substitution, context expansion/removal, JSON
  prompt encoding, and reasoning-effort rules.
- `tests/profiles.rs`: create/load, formatting-preserving update, unchanged
  invalid input, and pre-write validation.
- `tests/setup.rs`: endpoint rejection, host acknowledgement, discovered and
  fallback model selection, JSON prompt generation, numeric IDs, invalid
  selections, and Ollama endpoint materialization.
- `tests/catalog.rs`: catalog bounds/deduplication/versioning, HTTP parsing, ACP
  correlation/notifications/acknowledgement, cleanup, timeout, and cancellation.
- `tests/model_transport.rs`: endpoint restrictions, unary/stream provider
  mapping, every reasoning-effort value, reasoning separation, tool fragments,
  callback deadlines, chunking, terminal requirements, duplicates, malformed
  arguments, bounds, cancellation, redacted status, and stalled providers.
- `tests/rig_transport.rs`: endpoint restrictions, message/tool conversion,
  streaming, cancellation, unsupported reasoning, CLI selection, request and
  response limits, provider identity reuse, metadata bounds, tracing
  suppression, Ollama terminal/usage validation, and nested-runtime refusal.
- `tests/supervisor.rs`: success, retry context, terminal nonzero exit, idle and
  attempt timeouts, policy bounds, output limits, descendant cleanup, and
  retained pipes.
- `tests/model_cli.rs`: non-streaming and streaming Ollama CLI output.
- `tests/cli.rs`: authority acknowledgements, catalog text/JSON and unsupported
  providers, ACP listing, profile path selection, run/JSON behavior, setup
  qualification/force behavior, generated provider templates, ACP current-model
  selection, probe failures/cancellation, and process-group interruption.

## Uncertainty and unverified runtime conditions

- This record is based on static source and test inspection; the tests were not
  executed during documentation, so their current pass/fail status is unknown.
- Optional Rig behavior was inspected in feature-gated source and tests, but no
  feature-enabled build was performed.
- Live compatibility with particular Ollama, llama-server, Copilot, Gemini,
  llama-cli, curl, or wrapper versions was not exercised.
- Terminal-only prompt display/editor interaction, signal timing, blocked output
  destinations, filesystem races, and unusual process-tree behavior remain
  environment-dependent beyond the observed implementation and static tests.
- Clap-generated help text and exact parser exit statuses were not captured;
  the argument structure above comes from the derive declarations.
