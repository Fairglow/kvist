# tools

Repository helpers that are not part of the Rust workspace. Scripts are invoked
by the [justfile](../justfile) or CI, not by Cargo directly.

## `nextest.toml`

Shared repository scheduling policy. `just test` explicitly selects this file;
direct nextest runs should also pass `--config-file tools/nextest.toml`.
It reserves the complete test-thread pool for the native repository-wide
sandbox trial while ordinary tests stay parallel. It also reserves the pool for
Blad's dedicated `diagrams`/`mermaid_backend` binaries, Markdown Mermaid
regression, adapter budget tests and exact mixed-suite regressions that render
Mermaid: color-depth geometry, runtime-tool independence, session dump policy
and viewer diagram resizing. Source-only fixtures in mixed suites and unrelated
tests stay parallel. This replaces the previous three-name policy, which
omitted other engine callers, notably `color_depth_never_changes_visible_geometry`.
Native Mermaid retains a 500 ms cooperative wall-clock render deadline; cold
engine initialization and CPU scheduling count against it. These tests must
not compete with other suite work merely to test diagram semantics and
deterministic resource boundaries.

The CI profile inherits both default rules. No assertion, production limit,
retry or skip is changed. Both exclusive rules have priority 100, followed by
the long Skott and maerg Rust inventory trials at priority 50. Ordinary work
keeps priority 0 and normal parallelism. This prevents a late exclusive barrier
followed by a newly started inventory straggler. Priority never substitutes for
exclusivity. A test is reported slow every 30 seconds and fails after ten such
periods (five minutes); it is not retried or skipped. The repository sandbox
trial uses `target/repository-trial` for nested Cargo output, so another Cargo
build holding ordinary `target/debug` locks cannot stall that trial.
Pool reservation coordinates only this nextest
invocation, not unrelated host processes or another test runner. This tracked
policy does not depend on the ignored, sandbox-local `.config/nextest.toml`.

`test_nextest_policy.py` pins the scoped selector, whole-pool allocation and
CI-profile inheritance. Repository CI runs it with Python 3.11+ (standard
library only); it stays outside Blad's self-contained package tests:

```bash
python3 -B -m unittest discover -s tools -p 'test_*.py'
```

Changing the selector requires updating the expected fixture scope, then
checking effective nextest selection and start/finish ordering. The Python
regression checks configuration, not timing or filter interpretation.

## `profile_tests.py`

`just test-profile` runs the full nextest suite with the same scheduling policy
and writes private reports to a fresh directory under the system temporary
directory, printing its path immediately. CI uses the same profiler and retains
reports even when tests fail. Python 3.11+, Linux procfs and cargo-nextest are
required; no Python packages, daemon, credentials or network are needed.
Provision the native test prerequisites and offline vendor snapshot first.
The profiler never vendors, installs tools, retries failures or changes limits.

For a reproducible warm comparison, build once with the same package/features,
then profile the identical selection and thread count several times:

```bash
cargo nextest run --config-file tools/nextest.toml --offline --locked \
  --workspace --all-features --no-run
NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 python3 tools/profile_tests.py \
  --output /tmp/kvist-test-profile-1 -- \
  cargo nextest run --config-file tools/nextest.toml --offline --locked \
  --workspace --all-features --test-threads num-cpus --no-fail-fast \
  --message-format libtest-json-plus --message-format-version 0.1
```

`--output` must name a **new directory outside the checkout and working
directory**. The report directory is mode 0700 and its files mode 0600:

| File | Meaning |
| --- | --- |
| `summary.json` | Version 1 wall/CPU/capacity, memory, I/O, context-switch and low-CPU summary; longest tests first; explicit sampling/parse errors |
| `samples.jsonl` | Half-second, elapsed-time process-tree samples: PID/start identity, executable name, CPU, RSS, storage bytes and kernel wait channel |
| `stdout.log` | Original nextest JSON 0.1 events, including failures |
| `stderr.log` | Build/test progress and diagnostics; `tail -f` while investigating |

Other explicit argv commands can also be profiled without a shell; per-test
timings are available only when stdout contains the supported nextest events.
The original command's nonzero exit or signal is preserved. Profiler setup
errors fail explicitly; an incomplete summary and existing logs are retained
when possible. SIGINT/SIGTERM are forwarded to the owned command group, with
a two-second kill escalation. Remaining original-group members are also
terminated after the command exits, with an explicit warning.
This is diagnostic supervision, not a sandbox;
escaped descendants require the command's own supervisor.

