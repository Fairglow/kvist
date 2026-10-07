<!-- kvist-implementation-record-version: 1 -->

# Component Implementation Record

## Observed implementation: agent-runtime

## Observation basis and package

This replacement record was independently derived on 2026-10-02 from
`agent_runtime/src/**/*.rs`, `agent_runtime/tests/**/*.rs`, this package's
Cargo manifest, and executed tests. Intent documents, previous implementation
records, reviews, Git history, and engine/sandbox-runner implementation were
excluded. This is an observation, not intent review or compliance certification.
Paths are repository-relative; source-derived limitations are distinguished
from executed behavioral trials.

`agent_runtime/Cargo.toml` defines agent-runtime 0.2.0, Rust edition 2024,
minimum Rust 1.95, library `agent_runtime`, and binary `agent-run`.
Default features are empty; Tokio is optional. `agent_runtime/src/lib.rs` and
the binary explicitly reject non-Linux targets. The active direct transport is
blocking standard-library TCP, not an asynchronous framework client. This crate
supervises host processes; it does not provide a sandbox.

The public exports in `agent_runtime/src/lib.rs` include:

- Prompt acquisition: `resolve_prompt`, `MAX_PROMPT_BYTES`.
- Shell-free parsing/rendering: `split_raw_command`, `render_command`,
  `render_command_with_reasoning_effort`.
- Profiles: `ModelProfile`, default path, load/load-all/upsert, config byte
  maximum; setup collection/wizard variants, `SetupOptions`, `verify_profile`,
  `SETUP_TEST_PROMPT`.
- Canonical model messages/requests/turns/usage/stream events, reasoning effort,
  local provider, tool definitions/choice/intents, finish reason,
  `CancellationToken`, `ModelTransport`, `DirectModelTransport`.
- Catalog provider, validated provider model/catalog, discovery options and
  `discover_models`.
- Host supervision: `CommandSpec`, policy, attempt/retry context, forwarding
  and captured execution reports and functions.
- Shared signal install/register/clear/take-interrupted operations.
- Public `gbnf`, `loop_detection`, `trajectory` modules, their compiler/hash/
  detector APIs/constants, trajectory event/recorder/replay/report exports.
- Non-exhaustive `Error` and `Result`.

## Prompt and command mechanics

`agent_runtime/src/prompt.rs` resolves explicit prompt first, then file, then
editor, then stdin. File `-` means stdin. Without an explicit source, piped
stdin is read directly; terminal stdin asks about opening an editor, or reads
until EOF. Every returned prompt must be nonblank UTF-8 and at most 1 MiB.
Bounded reads take maximum+1 to catch growth. Prompt files use lstat plus
`O_NOFOLLOW | O_NONBLOCK`, and opened descriptors must remain regular files;
links/sockets/special files fail.

Editor selection uses VISUAL, then EDITOR, then `vi`. A whole value identifying
an existing file is used as program; otherwise the raw quoted command parser
splits it. One generated `prompt.md` in a tempfile-owned directory is appended,
the editor is executed without shell interpolation, successful exit is required,
and edited contents are revalidated. Editor execution itself is not governed by
SupervisionPolicy or a timeout; terminal choice reads are not byte-bounded.

`agent_runtime/src/command.rs` tokenizes whitespace outside single/double
quotes, preserves empty quoted arguments, and permits escaping active quote/
backslash within quotes. Unterminated quotes fail. This is not a shell:
operators, variables, globbing, command substitution and outside-quote
backslashes are not evaluated.

Rendering parses before replacement. The first token selects the executable
and is not prompt/path substituted; `{reasoning_effort}` cannot select it.
Arguments support `{prompt_json}` (complete escaped JSON string), `{prompt}`,
`{target_directory}`, `{context_files}`, `{reasoning_effort}`. Context
placeholder yields one argument per path; with no contexts, an exact placeholder
can remove the preceding option-looking argument. Effort behaves similarly
when absent; explicitly requested effort requires a declared argument
placeholder. Relevant paths must be UTF-8 when substituted. Substitution can
repeat within an argument and does not re-tokenize generated text. It is literal
replacement, not a general template language.

Executed `agent_runtime/tests/command.rs` includes
`renders_prompt_context_and_target_without_a_shell`,
`renders_a_prompt_as_a_complete_json_string`,
`removes_option_before_an_empty_context_placeholder`,
`reasoning_effort_requires_and_renders_an_explicit_placeholder`.
CLI file/stdin behavior is covered by
`agent_runtime/tests/cli.rs::standalone_cli_runs_a_prompt_from_a_file` and
`supervised_provider_cannot_consume_caller_stdin`.

