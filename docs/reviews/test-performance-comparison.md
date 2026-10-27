# Source-blind testing-performance comparison

Date: 2026-10-27. **Bounded comparison, not acceptance or certification.**

## Evidence boundary

Read only `ROOT_CONTRACT.md`, `skott/REQUIREMENTS.md:115–158`,
`skott/CONTRACT.md:210–222`, `skott/DESIGN.md:622–670`, and
`skott/IMPL.md:1–131`, plus the expressly supplied session artifacts beneath
`/home/stefan/.copilot/session-state/9c1b4b84-9989-4760-af85-a022fd157fa9/files/`.
Those comprise the two source-observation accounts, quality/red/warm logs,
selection/event-verification JSON, and the six named runs' summaries/stdout/stderr.
No source, tests, executable configuration, queues, history/diffs, prior reviews
or older implementation-record sections were inspected. No tests were executed.
Standard-library parsing independently checked supplied events and arithmetic;
all execution remains attributed to the implementer. The later profiler
observation supersedes the earlier account where repairs are explicitly recorded.

## Independently checked finite results

Using `exclusive-selection.json`, start/completion pairing and active-name
**counters**, all five workspace runs contain the same 1,875 passing executions.
Five names occur twice, so sets would miscount concurrency. Excluded 15
nonexecuted starts per run: five have ignored completions; ten lack execution
completions, consistent with stderr's 15 skips. Executed starts/completions balance.

All 34 selected exclusive executions have zero event-stream peer overlaps.
Ordinary concurrency reaches 20 under both policies. Each new-policy run starts
all 34 exclusive trials before ordinary work; the repository trial moves from
zero-based start 1800 to 33, and inventory checks from 708/1809/1812 to 35–37.
The separate CI-profile native run passes 17 tests, starts its exclusive trial
first without overlap, and reaches ordinary concurrency three. These recomputed
results agree with `scheduling-events-verification.json`; they establish observed
runner-event ordering, not cross-invocation or operating-system-wide exclusivity.

| Attributed run | Test phase (s) | Command wall (s) |
| --- | ---: | ---: |
| scheduling-before-final | 57.897 | 58.339 |
| scheduling-after | 28.920 | 29.508 |
| scheduling-after-repeat | 32.088 | 32.816 |
| scheduling-before-reverse | 64.219 | 64.659 |
| scheduling-after-final | 28.071 | 28.526 |

This supports a substantial improvement in these warmed, counterbalanced
observations—not a portable bound or isolated causal proof. Build identity and
command flags remain attributed; instrumentation changed between early/later
reports, and host/cache conditions were uncontrolled. Sampled CPU is
92.49–94.32 seconds; CPU/wall and waited-user-plus-system arithmetic agree.
Average estimated occupied cores rise from 1.44–1.59 to 2.86–3.31.
Nonzero counter-regression diagnostics are distinct from the empty sampling-error
lists. Early summaries incorrectly label the estimate a lower bound and omit
quota warnings; later summaries correct both. Capacity percentages remain
uncertain. Low-CPU intervals do not establish deadlock or CPU-bound behavior.

## Intent comparison and arbitration

1. **Metadata scope is unresolved.** Requirements promise installed-file bytes
   and metadata excluding access times. Observation records complete byte hashes,
   modes, lengths, nanosecond mtime/ctime and literal links, but omits ownership,
   hardlink identity and xattrs/ACLs. This does not satisfy an unrestricted reading
   of “metadata.” Human arbitration must determine the intended field set;
   narrower historical intent cannot be assumed from the permitted excerpts.
   One/two/eight-worker agreement and passing inventory fixtures support the
   recorded representation, not omitted fields or atomic snapshots.

2. **Privacy wording exceeds the demonstrated guarantee.** Requirements forbid
   recording environment values. Structured sampling reportedly avoids them, but
   arbitrary command stdout/stderr are retained unredacted. Consequently an
   absolute guarantee covering child output is unsupported; no actual secret
   disclosure is alleged. Clarify whether the prohibition includes raw logs.
   Private local modes/out-of-checkout placement are observed; CI artifact access
   controls were not established by this evidence.

3. **Other scoped mechanisms are supported, with verification limits.**
   Held-lock and nonexistent-watch red logs demonstrate the regressions;
   corresponding fixtures pass in the supplied event streams. Dedicated-target
   execution, bounded hashing, existing Git-resolved watches, linked-worktree/
   export handling, CI policy and separate doctests agree with observations.
   Warm logs show 21.79 seconds compiling, then 0.10 seconds without compilation.
   Freshness is demonstrated for that unchanged build, not every Git state.

## Remaining verification boundaries

- No supplied run exercises 30-second progress or five-minute termination.
  Their configuration is observed; failure-not-skip/retry behavior and unchanged
  production deadlines are not dynamically established here.
- Metadata fixtures cover tracked edits/worktrees/exports and epoch output,
  but not actual Cargo invalidation for each HEAD/ref/index/epoch transition.
  Deleted-path watch broadening remains a documented limitation.
- Final profiler observations and 18 passing Python tests support parsing,
  arithmetic, failure retention and repaired descendant cleanup. Handler
  restoration on exceptional setup/sampling paths lacks direct assertions.
  Final group SIGKILL has no subsequent group-empty wait; stopped/zombie
  descendants are not proof of reaping. Escaped sessions are explicitly outside
  scope. Report growth and attribution accuracy remain bounded-review limits,
  not newly invented acceptance requirements.
- Separate doctest success is logged. Exact lint/toolchain invocations, full
  sample contents, focused snapshot timings and actual hosted CI execution were
  not independently verified.

Human disposition is needed for metadata/privacy scope and acceptance of the
remaining verification limits. No intent or implementation was changed to
resolve these disagreements. This report does not certify security, universal
performance, full compliance or human acceptance.
