<!-- kvist-requirements-version: 1 -->

# Agent Runner — Requirements

## RUN-REQ-RELIABILITY

The October 2026 run remediation MUST resolve the selected model's effective
serving context, never silently assuming an 8192-token window. Explicit CLI
and per-model capacities MAY replace unavailable discovery; absent capacity
MUST fail with an actionable diagnostic. Automatic generation reserve SHOULD
allow reasoning and tool arguments (8192 tokens where the window permits).
Length-truncated generations MAY be regenerated with a larger bounded reserve
only before any effects, within the original prompt/attempt budgets.

Compaction MUST preserve all outstanding human goals and amendments until an
answer completes the task, including across continuation prompts. It MUST
retain complete tool groups and bounded structured file/page/result
references ahead of lossy content prefixes, compact before exhaustion, and
retain history according to available capacity rather than an arbitrary
six-group ceiling. Binary and
escaped results MUST have bounded serialized model-facing previews.
Summaries are lossy: aggregate fitting MAY discard older references and MUST
NOT be represented as preserving every reference or the complete history.

Repeated bounded native reads MUST remain usable for refreshing state.
Effectful/opaque repeats MUST remain bounded; successful distinct actions MUST
reset consecutive repeat stalls. Corrective notices MUST identify actual
pagination arguments, not assert that the model supplied an omitted offset.
Search MUST support bounded source filtering and regular-file scopes, report
excluded/oversized coverage explicitly, and distinguish non-directory paths
from forbidden symbolic links without weakening mutation protection.

An installed Rust toolchain MUST be usable through validated read-only
host-selected resources, without user-home/configuration/credential mounts,
toolchain installation, network access, or an engine dependency. Offline
vendored resolution MUST use sandbox-native paths and bounded private scratch.
The normal sandbox Cargo entry point MUST force `--offline --locked`, including
when callers omit those flags. Builds MUST fail actionably for a missing or
outdated `Cargo.lock`, without creating or changing it. Lockfiles and vendored
dependencies MUST be provisioned separately before locked builds.
Missing requested resources MUST fail clearly; auto discovery MUST NOT
advertise unusable tooling.

The full documented 16384-byte shell command bound MUST be executable: every
legal command MUST produce a sandbox request whose argv entries respect the
protocol's 4096-byte scalar bound, so multi-kilobyte commands (heredocs, inline
scripts, long invocations) MUST NOT fail request construction. The command
content MUST remain a read-only, host-staged context input outside the writable
workspace, and the command denylist MUST apply to the full command text in both
rendering forms.

Sandbox preparation cost MUST stay comfortably within the documented 30-second
bounds on representative hardware: per-invocation validation MUST NOT re-read
or re-hash full toolchain binaries, and the startup vendor snapshot MUST copy
bounded files concurrently without weakening its determinism, bounds, or
per-file drift detection.

Operational logs MUST retain retry and compaction notices, resolved capacity
provenance, actual failure reasons, per-prompt counts and unknown usage rather
than misleading zero usage. Provider usage and heuristic estimates MUST remain
distinguishable. No recovery MAY replay unknown tool effects or widen authority.

This exact remediation bundle has an explicit advisory-review exception:
the human requested implementation of the investigated fixes and subsequently
approved locked builds and necessary, minimally scoped unsafe; current Kvist
acceptance does not enforce review receipts. This exception is not compliance
evidence or an acceptance receipt. Separate security and compliance reviews
remain required after implementation.

## RUN-REQ-MODEL-SELECTION

Model selection and activation MUST prefer a model the default provider already
has loaded over a statically configured one, when the user has not explicitly
selected a model, so an already-active model starts with no load/switch cost.
The active model MUST be discovered by a bounded, read-only, loopback-only
provider probe (Ollama `GET /api/ps` for the loaded model and serving context;
llama-server `GET /props` for the loaded model), reusing the existing discovery
bounds (5 s, 1 MiB). The probe MUST NOT send inference, modify the provider, or
widen endpoint authority, and a probe failure (provider down, malformed, or no
model loaded) MUST NOT fail startup and MUST fall through to the next rule.

The selection precedence MUST be:

- An explicit selection wins and is never overridden by provider state: the
  `--model` flag (which `kvist prompt` passes for the role's profile) and the
  in-TUI Tab model selector.
- Otherwise, the default provider (`default_provider`) is the provider the
  session resolves. A loaded model on a default-provider endpoint whose
  provider-facing name matches a configured entry's `model` field (matched by
  provider model name, not the user-facing `id`) MUST be selected, and the
  session MUST announce that it uses the already-active model with no switch.
