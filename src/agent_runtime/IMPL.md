<!-- kvist-implementation-record-version: 1 -->

# Component Implementation Record

## Scope and evidence

This record is observational only. It was regenerated from static inspection of:

- `src/agent_runtime/src/lib.rs`
- `src/agent_runtime/src/main.rs`
- `src/agent_runtime/src/error.rs`
- `src/agent_runtime/src/command.rs`
- `src/agent_runtime/src/model.rs`
- `src/agent_runtime/src/catalog.rs`
- `src/agent_runtime/src/profile.rs`
- `src/agent_runtime/src/prompt.rs`
- `src/agent_runtime/src/setup.rs`
- `src/agent_runtime/src/supervisor.rs`
- `src/agent_runtime/src/direct_transport.rs`
- `src/agent_runtime/src/rig_transport.rs`
- `src/agent_runtime/tests/command.rs`
- `src/agent_runtime/tests/profiles.rs`
- `src/agent_runtime/tests/model_cli.rs`
- `src/agent_runtime/tests/supervisor.rs`
- `src/agent_runtime/tests/setup.rs`
- `src/agent_runtime/tests/catalog.rs`
- `src/agent_runtime/tests/model_transport.rs`
- `src/agent_runtime/tests/rig_transport.rs`
- `src/agent_runtime/tests/cli.rs`
- `src/agent_runtime/Cargo.toml`
- root `Cargo.toml` only for workspace membership and dependency shape

No requirements, contracts, designs, prior implementation records, docs, README, Git history, diffs, status, or runtime test execution were used.

## Workspace and feature shape

- `src/agent_runtime` is a workspace member and a local path dependency of the root `kvist` package as `agent-runtime = { version = "=0.1.0", path = "src/agent_runtime" }`.
- The crate builds a library named `agent_runtime` and a binary named `agent-run`.
- The crate is Linux-only at compile time:
  - `src/lib.rs` and `src/main.rs` both emit `compile_error!` when `target_os != "linux"`.
- Cargo features:
  - feature `rig-transport` is defined
  - default features include `rig-transport`
- Observed transport default:
  - with default features, `agent-run model --transport` defaults to `rig`
  - without `rig-transport`, the only available/default transport is `direct`

## Library surface observed in `lib.rs`

The crate re-exports:

- model catalog discovery:
  - `CatalogProvider`
  - `ModelCatalog`
  - `ModelDiscoveryOptions`
  - `ProviderModel`
  - `discover_models`
- command rendering:
  - `render_command`
  - `render_command_with_reasoning_effort`
  - `split_raw_command`
- transports:
  - `DirectModelTransport`
  - `RigModelTransport` behind feature `rig-transport`
- error/result:
  - `Error`
  - `Result`
- canonical model types:
  - `CancellationToken`
  - `FinishReason`
  - `LocalModelProvider`
  - `ModelMessage`
  - `ModelRequest`
  - `ModelStreamEvent`
  - `ModelTransport`
  - `ModelTurn`
  - `ModelUsage`
  - `ReasoningEffort`
  - `ToolChoice`
  - `ToolDefinition`
  - `ToolIntent`
- profile and prompt helpers:
  - `MAX_PROFILE_CONFIG_BYTES`
  - `ModelProfile`
  - `default_profile_config_path`
  - `load_profile`
  - `load_profiles`
  - `upsert_profile`
  - `MAX_PROMPT_BYTES`
  - `resolve_prompt`
- setup:
  - `SetupOptions`
  - `collect_profile`
  - `collect_profile_with_options`
  - `run_setup_wizard`
  - `run_setup_wizard_with_options`
  - `verify_profile`
- supervision:
  - `AttemptContext`
  - `CapturedExecutionReport`
  - `CommandSpec`
  - `ExecutionReport`
  - `RetryCause`
  - `SupervisionPolicy`
  - `run_supervised`
  - `run_supervised_capture`

## CLI behavior observed in `main.rs`

`agent-run` exposes four subcommands:

- `model`
- `models`
- `run`
- `setup`

### `model`

Observed arguments and defaults:

- prompt source:
  - positional prompt, or
  - `--file PATH`, or
  - `--editor`
- provider:
  - `ollama`
  - `llama-server`
- transport:
  - `direct`
  - `rig` when feature-enabled
