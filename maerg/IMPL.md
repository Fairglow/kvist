<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

Bounded observation of current `maerg` source and test source. This record
describes implemented behavior and its limits; it is not a compliance
determination, approval, certification, or review receipt.

## Scoped resilient task execution repair - 2026-10-28

Independent clean-slate `resilient-source-observer` inspected the current
retry, agent, task-command, shell and corresponding test source only.
`disposition-followup-observer` independently observed the narrow subsequent
cause-preservation, disposition-fence and evidence-identity changes.
Both excluded intent, queues, prior records, reviews, peer implementations,
chat and Git history. Neither edited files, executed tests or certified
compliance. Their source-derived observations are:

- Capacity discovery and generation have separately counted retry scopes,
  sharing the approved task wall budget. Native policy supplies attempt and
  backoff limits; explicit endpoints use defaults. Upper-half jitter is
  capped by remaining time; cancellation interrupts backoff.
- Required records precede requests/retries. Recording and entropy failures
  stop dispatch and preserve the preceding model cause text alongside the
  secondary error. This preservation is textual, not a typed source chain.
- Stream events are provisional. Only a completed validated turn reaches the
  broker; retries explicitly discard partial output without executing partial
  proposals. Token reservations remain cumulative. No retry loop encloses
  effects, verification or the whole task.
- Failed execution evidence precedes blocked disposition. Execution evidence
  alone no longer releases an in-progress/pending attempt fence. Observed
  execution plus a terminal queue status, an associated committed disposition,
  or pending/explicit human disposition resolves the guard. Only the
  lock-owning run's terminal transition exempts its exact task/attempt pair.
  Other attempts and recovery states remain checked. Durable disposition
  remains recognized after deliberate blocked-to-pending transitions.
- Failed persistence reports unconfirmed completion and both causes.
  Required evidence/control closure failures are returned even when execution
  already failed. Wrapped model errors retain timeout/output-limit flags.
- Successful implementation and verification leave pending human disposition;
  batches reread status and stop unless the preceding task actually completed.
  Guarded next-task selection refuses unresolved attempts.
- Suggested-task confirmation precedes working output and spinner creation;
  streaming dispatch suppresses a competing spinner. Full multiline and long
  diagnostics reach the existing wrapping renderer, which was not re-observed.
- Exclusive log creation tries the timestamp identity and 1023 ordered
  suffixes, never overwriting prior logs. Live journal and trajectory names
  derive from that identity. Log and journal reservation is not transactional:
  journal creation can still fail explicitly after an empty log is reserved.

The first observer identified cause loss during failed retry recording and
premature fence release. Implementer-attributed regressions reproduced both;
the narrow follow-up observed the repairs and identified no concrete defect
in those changed routines. A rapid explicit retry also reproduced same-second
log-name collision, which was repaired and independently observed.

Other bounded observations remain outside this repair: explicit human
finalization does not itself require successful execution/verification evidence;
its digest reads can be skipped on failure; produced change inventory covers
only immediate readable test files and is not a complete change set.
These are retained limitations, not automatically arbitrated intent changes.

Implementer-attributed execution: `cargo test --offline --locked -p kvist -p sav`
passes 988 tests across 58 suites with seven existing ignored tests. This
includes real PTY confirmation, retry exhaustion/recovery, interrupted-stream
discard, failure-persistence and deliberate reattempt regressions. Execution
output is retained in the session's `affected-tests-final.log`; it is not an
independently executed qualification or whole-component acceptance.

Independent source-blind `isolated-repair-comparator` compared the bounded
intent/task-definition bundle with these source-derived observations and
attributed named execution results, without tools, source or older records.
It established no concrete repair-scoped mismatch. Execution remains
implementer-attributed; exhaustive retry-status/boundary coverage, universal
EOF precedence and re-observation of the wrapping renderer remain gaps.
The earlier comparator read outside its allowlist and is not counted as
independence evidence. Neither comparison accepts revisions, resolves the
historical attempt or certifies the broader finalization/change-inventory
limitations. Those remain available for explicit human arbitration.

## Scoped Rust inventory parity repair - 2026-10-27

