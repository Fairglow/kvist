<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Direct Local Model Transport (`sav`)

**Component:** `sav`
**Task:** `sa-12-compliance-review`
**Compared artifacts:**
- Observed record derived **from source and tests only** — `sav/src/direct_transport.rs`
  (the sole production `ModelTransport` implementation), its unit tests, and the
  transport-conformance suites `sav/tests/{model_transport,model_capacity,
  loaded_model,brokered_tool_provider}.rs`, cross-checked against the transport's
  consumers `sav/src/{registry,model,main}.rs` and the bounded loop
  `sav/src/runtime/{driver,session}.rs`
- Canonical transport contract: `sav/CONTRACT.md` §`Additional runtime and
  transport mechanisms`, §`Data and schemas`, §`Behavioral guarantees`,
  §`Errors and failure semantics`, §`Security and authority`,
  §`Compatibility and verification`
- Decision matrix: the measured-gate table, the `Conformance tests` section, and
  the `Go/no-go gates` of `docs/rig-evaluation.md` (retained as the historical
  record of the Rig no-go; the direct transport is the conformance baseline it
  was adopted to remain)
- `sav/DESIGN.md` §`Algorithms and decisions`, §`Internal structure` (named task
  requirement anchors)
- `ROOT_CONTRACT.md` §`Change and compliance rules`
- bounded test evidence (named test functions, listed in each section) and the
  objective supply-chain measurements reproduced in §`Decision-matrix
  measurements`

**Reviewer stance:** Independent compliance reviewer for the `sa-12` direct
transport chain. Per the task instruction, the adapter's provider, streaming,
structured-intent, error, cancellation, endpoint, event, dependency, binary,
license, advisory, and TLS behavior is derived **without the intended component
documents** and then compared against the canonical transport contract and the
decision matrix. `sav/IMPL.md` is not consulted as a source of the record. This
record is the retained, independent comparison for adopting the first native
transport. It is not self-certified and establishes no acceptance or sign-off.

## Review boundary and method

The `sa-12` chain is: `sa-12-write-tests` → `sa-12-implement-code` →
`sa-12-security-audit` → **`sa-12-compliance-review` (this record)**. The
implementer and the security auditor did not author it.

**Read (derivation):** `sav/src/direct_transport.rs` in full (2,945 lines,
including its `mod tests`), the model types it implements
(`sav/src/model.rs`), the error taxonomy (`sav/src/error.rs`), the public seam
(`sav/src/lib.rs`), the registry transport builder (`sav/src/registry.rs`), the
CLI construction path (`sav/src/main.rs`), and the bounded loop that consumes
the transport (`sav/src/runtime/driver.rs` per-turn retry loop).
**Read (comparison):** `sav/CONTRACT.md`, `sav/DESIGN.md` (named anchors),
`docs/rig-evaluation.md` (decision matrix), `ROOT_CONTRACT.md`, `deny.toml`,
`sav/Cargo.toml`.
**Measured (objective):** the locked dependency graph, license set, and fresh
release binary of `kvist-sav` (commands and results in §`Decision-matrix
measurements`); the transport conformance suites below. No network operation
was possible or attempted; the advisory gate cannot be re-executed offline and
is recorded as such.
**Not executed as review:** no real `ollama` or `llama-server` process was
started; no real model, GGUF file, or chat template was loaded. The statement
that a locally installed provider matches this record is therefore *not*
established by this review — that is exactly the unevaluated-deployment class
this review is required to block, and it is carried in §`Deployment approvals`.

## Derived transport record (source + tests, no intent documents)

`sav` exposes exactly **one production model transport**: `DirectModelTransport`
(`src/direct_transport.rs:49`), implementing the provider-neutral
`ModelTransport` seam (`src/model.rs:261`). The only other `ModelTransport`
implementations in the crate are test fakes (`runtime/driver_tests.rs`,
`runtime/accounting_tests.rs`). It is constructed from an explicit
(`LocalModelProvider`, endpoint, deadline, response bound) tuple
(`DirectModelTransport::new`, `:121`) and its watchdog variants
(`with_watchdogs`, `:138`; `with_slot_timeout`, `:184`; `with_ttft_timeout`,
`:190`; `with_cadence_timeout`, `:196`). The registry pins one of two providers
per profile (`NativeModel.provider`), and the CLI `model` subcommand is
direct-provider-only (`sav::main.rs` `ModelProviderArgument` = `Ollama |
LlamaServer`).

### Provider surface (what the adapter speaks)

- **`LocalModelProvider::Ollama`** — native Ollama protocol: `POST /api/chat`
  (path selected in `execute_with_body`, `:448`), unary JSON or
  newline-delimited JSON stream, `done: true` terminal record, `done_reason`
  mapping, `prompt_eval_count`/`eval_count` usage, `options.num_predict` output
  bound, `think` reasoning-effort field, `format` structured-output field
  (`encode_request`, `:1074`).
- **`LocalModelProvider::LlamaServer`** — OpenAI-compatible protocol:
  `POST /v1/chat/completions` (path selected in `execute_with_body`, `:448`),
  unary JSON or `text/event-stream` SSE stream, `finish_reason` mapping,
  `prompt_tokens`/`completion_tokens` usage, `max_tokens` output bound,
  `reasoning_effort` field, `response_format` `json_schema` structured output
  **plus a compiled GBNF `grammar` string** (`:1143`),
  `stream_options.include_usage` on streaming, and an explicit `tool_choice`
  string (`"none"|"auto"|"required"`) — all added in `encode_request`, `:1074`.
- Provider is a **compile-time closed enum of exactly two variants**
  (`src/model.rs`); there is no string-dispatched third provider and no
  external-agent fallback from this type.
- One request per call: the transport performs exactly one HTTP exchange per
  `complete`/`stream`/`stream_with_deadline` call; retries are a host/loop
  decision (`runtime/driver.rs` per-turn retry loop), never internal.

### Endpoint and TLS surface

- Endpoint strings must be ASCII, start with `http://`, contain no user
  information, query, fragment, or path (`parse_endpoint`, `:697`). The host
  must be a **numeric loopback** IP address with an explicit nonzero port
  (`:724` `ip.is_loopback()`); IPv6 requires brackets. `https://`, hostnames,
  `user@`, `?`, `#`, and provider paths are all rejected before any I/O.
- No proxy support, no redirect handling, no TLS backend, no certificate
  handling anywhere in the module (verified by full-file inspection: the only
  scheme is `http://`, the only socket is `TcpStream::connect_timeout`
  (`connect_endpoint`, `:587`), and there is no `Authorization`/credential
  header emission — the request head built at `execute_with_body`, `:448`, is
  exactly `POST path`, `Host`, `Content-Type`, `Accept`, `Content-Length`,
  `Connection: close`).
- Discovery GETs (`get_bounded`, `:533`; `context_limit`, `:225`;
  `loaded_model`, `:354`) reuse the same endpoint parser and bounds, so the
  loopback restriction applies to metadata probes as well.

### Streaming surface

