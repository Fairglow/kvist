<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Evidence scope

This record was derived afresh from `sav/src/**`, `sav/tests/**`, and
`sav/Cargo.toml`, with the subsequently supplied workspace test-run log used only
as execution evidence. No intent documents, previous implementation record, peer
implementation, review history, or Git information were used.

**The observer did not run tests, builds, or executable probes.** Test-source
references below describe assertions separately from the execution
evidence summarized below. Findings are source-based engineering observations,
not acceptance, compliance, or security certification; passing existing tests
does not establish that the identified gaps are covered.

The current bounded refresh directly re-inspected `src/main.rs`,
`src/runtime/session.rs`, `src/runtime/driver.rs`, the new
`tests/registry_cli_policy.rs` and `src/runtime/accounting_tests.rs`, and relevant
direct-validation, registry-transport, signal, supervision, and existing runtime
test sections. Other component descriptions retain the original direct-source
observation; this was not an exhaustive second audit. The inspected workspace
execution log predates these repairs and is not post-repair validation.

## Observed public contract

### Package, CLI, and authority

Sav is a Rust edition-2024 library and `sav-run` binary, version 0.3.0, with
Rust 1.95 declared in its package manifest. The crate rejects non-Linux builds.
Its exported interfaces include prompt acquisition, shell-free command rendering,
provider catalogs, direct local HTTP model transports, provider/profile setup,
process supervision, native conversation/runtime controls, diagnostic journals,
trajectory recording/replay, loop detection, and fixed Rust runtime script
generation (`src/lib.rs`, `src/main.rs`, `Cargo.toml`).

The CLI exposes `model`, `models`, `run`, `replay`, `setup`, and `registry`.
Process execution through `run` requires `--allow-host-execution`. ACP catalog
discovery requires a separate `--allow-host-discovery`. Neither acknowledgment is
a sandbox or a filesystem/network/credential restriction. Library supervision
accepts caller-supplied execution authority rather than requiring those CLI flags.
Native HTTP inference does not require the process-execution acknowledgment.

`model` supports direct Ollama/llama-server selection or a named native registry
profile, prompt/file/editor input, streaming, canonical JSON output, optional
reasoning presentation, reasoning effort, and provider-native output schemas.
`models` emits provider-order IDs or a version-one catalog. `registry` emits
configured resolved profiles without discovery or qualification; listing includes
disabled profiles, whereas selecting a disabled profile fails. `run --profile`
accepts external profiles and refuses native profiles. The profile-selected model
path applies the stricter of the profile deadline and CLI `--timeout`, together
with CLI `--max-response-bytes`. Explicit slot/first-token watchdog flags conflict
with `--profile`; direct-provider mode retains their 60/120-second defaults and
ordering checks. External profiles default to zero retries and reject an explicit
positive retry count before spawn. Explicit `--command` retains a default of three
retries and an optional caller-selected count (`src/main.rs:547-605`,
`src/main.rs:662-710`).

### Shared registry, setup, and pinned resolution

`registry::Backend` distinguishes native HTTP models from external agents.
Native fields include provider, base URL, model, optional context/reserve values,
deadline, retry settings, and cadence timeout. External fields include an explicit
provider enum, executable, literal argument vector, and optional model.
Capabilities are separate `advertised`, `tested`, and `policy_enabled` identifier
lists; their contents do not themselves grant tool authority
(`src/registry.rs:30-239`, `src/registry.rs:301-377`).

The shared TOML document requires integer `schema_version = 1` and named profiles.
Unknown document/profile/backend-struct fields or invalid types fail parsing.
The decoder rejects TOML floats and datetimes. Limits include 64 KiB per file,
128 profiles per file and merged registry, and 1–128-byte ASCII selector names.
Each capability list permits at most 32 unique identifiers.

