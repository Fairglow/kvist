<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Scope and build

`agent-runtime` is a Linux-only Rust 2024 crate (`rust-version = 1.94`) with a
library and the `agent-run` binary. Unsafe Rust is forbidden. The optional
`rig-transport` feature adds the Rig/reqwest/tokio-backed transport; without
that feature, only the direct transport is selectable.

The public library exposes prompt acquisition, shell-free command rendering,
profile storage, setup, supervised execution, a canonical local-model request
model, and direct model transport. Its errors are a non-exhaustive typed
`Error` enum covering validation, profile, I/O, process, cancellation,
transport, provider-status, response-limit, malformed-response, and duplicate
tool-call failures.

## Command rendering and prompt input

Commands are parsed into a program and argument vector, without a shell or
interpolation. Single and double quotes group arguments; a backslash only
escapes the active quote or another backslash inside quotes. Unterminated
quotes and an absent program fail.

`{prompt}`, `{prompt_json}`, `{target_directory}`, `{context_files}`, and
`{reasoning_effort}` are substituted in arguments, never in the executable.
`{prompt_json}` is a complete JSON string with JSON escapes. Each context path
expands a context-bearing argument once. If no context paths are supplied, an
otherwise standalone context placeholder and immediately preceding
dash-prefixed option are removed. The same removal rule applies to a
standalone reasoning-effort placeholder when no effort was requested. A
requested effort requires a `{reasoning_effort}` placeholder.

Prompts may come from a value, regular non-link file, standard input, or an
editor. File opening uses `O_NOFOLLOW | O_NONBLOCK`; prompt input must be
nonblank UTF-8 and no more than 1 MiB. With no supplied source, non-terminal
stdin is read; terminal stdin offers the editor, then can accept EOF-terminated
input. Editor selection is `VISUAL`, then `EDITOR`, then `vi`; it uses a
temporary prompt file and requires a successful editor exit.

## Profiles and setup

A profile contains a case-sensitive name, provider identifier, and command
template. Names are 1--128 ASCII letters, digits, `.`, `_`, `-`, or `:`;
providers are nonblank 1--128-byte ASCII alphanumeric/`. _ -` identifiers;
commands are nonblank after trimming, at most 16 KiB, and must parse as a
command. TOML storage requires `schema_version = 1`, permits at most 128
profiles, rejects duplicate names and non-regular/symlink configuration files,
and limits configuration content to 64 KiB of UTF-8. Updates retain unrelated
TOML formatting/comments and existing profiles, validate before persistence,
write and sync a temporary file in the destination directory, replace or
create without clobbering as appropriate, then sync the directory.

The default configuration is an absolute `XDG_CONFIG_HOME/agent-runtime/config.toml`,
or `HOME/.config/agent-runtime/config.toml`. If the canonical path is absent
and `supervised-agent/config.toml` exists, the legacy path is selected.
Nonabsolute or empty environment paths are ignored.

The setup wizard communicates prompts, probe warnings, qualification status,
and save status only through its caller-provided `Write` object. It supports
llama-cli, llama-server, Ollama, Copilot, Gemini, and a custom executable.
Setup validates regular non-link executable/model paths, expands a leading
`~/`, probes conventional CLI executables with `--version`, and offers a
validated direct executable when that probe fails. A cancellation during that
probe is returned directly and does not prompt for fallback.

Llama-server URL probes accept bounded (2,048-character) HTTP/HTTPS URLs
without whitespace/control characters, probe `/health`, and attempt a bounded
64 KiB `/v1/models` discovery. Discovery accepts at most 128 distinct,
1--256-byte printable ASCII IDs without braces; advertised numeric IDs can be
selected, while an explicit numeric selection must be in range. Ollama's
default template materializes the chosen base URL. Generated Copilot and
Gemini templates use noninteractive flags; the Copilot template includes the
reasoning-effort placeholder. The llama-cli template uses
`--single-turn --simple-io --no-display-prompt --predict 4096`.

Every generated profile is live-qualified with the fixed prompt
`Reply with exactly: OK`, host execution acknowledged, a 30-second idle
timeout, 300-second attempt timeout, no retry, loop detection, and a 64 KiB
combined-output limit. Qualification runs with `run_supervised_capture`:
provider stdout and stderr are captured rather than forwarded. Thus provider
output is not mixed into setup output; setup reports its own status through the
provided writer. A qualification failure prevents saving by default and is
reported as a profile-setup error. `SetupOptions { force: true }` reports the
failure through that writer and saves anyway. A cancellation is returned
unchanged even when force is enabled, so no profile is saved from that path.
Qualification also refuses before spawning when host execution was not
acknowledged.

## Process supervision

`run_supervised` forwards child stdout/stderr; `run_supervised_capture` retains
them separately and returns only output from the successful attempt. Both run
the program directly with null stdin, piped stdout/stderr, an optional working
directory, and a separate Linux process group. Policies require an idle
timeout of 1 second through 1 hour, an optional attempt timeout through 24
hours, at most 10 retries, and a combined output limit of 1 through 16 MiB.

Reader threads poll nonblocking output, apply the shared byte limit, and drain
for up to one second after termination. Output forwarding waits at most one
second for a writable destination. SIGINT and SIGTERM request cancellation;
the process group is killed and awaited. Attempts are also killed on exit,
idle/repetition retry, limit failure, and other terminal paths. Escaped
descendants that retain pipes cause `OutputStreamsRetained` rather than an
indefinite wait.

