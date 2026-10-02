<!-- kvist-requirements-version: 1 -->

# Agent Runner — Requirements

## Purpose and scope

`agent-runner` is a first-class, interactive agent shell that lets a person talk
to an AI coding agent while the agent performs real work on their machine,
similar in feel to `gemini` or GitHub `copilot`. It is a standalone tool
(`agent-runner` binary) that is intentionally independent of the `kvist` CLI but
shares its bounded runtime mechanisms and its sandbox enforcement boundary so it
can also be launched from Kvist.

The person enters a natural-language prompt, selects a model and a thinking
effort, and then follows along live as the agent reasons, decides what to do,
runs tools, and reports results. By default every tool is executed inside
the independently installed Bubblewrap sandbox, so the agent never asks the
person for per-action permission: the available tools, the working directory,
and the authority boundaries are declared once, up front, in configuration.
The explicit interactive host opt-out is visibly unconfined and unavailable
headlessly.

## Stakeholders and concerns

- **The person at the terminal.** Wants a transparent, safe session: a live
  transcript of the agent's reasoning and tool calls, visible model and effort
  choices, clear and actionable errors, and clean cancellation. Their machine
  stays protected because tools, the working directory, and authority are
  declared once, up front, in configuration.
- **The AI coding agent.** Operates only within the tools, working directory, and
  authority boundaries that configuration declares, and never asks the person for
  per-action permission.
- **Kvist and future callers.** May launch `agent-runner` as a bounded subprocess
  and rely on the documented CLI, configuration schema, sandbox request contract,
  and deterministic, non-interactive diagnostics.

## Functional requirements

Successful use produces:

- a single command, `agent-runner`, that opens a modern terminal UI in the
  current directory;
- the ability to choose an configured model and a thinking effort before or
  during the session, with the current choice always visible;
- a live transcript that shows the agent's reasoning, every tool call it makes,
  and the result of each call, without exposing raw transport noise;
- output that fits the terminal width: the transcript, help, menu, session
  history, and replay wrap long lines at word boundaries inside their boxes so
  the borders stay intact, and text blocks keep their indentation, with
  continuation lines prefixed by the line's leading whitespace;
- tools that are pre-approved in configuration and never prompt for permission:
  a generous set of common Linux tools plus a package manager and build tools
  for one or more language profiles;
- a shell tool that advertises only the language tool-chains that genuinely reach
  the sandbox: each profile is configurable (`on`, `auto`, or `off`), `auto`
  advertises only when the interpreter is available inside the sandbox, and an
  explicitly requested or `on` profile that is unavailable fails startup rather
  than advertising a tool the sandbox cannot run;
- enforced default authority boundaries — persisted writes stay within the
  working directory, with sandbox process/output/network limits, and an honest
  warning for the explicit interactive unconfined opt-out;
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
- A small, robust set of sandbox-executed tools: shell, bounded/paginated file
  reads, whole-file writes, exact preimage edits, listing, literal search and
  file discovery.
- Command policy enforcement (an allow-by-default shell with a safe denylist and
  language profiles that surface the relevant package and build tools).
- Language tool-chain detection, configuration, and gating: advertise profiles
  only against what reaches the sandbox read-only `/usr` layout, let each
  configurable profile be `on`/`auto`/`off`, detect the project language
  advisingly, and fail at startup when a requested or `on` profile is missing.
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
- Fuzzy/merge-aware editing, automatic effect replay, remote credentials,
  arbitrary plugins, parallel effectful tools, and automatic VCS promotion.
- Multi-model concurrent sessions, remote model brokering, and any daemon.
- Non-Linux targets and any cloud or credential requirement for core commands.

## Quality requirements and constraints

- The tool MUST NOT ask the person for permission for individual tool actions.
  Authority is established once in configuration.
- Every agent tool call MUST run inside the sandbox unless the person explicitly
  selects the interactive host-execution opt-out. Headless mode forbids this
  opt-out. The tool MUST NEVER fall back to
  unconstrained host execution when the sandbox is unavailable; it MUST fail
  closed with an actionable diagnostic.
- The working directory and everything beneath it is writable by default.
  Writes outside the working directory are rejected before the sandbox request
  is built and, in any case, cannot succeed because the sandbox grants write
  authority only there.
- Reading is intentionally lenient: within the sandbox the agent can read files
  in the working directory and the read-only system layout. The executor may
  supply exact read-only context files; general configurable read roots remain
  unsupported.
- Invalid state MUST be modeled out of existence with types. Recoverable
  failures (filesystem, parsing, subprocess, model transport) MUST use
  explicit errors, never unwrap/expect/panic.
- A turn that fails on a temporal, recoverable model-transport error (a dropped
  connection, a provider timeout, or a transient server error) MUST be retried
  with backoff and recover when a fresh attempt can finish. Each retry is
  granted a larger time budget than the last — the per-turn deadline grows with
  the attempt number and is capped at the base deadline times the attempt
  budget — so a turn that merely ran past one deadline can complete once an
  attempt has room for the whole generation rather than timing out identically
  on every identical try. Cancellation is never retried, and a failure that
  exhausts the budget is reported, not hidden.
- Shared mutable state, blocking I/O in async paths, and background processes
  MUST NOT be introduced without a documented boundary and targeted tests.
- Filesystem data, configuration, YAML/TOML, subprocess output, environment
  values, and paths are untrusted input and MUST be validated for schema and
  bounds.
- Behavior MUST be deterministic and safe: stable ordering, explicit
  configuration, reproducible output, and no hidden network or filesystem side
  effects.