## Profile persistence and setup

`agent_runtime/src/profile.rs` stores schema_version integer 1 and
`[[profiles]]` entries with `name`, `provider`, `command`. Unrelated values,
tables, fields, comments and other profiles are preserved with toml_edit;
unknown surrounding fields are not rejected.

Bounds: 64 KiB configuration, 128 profiles, 128-byte names/providers, and
16 KiB commands. Names use ASCII alphanumerics plus `._-:`; providers use
alphanumerics plus `._-`; neither may be empty. Commands must be nonblank and
parse successfully without a shell. Names are case-sensitive and unique.
The provider field is descriptive, not an enumeration that dispatches
execution. Load validates all profiles before selecting one.

Default location uses absolute XDG_CONFIG_HOME, or absolute HOME/.config,
then `agent-runtime/config.toml`. Actual implementation falls back to
`supervised-agent/config.toml` if the canonical file is absent and that legacy
path exists; canonical takes priority.

Reads reject a linked/nonregular final file, bound metadata and stream growth,
and require UTF-8. Upsert validates the candidate and all existing data before
modification; creates parents, rejects a linked immediate parent, writes/syncs a
same-directory NamedTempFile, persists by replacement for existing stores or
no-clobber for new ones, and syncs the parent. It is atomic replacement, not a
transactional multiwriter update; all ancestor components are not traversed
with pinned no-follow descriptors.

`agent_runtime/tests/profiles.rs::update_preserves_comments_unrelated_values_and_profiles`,
`invalid_existing_configuration_remains_unchanged`, and
`rejects_invalid_profile_names_before_writing` were executed.
Legacy/canonical path behavior was executed in
`agent_runtime/tests/cli.rs::default_profile_path_reads_legacy_store_when_canonical_store_is_absent`
and `canonical_profile_store_takes_precedence_over_legacy_store`.

`agent_runtime/src/setup.rs` offers llama-cli, llama-server, Ollama, Copilot,
Gemini CLI, and custom wrapper. Setup collects model/provider executable/profile
name/template, validates it, runs mandatory live qualification using the fixed
prompt `Reply with exactly: OK`, then persists. Qualification checks successful
bounded process execution, not that the answer literally equals OK. Failure
prevents saving unless force=true; cancellation is never overridden by force.
`verify_profile` requires its host-execution acknowledgement argument before
spawning.

Conventional CLI executables receive a bounded `--version` probe (10-second
idle/attempt, 64 KiB output, no retry). Failed probe offers a direct executable
or compatible wrapper, verified as regular/executable and probed again.
Custom wrappers and GGUF selections require regular files; `~/` is expanded.
Setup input supports EOF, Esc, `cancel`, or `q` cancellation, but individual
interactive input lines are read without an explicit byte bound.

HTTP and ACP catalog discovery provides numbered models, defaulting to valid
provider current model or first item, plus a custom-model choice. Invalid
endpoint/cancellation stops selection; other discovery failure offers the
specific provider fallback. Selected IDs are printable ASCII, 1–256 bytes,
without braces. Profile name can differ from model ID.

Generated defaults actually include:

- llama-cli: model/prompt, `--single-turn --simple-io --no-display-prompt
  --predict 4096`; setup explains that bare inference is not a file/tool agent.
- llama-server: curl with `--disable --silent --show-error --fail-with-body`,
  POST JSON containing `{prompt_json}`, explicit chat-completions URL after `--`.
- Ollama: `env OLLAMA_HOST=<selected URL> ollama run <model> '{prompt}'`.
- Copilot: prompt, silent, allow-all-tools/no-ask-user, optional effort
  placeholder and model; Gemini: text output, yolo approval, skip-trust, model.
- Custom wrapper: prompt and repeatable context-files placeholder.

These setup wrappers run with host authority and can differ from the direct
transport's capabilities. Qualification uses 30-second idle, 300-second attempt,
loop detection, no retries, and 64 KiB output. Executed examples include
`agent_runtime/tests/cli.rs::setup_uses_fixed_prompt_and_refuses_failed_qualification`,
`setup_force_persists_profile_after_failed_qualification`,
`installed_gemini_and_copilot_use_noninteractive_templates`,
and `agent_runtime/tests/setup.rs::llama_server_default_json_encodes_the_rendered_prompt`.
These tests use local fixtures, not real accounts/provider services.

## Canonical model boundary

`agent_runtime/src/model.rs` defines:

- `ReasoningEffort`: none/minimal/low/medium/high/xhigh/max, lower-case wire
  values; parser trims and is case-insensitive.
- `LocalModelProvider`: ollama/llama-server.
- `ModelMessage`: System(String), User(String), Assistant{text, tool_intents},
  ToolResult{call_id, name, content}; externally tagged kebab-case serde.
- `ToolDefinition`: name, description, JSON parameters; `ToolChoice`:
  none/auto/required.
- `ModelRequest`: model, messages, tools, tool_choice; optional
  reasoning_effort/output_schema/max_output_tokens, omitted when absent.
- `ToolIntent`: canonical id, optional provider_id, name, object arguments.
- `FinishReason`: stop/length/tool-calls/content-filter/Other(String).
- `ModelUsage`: input_tokens, output_tokens, total_tokens.
- `ModelTurn`: text, optional omitted reasoning, tool_intents, finish_reason,
  provider, model, defaultable response_id, provider_request_id, optional usage.
- `ModelStreamEvent`: tagged `{type, value}`, kebab-case text-delta,
  reasoning-delta, tool-intent.

Tool definitions contain no execution implementation and tool intents are
untrusted proposals. There is no tool executor or autonomous agent loop in this
boundary.

`CancellationToken` clones an Arc<AtomicBool>, exposes cancel/query/reset with
release/acquire ordering. `ModelTransport` has blocking complete, stream,
stream_with_deadline, and deadline. The default deadline-override method ignores
the override and delegates to stream; callers cannot assume arbitrary injected
transports honor an extended/reduced deadline.

## Direct HTTP transport and request validation

`agent_runtime/src/direct_transport.rs` supports only numeric-loopback HTTP
with explicit nonzero port: IPv4 loopback or bracketed IPv6 loopback. DNS names,
remote addresses, HTTPS, credentials, queries/fragments, whitespace/control
bytes, and nontrivial base paths fail. Provider paths are fixed. There is no
ambient proxy, credential, TLS or framework fallback.

Constructor bounds deadline to positive ≤24 hours and response bytes to
1–16 MiB; with_watchdogs also validates positive ≤24-hour slot/TTFT timeouts.
Defaults are 15-second response-header/slot wait and 45-second streaming TTFT.
Builder setters for slot/TTFT/cadence merely assign durations without repeating
constructor validation. Cadence is absent by default. Stream deadline override
is capped to 24 hours and checked before/after decoding callbacks.

Before connecting, canonical validation requires:

- Nonblank ASCII model ≤256 bytes; 1–1024 messages, ≤128 definitions,
  combined message text ≤8 MiB.
- Optional max output tokens 1–1,048,576.
- Tool names 1–128 ASCII alphanumerics or `_.-`, unique definition names,
  descriptions ≤16 KiB, object parameters.
- Assistant IDs nonblank/control-free ≤256 bytes and unique within each
  assistant message; object arguments; valid tool-result identities/names.
  The transport does not enforce complete history/result pairing.
- Tools required for auto/required; output schema cannot coexist with callable
  tool choice. Retained definitions with choice none are not sent as tools.
- Encoded provider request ≤8 MiB.

The subsequently added
`agent_runtime/tests/model_transport.rs::reliability_large_context_requests_are_not_limited_by_the_old_two_mib_ceiling`
was separately executed against frozen production source. Its loopback fixture
accepts a 3-MiB user message and captures the request; an 8-MiB user message
whose provider encoding exceeds 8 MiB returns `InvalidModelRequest` before
connection. This checks the encoded request ceiling, not an 8-MiB allowance
for content plus additional uncharged framing.

Output-schema validation allows object nodes with type, title, description,
properties, required, additionalProperties, items, enum, const, anyOf, allOf,
minimum/maximum, min/max length and item count. It bounds encoding to 256 KiB,
depth to 32 and nodes to 4096; validates keyword value shapes, unique required
names that exist, supported types, nonempty enums/composition arrays, and
recursive schemas. Unknown keywords/references fail. This is a supported
provider-generation subset, not full JSON Schema validation or post-generation
conformance checking.

Requests POST to Ollama `/api/chat` or llama-server
`/v1/chat/completions`. Both use canonical system/user/assistant/tool history.
OpenAI-compatible call arguments are JSON text; Ollama arguments objects.
Assistant provider_id takes precedence over canonical id. Tool-result mapping
preserves call ID/name/content in provider-specific fields.

