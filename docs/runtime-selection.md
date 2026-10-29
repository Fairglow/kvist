# Runtime selection and ownership

The approved architecture is [ADR 0015](decisions/0015-shared-runtime-and-run-inspection.md).
Sav is the shared headless runtime library. Skott is its interactive/headless
workspace client and independent run inspector. Kvist/maerg owns protected task
authority, workflow and canonical evidence; Galla enforces approved effects.
Skott links Sav, not the `sav-run` executable.
Its native workspace tools and deliberately separate dependency-acquisition
boundary are described in [Native agent toolkit](agent-toolkit.md).

## Select a backend, not a different source of truth

| Backend | Loop owner | Suitable use | Authority |
| --- | --- | --- | --- |
| Native Ollama / llama-server | Shared Sav native runtime with host integration | Interactive/headless workspace work; protected Kvist authoring through its broker | Host-supplied tools and separately enforced grants |
| External coding agent | Selected CLI owns its internal loop; Sav supervises the complete process | Explicit opaque-agent execution | Explicit outer execution boundary; never inferred from provider flags |
| One-shot command | No first-party tool loop | Diagnostics and bounded prompt execution | Explicit acknowledged process execution |

Model transport, agent driver and effect enforcement are separate choices.
Copilot/Gemini catalog discovery through ACP does not make those agents native
model transports. An opaque external agent must not be wrapped inside the
native model/tool loop. Tool requests, provider output and permission switches
are never authorization.

Protected task authoring must reject a backend that cannot satisfy its broker
contract. Do not replace `kvist task run` with the workspace shell merely because
both can use a sandbox. Skott's workspace and maerg's protected component grants
are deliberately different.

## Shared configuration

Sav owns typed provider/profile resolution, bounded discovery, qualification and
atomic persistence. Skott and Kvist bind the same profiles rather than maintain
copied model definitions. Application bindings, UI preferences, tool policies
and task approval remain local to the application.

A provider instance identifies an endpoint or executable. A profile identifies
the provider-facing model and supported settings. Discovery is a time-bound
observation, not an approved configuration or guarantee of future availability.
Advertised, tested and enabled capabilities remain distinct.

Explicit task selections are resolved once per attempt. Loaded-model preference
is an interactive convenience and must not override a pinned task binding.
Configuration loading never implicitly starts a provider, performs billable
qualification, installs tools, or grants process/network authority.

## Interactive and batch operation

The same native mechanisms support both terminal and unattended operation.
Skott headless workspace execution retains its required private journals and
does not acquire an unconfined host fallback. Maerg retains its protected broker,
dependency phases, verification and human finalization.

`sav-run` remains a small diagnostic/setup/supervision client. Its continued
existence does not imply a second first-party native engine.

## Protected task retries and failure recovery

`task run` without an ID asks `Run suggested next task ...? [Y/n]`.
ENTER accepts; `n`, EOF or cancellation declines without execution. The shell
asks before opening its working stage. Streamed task execution has no competing
spinner, so prompts, retry notices and provider output cannot be overwritten by
cursor animation. The elapsed time formerly shown by the spinner included time
spent waiting for confirmation; it was not model execution time.

Inference retries transient socket failures, transport
watchdogs and HTTP 429/500/502/503/504/507 automatically. The pinned native
profile's `max_attempts` includes the first request (default 3, allowed 1-32);
`retry_base_delay_secs` and `retry_max_delay_secs` set the exponential schedule
(defaults 2 and 30 seconds). Each delay is jittered between half and all of the
capped schedule. Explicit command-endpoint profiles use those defaults.
Each retry reports phase, selected model/endpoint, failed attempt, cause, wait
and remaining task time on stderr and in the `*.events.jsonl` journal.
No logging flag or further confirmation is required to continue. Streamed
progress is provisional until the complete response validates: transient stream
failures report that partial output is discarded before retrying. No incomplete
tool proposal is executed.