Independent clean-slate `rust-inventory-observer` read only the authoring Rust
environment, native toolchain tests and shared runtime script source. It read
no intent, queue, prior record, review or Git history, ran no tests and made no
compliance determination. Its source-derived observations are:

- Preparation requires agreement between rustup's installed names and the
  host installation directory entries. Private registrations preserve those
  names exactly; no synthetic `default` registration is created.
- The selected installation is mounted read-only at `/rust/toolchain`;
  others use `/rust/toolchains/{sorted-index}`. Generated settings choose the
  selected name. Initialization copies settings into scratch and links the
  read-only registrations. Its separate `/rust/toolchains/default` alias is
  outside the registration directory.
- Wrappers select through rustup or an explicit leading `+name`; Cargo and
  cargo-clippy inject offline/locked resolution against `/rust/vendor`.
- The native trial compares host and sandbox inventory/default, compiler,
  rustdoc, targets and components; changes private defaults; compiles each
  installed target; and runs library tests and doctests. It asserts no
  synthetic registration, read-only installations, failed uninstall and
  unavailable selection, and compares recursive host state snapshots.
- These are finite source assertions, not execution evidence. The trial
  assumes multiple toolchains including `stable`, does not exercise pins or
  registry dependencies, and retains its pre-existing in-worktree-runner skip.
  Preparation uses `$HOME/.rustup` while the test helper honors `RUSTUP_HOME`;
  custom-home parity is not established. Executable digests and metadata
  rechecks are not whole-resource content authentication.

Implementer-attributed execution reproduced the inventory mismatch before the
repair and the explicit synthetic-registration assertion before removal.
After removal, all three ordinary `sandbox_toolchain` tests pass, including
the native parity/build/non-mutation and read-only workspace-provider trials;
four existing opt-in tests remain ignored. Strict all-target/all-feature
Clippy for maerg and Blad, workspace formatting and whitespace checks pass.
The three reported Blad failures did not reproduce in default/all-feature or
unoptimized library runs, 100 repeated library runs, or CPU-count nextest.
No Blad rendering behavior or production resource limit was changed.

The standalone all-feature CPU-count nextest run executed 1834 tests:
1833 passed, with 15 existing skips. The remaining failure was Skott's
`native_repository_tests_execute_inside_the_workspace_sandbox`, which exceeded
the sandbox preflight budget; whole-workspace success is not claimed.

Separate source-blind `rust-inventory-comparator` compared this supplement and
attributed execution with local intent and the root contract, without reading
source or test source. It identified no concrete repair-scoped mismatch.
Pins, registry dependencies, custom homes and the pre-existing skip versus
mandatory native coverage remain evidence gaps. The contract's all-indexed
mount wording versus the selected singular destination is pre-existing and
explicitly accommodated by design, not introduced by registration removal.
This is a scoped comparison, not acceptance or whole-component certification.

## Scoped model-budget regression repair - 2026-10-25

Independent clean-slate `budget-source-observer` inspected only the budget,
transport-construction and corresponding regression/fixture source. It excluded
intent, queues, prior records, reviews and history, ran no tests and made no
compliance judgment. The observations below are transcribed from that report:

- `TurnBudget` subtracts elapsed wall time saturating at zero.
  `model_deadline` rejects cancellation before expiry, returning the respective
  model-transport cancellation or timeout error. Initial native/explicit
  transport construction and each retry use that check; shared-native capacity
  discovery uses it when capacity is unspecified.
- Model identity resolution and token preflight still precede the host-turn
  deadline check. Endpoint construction still precedes the liveness probe;
  no configured agent command is executed by this path. These checks do not
  authorize effects. Retry limits, emitted-output restrictions and the shared
  wall-clock budget remain visible in the source.
- The new host-turn test supplies zero time for native and explicit paths,
  asserting timeout without cancellation and cancellation when both hold.
  It does not instrument construction/network calls or exercise
  positive-budget cancellation.
- Live steering/accounting assertions retain exact known-usage totals,
  cumulative token-limit failure and trajectory totals. The cancellation
  fixture retains assertions for an explicit cancellation error, the previous
  answer, known 11/7 usage and unsuccessful replay with total 18.
