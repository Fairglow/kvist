# Runner hardening: review and qualification record

This is a local engineering report, not a Kvist review receipt, task approval,
artifact acceptance or compliance certification. Controlled intent revisions
have not been formally accepted. Existing unrelated task states are preserved.

## Independent security audit

A separate read-only security-review context examined current runner authority
paths, provider transport, recording, native execution and lifecycle behavior.
It used the sandbox public contract/protocol, not enforcement internals.

| # | Severity | File | Lines | Vulnerability | Confidence |
|---|----------|------|-------|---------------|------------|
| 1 | 🟡 MEDIUM | `skott/src/headless.rs` | 236-241 at audit snapshot | Raw plain answers/diagnostics allowed terminal-control injection, including clipboard OSC commands | 9/10 |
| 2 | 🟡 MEDIUM | `skott/src/process.rs` | 287-335 at discovery snapshot | A directly owned unconfined child could leave its original process group and survive cancellation, timeout or overflow | 9/10 |

**Resolved:** shared terminal-safe rendering visibly escapes C0/C1 controls
except LF/tab in plain answers and human diagnostics. JSON and private
diagnostic transcripts preserve the original text. A captured-output
regression contains only harmless sentinel text; no escape payload was sent
to a live terminal. Exploitation required an attacker-influenced answer, a
terminal accepting clipboard commands, and a subsequent human paste; this was
not automatic host execution.

The audit additionally identified an unestablished history limitation:
path-following host reads and truncation after unbounded allocation.
History now opens descriptor-relatively with no-follow traversal through every
component, uses nonblocking opens before rejecting nonregular files, checks
metadata and bounds actual reads, and decodes replay lossily. Symlink,
ancestor-link, oversized and invalid-UTF-8 regressions failed before the fix.

The same independent reviewer verified the scoped corrections against rebuilt
current source: **9 history, 1 diagnostic and 4 headless tests passed**.
It reported no remaining concrete finding in that fix-verification scope.
An intermediate test-module placement build error was fixed and requalified;
older-binary results were not substituted for current-source verification.
This was a scoped fix verification, not a second broad audit.

The subsequent independent audit identified finding 2 in the newly shared
supervisor. **Resolved:** cleanup signals the original owned group and
independently kills the retained direct child before reaping. Group-signal
success or `ESRCH` no longer implies direct-child termination, and cleanup never
signals the child's substituted group or the supervisor's group. Six regressions
failed before the fix: cancellation, timeout and overflow, each with an empty
or retained-member original group.

The independent reviewer inspected the correction and reran **26 subprocess
and 5 supervisor-helper tests**, all passing. It closed finding 2 with 9/10
confidence and reported no remaining concrete finding in that correction scope.
The broader remediation audit also inspected bounded configuration ingestion,
capability probes, finish integrity and semantic cadence without establishing
an additional concrete vulnerability.

The final independent follow-up covered untrusted Markdown/Unicode rendering,
large replay viewports, explicit host-turn acknowledgment and bounded,
cancellable identity/workspace preparation. It reported no concrete
vulnerability in that scope and independently reran **97 current-source
regressions**, all passing. This does not expand the audit to sandbox
enforcement internals or resolve the global/local authority conflict.

The final startup/resize follow-up independently checked canonical directory
validation and wiring, Markdown continuation provenance, bounded resized
retention and styled grapheme/cell handling. It reran **23 focused cases** and
found no concrete vulnerability in that batch. The subsequent table-padding
correction is a pure layout fix covered by the final full suites below.

The final content-reachability audit inspected complete highlighted-code and
multiline table wrapping plus reasoning reveal/eviction bookkeeping. It
independently reran the **four new failed-before regressions**, all passing,
and found no concrete vulnerability in that scope. It did not independently
rerun the whole library or certify fonts, aggregate retention or enforcement.
Its last marker follow-up independently reran three current-source
reveal/eviction tests and found no vulnerability. Wrapped placeholder
continuations now cannot consume another hidden run.
The final initial-construction follow-up independently reran four narrow
marker/reveal/eviction cases, finding no vulnerability. Initial collapsed
labels now wrap with the same provenance before any resize.
The prefix-aware code follow-up independently reran three fitting-indent,
complete-tail and decorated-width cases; all passed, with no vulnerability
identified in that scoped correction.