Only idle timeout and detected stdout repetition retry. Repetition is either
three identical adjacent byte substrings of 10--512 bytes in the last 4 KiB of
lossily decoded stdout, four repeated trimmed lines, or three repetitions of a
two-line suffix. A retry receives a one-based context and a notice that prior
side effects may have occurred. A nonzero exit, output limit, attempt timeout,
I/O failure, and cancellation are terminal. Retry messages are written to
process stderr.

## Canonical model types and direct transport

Canonical requests contain an ASCII model name, ordered system/user/assistant
and tool-result messages, tool definitions, tool choice, and optional effort.
Validation requires 1--1,024 messages, at most 128 tools, at most 8 MiB total
message text, unique valid tool names, valid tool identities/arguments, and
object tool schemas. Model names are nonblank ASCII up to 256 bytes; tool names
are 1--128 ASCII alphanumeric/`_`/`-`/`.`; identities are nonblank,
non-control, and at most 256 bytes. Tool choice other than `None` requires
tools.

`DirectModelTransport` connects by TCP only to `http://` numeric loopback
addresses with an explicit nonzero port. It rejects hostnames, non-loopback
IPs, HTTPS, credentials, query/fragment, and caller-selected paths; IPv6 must
be bracketed. The same numeric-loopback-only/explicit-port restriction is
implemented by the Rig endpoint validator. Direct requests go to
`/api/chat` for Ollama and `/v1/chat/completions` for llama-server. Deadlines
are greater than zero and at most 24 hours; response limits are 1 through
16 MiB. Requests are limited to 2 MiB, headers to 64 KiB, and stream records
to 1 MiB.

The direct HTTP implementation checks cancellation and deadline during
connect, reads, writes, event delivery, and after callbacks return. It accepts
HTTP/1.0 or 1.1 success responses with content-length, chunked, or
close-delimited framing, rejects invalid/conflicting framing, and never
includes provider error bodies in status errors. Non-success responses become
typed status errors. Oversized declared or received bodies fail before
decoding; a close-delimited oversized streamed body is rejected before it
emits events.

Ollama uses native messages, optional `tools`, and no tool-choice field;
required tool choice is rejected. Llama-server uses OpenAI-compatible
messages, tools, and `none`/`auto`/`required` tool choice. Assistant tool
arguments are object values for Ollama and JSON text for llama-server. Tool
results use the provider-specific fields. `none`, `minimal`, `low`, `medium`,
`high`, `xhigh`, and `max` map to llama-server `reasoning_effort`; on Ollama,
`none` maps to `think: false` and the other six map to a `think` string.
Tests exercise all seven mappings on both direct providers.

Unary and streamed responses normalize text, separate reasoning, tool intents,
finish reason, provider/model identifiers, response identifiers where present,
and usage. Plain `agent-run model` output writes provider text exactly: it
does not add a newline or other framing. Streaming emits text and reasoning
deltas in received order and emits complete tool intents after assembly;
terminal records are required. The model CLI can direct reasoning to stderr
with `--show-reasoning`; it conflicts with JSON output. JSON output writes one
serialized turn followed by a newline.

Tool calls must have unique identities, valid names, and JSON-object
arguments. OpenAI streaming assembles indexed argument fragments; Ollama
creates deterministic `ollama-call-N` identities when no provider identity is
present. Missing/invalid terminal data, metadata, JSON, usage, tool calls, and
finish values are malformed-response failures. `stop` with tool calls
normalizes to `ToolCalls`, while explicit `length` is retained. Provider
reasoning fields remain separate from answer text.

## Rig transport

With `rig-transport`, `RigModelTransport` converts canonical messages and
tools through Rig but never registers executable tools. It uses a
redirect-disabled, no-proxy reqwest client, bounds serialized requests at
2 MiB and responses at the configured 1--16 MiB limit, and rejects multipart
requests. It refuses use inside an existing Tokio runtime. A fresh
single-thread runtime applies its deadline and polls cancellation every
10 ms; tracing is suppressed for the Rig call so request and response payloads
do not reach a caller-installed tracing subscriber.

Rig validates the canonical request and provider capabilities before creating
the runtime or performing provider I/O. It rejects every requested reasoning
effort rather than silently dropping it, and rejects Ollama required tool
choice. Rig does not expose provider reasoning: received reasoning/non-text
content is a malformed-response error, and the model CLI rejects
`--transport rig --show-reasoning` before transport construction. Rig preserves
complete text/tool intents, validates metadata and tool values, reuses a prior
provider tool-call identity for its related tool result, requires terminal
Ollama unary responses, and detects Ollama usage overflow.

## Binary behavior

`agent-run model` accepts exactly one prompt source, a provider, endpoint,
model, optional direct/Rig transport, stream mode, response limit, timeout,
effort, reasoning display, and JSON mode. `agent-run run` requires explicit
`--allow-host-execution`, uses either a raw command or stored profile, and
offers context paths, working directory, timeout/retry/loop/output controls,
effort, and JSON mode. Its JSON mode uses captured stdout as `{"content": ...}`
and suppresses captured stderr; invalid UTF-8 stdout is decoded with
`String::from_utf8_lossy`, so each invalid sequence is represented with
U+FFFD. `agent-run setup` delegates its standard input/output locks and setup
options to the wizard. The binary prints top-level errors as `error: ...` on
stderr and returns failure.

## Observed test coverage and limits

Runtime tests cover command rendering; profile persistence; setup selection,
qualification, force, cancellation, and input validation; supervisor retries,
limits, descendants, and cancellation; direct HTTP unary/streaming framing,
tool conversion, limits, cancellation, and deadlines; model CLI exact output;
and Rig conversion, bounds, privacy, capability rejection, cancellation, and
runtime nesting. Tests use local listeners and process fixtures. They do not
demonstrate behavior against real provider installations, non-Linux targets,
or arbitrary third-party CLI implementations.