Native validation supplies an empty endpoint's provider default
(`127.0.0.1:11434` for Ollama; `127.0.0.1:9931` for llama-server).
It checks deadline 1–600 seconds, attempts 1–32, cadence at most 600 seconds
(zero disables it), ordered retry delays capped at 3600 seconds, and optional
context/reserve bounds up to 1,048,576 with context greater than reserve when
both are supplied. This validates a transport without contacting it. Native model
text validation here is broader than the direct request validator.

External definitions bound executable text to 4096 bytes, arguments to 128
entries of at most 16,384 bytes, and optional model text to 1024 bytes.
Argument validation rejects NUL and a heuristic list of credential-like
substrings. This is not comprehensive secret detection.

`RegistryPaths` loads user, project, then explicit sources. A complete higher
precedence profile replaces a lower precedence profile by name; fields are not
merged. The project source is `<project>/.kvist/providers.toml`. The user source
is absolute `XDG_CONFIG_HOME/sav/registry.toml`, otherwise absolute
`HOME/.config/sav/registry.toml`. Missing optional sources are skipped; an explicit
missing source or any malformed present source fails closed. Lower precedence
sources are validated even when subsequently overridden
(`src/registry.rs:447-562`).

Resolution returns a cloned name, normalized profile, source pathname, and SHA-256
digest of the **entire original source text**. Subsequent filesystem changes do
not alter the loaded selection. This is an in-memory snapshot, not authentication
of its producer, executable-content pinning, or automatic revalidation at use.
`ResolvedProfile` fields are public and mutable by the embedding caller.

Upsert validates the existing document and replacement before serializing,
then uses the profile filesystem helpers to write, synchronize, atomically
persist, and synchronize the parent directory. First creation is no-clobber.
Configuration reads use bounded UTF-8 reads, leaf `O_NOFOLLOW`/`O_NONBLOCK`,
and opened-file regularity/size checks. Persistence rejects a symlink final
parent, but does not descriptor-walk every ancestor or serialize concurrent
read-modify-write operations (`src/profile.rs:229-345`,
`src/registry.rs:565-604`).

`run_registry_setup` offers llama-cli, llama-server, Ollama, Copilot, Gemini, or a
custom executable; Claude is representable in the external provider enum but is
not a wizard choice. Setup performs explicit discovery, permits a numbered or
custom model choice, validates direct executable/model paths where applicable,
and qualifies using `Reply with exactly: OK`. Native qualification disables tools
and reserves 32 output tokens; external qualification uses no retries, a
30-second idle limit, 300-second attempt limit, and 64-KiB output bound.
Qualification success means the transport/process call succeeded; it does not
verify that the returned answer equals `OK`, populate capability-test lists, or
authorize later work. `--force` permits saving after qualification failure, not
after cancellation (`src/setup.rs:24-270`).

Separate exported `ModelProfile`, `load_profiles`, `upsert_profile`, and
`run_setup_wizard*` APIs still operate a command-template TOML array-of-tables
store, with a default `sav/config.toml` path. The CLI's shared registry resolution
does not fall back to that store. These are distinct observed library interfaces,
not interchangeable shared-registry schemas.

### Native model/runtime interfaces

The model boundary provides typed messages, tool definitions/proposals, requests,
finish classifications, optional provider usage/identities, ordered text/reasoning
deltas, and cloneable cooperative cancellation. A model tool descriptor contains
no executor. `ModelTransport::stream_with_deadline` defaults to ignoring the
override; direct HTTP transport overrides it (`src/model.rs`).

`AgentSession::next_request` selects `ToolChoice::None` when no tools were supplied
and `ToolChoice::Auto` otherwise. Host instructions remain the leading system
message; reasoning effort, tool definitions, and the selected model identity are
carried into the request (`src/runtime/session.rs:301-325`).

`runtime::NativeRuntime` consumes an `AgentSession`, transport, injected
`ToolExecutor`, fallible `EventSink`, cancellation token, context manager, optional
`Recorder`, and optionally a private `ControlMailbox`. The executor owns effects
and policy. There is no implicit process fallback or registry reload inside the
loop. Recording is optional; recorder defaults for some observation hooks are
no-ops, so durability depends on the supplied implementation.