Remaining audit limitations: no independent adversarial native-file race or
enforcement-internals audit, no termination guarantee for unobservable escaped
host descendants or uninterruptible kernel work, and no claim that private
diagnostic storage is tamper-proof or secret-free.

## Executed qualification

Final qualification snapshot: 2026-10-02, after provider, input, process,
direct-child cleanup, presentation, preflight, startup and resize corrections.
Earlier 388/1105, 437/1154, 452/1169, 460/1177, 464/1181 and
465/1182 and 466/1183-test logs remain historical snapshots and are
not substituted for the current results.

| Gate | Result |
| --- | --- |
| `cargo test --locked -p skott -p sav` | 467 passed (327 runner, 140 runtime); 3 live tests ignored by default |
| Full `cargo test --locked --workspace` with the fixture-only Git setting below | 1184 passed; 3 live tests ignored |
| `cargo clippy --locked -p skott -p sav --all-targets -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed |
| `cargo build --locked -p skott -p sav -p kvist` | Passed |
| Explicit `live_llama -- --ignored --test-threads=1` | 3 passed using the real installed boundary |
| 20 consecutive `subprocess_supervision` suite runs with the unchanged final process/executor/preflight source | 520 passes; 26 cases per run |
| `kvist component validate skott` / `sav` | Passed; structural validation only, not acceptance |
| `kvist doctor` | Project/artifact structure current; all four component queues valid |

Named final logs in the session evidence directory:
`runtime-delivery-qualified.log`, `delivery-qualified-gates.log`,
`live-delivery-qualified.log` and `subprocess-complete-repeat.log`.
They record actual commands, UTC times and
named results; primary runtime/live logs record source identities, and the
live log also identifies its rebuilt test caller and external binaries.
`subprocess-qualified-repeat.log` remains the earlier process-only stability
qualification, not a substitute for the final repeat. The final snapshot
records Rust/Cargo 1.99; executing
the declared minimum Rust 1.95 toolchain is not claimed.

A full workspace rerun exposed a new test-fixture race, not a production
cleanup failure: mutable executable scripts could fail with `ETXTBSY` before
publishing the expected PID. The fixture now uses an immutable executable
script, non-executable per-test input, atomic PID publication and bounded
initialization handshakes. No assertion was weakened. The failed run is
retained as `remediation-final-quality-workspace.log`; the final named workspace
and both twenty-run stability logs above passed after the fixture-only repair.

The host sets `safe.bareRepository=explicit`. An existing maerg acceptance
test pushed a bare-remote fixture successfully but then implicitly discovered
that bare repository during `rev-parse`, producing an empty stdout assertion.
The isolated test reproduced the same failure. Full workspace verification
therefore used this **process-local fixture override**:

```sh
GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.bareRepository \
GIT_CONFIG_VALUE_0=all cargo test --locked --workspace
```

No production enforcement, test assertion or persistent host Git configuration
was weakened. Runner/runtime suites require no such override.

Live qualification used `/opt/target/release/galla-runner`,
`/usr/bin/bwrap`, and the rebuilt native helper. Tests confirmed bounded llama
streaming, normal Stop classification, real SHA-bound read/edit/verify,
preserved CRLF and missing final newline, no workspace staging files,
unavailable host-private files and denied host-local TCP access.
Model identity: `Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL`.
Endpoint: numeric-loopback `http://127.0.0.1:9931`.
No model server parameters were changed. Ollama live behavior is not claimed.

Current-source deterministic tests additionally establish preserved explicit
llama finishes, unknown missing/null terminal reasons, rejection of malformed
reason types, and meaningful decoded generation cadence rather than byte or
heartbeat liveness. A runner test drives a real loopback HTTP/SSE response
through the actual transport and loop: Stop or missing-finish tool proposals
cannot dispatch a tool or produce a successful answer.

The final presentation/preflight batch reproduced seven UI/CLI failures,
three preparation failures and an additional private resolver failure before
repair. Fixes handle incomplete fences, exact separator consumption, ordered
grapheme/cell-width wrapping, replay navigation above 65,535 rows and explicit
host override acknowledgment. Preparation now hashes held regular no-follow
descriptors incrementally with actual read bounds and scans under explicit
entry/depth/path quotas and cooperative cancellation/time limits. Exact quota
boundaries and interrupted-multicall reuse have named passing tests.
The old test expecting silent sandboxed host-override acceptance was updated
to assert rejection, consistent with the deliberately tightened contract.
Failed-before logs are preserved; no universal real-time/kernel guarantee is
inferred from these tests.