- **Fail-closed terminal requirement:** a stream that ends without its terminal
  record fails. OpenAI-compatible requires a `data: [DONE]`
  (`OpenAiStreamState::finish`, `:2127` — `"stream ended without terminal
  record"`); Ollama requires a `done: true` record (`OllamaStreamState::finish`,
  `:2217`). A partial stream therefore **never** yields a `ModelTurn`.
- **Framing:** HTTP/1.1 `Content-Length`, `Transfer-Encoding: chunked` (with
  trailers), and close-delimited bodies are all decoded (`read_response`,
  `:1329`; `parse_response_head`, `:1470`; `read_chunked_body`, `:1521`); chunk
  boundaries may split records. Premature framing EOF is `ModelTransportIo`
  with `io::ErrorKind::UnexpectedEof` (`unexpected_eof`, `:1691`) and is
  `is_retryable()` — it is never a complete turn.
- **SSE:** multi-line `data:` reassembly, `event:` allowlist with non-allowlisted
  names ignored (with bounded diagnostics), `id:`, `retry:`, comment lines
  (`:`), BOM stripping, `error` event names and chunk-level `"error"` payloads
  failing the stream, and `ping`/usage-only records without `choices` tolerated
  (`OpenAiStreamState::process_line`, `:1928`; `process_data_payload`, `:2052`).
- **Records are bounded:** a single undelimited record longer than 1 MiB fails
  (`MAX_RECORD_BYTES`, `:27`); stream tool-call index < 128, stream tool names
  ≤ 128 bytes, and argument strings ≤ 1 MiB (`merge_openai_tool_deltas`,
  `:2352`).
- **Watchdogs (streaming only):** slot-allocation (response-header) watchdog
  default 15 s (`DEFAULT_SLOT_ALLOCATION_TIMEOUT`, `:31`) →
  `SlotAllocationTimedOut`; time-to-first-token watchdog default 45 s
  (`DEFAULT_TTFT_WATCHDOG_TIMEOUT`, `:34`) → `TtftTimedOut`; optional
  inter-token cadence watchdog (`with_cadence_timeout`) →
  `InterTokenCadenceTimedOut`. Cadence starts only on decoded nonempty
  answer/reasoning text or tool name/argument progress and is not reset by
  heartbeats, empty/control deltas, usage-only records, or HTTP framing
  (the `progress` flag threaded through `process_line`, `:1928`). Unary calls
  do not apply the streaming header watchdog (`:488` `stream.then_some(...)`).
- **Overall deadline** bounds every I/O stage and is checked before and after
  every `on_event` callback and after the terminal `finish`
  (`stream_with_deadline`, `:655`, and the `deliver` closure immediately after
  it). The per-call deadline override is clamped to 24 h
  (`:664` `deadline.min(MAX_DEADLINE)`); `MAX_DEADLINE` is 24 h (`:16`).
- **Events:** only three canonical `ModelStreamEvent`s are emitted —
  `TextDelta`, `ReasoningDelta`, `ToolIntent` (complete, post-assembly)
  (`src/model.rs`); no raw-chunk, no usage, no finish event. Tool intents are
  delivered only after the terminal record, never mid-stream.
- **Identity:** provider-supplied `model`, `id`, SSE `id:`, and
  `done`/`usage` are bounded (≤ 256 bytes, control-free) and normalized into
  `ModelTurn.{model,response_id,provider_request_id,usage}`
  (`optional_bounded_string`, `:2285`); absent identities are `None`, never
  fabricated.

### Structured-intent surface

- **Descriptors:** canonical `ToolDefinition` (name ≤ 128 ASCII
  `[A-Za-z0-9._-]` — `validate_tool_name`, `:1056`; description ≤ 16 KiB;
  `parameters` a JSON object) maps to the OpenAI `tools[]`/`function` shape for
  llama-server and the same shape for Ollama (`encode_request`, `:1074`); no
  provider-hosted tools, no `type:"custom"`, no server-side callbacks.
- **Capability rejection is explicit:** `ToolChoice::Required` on Ollama fails
  with `UnsupportedCapability { provider: "ollama", capability: "required tool
  choice" }` **before** request encoding or I/O (`execute_with_body`, `:448`).
  There is no silent downgrade to `auto`.
- **Intents are untrusted data, not actions:** decoded calls become
  `ToolIntent { id, provider_id, name, arguments }` (`src/model.rs`); nothing in
  this module executes, authorizes, or dispatches them. `ToolIntent` is the
  only tool-shaped output, and it is delivered through the canonical
  event/turn types.
- **Identity invariants:** duplicate call identities in one turn are
  `Error::DuplicateToolCall` (unary `parse_openai_tool_calls`, `:2304`; the
  stream identity-change check in `merge_openai_tool_deltas`, `:2352`;
  `finish_openai_tool_calls`, `:2433`; the Ollama `provider_ids` set in
  `parse_ollama_tool_calls`, `:2649`). A stream tool call with no identity at
  finish is malformed. Ollama calls lacking a provider `id` get the
  **synthesized** turn-local identity `ollama-call-{index}` with
  `provider_id: None`.
- **Arguments:** must be a JSON object. Missing/`null`/empty-string arguments
  default to `{}` with a warning (`parse_openai_arguments_for_tool`, `:2475`;
  `parse_ollama_tool_calls`, `:2649`); stringified arguments parse leniently —
  code-fence stripping, brace extraction, control-escape/trailing-comma repair
  (`repair_tool_arguments_json`, `:2538`;
  `escape_raw_controls_and_trailing_commas`, `:2571`); unrepairable JSON
  becomes a synthetic object carrying `_malformed_arguments_error` plus a
  bounded `_malformed_arguments_preview` (`:2532`) so the host turn can recover
  without the transport inventing an alternative call. Non-object results are
  malformed either way.
- **Structured output:** `output_schema` is the bounded common JSON-Schema
  subset (256 KiB / 32 levels / 4,096 nodes, keyword allowlist —
  `validate_output_schema_node`, `:864`); it is sent as Ollama `format` or
  llama-server `response_format.json_schema` (stable envelope name
  `kvist_output`, `strict: true`) **and**, when `gbnf::compile_json_schema`
  succeeds, as an additional llama-server `grammar` field (`:1143`). Schemas
  and callable tools are rejected together before I/O
  (`validate_request`, `:788` — `"output schema cannot be combined with
  callable tools"`). The returned text is **not** validated against the schema;
  that is the caller's duty.
- **Bounds before I/O:** model selector (nonblank, control-free ASCII ≤ 256),
  message count 1–1024, total message text ≤ 8 MiB, tools ≤ 128 with unique
  names, `max_output_tokens` 1–1,048,576, tool-call identity ≤ 256 — all
  validated before the socket is opened (`validate_request`, `:788`; test
  `invalid_model_selectors_fail_before_connect_for_both_providers`).

### Error and cancellation surface

- Typed taxonomy only: `InvalidModelRequest`, `InvalidModelTransport`,
  `UnsupportedCapability`, `ModelTransportCancelled`, `ModelTransportTimedOut`,
  `SlotAllocationTimedOut{timeout}`, `TtftTimedOut{timeout}`,
  `InterTokenCadenceTimedOut{timeout}`, `ModelProviderStatus{status}`
  (bounded — carries the numeric status only; the body is intentionally
  omitted), `ModelResponseLimitExceeded{max_bytes}`,
  `MalformedModelResponse{reason}` (reasons are static or size-bounded, never
  raw payloads), `DuplicateToolCall`, `ModelTransportIo{operation, source}`
  (operation is a `&'static str`; the source is the std I/O error, which
  carries no endpoint or payload data) (`src/error.rs`).
