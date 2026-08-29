<!-- kvist-specification-version: 1 -->
# Supervised Agent Component Specification

<details open>
<summary>Layer 1: Executive summary and public contract</summary>

## Purpose

Provide a reusable Linux-first Rust library and standalone CLI for acquiring a
prompt, rendering a provider command without a shell, supervising its process,
and retrying bounded nonfatal failures with explicit prior-attempt context.
The component is independent of Kvist task queues, roles, component discovery,
and compliance workflow.

## Public contract

The `supervised_agent` library exposes bounded prompt acquisition, shell-free
command-template rendering, typed supervision policy and attempt context, and
supervised host execution. A caller supplies the prompt, command template,
context paths, working directory, and retry policy. Each attempt is built from
fresh typed input; retries receive a deterministic notice describing the prior
failure and warning that prior side effects may remain.

The library also owns reusable named provider profiles, their bounded
formatting-preserving TOML store, provider-specific setup defaults, profile
validation, optional command verification, and terminal-neutral setup
operations over caller-provided input/output streams. A profile contains a
case-sensitive name, provider kind, and command template. It does not contain
Kvist roles, component paths, task state, or compliance policy.

`supervised-agent setup` interactively creates or updates a profile in the
Linux user configuration, defaulting to
`$XDG_CONFIG_HOME/supervised-agent/config.toml` or
`$HOME/.config/supervised-agent/config.toml`. Empty or relative base-directory
environment values are ignored, and resolution fails if neither base is an
absolute path. `supervised-agent run` accepts
prompt text, `--file`, `--editor`, or redirected standard input; exactly one of
a command template or stored profile; optional context paths and working
directory; idle/loop/retry controls; and the explicit
`--allow-host-execution` acknowledgement. It streams provider output and exits
nonzero on invalid input, unknown profiles, command failure, exhausted
supervision, or refusal.

The component supports Linux only. Other targets fail explicitly rather than
silently providing a different process or filesystem contract.

</details>

<details>
<summary>Layer 2: Architectural guarantees</summary>

## Constraints and invariants

- Rust 1.85 and edition 2024 are supported; unsafe Rust is forbidden.
- Commands are parsed and spawned directly without a shell. Quotes group
  arguments, but expansion, redirection, pipelines, and shell operators have
  no special meaning.
- Prompt input must be nonblank UTF-8 no larger than 1 MiB. Prompt files are
  regular non-link files opened without following a final symbolic link.
- Profile configuration is UTF-8 TOML no larger than 64 KiB. Existing
  configuration must be a regular non-link file with `schema_version = 1`.
  It contains at most 128 uniquely named profile tables. Profile names are
  nonblank ASCII identifiers of at most 128 bytes using letters, digits,
  `.`, `_`, `-`, and `:`; commands are nonblank and at most 16 KiB.
- Profile updates preserve unrelated TOML values, comments, ordering, and
  profiles; replace a profile with the same name or append it; validate the
  result; and use same-directory synchronized atomic persistence.
- The idle timeout is positive and at most 3,600 seconds. Automatic retries are
  bounded to at most 10. Combined output is positive and bounded to at most
  16 MiB, stream transport uses bounded memory, and loop detection retains at
  most 4 KiB of UTF-8 output.
- The supervisor retries only idle timeouts and deterministic repeated-output
  loops. A nonzero exit, spawn failure, stream failure, invalid contract, or
  cancellation is terminal.
- Every retry rebuilds the provider command and may append the supplied retry
  notice to its prompt. The notice is advisory: it does not prove that an agent
  inspected prior changes, make operations idempotent, or restore state.
- On supervised termination, the Linux process group is terminated and waited
  for before another attempt starts.
- Output readers use nonblocking polling. If output pipes remain open for one
  second after process-group termination, readers stop and the attempt fails
  terminally rather than hanging; this detects but does not terminate a
  descendant that escaped the process group.
- SIGINT and SIGTERM request cancellation through the supervisor; cancellation
  terminates and reaps the process group before returning a terminal result.
- Host execution requires an explicit acknowledgement at the standalone CLI
  boundary. The library names host execution directly and does not describe it
  as contained, restricted, or sandboxed.
- Host mode inherits the caller's filesystem, credentials, network, and
  executable authority. Declared context files control command arguments only;
  they are not an access-control list.
- Provider endpoint probes are advisory. Profile verification runs the exact
  generated command only after a distinct full-host-authority acknowledgement.
  Failed verification defaults to refusing profile persistence, with an
  explicit save-without-verification choice. Probe URLs are HTTP or HTTPS,
  contain no whitespace or control characters, and are limited to 2,048 bytes.