Llama-server sends tool_choice and stream_options.include_usage=true when
streaming; optional bound becomes max_tokens, effort becomes reasoning_effort.
Ollama omits callable tools for none, rejects Required before I/O, encodes bound
as options.num_predict and effort as think=false for none, otherwise the string.
Ollama output schema becomes format; llama-server receives strict json_schema
response_format named kvist_output and, when compilation succeeds, a grammar.

Executed request evidence in `agent_runtime/tests/model_transport.rs` includes
`direct_transports_encode_output_bounds_and_preserve_absent_defaults`,
`invalid_output_bounds_fail_before_connect_for_both_providers`,
`direct_transports_map_every_reasoning_effort_value`,
`direct_transports_encode_provider_native_output_schemas`,
`output_schema_rejects_non_objects_and_callable_tools_before_io`, and
`rejects_non_loopback_and_non_http_endpoints`.

## Framing, watchdogs, terminal state, and errors

The direct transport writes a bounded HTTP/1.1 JSON request, uses connection:
close, and polls read/write timeout conditions at 100-ms intervals with
cancellation/deadline checks. HTTP/1.0 and HTTP/1.1 responses support
Content-Length, chunked, or close-delimited bodies. Duplicate Content-Length,
conflicting framing, unsupported transfer encodings, short/excess bodies,
invalid chunk sizes/endings, and malformed records fail. Headers/trailers are
bounded around 64 KiB; stream records at 1 MiB; configured response bound
applies before delivering oversized body chunks. Non-2xx returns typed status
without retaining provider body.

Slot timeout measures response-header arrival after sending the request.
Streaming TTFT measures first body data after headers, not necessarily a decoded
token. First body bytes disable the first-body stage even when they are partial
records or heartbeat/control metadata. Cadence only arms/resets after decoded
nonempty text, reasoning, native tool fragments/calls; heartbeats, empty
deltas, role/usage/control frames and terminal markers do not reset it.
Slow initial record fragments do not arm cadence. A stream with no semantic
progress remains bounded by overall caller deadline, not a perpetually renewed
cadence timer. Unary calls have no streaming TTFT/cadence stage.

Llama SSE accepts blank lines/comments or `data:` lines, not arbitrary SSE
fields. Text is concatenated in order, reasoning kept separate using
reasoning_content/reasoning/thinking. Tool fragments merge by index in a
BTreeMap, indices <128; ID cannot change, names ≤128, argument text ≤1 MiB,
final IDs unique and arguments valid JSON objects. `[DONE]` is required.
Text/reasoning can be delivered before completion; complete tool intents are
delivered only when final assembly validates. Explicit finish remains explicit:
Stop with tools is not rewritten to ToolCalls; absent/null reason is
Other("unknown"), even with `[DONE]` or tools.

Ollama uses NDJSON, accumulates thinking/reasoning separately, requires
done=true, rejects records after terminal, caps total calls to 128, rejects
duplicate provider IDs, and synthesizes missing call IDs as ollama-call-N.
Missing/native Stop ending normalizes to ToolCalls when calls exist, otherwise
Stop; explicit Length/filter/other endings remain unchanged.

Provider model/response identity strings are bounded/control-free; llama
response ID is preserved, provider_request_id is not populated from headers.
Usage is optional. Llama reads prompt/completion counts and provided total,
otherwise saturating sum; it does not verify a supplied total matches parts.
Ollama reads prompt_eval_count/eval_count with checked sum, returning absent
usage when the input count is unavailable. Finish reason parsing preserves
provider-specific bounded text.

Failed/cancelled streams return no accepted terminal ModelTurn but may already
have emitted provisional text. This crate does not enforce that a Stop answer
is nonblank or that ending/call consistency permits tools; consumers must make
that decision. No tools are executed here.

`agent_runtime/src/error.rs` supplies typed configuration/schema/capability,
transport cancellation/deadline/slot/TTFT/cadence, status/limit/malformed,
duplicate-call, socket, process and filesystem errors. `is_retryable` is true
for transport watchdog/deadline errors; reset/abort/broken-pipe/timeout/refused/
unexpected-EOF socket kinds; HTTP 429, 500, 502, 503, 504, 507. Cancellation,
malformed/oversized responses, invalid configuration and other status/kinds are
not retryable. Direct transport itself does not retry; the embedding caller
decides. Diagnostics intentionally avoid provider response bodies, but raw
model output and arbitrary filesystem/command errors are not globally redacted
or terminal-control sanitized.