- **Retry classification is explicit:** `Error::is_retryable()` (`src/error.rs`)
  returns true for the four timeout classes, for transport I/O kinds
  `ConnectionReset|ConnectionAborted|BrokenPipe|TimedOut|ConnectionRefused|
  UnexpectedEof`, and for provider statuses `429|500|502|503|504|507`;
  cancellation, malformed, limit, duplicate, and client errors are not
  retryable. No classification authorizes replay — retry policy is the
  loop's.
- **Cancellation** is cooperative via `CancellationToken`, checked before and
  after every read, write, callback, and the terminal assembly
  (`check_cancelled`, `:1726`; `check_deadline`, `:1734`). It surfaces as
  `Error::ModelTransportCancelled` (not an I/O or timeout error) and is
  non-retryable. A cancelled streaming call returns no terminal turn and has
  already delivered only complete events.

### Event surface (summary)

- `ModelStreamEvent::{TextDelta, ReasoningDelta, ToolIntent}` only.
- `ModelTurn` = `{text, reasoning, tool_intents, finish_reason, provider,
  model, response_id, provider_request_id, usage}`; finish reasons are
  normalized `Stop|Length|ToolCalls|ContentFilter|Other(String)` with the
  provider-specific conventions (llama-server explicit-reason preservation and
  `[DONE]`-only ⇒ `Other("unknown")` — `parse_finish_reason`, `:2728`; Ollama
  `done`-convention inference to `ToolCalls`/`Stop` — `:2749`, `:2760`);
  malformed finish reasons (non-string, > 128 bytes, control-bearing) are
  typed failures, not guesses.

### Dependency, binary, license, advisory, TLS surface

Derived from `sav/Cargo.toml` and `Cargo.lock` (objective measurements in
§`Decision-matrix measurements`): 11 declared dependencies (clap, hex, nix,
serde, serde_json, sha2, tempfile, thiserror, toml_edit, tracing, plus dev-only
tracing-subscriber); no reqwest, no tokio/async runtime, no hyper, no TLS
backend (rustls/native-tls/openssl) anywhere in the locked normal+build graph
of `kvist-sav`. TLS is structurally absent, not feature-disabled: the adapter
speaks raw TCP on loopback and the only valid scheme is `http://`.
## Decision-matrix measurements (objective, reproduced)

Measured in the verification sandbox on `rustc 1.100.0 (0ef1bc386 2026-11-09)`
against the locked `Cargo.lock` (sha256
`323e8e1bec2d07019d715bd471651a576c0ea3940ce413c73983fc499a27edb0`). Package
names resolve to the current `kvist-sav`; the matrix command in
`docs/rig-evaluation.md` names `-p sav`, which is not the locked package name —
the same command with the correct name is recorded here.

| Gate (from the decision matrix) | Measurement command (reproduced) | Observed result |
| --- | --- | --- |
| Locked package count (advisory) | `cargo tree --locked --target x86_64-unknown-linux-gnu -p kvist-sav -e normal,build --prefix none \| sed 's/ (\*)$//' \| sort -u \| wc -l` | **62** unique packages (normal+build). Variants: normal-only 60, build-only 1, all-edges (normal,build,dev) 66. The historical "49 direct-only" figure predates the current locked dependency set; both numbers are decision measurements, not a hard limit. The 75-package *marginal* review guideline is for a *delta* against a baseline, not an absolute count. |
| Release binary size (≤ 15 MiB gate) | `cargo build --release --offline --locked -p kvist-sav --target-dir <fresh>`; `stat -c %s <fresh>/release/sav` | **2,242,248 bytes** (2.04 MiB) unstripped, fresh target directory, `x86_64-unknown-linux-gnu`, release profile. Well below the 15 MiB limit and below the 2,473,416-byte direct-only baseline recorded in the decision matrix (the current toolchain is newer, so the absolute figure differs; the comparison that matters — direct-only, not a framework — is unchanged). |
| TLS with defaults disabled | `cargo tree --locked --target x86_64-unknown-linux-gnu -p kvist-sav -e normal,build --prefix none \| grep -iE 'rustls\|native-tls\|openssl\|reqwest\|tokio\|hyper\|async'` | **No match.** No Rustls, no native-tls, no OpenSSL, no reqwest, no tokio, no hyper, no async runtime is selected. Local-only scope is structurally enforced, not feature-disabled. |
| Licenses (`deny.toml` allow list) | `cargo deny check licenses bans sources` | **`bans ok, licenses ok, sources ok`** (exit 0). The workspace `deny.toml` allow list is `AGPL-3.0-or-later, Apache-2.0, BSD-3-Clause, MIT, MPL-2.0, Unicode-3.0, Zlib`. The direct-only graph's license tokens (from `cargo metadata` over the 60 third-party crates) are all OR/AND clauses whose **every** option resolves to an allowed license: `MIT (9)`, `MIT OR Apache-2.0 (40)`, `Apache-2.0 OR MIT (7)`, `MIT/Apache-2.0 (1)`, `(MIT OR Apache-2.0) AND Unicode-3.0 (1)`, `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT (2)`, `Unlicense OR MIT (1)`. Every token has at least one allowed alternative, so the gate passes. |
| Advisories (`cargo deny check advisories`) | `cargo deny check advisories` | **Not re-executable offline.** The RustSec advisory database must be cloned from `https://github.com/RustSec/advisory-db`, which is network-denied in this sandbox (`Could not resolve host: github.com`). There is no vendored advisory DB on disk. This gate is therefore **unevaluated in this environment**, not failed. It must be re-run with network access before the advisory result is asserted. `deny.toml` sets `[advisories] ignore = []` and `unmaintained = "none"`, so a full run is required for a clean pass. |
| Sources (`deny.toml` sources) | (same `cargo deny check licenses bans sources` run) | **`sources ok`** — only the crates.io registry index is referenced; no git source, no unknown registry. |

**Decision-matrix conformance tests (from `docs/rig-evaluation.md`
`Conformance tests`), mapped to the bounded suites executed here:**