- endpoint: required
- model: required
- `--stream`: off by default
- `--timeout`: default `300` seconds
- `--max-response-bytes`: default `1_048_576`
- `--reasoning-effort`: optional
- `--output-schema JSON_SCHEMA`: optional JSON value parsed at runtime
- `--show-reasoning`: text-mode only; rejected with `--json`
- `--json`: emit one canonical `ModelTurn` JSON value

Observed behavior:

- `resolve_prompt` is always called before transport dispatch.
- With `--transport rig`, `--show-reasoning` is rejected before network access as unsupported.
- `--output-schema` must parse as one valid JSON value or the command fails.
- Non-streaming, non-JSON mode writes only `turn.text` to stdout.
- Non-streaming `--show-reasoning` writes reasoning to stderr, then a newline, then answer text to stdout.
- Streaming, non-JSON mode:
  - prints text deltas directly to stdout
  - optionally prints reasoning deltas to stderr when using direct transport and `--show-reasoning`
  - ignores `ToolIntent` stream events at CLI presentation time
- Streaming `--json` does not emit incremental events; it collects the full stream and writes one final `ModelTurn` JSON object.
- When stderr is a terminal, non-JSON `model` mode prints:
  - `Prompt:`
  - prompt text
  - `Response:`
  to stderr before provider output.

### `models`

Observed arguments and defaults:

- provider:
  - `ollama`
  - `llama-server`
  - `copilot`
  - `gemini`
  - `llama-cli`
  - `custom-script`
- `--endpoint HTTP_LOOPBACK_URL`
- `--executable PATH`
- `--allow-host-discovery`
- `--timeout`: default `5` seconds
- `--max-response-bytes`: default `65_536`
- `--json`

Observed behavior:

- `llama-cli` and `custom-script` are rejected as unsupported for catalog discovery without spawning anything.
- `--executable` is rejected for HTTP providers (`ollama`, `llama-server`).
- `--endpoint` is rejected for ACP providers (`copilot`, `gemini`).
- Text output writes one model id per line in catalog order.
- JSON output writes one serialized `ModelCatalog` object followed by a newline.

### `run`

Observed arguments and defaults:

- prompt source:
  - positional prompt, or
  - `--file PATH`, or
  - `--editor`
- exactly one of:
  - `--command TEMPLATE`
  - `--profile NAME`
- `--config PATH`
- repeated `--context PATH`
- `--working-directory DIR`: default `.`
- `--idle-timeout`: default `900` seconds
- `--detect-loops`: off by default
- `--max-retries`: default `3`
- `--max-output-bytes`: default `1_048_576`
- `--allow-host-execution`: required
- `--reasoning-effort`: optional
- `--json`

Observed behavior:

- Host execution is rejected unless `--allow-host-execution` is supplied.
- When `--profile` is used, the profile is loaded from the resolved config file and only its `command` field is used.
- The rendered command is run under `run_supervised` or `run_supervised_capture`.
- Retry attempts append `AttemptContext::retry_notice()` to the prompt before re-rendering the template.
- `--json` emits exactly one object:

```json
{"content":"..."}
```

- In `run --json`, stdout from the successful supervised command is decoded with `String::from_utf8_lossy`; invalid UTF-8 becomes replacement characters.
- In `run --json`, stderr from the supervised command is not surfaced on success.

### `setup`

Observed arguments:

- `--config PATH`
- `--force`

Observed behavior:

- Runs an interactive setup wizard and persists the resulting profile.
- Uses the fixed test prompt `Reply with exactly: OK` for qualification.
- Without `--force`, failed qualification prevents persistence.
- With `--force`, failed qualification is reported but the profile is still written unless setup was cancelled.

## Canonical data shapes

### `ReasoningEffort`

Observed values:

- `none`
- `minimal`
- `low`
- `medium`
- `high`
- `xhigh`
- `max`

### `LocalModelProvider`

Observed values:

- `ollama`
- `llama-server`

### `ToolChoice`

Observed values:

- `none`
- `auto`
- `required`

### `ToolIntent`

Observed shape:

```json
{
  "id": "string",
  "provider_id": "string or null",
  "name": "string",
  "arguments": {}
}
```

Constraints observed in validation:

- `id`: nonblank, no control characters, length ≤ 256
- `name`: 1-128 ASCII bytes, limited to letters, digits, `_`, `-`, `.`
- `arguments`: JSON object only