- The shell tool MUST advertise a language tool-chain only when its interpreter
  reaches the sandbox's read-only `/usr` layout, never against the host `PATH`;
  `rustup` stubs MUST NOT be advertised as buildable tool-chains. Each
  configurable profile is gated by its `on`/`auto`/`off` setting, an explicit
  forced profile, or the default; `auto` advertises only when available, `off`
  never advertises, and an explicit or `on` request for an unavailable profile
  MUST fail startup with an actionable diagnostic rather than advertise a tool
  the sandbox cannot run.

## Acceptance and traceability

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
- Given an explicit or `on` language profile whose interpreter does not reach the
  sandbox, `agent-runner` fails startup with a `ToolchainUnavailable` diagnostic
  instead of advertising a tool the sandbox cannot run; a profile set to `auto`
  is advertised only when its interpreter is available.
- Given a project whose root manifest names a language, `agent-runner` may log
  the detected language but does not advertise that profile unless its interpreter
  is available or the profile is explicitly enabled.
- Given interactive input, the UI renders a transcript, the status bar reflects
  the model and effort, and Ctrl+C cancels a running turn cleanly.
- Every behavior above is covered by an automated test with an injected
  transport or a captured sandbox request.
- A turn that fails a temporal model-transport error at least once is retried
  with backoff and completes when a later attempt finishes; each retry reports
  that it is in progress with its enlarged budget, and a failure that exhausts
  the retry budget is reported as a terminal failure without crashing.

## Security-first runtime hardening

This extension implements the P0/P1 recommendations in
`../docs/agent-runtime/upstream-agent-comparison.md`. The user's improvement
request authorizes these conservative changes; remote authority and automatic
resume remain deliberately deferred. Advisory intent-review enforcement is not
implemented and no acceptance receipt is claimed.

### RUN-REQ-AUTHORITY

This is an interactive workspace agent, not the engine's
  protected-task broker. Sandboxed effects remain network-denied; host execution
  remains an explicit interactive opt-out. Headless execution MUST reject host
  opt-out and disabled recording. It MUST NOT authorize tasks, write engine
  evidence, accept intent, or promote results.
  The UI and model instructions MUST identify the actual execution scope.
  Operational records MUST identify the scope, workspace, policy and limits
  without claiming engine authorization.
  Plain answers and human diagnostics MUST visibly escape terminal controls
  other than LF/tab; JSON and private transcripts retain the original text.
### RUN-REQ-CONTEXT

Before every provider request, account for the complete
  serialized canonical request, including system instructions and tool schemas,
  with an explicit output-token reserve enforced at the provider. Estimates are
  not tokenizer guarantees. Compact only complete tool-call/result groups;
  preserve active user goals and system instructions. An irreducibly oversized
  request MUST fail before provider I/O. Summaries are explicitly lossy,
  non-authoritative history. Combined model-facing tool output MUST be bounded.
### RUN-REQ-LIFECYCLE

Recording MUST be fallible. A required dispatch record
  MUST be synchronized before invoking an executor. Recording failure MUST stop
  further effects. Final answer, failed, cancelled, exhausted, and budget-limited
  runs MUST be distinguishable; a previous prompt's answer MUST NOT become a new
  prompt's result. Interrupted dispatches have unknown effects, never replayable
  effects. Journals are local operational records, not compliance certification.
  Worker teardown MUST join even with retained prompt senders or a full event
  queue. Collapsing reasoning MUST preserve all non-reasoning transcript rows.
  History reads MUST reject links/nonregular files and bound bytes before
  allocation, including growth after opening; unusable entries are logged.
  Built-in tool executors MUST bound combined captured bytes and intermediate
  buffering on every exit path. Stdin writes and post-exit drains MUST be
  cancellation/deadline-aware. Reader ownership and process-group cleanup MUST
  be explicit; retained output descriptors MUST fail instead of hanging or
  returning success.
### RUN-REQ-BUDGET

One prompt deadline bounds model requests, retries, waits,
  and cooperative tool execution. Retry waits MUST be cancellable. Identical
  repeated action arguments MUST receive correction and eventually stop without silently
  widening authority. Cancelled multi-call turns MUST retain valid paired
  results for subsequent prompts. Injected turn limits MUST be in 1..=50.
### RUN-REQ-TOOLS

Provide bounded/paginated text reads, directory listing,
  literal search and scoped file discovery, plus exact-single-occurrence edits
  bound to an expected SHA-256 preimage. Preserve unrelated bytes, CRLF and
  missing final newlines. Reject stale/ambiguous matches, malformed/unknown
  arguments, traversal, and symbolic-link mutation paths. File effects run in a
  small Rust helper through the same sandbox executor. Payload staging MUST be
  host-owned, private, outside the writable workspace, unrelated to provider
  call IDs, and cleaned on every return path.
### RUN-REQ-HEADLESS

Provide terminal-free execution over the same loop with
  versioned NDJSON events, ordered sequence IDs, diagnostics on stderr, an
  explicit final disposition and nonzero unsuccessful status. Required journal
  files MUST be private, no-clobber and outside the sandbox writable scope.
### RUN-REQ-PROVIDERS

Preserve llama-server and Ollama. Honor output bounds in
  both wire protocols. Test fragmented streaming tools, finish classification,
  transport/cancellation failures, context rejection, and retry boundaries
  deterministically. Keep live llama-server qualification explicit and opt-in.

Acceptance requires tests before production changes, targeted formatting and
lint/build gates, native isolated file-tool trials, opt-in llama-server trials,
a separate security audit, and independently derived/source-blind review
evidence. Unavailable isolation MUST be reported, never bypassed to pass a test.