`RunSummary` separates final answer, turns, executed tools, cancellation,
turn exhaustion, budget exhaustion, and failure. Success requires an answer
without an interrupted/failed disposition. Streamed text is provisional;
only a complete validated normal-stop answer becomes a final answer.
Operational events distinguish provider, estimated, and unavailable token
accounting. Live input and prior provider total are separate counters, while live
output adds the current attempt's character-derived estimate to accepted prior
output. Provider totals are retained independently rather than reconstructed as
input plus output (`src/runtime/session.rs:85-120`,
`src/runtime/driver.rs:419-432`).

### Observation and control interfaces

`RunJournal` writes version-one JSONL records with run ID, sequence, and an event
object containing a bounded type identifier. Owner-controlled `run_start` and
`run_end` delimit the diagnostic lifecycle. It is not a canonical checkpoint,
tool grant, or executable replay format. `JournalFollower` reads the original
held descriptor and reports explicit terminal completion, not completion at EOF
(`src/observation/journal.rs`).

`ControlServer` exposes only schema-one, nonzero-ID `Cancel` and `Guide` requests.
The private socket pathname is an explicit process-control capability, separate
from a journal path. Replies correlate request IDs and distinguish queueing,
cancellation requested, and rejection; queueing is not model delivery.
`bind_runtime_controls` rejects guidance/cancellation when no run is active and
never turns guidance into a new idle task (`src/observation/mod.rs:15-39`).

### Representative existing helpers

Prompt acquisition accepts direct text, regular non-link UTF-8 files, stdin, or
an explicitly invoked editor, with a 1-MiB nonblank prompt bound. Editor selection
uses `VISUAL`, then `EDITOR`, then `vi` and executes argv without a shell
(`src/prompt.rs`).

Command-template rendering tokenizes quotes without shell evaluation and expands
prompt, JSON prompt, context paths, target directory, and explicit reasoning
effort placeholders. Structured external argv instead substitutes recognized
placeholders in one pass, expands standalone `{context_files}`, retains unknown
placeholders literally, and bounds the rendered result to 256 arguments/2 MiB.
The two rendering APIs have different substitution/default semantics
(`src/command.rs`, `src/registry.rs:243-299`).

Catalogs preserve first occurrence/order of duplicate IDs, cap unique entries at
128, and discard a current-model ID absent from the catalog. HTTP discovery uses
Ollama `/api/tags` or llama-server `/v1/models`. Copilot/Gemini ACP discovery
starts one acknowledged host process, exchanges correlated `initialize` and
`session/new` requests with no MCP servers, and sends no inference prompt.
Unsupported provider-to-client requests, wrong IDs, malformed records, excess
output, and timeout fail discovery (`src/catalog.rs`).

The separate trajectory API appends/replays `TrajectoryEvent` JSONL and the
CLI `replay` summarizes those events; it does not execute recorded tools.
Unlike `RunJournal`, that recorder reopens a pathname, follows normal filesystem
resolution, and has no comparable record/total size, private-file, sequence, or
durability enforcement (`src/trajectory.rs`).

GBNF helpers generate grammars for primitive/object/array/enum schemas and tool
selection, not a full JSON Schema validator. The mixed optional-property branch
uses a repeatable property alternative rather than enforcing all required names
or uniqueness. No explicit depth/node/output cap is implemented in this helper;
it is distinct from direct transport's bounded output-schema validator
(`src/gbnf.rs`).

`offline_rust` returns fixed initialization/wrapper script text for a closed
`RustTool` set. Wrappers select an existing toolchain and inject offline,
locked, vendored Cargo resolution, including the delegated Clippy companion;
the module does not itself install, run, or sandbox tools (`src/offline_rust.rs`).

## Observed requirements and constraints

These are implementation-enforced conditions and caller obligations, not a
comparison with external intent.