- Generated llama-server commands use `{prompt_json}` for request bodies.
  Generated Ollama commands materialize the selected endpoint through
  `OLLAMA_HOST` rather than depending on ambient endpoint configuration.
- Kvist-specific configuration, role selection, task transitions, approvals,
  sandbox runner validation, logs, and compliance evidence remain outside this
  component.

</details>

<details>
<summary>Layer 3: Detailed strategy and algorithms</summary>

## Design and failure paths

### Supervision and retry algorithm

Validate policy before spawning. For attempt one, build a command with no prior
failure. Capture stdout and stderr concurrently, forwarding bytes to the
caller's terminal. Reset the idle timer whenever either stream produces bytes.
Maintain a bounded stdout suffix for cycle, repeated-line, and alternating-line
loop detection. Output is read in bounded chunks through a bounded channel and
forwarded only while the destination is writable. Exhausting the combined
output budget or blocking the output destination is terminal.

Successful exit terminates any descendants that retained the process group's
output descriptors and returns an execution report. Nonzero exit returns a
terminal error. Idle or loop detection terminates the process group, waits for
it, and either returns an exhausted-retry error or starts the next attempt. The
next attempt context identifies its one-based number, prior failure class, and
a stable warning that files or external systems may already have changed.
SIGINT and SIGTERM are converted to a cancellation flag checked by the monitor;
the same process-group cleanup runs before cancellation is returned.

### Command and prompt handling

The command renderer separates quoted arguments without invoking a shell and
substitutes `{prompt}`, `{prompt_json}`, `{context_files}`, and
`{target_directory}`. `{prompt_json}` emits the complete JSON string value,
including quotes and escaping. An empty standalone context placeholder removes
its immediately preceding option argument. Prompt acquisition selects exactly one explicit source, reads
redirected input automatically, and offers an editor only at an interactive
terminal. Editor selection is `VISUAL`, then `EDITOR`, then `vi`.

### Profile configuration and setup

Version 1 profile configuration has integer `schema_version = 1` and an array
of profile tables:

```toml
schema_version = 1

[[profiles]]
name = "local-coder"
provider = "ollama"
command = "ollama run qwen3-coder '{prompt}'"
```

The loader validates the complete document before returning profiles. The
updater parses and validates an existing document before mutation, updates only
the matching profile table or appends one, validates the edited document and
size, then atomically persists it. Invalid, oversized, link-like, duplicate, or
unsupported configuration remains unchanged.

The reusable setup interaction selects llama-cli, llama-server, Ollama,
Copilot, Gemini, or a custom executable; collects a profile name and editable
command template; and may perform an advisory endpoint probe. Optional model
verification displays the exact host-authority warning and defaults its
acknowledgement to refusal. The standalone command persists the resulting
profile. Kvist may call the same collection API or load an existing standalone
profile, but it materializes only the selected name and command into its own
role configuration so later Kvist execution approval remains bound to exact
command bytes.

### Deferred workspace recovery

A retry warning is viable only as cooperative context. It cannot prevent or
undo duplicated side effects. Copying or archiving files before execution can
aid recovery but must not overwrite concurrent user changes, follow links,
lose metadata, or invoke destructive source-control reset. The preferred
future design is a bounded immutable input snapshot plus a private writable
overlay. A successful attempt yields a reviewed patch or artifact set; a failed
attempt discards the overlay. Archive-and-restore is a fallback only after
conflict detection and explicit user approval.

### Deferred Linux isolation

`fakeroot` changes apparent ownership results for cooperating processes; it
does not remove filesystem, process, credential, or network authority and is
not a sandbox. A restricted dedicated user can reduce discretionary access,
but only when ownership, groups, inherited descriptors, credentials, runtime
files, and network access are also controlled.

Future strict Linux execution should place the provider or its tools behind a
separate backend interface. Candidate tiers are:

1. Snapshot workspace plus explicit host acknowledgement for recovery only.
2. Dedicated uid/gid, cleared environment, resource limits, and process-group
   lifecycle management.
3. Bubblewrap namespaces and read-only mounts, optionally reinforced by
   Landlock and seccomp.
4. OCI with gVisor or a microVM for hostile or multi-tenant workloads.

Tool permission should use a trusted typed broker, optionally exposed through
MCP, rather than trusting provider approval prompts. Hosted-provider access
should be mediated separately from sandboxed tool execution so credentials and
general network authority are not placed inside the agent workspace.

### Deferred platforms

Platform-independent policy and protocol types must not contain Linux syscalls
or path assumptions. Linux process control belongs behind an execution backend.
macOS support requires an independently tested Seatbelt or VM design. Windows
support requires an independently tested AppContainer/restricted-token and Job
Object design, or a VM backend. Unsupported platforms remain disabled until
their guarantees and native CI are approved.

</details>