### `ModelUsage`

Observed shape:

```json
{
  "input_tokens": 0,
  "output_tokens": 0,
  "total_tokens": 0
}
```

### `ModelTurn`

Observed fields:

```json
{
  "text": "string",
  "reasoning": "optional string",
  "tool_intents": [],
  "finish_reason": "unit variants serialize as kebab-case strings",
  "provider": "ollama|llama-server",
  "model": "string",
  "response_id": "optional string",
  "provider_request_id": "optional string",
  "usage": "optional object"
}
```

Notes from code and tests:

- `response_id` is optional and omitted when absent.
- Tests cover deserializing a `ModelTurn` JSON value that lacks `response_id`.
- `FinishReason` unit variants are:
  - `stop`
  - `length`
  - `tool-calls`
  - `content-filter`
- `FinishReason::Other(String)` uses serde's default enum representation for that variant rather than a plain string.

### `ModelCatalog`

Observed JSON shape:

```json
{
  "format_version": 1,
  "provider": "copilot|gemini|ollama|llama-server",
  "current_model_id": "string or null",
  "models": [
    {
      "id": "string",
      "name": "string",
      "description": "optional string"
    }
  ]
}
```

Observed rules:

- `format_version` is always `1`
- duplicate model ids are deduplicated by retaining the first occurrence
- catalog order otherwise preserves provider order
- empty catalogs are rejected
- maximum unique models: `128`
- `default_model_id()` returns `current_model_id` when present and consistent, otherwise the first model id

## Prompt acquisition

Observed prompt source resolution in `prompt.rs`:

- If positional prompt is present, it is used directly.
- Else if `--file PATH` is present:
  - `-` reads stdin
  - any other path must be a regular non-link file
- Else if `--editor` is present, an editor is launched.
- Else:
  - if stdin is non-terminal, stdin is read
  - if stdin is a terminal, the user is prompted on stderr to open an editor; answering `n`/`no` switches to stdin entry

Observed bounds and validation:

- maximum prompt size: `1_048_576` bytes
- prompt must be valid UTF-8
- prompt must not be blank after trimming
- prompt files are opened with `O_NOFOLLOW | O_NONBLOCK`
- prompt files must remain regular non-link files after open

Observed editor behavior:

- creates a temporary directory with `tempfile::tempdir()`
- writes `prompt.md` inside it
- editor is `VISUAL`, then `EDITOR`, then `vi`
- if the editor environment value names a file path, that file is executed directly
- otherwise `split_raw_command` is used to parse the editor command line

## Command template rendering

Observed supported placeholders:

- `{prompt}`
- `{prompt_json}`
- `{target_directory}`
- `{context_files}`
- `{reasoning_effort}`

Observed parsing/rendering rules:

- Templates are split without shell interpolation.
- Single and double quotes are supported for grouping.
- Backslashes inside quotes only escape the active quote or a backslash.
- Unterminated quotes are rejected.
- The executable is the first parsed argument and must not be empty.
- `{reasoning_effort}` is forbidden in the executable position.
- If a reasoning effort is requested but the template contains no `{reasoning_effort}` placeholder, rendering fails.
- `{prompt_json}` is rendered as a complete JSON string literal.
- `{context_files}` repeats the argument once per supplied context path.
- If `{context_files}` or `{reasoning_effort}` appears as an argument by itself and the preceding rendered argument starts with `-`, the preceding option is removed when the placeholder expands to nothing.

## Profile storage

Observed profile file behavior in `profile.rs`:

- default config path resolution:
  - use absolute `XDG_CONFIG_HOME` if valid
  - else use absolute `HOME/.config`
  - canonical path: `agent-runtime/config.toml`
  - if that file is absent and legacy `supervised-agent/config.toml` exists, the legacy path is selected
  - if the canonical file exists, it takes precedence even if invalid
- configuration file requirements:
  - regular non-link file
  - valid UTF-8
  - size ≤ `64 KiB`
- TOML schema:
  - `schema_version = 1`
  - `profiles` is an array of tables
- profile limits:
  - at most `128` profiles
  - name length `1..=128`
  - command length `1..=16384` after trim/nonblank check

Observed field validation:

- `name`: ASCII letters/digits plus `.`, `_`, `-`, `:`
- `provider`: nonblank ASCII identifier using letters/digits plus `.`, `_`, `-`
- `command`: must parse as a raw command template