- Native HTTP accepts only ASCII `http://` numeric-loopback endpoints with an
  explicit nonzero port, no user information/query/fragment, and no provider path
  other than an optional trailing slash. There is no hostname resolution,
  authentication configuration, proxy, redirect, remote HTTPS, or cloud SDK
  integration in this transport.
- Direct model requests bound messages to 1024, tools to 128, aggregate message
  text and encoded request to 8 MiB, tool descriptions to 16 KiB, and response
  bytes to a configured maximum at most 16 MiB. Output tokens, when present,
  must be 1–1,048,576. Model selectors must be nonblank ASCII and at most 256
  bytes. Output schemas require object nodes, a supported keyword subset,
  at most 256 KiB, depth 32, and 4096 nodes; callable tool choice cannot be
  combined with an output schema (`src/direct_transport.rs`).
- Requests/streaming have overall deadlines, slot/first-token watchdogs, and
  optional semantic-progress cadence checks. Heartbeats and empty metadata do
  not count as token progress. Cancellation and socket deadlines are polled;
  callbacks and custom transports remain trusted, synchronous collaborators.
- Runtime limits validate positive wall time at most 24 hours, estimated
  token budget at most one billion, reserve at most 1,048,576, and turns 1–500.
  Defaults are 24 hours, 100 million estimated tokens, 1024 reserve, and 500
  turns. These are whole-prompt limits; guidance does not refresh them.
- Context accounting counts serialized canonical request bytes, escaping,
  tools, and framing plus response reserve using a four-bytes-per-token
  heuristic. It is neither an exact tokenizer nor a universal upper bound.
  Provider usage is separate from the precharged attempt budget.
- A complete turn must be a nonblank tool-free `Stop`, or nonempty `ToolCalls`.
  At most 32 calls with unique bounded non-NUL IDs, bounded names, object
  arguments, and at most 1 MiB encoded arguments per call are accepted.
  Invalid, length-limited, filtered, unknown, or inconsistent completions cannot
  directly dispatch tools (`src/runtime/session.rs:621-665`).
- Mailbox pending tasks/guidance share 32 messages and 1 MiB of text. Active
  guidance is at most 64 KiB; idle task text may use the full text bound.
  Editing/withdrawal is permitted only before claim. IPC narrows guidance to
  16 KiB and rejects blank text and terminal controls except newline/tab.
- Live journals cap a newline-inclusive record at 64 KiB, total bytes at
  64 MiB, and each follower poll at 256 records. IPC frames cap at 20 KiB;
  the server gives an accepted client a 250-ms exchange deadline, and client
  calls require a positive timeout at most five seconds.
- Process supervision validates idle time at most 3600 seconds, optional
  per-attempt time at most 24 hours, retries at most ten, and combined output
  at most 16 MiB. An absent attempt timeout is allowed. The observed opaque
  execution API explicitly forbids retries.

## Observed internal design and failure paths

### Native execution, accounting, and controls

The runtime starts a prompt-deadline watcher, owns the mailbox for the run,
attempts a session-start record, then prepares each request through
`ContextManager::prepare`. Preparation validates complete tool-call/result
groups, preserves system messages and outstanding user goals, retains the newest
safe unit, and compacts eligible older groups transactionally. Summaries are
bounded lossy user-role history explicitly labeled non-authoritative. Failure
leaves the supplied request and rolling summary unchanged. Legacy `compact` and
`maybe_compact` interfaces lack this full-request/reserve guarantee.

Before each provider attempt, estimated request cost plus output reserve is
deducted from the shared budget without refunds on interruption/failure.
Transient transport failures use deterministic capped exponential backoff;
attempt deadlines grow with attempt number but are clamped by remaining prompt
time. A length finish may regenerate with doubled reserve only if it still
fits context; no tools from the length-limited response run. Guidance preserves
the current attempt number/reserve rather than resetting the allowance
(`src/runtime/driver.rs:648-899`).