- Those two fixtures allow 60 seconds for model work rather than asserting
  durable-recording latency. Their provider accept helper polls nonblockingly
  with a 60-second guard and skips empty liveness connections. Accepted streams
  retain three-second per-read timeouts; this is not a hard end-to-end read
  bound. Steering and provider-close interleavings remain finite tested cases,
  not exhaustive proofs. Cancellation/expiry checks are not atomic with
  subsequent work.

Implementer-attributed execution: the zero-budget regression first failed with
an invalid-deadline configuration error, then passed after the guard repair.
All eight live-runtime integration tests pass, including ten repeated
four-worker nextest iterations (80 executions). Final all-feature workspace
verification with four workers passes all 1817 tests, with 15 existing skips;
strict workspace lint and formatting pass. CPU-count-concurrency runs still
encounter intermittent native-render and repository-preparation timeouts under
observed host I/O/memory pressure, including after diagram-test isolation.
Their production limits were not relaxed, and that full-concurrency limitation
remains unresolved. No component acceptance or whole-runtime certification
is implied.

The separate source-blind `budget-intent-comparator` compared this supplement
and attributed execution with local intent and the root contract, without
reading implementation/test source or running tests. It found no concrete
scoped discrepancy in cancellation-first deadline checks, retained usage or
cumulative accounting. It retained that zero-budget cases do not prove
positive-budget cancellation, absence of network/construction calls or every
race; checks are non-atomic and per-read fixture timeouts are not end-to-end
bounds. Fixture scheduling slack changes no production limit or authority.
This comparison is neither blanket compliance nor acceptance.

## Observed public contract

### Command and workflow surface

The package exposes the `kvist` executable and library modules for command
dispatch, project/component inspection, durable task queues, supervised
execution, provisioning, and domain errors. The library forbids unsafe code
and rejects non-Linux compilation.

The CLI includes project initialization, conversion, import, reverse
discovery, tree/status/doctor, queue repair, component document
creation/validation/acceptance/commit, task selection/transition/run/log/replay,
policy approval, unlock/recovery/finalization, agent configuration and
qualification, dependency/toolchain provisioning, custom prompts, an
interactive workspace shell, help, and completions. A hidden `authoring-apply`
command applies staged effects. This is a summary, not an exhaustive
option-by-option interface specification.

Task execution is distinct from arbitrary custom prompts. Tasks use validated
project/component state, VCS tracking checks, explicit execution approval,
lifecycle locking, task-kind role selection, and the protected broker/effect
path. Implementation success additionally invokes verification and records
pending human disposition. Other successful task kinds can transition directly
to completed. Surfaced decisions pause in awaiting-decision; ordinary recorded
agent/verification failures can lead to blocked state. Errors can instead leave
an unfinished prepared attempt. Exact task selection, item-prefix batches, and
`all` are supported; omission of a task requires interactive confirmation.

Custom prompts delegate to a standalone `skott` subprocess, forwarding prompt
and model/effort selection. Child runtime and sandbox behavior are delegated,
not established by this inspection. Headless prompt dispatch rejects the host
execution opt-out. The interactive workspace shell is a separate front end to
these workflows.

Evidence: `src/lib.rs`, `src/cli.rs`, and
`src/task_commands.rs::{run_task_with_controls,run_task_or_item_with_controls,
verify_task,finalize}`.

### Shared selection and pinned authority

`AgentConfig` retains typed, enabled shared registry profiles as loaded
snapshots. Named and role-based resolution select the configured profile;
resolution does not itself rediscover a backend. Shared bindings disallow
mixing copied local provider/profile definitions and restrict role tables to
profile references plus role policy. Registry parsing and lookup are delegated
to `sav::registry`.

The agent configuration source digest incorporates resolved profile names,
source paths, and digests. Execution approval also incorporates serialized role
configuration, limits/redaction policy, root-context digest, sandbox settings,
runner/backend identity, and test policy. Task execution passes its approved
runner, capability probe, and policy digest into effect execution.

Protected shared task selection requires the resolved native backend. Disabled
or incompatible selections are refused; an external-agent profile is not
silently executed instead. Explicit local command configurations remain a
separate supported path: Kvist extracts model endpoint/provider/model data and
uses structured transport rather than executing the configured agent command.