Observed write behavior:

- existing files are parsed and revalidated before modification
- comments and unrelated keys are preserved through `toml_edit`
- invalid existing configuration aborts the write and leaves the file unchanged
- new content is re-parsed and revalidated before persistence
- writes are atomic via `tempfile::NamedTempFile::new_in(parent)` and `persist`/`persist_noclobber`
- parent directory is created as needed, must be a real directory, and is fsynced after write

## Setup wizard behavior

Observed provider menu:

1. `llama-cli`
2. `llama-server`
3. `ollama`
4. `copilot`
5. `gemini-cli`
6. `custom-script`

Observed default selection when the answer is not `1`, `2`, `4`, `5`, or `6`:

- provider defaults to `ollama`

### Generated default command templates

Observed default template families:

- `llama-cli`
  - executable/wrapper path
  - `--model <gguf>`
  - `--prompt '{prompt}'`
  - `--single-turn --simple-io --no-display-prompt --predict 4096`
- `llama-server`
  - `curl`
  - POST to `<base>/v1/chat/completions`
  - `--json` request body containing the rendered prompt JSON
- `ollama`
  - `env OLLAMA_HOST=<base> ollama run <model> '{prompt}'`
- `copilot`
  - `<binary> --prompt '{prompt}' --silent --allow-all-tools --no-ask-user --reasoning-effort '{reasoning_effort}' --model <model>`
- `gemini-cli`
  - `<binary> --prompt '{prompt}' --output-format text --approval-mode yolo --skip-trust --model <model>`
- `custom-script`
  - `<script> --prompt '{prompt}' --context '{context_files}'`

### Executable probing

Observed behavior:

- `copilot` and `gemini` first probe the conventional executable name with `--version`
- on conventional probe failure, setup asks for a direct executable or wrapper path
- supplied executable paths are canonicalized, must be regular non-link files, and for custom wrappers must also be executable
- a failed version probe on the explicit fallback path aborts setup
- cancellation during the conventional probe aborts setup without asking for fallback input

### Model discovery during setup

Observed discovery defaults:

- llama-server base URL default: `http://127.0.0.1:9931`
- ollama base URL default: `http://127.0.0.1:11434`
- discovery timeout: `5` seconds
- discovery response bound: `64 KiB`

Observed fallback behavior when discovery fails:

- if failure is `ModelTransportCancelled` or `InvalidModelTransport`, setup returns that error immediately
- all other discovery failures produce a warning and then offer:
  - one documented fallback model id
  - one explicit `"Other model ID..."` option

Observed fallback model ids:

- llama-server: `default`
- ollama: `llama3.1:8b`
- copilot: `auto`
- gemini: `auto`

Observed model selection behavior:

- when discovery succeeds, setup lists discovered models in catalog order
- the default numbered selection is the provider `current_model_id` when present; otherwise the first model
- selected or manually entered model ids must be printable ASCII, length `1..=256`, and must not contain braces

### Setup qualification

Observed verification behavior:

- `verify_profile` refuses to run without host execution acknowledgement
- setup passes acknowledgement internally as `true`
- verification policy:
  - `idle_timeout = 30s`
  - `attempt_timeout = 300s`
  - `detect_loops = true`
  - `max_retries = 0`
  - `max_output_bytes = 64 KiB`
- verification executes the exact rendered command in the provided working directory
- the prompt used for qualification is always `Reply with exactly: OK`

## Model request validation

Observed shared validation in `direct_transport::validate_request`:

- `model`:
  - nonblank
  - ASCII only
  - length ≤ `256`
- message count: `1..=1024`
- total text bytes across canonical message text/content fields: ≤ `8 MiB`
- tools:
  - count ≤ `128`
  - unique names
  - description length ≤ `16 KiB`
  - parameters must be a JSON object
- `tool_choice != none` requires at least one tool
- `output_schema` cannot be combined with callable tools

Observed output schema subset and bounds:

- encoded size ≤ `256 KiB`
- maximum depth `32`
- maximum nodes `4096`
- every node must be a JSON object
- allowed keywords only:
  - `type`
  - `title`
  - `description`
  - `properties`
  - `required`
  - `additionalProperties`
  - `items`
  - `enum`
  - `const`
  - `anyOf`
  - `allOf`
  - `minimum`
  - `maximum`
  - `minLength`
  - `maxLength`
  - `minItems`
  - `maxItems`