Generation-rate timing uses a `DecodeClock` spanning decoded-event observations,
subtracting callback handling time. First-event wait, terminal wait after the last
event, retry backoff, and tool execution are outside the observed decode interval.
A single buffered event has no measured interval and produces a zero rate rather
than division by a near-zero placeholder. Timing accumulates across failed and
length-regenerated attempts and into the run accumulator before steering/error
handling; live averages also include earlier attempts in the current turn.
Post-turn generation and average rates both use cumulative accepted output over
accumulated observed decode seconds. This is an observation convention, not
provider-internal generation timing or complete discarded-attempt token usage.
Elapsed prompt time starts with the prompt budget, including initial recording
(`src/runtime/session.rs:85-155`, `src/runtime/session.rs:699-742`,
`src/runtime/driver.rs:366`, `src/runtime/driver.rs:676-805`,
`src/runtime/driver.rs:906-936`).

A separate `Operation` token mirrors prompt cancellation, mailbox shutdown, or
pending guidance at approximately 10-ms intervals. Guidance interrupts quiet
generation and backoff without cancelling the whole prompt or interrupting an
already executing effect. Partial interrupted model output is not added as an
assistant message. Pending later calls from a superseded tool turn receive
explicit rejected results, preserving call/result pairing.

Mailbox locking linearizes submission classification, claim/edit/withdraw,
dispatch readiness, and final-answer completion. Guidance receives correlated
accepted/claimed/delivered, withdrawn, or dropped observations. The bridge's
acceptance reply is earlier and distinct from these delivery observations.
Delivery records are attempted before another provider request/effect.
Required recording failures stop subsequent work. A dispatch-record hook runs
before the final mailbox/cancellation recheck and host executor invocation.
The readiness point is not a grant; guidance arriving after that point belongs
to an already-started dispatch.

Non-repeatable effects use canonical action hashes and an eight-entry repeat
ring, progressing to rejection and a stalled-proposal circuit breaker.
Host-declared repeatable observations bypass the effect breaker and can emit
pagination guidance. The runtime does not automatically replay a tool,
prove filesystem change, invoke observation-invariant/reasoning detection, or
apply a temperature change merely because the shared detector supports those
operations (`src/runtime/driver.rs:515-610`, `src/loop_detection.rs`).

Tool previews include observed exit/status/timeout/cancellation/output-limit
flags and are bounded to an approximately JSON-encoded 8-KiB envelope.
Invalid UTF-8/NUL-bearing output is replaced with byte-count/SHA-256 metadata,
not lossy binary payload. Text is not generically secret-redacted. Full
`ToolOutcome` data still reaches a recorder, whose storage/redaction policy is
owned by the host (`src/runtime/session.rs:347-412`).

On exit the driver drains withdrawal/drop notices, ends mailbox ownership,
clears invalid final answers, and attempts terminal recording even after an
inner failure. Recorder terminal failure returns a recording error including
the original failure description. Some errors return before a terminal sink
event; callers must handle the returned `Result`, not rely exclusively on
`Finished`/`PromptEnd`/`Failed`. Watcher drops notify and join their threads.
Executors/transports/sinks/recorders must cooperate: Sav cannot preempt a blocked
injected callback or force cancellation of an arbitrary executor.

### Bounded live journal and local IPC

Journal creation serializes/validates initial metadata before reserving a
no-clobber filename, then writes and synchronizes the start record. Path opening
walks ancestor directories with held descriptors and `openat` no-follow flags;
the leaf is nonblocking, close-on-exec, and checked as a regular file.
Creation requests owner-only read/write permissions. The writer and follower
retain original descriptors across pathname replacement
(`src/observation/journal.rs:317-385`).

Record serialization stops at its byte bound before append. Validation failures
do not append; actual write/sync failure poisons the writer. Explicit
checkpoints synchronize prior diagnostics before host effects. Finish appends and
synchronizes a bounded terminal disposition. Total-capacity exhaustion does not
reserve room for a final record, so completion recording can itself fail.