Requests and backoff consume the same approved role `timeout_seconds`; retries
never extend it, reset cumulative token charges or change authority. A request
can exhaust the remaining budget before all permitted attempts run. Increasing
role limits or changing the registry requires reviewing the effective policy
and running `kvist task approve-policy` again. Cancellation, malformed responses,
invalid configuration and recording failures stop
without retry. Tool effects, verification commands and entire tasks are never
automatically replayed: an interrupted operation may already have changed files.

Protected task startup does not require serving-capacity metadata. A configured
native `context_limit` skips the probe. Otherwise one optional GET has at most
one second or 10% of remaining task time, whichever is smaller. Missing capacity,
an unsupported metadata route or a transient failure produces a warning and a
`capacity_unavailable` event, then inference proceeds. Invalid metadata,
authorization failures and cancellation still stop explicitly. Inference retains
its own retries; optional metadata consumes none of those generation attempts.

Unknown capacity uses a **provisional local working budget**, not an inferred
server limit: the larger of 32768 and estimated initial input plus output reserve
plus 8192 tokens of history headroom, capped at 1048576. A native profile's output
reserve is configured or 8192. Explicit command-endpoint profiles retain their
existing 8192-token local context and 1024-token output reserve without a probe.
If estimated initial input, reserve and headroom exceed the provisional ceiling,
startup fails explicitly rather than silently dropping headroom or input.
The provider can reject a request exceeding its actual window. Set the shared
profile's `context_limit` to the actual per-request serving window for predictable
compaction; context size does not measure throughput or predict prefill duration.
Skott's separate workspace startup still requires configured or discovered capacity.

The reported 2026-10-29 Maerg attempt-0002 reached inference: its journal records
`context_limit: 262144`, then an inference request before the 300-second timeout.
The server was still processing a roughly 130000-token prompt when Kvist
cancelled it. That is not failed capacity discovery. Protected prompts previously
included the entire local task queue (about 280 KB in this incident), including
unrelated completed-task history. They now validate the complete queue but inline
only the selected task's authored definition; all other selected artifacts remain
exact. Diagnostics announce inference separately, with estimated input, output
reserve and remaining approved time. Estimates are not exact provider token counts.
Large intent documents or a slow/cold model can still exhaust five minutes.
Choose a faster model or explicitly increase the architect role's
`timeout_seconds` and, if needed, its shared profile's `deadline_secs` (maximum
600 per request), then review and reapprove policy. No timeout or approval is
silently increased, and this repair does not rerun or complete the blocked task.

The transport's historical "slot allocation" error actually measured time until
HTTP response headers, not provider slot ownership. The streaming header
watchdog remains bounded, but unary generation and metadata GETs now use the
complete caller request deadline. A slow metadata response can represent model
loading/switching, a blocked route or a stalled server; free inference slots do
not prove that metadata is responsive. Provider logs are needed to distinguish
these causes. A shared role binding can also select a different model from the
shell's convenience model: execution reports the actual role and model.

An ordinary exhausted or persistent failure returns nonzero and records
`blocked`, the underlying diagnostic, attempt identity and inspection commands.
Read its log with `kvist task log COMPONENT TASK`, inspect retained effects, fix
the cause, then deliberately return it to `pending` with `task transition`
before starting a new attempt. A successful implementation remains
`in-progress` pending the reported exact `task finalize ... accept` command.
Multi-task selections stop there or at a decision/failure; they do not call it
completed or silently proceed. `task next` refuses unresolved attempt evidence.

If failure evidence or the final queue update cannot be persisted, the error
reports both the original cause and the persistence problem. Completion is
unconfirmed; inspect the retained journal rather than deleting the fence.
`task recover ... --disposition execution-did-not-start` is only for
host-authenticated runner-descriptor failures proving no launch/write exposure;
missing model turns or diagnostic logs alone cannot establish that proof.

