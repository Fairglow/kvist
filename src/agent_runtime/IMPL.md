<!-- kvist-implementation-record-version: 1 -->
# Root Component Implementation Record

## Package and public API

The `agent-runtime` Cargo package builds the `agent_runtime` library and
the `agent-run` binary. Compilation emits an error on targets other than
Linux. The library publicly exports prompt resolution, command rendering, raw
command splitting, supervision policy, attempt context, command specification,
execution report, retry cause, and its error/result types.

`SupervisionPolicy` contains an idle duration, optional per-attempt wall
duration, loop-detection switch, retry count, and combined-output byte limit.
`CommandSpec` contains a program, arguments, and optional working directory.
`run_supervised` receives a callback that builds a fresh command from each
`AttemptContext`; its successful report contains the number of attempts.

`ModelProfile` contains a case-sensitive name, provider identifier, and command
template. Public profile APIs resolve the Linux user configuration path, load
all profiles or one named profile, create or replace a profile, collect a
profile interactively, verify its exact command, and run collection plus
persistence as a setup wizard.

The public model API contains canonical messages, tool descriptors, tool
choice, untrusted tool intents, model turns, finish reasons, usage,
stream events, a cloneable cancellation token, and the `ModelTransport` trait.
`DirectModelTransport` implements that trait for Ollama and llama-server
without exposing provider response types. With the non-default
`rig-transport` feature, `RigModelTransport` implements the same public
component-owned boundary over exactly pinned `rig-core` 0.42.0.

## Direct local model transport

Transport construction accepts a typed provider, an ASCII HTTP loopback
endpoint with an explicit port, a positive deadline through 24 hours, and a
positive response limit through 16 MiB. Endpoint parsing accepts loopback IP
addresses or `localhost`; stores only resolved loopback socket addresses; and
rejects HTTPS, credentials, query, fragment, provider paths, non-loopback
hosts, controls, whitespace, and ambiguous IPv6 authority.

The adapter writes HTTP/1.1 POST requests directly through `TcpStream`.
Ollama uses `/api/chat`; llama-server uses `/v1/chat/completions`. It does not
consult proxy environment variables, follow redirects, add credentials, or
select a provider path from response data. Reads and writes use 100 ms socket
timeouts to recheck cooperative cancellation and the overall deadline.

Canonical requests are validated before connecting. They require a bounded
ASCII model selector, 1 through 1,024 messages, no more than 128 unique tools,
at most 8 MiB of message text, object tool schemas and arguments, bounded tool
identities, and no more than 2 MiB of serialized JSON. Ollama rejects required
tool choice before network access. llama-server receives explicit `none`,
`auto`, or `required` tool choice.

Response handling supports Content-Length, connection-close, and chunked
framing. Headers, decoded body bytes, and individual streaming records have
independent bounds. Conflicting or duplicate framing, truncated bodies and
chunks, non-success status, malformed JSON, missing stream terminals, invalid
arguments, and duplicate call identities produce typed errors. Provider error
bodies are discarded before error construction.

llama-server streaming incrementally parses SSE and assembles indexed,
fragmented tool calls. Ollama streaming incrementally parses NDJSON. Text
deltas are delivered to the callback as their complete records arrive; tool
intents are delivered only after complete object arguments validate. Ollama
calls without provider identities receive deterministic turn-local
`ollama-call-N` identities and retain `provider_id = None`.

The standalone `model` subcommand resolves the normal bounded prompt sources,
builds one user message with no tools, and performs a unary or streaming local
request. It writes text to standard output and flushes every streaming delta.
It does not require host-execution acknowledgement because it neither starts a
provider process nor permits a non-loopback destination.

The direct adapter adds Serde and serde_json. The measured locked
`x86_64-unknown-linux-gnu` normal/build graph is 49 packages versus 42 before
the adapter. The release executable is 2,265,320 bytes versus 1,947,376 bytes.
No TLS package is selected. The machine-readable dependency policy is
`deny.toml`; advisory and allowed external dependency license checks pass.
The two workspace packages still lack Cargo license expressions, so an
unqualified cargo-deny license run reports those pre-existing metadata gaps.

## Optional Rig model transport

`RigModelTransport` supports the same typed Ollama and llama-server provider
selection behind the `rig-transport` Cargo feature. It uses Rig's Ollama and
llamafile/OpenAI-compatible completion models, but converts every request,
response, stream event, tool descriptor, and tool call at the
`agent_runtime` boundary. No Rig type appears in the public canonical
request or result, and no Rig tool implementation is registered.