Executed stream evidence in `agent_runtime/tests/model_transport.rs` includes
`llama_server_stream_assembles_text_and_fragmented_tool_arguments`,
`llama_server_stream_terminal_marker_alone_does_not_infer_stop`,
`ollama_native_terminal_stop_or_absence_keeps_tool_call_convention`,
`streams_reject_duplicate_call_ids_before_delivering_tool_intents`,
`direct_stream_checks_deadline_after_finish_callback_returns`,
`decodes_chunked_stream_records_split_across_http_chunks`,
`slot_allocation_timeout_aborts_stalled_connection`,
`ttft_watchdog_aborts_stalled_stream`,
`cadence_ignores_heartbeats_empty_control_and_usage_frames_in_every_framing`,
`reasoning_and_native_tool_progress_start_cadence_before_intent_delivery`,
`cadence_does_not_start_on_slow_fragmented_initial_records`.

## Serving capacity and model catalogs

`DirectModelTransport::context_limit` is selected-model runtime-capacity
discovery, not training-window inference: bounded to min(configured deadline,
60 seconds) and 1 MiB. Llama GETs percent-encoded `/props?model=<id>` and reads
default_generation_settings.n_ctx. Ollama GETs `/api/ps` and selects matching
name/model, permitting implicit :latest for an untagged selector, then reads
context_length. Missing/null capacity is None; advertised values must be integer
1–1,048,576. Ollama loaded-model array is at most 128. No inference/tool action
is sent. A subsequent read of the frozen `context_limit` function observed that
non-object capacity metadata is classified as `MalformedModelResponse`, not
`InvalidModelRequest`; invalid caller model selectors still use
`InvalidModelRequest`. This classification refinement is a source observation,
not claimed as covered by the earlier package execution.
Executed `agent_runtime/tests/model_capacity.rs` covers
`selected_llama_context_uses_model_qualified_runtime_properties`,
`ollama_capacity_is_for_the_matching_loaded_model_only`, and
`capacity_discovery_rejects_cancelled_requests_and_unsafe_endpoints`.

`agent_runtime/src/catalog.rs` exposes separate bounded catalog discovery:
Ollama `/api/tags`, llama `/v1/models`, or Copilot/Gemini ACP subprocess.
Options hold endpoint, executable, working_directory, timeout, response bound,
host-discovery acknowledgement. Defaults: `.`, 5 seconds, 64 KiB, no host
acknowledgement; maxima 30 seconds/1 MiB.

Canonical ModelCatalog JSON uses format_version=1, provider, nullable
current_model_id, models with id/name/optional description. It retains first
duplicate ID in provider order, permits at most 128 unique models and requires
at least one; unknown current ID is dropped. IDs are 1–256 printable ASCII
bytes without braces; names ≤1024 bytes and descriptions ≤4096 UTF-8 bytes,
control-free (description may be empty). Ollama derives descriptions from
family/parameters/quantization/format; llama uses ID as display name.

ACP requires acknowledgement before spawning. It canonicalizes cwd, executes
one `--acp` process with inherited host environment/authority, own group, piped
stdin/stdout, discarded stderr. It sends JSON-RPC 2.0 initialize id 0 with
protocolVersion 1/clientCapabilities {}, requires confirmation, then session/new
id 1 with absolute cwd and mcpServers=[]. It extracts models.availableModels
and currentModelId. No inference prompt, tool bridge, or persistent session is
created by this client path.

Records are bounded to 64 KiB, total stdout to the option bound, polling 50 ms
with cancellation/deadline. Notifications without IDs are ignored; wrong IDs,
provider-to-client requests, protocol errors, malformed records and premature
EOF fail. Owned process group is SIGKILLed/reaped, with one-second stdout drain;
retained/escaped pipe holders fail explicitly. ETXTBSY spawn receives up to ten
10-ms retries. ACP stdin writes and direct child wait use blocking operations:
the nominal deadline is not a comprehensive hard guarantee for every kernel/
write/reap condition. A substantive discovery error takes precedence over
cleanup error. Signal installation occurs, but ACP does not register the
process group in the shared active-group registry.

Executed `agent_runtime/tests/catalog.rs` includes
`canonical_catalog_is_bounded_deduplicated_and_serializes_as_version_one`,
`acp_discovery_correlates_responses_and_uses_absolute_no_bridge_session`,
`acp_discovery_requires_acknowledgement_and_rejects_wrong_ids_and_requests`,
`acp_cleanup_deadline_applies_while_a_descendant_keeps_writing`,
`acp_timeout_terminates_and_reaps_the_process_group`,
`acp_cancellation_terminates_and_reaps_the_process_group`.

## Host supervision and interrupts