Evidence: `src/config.rs::{resolve_shared_registry,AgentConfig}`,
`src/task_commands.rs::build_execution_approval_for_runner`, and
`src/agent.rs::{select_model_choice,execute_host_turn}`.

### Observation and explicit controls

Agent execution creates a required live journal at
`.kvist/runs/<session>.events.jsonl` and announces its path on stderr before
generation. Events include shared-profile identity, model budgets and request
counts, redacted model output/failures, broker counts, dispatch/results,
guidance/control notices, compaction, and terminal disposition. These live
records are labeled noncanonical evidence, separate from workflow acceptance.

`task run --allow-control` is explicit opt-in and is passed through text/JSON
dispatch and batch execution. Default execution opens no control server.
With opt-in, binding a private endpoint and enforcing its protocol are
delegated to `sav::observation::bind_runtime_controls`; the endpoint is
announced on stderr. A library entry point accepts caller-owned mailbox and
cancellation objects without opening IPC.

Guidance is supplemental: the original task and selected artifacts are pinned
in the session's system context, the original task is also supplied as the
initial user message, and guidance appends another user message explicitly
preserving task and approved scope. Mailbox notices distinguish accepted,
claimed, delivered, withdrawn, and dropped guidance. Superseded calls receive
not-executed tool results rather than being silently treated as applied.

Evidence: `src/agent.rs::{execute_agent_with_controls,execute_agent_controlled,
execute_agent_inner}`, `src/agent/live.rs`, `src/agent/controls.rs`,
`src/cli.rs::TaskCommand::Run`, and `tests/control_cli.rs`.

## Observed requirements and constraints

The following are source-enforced checks and observed limits, not inferred
normative requirements.

- Agent execution has a 64-generation ceiling and a shared elapsed-time budget.
  Transient generation retries are capped at three attempts, additionally
  bounded by shared native retry settings. Steering interruptions consume
  generation/attempt budgets; they do not create unlimited retries. A normal
  completed turn resets its generation-attempt counter.
- The endpoint transport is constructed for validation before liveness contact.
  The probe independently requires a numeric loopback address, uses short
  connects, checks cancellation, and performs cancellation-aware backoff.
  Malformed/oversized responses, response-level failures, and cancellation are
  not ordinary availability retries. Text-emitting streamed attempts are not
  retried.
- Shared native context capacity is explicit or obtained through a delegated
  capacity lookup. If unavailable, the run refuses inference rather than
  guessing capacity or choosing an opaque backend. Response reserve is
  configured or derived from capacity. Explicit local endpoint selections use
  the runtime library's default context capacity and a default response reserve.
- Selected context is restricted to the five local component artifacts and
  exact root/parent contract file mounts. At most seven paths are accepted,
  each path is bounded to 4,096 bytes, each file to 1 MiB, and aggregate file
  content to 4 MiB. Files must be regular UTF-8 files; descriptor-relative
  no-follow opening covers each path segment. Size and modification/change
  timestamps are compared around bounded reads. There is no recursive
  source/peer discovery.
- Context files and the original task are quoted into system context before
  request preparation. Delegated context management can compact completed
  conversation groups but a request whose retained context cannot fit fails;
  selected artifacts are not silently omitted to obtain a request.
- A configured role token limit is cumulative. Each inference attempt reserves
  estimated input plus output allowance before dispatch, reducing output
  allowance to fit remaining budget. Known positive usage replaces that
  reservation; failed, missing-usage, and zero-usage attempts retain conservative
  charges. Retries and steering share this accounting.
- Reported input/output totals sum known usage across retained turns, not just
  the last turn. The log distinguishes reported/unavailable turns and
  conservatively charged or unknown attempts. Known totals are not a claim of
  exact usage for unreported work.
- The closed authoring vocabulary is `write_file`, `edit_file`,
  `propose_decision`, and `request_dependency`. Default file authority is
  `src/` and `tests/`, with protected document names and absolute/traversal
  destinations rejected. Default depth/content bounds are 12 levels, 1 MiB
  per effect, and 8 MiB per plan. Edits require a single matching substring.
  These tools do not grant arbitrary shell execution or general file reading.

