<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Observation basis

This record describes the reusable `agent-runtime` child crate from its Rust
source and tests. It does not inherit the root Kvist engine's project,
component, VCS, execution-approval, or sandbox policies.

The crate is a Rust 2024 library and `agent-run` binary, version `0.1.0`, with
minimum Rust `1.94`. It forbids unsafe code and currently rejects non-Linux
builds. The optional `rig-transport` feature adds Rig, Reqwest, Tokio, Futures,
Bytes, and Tracing integrations.

## Public entry points

The library exports:

- shell-free `render_command` and `split_raw_command`;
- bounded prompt resolution;
- durable model-profile load/upsert/setup functions;
- bounded host-process supervision;
- provider-neutral model request, message, tool, turn, usage, streaming, and
  cancellation types;
- the `ModelTransport` trait;
- direct local HTTP transport;
- optional Rig-backed transport;
- typed errors.

The `agent-run` binary exposes:

- `model` for local Ollama or llama-server completion/streaming;
- `run` for supervised host execution from a command or named profile;
- `setup` for interactive profile creation.

Host execution through `run` requires `--allow-host-execution`.

## Prompt and command handling

Prompts can come from a direct value, regular non-link file, stdin, or an
editor. Inputs must be nonblank UTF-8 and no larger than 1 MiB. Prompt files
are opened with no-follow/nonblocking flags and rechecked after opening.

Command templates are parsed without a shell. Quotes group arguments and
unterminated quotes are rejected. Supported substitutions are `{prompt}`,
`{prompt_json}`, `{target_directory}`, and repeated `{context_files}`.
Non-UTF-8 interpolated paths are rejected.

## Durable profile format

Profiles are version-one TOML with `[[profiles]]` entries containing `name`,
`provider`, and `command`. The canonical Linux path is under
`$XDG_CONFIG_HOME/agent-runtime` or `$HOME/.config/agent-runtime`, with a
legacy `supervised-agent` fallback when present.

Profile files are regular non-link UTF-8, bounded to 64 KiB and 128 entries.
Names/providers/commands have explicit syntax and size validation. Upsert uses
`toml_edit`, preserves unrelated formatting, validates the complete result,
and persists through a synchronized same-directory temporary file.

## Host-process supervision

`run_supervised` validates policy bounds, installs SIGINT/SIGTERM handling,
creates one process group per attempt, supplies null stdin, captures stdout and
stderr through nonblocking reader threads, forwards bounded output, and kills
and waits for the process group at attempt termination.

Policies bound idle time, optional total attempt time, retries, and combined
output. Only idle timeout and deterministic repetition are retried. Retry
contexts warn that prior attempts may have produced side effects. Nonzero
exit, output exhaustion, total timeout, cancellation, retained streams, and
I/O failures are terminal.

## Model transport boundary

Canonical requests contain a bounded model name, ordered messages, descriptive
tool schemas, and none/auto/required tool choice. Tool proposals are returned
as untrusted data with validated identities, names, and JSON-object arguments;
this crate never executes them.

`DirectModelTransport` accepts only explicit-port loopback plain-HTTP
endpoints. Provider selection fixes Ollama `/api/chat` or llama-server
`/v1/chat/completions`. Requests, messages, tools, HTTP headers, response
bodies, and stream records have hard bounds. Connection, write, read,
stream-callback, cancellation, and deadline paths are checked.

The direct HTTP parser accepts HTTP/1.0/1.1 content-length, chunked, or
close-delimited responses and rejects conflicting framing. Unary and streaming
decoders normalize text, complete tool intents, terminal reasons, identities,
and token usage. Streaming requires terminal records and rejects duplicate
tool calls, malformed argument fragments, records after termination, and
usage overflow.

With `rig-transport`, a restricted Reqwest client disables redirects and
proxies, accepts only numeric loopback HTTP endpoints, bounds request and
response data, suppresses provider payload tracing, and converts Rig results
to the same provider-neutral types. It refuses execution inside an existing
Tokio runtime.

## Setup behavior

The interactive setup can construct profiles for llama-cli, llama-server,
Ollama, Copilot CLI, Gemini CLI, or a custom executable. Executable and model
paths are checked as regular non-link files; custom commands require executable
permissions. Provider probes are bounded. Live verification requires explicit
host-authority acknowledgement and runs with fixed supervision bounds before
profile persistence.

## Security and resource boundaries

- This crate provides process supervision, not a sandbox, filesystem boundary,
  credential restriction, or general network isolation.
- Supervised commands are direct program/argument execution with null stdin.
- Direct model transport is loopback-only; no TLS, redirects, proxies, or
  arbitrary URL paths are supported.
- Model tool intents remain non-executable data.
- Prompt, profile, command, process, request, response, stream, metadata, and
  setup-discovery resources are bounded.
- Provider error bodies are omitted from typed status errors.

## Errors, recovery, and incomplete surfaces

The non-exhaustive error enum separates command, prompt, profile,
acknowledgement, supervision, process, cancellation, transport, provider
status, limit, malformed response, duplicate tool call, and contextual I/O
failures.

No durable conversation, response, tool-execution, retry journal, or token
store exists; only profile TOML is persistent. The standalone model command
does not bridge process signals into its cooperative model cancellation token.
Standalone `run` sets no total attempt timeout. Setup-generated commands can
exercise the user's broader host/network authority.

## Verification evidence observed

Tests cover command quoting/interpolation, prompt sources, profile persistence,
setup confirmation, supervision retries/timeouts/output limits/process-group
cleanup, direct Ollama/llama-server unary and streaming conversion, HTTP
framing and response bounds, cancellation, terminal requirements, tool-call
validation, and optional Rig capability/limit/runtime behavior.

The component-neutral implementation-record heading is imposed by the root
validator; no agent-runtime production behavior otherwise changed in this
refresh.