`agent_runtime/src/supervisor.rs` takes `SupervisionPolicy` with positive idle
timeout ≤3600 seconds, optional positive attempt timeout ≤24 hours, loop flag,
max retries ≤10, combined output 1–16 MiB. `CommandSpec` has program, exact
arguments and optional cwd. Commands run directly with inherited environment
and host privileges; stdin is null. No filesystem/network/credential isolation
or host-acknowledgement gate exists at this low-level API.

For each attempt a caller closure receives number and prior RetryCause and
returns a command. Empty program fails. `run_supervised` forwards stdout/stderr
as bytes; `run_supervised_capture` retains only successful-attempt output,
resetting prior captures. Reports contain attempt count; capture reports also
contain stdout/stderr. Forwarding can expose provisional output from failed
attempts and retries.

Two owned reader threads read nonblocking pipes in 4096-byte chunks with
50-ms polling, a 16-slot bounded channel, shared atomic combined-output budget,
and backpressure stop flags. The main monitor checks cancellation, optional
absolute attempt timeout, status, and output activity. Idle watches bytes from
either stream; continuing output does not extend absolute attempt timeout.
Only idle/repetition produce automatic retry; nonzero exit, overflow, absolute
timeout, cancellation, spawn/read/forwarding and retained-stream failures are
terminal. Retry count means retries after the initial attempt.

Deterministic stdout repetition checks a UTF-8-safe lossy 4096-byte suffix for
three repeated 10–512-byte blocks, four identical nonempty trimmed lines, or
three repeats of a two-line pattern. This is not semantic progress detection.
Retry notices explicitly warn that prior attempts may have changed state and
must be reconciled; retries do not roll effects back. Extra RetryCause variants
are exposed but not generated by this monitor.

Child gets its own process group. On completion/error, group is SIGKILLed,
direct child waited, readers drained and joined; a one-second drain timeout
stops readers and reports retained streams. Forwarding polls output readiness
for one second, then writes/flushes; this is not a guarantee against every
partial/blocking write. Group cleanup/direct wait itself is not bounded by the
pipe-drain timeout. This supervisor differs from agent-runner's private
single-threaded child pump: it polls/reaps via try_wait and has no universal
escaped-process/kernel-work termination guarantee.

`agent_runtime/src/interrupt.rs` installs SIGINT/SIGTERM handlers once with
SA_RESTART using unsafe sigaction calls; handler only sets an atomic flag and
sends SIGINT to the currently registered positive group. Both incoming signals
forward SIGINT. Supervision registers/clears one process-global group and
consumes the interrupt flag. The registry is not a per-session concurrent
multi-child registry; callers must coordinate. `clear_active_process_group_if`
provides compare-and-clear protection for a matching registration. Failed
handler installation warns rather than aborting.

Executed `agent_runtime/tests/supervisor.rs` includes
`retry_context_warns_about_prior_side_effects`,
`nonzero_exit_is_not_retried`,
`attempt_timeout_is_independent_of_continuing_output`,
`output_limit_is_terminal_and_not_retried`,
`escaped_descendant_retaining_output_fails_without_hanging`.
`agent_runtime/tests/cli.rs::interrupt_terminates_the_supervised_process_group`
and both interrupt module unit tests were executed.

## Loop detector, grammar, and trajectory utilities

`agent_runtime/src/loop_detection.rs` recursively normalizes JSON with sorted
object keys; action hash is SHA256(tool name concatenated with canonical JSON),
observation hash SHA256(stdout, NUL, stderr). `ActionHashRing` retains eight
actions/reasoning traces by default (capacity floor one); exposes history,
record action/observation, checks, reset/clear and base temperature.

Second consecutive identical action or third occurrence in the window is
blocked based on action hashes, not proof of filesystem invariance. Three
identical recent observation hashes trigger a stall. Reasoning compares last
accepted trace using lowercase tokenized 8-gram Jaccard similarity ≥0.88
(word sets for shorter texts). Decisions are Proceed, first-stall
SoftCorrection, subsequent TemperatureJitter (base default 0.2, +0.5 capped
2.0), and four-stall CircuitBreaker. Decisions contain messages, not actual
inference changes or authorization. Recording a non-invariant observation
resets stalls; callers can reset independently. Neither host supervisor nor
direct transport automatically wires this detector into model/tool execution.
Tests in `agent_runtime/tests/loop_detection.rs` exercise canonical hashes,
window/action/observation/reasoning paths and escalation.