Evidence: `src/agent.rs`, `src/agent/context.rs`,
`src/agent/accounting.rs`, and `src/authoring/mod.rs`.

## Observed internal design and failure paths

### Conversation and effect ownership

`execute_agent_inner` selects the backend, creates log/live state, establishes
mailbox ownership and optional IPC, resolves capacity, materializes selected
context, and starts a `sav::runtime::AgentSession`. Each iteration delivers
guidance, obtains a request, prepares context, performs token preflight, and
calls observed structured transport under the remaining time budget.
Conversation, context preparation, operation-token linkage, transport protocol,
and shared control internals are library delegation.

A completed model turn is retained before token-limit checks, turn validation,
steering processing, broker decisions, or effects can return an error.
Superseded completed turns therefore retain available usage. Interrupted
attempts without a completed response cannot supply an invented turn or usage
report.

The broker reduces untrusted proposals to file effects, decisions, accepted
dependency requests, or dropped intents. A decision records bounded proposal
evidence and pauses before that turn's effects. Dropped intents prevent file
effect application. Accepted dependency requests are recorded, but protected
fulfillment is explicitly unsupported as described below.

File effects are staged under component state with content identity and then
applied by the current Kvist executable inside the effect sandbox. Requests
carry the task's pinned runner/probe/policy, read-only context and staged input,
network-denied authoring phase, remaining timeout, output bound, and owner
cancellation token. The applier revalidates destination/content and edit
preconditions before replacement. Effects are sequential, not transactional:
earlier writes are not rolled back when later work fails.

### Steering, cancellation, and retained outcomes

Dispatch readiness is checked before staging and before launching another
effect. A conservative uncertain-effect record is inserted into the
owner-retained turn before dispatch. Confirmed results replace it; dispatch
errors leave it explicitly uncertain. Cancelled, timed-out, output-limited, or
otherwise unsuccessful dispatched effects are treated as potentially mutating,
not as proof that nothing changed. Staged-file cleanup is scope-owned.

The turn/effect collection lives outside the execution closure. The finalizer
attempts one aggregate log/trajectory record after ordinary success, early
error, or cancellation from that closure. The text log is synchronized;
trajectory recording errors propagate rather than being silently ignored.
Original execution errors remain errors even if recording also fails; a
recording failure is then logged. Mailbox closure and live finish are both
attempted without replacing an existing execution error.

Trajectory entries redact session/task/call/tool/destination diagnostics and
retain dispatch/result correlation. Its legacy mutation boolean cannot express
uncertainty, so possible mutation is represented conservatively as true with
an explanatory diagnostic. Decision/dependency evidence filenames are SHA-256
hashes of call IDs; untrusted IDs are payload data, not path selectors.
Replacement uses the shared synchronized atomic file helper.

These changes do not make storage infallible: initialization failures before
the execution closure, recording failures, crashes, and panics do not establish
complete evidence. A bounded text log can also truncate later detail.

Evidence: `src/agent.rs::{execute_agent_inner,apply_authoring_effects_with,
summarize_effect,record_turn_outcome,evidence_filename,write_atomic}`,
`src/agent/controls.rs`, and `src/agent/regression_tests.rs`.

### Redaction and live records

Streaming redaction scans at UTF-8 character boundaries and chooses the longest
matching configured literal. Incomplete candidate prefixes are held across
deltas and final flush is explicit. This avoids arbitrary byte slicing and
preserves matches across the tested Unicode/overlap partitions.

`LiveRun` recursively filters string values in metadata and events using its
stored profile redaction values, preserving `type` discriminators. The run
identity in the journal envelope is derived from a hash. Text/result filtering
also uses the agent redaction set, which includes allowed environment values.
Diagnostic recording remains required whether or not an observer is attached.

The sets are not identical: `LiveRun::create` receives
`profile.redaction_values`, while `redaction_values(profile, sandbox_config)`
later adds environment values for text/outcome filtering. Consequently,
environment-only literals in model/call metadata are not assured the same
recursive filtering as configured profile literals. Literal replacement is
not general secret detection, and announced filesystem paths/tracing output
are not established as universally redacted.

