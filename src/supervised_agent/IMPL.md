<!-- kvist-implementation-record-version: 1 -->
# Root Component Implementation Record

## Package and public API

The `supervised-agent` Cargo package builds the `supervised_agent` library and
the `supervised-agent` binary. Compilation emits an error on targets other than
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
the working directory.

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

`supervised-agent setup` runs profile collection and writes the result to an
explicit `--config` path or the Linux user default. `supervised-agent run`
accepts positional, file, editor, or redirected prompt input plus exactly one
explicit command template or named stored profile. It also accepts a profile
configuration override, repeated context paths, a working directory, idle
duration, loop detection, retry count, and output limit. Without
`--allow-host-execution` it refuses before acquiring prompt input or spawning
the configured command. On retries it appends the generated notice to the
prompt before rendering a fresh command.

## Observed limitations

Host execution inherits the invoking process's ambient filesystem, network,
credentials, environment, and executable authority. Context paths affect
arguments only. The package implements no filesystem snapshot, archive,
rollback, restricted identity, namespace, seccomp, Landlock, container, VM,
tool broker, provider network broker, or macOS/Windows backend. Its
acknowledgement and retry notice communicate risk but do not enforce isolation
or idempotency.