`agent_runtime/src/gbnf.rs` compiles primitive types, primitive enums, arrays,
and sorted object properties to grammar text. Tool compilation produces one
`{"name":..., "arguments":...}` dispatch alternative and rejects an empty tool
list. All-required object path enforces fixed key sequence; optional-property
path permits a repeated arbitrary selection of known keys and does not ensure
required membership/uniqueness. Bounds, numeric/string/item constraints,
const/composition/additionalProperties are not fully enforced by this compiler.
Public compilation itself has no general node/depth/resource guard; the direct
transport's output-schema validator supplies separate bounds. It can omit
grammar if compilation fails while still sending native response_format.
Grammar is a generation aid, not a complete validator or authority boundary.
All nine `agent_runtime/tests/gbnf.rs` cases ran, including
`compile_strict_object_schema_with_required_properties` and
`compile_object_with_optional_properties`; these do not prove full schema
equivalence or execution inside a real llama grammar engine.

`agent_runtime/src/trajectory.rs` defines `{event: snake_case, ...}` events:
session_start/session_finish IDs/task/timestamps/totals/success; turn_start;
prompt_eval optional cached/new tokens and duration; model_reasoning;
tool_dispatch call/tool/raw args/action hash; tool_result raw stdout/stderr,
exit_code/bytes/state_mutated bool; turn_finish output tokens/reason.
Values are caller assertions, not independently approved effects or evidence.

`TrajectoryRecorder` stores a path, creates parent directories best-effort,
opens append/create per event, and writes one JSON line. It does not enforce
private modes, no-link traversal, schema-version envelopes, sequence numbers,
byte/session bounds, cross-process serialization or sync durability.
`replay_trajectory` opens a journal, parses nonblank lines into a retained
event vector, and reports IDs, highest retained turn, counts, recorded final
success. Optional max-turn stops when a TurnStart advances beyond limit;
it is not an executable checkpoint or a validator of sequence/call pairing.
Input lines/total retained events are not size-bounded. Replay never executes
recorded tools. All three trajectory tests ran, including
`record_and_replay_full_trajectory`, malformed and missing-file cases.

## Standalone CLI and operational limitations

`agent_runtime/src/main.rs` implements:

- `agent-run model`: explicit local provider, endpoint, model and prompt/file/
  editor/stdin; stream/unary, reasoning hint, output-schema JSON string,
  show-reasoning stderr or canonical turn JSON. Defaults: 300-second overall,
  60-second slot, 120-second TTFT, 1 MiB response. Requires slot < TTFT < overall.
  Tools are absent/choice none. Plain response bytes are written exactly; no
  terminal-control sanitization, autonomous tool execution, context compaction,
  retry policy or native max-output-tokens CLI flag. JSON streaming suppresses
  intermediate event presentation. Direct model CLI does not install the host
  supervisor's signal handler or link a signal watcher to its cancellation token.
- `agent-run models`: bounded provider discovery with optional endpoint or ACP
  executable, host-discovery acknowledgement, timeout/response bound, plain
  ID lines or catalog JSON. Endpoint/executable restrictions depend on provider.
  llama-cli/custom-script catalogs are rejected without spawning.
- `agent-run run`: exactly command template or named profile, optional config,
  prompt source, repeated context paths, cwd (default `.`), effort hint,
  idle timeout (default 900), loop flag, retries (default 3), output bound
  (default 1 MiB), mandatory allow-host-execution. Acknowledgement is checked
  before prompt acquisition. No overall attempt timeout is configured by this
  CLI; continuing output can continue until another terminal bound is reached.
  Retry notice is appended to prompt before re-rendering. Plain mode forwards
  streams; JSON captures and emits only `{"content": <lossy stdout>}` from the
  successful attempt, excluding stderr/attempt count.
- `agent-run setup`: selected/default profile config and force switch;
  interactive provider discovery/qualification/persistence as above.
- `agent-run replay`: diagnostic event display or replay-report JSON, optional
  max-turn filter; no process execution.

Plain prompt preamble appears on terminal stderr only, not piped stdout.
Runtime CLI errors print `error: ...` and exit 1; successful operations exit 0,
apart from clap's own argument handling. There is no terminal UI, durable
mandatory journal, project task approval interface, or sandbox selection in
this binary. Trajectory logging is opt-in library use and is not automatically
created by these run/model commands.

Executed CLI evidence includes
`agent_runtime/tests/model_cli.rs::streaming_model_command_preserves_provider_content_exactly`,
`agent_runtime/tests/cli.rs::host_execution_requires_explicit_acknowledgement`,
`standalone_json_run_emits_only_captured_content`,
`standalone_json_run_replaces_invalid_utf8_content`,
`models_reports_manual_providers_as_unsupported_without_spawning`,
`standalone_cli_replays_trajectory_json`.