- Otherwise, if exactly one model is configured for the default provider, it
  MUST be auto-selected.
- Otherwise, the default provider's default model (the `[[models]]` entry
  marked `is_default`) MUST be used as a fall-back, and the session MUST
  announce that the default model is loaded. A model load/switch is not a
  timeout error: the session MUST wait for the (re)loaded model to become
  available, and a prompt submitted while it loads is held and dispatched once
  the model is ready (replayed on a failed load, never lost).
- Otherwise (no active match, several default-provider models, and no single
  `is_default` model), the session MUST not silently guess: the interactive TUI
  MUST start with no model selected and defer loading until the user selects and
  submits, and headless execution MUST fail fast before provider inference with
  an actionable diagnostic that lists the configured default-provider ids, names
  any active provider model found, and suggests `--model` and marking one entry
  `is_default`.

`default_provider` is a required configuration field naming the provider the
session falls back to; it MUST name at least one configured model. At most one
`[[models]]` entry per provider MAY be marked `is_default`. The per-provider
default model MUST be used only when the default provider reports no active
model, and MUST NOT override an active model, an explicit selection, or a single
default-provider model. When the provider
has an active model not present in `[[models]]`, the tool SHOULD offer to
configure it by printing a ready-to-paste `[[models]]` entry (provider,
base_url, provider model name, and sane defaults), derived as with
`--import-kvist`, and MUST NOT modify the configuration file automatically.

## RUN-REQ-REVIEW

When enabled, a completed headless implementation run MUST be followed by a
single automatic independent review run before the process returns. The review
runs only on a successful implementation disposition (`completed`); failed,
cancelled, exhausted, and budget-limited runs skip review with a visible
diagnostic note.

