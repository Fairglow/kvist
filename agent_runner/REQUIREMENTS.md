<!-- agent-runner-requirements-version: 1 -->

# Agent Runner — Requirements

## Purpose

`agent-runner` is a first-class, interactive agent shell that lets a person talk
to an AI coding agent while the agent performs real work on their machine,
similar in feel to `gemini` or GitHub `copilot`. It is a standalone tool
(`agent-runner` binary) that is intentionally independent of the `kvist` CLI but
shares its bounded runtime mechanisms and its sandbox enforcement boundary so it
can also be launched from Kvist.

The person enters a natural-language prompt, selects a model and a thinking
effort, and then follows along live as the agent reasons, decides what to do,
runs tools, and reports results. Every tool the agent runs is executed inside
the independently installed Bubblewrap sandbox, so the agent never asks the
person for per-action permission: the available tools, the working directory,
and the authority boundaries are declared once, up front, in configuration.

## Outcomes

Successful use produces:

- a single command, `agent-runner`, that opens a modern terminal UI in the
  current directory;
- the ability to choose an configured model and a thinking effort before or
  during the session, with the current choice always visible;
- a live transcript that shows the agent's reasoning, every tool call it makes,
  and the result of each call, without exposing raw transport noise;
- tools that are pre-approved in configuration and never prompt for permission:
  a generous set of common Linux tools plus a package manager and build tools
  for one or more language profiles;
- enforced authority boundaries — writes stay within the working directory, and
  the agent runs only in the sandbox, with process, output, and network limits;
- clear, actionable errors and structured logging instead of panics; and
- a test suite that covers configuration, tool policy, sandbox request
  construction, and the agent loop with an injected transport.

## Scope

### In scope

- Loading and validating a TOML configuration that declares models, a default
  model, a default thinking effort, the tool policy, the working directory, and
  the sandbox runner and backend paths.
- A model-agnostic agent loop that performs streaming turns and executes the
  tool intents the model proposes, feeding results back.
- A small, robust set of tools mapped to sandbox-executed commands: a shell
  tool, file read/write helpers, and a directory lister.
- Command policy enforcement (an allow-by-default shell with a safe denylist and
  language profiles that surface the relevant package and build tools).
- Construction and execution of a version-one Authoring-phase sandbox request
  against the installed `kvist-sandbox-runner`, reusing its closed protocol and
  Bubblewrap enforcement.
- A first-class terminal UI: scrollable transcript, model/thinking selectors,
  input line, status bar, and an help overlay, with responsive cancellation.
- Help output and informative error handling and logging aligned with Kvist.

### Out of scope (deliberately deferred)

- Networked package installation during authoring. The Authoring phase denies
  network by contract; dependency acquisition is a separate sandbox phase and
  is planned behind an explicit configuration switch. Package managers are
  present and usable for offline and local operations.
- Structured file-edit tools that parse and patch existing files token by token.
  The shell tool plus atomic write cover this today; a merge-aware editor is a
  later tool.
- Multi-model concurrent sessions, remote model brokering, and any daemon.
- Non-Linux targets and any cloud or credential requirement for core commands.

## Constraints

- The tool MUST NOT ask the person for permission for individual tool actions.
  Authority is established once in configuration.
- Every agent tool call MUST run inside the sandbox. It MUST NEVER fall back to
  unconstrained host execution when the sandbox is unavailable; it MUST fail
  closed with an actionable diagnostic.
- The working directory and everything beneath it is writable by default.
  Writes outside the working directory are rejected before the sandbox request
  is built and, in any case, cannot succeed because the sandbox grants write
  authority only there.
- Reading is intentionally lenient: within the sandbox the agent can read files
  in the working directory and the read-only system layout; additional read
  roots may be declared in configuration.
- Invalid state MUST be modeled out of existence with types. Recoverable
  failures (filesystem, parsing, subprocess, model transport) MUST use
  explicit errors, never unwrap/expect/panic.
- Shared mutable state, blocking I/O in async paths, and background processes
  MUST NOT be introduced without a documented boundary and targeted tests.
- Filesystem data, configuration, YAML/TOML, subprocess output, environment
  values, and paths are untrusted input and MUST be validated for schema and
  bounds.
- Behavior MUST be deterministic and safe: stable ordering, explicit
  configuration, reproducible output, and no hidden network or filesystem side
  effects.

## Acceptance criteria

- Given a configuration that declares one model and a working directory,
  `agent-runner` opens the UI, rejects non-interactive input with an actionable
  diagnostic, and does not start a session.
- Given a prompt, the agent performs at least one model turn, and when the model
  proposes an approved tool call, the tool runs in the sandbox and its result is
  shown and fed back. The loop ends when the model stops proposing tools.
- Given a model-intended command that matches the denylist (for example
  `rm -rf` on a system path), the agent runner rejects the call with a clear
  reason and does not build a sandbox request for it.
- Given a write whose target is outside the working directory, the agent runner
  rejects the call with a clear reason.
- Given a missing or misconfigured sandbox runner or backend, the tool reports
  the problem and exits without executing anything on the host.
- Given interactive input, the UI renders a transcript, the status bar reflects
  the model and effort, and Ctrl+C cancels a running turn cleanly.
- Every behavior above is covered by an automated test with an injected
  transport or a captured sandbox request.