## Executed verification

Executed `cargo test -p agent-runtime --offline --locked` independently using
project-local TMPDIR fixture storage; it completed successfully:

| Test target | Passed |
| --- | ---: |
| Library unit tests | 9 |
| catalog | 11 |
| cli | 25 |
| command | 6 |
| gbnf | 9 |
| loop_detection | 11 |
| model_capacity | 3 |
| model_cli | 2 |
| model_transport | 44 |
| profiles | 4 |
| setup | 9 |
| supervisor | 9 |
| trajectory | 3 |

Total 145 passed, zero failed, zero ignored. Binary unit target and doctests
ran zero tests. This full-suite execution preceded the final capacity-error
classification refinement and new large-request fixture.
After production source was reported frozen,
`cargo test -p agent-runtime --offline --locked --test model_transport
reliability_large_context_requests_are_not_limited_by_the_old_two_mib_ceiling`
passed one test, with 44 filtered out. This separate targeted result is not
reported as a rerun of the entire package.
An initial combined two-package invocation stopped on an
agent-runner integration assertion before reaching runtime; it is not counted
as successful runtime verification. Captured outputs and fixture storage were
removed before writing this record.

Transport/catalog/setup trials use loopback fixture servers and subprocess
fixtures, not live Ollama/llama-server or real Copilot/Gemini account discovery.
No real editor interaction, non-Linux build, optional-feature build, formatter/
linter, adversarial full JSON Schema equivalence assessment, or engine/sandbox
implementation assessment was performed. Source observations explain the
bounded mechanisms and gaps; passing tests are not certification of arbitrary
host isolation, side-effect rollback, provider correctness, or full recovery.

## Subsequent observation: signal-installation unsafe boundary

This refresh adds the current signal-installation behavior and supplied
native unit-test evidence. Unaffected observations and earlier independently
executed results remain unchanged.

### Source-observed behavior

`agent_runtime/src/lib.rs:1–11` documents the unsafe boundary and points to
`interrupt::install_handler`. A search of current `agent_runtime/src/**/*.rs`
found exactly two `unsafe` expressions: the SIGINT and SIGTERM `sigaction`
calls in `agent_runtime/src/interrupt.rs:66,73`.

`agent_runtime/src/interrupt.rs::install_handler` uses process-global `Once`
to attempt both installations once, with a static handler, an empty signal
mask and `SA_RESTART`. Each failed installation emits its own `tracing::warn!`
with the error and leaves that signal's existing disposition unchanged.
The two calls are attempted independently: one failure does not skip the
other. The public function returns no installation result, and `Once`
prevents a subsequent call from retrying a failed installation.

The source safety explanation confines handler work to atomics and `killpg`,
with no allocation, locks, or formatting, and describes bounded supervision
polling as the path for observing the flag. The actual handler sets the
interrupt flag with sequential consistency, loads the single registered
process group, and signals it only when positive and convertible to `i32`.
It forwards **SIGINT**, including when invoked for SIGTERM; its input signal
number is ignored. `take_interrupted` atomically returns and clears the flag.
This remains one global active-group slot, not independent registrations for
concurrent groups. These are implementation observations, not a general
proof of async-signal safety.

### Native test coverage and supplied execution evidence

Both tests are in `agent_runtime/src/interrupt.rs`:

- `interrupt::tests::install_handler_is_idempotent_and_flag_round_trips`
  calls installation twice, then checks false/true/false flag consumption.
  It does not assert the operating system's installed signal disposition or
  exercise installation-failure warnings.
- `interrupt::tests::handler_forwards_to_the_active_process_group` launches
  `sleep 60` in its own process group, registers it, directly calls the
  handler with `2`, observes the interrupt flag, waits for an unsuccessful
  child exit, and clears registration. It exercises real group signaling,
  but does not deliver an OS signal to the parent handler, verify a specific
  child termination signal, or test SIGTERM/failure/concurrent-group cases.

The explicitly supplied source/test execution file
`/home/stefan/.copilot/session-state/b6381b27-3338-4699-8a06-e12c43d5ce0e/files/runtime-unsafe-signal-tests.txt`
shows both named tests passed: two passed, zero failed/ignored, seven filtered
out. This refresh inspected that evidence without rerunning its command; it
is a selected native library-test result, not a new full-package execution or
signal-safety certification. No result is claimed for the further full
affected suite, strict lint, or release rebuild reported as in progress.
<!-- kvist-implementation-record-version: 1 -->

# Component Implementation Record