The follower retains partial live tails, enforces version/identity/contiguous
sequence, exactly one initial start, bounded event types and terminal disposition,
and rejects data after termination. Malformed polling poisons further polls.
EOF without `run_end` or with a partial final record is incomplete; it is not
proof of rollback, failure-free execution, or known effect disposition.

Controls use a generated private directory (0700), Unix socket (0600), and one
owned listener thread. A bounded newline frame is schema-validated before the
host callback; replies are validated/correlated before sending. Nonblocking
connect plus poll/read/write deadlines bound exchanges. Malformed requests do
not invoke the callback. Server drop sets shutdown, joins the listener, and
removes the owned directory/socket (`src/observation/control.rs`).

The control callback is trusted to enqueue/acknowledge promptly; its execution
is not itself time-limited. The pathname must remain outside agent context and
journals, which this generic mechanism cannot enforce for its caller. There is
no peer-credential authentication, per-request deduplication, or same-UID
isolation. Client metadata checks cover the socket/final parent, not a
descriptor-anchored full ancestor walk. Diagnostic/event payloads are not
generically redacted or safe terminal-rendering output.

### Opaque supervision and cancellation

Supervision uses `Command`/literal argv, null stdin, piped stdout/stderr, and a
fresh process group. No implicit shell is involved, although an explicitly
selected shell/executable retains its own semantics and ambient host authority.
`run_supervised_observed` accepts a cancellation token and fallible live byte
observer; bytes remain opaque and are never decoded into native tool authority
(`src/supervisor.rs:349-369`).

Two nonblocking reader threads use an atomic combined output budget and a
16-slot bounded channel with 4096-byte chunks. Monitoring polls cancellation,
optional wall timeout, child exit, output, and idle/repetition conditions.
Observer failure stops monitoring. Cleanup kills the owned process group,
waits for the child, drains accepted output, and joins readers on normal and
monitor-error paths. Drain timeout is one second; escaped descendants retaining
output produce `OutputStreamsRetained` rather than an indefinitely joined reader.
There is no containment or termination guarantee for a descendant that escapes
the process group (`src/supervisor.rs:465-770`, `src/supervisor.rs:866-907`).

General supervision may retry idle/repetition failures, with caller-visible
prior-effect uncertainty notices; nonzero exit, output overflow, cancellation,
and attempt timeout are terminal. The observed opaque API refuses any nonzero
retry allowance. Cancellation/attempt-timeout errors take priority after cleanup.

Signal handling installs one process-global SIGINT/SIGTERM handler. Both signals
set a shared atomic flag and forward SIGINT to the single registered group.
The two installation calls are the documented unsafe sites; installation failure
is logged rather than returned. Concurrent independent supervisors/watchers
share one group slot and a consuming interrupt flag, not a per-run signal
registry (`src/interrupt.rs`, `src/runtime/session.rs:708-742`).

## Bounded repair observations, unresolved findings, and limits

These conclusions come from current source paths, not implementer descriptions
or reproduced test runs. No statement determines compliance with external intent.

### Re-inspected mechanisms

- **External-profile retry policy:** `src/main.rs:692-701` derives zero retries
  for an external profile and returns an invalid-policy error for positive
  explicit retries. Both forwarding and JSON capture use this derived policy.
  `tests/registry_cli_policy.rs:111-149` checks default one-attempt behavior after
  a repetition failure, positive-retry refusal before spawn, and explicit zero,
  in both output modes. The source no longer exhibits the previously observed
  profile retry bypass. Explicit command retries remain a distinct caller-chosen
  authority surface (`tests/registry_cli_policy.rs:152-188`).
- **Profile model bounds:** `src/main.rs:574` now calls
  `native.transport_bounded(timeout, arguments.max_response_bytes)`.
  `src/registry.rs:147-166` takes the smaller profile/caller deadline and passes
  the caller output cap into validated transport construction. Profile mode
  rejects explicit slot/TTFT overrides in CLI parsing instead of silently
  accepting them (`src/main.rs:164-171`). Source tests cover unary/streaming
  response overflow, a short caller deadline, and invalid/inapplicable values
  before provider contact (`tests/registry_cli_policy.rs:209-335`). The earlier
  ignored-bound mechanism is absent from the re-inspected path.