The review run MUST be a completely fresh session: a new conversation, a new
model context, a new operational journal, and no message, summary, transcript
or compaction history from the implementation run. Its inputs are limited to
the review system prompt, the composed review prompt (the original task prompt
and the implementation run's final answer), and the observable workspace
state. The implementation transcript MUST NOT be included.

The reviewer model MUST be a configured model id. The selection precedence is
an explicit `--review-model`, then the configured `[review] model`, then the
implementation model. A model different from the implementer SHOULD be
configured; a same-model review remains valid as fresh-context
re-verification and MUST be labeled as such in the report.

The review MUST assess the implementation against a fixed rubric covering
suitability for the task and correctness, robustness, idiomaticness,
efficiency, reliability, resilience, error handling, safety, security,
readability, maintainability, structure, and test quality. The reviewer MUST
verify claims by reading code and by running builds and tests inside the
sandbox, MUST report `cannot verify` for anything the workspace cannot
demonstrate, and MUST NOT invent test results or coverage.

Reviewer file mutation MUST be disabled by default. An explicit `apply_fixes`
enables edits; in that mode the reviewer MUST assess before fixing, re-run the
relevant builds and tests after fixing, and report the as-delivered and
after-fix states separately, listing every fix it made.

The review is advisory. Findings and severity MUST NOT change the
implementation run's disposition, MUST NOT block the returned answer, and are
not compliance evidence, engine authorization, canonical task evidence, or an
acceptance receipt. A review failure MUST NOT fail a completed implementation
run by default (`on_failure = "warn"`); an explicit `on_failure = "fail"`
makes the process exit unsuccessfully after the review failure is reported.

The review MUST emit a versioned machine-readable summary (reviewer model,
implementation model, mode, disposition, verdict, bounded findings with
severity, category, file and summary, fixes applied, tests-passing state, and
an honest narrative assessment). Plain (non-JSON) output MUST print the
implementation answer and then the assessment.

The review run uses the same sandbox request shape, grants, and network
denial as the implementation run, and MUST NOT widen any authority. Its
journal MUST satisfy the same private, no-clobber, outside-the-writable-
workspace rules and MUST identify the implementation model, the reviewer
model, and the mode in its metadata. Review wall-clock, token, and turn
budgets are resolved per model and MUST be bounded and validated.

The implementation answer embedded in the review prompt is untrusted model
output and MUST be treated as data, never as instructions. The review adds no
persistent state beyond its journal: a rerun reviews the current workspace
state and is never resumed or cached.

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
- a model that the provider already has loaded is preferred over a statically
  configured one when the user has not explicitly selected a model, so an
  already-active model starts with no load/switch cost;
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

- Loading and validating a TOML configuration that declares models, a
  last-resort default model, a default thinking effort, the tool policy, the
  working directory, and the sandbox runner and backend paths.
- Model selection and activation: resolving the session's model by preferring an
  explicit selection, then the default provider's already-loaded model
  (discovered by a bounded, read-only, loopback-only probe), then a single
  default-provider model, then the default provider's default model as a
  fall-back, and deferring to the user (TUI) or failing fast (headless) only
  when none of those resolve; the per-provider default model never overrides an
  active model, an explicit selection, or a single default-provider model. An
  active provider model that is not configured MAY be offered as a
  ready-to-paste `[[models]]` entry, never written to the configuration file.
- A model-agnostic agent loop that performs streaming turns and executes the
  tool intents the model proposes, feeding results back.
- A small, robust set of sandbox-executed tools: shell, bounded/paginated file
  reads, whole-file writes, exact preimage edits, listing, literal search and
  file discovery.
- Command policy enforcement (an allow-by-default shell with a safe denylist and
  language profiles that surface the relevant package and build tools).
- Language tool-chain detection, configuration, and gating: advertise profiles
  only against validated read-only resources that reach the sandbox, let each
  configurable profile be `on`/`auto`/`off`, detect the project language
  advisingly, and fail at startup when a requested or `on` profile is missing.
- Construction and execution of a version-one Authoring-phase sandbox request
  against the installed `kvist-sandbox-runner`, reusing its closed protocol and
  Bubblewrap enforcement.
- A first-class terminal UI: scrollable transcript, model/thinking selectors,
  input line, status bar, and an help overlay, with responsive cancellation.
- Help output and informative error handling and logging aligned with Kvist.
- An optional independent review phase for headless execution: after a
  successful run, a fresh-context review session (configurable second model)
  assesses the implementation against a fixed quality and security rubric and
  returns an advisory machine-readable assessment, with an explicit opt-in
  mode in which the reviewer applies fixes and re-verifies.

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
  reaches the sandbox's read-only System or prepared Rust resources, never
  merely against the host `PATH`;
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
- Given interactive input while the model is still starting, the UI is usable
  and a submitted prompt is dispatched when the model is ready; a model/effort
  change re-starts the model with the per-turn deadline applied to the switch.
- Given an ESC-menu selection, the menu closes: a history item opens the
  replay overlay, and "New session" starts a fresh session; Ctrl+N starts a new
  session from anywhere.
- Given a long prompt, the terminal UI accepts it without the previous
  16,384-character bound, and the session record is named by date, time, and
  the first words of the first prompt.
- Given no explicit model selection and a provider that already has a configured
  model loaded, `agent-runner` selects that model without a load/switch and
  announces it; the probe is read-only and a down provider falls through rather
  than failing startup.
- Given no explicit selection, no active match, and exactly one default-provider
  model, `agent-runner` auto-selects it; given several default-provider models,
  no active match, and no `is_default` model, the TUI starts with no model
  selected and headless fails fast with an actionable diagnostic. When the
  default provider has no active model but a `is_default` model, that model is
  used as the fall-back.
- An explicit `--model` or Tab selection is never overridden by an active
  provider model, and the per-provider default model never overrides an active
  model, an explicit selection, or a single default-provider model.
- An active provider model that is not configured produces an offered
  ready-to-paste `[[models]]` entry and never a write to the configuration file.
- Given a completed headless run and an enabled review, `agent-runner` starts
  a fresh-context review session with the resolved reviewer model, emits a
  versioned review summary with an advisory assessment, and prints the
  assessment after the implementation answer; the review sees no
  implementation transcript.
- Given an enabled review whose implementation run failed, was cancelled, was
  exhausted, or was budget-limited, the review is skipped with a visible
  diagnostic and the implementation disposition and exit status stand.
- Given a review that fails under the default `on_failure = "warn"`, the
  completed implementation run still succeeds and its answer is returned;
  under `on_failure = "fail"` the process exits unsuccessfully after the
  review failure is reported.
- Given `apply_fixes`, the reviewer's assessment distinguishes as-delivered
  findings from after-fix state, lists the fixes applied, and reports the
  result of the re-run builds and tests.
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
repeated opaque/effectful action arguments MUST receive correction and
eventually stop without silently
widening authority. Cancelled multi-call turns MUST retain valid paired
results for subsequent prompts. Injected turn limits MUST be in 1..=500.
Default prompt budgets MUST be safety bounds (24-hour wall, 100,000,000
estimated tokens), so a prompt runs uninterrupted to a result, a detected
hang/loop, or a real failure. The provider accepting a request — including a
model load/switch — and the first-token wait MUST be bounded by the turn's
own deadline, not a short fixed probe, so a slow switch completes within the
turn. The terminal UI MUST appear without waiting for the model, and a
failed startup MUST NOT quit the app.

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