The final comparison additionally exposed deferred interactive directory
validation and Markdown resize consuming following unmarked plain rows.
Six more executable regressions failed before substantive repairs, and a
separate table-padding case reproduced reversed left/right alignment.
The selected directory now uses the same canonical existing-directory
validator in both modes. Markdown source/continuation provenance is explicit;
repeated resizing preserves following notices/reasoning and enforces the
existing 5000-row retention bound. Styled text/code/table handling uses whole
graphemes and cell widths, including final decorated rows. Narrow layouts,
Unicode and table-padding cases pass. Fonts/emulators and indivisible glyphs
wider than a viewport are not universally certified.

The subsequent bounded comparison identified three remaining concrete
reachability defects, reproduced before repair: highlighted code and table
tails were discarded, and revealed reasoning kept its pre-resize width.
A fourth failed-before case exposed mismatched hidden runs after placeholder
eviction. Code now wraps complete highlighted spans; table cells emit
successive wrapped rows; reveal uses the current width; and evicted placeholders
discard the corresponding hidden runs. Both collapse and reveal retain the
existing visible-row limit. These are presentation fixes, not expanded effects.
The final source observation exposed another positional marker defect: wrapping
one placeholder could restore later runs before an intervening notice. The
fifth failed-before case reproduced the wrong order; explicit continuation
classification fixes it, with all five retention cases passing.
The sixth failed-before case covered initial collapsed-label overflow on
narrow displays. Initial construction now wraps and classifies head/continuation
rows immediately, rather than waiting for resize.
The seventh failed-before case covered fitting indentation missing from code
continuations. Prefix-aware styled wrapping now repeats the source whitespace
without discarding code tails; the gutter remains on the first row.

Final artifact inspection identified why earlier project-wide inspection had
reported invalid state: newly added task requirement references were not
lexically sorted. The affected references were reordered without changing
their meaning, dependencies, accepted revision hashes or unrelated tasks.
`complete-artifact-inspection.log` records the corrected project structure.
Both `task next` calls now refuse for the legitimate **stale controlled
intent** gate, not invalid queues. No `component accept` command was issued;
structural readiness does not adjudicate the authority conflict below.

## Independent observation and intent comparison

The initial source-only observation and distinct source-blind comparison
produced [F1-F9](runner-intent-review.md), exposing the actual process, finish,
capability, cadence and configuration defects repaired above. The original
findings are retained rather than erased by adjusting intent.

Fresh source-only records have been derived in separate component contexts
without reading intent, queues, previous records/reviews or Git history.
They distinguish inspected tests from supplied execution evidence. The
unchanged runtime record retains its 27-file linkage. The runner record and
distinct source-blind comparison are current against the delivery-qualified
logs after the last fitting-indent correction. All 36 runner identities match
both runtime/live logs; all 27 runtime identities also match the retained
runtime observation. The final comparison resolves the changed-scope concrete
process/input/provider and presentation defects, including code/table tails,
directory selection, resize/reveal preservation, marker correspondence,
initial label wrapping and fitting code indentation. It establishes no
additional concrete defect in that changed scope.

The full historical findings and final classifications remain in
[the independent comparison](runner-intent-review.md). F2/F8 retain cooperative
and process-evidence limits, not missing built-in preparation or current live
caller linkage. Independent review tasks are completed as review work; their
completion is not acceptance or a successful global compliance verdict.

**Acceptance remains unclaimed.** F4 requires explicit human arbitration of the
root protected-artifact/shell rules versus the local writable-workspace shell.
The maerg's protected task broker is unchanged. F9 concerns the older runtime
trajectory replay API, not the new runner journal/history format; it remains a
separate older-scope discrepancy. C4 concerns the older runtime's unsafe signal
installation versus its unsafe prohibition. Toolchain interpreter probes do not
qualify every companion package/build tool. None of these limitations is
silently waived.

The unconditional width guarantee and exact preservation of indentation wider
than the viewport also conflict. Current wrapping preserves that indentation;
this boundary needs explicit policy arbitration, not a concealed intent
rewrite. Physical font/glyph behavior, block-atomic eviction and aggregate
conversation/hidden-history/journal quotas remain unqualified. The narrow,
named fixes do not certify the remainder of the older UI/runtime.

Follow-up queue entries were registered during remediation. Their timestamps
record bookkeeping, not independently authenticated test-before-code history
or formal acceptance receipts.