- **Tool-free native composition:** `src/runtime/session.rs:316-320` selects
  `None` for an empty tool list, satisfying the direct validator's condition at
  `src/direct_transport.rs:764-765`. Tests assert both choice branches and invoke
  that production validator with a no-tool session request
  (`src/runtime/accounting_tests.rs:45-71`). That directly covers validation
  composition, not an end-to-end runtime/network session.
- **Live counters and timing:** the driver supplies distinct cumulative input,
  provider total, and accepted output (`src/runtime/driver.rs:425-432`).
  Live output is cumulative, and retry decode timing is accumulated outside the
  attempt loop and into the shared run state before error/steering disposition
  (`src/runtime/session.rs:85-155`, `src/runtime/driver.rs:676`,
  `src/runtime/driver.rs:803-805`). Counter, production-loop, first-token/
  presentation exclusion, and retry-time tests are present at
  `src/runtime/accounting_tests.rs:90-400`. The previously observed counter
  conflation and retry-local timing reset are absent from these paths.

No new concrete failure was identified in this bounded inspection of those
repairs. This is not an assertion that every timing, authority, or lifecycle
interleaving is tested.

### Unresolved independent finding

**Concurrency/lifecycle limitation: process signal ownership is singular.**
   Registration overwrites one global PGID (`src/interrupt.rs:88-94`).
   Supervisor cleanup clears it unconditionally
   (`src/supervisor.rs:299-300`, `src/supervisor.rs:551`), even though a
   conditional-clear helper exists. Concurrent runs can lose another live
   run's forwarding registration or consume its interrupt flag. Callers need
   serialized ownership or independent cooperative tokens; no such serialization
   is enforced by these public supervision APIs.

Additional boundaries requiring caller judgment are the non-sandboxed external
process authority, secret-bearing event/output handling, trusted nonblocking
callbacks, heuristic context/token estimates, public snapshot mutability,
non-transactional concurrent registry updates, partial-schema GBNF behavior,
and the less restrictive separate trajectory API described above.

## Test-source coverage and execution limitations

The inspected test sources provide the following evidence of intended checks.
The workspace run below supplies aggregate passing execution evidence, but does
not by itself map each source assertion to a named executed test:

| Source | Observed test cases |
| --- | --- |
| `tests/registry.rs` | Whole-profile precedence, source/digest snapshot stability after file change, schema/type/endpoint/credential/bounds refusal, disabled profiles, malformed/missing sources, symlink upsert refusal, literal one-pass argv, read-only inspection, and one profile-selected native CLI request. |
| `tests/registry_cli_policy.rs` | External-profile one-attempt/default/explicit-zero semantics, positive retry refusal before spawn, preserved explicit-command retry choices, unary/streaming caller response/deadline bounds, and pre-contact rejection of inapplicable watchdogs/invalid limits. |
| `src/runtime/accounting_tests.rs` | No-tool/with-tool request shape, production direct-validation composition, deterministic decode-clock arithmetic, cumulative live input/output/provider totals, first-token/presentation/backoff exclusion, retry decode accumulation, two-turn production-loop counters/rates, and initial-recording inclusion in elapsed time. |
| `tests/runtime_controls.rs` | Quiet-generation interruption, provisional-output exclusion, edit/withdraw/queue bounds, retained active-effect result and rejected later calls, shutdown, backoff attempt preservation, recording failure before presentation/dispatch, dispatch-recording guidance race, withdrawal after interruption, final-answer/submission classification, retained token charge, attempt cap, and shared wall deadline. |
| `tests/observation.rs` | Live checkpoint/tail following, held-descriptor replacement behavior, 256-record batches, sequence/version/run/link/special-file rejection, atomic bounded serialization, private socket modes/drop cleanup, bounded idle client/malformed request handling, late-guidance rejection, and total journal capacity refusal. |
| `tests/external_supervision.rs` | Opaque byte delivery, cancellation/observer-failure termination, retry refusal, and pre-cancel refusal before spawn for the observed library API. |
| `tests/supervisor.rs` | General retry uncertainty, nonzero no-retry, idle/attempt/output limits, process-group descendant cleanup, and escaped retained-pipe termination of the supervisor wait. |
| `tests/model_transport.rs`, `tests/model_capacity.rs`, `tests/loaded_model.rs` | Provider wire mapping, explicit/non-success finish handling, output bounds/schema fields, reasoning separation, malformed/duplicate/truncated response refusal, streaming/chunking, cancellation/deadlines/cadence, selected-model serving capacity, and loaded/router model identification. |
| `tests/command.rs`, `tests/profiles.rs`, `tests/setup.rs`, `tests/catalog.rs`, `tests/cli.rs` | Representative argv/JSON rendering, command-profile persistence, setup qualification/force/selection and acknowledgment paths, HTTP/ACP discovery correlation, and CLI acknowledgment/output/profile-store behavior. Some CLI/catalog cases were indexed by test name rather than fully traced. |
| `tests/gbnf.rs`, `tests/trajectory.rs`, `tests/loop_detection.rs`; source-local unit tests | Grammar string-shape/error cases, trajectory roundtrip/error cases, hash/repeat-detector cases, retry arithmetic, assistant final-answer helper, legacy context compaction, signal forwarding, and wrapper-script content assertions. |