| Matrix conformance area | Suite / test evidence (executed) | Result |
| --- | --- | --- |
| Text and streaming: unary success; split SSE/NDJSON frames; ordering and terminal assembly; non-success/malformed/truncated/missing-terminal; cancellation and deadline races; normalized identity/usage/finish | `tests/model_transport.rs` — `llama_server_unary_maps_messages_tools_and_tool_calls`, `ollama_unary_maps_native_tool_calls_and_rejects_required_choice`, `llama_server_stream_assembles_text_and_fragmented_tool_arguments`, `ollama_stream_emits_reasoning_before_answer_text`, `decodes_chunked_stream_records_split_across_http_chunks`, `ollama_stream_requires_terminal_record_and_emits_complete_intent`, `llama_server_stream_rejects_truncated_sse_and_provider_errors`, `streaming_cancellation_after_a_delta_returns_no_terminal_turn`, `deadline_interrupts_a_stalled_provider`, `stream_with_deadline_applies_the_override_not_the_base`, `streaming_emits_before_the_terminal_record_arrives` | pass (50/50 in the suite) |
| Structured tool intent: descriptor→provider schema; valid + streamed-argument assembly; stable identity; malformed/invalid arguments; duplicate IDs and duplicate terminal calls; multiple calls/ordering (sequential); unsupported named/parallel choice; no silent downgrade; tool results→next request; exact server/model/chat-template matrix | `tests/model_transport.rs` — `direct_transports_encode_provider_native_output_schemas`, `rejects_duplicate_calls_invalid_arguments_and_oversized_responses`, `streams_reject_duplicate_call_ids_before_delivering_tool_intents`, `llama_server_stream_rejects_malformed_tool_deltas_without_repairing_them`, `both_providers_preserve_non_success_finishes_with_tool_intents`, `ollama_unary_maps_native_tool_calls_and_rejects_required_choice`; `tests/brokered_tool_provider.rs` (14 tests, broker-mediated intent dispatch, sequential) | pass (transport 50/50; brokered 14/14). The **exact live server/model/chat-template matrix is the one area not executed here** (no real provider run); it is carried as the unevaluated item in §`Deployment approvals`. |
| Authority and redaction: endpoint/credential refs from approved profiles only; no untyped provider JSON API; no tool execution; no host tool enabled; no framework type escapes; raw requests/responses/credentials/transcripts absent from default events and evidence; TRACE sentinel coverage of logs and formatted errors | `tests/model_transport.rs` — `cancellation_and_provider_errors_are_typed_and_redacted` (asserts `SENTINEL_PROVIDER_SECRET` is absent from `error.to_string()` and `format!("{error:?}")` after a 500 and after an SSE `error` event), `llama_server_stream_rejects_truncated_sse_and_provider_errors` (same sentinel, stream path). **However** these tests assert only on the **returned error string**, not on a live `tracing` subscriber or on **stderr**. See D-4 (diagnostic leakage via `eprintln!`/`tracing::warn!` value previews) and D-5 (no TRACE-subscriber sentinel test) in §`Comparison — decision matrix`. | pass as to the error-string assertions; **two residual gaps retained** (D-4, D-5). |
| Build and supply chain: locked Linux build/tests; Rust MSRV and current-stable builds; complete feature/dependency inventory; no facade/agent/derive/vector/memory companion crate; local-only build has no selected TLS backend and makes no HTTPS claim; release binary delta ≤ 15 MiB; every transitive license in the allowed set (or explicit approval); `cargo deny check advisories` with no unwaived advisory | §`Decision-matrix measurements` above; `sav/Cargo.toml` (no features, `default = []`, no companion crate); `deny.toml` | build + TLS + license + sources pass; **advisory unevaluated offline** (see table); **MSRV** is `rust-version = "1.95"` (declared in `sav/Cargo.toml`) — the build here ran on 1.100.0 (current stable), which satisfies but does not equal the declared MSRV, and the matrix also asks for the specific MSRV compiler build, which was not separately executed. |

**Test execution record (this review ran the suites):**
`cargo test --offline --locked -p kvist-sav --test model_transport --test
model_capacity --test loaded_model --test brokered_tool_provider` →
`model_transport` **50 passed / 0 failed**, `model_capacity` **3 passed / 0
failed**, `loaded_model` **5 passed / 0 failed**, `brokered_tool_provider`
**14 passed / 0 failed**. Plus `--lib direct_transport` unit tests **6 passed /
0 failed** (of 58 lib tests filtered). Total for the transport-relevant suites
run by this review: **78 passed, 0 failed**. (The full `sav` suite is run
automatically by Kvist in the verification sandbox; this review asserts only
the named, transport-relevant subset above.)
## Comparison — canonical transport contract (`sav/CONTRACT.md`)

Each clause below is compared between the *derived* record (source + tests) and
the *intended* contract. No clause comparison found a behavioral contradiction.