For the reported 2026-10-28 Maerg incident, the saved journal contains
`run_start`, `profile_selected`, then `run_end: failed`; the trajectory has zero
turns and the canonical attempt has only `prepared`. The files end about
15 seconds after the run began. The incident-era capacity-discovery call had
no retries; later source added a retry loop but still retained the hidden GET
header limit. The repair preserves the old evidence and does not mark its
`shared-runtime-compliance-review` task complete.
After reviewing the evidence and any current intent staleness, a human can
explicitly abandon that attempt, without claiming rollback or no execution:

```sh
kvist task finalize maerg shared-runtime-compliance-review attempt-0001 block \
  --reason "Abandon failed capacity-discovery attempt after reviewing retained effects; completion is not certified."
# Review the changed intent before accepting it:
kvist component accept maerg
kvist task transition maerg shared-runtime-compliance-review pending
```

Explicit block finalization and log inspection permit otherwise valid stale
intent. This avoids a circular gate: acceptance remains fenced by an unresolved
attempt, but rejecting it does not require accepting changed intent first.
Rejection retains the evidence and effects even when result digests drifted.
It requires the latest exact task/attempt and a nonblank reason, never commits
changes, and cannot bypass explicit recovery fences or active operation locks.
If its evidence append is interrupted after the blocked decision is stored,
repeat the same block command and reason to finish recording. A stale component
still cannot run, accept a task result or transition back to pending until
its intent is reviewed and accepted. Acceptance reports remaining blocked tasks
rather than claiming they became current.

Recovery gates distinguish authority from task status:

| Operation | Valid tracked component states |
| --- | --- |
| Ordinary transition | Current or blocked; dependency/reason checks still apply |
| Log inspection, explicit block, stale-lock cleanup, authenticated no-launch recovery | Current, blocked or stale |
| Task execution, selection and result acceptance | Current only |

Unlock never clears an attempt fence or removes a live owner's lock, even with
`--force`. If it removes a stale lock but unresolved evidence remains, the
failure explicitly reports both facts. No-launch recovery still checks the exact
authenticated approval, scope and queue; it does not accept changed intent.
Missing, invalid, unsupported and untracked artifacts remain errors, not reset
opportunities. Unknown effects or broken/unauthenticated recovery evidence need
human arbitration; no blanket force-reset or universal crash recovery is claimed.

Awaiting-decision transitions retain their nonblank reason in both queue and
audit records. After deciding and accepting any changed intent, explicitly
return the task to pending or in-progress. Blocked guidance similarly reopens
with `task transition COMPONENT TASK pending` without `--reason`; it does not
silently execute the task.

Both finalization dispositions name the latest exact task/attempt and reject
conflicting decisions. Acceptance stores the completed queue decision before
appending final evidence. If that append is interrupted, repeat the same
`task finalize COMPONENT TASK ATTEMPT accept` without `--commit` to finish
recording, not task execution; an identical repeat does not change timestamps or
duplicate the event. Existing commit-pending recovery remains separate.

Use the rebuilt/reinstalled binary for a new run. Review any stale intent before
`component accept`; do not accept it merely to bypass a gate. A different
next-ready task never establishes the preceding task's completion.

## Observation and control

Run owners append versioned live diagnostic records during execution.
`skott --attach JOURNAL` inspects those records without loading models or tools.
Without a terminal it prints a snapshot; `--follow` waits for appended records.
Closing inspection never terminates execution. EOF without a terminal
disposition is incomplete, not successful completion.

Controls are separate private local capabilities. Supply `--control SOCKET`
with `--cancel-run` or `--guide TEXT` to send an explicitly authorized request.
Queue acknowledgement is not model-context delivery. Guidance supplements the
existing task without changing approved intent, grants or accounting; executing
effects are not interrupted to steer. Cancellation cannot undo partial or
unknown effects. Neither logs nor a missing process authorize replay/resume.

## Frameworks and deferred integrations

The `rig-core` experiment was rejected and removed in
[ADR 0009](decisions/0009-brokered-model-transport-and-isolated-effects.md).
No Rig transport or agent runtime is part of the current dependency graph.
MCP, richer ACP execution and other agent frameworks require explicit adapters,
capability tests and host authority contracts before activation. They do not
replace canonical Kvist state or evidence.