No Sav-local dedicated tests of `ContextManager::prepare` malformed-group,
transactional failure, or first-request reserve boundaries were found in the
inspected sources; runtime-control tests exercise successful preparation
indirectly. The new CLI policy and accounting tests cover the formerly missing
repair cases described above. Their timing assertions use deterministic clock
arithmetic or bounded synthetic sleep fixtures, not actual provider decode
telemetry. The new no-tool case directly checks validation but does not execute
the native loop through a real HTTP endpoint. No newly inspected dedicated
regression checks decode-time retention specifically across guidance or
length-regeneration, nor concurrent signal-owner isolation.

### Inspected pre-repair workspace execution evidence

The caller reported that
`CARGO_TARGET_DIR=/opt/target cargo test --offline --locked --workspace --no-fail-fast --quiet`
exited 0. The observer read and mechanically summarized the supplied log at
`/home/stefan/.copilot/session-state/13dd58ac-9d6a-4316-9d9b-bada72cc2b4c/files/workspace-tests-refreshed.log`:
102 test-result summaries, all `ok`, totaling **1687 passed, 0 failed, and
14 ignored**. Those totals corroborate the supplied report; they are
workspace-wide, not a Sav-only count. The caller identified the ignored cases
as existing; their provenance was not independently checked against history.
This run predates the currently observed repairs and must not be treated as
evidence that the repaired workspace has passed its full suite.

The caller additionally reported successful workspace all-target Clippy with
`-D warnings`, ordinary workspace rustdoc, and strict Sav/Skott rustdoc. Their
logs were not inspected. Strict whole-workspace rustdoc was reported to encounter
unchanged warnings in other components; that is not recorded as a successful
strict workspace gate, nor were those peer sources or their change status
inspected.

### Reported post-repair targeted execution evidence

Implementers reported **41 targeted CLI/registry tests** and **103 runtime tests**
passing, together with strict Sav Clippy and formatting. No corresponding output
was supplied for inspection in this refresh, so these remain reported results,
not observer-run or independently log-verified results. Full post-repair
workspace validation had not yet been run according to the caller. No test
selection, coverage percentage, or current workspace-wide gate is inferred from
those counts.

This execution evidence does not resolve the source-based findings or establish
coverage of real provider accounts, installed external agents, filesystem faults,
same-UID adversaries, arbitrary callback stalls, non-Linux operation, or every
host integration. No peer implementation, production code, or test code was
changed or inspected beyond the stated source boundary.