| ID | Contract clause (canonical) | Observed behavior | Status |
| --- | --- | --- | --- |
| C-1 | Bounded metadata GETs and unary inference allow headers within the complete caller deadline; **only streaming inference** applies the `slot_allocation_timeout` header watchdog (default 15 s); `SlotAllocationTimedOut` does not prove slot exhaustion; overall deadline and cancellation bound every I/O stage; **one request** per call; embedding host owns retries | Slot watchdog default 15 s (`:31`); applied only when `is_streaming` (`:488` `stream.then_some(...)`); `SlotAllocationTimedOut{timeout}` carries no slot-state claim; `check_cancelled`/`check_deadline` around every read/write/callback; exactly one HTTP exchange per call; `runtime/driver.rs` owns the retry loop | compliant |
| C-2 | Premature HTTP-framing EOF is `ModelTransportIo` with `UnexpectedEof`; invalid *complete* framing/payloads remain malformed responses; **partial stream events are provisional and never a complete executable turn** | `unexpected_eof` (`:1691`) → `ModelTransportIo{UnexpectedEof}`, `is_retryable()`; `read_response`/`read_chunked_body` return `MalformedModelResponse` for invalid complete framing; terminal requirement (`:2127`, `:2217`) means no `ModelTurn` without the terminal record; `close_delimited_body_is_bounded_before_stream_events` proves no events escape a failing body | compliant |
| C-3 | `DirectModelTransport::loaded_model` reads `/props` for standalone llama-server identity (`model`, `model_alias`, then `model_path`); router mode does read-only `/models` discovery and returns a *loaded* entry's `id`, never the alias; both share the caller's deadline and cancellation; responses ≤ 1 MiB; catalogs ≤ 128; identifiers nonblank/control-free ≤ 256 B; **Ollama uses `/api/ps`** | `loaded_model` (`:354`) branches on `role == "router"` → `/models` + `status/value == "loaded"`; standalone → `model`/`model_alias`/`model_path`; Ollama → `/api/ps` with `name`/`model`; single shared `deadline.min(MAX_CAPACITY_DISCOVERY)`; 1 MiB bound; 128-entry catalog bound; `loaded_model_id` enforces the identity bounds | compliant |
| C-4 | `context_limit` discovers optional serving capacity bounded to 1..=1,048,576 without inference; llama-server `/models` first, **HTTP 404 alone** selects `/props?model=` fallback; Ollama uses matching loaded-model `/api/ps` metadata (untagged name also matches `:latest`); absent metadata is `None`, malformed is an error; numeric-loopback-only; llama-server streaming includes `stream_options.include_usage` | `context_limit` (`:225`) → `llama_router_context_limit` (`:253`) on 404 → `llama_standalone_context_limit` (`:280`); `parse_context_limit` (`:523`) rejects 0/negative/`>1048576`/non-integer; `ollama_context_limit` (`:300`) matches `name`/`model` incl. `:latest`; `get_bounded` enforces loopback and 1 MiB; `include_usage` added for streaming llama-server in `encode_request` | compliant |
| C-5 | `ModelRequest.output_schema` is the bounded common JSON-Schema 2020-12 subset (no references/definitions/conditionals/`oneOf`/pattern-properties/unknown keywords; 256 KiB / 32 levels / 4,096 nodes); sent **unchanged** as Ollama `format` or OpenAI-compatible `response_format.json_schema` with stable envelope `kvist_output`; it is a provider-native constraint, **not** evidence returned text conforms | `validate_output_schema_node` (`:864`) enforces the exact allowlist, sizes, and levels; `encode_request` (`:1074`) sends the schema object unchanged under `format` (Ollama) / `response_format.json_schema{name:"kvist_output",strict:true}` (llama-server); returned text is not schema-validated (contract's own words); see D-2 for the *additional* `grammar` field | compliant (with D-2 observation) |
| C-6 | `max_output_tokens: Option<u32>` bounds generation when supplied; accepted 1..=1,048,576; invalid values fail **before network I/O**; llama-server `max_tokens`; Ollama `options.num_predict`; absent bound leaves provider defaults | `validate_request` (`:788`) rejects `0`/`>1048576` before connect (`invalid_output_bounds_fail_before_connect_for_both_providers`); `encode_request` (`:1074`) writes `max_tokens` / `options.num_predict`; absent → field omitted (`direct_transports_encode_output_bounds_and_preserve_absent_defaults`) | compliant |
| C-7 | Direct model streaming emits answer deltas and, when requested and supplied, separate reasoning deltas; plain text writes only answer to stdout; `--show-reasoning` writes reasoning to stderr; `--json` emits one canonical terminal turn and is mutually exclusive with `--show-reasoning`; no added line termination | Transport emits `TextDelta`/`ReasoningDelta` separately (`:1928`, `:2163`); the CLI presentation rules are in `main.rs` `model` handling (answer to stdout, `--show-reasoning` to stderr, `--json` one object, `conflicts_with`); `ModelTurn.text` is the raw accumulated string, no terminator added | compliant |
| C-8 | **The direct transport is the sole model transport.** Structured-output schemas and callable tools cannot be requested in the same turn; that combination fails **before provider I/O**. Callers parse and validate returned content independently | Only production `ModelTransport` impl is `DirectModelTransport` (verified: all others are test fakes); `validate_request` (`:788`) rejects schema+tools pre-I/O (`output_schema_rejects_non_objects_and_callable_tools_before_io`); no response validation in the module | compliant |
| C-9 | llama-server preserves explicit `finish_reason:"stop"` as `Stop` **including with tools**; missing/null unary or `[DONE]`-without-reason → `Other("unknown")`; null/absent intermediate deltas do not replace an explicit reason; turns remain untrusted; non-string/oversized/control-bearing reasons are typed failures | `parse_finish_reason` (`:2728`): `stop→Stop` unconditionally, `null/None→Other("unknown")`, non-string/`>128`/control → malformed; stream finish preserves first explicit reason (`finish_reason` set only when non-null, `:2052`); Ollama `done`-convention (`:2749`/`:2760`) infers `ToolCalls`/`Stop` only for stop/missing/null; tests `llama_server_unary_explicit_stop_with_tools_remains_stop`, `..._stream_terminal_marker_alone_does_not_infer_stop`, `..._null_or_absent_deltas_do_not_erase_explicit_finish`, `both_providers_reject_malformed_finish_reasons`, `ollama_native_terminal_stop_or_absence_keeps_tool_call_convention` | compliant |
| C-10 | `with_cadence_timeout` starts after the first decoded nonempty answer/reasoning fragment or native tool name/argument progress; resets only on such progress; llama-server tool fragments count when their records decode, Ollama native calls count before terminal delivery; heartbeats/empty deltas/control fields/usage-only records/HTTP chunk framing/undecoded bytes **do not** reset; unary ignores cadence | Cadence arming via the `progress` flag threaded through `process_line` (`:1928`); `observe_body(progress)` in `BodyWatchdog` (`:1274`) sets `cadence_deadline` only on progress; `cadence_ignores_heartbeats_empty_control_and_usage_frames_in_every_framing`, `cadence_does_not_start_on_slow_fragmented_initial_records`, `reasoning_and_native_tool_progress_start_cadence_before_intent_delivery`, `cadence_tracks_tool_fragments_and_reasoning_before_late_tool_events`, `caller_deadline_and_cancellation_still_bound_heartbeat_streams` | compliant |
| C-11 | Slot watchdog bounds header acquisition; **first-body watchdog bounds initial streaming body acquisition, not the first decoded token**; incomplete initial record does not start cadence; until meaningful progress decodes, wall deadline and byte bounds apply; once cadence starts, an incomplete later record remains under the cadence deadline; earliest applicable deadline and cancellation remain effective through framing and event delivery | `BodyWatchdog` (`:1266`): `first_body`/`ttft` phase → `TtftTimedOut`, `cadence_deadline` phase → `InterTokenCadenceTimedOut`; `read_deadline()` picks the earliest stage (`:1290`); `check` before/after every read and callback; `ttft_watchdog_aborts_stalled_stream`, `inter_token_cadence_watchdog_aborts_hung_stream`, `cadence_does_not_start_on_slow_fragmented_initial_records` | compliant |
| C-12 | A streaming call may carry an explicit per-call deadline overriding the configured one; the default streaming call uses the configured deadline; transports that cannot vary the deadline keep their fixed bound via the default path | `stream_with_deadline` (`:655`) overrides, clamped to `MAX_DEADLINE` (`:664`); `ModelTransport::stream_with_deadline` default impl drops the override for other transports (`src/model.rs:261`); `stream_with_deadline_applies_the_override_not_the_base` proves the short override, not the 30 s configured bound, ends a stalled turn | compliant |
| C-13 | Endpoint probes are advisory and bounded; **local transports disable proxies and redirects, allow only numeric loopback HTTP endpoints, bound request/response data, and suppress payload-bearing third-party tracing from canonical output**; tool calls are untrusted intent and only a typed broker validates/canonicalizes/dispatches | `parse_endpoint` (`:697`) rejects non-loopback/non-http; no proxy/redirect code exists in the module; request ≤ 8 MiB (`MAX_REQUEST_BYTES`), response bounded per construction, records ≤ 1 MiB; tool output is `ToolIntent` only and the broker is `runtime/session.rs` `ToolExecutor` (host-supplied); tests `rejects_non_loopback_and_non_http_endpoints`, `reliability_large_context_requests_are_not_limited_by_the_old_two_mib_ceiling`, `close_delimited_body_is_bounded_before_stream_events` | compliant (with D-4 observation on the stderr/warn diagnostic path) |
| C-14 | Model transports return canonical output or explicit typed failure and **never authorize or execute tools**; unsupported capability fails rather than silently degrading | No execution path exists in the module; `ToolChoice::Required` on Ollama → `UnsupportedCapability` pre-I/O (`:448`); no capability is downgraded anywhere (`ollama_unary_maps_native_tool_calls_and_rejects_required_choice`) | compliant |
| C-15 | Blank/conflicting/oversized/non-UTF-8/malformed/unsupported/unknown inputs fail before the relevant provider effect; provider and HTTP failures preserve bounded actionable context **without raw payloads or secrets**; cancellation/timeout/overflow run explicit cleanup | All request bounds validated pre-connect (`:788`); error strings are static or `&'static str` operations; `ModelProviderStatus` carries the number only; `check_cancelled`/`check_deadline` around every I/O; socket dropped on scope exit (no keep-alive to leak); `cancellation_and_provider_errors_are_typed_and_redacted` asserts the sentinel secret is absent from both `Display` and `Debug` of the error | compliant (with D-4/D-5 observations) |
| C-16 | Conformance uses deterministic command, profile, setup, supervisor, direct transport, stream, and cancellation tests; real provider promotion requires comparison with the direct adapter, dependency review, security audit, and independent compliance review | The deterministic suites exist and were executed by this review (§`Decision-matrix measurements`, 78/78 transport-relevant tests pass); this record is the independent compliance review the clause names | compliant |

**Total hard behavioral mismatches against the canonical transport contract: 0.**

## Comparison — decision matrix (`docs/rig-evaluation.md`)

The decision matrix was written for the rejected Rig experiment, but its gates
define the acceptance bar any native transport (including the retained direct
baseline) must meet, and the matrix's `Go/no-go gates` are the decision
criterion the task names. Mapping the observed record and measurements:

| Gate | Requirement | Observed | Status |
| --- | --- | --- | --- |
| 1 | Exact crate/resolved deps build and test on the MSRV and current stable | `sav` MSRV is declared 1.95; the reviewed build ran on 1.100.0 (current stable) — passes current-stable; the **exact MSRV (1.95) build was not separately executed** in this review | partially evaluated (D-6) |
| 2 | Private adapter exposes only standalone-owned canonical types | `DirectModelTransport` is the only non-fake `ModelTransport` impl; it consumes/produces only `sav` canonical types (`ModelRequest`, `ModelTurn`, `ModelStreamEvent`, `ToolIntent`, `FinishReason`, `ModelUsage`, `CancellationToken`); no third-party type crosses the seam | pass |
| 3 | Endpoint, credentials, capability decisions, cancellation, deadlines, and redaction remain host-controlled | Endpoint and provider are operator-selected at construction; no credential routing exists; `UnsupportedCapability` is explicit; cancellation via host-supplied `CancellationToken`; deadlines are caller-supplied (including per-call override); redaction bounds are in the transport and errors carry no payload (D-4 residual on the stderr diagnostic path) | pass (with D-4) |
| 4 | Fake, Ollama, and llama-server text/stream conformance passes | Executed: 50/50 `model_transport` (fake OpenAI-compatible + Ollama fixtures, both providers, unary + stream), 3/3 `model_capacity`, 5/5 `loaded_model` | pass (fake + both provider protocols) |
| 5 | Transport-only structured tool-intent conformance passes **for the exact deployment matrix** | Descriptor/intent conversion, stable IDs, malformed/duplicate handling, and sequential broker dispatch pass against fakes (50/50 + 14/14); the **exact live llama-server version / GGUF model / chat-template matrix was not executed** — no real provider was run | partially evaluated (see §`Deployment approvals`) |
| 6 | Malformed and truncated streams fail closed | Truncated SSE (no `[DONE]`), mid-record cut, Ollama no-terminal, oversized records, malformed tool deltas, duplicate IDs: all fail with typed errors and **no** terminal turn and **no** mid-stream tool delivery (`ollama_stream_requires_terminal_record_and_emits_complete_intent`, `llama_server_stream_rejects_truncated_sse_and_provider_errors`, `streams_reject_duplicate_call_ids_before_delivering_tool_intents`, `llama_server_stream_rejects_malformed_tool_deltas_without_repairing_them`, `close_delimited_body_is_bounded_before_stream_events`) | pass |
| 7 | TRACE sentinel tests demonstrate no content leakage | Error-string sentinels pass (`SENTINEL_PROVIDER_SECRET` absent from `Display`/`Debug`). **No test installs a `tracing` subscriber at TRACE and asserts sentinel absence in logs, and the `eprintln!`/`tracing::warn!` diagnostic path is not covered by a sentinel test** | **not fully demonstrated** (D-4, D-5) |
| 8 | Dependency footprint documented; size, license, advisory, and local-HTTP/TLS decisions satisfy objective gates | Size 2.04 MiB ≤ 15 MiB (pass); license `bans/licenses/sources ok` (pass); no TLS/async backend (pass); package count 62 (advisory, documented); **advisory gate unevaluated offline** | partially evaluated (D-3) |
| 9 | An exact-version upgrade cannot bypass the conformance suite | The conformance suites are pure std + `serde_json` with no version-gated behavior; `--locked` resolution pins all versions; the suite does not branch on toolchain or dependency version | pass |

**Go/no-go for the direct transport as the first native transport:** gates 2,
3, 4, 6, and 9 **pass**; gate 1 is **partially evaluated** (current-stable
build passes, exact-MSRV build not re-executed); gate 5 is **pass for the
fake/provider-protocol matrix and unevaluated for any specific live
deployment**; gate 7 is **not fully demonstrated** (D-4/D-5); gate 8 is
**partially evaluated** (advisory unevaluated offline, D-3). No gate **fails**.
Per the task's expected outcome, the unevaluated and partially-evaluated items
are recorded and block deployment claims for every unevaluated combination
(§`Deployment approvals`) rather than being treated as passes.

## Unsupported capabilities (explicit, recorded)

Capabilities the adapter does **not** provide, each failing or absent rather
than silently degrading:

- **HTTPS/TLS** — structurally absent; only `http://` loopback endpoints are
  accepted. There is no TLS backend in the dependency graph, so a TLS
  requirement is an unmet endpoint policy, not a feature flag.
- **Non-loopback endpoints, proxies, redirects, authentication** — rejected at
  construction; no credential routing or `Authorization` header exists.
- **Ollama `ToolChoice::Required`** — explicit `UnsupportedCapability` before I/O
  (Ollama's native protocol has no required-choice constraint in this adapter).
- **Provider-hosted tools, server-side callbacks, `type:"custom"` tools** —
  never emitted; tools are descriptors only.
- **Structured output + callable tools in one turn** — rejected before I/O.
- **JSON-Schema keywords outside the bounded subset** (`$ref`, `definitions`,
  conditionals, `oneOf`, `patternProperties`, unknown keywords) — rejected at
  validation.
- **Non-string/oversized/control-bearing finish reasons, duplicate tool-call
  identities, streams without terminal records, oversized records/responses**
  — typed failures; never guesses.
- **Automatic retries** — none; one request per call, host/loop-owned retry.
- **Any provider other than Ollama and llama-server** — not representable in
  `LocalModelProvider`.
- **Schema conformance of returned text** — the schema is a generation
  constraint only; the caller must validate returned content.

## Deployment approvals (exact provider/server/model combinations)

The task requires this review to **approve exact provider/server/model
combinations** and to **block native-loop use for every unevaluated or
discrepant deployment**. Applying that:

**Approved (conformance-evaluated, no discrepancies):**

| # | Provider | Server protocol / endpoint class | Model | Mode | Basis |
| --- | --- | --- | --- | --- | --- |
| A-1 | `Ollama` | native Ollama, numeric-loopback HTTP (default `http://127.0.0.1:11434`) | any model advertised by that Ollama instance, ≤ 256-byte control-free ASCII id | unary + streaming, text + tool intent + structured output | Protocol conformance exercised against a deterministic fake of the Ollama wire protocol (both unary NDJSON and streaming, terminal requirement, `done`-convention, usage, tool calls, reasoning) — 50/50 transport tests; no contract discrepancy found |
| A-2 | `llama-server` | OpenAI-compatible `/v1/chat/completions`, numeric-loopback HTTP (default `http://127.0.0.1:9931`) | any model loaded by that llama-server, ≤ 256-byte control-free ASCII id | unary + streaming, text + tool intent + structured output (incl. GBNF `grammar`) | Protocol conformance exercised against a deterministic fake of the OpenAI-compatible wire protocol (SSE + chunked, `[DONE]`, `finish_reason` preservation, tool-call deltas, usage, reasoning) — 50/50 transport tests; no contract discrepancy found |

These two approvals are **protocol-level**: they certify that the adapter is
complete enough for the bounded loop *for the wire protocol of that provider*,
i.e., that no provider-specific semantic gap between the observed record and
the canonical contract was found for that protocol. They are **not** approvals
of any specific installed binary.

**Blocked / unevaluated (native-loop use is not approved for these):**

- **Every specific live deployment** — no real `ollama` binary/version or
  `llama-server`/GGUF/chat-template combination was exercised. Per the matrix,
  "unavailable live services or models are recorded as `unevaluated`, never
  `passed`." Any concrete deployment (exact Ollama release, exact llama-server
  release + GGUF + Jinja template) must be individually conformed and recorded
  before native-loop use. **All unevaluated combinations are blocked for
  native-loop use by this record.**
- **The advisory gate** — unevaluated offline (D-3); a deployment is not
  supply-chain-complete until `cargo deny check advisories` passes with network
  access.
- **The exact-MSRV (Rust 1.95) build** — not re-executed (D-6); current-stable
  (1.100.0) passes.
- **Any deployment that would require HTTPS/TLS, a non-loopback endpoint, a
  proxy, a redirect, or authentication** — unsupported by the adapter; blocked
  by design (recorded in §`Unsupported capabilities`).
- **Any provider other than Ollama/llama-server** — not representable; blocked.

## Findings, observations, and discrepancies

| ID | Item | Type | Severity | Disposition |
| --- | --- | --- | --- | --- |
| D-1 | `grammar` field is **undocumented in the contract.** The contract says the schema is sent "unchanged as Ollama `format` or OpenAI-compatible `response_format.json_schema` … envelope `kvist_output`." The observed llama-server request **also** carries a `grammar` (GBNF) string field derived from the schema (`:1143`), sent only when `gbnf::compile_json_schema` succeeds. This is an additive wire field the contract does not name. It does not weaken the stated constraint (the schema is still sent unchanged) and the test `direct_transports_encode_provider_native_output_schemas` asserts its presence. | observation (contract under-specification, not a behavioral defect) | low | **Retain for arbitration:** either amend `CONTRACT.md` to document the optional `grammar` field, or record it as an intentional llama.cpp-specific enhancement. No deployment block follows. |
| D-2 | (Consequence of D-1) llama-server structured-output requests can carry **two** generation constraints (`response_format` + `grammar`) simultaneously, while Ollama carries one (`format`). This asymmetry is not stated in the contract. | observation | low | Roll into the D-1 arbitration. |
| D-3 | **Advisory gate unevaluated offline.** `cargo deny check advisories` cannot run (RustSec DB is network-fetched; no vendored DB). This is an *objective measurement gap*, not a defect. | unevaluated supply-chain gate | medium | **Blocks the supply-chain "complete" claim** until re-run with network access. License/bans/sources gates pass. `deny.toml` has no waivers and `ignore = []`. |
| D-4 | **Diagnostic preview channel (stderr + tracing) is not sentinel-covered.** Several diagnostic sites emit a **bounded, control-escaped (`sanitize_sse_diagnostic`, `:2246`) preview of provider-supplied input**: the three `eprintln!` sites (`:1989`, `:2030`, `:2044`) plus adjacent `tracing::warn!` sites preview SSE `event`/`retry`/unsupported-field **values** (≤ 256/128/64 chars, `:1984`, `:2025`, `:2037`–`:2038`); `warn_extra_json_fields` (`:2458`) previews an **unrecognized tool-call field value** (`value_preview`, ≤ 160, `:2464`); and `parse_lenient_tool_arguments` previews the **raw malformed tool-argument string** (`raw_preview`, ≤ 240, `:2517` and `:2522`–`:2527`) when repair is attempted. These previews are emitted only for *unsupported/malformed* input, are bounded and control-escaped, and **never** carry successfully-parsed answer/reasoning text, tool-call names, *valid* tool arguments, the prompt, or credentials (the transport emits no credential header). The contract's "suppress payload-bearing third-party tracing from canonical output" governs *canonical output* (events/turns), which is clean. But matrix gate 7 (TRACE sentinel) and the redaction tests assert only on the **returned error string** — not on a live `tracing` subscriber or on **stderr** — so a sentinel placed in an unsupported SSE field value, an unrecognized tool-call field, or a malformed tool-argument string would surface in one of these previews without a test catching it. This is a bounded, non-data-plane diagnostic path, not a payload leak — but it is a residual the sentinel suite does not close. | observation (redaction coverage gap) | medium | **Retain for arbitration:** either (a) extend the sentinel test to install a TRACE `tracing` subscriber and capture stderr, asserting sentinel absence for an unsupported-field value; or (b) drop the value preview from the `eprintln!`/`tracing::warn!` diagnostics (log field name + length only). No deployment block follows (the previews are bounded, control-escaped, and exclude `data:` payloads), but the matrix gate-7 claim of "no content leakage demonstrated" should not be asserted as fully met until one of these is done. |
| D-5 | **No TRACE-subscriber sentinel test exists** in the transport suites (only `supervisor.rs` installs a subscriber, at `DEBUG`). Matrix gate 7 explicitly asks that sentinel prompts/tool args/results/endpoint credentials/provider errors be run "with TRACE globally enabled" and proven absent from logs, errors, events, and attachments. The observed tests cover errors, events, and canonical output; **logs (a subscriber) are not covered.** | conformance-coverage gap | medium | Roll into D-4. Until a subscriber-level sentinel test exists, gate 7 is "not fully demonstrated," not "pass." |
| D-6 | **Exact-MSRV build not re-executed.** Matrix gate 1 asks for build+test on the MSRV *and* current stable. Declared MSRV is 1.95; this review ran on 1.100.0. The 1.95 build was not separately run. | unevaluated build gate | low | Re-run on a 1.95 toolchain to close. No discrepancy found at current-stable. |
| D-7 | **`syn` appears at two versions (2.0.119 and 3.0.10)** in the locked graph (from different proc-macro deps). `deny.toml` sets `multiple-versions = "warn"`, so this is a warning, not a deny; `bans ok` confirms it is not a failure. | observation (benign) | none | No action; noted for completeness of the dependency inventory. |

**No discrepancy found that alters the behavioral contract or the go/no-go
gates.** D-1/D-2 are contract-coverage (documentation) items; D-3/D-4/D-5/D-6
are objective measurement/coverage gaps that are recorded and block the
corresponding *claims* (supply-chain-complete, gate-7-fully-demonstrated,
exact-MSRV) rather than being treated as passes; D-7 is benign.

## Bounded-loop sufficiency and replaceability

- **Complete enough for the bounded loop:** the loop (`runtime/driver.rs`)
  consumes the transport only through the `ModelTransport` seam —
  `complete`/`stream`/`stream_with_deadline`/`deadline` — and supplies its own
  tools (`ToolExecutor`), execution backend, recorder, budgets, and retry
  policy. The transport provides bounded canonical text, reasoning, complete
  tool intents (never executed), typed errors with explicit retryability,
  per-call deadline override for the loop's growing-budged retries, and
  cancellation. The loop's per-turn retry uses `transport.deadline()` as the
  base and `stream_with_deadline` to grant a larger budget per attempt, capped
  at `base × max_attempts` — exactly the seam the contract's per-call-deadline
  clause (C-12) specifies. No transport behavior forces the loop to do
  unbounded work or to interpret provider-specific material.
- **Remains replaceable:** the transport is a private implementation behind the
  public, provider-neutral `ModelTransport` trait; it appears in no public
  type's signature except as the concrete `DirectModelTransport` handle the
  host constructs. The canonical types (`ModelRequest`/`ModelTurn`/
  `ModelStreamEvent`/`ToolIntent`) are owned by `sav::model`, not by the
  transport. A future adapter (e.g., a re-evaluated pinned Rig release) can
  implement the same trait without touching the loop, the broker, the
  execution backend, or the canonical contract — which is precisely the
  replaceability the task's purpose requires and the decision matrix's
  "best future boundary" (hybrid) envisions.

## Verdict

| ID | Clause / finding | Status | Severity |
| --- | --- | --- | --- |
| C-1 | Streaming-only slot watchdog (15 s default); one request per call; host retries | compliant | none |
| C-2 | Premature framing EOF = retryable `UnexpectedEof`; partial stream ≠ complete turn | compliant | none |
| C-3 | `loaded_model` router/standalone/Ollama discovery; bounds | compliant | none |
| C-4 | `context_limit` serving capacity; 404-only `/props` fallback; `include_usage` | compliant | none |
| C-5 | `output_schema` bounded subset; unchanged wire; not a conformance proof | compliant (D-1) | low |
| C-6 | `max_output_tokens` wire shape; fail before I/O | compliant | none |
| C-7 | Streaming answer/reasoning separation; CLI presentation | compliant | none |
| C-8 | Sole transport; schema+tools mutually exclusive; caller validates | compliant | none |
| C-9 | Finish-reason conventions per provider; malformed = typed failure | compliant | none |
| C-10 | Cadence arming on real progress only; heartbeats don't reset | compliant | none |
| C-11 | Slot/first-body/cadence watchdogs; earliest deadline wins | compliant | none |
| C-12 | Per-call deadline override; clamped; default path for fixed transports | compliant | none |
| C-13 | Loopback-only, no proxy/redirect, bounded data; tool intent untrusted | compliant (D-4) | low |
| C-14 | No tool execution; explicit unsupported capability | compliant | none |
| C-15 | Bounds before effect; redaction; typed failure | compliant (D-4/D-5) | medium |
| C-16 | Deterministic conformance; independent compliance review required | compliant | none |
| D-1 | `grammar` wire field undocumented in contract | observation | low |
| D-2 | Dual-constraint (schema+grammar) on llama-server, not in contract | observation | low |
| D-3 | Advisory gate unevaluated offline | unevaluated | medium |
| D-4 | stderr/tracing diagnostic preview not sentinel-covered | observation | medium |
| D-5 | No TRACE-subscriber sentinel test | conformance gap | medium |
| D-6 | Exact-MSRV (1.95) build not re-executed | unevaluated | low |
| D-7 | `syn` dual-version in graph (warn-level, benign) | observation | none |

**Total hard behavioral mismatches against the canonical transport contract:
0.**
**Decision-matrix go/no-go gates that fully pass: 2, 3, 4, 6, 9.**
**Gates partially evaluated / unevaluated (not failing): 1 (D-6), 5 (live
deployment matrix), 7 (D-4/D-5), 8 (D-3 advisory).**
**Deployment approvals: A-1 (Ollama protocol) and A-2 (llama-server
OpenAI-compatible protocol) approved at the protocol level; every specific
live deployment and every unevaluated gate is blocked for native-loop use
until individually evaluated.**

## Conclusion

The first native transport (`DirectModelTransport`) is **complete enough for
the bounded loop and remains replaceable**. Derived independently from source
and tests without the intended documents, its provider, streaming,
structured-intent, error, cancellation, endpoint, and event behavior is
**compliant on all sixteen canonical-contract clause comparisons** with **zero
hard behavioral mismatches**: it speaks exactly two provider protocols over
numeric-loopback plain HTTP, is the sole production model transport, fails
closed on truncated/malformed streams, delivers tool calls only as untrusted
complete `ToolIntent`s, rejects unsupported capabilities explicitly (notably
Ollama required-choice), bounds every input and output before and during I/O,
classifies every error with an explicit retryability, preserves cancellation
throughout, and exposes a provider-neutral `ModelTransport` seam the loop and
any future adapter can consume without touching host authority. The dependency
graph is small (62 locked normal+build packages), free of TLS/async/reqwest
(no HTTPS claim, no TLS backend selected), and passes the license/bans/sources
gates with no selected advisory waiver.

Because the expected outcome requires **approving exact combinations and
blocking every unevaluated or discrepant deployment**, this record approves
the two *protocol-level* deployments (A-1 Ollama, A-2 llama-server
OpenAI-compatible) and **blocks native-loop use for every specific live
deployment** (no real `ollama`/`llama-server`/GGUF/chat-template was
exercised — recorded `unevaluated`, never `passed`), as well as for any
deployment requiring HTTPS/TLS, non-loopback, proxy, redirect, or
authentication (unsupported by design).

The retained items are **not behavioral defects**: D-1/D-2 are contract
documentation gaps (the additive `grammar` field); D-3 is an objective
measurement gap (advisory DB unreachable offline) that blocks the
supply-chain-complete claim; D-4/D-5 are redaction-coverage gaps (a bounded,
control-escaped stderr/tracing preview of unsupported SSE fields, unrecognized
tool-call fields, and malformed tool-argument strings is not closed by a
TRACE-subscriber sentinel test) that block the gate-7 "no content leakage
demonstrated" claim; D-6 is an unevaluated exact-MSRV build; D-7 is benign. Formal sign-off is blocked while D-1, D-3, D-4/D-5, and D-6 remain
unresolved or unarbitrated, and while the exact live deployment matrix is
unevaluated; acceptance, if required, remains subject to the separate review
receipt/acknowledgement or a reasoned exception per the change-and-compliance
rule.

This record is the independent gate disposition for the `sa-12` direct
transport chain. It is not self-certified, establishes no clause-level
certification beyond the record above, and does not constitute acceptance or
sign-off.