Evidence: `src/agent/redaction.rs`, `src/agent/live.rs`, and
`src/agent.rs::{execute_agent_inner,redaction_values,redact}`.

### Protected provisioning and verification

The agent invokes `vendor_project_protected` with the task-approved runner,
VCS selection, policy identity, owner cancellation token, and remaining budget.
That entry point refuses all model dependency fulfillment: it lacks an exact
requested-dependency binding to the existing lock/manifest. It checks
pre-cancellation/zero budget and returns a clear unsupported error without
inspecting paths, staging work, or launching a host package manager. Existing
ready vendoring is not misrepresented as fulfillment of a fresh request.
The agent propagates this error after retaining request/turn evidence; it does
not log acquisition failure and continue as though dependencies were available.

`check_protected_vendor_readiness` is a separate read-only operation. It checks
controls, policy-identity syntax, comparison against the provided runner
identity, Rust strategy, verified supported manifest, locked readiness, and
absence of stale registry material. It refuses other languages or Rust state
needing repacking. It does not fetch, fulfill a model request, or fall back to
the standalone provisioning command.

Standalone `vendor_project` remains distinct human-invoked provisioning:
its Rust path can fall back from sandbox fetch to host `cargo vendor`, and
other language paths can run host package managers. Host repacking/capture
there does not become protected merely because protected request options exist.

Task verification remains outside the agent control-server scope. It checks
approval and component working-directory policy, then routes through generic
network-denied or vendored offline sandbox topologies. Implementation success
records pending human disposition. Runner/OS enforcement and peer runtime
internals are not independently established here.

Evidence: `src/vendor_command.rs::{ProtectedVendorOptions,
vendor_project_protected,check_protected_vendor_readiness,vendor_project}`,
`src/vendor_command/protected_tests.rs`, `src/agent.rs::execute_agent_inner`,
and `src/task_commands.rs::verify_task`.

## Remaining observed scope and workflow limits

- The optional private server exists only during agent execution; it does not
  control later verification. Readiness checks and sandbox launch are separate
  caller operations. This source inspection does not establish the delegated
  mailbox's concurrency linearization or prove absence of a dispatch/guidance
  race.
- On an agent error, lower-level log/trajectory finalization is attempted, but
  `run_task_with_controls` still propagates `Err` before appending its
  task-level `agent-execution` record. Lower-level evidence preservation is
  not the same as a completed task journal/status transition.
- Successful implementation finalization still builds its change list from
  immediate files in `component/tests`, labeling them create with null
  pre-digests. Source files, nested tests, and deletions are not captured by
  that enumeration; listed-change validation does not supply a complete
  post-execution source-tree snapshot. This is a broader workflow limit,
  separate from runtime turn/effect retention.
- The dependency broker's private-origin predicate still checks prefixes of
  the full origin rather than parsing the host. A private numeric host inside
  an HTTPS URL can be classified as accepted. Protected fulfillment nevertheless
  refuses before any fetch; classification alone does not demonstrate network
  contact.
- Verification's literal set includes architect/developer values and sandbox
  environment values, not security-reviewer-only literals. The additional
  test-policy environment allowlist only restricts the sandbox allowlist:
  `allowed_environment` intersects the lists. There is no source support for
  an assertion that test-policy-only variables are additionally exposed.
- Shared registry qualification, control endpoint permissions/authentication,
  context estimation/compaction, journal protocol/durability, transport
  cancellation, and sandbox OS enforcement depend on delegated libraries or
  processes. This record does not infer their internals from call sites.

Evidence: `src/task_commands.rs:4196-4228`, `:4285-4306`,
`src/task_commands.rs::{finalize,evidence_redactions}`,
`src/authoring/mod.rs::is_unverified_origin`,
`src/sandbox.rs::allowed_environment`, and the agent sources above.

## Evidence and verification limits

The current observation read only permitted `maerg` source/test source.
It did not read standalone intent documents, queues, previous implementation
records, repository metadata/history/diffs, peer implementation, or prior
reviews as evidence. No production/test files were modified. The implementation
record was replaced without reading its prior contents.