Interpret **CPU seconds / wall seconds** as average occupied cores, not as a
percentage of the whole machine. Capacity accounts for CPU affinity and visible
inherited cgroup-v2 quotas; missing quota visibility is reported, and affinity
alone may overestimate capacity in other mount layouts.
`cpu_seconds` reconciles observed PID/start-time
lifetimes with waited-child CPU, retaining observed ended subtrees when
namespace teardown loses their accounting, without adding them twice.
It is a **sampled estimate**, not exact machine-wide accounting or a guaranteed
lower bound: non-atomic reads/reaping can overlap, while unseen work is missed.
`waited_child_cpu_seconds` and user/system values are reported separately;
those can undercount sandbox descendants. Short-lived unseen children count
only when their waited CPU survives. External daemons such as sccache and
unseen orphaned/unwaited processes are not counted.
RSS sums shared pages more than once: sampled aggregate peak is not physical
memory consumption. `largest_process_peak_rss_kib` is the Linux waited-process
high-water mark, not that aggregate. Sampled storage counters can miss work
and overlap waited-child accounting; they are not unique physical I/O totals.
Block counters include waited-child I/O. Host pressure snapshots are **host-wide**
context, not attribution to this run. Sampling can miss short-lived processes;
counter regressions and inaccessible resources are reported, not concealed.
Per-process raw inclusive CPU counters must not be summed; the reconciled
sample total handles their ancestry. Thread wait summaries distinguish a
sleeping leader from its working/blocked workers (at most 256 threads per
process, with truncation marked).
Profiles include instrumentation overhead; compare repeated warm runs, and
measure cold build time separately.

The profiler reports sustained low CPU live and retains intervals below
0.25 occupied cores. Low CPU is a **symptom, not a deadlock diagnosis**:
timeout/cancellation, PTY and network fixtures deliberately wait. Correlate
samples with the longest test names and stage diagnostics. High I/O pressure
or `D` state suggests storage waits; `locks_lock_inode_wait` plus a Cargo
lock-owner/waiter pair suggests lock contention; `futex` alone proves neither.
For a live suspected lock, inspect only the reported process IDs with
`ps -p PID -o pid,ppid,stat,pcpu,rss,wchan:32,args` and
`lslocks -o PID,COMMAND,MODE,PATH,BLOCKER`. For the Rust chain, set
`SKOTT_LOG=skott=debug` for bounded preparation/dispatch durations.
Do not raise deadlines or serialize the whole suite to conceal a stall.

Nextest does not execute doctests. After a full verification run, use
`cargo test --offline --locked --workspace --all-features --doc` separately.
CI does this explicitly. Logs can contain test output and paths; they are
private diagnostics, not canonical evidence or guaranteed secret-free artifacts.
The profiler does not collect environment values or unrelated command lines.
Raw command output is not redacted: review it before sharing or uploading.
Report size, event count and total command duration are not capped; use this
on bounded test commands, not indefinite services. For exact isolated CPU
accounting, use a separately managed delegated cgroup where available.

### Measured investigation (2026-10-27)

With the same warmed workspace, features and 20 test threads, the old scheduling
policy took 57.897/64.219 seconds; the new policy took
28.920/32.088/28.071 seconds. Every run passed 1,875 tests with the existing
15 skips. Order was counterbalanced, but shared-host storage/cache conditions
still vary; these are observations, not a portable performance threshold.
Approximate total CPU stayed near 93 seconds while average occupied cores rose
from 1.4–1.6 to 2.9–3.3. All 34 reserved trials remained exclusive and ordinary
work reached 20 concurrent tests. The native repository trial took 3.4–3.6 seconds.

The reported spontaneous timeout did not recur. Holding the ordinary target's
Cargo locks did reproduce an idle lock wait; the dedicated nested target
passed that regression. Separately, a nonexistent crate-local Git watch forced
unchanged Skott builds to recompile. Git-resolved existing inputs fixed that:
after one 21.79-second build, the identical repeat finished fresh in 0.10 seconds.
Deleted tracked files still require an existing-parent watch, which can broaden
invalidation; this is not a promise that every dirty checkout avoids rebuilds.

The remaining critical path is the 24–27-second inventory trial, not an exhausted
thread pool. Samples also observed storage/journal waits and intentional
timeout/terminal waits. More test threads or larger deadlines do not remove
those dependencies. Preserve full inventory verification; measure its stages
before considering independently splittable trials or storage/cache changes.
The local `msrv` recipe still uses Cargo's runner; shared scheduling and slow-test
policy apply to nextest (`test`, `test-profile` and both workspace CI jobs).

## `cargo_install_binaries.py`

Extracts the built executables from a Cargo NDJSON build log and copies them to
an install directory. The justfile's `build` and `release` recipes run the
workspace build with `--message-format=json-diagnostic-rendered-ansi` into a
log file, then call this script to install the resulting binaries:

```bash
python3 tools/cargo_install_binaries.py <log-file> [install-dir]
```

- `<log-file>` — the Cargo NDJSON build log (e.g. `.kvist/cargo-build.log` or
  `.kvist/cargo-release.log`).
- `[install-dir]` — where to copy the binaries. Defaults to `~/.kvist/bin`.

The script parses only `compiler-artifact` messages that carry an `executable`
path (i.e. binary targets), copies each to the install directory, and reports a
warning if it saw compiler artifacts but none were binaries. It is a plain
Python 3 standard-library script with no third-party dependencies.