- `type` must be one of:
  - `null`
  - `boolean`
  - `object`
  - `array`
  - `number`
  - `string`
  - `integer`
  or a nonempty array of those strings
- `required` must contain unique property names already present in `properties`
- `additionalProperties` may be boolean or schema
- `enum`, `anyOf`, and `allOf` must be nonempty arrays

## Direct transport

### Endpoint policy

Observed endpoint restrictions in `DirectModelTransport::new` and `parse_endpoint`:

- only `http://`
- ASCII only
- no whitespace
- no userinfo, query, or fragment
- no caller-selected path
- explicit port required
- host must be a numeric loopback IP
- bracketed IPv6 is accepted only with `[host]:port` syntax

### Request encoding

Observed provider-specific request mapping:

- Ollama:
  - POST `/api/chat`
  - `model`
  - `messages`
  - `stream`
  - optional `think`
    - `false` for reasoning effort `none`
    - effort string for any other reasoning effort
  - optional `format` containing the supplied output schema
  - optional `tools`
  - no `tool_choice` field
- llama-server:
  - POST `/v1/chat/completions`
  - `model`
  - `messages`
  - `stream`
  - optional `reasoning_effort`
  - optional `response_format`:

```json
{
  "type": "json_schema",
  "json_schema": {
    "name": "kvist_output",
    "strict": true,
    "schema": {}
  }
}
```

  - optional `tools`
  - `tool_choice` as `"none" | "auto" | "required"`

Observed message encoding:

- system/user messages become role/content pairs
- assistant messages with tool intents encode provider-facing `tool_calls`
- tool result messages encode provider-native tool-role records
- for llama-server assistant tool calls, arguments are serialized as JSON text
- for Ollama assistant tool calls, arguments remain JSON objects

Observed request bounds:

- serialized request size ≤ `2 MiB`
- configured deadline must be `> 0` and `≤ 24h`
- configured response bound must be `1..=16 MiB`

Observed capability restriction:

- Ollama rejects `ToolChoice::Required` before I/O

### Response handling

Observed HTTP handling:

- supports HTTP/1.0 and HTTP/1.1 only
- rejects non-2xx responses with `ModelProviderStatus { status }`
- does not retain provider error bodies in the exposed error
- supports:
  - `Content-Length`
  - `Transfer-Encoding: chunked`
  - close-delimited bodies when neither framing header is present
- header bytes are bounded to `64 KiB`
- stream record bytes are bounded to `1 MiB`

Observed unary decoding:

- llama-server:
  - reads only the first choice
  - extracts text from `message.content`
  - extracts reasoning from the first present key among:
    - `reasoning_content`
    - `reasoning`
    - `thinking`
  - parses `tool_calls`
  - uses top-level `id` as `response_id`
  - leaves `provider_request_id` as `None`
  - parses token usage from `prompt_tokens`, `completion_tokens`, and optional `total_tokens`
- Ollama:
  - requires `done == true`
  - extracts text from `message.content`
  - extracts reasoning from `thinking` then `reasoning`
  - parses `tool_calls`
  - always leaves `response_id` and `provider_request_id` as `None`
  - computes usage as `prompt_eval_count + eval_count`

Observed tool-call normalization:

- llama-server:
  - tool call `id` is required
  - canonical `id` and `provider_id` are both set to the provider id
- Ollama:
  - provider id is optional
  - canonical id is `provider_id` when present
  - otherwise canonical id is synthesized as `ollama-call-<offset>`

Observed finish-reason mapping:

- `"stop"` becomes:
  - `stop` when no tool intents exist
  - `tool-calls` when tool intents exist
- `"length"` remains `length` even when tool intents exist
- `"tool_call"` and `"tool_calls"` map to `tool-calls`
- `"content_filter"` maps to `content-filter`
- missing/null finish reason is inferred from tool presence

### Streaming behavior

Observed llama-server streaming behavior:

- expects SSE records using `data:`
- ignores empty lines and comment lines beginning with `:`
- rejects other SSE fields
- `[DONE]` is the required terminal marker
- text deltas are emitted immediately as `ModelStreamEvent::TextDelta`
- reasoning deltas are emitted immediately as `ModelStreamEvent::ReasoningDelta`
- tool call fragments are accumulated by `index`
- completed tool intents are emitted only after stream finish