**Current execution evidence:** the caller reports that selected maerg
library, agent, live-runtime, control-CLI, and task-command tests passed, and
that all 60 dogfood boundary tests passed. No corresponding fresh run log was
independently inspected for this refresh. No tests, builds, linters, or
reproduction commands were run by this observer. Fresh broader workspace gates
were still pending when this observation was requested; no post-repair
workspace-wide test, Clippy, or rustdoc pass is claimed.

**Inspected test-source coverage, distinct from execution evidence:**

- `src/agent.rs` includes Unicode/overlap redaction cases checked across UTF-8
  partitions, endpoint/probe and retry bounds, streaming retry behavior,
  selection, and effect summaries.
- `src/agent/accounting.rs` covers cumulative known usage, output capping,
  conservative missing/zero usage charges, and reservations retained across
  retries/interruption.
- `src/agent/regression_tests.rs` covers selected-context restrictions,
  no-follow paths, UTF-8/file/aggregate/context-window bounds, retained partial
  and uncertain dispatch outcomes, cumulative known usage with missing reports,
  pre-cancelled probing, redacted trajectory correlation, and hashed evidence
  filenames for hostile call IDs. Injected dispatch tests establish local
  bookkeeping behavior, not real sandbox writes.
- `src/agent/live.rs` covers recursive diagnostic-value redaction with valid
  journal envelopes.
- `tests/live_runtime.rs` checks early journal visibility, profile-literal
  filtering in returned model metadata, quiet-generation steering, original
  task preservation, mailbox closure, opaque-backend refusal, actual selected
  context in native and explicit endpoint requests, cumulative steering usage
  and token limits, forbidden endpoint refusal before probing, retention after
  cancellation, and refusal of context that cannot fit.
- `tests/control_cli.rs` covers default-off/explicit opt-in parsing in both
  output modes, not an independent control-socket round trip.
- `src/vendor_command/protected_tests.rs` covers unconditional unbound-request
  refusal, pre-cancellation/exhausted budgets, read-only ready-Rust inspection,
  pinned runner mismatch, malformed policy, refusal of unsupported/stale/missing
  material, and host-tool sentinels.
- The inspected dogfood fixture in `tests/dogfood_boundary/support.rs` uses a
  shared native mock profile with explicit `context_limit = 65536` and
  `response_reserve = 1024`. Its context capacity is fixture configuration,
  not evidence that production silently omits context or assumes that capacity.

This bounded observation does not establish exhaustive race coverage,
crash/power-loss durability, universal secret filtering, exact billing for
unreported provider work, or absence of other defects.

## Source-only test/helper observation (2026-10-27)

Observed only `maerg/tests/live_runtime.rs`, `tools/profile_tests.py`, and
`tools/test_profile_tests.py`; no files changed or tests executed by this
observer.

- **Effect gate:** A synthetic Bash runner consumes its request, connects to an
  ephemeral loopback TCP port, announces readiness, and waits for `continue`.
  Tests inject guidance before release or release before cancelling the
  subsequent generation. This synchronizes effect execution; it neither
  performs the requested file write nor demonstrates sandbox isolation.
- **Usage evidence:** Assertions retain completed-turn usage across steering:
  24 input/12 output tokens, with trajectory total 36. The token-limited case
  fails while retaining known usage 6500/7 and total 6507; these tests alone do
  not establish the budget calculation. Steering before any usage arrives
  records one conservatively charged/unknown attempt separately from the later
  known 10/4 tokens. Cancellation preserves earlier answer text, known 11/7
  usage, and an unsuccessful trajectory ending with total 18.
- **Profiler failures:** Completed failed-test events are retained and printed
  individually before up to five slowest nonfailed events. A fixture checks
  two fast failures remain visible despite six slower successes, without
  printing their embedded failure details. Command exit status is preserved;
  malformed event data becomes report diagnostics.
- **Scope and limitations:** Gate waits have explicit deadlines, but thread
  joins and some older listener accepts lack independent deadlines; request
  accumulation has no size cap. Helpers require Unix paths and permissions,
  Bash `/dev/tcp`, and external executables. Most sockets are loopback-only;
  the forbidden-endpoint fixture briefly binds all interfaces. The profiler
  launches explicit argument vectors in a new session, cleans up that process
  group, and creates private reports outside the checkout; escaping
  descendants are not contained. It scans host-visible process metadata,
  retains raw command output, and imposes no overall runtime or report-size
  limit. Linux sampling can miss brief/reparented processes and peaks. Coverage
  is synthetic and source-observed, not proof of portability, isolation,
  compliance, or acceptance.