Construction accepts only numeric HTTP loopback socket addresses with an
explicit nonzero port. It rejects hostnames, HTTPS, credentials, paths,
queries, and fragments. Its Reqwest client disables redirects and environment
proxies. A component-owned Rig `HttpClientExt` implementation rejects
serialized requests above 2 MiB and bounds both declared and incrementally
read unary and streaming responses before provider normalization. Multipart
requests are unsupported.

The synchronous trait implementation creates a current-thread Tokio runtime
per call and rejects invocation from an active Tokio runtime rather than
panicking in nested `block_on`. An outer timeout covers client construction,
connection, request, decode, and stream consumption. Cooperative cancellation
is polled every ten milliseconds. Synchronous stream callbacks are checked for
cancellation and deadline expiry immediately before and after invocation, but
cannot be preempted while caller code is executing. Rig errors are reduced to
bounded canonical status, request, response-limit, malformed-response,
timeout, cancellation, or framework-operation failures without retaining
provider response bodies.

Before Rig normalizes Ollama responses, the bounded client requires unary
responses to be terminal and rejects checked overflow of Ollama's input and
output usage counters in both JSON and incrementally framed NDJSON. This
prevents Rig 0.42.0 from accepting `done: false` unary responses or panicking
while adding provider-controlled `u64` counters.

Canonical `none` tool choice removes tool definitions before Rig sees the
request. Ollama `required` fails before network access because Rig 0.42.0
cannot preserve it. Assistant tool calls retain provider identities in a
turn-local map so later canonical tool results replay the provider-issued ID.
Output calls are object-validated, deduplicated, and returned only as untrusted
`ToolIntent`s. Response-scoped IDs and transport request IDs remain distinct.
Provider model, identity, and other-finish metadata is bounded before entering
the canonical turn.

Rig 0.42.0 emits payload-bearing tracing independently of its content telemetry
flag. Each framework call therefore runs under a no-op tracing dispatcher; a
TRACE sentinel test confirms prompt and response values do not reach the
caller's subscriber. This also suppresses callback-generated tracing while the
stream is being polled and remains a documented prototype limitation.

The optional graph contains 180 target-specific normal/build packages versus
52 for the direct default, a delta of 128. The unstripped release executable is
6,529,656 bytes versus 2,281,272 bytes, a delta of 4,248,384 bytes. No TLS
backend is selected. Rust 1.94 and current Rust 1.98 build the selected feature;
Rust 1.94 is the supported floor because it is the exact release toolchain Rig
tests, while Rig itself declares no MSRV.

## Profile configuration and setup

Standalone profile configuration is TOML limited to 64 KiB with integer
`schema_version = 1` and up to 128 profile tables. Names are limited to 128
ASCII bytes containing letters, digits, `.`, `_`, `-`, or `:`. Provider
identifiers use the same set without `:`, and commands are nonblank, parseable,
and at most 16 KiB. Loading rejects an absent required value, duplicate name,
wrong type, unsupported schema, malformed command, link-like file, non-regular
file, invalid UTF-8, and oversized input.
Default discovery accepts only an absolute nonempty `XDG_CONFIG_HOME`, then an
absolute nonempty `HOME` with `.config`; profile resolution never depends on
the working directory. It selects `agent-runtime/config.toml` when that path
exists or when neither store exists. If the canonical file is absent and the
corresponding former `supervised-agent/config.toml` exists, it selects that
legacy file. An existing canonical path always takes precedence, including
when later validation rejects it, so invalid canonical state cannot silently
fall through to legacy configuration.

Profile updates parse and validate existing content before mutation. They
replace only the matching table's provider and command or append a new table,
preserving other values, comments, profile order, and unknown table fields.
The generated document is reparsed and validated before a synchronized
same-directory temporary file is persisted with replacement or no-clobber
semantics. The parent directory is synchronized afterward.

The setup interaction supports llama-cli, llama-server, Ollama, Copilot,
Gemini, and custom executable defaults. HTTP endpoint probes invoke a fixed
`curl` command after `--` and are advisory; URLs must be bounded HTTP or HTTPS
values without whitespace or controls. Custom executables must be regular
non-link files with an executable mode bit. Optional verification warns that
the exact generated command receives full host authority and requires a
separate acknowledgement that defaults to refusal. Verification failure
defaults to refusing persistence but can be explicitly overridden.
llama-server defaults encode prompts through `{prompt_json}`. Ollama defaults
preserve the selected endpoint in an `OLLAMA_HOST` argument to `env`.