Observed Ollama streaming behavior:

- expects newline-delimited JSON records
- empty lines are ignored
- a terminal record with `done == true` is required
- reasoning deltas are emitted as they appear
- text deltas are emitted as they appear
- tool intents are accumulated across records and emitted after stream finish

Observed callback/deadline behavior:

- streaming checks cancellation and deadline:
  - before event delivery
  - after event delivery
  - again after final assembly
- tests cover timeout after a slow callback returns

## Rig transport

This section describes the feature-gated transport compiled by default in the current crate configuration.

### Transport shape

Observed constructor behavior:

- endpoint must be `http://<numeric-loopback-ip>:<port>`
- no path, query, fragment, or userinfo
- deadline must be `> 0` and `≤ 24h`
- response limit must be `1..=16 MiB`

Observed implementation details:

- constructs a `reqwest` client with:
  - redirects disabled
  - proxy use disabled
- creates a current-thread Tokio runtime per call
- refuses to run if already inside an active Tokio runtime
- installs a `tracing::subscriber::NoSubscriber` dispatch while blocking on the runtime

Tests statically cover that prompt/response sentinel strings do not reach an enclosing tracing subscriber.

### Capability and fallback behavior

Observed restrictions:

- `request.reasoning_effort.is_some()` is rejected before network access as unsupported by `rig-transport`
- `--show-reasoning` with `--transport rig` is rejected before network access
- Ollama still rejects `ToolChoice::Required`

Observed default/fallback behavior:

- default CLI transport is `rig` because `rig-transport` is in the crate's default feature set and the CLI enum default switches to `Rig` when the feature is enabled
- direct transport is available only by explicit `--transport direct`
- a failed Rig request is not replayed through the direct transport

### Request mapping

Observed behavior:

- reuses `validate_request` from the direct transport
- converts canonical messages to Rig chat history
- provider call ids from prior assistant tool intents are remembered and reused for subsequent tool results when possible
- if a tool result refers to a call without a remembered provider id, `ToolCallId::new_or_mint` is used inside Rig conversion
- output schema mapping matches direct transport:
  - Ollama receives `format = <schema>`
  - llama-server receives `response_format.json_schema` with `name = "kvist_output"` and `strict = true`
- `record_telemetry_content` is set to `false`

### Response mapping

Observed unary behavior:

- text content is accumulated
- tool calls become `ToolIntent`
- reasoning, images, and unknown content are rejected as malformed
- returned `ModelTurn.reasoning` is always `None`
- response metadata is bounded and validated:
  - model identity ≤ `256`
  - response identity ≤ `256`
  - provider request identity ≤ `256`
  - no control characters

Observed streaming behavior:

- text deltas are emitted immediately
- completed tool-call events are emitted immediately as `ToolIntent`
- `ToolCallDelta` events are ignored until a completed call arrives
- a provider terminal record is required
- returned `ModelTurn.reasoning` is always `None`

Observed Ollama-specific Rig validation:

- unary Ollama responses are prevalidated to require `done == true`
- unary and streaming Ollama usage fields are checked for `prompt_eval_count + eval_count` overflow

Observed Rig error mapping:

- provider HTTP status becomes `ModelProviderStatus`
- request oversize becomes `InvalidModelRequest`
- response oversize becomes `ModelResponseLimitExceeded`
- decode failures become `MalformedModelResponse`
- other framework failures become `ModelTransportFramework`

## Catalog discovery

### Shared catalog rules

Observed bounds:

- discovery timeout must be `> 0` and `≤ 30s`
- discovery response limit must be `1..=1_048_576`
- model id length `1..=256`
- model ids must be printable ASCII and may not contain braces
- model name max length `1024`
- model description max length `4096`
- control characters in names/descriptions are rejected

### HTTP discovery

Observed provider mappings:

- `ollama`:
  - default endpoint `http://127.0.0.1:11434`
  - GET `/api/tags`
  - each `models[].name` becomes both id and name
  - description is synthesized from available `details` keys:
    - `family`
    - `parameter_size`
    - `quantization_level`
    - `format`
- `llama-server`:
  - default endpoint `http://127.0.0.1:9931`
  - GET `/v1/models`
  - each `data[].id` becomes both id and name

### ACP discovery