## Long-session source-observation supplement (2026-10-28)

**Evidence boundary:** Source/test inspection only. No commands, tests, or builds were executed. Regression assertions are source evidence rather than executed proof; these observations do not certify compliance.

Each protected execution creates a fresh conversation, context manager, task budget, control ownership, token budget, live journal, and retained-turn collection. There is no fixed generation-count ceiling. Role defaults impose a 300-second task budget, no cumulative-token limit, and 65,536-byte output capture. Configuration accepts task timeouts through thirty days and output limits through 1 MiB.

`TurnBudget::model_deadline` now returns the lesser of remaining task time and **24 hours**, rejecting cancellation or exhausted time first. This limits individual transport requests without shortening the stored task budget. Shared native transports additionally apply their configured attempt deadline. Gateway probes, retries/backoffs, and effect timeouts consume the same remaining task budget. Enforcement is phase-based rather than an independent whole-task deadline watcher.

`RunControls::deliver` now returns whether guidance actually reached the conversation. Withdrawal-only, dropped, or empty batches do not renew attempts. The execution loop resets `model_attempts` when delivery returns true; it also resets after an ordinary completed generation. Guidance therefore renews generation opportunities without resetting elapsed task time, retained evidence, or cumulative token accounting.

Known provider usage accumulates across retained completed turns. Failed/interrupted attempts and unavailable or zero usage retain conservative reservations. Remaining token capacity can reduce the next output allowance; nonzero provider usage replaces its reservation. Transient model attempts are capped at three, further restricted by shared-profile settings. Failed attempts that already streamed text to stdout are not automatically retried.

Generation uses operation-scoped cancellation, while executing effects receive task cancellation. Guidance preserves the system-pinned original task, skips undispatched calls, and retains completed effects. Complete turns are retained before validation/effect handling. Durable dispatch precedes execution; uncertain effects are explicitly retained rather than replayed. Finalization attempts to preserve earlier evidence on cancellation/error.

Selected context is restricted to recognized local artifacts and exact root/parent mounts: at most seven paths, 4,096 encoded bytes per path, 1 MiB per file, and 4 MiB aggregate. Reads reject links, nonregular files, invalid UTF-8, and detected changes. Shared native capacity uses configuration/discovery; explicit endpoint mode uses an 8,192-token window and 1,024-token reserve. Compaction prefers four recent groups, but `run_turns` grows independently of compacted history.

Live diagnostics inherit Sav’s journal rotation and follower behavior. Final logs are bounded/redacted; trajectories retain known usage and possible mutation.

`multi_day_task_deadlines_fit_direct_transport` asserts that a seven-day budget produces a transport-compatible request deadline while retaining over six days. `multi_day_explicit_endpoint_run_accepts_repeated_human_guidance` asserts five interruptions followed by a sixth successful request. Other inspected tests assert 66-generation continuation, cumulative accounting, context refusal, and cancellation evidence. Dependency-acquisition internals were excluded from this refresh.

### Source-derived approval and effect-timeout addendum

**Maerg addendum — source-derived:** Approval remains bound to configuration-source identity/digest, serialized profile digests, and explicit timeout fields for all three roles. Approval loading authenticates the recorded material; execution compares it with current material. The timeout-change regression changes the developer timeout from five to six seconds and asserts rejection with the source-identity/digest diagnostic, unchanged queue contents, and no sandbox-request file. Effect dispatch passes exactly `Some(remaining_task_budget.min(Duration::from_secs(86_400)))` to the sandbox executor: at most 24 hours, further clipped to remaining task time, with no positive minimum. This observes the supplied timeout, not independently verified executor enforcement. The seven-day-configured fixture scripts 65 effect-producing generations followed by a final response and asserts 66 generations and success; it does not assert seven elapsed days or directly test timeout clipping.

No commands, tests, or builds were executed; these are source observations and test-source assertions, not compliance certification.