llama-cli, Gemini, and Copilot setup first runs the conventional executable
with `--version` under ten-second idle and wall timeouts and a 64 KiB output
bound. A failed conventional probe prints its diagnostic and requests an
executable regular non-link fallback, which must pass the same probe without
changing the profile provider. Cancellation returns immediately. Fallback
executables and GGUF files are canonicalized to stable absolute paths.
llama-cli defaults use a validated GGUF path, prompt,
single-turn subprocess I/O, hidden prompt echo, and a 4,096-token prediction
bound. Gemini defaults invoke `gemini` with headless text output, host-agent
approval, workspace trust bypass, and an optional model. Copilot defaults use
headless silent output, tool approval, disabled user questions, and an optional
model. Setup prints the generated template and explicitly warns when live model
qualification is skipped.

## Prompt and command handling

Prompt resolution accepts one caller-selected positional value, path, editor
request, or implicit standard input. Values must be nonblank UTF-8 and no
larger than 1 MiB. A path is inspected as a regular non-link file, opened with
Linux `O_NOFOLLOW` and `O_NONBLOCK`, checked again through the opened handle,
then read through the common bound. Interactive input offers an editor and
otherwise reads through end-of-file. Editor selection uses `VISUAL`, `EDITOR`,
then `vi`; the editor is spawned directly.

The command parser recognizes single- and double-quoted groups and a backslash
before the active quote or another backslash. It does not invoke a shell.
Rendering substitutes prompt and target-directory text, JSON-encodes
`{prompt_json}` as a complete string value, repeats an argument containing
`{context_files}` once per path, and removes a standalone empty context
placeholder with its immediately preceding option.

## Process supervision

Policy validation accepts idle durations from one through 3,600 seconds,
positive optional attempt durations through 24 hours, at most ten retries, and
output limits from one byte through 16 MiB. Every child
receives null standard input and starts in a new Linux process group with piped
stdout and stderr. Two reader
threads poll nonblocking descriptors and read 4 KiB chunks into a bounded
16-entry channel. A shared atomic budget caps combined bytes before enqueueing. The supervisor forwards output
after polling the destination for writability and treats output overflow,
blocked output, stream failure, spawn failure, and invalid policy as terminal.
Exceeding the per-attempt wall duration is terminal even while output
continues.

Either output stream resets the idle timer. A bounded 4 KiB stdout suffix is
checked for three identical byte cycles, four identical nonblank lines, or
three alternating line pairs. Idle and repetition events are retryable up to
the configured count. Nonzero exit is terminal. A temporary signal listener
turns SIGINT and SIGTERM into terminal cancellation. Every completion path
sends `SIGKILL` to the process group, tolerates an already absent group, waits
for the direct child, drains the bounded stream channel, and joins both readers.
Readers stop after a one-second post-termination drain deadline. Output pipes
still retained at that point produce a terminal error, preventing an escaped
descendant from hanging the supervisor while making no claim that host-mode
process groups can terminate descendants that create a new session.

A retry context records the next one-based attempt and prior idle or repetition
cause. Its generated notice says that an earlier attempt may have modified
files or external systems and directs the provider to reconcile state before
repeating non-idempotent work. The callback decides where that notice is used.

## Standalone command

`agent-run setup` runs profile collection and writes the result to an
explicit `--config` path or the Linux user default. `agent-run run`
accepts positional, file, editor, or redirected prompt input plus exactly one
explicit command template or named stored profile. It also accepts a profile
configuration override, repeated context paths, a working directory, idle
duration, loop detection, retry count, and output limit. Without
`--allow-host-execution` it refuses before acquiring prompt input or spawning
the configured command. On retries it appends the generated notice to the
prompt before rendering a fresh command.

`agent-run model` accepts the same prompt source alternatives plus
transport, provider, loopback endpoint, model, deadline, response limit, and
streaming selection. It sends no tools and prints only model text. The
transport defaults to `direct`; `--transport rig` exists only when compiled
with `rig-transport`.

## Observed limitations

Host execution inherits the invoking process's ambient filesystem, network,
credentials, environment, and executable authority. Context paths affect
arguments only. The package implements no filesystem snapshot, archive,
rollback, restricted identity, namespace, seccomp, Landlock, container, VM,
tool broker, provider network broker, or macOS/Windows backend. Its
acknowledgement and retry notice communicate risk but do not enforce isolation
or idempotency.

Both model transports are synchronous and intended for a dedicated execution
worker. They support local unencrypted HTTP only and have no hosted
provider, TLS, credential, proxy, redirect, HTTP/2, compression, multimodal,
parallel-tool, or model-discovery behavior. Live Ollama and llama-server model
matrices remain unevaluated when no local model/server is available. The Rig
prototype additionally suppresses caller tracing during framework polling and
exceeds the normal marginal package-count gate; it remains non-default pending
independent security and compliance review. A synchronous stream callback
cannot be forcibly interrupted while executing; deadline and cancellation are
reported when it returns, so callers must keep delivery bounded and
nonblocking.