Observed provider mappings:

- `copilot` default executable: `copilot`
- `gemini` default executable: `gemini`

Observed gating:

- ACP discovery is refused unless `allow_host_discovery` is true

Observed process behavior:

- executable is spawned with `--acp`
- current directory is the canonicalized absolute `working_directory`
- stdin and stdout are piped
- stderr is discarded to `/dev/null`
- the provider is placed in its own process group

Observed protocol sequence:

1. send `initialize` with:
   - `jsonrpc = "2.0"`
   - `id = 0`
   - `params.protocolVersion = 1`
   - empty `clientCapabilities`
2. require matching response `id = 0` and `result.protocolVersion = 1`
3. send `session/new` with:
   - `id = 1`
   - `params.cwd = <absolute canonical working directory>`
   - `params.mcpServers = []`
4. require matching response `id = 1`

Observed ACP parsing rules:

- notifications with `method` and no `id` are ignored
- provider-to-client requests are rejected
- `jsonrpc` must be `"2.0"`
- `error` in a correlated response is rejected
- `availableModels` is required
- `name` defaults to `modelId` when absent
- `description` may be string or null
- `currentModelId` may be string or null
- inconsistent `currentModelId` is dropped rather than causing failure

Observed ACP bounds and cleanup:

- per-record bound: `64 KiB`
- total received stdout bound: `options.max_response_bytes`
- polling interval: `50 ms`
- cleanup kills the provider process group with `SIGKILL`
- cleanup then waits for child exit and drains stdout
- if a descendant retains stdout beyond a 1 second drain window, discovery fails with `OutputStreamsRetained`
- tests cover timeout and cancellation reaping the process group

## Supervision

### Policy bounds

Observed limits:

- `idle_timeout`: `1s..=3600s`
- `attempt_timeout`: positive and `≤ 24h` when present
- `max_retries`: `0..=10`
- `max_output_bytes`: `1..=16 MiB`

### Process execution behavior

Observed execution model:

- `CommandSpec` stores:
  - `program`
  - exact argument vector
  - optional working directory
- the child process is spawned without a shell
- stdin is always `Stdio::null()`
- stdout and stderr are piped
- the child is placed in its own process group
- per-stream reader threads set the file descriptors nonblocking and poll them
- combined stdout+stderr bytes count against one shared output budget

### Retry behavior

Observed retry causes:

- `IdleTimeout`
- `RepetitionLoop`

Observed loop detection:

- active only when `detect_loops` is true
- checks stdout only
- detects:
  - three identical trailing byte sequences of length `10..=512`
  - four identical trailing nonblank trimmed lines
  - three repetitions of a two-line alternating suffix

Observed non-retry terminal failures:

- nonzero exit status
- output limit exceeded
- attempt timeout
- cancellation

Observed retry context:

- retries pass an `AttemptContext` to the caller
- `retry_notice()` returns an advisory warning that prior attempts may have changed files or external systems
- `run` appends this notice to the prompt before rerendering the command

### Cancellation and cleanup

Observed behavior:

- SIGINT and SIGTERM are hooked via `signal-hook`
- the first received signal cancels the shared token
- each attempt terminates the whole child process group with `SIGKILL`
- after termination, output is drained for up to 1 second
- lingering open pipes after that window become `OutputStreamsRetained`
- tests cover cleanup of descendants and interruption of the top-level `run` command

## Static test evidence

The following evidence is from source inspection only. Tests were not run for this record.

### `tests/command.rs`

Covers:

- placeholder substitution without a shell
- repeated `{context_files}` expansion
- `{prompt_json}` escaping
- quote parsing failure on unterminated input
- `{reasoning_effort}` rendering and omission semantics

### `tests/profiles.rs`

Covers:

- creating and loading a profile
- preserving comments and unrelated TOML values on update
- leaving invalid existing config unchanged
- rejecting invalid profile names before writing

### `tests/model_cli.rs`

Covers:

- `agent-run model` against a fake local Ollama endpoint
- direct model output schema passthrough in CLI mode
- streaming CLI preserving provider text exactly

### `tests/setup.rs`

Covers:

- rejecting non-loopback discovery URLs
- refusing profile verification without host acknowledgement
- llama-server prompt JSON encoding
- discovered model listing and selection
- fallback/default model selection when discovery fails
- accepting numeric model ids
- rejecting invalid numbered and brace-bearing model selections
- Ollama command materializing the selected endpoint

### `tests/catalog.rs`

Covers:

- catalog serialization format version `1`
- duplicate-id deduplication and current-model dropping
- invalid model/catalog rejection
- HTTP discovery for Ollama and llama-server
- ACP initialize/session correlation
- ignoring ACP notifications
- host-discovery acknowledgement gate
- rejecting malformed ACP response ids and provider requests
- retained-stdout cleanup failures
- ACP timeout and cancellation process-group reaping

### `tests/supervisor.rs`

Covers:

- one-attempt success reporting
- retry notice contents
- no retry after nonzero exit
- idle timeout termination
- attempt timeout independent of continuing output
- retry bound validation
- output-limit terminal behavior
- descendant cleanup
- retained-output failure without hanging

### `tests/model_transport.rs`

Covers:

- direct output schema passthrough to both providers
- schema validation before I/O
- loopback-only endpoint policy
- llama-server unary mapping of tool calls and usage
- Ollama unary mapping and required-tool-choice rejection
- every reasoning-effort mapping
- reasoning kept separate from answer text
- llama-server stream assembly from fragmented tool-call arguments
- Ollama reasoning/text stream ordering
- deadline checks after callback return
- backward-compatible `ModelTurn` deserialization without `response_id`
- event delivery before terminal record arrival
- chunked HTTP decoding across chunk boundaries
- required terminal Ollama stream record
- duplicate tool-call rejection
- non-object argument rejection
- oversize response rejection
- preservation of explicit `length` finish reason with tools
- typed cancellation, timeout, and redacted provider-status errors
- close-delimited body bounding before event delivery

### `tests/rig_transport.rs`

Covers:

- output schema passthrough through Rig
- default CLI transport being `rig`
- explicit `--transport direct` fallback path
- no silent replay from failed Rig to direct
- loopback-only Rig endpoint policy
- unary and streaming Rig mappings
- preflight cancellation
- reasoning-effort rejection before network access
- `--show-reasoning` rejection with Rig CLI transport
- declared oversize-body rejection
- deadline enforcement
- Ollama required-tool-choice rejection
- deadline checks after callback return
- oversize request rejection before network access
- streamed response limit classification
- provider call-id reuse for tool results
- metadata validation
- tracing isolation
- malformed nonterminal Ollama unary rejection
- usage-overflow rejection
- nested Tokio runtime rejection

### `tests/cli.rs`

Covers:

- `--allow-host-execution` gating
- deterministic text and JSON output for `models`
- unsupported manual providers for `models`
- ACP host-discovery acknowledgement gate
- default config path resolution from `XDG_CONFIG_HOME` and `HOME`
- legacy profile path fallback
- canonical-store precedence over legacy store
- prompt file execution
- `run --json` output shape and lossy UTF-8 replacement
- profile-driven reasoning-effort selection
- `model --json` / `--show-reasoning` flag conflict
- fixed setup qualification prompt
- `--force` persistence after failed qualification
- supervised child inability to consume caller stdin
- setup persistence followed by `run --profile`
- generated llama-cli wrapper template flags
- generated Gemini/Copilot noninteractive templates
- ACP current-model default selection during setup
- unusable explicit provider fallback rejection
- cancelling the conventional provider probe
- SIGINT cleanup of the supervised process group

## Observed limitations and current constraints

- Linux only.
- No sandboxing or isolation boundary is implemented; comments in `lib.rs` and `supervisor.rs` explicitly describe host-process supervision only.
- `model` CLI currently supports only local HTTP providers:
  - `ollama`
  - `llama-server`
- HTTP transports require numeric loopback endpoints and plain HTTP; hostnames and HTTPS are rejected.
- Default CLI model transport is Rig, but Rig currently does not preserve/provider-surface reasoning output:
  - `ModelTurn.reasoning` remains `None`
  - `--show-reasoning` is rejected
  - reasoning effort is rejected
- Direct transport is the explicit fallback when reasoning output or reasoning-effort passthrough is needed from the current CLI.
- No automatic transport fallback exists after Rig failure.
- Output-schema support is intentionally limited to a validated subset.
- `models` catalog discovery is not implemented for `llama-cli` or `custom-script`.
- Prompt editing currently relies on a temporary file in a temporary directory.
