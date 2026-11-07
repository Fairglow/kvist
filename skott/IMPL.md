<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Bounded inspection recovery observations - 2026-11-07

`tool-recovery-source-observer` independently inspected current
`src/file_tools.rs`, `src/tools.rs`, `src/run.rs` and
`tests/native_file_tools.rs`, excluding intent, queue, previous records/reviews,
chat and Git history. It ran no tests and changed no files.
It observed deterministic descriptor-relative traversal that counts and skips
enumerated symlinks and special entries without opening those classified entries.
Explicit special search scopes are rejected before opening; ordinary files are
opened nonblocking/no-follow and descriptor-checked. Unreadable ordinary files
and detected unsupported substitutions still fail.

Search admits whole files up to the remaining 8 MiB budget, including an exact
fit. The next otherwise eligible file that cannot fit stops recursive traversal.
Collected matches survive with `scan_limit_reached`, actual charged bytes,
incomplete coverage and narrowing guidance. Sorted result pagination describes
collected matches, not unscanned files; calls rescan rather than retain snapshots.
Binary bytes consume budget; oversized and literal-filtered files do not.
Existing file, depth, entry, matching-line and encoded-output bounds remain.
Hard traversal/match errors include narrowing instructions.

Model instructions distinguish recoverable inspection/toolchain limits from
security denials, continue safe independent work, and explain concrete blockers.
User-requested manifest edits are no longer categorically prohibited; acquisition
and lock regeneration remain explicit host operations, with vendor/check/restart
before prepared offline/locked verification. Prompt assertions establish text,
not actual model compliance.
Separate `prerequisite-source-observer` inspected only `src/error.rs`, without
intent/history and without executing tests. It observed preservation of the
specific prerequisite cause/remedy, an affected-operation retry restriction and
explicit permission to continue other supported work, replacing the unrelated
installation-repair prescription. Its assertions likewise verify presentation,
not dispatch or model behavior. Neither observer established a deterministic
high-confidence defect or certified compliance.

The main observer retained filesystem race limits: pathname classifications can
race with opens, regular-to-regular replacement is not identity-checked, and
same-size content changes evade size checks. A racing ordinary-to-special
substitution may be opened before descriptor rejection. Classified-special skip
behavior is not a race-proof guarantee; no sandbox escape was demonstrated.

Implementer-attributed execution: five new/changed regressions failed before
production repair in nextest `a4fb732b-1f07-4e66-bc72-0e93cc3a12ca`.
The supplementary prerequisite-message regression independently failed before
its production repair in `6100a788-4e82-48cd-b50d-b7d29928e989`.
Final nextest `f0a3ccc3-e546-4629-97eb-ec683be1a973` passed all 67 selected
native-file, incremental mutation, worker/registry and error tests, including
actual SandboxExecutor special-entry/partial-search/narrowed-recovery trials.
Strict Skott all-target Clippy, formatting, release binaries and whitespace
checks passed. Component intent structurally validates; acceptance is unchanged.

Read-only helper checks on the actual checkout reproduced the recorded discovery
and search argument shapes: discovery skipped one special entry and returned
without failure; broad search retained partial results after 8,265,444 charged
bytes. Narrowing to `maerg/src/bark` with literal `.rs` filtering found
`tui/workspace/view.rs:133` with complete coverage and 497,748 charged bytes.
These checks used host namespace paths, not a claimed whole-checkout sandbox
trial. The original session ended cancelled after five read-only calls, three
unsuccessful; no shell/mutation calls or completed fix were recorded.
Its stale vendored lock state remains a real build prerequisite: a read-only
check still fails. No dependency provisioning, original answer-input change,
automatic prompt replay, acceptance or blanket security certification occurred.

Separate `tool-recovery-source-blind-comparator` compared scoped intent with
these observations and attributed evidence without Rust/source access.
It reported correspondence for classified-special handling, exact/overflow
whole-file budgets, truthful collected-result pagination, retained bounds and
operation-specific authoring/verification guidance. It retained the unconditional
intent wording "without opening" versus a racing ordinary-to-special substitution
as an explicit arbitration item; intent was not rewritten to conceal that limit.
It also retained same-size/replacement races, cooperative execution, finite
field-invariant evidence and the lack of real-model qualification. Its initial
oversized IMPL read exposed older observation sections, which it excluded from
its comparison; strict record-scope isolation was therefore not achieved.
No acceptance or compliance certification is implied.

Native `component validate skott` and `doctor` validate this component's intent
and queue. `task next skott` remains blocked by pre-existing missing structural
sections in the separate dependencies component, not by this recovery change.
Release binaries were rebuilt at `/opt/target/release/skott` and
`/opt/target/release/skott-file-tool`; the separate user-installed launchers and
running sessions were not replaced or stopped.

## Final native Rust support observations - 2026-11-06

`final-rust-impl-observer` independently read source/test assertions without
intent, queues, prior records/reviews or history and executed no tests.
Installed-only preparation exposes the selected and additional installations
read-only with private rustup state, explicit selection, offline/locked Cargo,
validated source replacement and disabled automatic installation. Vendor
material is copied outside the writable checkout and checked against managed
lock/manifest/resolver controls; running sessions retain this immutable copy.
Actual Rust startup timeout diagnostics now state its 180-second budget,
separately from request preflight.

Native assertions cover all installed compiler/target selections, default
switching, linking/execution, unit/integration/doctests, formatting, Clippy,
documentation, missing/stale locks, offline Git dependencies, denied mutations
and unchanged host inventory. Environment-specific trial prerequisites and
existing skip branches remain; this does not promise arbitrary system libraries,
credentials, source layouts or toolchain installation.

The observer identified enumeration-only aggregate size accounting as unsafe
against subsequent file growth. `snapshot-bound-amendment` independently observed
the repair's enumeration fingerprints, pre-copy identity check and exact final
length/identity checks. `final-copy-byte-observer` then observed the additional
per-chunk byte-count guard before writing, so partial destinations cannot exceed
the matched pre-copy length either. The fingerprint includes file length; these
metadata checks are not enumeration-time content hashes or an atomic filesystem
snapshot. The growth regression failed before repair and later passed; concurrent
mid-copy growth was not executed as a separate adversarial trial.

Implementer-attributed execution: after the unrelated lock update, root vendor
state was stale and refreshed explicitly using existing Cargo cache only.
One combined run passed 387/388 cases but timed out at Rust startup during
competing builds. A later diagnostic real-checkout trial passed with enumeration
23402 ms, copy completion 80456 ms and complete Rust preparation 81342 ms:
21551 entries, 18483 files, 3069 directories and 376894920 content bytes.
Eight installed toolchains were exposed. This is measured finite execution,
not reproduced cold-cache contention or guaranteed immunity to arbitrary I/O.
Final nextest `2e9bc4ba-255b-4878-b85f-aafe0ae1e259` passed all 389 affected cases,
including the full real-checkout Git/list/Cargo trial. After the final write
guard, `f41a0ae3-0673-4c4f-90da-ff2c9c6d4b52` passed all 15 Rust-environment units;
strict workspace lint, formatting and release builds passed.
No acceptance, automatic replay or blanket security certification is implied.

Separate source-blind `rust-source-blind-comparator` found correspondence for
installed Rust tooling, immutable offline consumption, repaired copy bounds and
180-second diagnostics. It retained the older requirement's “comfortably within
30 seconds on representative hardware” wording as an unresolved intent/performance
discrepancy: the measured shared-host startup exceeded 30 seconds, and hardware
representativeness was not established. The 180-second cooperative deadline is
not a claim that the older performance target was achieved. Source/intent sides
were not changed to conceal that finding; human arbitration remains required.
The review also retained native prerequisites, filesystem race limits and the
lack of a separate mid-copy growth adversarial execution.

Subsequent production-runner nextest
`5c1a5560-129c-461a-b27b-4c29eac74ef1` reran and passed all 389 affected cases
after the final byte guard and queue updates, closing that specific reported
rerun gap. Release Skott and its file helper were atomically installed with
byte/version verification; installed full vendor readiness passed. Running
processes were left untouched and existing sessions still require restart.
The broader intent/performance discrepancies above remain unresolved.

## Independent preflight-budget observations - 2026-11-06

Fresh `scope-budget-source-observer` inspected only current scoped sandbox,
executor and test source. It excluded all intent, queues, prior records/reviews,
chat and Git history and peer implementations; it made no edits, delegated
nothing and executed no tests. The following is its source-derived supplement,
not acceptance or certification.

Request construction shares one cooperative deadline across identity hashing,
workspace inspection, Rust-resource validation, context hashing and request
validation: `min(wall_time_ms, 120 seconds)`. Stage transitions preserve the
overall start. Budget errors include stage, limit, total elapsed time and stage
elapsed time (`src/sandbox.rs:125-295,326-377`).

Identity reads check cancellation/deadline between 64-KiB chunks and reject files
initially exceeding, or growing beyond, 256 MiB (`src/sandbox.rs:400-455`).
Workspace inspection enumerates hidden directories without exclusion filters,
checks symlinks and deduplicates scheduled directory paths. Aggregate limits are
one million entries, depth 128 and 32 MiB of charged directory-path storage
(`src/sandbox.rs:470-504`; `src/sandbox/scope.rs:166-265`).
Eight scoped workers share scheduling/accounting. Completion requires no queued
or active directories; exits notify peers, failures/panics stop inspection, and
started workers are joined (`src/sandbox/scope.rs:19-82,114-163,269-283`).

The executor stages payloads before construction and reaches runner dispatch
only after successful preflight and cancellation checks. Logging records stage
starts, scan counts/timing, request metadata and runner outcome metadata rather
than command/output payloads in these statements
(`src/executor.rs:108-173`; `src/sandbox.rs:297-322,595-655`).

Read, not executed, assertions cover shared/capped deadlines, synthetic
45-second elapsed preflight, hidden-escape detection, quota boundaries,
alias/cycle deduplication, concurrent inspection and cancellation/panic handling
(`src/sandbox.rs:801-1005`; `src/sandbox/scope.rs:291-449`).
Diagnostic assertions require chain markers and exclude selected
command/output/environment-value sentinels
(`tests/rust_build_environment.rs:311-387`).
The repository trial asserts successful shell inspection, native root listing
containing `Cargo.toml`, and nested repository-test output containing
`test result: ok` (`tests/rust_build_environment.rs:392-446`).
Request assertions cover authoring/network configuration, internal versus
escaping links, context confinement and missing-runner fail-closed errors
(`tests/component_tests.rs:668-756,813-935`).

The observer identified no high-confidence scoped correctness/security defect.
Filesystem calls are not preemptible, so worker joining can exceed the nominal
budget. Preparation and execution have separate budgets. Inspection is not a
filesystem snapshot or concurrent-mutation defense; path charging is not an
exact heap ceiling. Uninspected callees, actual execution and cold-cache
performance were not verified by this observer.

### Attributed execution evidence

The implementing parent recorded three failing regressions before production
changes, then 18 passing sandbox units. Nextest run
`30d33440-cc1b-4822-94dc-5a258ade31bb` passed all 344 selected tests; 15 unselected
tests were outside that run. Selection covered Skott units, component/native
file-tool callers, the real repository trial and diagnostic payload exclusion.
The actual checkout's initial Git inspection, subsequent native root listing and
existing nested offline Cargo trial all completed under Galla/Bubblewrap.
Those three warm scans each covered 225862 entries and 9890 directories in
101, 64 and 82 ms respectively (total preflight 122, 94 and 98 ms).
Scoped strict Clippy, formatting, whitespace checks and the release binary build
passed. These results are attributed parent execution, not independently
re-executed by the observer.

The original private journal establishes zero executed tools and a
`workspace_scope` preflight timeout at 30029 ms, not a Git failure or established
OOM. A bounded sequential directory census took 21.057 seconds before the warm
native trial. Exact historical contention/cold-cache causation was not
reproduced or proven; no global cache flushing or host authority expansion was
used.

Nextest run `27d66dd7-3ae1-4054-95f5-37ca97ee2bb0` repeated the actual repository
trial five times with the configured `/opt/target/release/galla-runner`; all five
iterations passed. At 2026-11-06T22:16:55Z the parent atomically replaced only the
installed `/home/stefan/.kvist/bin/skott` from the repaired release build,
verified byte equality and ran its version command successfully. Running
processes were not stopped; existing sessions require restart to use the repair.
No dependencies, toolchains or enforcement grants were provisioned or broadened.

### Independent source-blind comparison

Separate `scope-budget-source-blind-comparator` read only the scoped local
requirements, contract and design and this fresh observation/evidence section,
not source/tests, queues, prior records/reviews, peer documents or history.
It established no concrete scoped contradiction: the record represents full
hidden/generated traversal, aggregate quotas, bounded concurrency,
fail-before-dispatch behavior, cancellation, owned joins and the shared
`min(wall_time_ms, 120 seconds)` limit preserving smaller budgets.

It retained evidence limits: smaller-budget/exact-boundary executions and fresh
escaping-link detection were not individually itemized in the execution account;
absence of caching, replay and host fallback was not explicitly established by
the observation. Synthetic 45-second elapsed preparation is not a prolonged
physical filesystem scan. Selected passing tests do not independently establish
every obligation, and 15 tests were unselected. The original-journal account
establishes zero executed tools and a `workspace_scope` timeout, not Git failure,
OOM or cold-cache causation. Census/warm scans do not reproduce historical
contention. Cooperative calls/mandatory joins can exceed the nominal budget,
consistently with the stated limitation.

This comparison is attributed, bounded and not certification, acceptance or a
review receipt. Its limitations remain recorded for human arbitration. The
subsequent five-iteration production-runner repetition and installed-binary
replacement above are parent execution, not independently reviewed by this
comparator.

## Independent shared dependency observations - 2026-11-06

Fresh `offline-dependency-source-observer` read only scoped current source/test
assertions and explicitly referenced shared providers, excluding intent,
queues, prior records/reviews and history. It changed/delegated nothing, ran
no tests, and established no reportable scoped security vulnerability in its
initial read or two selective corrective follow-ups. This is not acceptance.

Current selective reads show complete routing for the previously omitted recovery fence and resolver directory, shared validation against the private copied vendor tree, and structurally retargeted read-only runtime resolver staging. Installed-Cargo failures retain selected-toolchain context, and lock-access failures provide dependency-prerequisite guidance. Earlier private-snapshot, fixed-grant, offline/locked wrapper, and no-installation observations are carried forward—not independently re-established here. Namespace enforcement and comprehensive adversarial ancestor-race resistance remain caller/backend boundaries rather than proven consequences of these corrections. Sources: `/opt/proj/kvist/skott/src/rust_environment.rs:726–773`; `/opt/proj/kvist/skott/src/rust_environment/host.rs:47–88`; `/opt/proj/kvist/skott/src/dependencies.rs:30–73`.

The initial read observed separate vendor/check/offline flags conflicting with
model/session options and early dispatch before config/model/UI initialization
(logging initializes first). Check is shared filesystem-only readiness without
installed-toolchain resolution. Provisioning resolves installed Cargo, including
for reusable/local-only state; its preliminary metadata check is not a full
lock parse before that selection.

Private vendor copying precedes control checks and uses read-only files outside
the writable workspace, with fixed read-only `/rust/vendor` grants. The first
catalogue-only/control-bypass discrepancy was observed replaced by common
snapshot/manifest/fence/resolver validation; the final follow-up observed
recovery-only and empty resolver directories entering that gate. New assertions
exercise these states, missing-lock guidance and real offline local-Git
test/Clippy/documentation consumption. Existing assertions cover original-alias
masking, no credential mounts, immutable-copy behavior and unchanged locks.
They were read, not executed, by this observer.

Checksum consistency is not cryptographic origin proof. Full ancestor-race
resistance and namespace enforcement were not proven by source inspection.
The earlier out-of-scope preparation-budget observation remains attributed:
the constant is 180 seconds while existing text refers to 30 seconds.
Parent-run test results remain separate evidence.

## Scoped standalone observations - 2026-11-06

The independent `standalone-source-observer` security-review context read only
the scoped current Skott source/tests. It excluded intent, queues, prior
records/reviews, architecture/root/vision, history/diffs and peer
implementations, made no edits and ran no tests. It established no
high-confidence security vulnerability or must-fix bug in that scope.
The following supplement is its source-derived observation, not certification.

### Configuration initialization

Sources: `src/config_init.rs:49-135`; `src/main.rs:28-54`.

- `create(path, project, model, registry)` requires an existing project directory
  and destination parent.
- CLI initialization precedes normal configuration discovery; the default
  destination is `skott.toml` under `--cwd` or the launch directory.
- Without a selected model, output contains schema version 1, the default
  provider and commented guidance; no workspace is pinned.
- With a model, output references one native shared profile and its canonical
  registry source, without copying model facts.
- An explicit registry without a model is rejected before staging.
- Publication uses a same-parent temporary file, file synchronization and
  `persist_noclobber`; existing destinations are not replaced.
- Directory synchronization follows publication: its failure returns an error
  after the file has been created.

### Discovery, loading and validation

Sources: `src/cli.rs:182-278`; `src/config.rs:490-728,1027-1113`.

- Discovery order is explicit path, launch-directory configuration, XDG user
  configuration, then XDG system locations; no ancestor-directory search occurs.
- Explicit paths are returned without existence checks. Discovery uses
  `is_file`; loading performs separate descriptor checks.
- Relative `XDG_CONFIG_HOME` falls back to `HOME/.config`; system-directory
  entries are not similarly restricted to absolute paths.
- Reads use `O_NOFOLLOW | O_NONBLOCK`, require a regular descriptor, enforce
  64 KiB metadata and read bounds, and require UTF-8.
- `O_NOFOLLOW` protects the final component; this reader does not prohibit
  symlink ancestors.
- Parsing failures report a line number without reproducing configuration
  contents; schema and configuration invariants are subsequently validated.
- Registry overrides take precedence over configured paths; configured relative
  registry paths resolve beside the configuration.
- Omitted working directories use the process launch directory. Normal
  discovery does not use the CLI `--cwd` override.

### FileRequest namespace and roots

Source: `src/file_tools.rs:198-399,447-460`.

- Requests use closed typed arguments and bounded lexical absolute paths; dot
  components, repeated separators, trailing separators except `/`, and NUL are
  rejected.
- Mutations must target descendants strictly inside the write root;
  sibling-prefix paths and the root itself are rejected.
- `host_mapped` canonicalizes the selected workspace; `sandbox_mapped` requires
  the prepared namespace to match the request namespace.
- Exact-root paths map directly to the host root; descendant paths retain their
  relative suffix. Mapping revalidates the resulting request.
- Read operations outside the namespace remain unchanged. These helpers are
  not authorization or host-shell confinement mechanisms.

### Prepared sandbox observation

Sources: `src/executor.rs:58-90,283-336`;
`src/executor/progress.rs:95-163`.

- Observation requires exactly one writable authoring grant whose destination
  equals the prepared working directory, and maps through that grant's source.
- Preparation and pre-observation failures prevent the execution callback;
  post-observation failures preserve the outcome but reject progress evidence.
- Evidence compares before/after revisions, is paired with the complete
  intent/outcome identity, consumed once and capped at 128 pending observations.
- Indistinguishable pending executions receive ambiguity errors rather than
  interchangeable evidence.
- Payload staging uses an external workspace-parent directory with explicit
  directory/file modes of `0700`/`0600` and RAII cleanup; canonical `/` cannot
  provide that staging parent.

### Test assertions and limitations

- `tests/shared_registry.rs:17-234` asserts creation guidance, private starter
  permissions, no-clobber cases, incompatible modes, profile references and
  rejected selections.
- `tests/native_file_tools.rs:35-111` asserts exact-root/nested mapping and
  ordinary-project sandbox discovery; `src/executor.rs:424-545` uses callbacks
  to assert prepared-grant observation and rebinding rejection.
- `src/executor/progress.rs:194-605` asserts revision evidence, ambiguity,
  cancellation and observation failures. These are not runtime-wide isolation
  proofs.
- Tests were read, not run by the observer. Excluded registry,
  revision-observer and sandbox implementations were not inspected.

Parent-executed evidence, separate from the independent source observation:
four initial bootstrap regressions and three exact-root regressions failed
before production changes; the native case reproduced the reported error.
The final selected run passed 391 tests: 257 library, 40 component, 5 headless,
3 input-boundary, 17 model-selection, 41 native-file, 15 shared-registry and
13 startup-lifecycle tests, with no failures/ignores. This includes actual
installed sandbox root list/find/search in a temporary non-Kvist project,
cross-project profile references and user-wide discovery.
`cargo clippy --offline --locked -p skott --all-targets -- -D warnings`,
format checking and `git diff --check` passed. No live model inference or
exhaustive filesystem race testing was performed. No acceptance, receipt or
runtime-wide compliance claim follows from these results.
The updated component intent passed `component validate skott --json`.
Read-only `task next` inspection was blocked by the enclosing project's
`invalid` current-state gate; no acceptance or revision reset was performed.

### Separate source-blind comparison

`standalone-source-blind-comparator` read only the standalone requirement,
contract, design and this scoped observation/evidence section; no source,
executable tests, queue, history/diffs or other records were inspected.
It established no material scoped contradiction, not certification or acceptance.
The observations support early explicit initialization, unpinned workspaces,
profile references, no-clobber publication, exact-root revalidation,
prepared-grant mapping and strict mutation boundaries. A starter default
provider does not establish an invented model.

The comparator retained detail gaps: relative `--config` semantics,
fallback/provider correspondence, exact absent-file guidance, offline/no-provider/
no-tool/no-UI guarantees and omission of sandbox/tool-policy overrides are not
all explicit in the independent observation bullets. Permission and rejected
selection assertions, the 391-test run and lint/format results are attributed
evidence, not independently executed verification. Parent-directory sync can
fail after publication; this is a recovery caveat, not an established
atomicity/no-clobber contradiction. Excluded dependencies, filesystem races,
runtime-wide isolation and live inference remain outside this comparison.
These limitations are retained for human arbitration rather than changing
intent or representing an acceptance receipt.

## Scoped operational observations - resilience phase 4

### Final direct-error and fixture boundary observation

Fresh `skott-p4-final-boundary-observer` independently inspected only local
executor/error/operational source and the relevant test/helper paths, excluding
intent, queues, records/reviews, history and peers. It made no edits/delegation.
The direct executor preserves sandbox-unavailable/build and toolchain-unavailable
domains; native tests still require prerequisite/not-executed/fix-prerequisite.
There is no local host fallback. Direct cooperative cancellation checks retain
the existing sandbox-execution error/exit 1, distinct from explicit cancelled/
exit 130. Imported native adaptation is tested, not source-derived here.

Prepared observation tests bind the original directory, reject a later symlink
before the execution closure and attribute no unrelated rebound change.
Executed operations retain malformed completed reads/results, fence a second
dispatch after notice-record failure and preserve all five resource identities.
Synthetic post-dispatch failure retains possible effects/reconciliation, not a
fabricated not-executed result.

The PTY fixture copies the launcher, asserts byte identity/executable mode and
owns its directory/workspace through RAII. Ambient runner `TMPDIR` is unset;
child scratch is explicitly fixture-owned. The launcher executes a fixture
script, not the requested sandbox command. Actual PTY/error/quit/restoration
assertions pass; the source and current directories share a device, so this is
not an empirical cross-filesystem trial. Copy removes the hard-link dependency;
Unix PTYs, executable mounts and system utilities remain prerequisites.

Independent offline/locked executions pass one direct-domain, seven operations,
four executor, one terminal-error and one actual PTY case (14 distinct).
No concrete local bug is established. Staging still lacks a local byte cap/
interruptible write or fsync; field rendering is bounded, not whole-message
secrecy. Tests do not prove imported sandbox/detector behavior, rollback,
hostile-race resistance, durable disk recording or exhaustive replay counts.
A separate narrow security reviewer reports no vulnerabilities.

Parent broad serial execution first reproduced the wrong direct missing-runner
domain and PTY hard-link cross-device failure. The implementing-context index
attributes preservation of the domain and portable executable copying, 65
selected passes and unchanged strict checks. Final parent full Sav/Skott
execution, with ordinary ambient `TMPDIR`, passes 876 tests with three existing
ignores. This follows rather than replaces the earlier 435 targeted evidence.

Separate `skott-p4-final-source-blind` directly reads only requirements
262-282, contract 110-145, design 56-89, and this phase-four observation, plus
the scoped task projection/reported execution. It establishes no concrete
contradiction, retaining direct-cancellation, copy-versus-cross-device-trial,
imported behavior, staging/blocking/content and partial-assertion limits.
Its contract excerpt ends mid-sentence and it claims nothing about the omitted
continuation; the earlier operational comparison remains separate evidence.
It performs no searches/source/tests/execution/delegation and certifies nothing.

Fresh `skott-p4-source-observer` read only local operational, CLI, executor,
host/external, session/recording, worker/TUI and relevant test source, excluding
intent, queues, records/reviews, history and peer internals. It made no edits,
did not delegate and did not certify implementation.

Operational rendering preserves typed category, effect certainty, recovery
and stopping-resource metadata. Diagnostic fields are UTF-8 bounded to 4096
bytes before/after control escaping, plus a truncation marker. Failed tools
render a cross, not a success checkmark; terminal events clear working state.
Headless plain output exposes cause/next action on stderr; JSON retains events.
Cancellation denies rollback, and failure guidance requires reconciliation
instead of blind replay. Exact tests establish process-exit/prerequisite
categories, not exhaustive backend classification.

Private journal files are descriptor-held, no-follow, exclusive mode 0600.
Dispatch and typed failure records synchronize; finish records retain stop,
counts, failure and usage completeness. Base dispatch/results omit argument
values/raw output, retaining shape/hashes and outcome metadata. This is not
universal redaction: readable/structured transcripts contain content, live
records include diagnostics, external journals retain text/raw hex. Printable
helpers escape/bound output rather than recognizing arbitrary secrets.

The transcript retains the observed summary before required finish writes.
Headless error handling preserves turn/tool counts and original stopping
metadata while displaying the returned failure. Executed saturation establishes
durable cancellation plus a later delivery-failure presentation, not exhaustive
first-cause precedence inside the imported runtime.

Worker delivery uses a 128-event synchronous channel with 10-ms full-send
polling. Prompt cancellation releases blocked producers without shutdown or
draining. Failed cancelled sends return typed cancellation rather than success.
A separate handoff retains at most one terminal/progress event per kind, four
events total, drained after queued events. This bounds event counts, not all
queued bytes. Shutdown/disconnection reports channel closure.

Prompt-local read cursors retain at most 4096 paths. Failed/malformed pages
do not advance them; completed executor outcomes return before validation
failure is exposed through progress. Tests preserve stdout/stderr, recorded
outcomes, completed counts and prevent another model request. Detailed native
revision and sandbox enforcement implementations were outside this scope.

The native generation exception reaches TUI/headless construction. Tests
retain default detection, explicit opt-out, token limits and effect-repeat
protection. Long explicit-host shell scripts remain literal staged bytes;
only the generated wrapper is translated and the path is quoted through `$1`.
RAII staging is outside the canonical workspace. Real subprocess assertions
cover quoted parent paths, heredocs, status/stderr and cleanup after ordinary
failure, supervision error and started-process cancellation. Intentional shell
semantics are not disabled by literal staging.

External dispatch requires explicit host acknowledgement and announces opaque
native detection/steering, no rollback/replay. Local policy selects zero retries
and disabled native loop detection; nonterminal/headless external execution is
refused. Imported supervision/mount internals were not inspected.

Independent offline/locked execution passes 21 new cases: five library, six
loop, four operational CLI, two external, two PTY and two subprocess tests.
Fifteen cursor/journal/terminal/control units and one external cancellation
test also pass (37 distinct tests). Selected exact queue/content/count tests are
strong; other checks use non-success or substring assertions rather than
exhaustive cross-record consistency.

The implementer-attributed private execution index reports 435 serial passes,
strict lint/formatting and genuine diagnostic/backpressure/read/headless/CLI/
external/staged-script reds. Executor uncertainty became green from shared
changes, not a local red. Mid-edit API and fixture mistakes are excluded.
Two existing parallel IPC/startup timeouts passed serially with unchanged
deadlines and no skips; they were not independently reproduced here.
A separate scoped security reviewer found no vulnerabilities. No concrete new
defect was established by this source observation; no acceptance is implied.

The separate exact-range `skott-p4-source-blind-comparator` inspected only
scoped intent/queues, this observation and Sav's public operational API, with
no searches, source/tests or execution. It established no concrete scoped
contradiction, retaining content/redaction, count-versus-byte, imported/cooperative
and partial-assertion limits. It did not infer staged-script privacy from
journal permissions; that specific permission evidence gap has a bounded
follow-up. Protected repair received no credit. No certification is claimed.

The bounded follow-up strengthened actual staged-script assertions without
changing production: implementing-context provenance identifies `stage_bytes`
setting directory 0700 and payload 0600 before writing. Parent independently
reran two real host-script cases plus the private-outside-workspace staging unit;
all three pass. Assertions cover actual modes, non-links, literal bytes and
exact owned cleanup. This supplies the missing permission evidence, not a
privacy claim inferred from journal files or blanket certification.

## Scoped progress observations - resilience phase 3

Fresh `skott-p3-final-observation` read local executor, native progress/revision,
host, session/read-cursor source and native/loop test assertions. It excluded
intent, queues, previous records/reviews, peers and history, changed nothing and
did not delegate. These are source-derived facts, not certification.

Native mutation progress compares missing/present complete bounded SHA-256
revisions around writes, edits, appends and patches. Empty-file creation and
changed bytes count; identical bytes, inode-only replacement and successful
shell claims do not. Measured changes can remain evidence with a failed tool
outcome. Pending identity includes complete serialized intent and length-framed
status/output. Evidence is single-consumption with at most 128 pending entries.
All value-identical unconsumed entries are marked ambiguous and retrieval fails
explicitly rather than borrowing another execution's change; later fresh
executions work after consuming those errors.

Host observation uses the same mapped prepared helper payload as execution.
The observer holds the root descriptor, compares device/inode with the current
pathname, and traverses descriptor-relative no-follow paths. Detected ordinary
or symlink root replacement fails with reconciliation guidance. Revision reads
poll cancellation before/after bounded 8192-byte chunks and observations.
Pre-observation failure prevents execution. Post-observation failure preserves
the original outcome but refuses evidence; completed effects are not replayed.
Cancellation is cooperative, not asynchronous interruption of a kernel call.

Prompt-local read cursors validate page JSON, metadata, digest and requested
boundaries. Only a changed prior validated same-path digest counts as state
change; first/additional unchanged pages do not. Omitted offsets use prior
continuation and digest; explicit offsets override. Failed/malformed pages do
not advance state. Retention is bounded to 4096 paths and reset for a new run.
Executed tests retain 540-page investigations, changed-state verification and
fair duplicate-batch feedback while stopping unchanged terminal reads/no-ops.

No atomic external-writer transaction or causal attribution is established.
Descendant namespaces may be reopened independently, and move-and-restore
races between checks are not comprehensively detected. Executor-level errors
can bypass post-observation after possible spawning; malformed successful reads
lose their outcome at the bridge boundary. These error/effect-reporting limits
are operational-phase work. Sandbox selection was independently mapped after
request preparation, an additional preparation mismatch assigned a corrective
follow-up before this phase can close.

Independently executed offline/locked, with `TMPDIR="$PWD"`: 19 selected library
tests, 57 loop integration, 39 native-file and eight incremental-file tests
passed (123 total). An unnecessarily broader library/integration rerun also
passed 248 + 104 tests; incremental/generation test source was not inspected.
Imported shared fairness/detection mechanics and sandbox backend internals were
not source-inspected.

Implementer-attributed corrections reproduced four identity/rebinding failures,
four cancellation failures and one prepared-host scope mismatch before fixes.
The same 123 tests plus strict all-target lint and formatting then passed.
Seven corrective logs were byte-verified into private
`skott-p3-host-20261106-followup.tar.gz`; owned scratch leaves were removed while
unrelated recovery data was preserved. A separate scoped security reviewer
found no vulnerabilities in these changed progress paths, not a blanket claim.

### Final prepared sandbox scope observation

Fresh source-only `skott-p3-sandbox-observer` read only the prepared sandbox
mapping and local progress/helper source, excluding all intent, records,
reviews, peer internals and history. It observed that revision inspection now
maps through the prepared request's sole writable authoring grant, checks
namespace correspondence, and executes that same request without
recanonicalizing the original selection. Prepared-source traversal rejects
symlink components; observation pins the root inode and checks its binding.

Its independent three `sandbox_prepared_scope_` cases establish that changing
the original grant earns state-change evidence while the rebound selection
stays untouched, changing only the rebound selection earns no evidence, and
prepared-source symlink replacement fails before effects with reconciliation
guidance. Twelve executor-progress cases also pass (15 total). These tests use
closures, not a real sandbox. Imported request/mount enforcement was not
inspected; pathname preparation and observation are not an atomic namespace
transaction. No concrete new contradiction was established within this scope.

The implementer first reproduced all three prepared-scope failures, then
passed 62 selected tests plus strict lint/formatting. Four logs were byte-verified
into private `skott-p3-sandbox-20261106.tar.gz` and exact owned leaves cleaned.
A separate narrow prepared-grant security audit found no vulnerabilities,
retaining the non-atomic namespace and uninspected-runner limits.

The first source-blind comparison exposed unrelated snippets during locator
search and is provisional, not qualifying fresh-scope evidence. Fresh
`skott-p3-exact-range-comparator` used only explicit allowed intent/queue,
provider-contract and observation ranges with no searches, source/tests or
execution. It established no concrete phase-three contradiction, preserving
imported-mechanism, overlapping execution-count, closure-based sandbox and
non-atomic namespace limits. Operational error/presentation/teardown gaps
remain phase-four obligations. This comparison is not acceptance/certification.

## Scoped generation adapter observations - resilience phase 2

Clean-slate `skott-generation-source-observer` read local source and integration
tests only, excluding intent, queues, records/reviews, Sav internals and history.
It changed nothing and ran no checks.

`Skott::with_retry` defaults generation detection on; its explicit builder
changes only that private boolean. `run_controlled` builds a shared native
runtime, forwards validated limits and that boolean, and passes session,
transport, parent cancellation, context, mailbox and optional recorder through
static bridges. Skott implements no separate detector or correction machine.
Sink events and recorder request/event/turn/usage/control/effect callbacks are
forwarded without rewriting. Recorder trait defaults do not guarantee durable
capture; concrete private journals and transcripts own that responsibility.
Executor read cursors and tool resolution are unaffected by the exception.

Three inspected generation tests assert two corrections followed by completion
without discarded effects, persistent repetition with an unsuccessful summary,
and one-request acceptance under the explicit intentional-output exception.
Other assertions cover compaction and uncapped productive runs. The script
fixtures are not live-provider qualification; their count-based pairing helper
does not prove call-ID correspondence. The exception-only test contains no tools.
No high-confidence generation-specific adapter defect was established; shared
detector semantics were outside the observer's context.

Runner-attributed execution: all 50 integration tests and strict all-target
Clippy pass after shared accounting corrections. A separate scoped security
observer reported no vulnerabilities. Neither observer independently executed
these checks or certified compliance.

Separate source-blind `skott-p2-intent-comparator` found no established adapter
contradiction, but noted missing assertion-level proof for ID pairing,
retained tool protection under the exception and both frontend entrypoints.
Shared detector behavior was outside its evidence. An overbroad initial IMPL
read exposed older sections; the comparator disclosed and excluded them from
reasoning, read no source/tests and ran no checks.

The full suite then exposed an unrelated-purpose fixture now caught by the
detector: `FloodTransport` concatenated identical unbroken provisional text and
stopped before filling its 128-slot queue. Counter-bearing novel text preserves
the actual backpressure/teardown threshold without disabling detection or
relaxing the assertion. This fixture correction is runner-attributed.

## Hidden runtime-state scope regression - 2026-11-05

Independent clean-slate `repair-source-observer` read only bounded source/test
ranges, excluding intent, queues, prior records/reviews, chat and Git history.
It edited nothing and executed no checks.

Eight joined scoped workers share directory scheduling, visited paths, quotas
and stop state. Directory I/O occurs outside the queue lock. Spawn failures,
panics and poisoned state become errors; preflight checks recur while scanning.
Traversal has no hidden-name exemption: it schedules ordinary directories,
checks lexical and canonical link confinement, and schedules directory targets.
The hidden-directory test asserts rejection of escaping links beneath both
`.generated/deep` and `.kvist/deep`. These are inspected assertions, not executed
evidence. Scheduling is nondeterministic, and pathname inspection does not
freeze concurrent filesystem mutations.

Attributed implementer evidence: the strengthened regression failed against the
uncommitted `.kvist` exemption and passed after its removal. All 202 selected
sandbox/vendor, dogfood, viewer and terminal-host tests passed. The checkout's
stale generated toolchain links were preserved outside the workspace; Maerg's
separate runtime-staging repair prevents persistent recreation, rather than
relaxing this component's traversal.

Separate source-blind `repair-intent-comparator` found the observed traversal
and hidden-path assertions consistent with scope intent. Actual-checkout success
and scheduling remained unverified at comparison time. It disclosed incidental
prior-record snippets from a text lookup, excluded them, and read no source/tests.
Execution remains implementer-attributed; this diagnostic comparison is not
acceptance or certification.

Final implementer verification: all-feature workspace Nextest run
`e88356b1-350a-460b-8410-a15d620d2c2c` passed all 2,330 tests with 13 existing
skips. The actual-checkout native sandbox trial passed in 85.810 seconds with
complete `.kvist` inspection restored. Strict workspace Clippy, formatting,
doctests and structural component validation passed; scope quotas, production
deadlines and scheduling were unchanged.

## External CLI fixture isolation (2026-10-29)

Independent clean-slate `external-fixture-observer` read only the external-only
CLI test function, excluding intent, queues, prior records/reviews, runtime and
peer source, execution logs and chat/Git history. It edited nothing and ran no
checks. Its source-derived observations are:

- The fixture writes an explicit `copilot` external-agent registry and a Skott
  configuration naming native default provider `ollama`.
- Listing and both refused-execution children receive the fixture directory as
  working directory and `HOME`, and its `user-config` path as `XDG_CONFIG_HOME`.
- Listing must succeed, report no native models and exactly one external-agent
  entry. Both headless and host-execution flag attempts must fail with an
  interactive-terminal diagnostic, without creating `.skott`.
- These are inspected assertions, not observed executions. The no-effects
  assertion covers `.skott` absence, not arbitrary filesystem/process effects.

Execution/diagnosis attributed to the implementing context: the original and
strengthened regression both failed before isolation, reporting a missing
`default_provider` model before listing. Explicit registry paths overlay user
registries, so ambient native entries contaminated this external-only fixture.
No runtime source or validation/authority behavior was changed. Nextest run
`f3245ca5-7301-428e-9513-1a73080a61ea` passed all 32 selected external-agent,
shared-registry and configuration tests; 482 tests were outside the selection.
Strict all-target Skott Clippy, formatting and whitespace checks passed.

Separate source-blind `external-fixture-comparator` compared the scoped intent
and contract with this record, without source access. It found no concrete
scoped contradiction: both configuration paths are isolated, the external-only
catalog is consistent with the conditional native-default requirement, and
terminal refusals retain the documented boundary. Execution remains attributed,
not independently rerun. Arbitrary side-effect absence, suite-wide isolation,
acceptance and compliance certification are not established by this correction.

## Repository test performance and diagnostics (2026-10-27)

### Independent source-only observations

Two fresh contexts read source/tests/executable configuration only, excluding
intent, queues, existing records, reviews, chat and Git history. Neither ran
tests or certified compliance. The first covered the changed native fixtures,
build script, profiler, scheduling, recipes and CI. The second independently
observed the final profiler and its tests after a cleanup finding was repaired.
The following scoped account is derived from those observations.

Nested repository Cargo commands set
`CARGO_TARGET_DIR=/workspace/target/repository-trial`. A native fixture holds
the ordinary target's legacy/build/artifact locks while the shared helper
compiles and tests a dependency-free crate; its watchdog cancels after five
seconds. The actual repository trial executes Cargo metadata and Galla library
tests through the isolated target.

The host inventory fixture enumerates entries without following symlinks,
records mode/length/nanosecond mtime/ctime and literal link targets, and hashes
every regular file in 64 KiB chunks. Up to eight scoped workers take indices
atomically and merge results into the existing sorted map. File metadata is
rechecked after hashing. Fixture assertions compare one/two/eight workers,
empty/binary/nested content, links and changed bytes. The real trial compares
complete before/after recorded inventories despite a panic in its body.

The two existing full-pool nextest rules have priority 100; native inventory
checks have priority 50 without a pool reservation. Ordinary work retains
priority zero. The shared harness reports slow tests every 30 seconds and
terminates after ten periods. CI uses this policy for both workspace jobs,
retains private diagnostics for seven days, and executes doctests separately.
The local `msrv` recipe remains on Cargo's runner, outside nextest policy.

The build script watches existing Git-resolved HEAD/index/refs/packed-refs and
tracked workspace paths, plus its own source and `SOURCE_DATE_EPOCH`. Missing
tracked files use an existing ancestor. Git inventory output is byte-bounded;
non-UTF-8/newline watch paths fail explicitly. Linked worktrees resolve their
administrative paths through Git. Exports retain unknown metadata. Epoch date
arguments use a single `@SECONDS` operand. Fixture tests manually execute the
compiled build script; actual Cargo freshness requires separate execution.

The standard-library Linux profiler launches explicit argv, not a shell, with
null stdin, redirected output and a new session. It creates fresh reports
outside the checkout/current directory with private directory/file modes.
It samples observable descendants, including bounded thread wait summaries,
and streams full samples while retaining summary scalars in memory. Per-test
durations come from validated completed nextest JSON events. Command failures
and signals remain failures; setup/reporting failures produce incomplete
diagnostics where possible.

CPU identities include PID/start time. The tracker reconciles own, live-child,
ended-child and waited CPU, retaining new observed own-CPU increments despite
lost namespace accounting. The final output labels this a sampled estimate,
not an exact or guaranteed lower-bound measurement. Waited user/system usage
is separate. Missing quota visibility warns that affinity can overestimate
capacity. Aggregate sampled RSS can duplicate shared pages; PSI is host-wide.

The initial observer found that leader exit bypassed group cleanup and
escalation. The final observer sees handlers installed before launch, cleanup
on all protected exits, a bounded direct-child wait, remaining-group TERM/KILL
escalation and nested handler restoration. Tests assert stopped ignoring
descendants after both ordinary leader exit and interruption. Cleanup does not
reap arbitrary descendants or contain escaped sessions; final group SIGKILL
is not followed by a group-empty wait.

Retained limitations: raw command logs are unredacted; report size, retained
lifetimes/events and total runtime are not capped. Non-atomic CPU reads can
overlap or miss work. Cgroup mount layouts/hidden limits are not fully resolved.
Inventory comparison is not an atomic filesystem snapshot and records neither
ownership, hardlink identity nor xattrs/ACLs. Deleted tracked paths can broaden
Git watches. Git subprocess output bounds are not elapsed-time limits.
Sampled lifetime I/O counters can miss work or overlap waited-child accounting;
they are not unique physical I/O totals.

### Implementer-executed evidence

The original spontaneous timeout did not recur. Before target isolation, the
held-lock fixture printed `Blocking waiting for file lock on build directory`
and reached its five-second cancellation; after isolation it passed. A build
metadata regression first failed on a nonexistent crate-local Git watch.
Additional profiler regressions first exposed lost-accounting CPU plateaus,
silent missing-quota fallback and surviving descendants in both exit modes.

All runs below used offline/locked workspace/all-features nextest, the same
20-thread pool, warmed builds and the same final Rust implementation. Only
the selected scheduling policy differed. Order was counterbalanced:

| Scheduling | Test-phase seconds | Passing / existing skips |
| --- | --- | --- |
| Old policy | 57.897, 64.219 | 1875 / 15 each |
| New policy | 28.920, 32.088, 28.071 | 1875 / 15 each |

Command walls were 58.339/64.659 and 29.508/32.816/28.526 seconds respectively.
Sampled CPU estimates stayed near 93 seconds; average occupied cores rose from
1.4–1.6 to 2.9–3.3. Raw candidate regressions were retained, not hidden; final
reports warn about one invisible leaf quota. No sampling or event-parse errors
were recorded. Host storage/cache conditions vary; these are finite observations,
not a portable utilization threshold or a CPU-bound claim.

Actual nextest filter selection and start/completion events showed all 34
reserved trials with zero peer overlaps, ordinary concurrency reaching 20, and
the native repository trial moving from executed start 1800 to 33 (zero-based).
The inventory checks moved from late starts to 35–37. A separate inherited
CI-profile run with three threads passed all 17 native tests, started its
repository trial first without overlap and reached ordinary concurrency three.
Analysis used counters for duplicated lib/bin test names and excluded ignored
starts without execution, rather than misclassifying them as active work.

The native repository trial took 3.414–3.588 seconds. The remaining longest
inventory trial took 23.639–27.353 seconds under the new policy. Sampled thread
waits included storage/journal waits and intentional sleeping/pipe waits.
Parallel complete snapshots measured 2.332/2.366 seconds in the focused trial;
the rejected path-key sorting variant was slower and was removed.

After one 21.79-second full build, an identical unchanged no-run build finished
in 0.10 seconds without compiling Skott. Strict all-target/all-feature Skott
Clippy, Rust 1.95.0 all-target/all-feature Skott check, all workspace doctests,
formatting and all 18 Python tooling tests passed. The final CPU fixture uses
a process-CPU budget rather than a wall-clock busy loop and asserts actual
waited CPU; this test-only refinement followed the source observation.

These are implementer-run checks, not independent execution or acceptance.
The accepted intent revisions remain untouched; revalidation is explicitly
stale for the changed documents.

### Independent source-blind comparison

A separate source-blind context independently parsed the selected tests,
runner events and arithmetic, and compared only scoped intent with these fresh
observations and attributed execution logs. Its durable report is
[`docs/reviews/test-performance-comparison.md`](../docs/reviews/test-performance-comparison.md).
It confirmed the finite exclusivity/concurrency and timing results without
claiming a portable performance bound or independently executing tests.

The reviewer retained two intent questions for explicit human arbitration:
whether "metadata" includes ownership/hardlink/xattr/ACL fields beyond the
preserved representation, and whether the environment-value prohibition is
intended to cover unredacted arbitrary child output. No actual disclosure is
alleged. The five-minute runner limit and every Git invalidation transition were
not dynamically exercised; cleanup/estimator limits remain explicit. Intent
was not changed to conceal these findings. Review completion is not acceptance;
the accepted revisions remain unchanged and revalidation remains stale.

## Prompt-local read continuation and incremental tools (2026-10-27)

### Independent source-only observation

A separate clean-slate observer inspected the native tool implementation and
its test sources, excluding intent, queues, existing records, documentation,
reviews, chat and Git history. It did not execute tests. The following is an
excerpted summary of its source-derived observations, not a compliance claim.

The registry exposes, in order, `shell`, `read_file`, `write_file`, `list_dir`,
`find_files`, `search_files`, `edit_file`, `append_file`, `apply_patch`.
Native arguments are closed typed objects. Explicit null is not omission for
optional strings. Rendered native requests carry JSON rather than shell text.

Each `Skott::run_controlled` invocation constructs a fresh executor bridge with
a mutex-protected map of at most 4096 exact canonical path strings. The helper
and direct executors remain stateless. Omitted offsets initially use zero;
subsequent implicit reads receive the stored end offset and digest. Explicit
offsets bypass injection. Changing limits does not reset a cursor. Mutations
do not update it, so an implicit post-mutation read can fail stale and require
an explicit zero-offset restart. The sequential runtime, not the mutex alone,
keeps resolve/execute/observe ordered.

Only successful, bounded, complete JSON pages with consistent offset, content
length, continuation, total size and digest metadata advance state. Failed or
malformed results do not advance it. The bridge cannot independently recompute
a whole-file digest from a single page. Reads before EOF are repeatable; after
EOF an explicit earlier offset remains repeatable, while terminal proposals use
Sav's existing argument-based action detector. One additional empty EOF read
can execute before identical later proposals are rejected.

Dispatch recording receives the original proposal before cursor resolution.
Successful page results contain the actual offset and digest; failed implicit
continuations do not expose injected arguments through this bridge. No
automatic effect replay or new filesystem/network authority was observed.

Append requires an existing UTF-8 preimage and digest and inserts no newline.
Patch requires an existing UTF-8 preimage and digest, exact ordered path-free
unified hunks, valid coordinates/counts and LF transport lines. CR is content;
the explicit no-final-newline marker removes the transport LF. All hunks are
computed before replacement. Unsupported formats, stale/context mismatches,
oversized results and no-change patches fail before installation.

All mutations share descriptor-relative no-follow parent/target access,
same-directory exclusive staging, file synchronization, a preimage recheck,
atomic rename and directory synchronization. Replacements preserve mode, not
ownership, ACLs, xattrs or hardlink identity. They are not transactional
compare-and-swap against external writers. Post-rename I/O failures may report
failure after an effect. Native sandbox requests stage private read-only
payload/helper grants outside the workspace; host execution remains a separate
unconfined path.

### Implementer-executed evidence

Before production changes, five incremental-tool regressions failed because
the tools/read digest argument were absent. The loop continuation regression
failed with repeated first-page content rather than interleaved next pages.

The affected library/component/incremental/native-file/loop/steering suites
passed: 224 + 40 + 7 + 36 + 45 + 6 tests. The explicitly selected real sandbox
trial passed append and patch through the installed helper, workspace
confinement, inaccessible outside content and denied host-loopback network.
Strict all-target Skott Clippy, formatting and patch whitespace checks passed.
These are finite local executions, not model-performance or blanket isolation
claims. Live-model tests were not run.

Subsequent result-bound/binary-preimage and stale-continuation recovery
regressions, plus prompt/error-hint wording clarifications, are additional
work beyond the observer's original source scope. Final execution passed
224 library, 40 component, 8 incremental-file, 36 native-file, 46 loop and
6 steering tests (360 total), plus the real sandbox trial. The added loop
regression observes two identical failed continuations retaining their offset
and digest, then successful explicit restart against a changed preimage.
All-target strict Clippy, workspace formatting checks and patch whitespace
checks passed again. No additional production algorithm changed.

### Ancillary existing-code observations

The source-only observer identified two existing-code issues outside this
change's new cursor/append/patch behavior: host shell execution does not stage
the script produced for commands over 4096 bytes (`host.rs:98-111`), and a
new whole-file write target can equal its generated replacement filename,
temporarily exposing that destination before the stale check fails
(`file_tools.rs`, `Replacement::create` and `replace`). Neither existing
mechanism was changed here. Append/patch require existing targets, which
cannot be allocated by the exclusive temporary-file creation. These findings
are retained for separate arbitration, not silently certified away.

### Independent source-blind comparison

A separate reviewer read only the scoped requirements, native-tool contract,
tool-rendering design, root contract, this new implementation-record section
and the finite execution log. Its report is retained at
[`docs/reviews/tool-ergonomics-comparison.md`](../docs/reviews/tool-ergonomics-comparison.md).
It found substantial correspondence and no concrete contradiction in the new
cursor/append/patch behavior. It identified an underspecified no-op patch
policy, later-observation and coverage limits, and retained the two ancillary
existing-code findings for separate arbitration. The test log's aggregate
counts do not independently prove individual assertions or identify all
quality-gate commands. No intent was automatically changed to conceal these
findings, and no acceptance or formal compliance is claimed.

## Repository scheduling configuration relocation (2026-10-25)

### Independent source-only observation

The following observation preceded the recipe-assertion refinement below.
Its scope was `tools/nextest.toml`, the `test` recipe, and the manifest-directory,
native repository trial and scheduling regression source excerpts. The observer
did not read intent, prior records, reviews or Git history, or execute tests.

The `test` recipe runs `kvist vendor`, then invokes
`cargo nextest run --config-file tools/nextest.toml --locked --workspace --all-features --test-threads num-cpus`.
Configuration selection is explicit; profile selection is not.

The configuration contains one default-profile override selecting
`package(=skott) & binary(=rust_build_environment) & test(=native_repository_tests_execute_inside_the_workspace_sandbox)`
with `threads-required = "num-test-threads"`. This declares whole-pool thread
reservation for that exact package, binary, and test combination, rather than
merely assigning a serial group. An empty `profile.ci` table is also present.
These are configuration declarations, not observed scheduler execution or
evidence of exclusivity across separate invocations.

The native test carries `#[serial]`, checks that `.kvist/vendored` exists,
constructs a sandbox executor using `/usr/bin/bwrap`, and requests Cargo metadata
followed by `cargo test -p galla --lib -- --test-threads=2`. Its visible success
assertion checks whether returned output contains `test result: ok`; helper
behavior is outside this observation.

Two additional tests assert that parsed TOML contains the exact filter with the
required reservation string, and that the first nextest command found in
`justfile` includes the explicit configuration argument. The initial observer
identified that the latter could match an unrelated recipe. That finding
prompted the refinement independently observed next.

### Independent observation of the refined recipe assertion

A separate clean-slate context read only the refined test excerpt:

The test selects the repository Just recipe beginning with the exact header
`test:`, then searches its initial contiguous indented lines for the first
command whose trimmed text starts with `cargo nextest run `. It splits that
command on whitespace and asserts that adjacent arguments are exactly
`--config-file` and `tools/nextest.toml`. It fails if the justfile cannot be read,
the exact recipe header is absent, no matching command is found, or that
argument pair is missing. This is a static text check: it does not execute Just
or nextest, parse shell syntax, verify the configuration file's existence or
contents, or demonstrate actual scheduling behavior.

### Implementer-executed evidence

- A tracked-only archive of `38be501` reproduced the CI missing-file panic
  without moving or modifying the ignored local configuration.
- Before adding the shared configuration and recipe flag, both scheduling
  regressions failed. The original full-pool assertion remains unchanged.
- Stable and Rust 1.95.0 each passed both regressions with all features. The
  same binaries each passed against a staged tracked-only archive containing no
  `.config/nextest.toml`.
- In that isolated archive, removing the flag from `test` while adding a
  correctly configured unrelated recipe made the refined assertion fail.
  Restoring the archive made both toolchains pass both checks again.
- Explicit nextest default and CI profile runs each passed both checks, using
  four and three threads respectively. `just --dry-run test` showed the explicit
  configuration argument. Formatting, strict all-feature targeted Clippy and
  patch whitespace checks passed.

These initial executions cover the configuration relocation and recipe
selection, not native repository-trial overlap or the whole workspace.
The existing exact selector and full-pool setting are retained. No production sandbox
behavior or ignored local configuration was changed. Custom overrides, separate
invocations, external writers and stale fixtures remain outside the guarantee.

### Initial independent source-blind comparison

Independent source-only observations match the relocation intent:
`tools/nextest.toml` declares the exact package/binary/test selector and full-pool
reservation in the default profile; `just test` explicitly selects that file.
The refined recipe assertion targets `test:` rather than potentially accepting
an unrelated recipe. These are finite static observations, not
scheduler-execution proof.

Implementer-executed evidence reports passing regressions on two toolchains,
tracked-only archives without `.config/nextest.toml`, a negative recipe-selection
check, and explicit default/CI-profile runs with different thread counts. This
supports checkout independence and configuration selection, but those runs
exercised the regressions, not native-trial exclusivity or ordinary-test
concurrency.

No concrete implementation contradiction is established. However, the scoped
requirement and design demand runtime demonstration of both exclusivity and
retained parallelism; that demonstration was absent from the permitted new
evidence. The reviewer requested arbitration on fresh event-based verification,
without relying on historical evidence or claiming formal acceptance.

### Additional implementer-executed native scheduling evidence

Fresh native verification was performed rather than relaxing the requirement.
The complete `rust_build_environment` integration binary passed all 16 tests
under both the default profile with four threads and the inherited CI profile
with three threads. Both runs used the tracked configuration, all features,
offline locked builds and `KVIST_GALLA=/opt/target/debug/galla-runner`.
Structured events were captured using
`--message-format libtest-json-plus --message-format-version 0.1` and
`NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1` for independent examination. The nextest run
IDs were `c087798b-9fd9-41da-8b3c-1710c3bc52e3` and
`3a92d26b-0913-4a25-a24a-d7fe117586d4`, respectively.

### Independent source-blind runtime evidence comparison

A separate context parsed only the two fresh event streams and scoped
scheduling intent with a standard-library script:

| Metric | Default (4 threads) | CI (3 threads) |
| --- | ---: | ---: |
| Starts / completions / passes | 16 / 16 / 16 | 16 / 16 / 16 |
| Native trial runs / passes | 1 / 1 | 1 / 1 |
| Active peers at native start | 0 | 0 |
| Peer starts during native trial | 0 | 0 |
| Overlap violations | 0 | 0 |
| Peak ordinary concurrency | 4 | 3 |
| Unfinished / ignored / ambiguous events | 0 / 0 / 0 | 0 / 0 / 0 |

Suite totals reconcile. No observed scheduling discrepancy. These streams
demonstrate isolation and ordinary parallelism, not configuration implementation,
recipe wiring, or clean-checkout independence. This fresh evidence addresses the
initial comparison's runtime-evidence gap; neither comparison is formal
acceptance or blanket certification.

## Observed public contract

### Scope and entry points

- `skott/Cargo.toml` declares version 0.3.0, edition 2024, Rust 1.95, a
  `skott` library, the `skott` executable and `skott-file-tool`. The library
  forbids unsafe code but uses Unix/Linux filesystem and process facilities.
- This is a workspace agent frontend, not a protected task broker. Generated
  system prompts disclaim acceptance, approval, canonical evidence and output
  promotion authority. Journal and transcript metadata mark
  `canonical_evidence: false` (`src/run.rs`, `src/session_log.rs`,
  `src/transcript_writer.rs`, `src/external.rs`).
- The binary initializes diagnostics, parses arguments and dispatches inspection,
  shared setup, model listing, external-agent execution, headless execution or
  the terminal UI. Attachment and setup precede Skott configuration discovery
  (`src/main.rs:29-70`). Source-derived behavior and supplied executed evidence
  are distinguished below.
- Public library surfaces include configuration/model selection, tool rendering,
  sandbox and explicit host executors, native session/context adapters,
  synchronous runs, owned session workers, recording/history, independent
  inspection, external runs, Markdown presentation and TUI configuration
  (`src/lib.rs`, `src/session.rs`, `src/run.rs`).

### Configuration and shared profiles

- Configuration discovery is explicit `--config`, current-directory
  `skott.toml`, user XDG configuration, then system XDG configuration. The
  loader accepts schema version one and bounded UTF-8 TOML from a regular
  non-link final file, with a 64 KiB encoded/read bound. Unknown top-level and
  several nested fields fail parsing; generic parse diagnostics identify a line
  without echoing the offending value (`src/cli.rs`, `src/config.rs`).
- Configuration includes a working directory, local models/default provider,
  reasoning effort, tool policy/profiles, sandbox executable paths and theme.
  Startup resolves the effective working directory canonically and requires a
  directory. Local model identities and native options are validated through
  Sav registry calls. Defaults include medium reasoning effort and per-model
  300-second deadlines, retry settings supplied by the shared retry
  type/defaults, and a 30-second cadence timeout; exact shared retry
  defaults are not independently observed here.
- With a registry section, explicit `--registry`, or no inline models, the
  loader calls Sav `RegistryPaths::for_project(current_dir).load()` and
  `resolve(name)`. A configuration-relative registry path is resolved relative
  to its configuration file; the CLI registry path overrides it. An empty
  profile selection enumerates enabled resolved profiles. Duplicate
  local/shared IDs fail. Native profiles become selectable `Model` values;
  external profiles remain outside the native model list. Resolved profile
  name/source/digest/backend values are held in memory, rather than reloaded
  during an ongoing session (`src/config.rs`).
- `[registry].default_model` is an optional local fallback binding, not a
  provider/model definition. After retaining/resolving shared profiles, the
  loader requires this name to match both a native model entry and a retained
  shared-profile key, with the configured `default_provider`. It then sets
  that in-memory model's `is_default` flag. A local inline model alone cannot
  satisfy the binding, and an external profile has no native entry to bind.
  Unknown, filtered-out and wrong-provider selections return a
  `registry.default_model` configuration error. Automatic registry enumeration
  filters disabled profiles; explicit profile-list resolution continues to
  delegate validity/enablement checks to Sav rather than implementing a new
  local enabled check (`src/config.rs:439-444,490-551`).
- The shared fallback does not override another existing native default:
  ordinary per-provider duplicate-default validation still rejects that
  combination. This binding changes in-memory selection metadata only; there
  is no file-write path in the inspected loader, and backend/model/options
  remain those resolved from the shared profile
  (`src/config.rs:534-551,644-659`).
- `--setup` delegates provider discovery/qualification/save to
  `sav::run_registry_setup`; `--force` is forwarded as a setup option.
  Registry layering, qualification, persistence and backend argument validation
  are delegated mechanisms, not independently inspected implementations.
- Interactive automatic model selection prefers an explicit selection, a sole
  applicable model, an active match within the default provider, then a marked
  provider default; otherwise it requests selection. Multiple distinct active
  matches currently select the first configuration-order match; duplicate
  matches of the same endpoint/loaded identity are treated as ambiguous.
  Discovery uses bounded Sav metadata calls, without inference. Headless
  selection supplies an empty active-model map and therefore uses
  explicit/configured selection, not mutable provider loading state
  (`src/config.rs:927-1028`, `src/headless.rs`). A shared registry fallback is
  an ordinary marked default for this selection function: it remains below
  explicit selection, sole-applicable-model selection and an active native
  match; the binding does not make external profiles auto-selected.
- Context capacity is CLI override, model configuration or selected-provider
  discovery, with no assumed 8192-token fallback. Accepted resolved windows are
  2 through 1,048,576 tokens. Output reserve is CLI, model configuration, or
  one quarter of capacity clamped to 1 through 8192, and must fit below the
  window and satisfy shared run-limit validation. Provenance is displayed and
  recorded (`src/config.rs:99-155`).

### Execution modes and presentation

- Native interactive execution is sandboxed and multi-turn by default.
  `--allow-host-execution` explicitly selects unconfined host commands, with
  one turn by default; `--host-turns` must be within the shared maximum.
  Interactive logging may be disabled. The normal interactive log location is
  `.skott/runs` inside the writable workspace, so private Unix modes do not make
  these default logs inaccessible to the workspace agent
  (`src/tui/mod.rs:82-197`, `src/host.rs`).
- `--headless PROMPT` runs one sandbox-only prompt without a terminal and
  requires durable logs outside the workspace. It rejects host execution,
  host-turn overrides, disabled logs, blank prompts and prompts over 64 KiB.
  Its default log location is under absolute XDG state or HOME state storage.
  Plain stdout contains only a successful, terminal-escaped answer; JSON mode
  emits sequence-numbered schema-one NDJSON, including startup metadata and a
  terminal summary. Runtime failure/cancellation is unsuccessful; the binary
  maps cancellation to 130. Some startup failures return diagnostics before any
  summary is emitted (`src/headless.rs`, `src/main.rs`).
- CLI default whole-prompt limits are 86,400 seconds and 100,000,000 estimated
  budget tokens; argument parsers bound explicit values. Retry, context,
  turn/deadline/token accounting and proposal validation run through Sav's
  native runtime, not through an independently implemented Skott loop.
- The TUI has an editable multiline prompt, model/effort selection, theme
  selection/reloading, reasoning collapse/reveal, progress and usage labels,
  scrollback, help/menu and read-only history replay. Ctrl+Enter or Enter on a
  blank subsequent line submits; Ctrl+C cancels active work or quits when idle;
  Ctrl+Q quits from the live view; new session shuts down the previous worker.
  Modal overlays handle their own keys before live-view bindings
  (`src/tui/app.rs`, `src/tui/mod.rs`).
- Markdown is rendered by calls to Blad and locally adapted from ANSI into
  ratatui spans; its implementation was not inspected. Display reflows with
  width/theme, and retained live rows are capped at 5000. History reads are
  regular/non-link, bounded to 5 MiB each and 4096 directory entries.
  Intentional clipboard copying uses OSC 52; raw diagnostic files may retain
  sensitive text and terminal control bytes (`src/markdown.rs`,
  `src/tui/app.rs`, `src/history.rs`).

## Observed requirements and constraints

These are source-enforced conditions and source-visible limits, not statements
derived from intent documents.

- The model-facing registry exposes stable generic tools: `shell`, `read_file`,
  `write_file`, `list_dir`, `find_files`, `search_files`, `edit_file`. Language
  profiles change advertised available toolchains; they do not add a separate
  language execution authority. Generic is always present. On/auto/off gating
  and explicit overrides distinguish required unavailable tools from optional
  omitted tools (`src/tools.rs`, `src/toolchain.rs`).
- Shell input is exactly one nonblank, NUL-free `command`, at most 16 KiB.
  Configured deny substrings/prefixes extend a built-in minimum. This string
  filter is advisory, not shell isolation. Commands up to the protocol's
  4096-byte scalar bound are inline; longer commands use a staged read-only
  script in sandbox execution (`src/tools.rs:420-491`, `src/executor.rs`).
- Native file requests are closed, revalidated typed payloads at most 256 KiB.
  Paths are absolute, bounded to 4096 bytes and lexically canonical without
  empty/dot/parent components. Mutations must be strictly below the write root;
  read authority instead comes from the process filesystem namespace.
  `skott-file-tool` is not itself a sandbox.
- Reads hash the complete file, accept UTF-8 byte offsets and return adaptive
  pages with actual `next_offset`. Complete files are bounded to 1 MiB;
  read pages have default 4096 and maximum 16384 requested bytes. List/search/
  discovery pages default to 100 and cap at 256 entries. Success JSON is fitted
  below the 7000-byte complete encoded-output bound, including outer string
  escaping (`src/file_tools.rs`, `src/bin/file_tool.rs`).
- Search/discovery are literal, deterministic and bounded: 4096 traversal/
  result entries, depth 32, 8 MiB scanned file content, 1024-byte query/filter
  and line previews. Generated directories are pruned unless opted in, with
  skip/exclusion counters and a coverage-completeness flag. Unsupported
  ordinary entries and hard resource overruns fail explicitly.
- Writes accept at most 64 KiB content and optional preimage SHA-256. Edits
  require the expected digest and exactly one old-text occurrence, including
  overlapping occurrences. Descriptor-relative no-follow traversal,
  exclusive replacement files, mode preservation, sync and rename protect
  these mutations. A fresh preimage check detects ordinary intervening
  changes, but is explicitly not transactional compare-and-swap against
  arbitrary concurrent writers (`src/file_tools.rs:1040-1152`).
- Sandbox tools always build an Authoring-phase, version-one Galla request:
  canonical writable workspace at the policy write root, read-only context/
  toolchain grants, denied network and explicit identities/resources.
  Defaults per tool are 120 seconds, 1 MiB captured output, 256 processes,
  4096 files, 64 MiB per file and 1 GiB scratch. The installed runner must be
  outside the workspace. Request construction calls Galla validation; actual
  isolation/resource enforcement is delegated to the installed runner/backend,
  not established merely by constructing the request (`src/sandbox.rs`).
- Preflight bounds workspace inspection to 30 seconds, one million entries,
  depth 128 and 32 MiB path storage; identity reads cap each file at 256 MiB.
  Escaping workspace symlinks, overlapping context roots and unsupported
  context entries fail closed. Native helper paths must be regular, non-link
  with non-link ancestors and outside the workspace. Payloads use private
  host-generated names in the workspace's canonical parent, with RAII cleanup;
  an unwritable parent fails rather than falling back into the workspace.
- Rust workspace resolution selects installed resources, validates pins and
  installed inventories, snapshots `.kvist/vendored` outside the writable
  workspace and exposes read-only `/rust/*` grants. It does not provision
  dependencies or installations. Local code stages fixed wrappers obtained
  from `sav::offline_rust`, private registrations and per-invocation homes/
  caches, with persistent `/workspace/target`. Wrapper semantics such as
  offline/locked Cargo and installed-version selection are supported by local
  test assertions, not independent observation of Sav wrapper source.
  Snapshot bounds are 1 GiB and 100,000 entries. The actual preparation constant
  is 180 seconds, despite comments/error text referring to 30 seconds
  (`src/rust_environment.rs`, `src/rust_environment/inventory.rs`).

## Observed internal design and failure paths

### Shared native runtime and real steering paths

- `AgentSession` wraps/dereferences Sav's session; `ContextManager` similarly
  wraps Sav's context manager; events, guidance states, mailbox, limits,
  summaries and retry policy are shared types. `Skott::run_controlled` constructs
  `sav::runtime::NativeRuntime` and adapts the local executor, sink and recorder.
  The executor bridge marks native observation tools repeatable and supplies a
  special repeated-page-zero read notice. No Sav implementation was read
  (`src/session.rs`, `src/context.rs`, `src/retry.rs`).
- The worker owns conversation, transport, context and recorder across prompts.
  It obtains tasks from one shared mailbox, records the prompt before running,
  invokes the shared controlled runtime, resets cancellation after an ordinary
  run and terminates on recording/channel/runtime errors. The event channel has
  128 slots and shutdown-aware backpressure. Prompt-sender lifetime is tracked
  separately so dropping all senders permits an idle worker to exit
  (`src/run.rs:29-337`).
- Submission classification belongs to the mailbox/run owner, not the UI's
  potentially stale `running` flag. The TUI sends to the current worker's
  `PromptSender::send`; while active, accepted text is supplemental guidance.
  If the selected model/effort changed, it first uses guidance-only `steer`;
  idle rejection permits replacement with the explicitly selected worker,
  rather than accidentally starting a new task on the former model. Rejected
  input is restored to the editor. Model/effort changes alone do not replace
  an active task (`src/tui/mod.rs:809-880`, `src/run.rs:35-53`).
- The local worker test
  `real_submission_path_interrupts_active_generation_instead_of_queuing_a_task`
  asserts cancellation of a quiet generation, a second request containing the
  original task and guidance, accepted/claimed/delivered notices, idle
  guidance rejection and no replacement task. This is test SOURCE evidence,
  not an executed result (`src/run.rs:861-915`).
- Startup model/tool discovery and model bootstrap use separately owned,
  cancellable jobs off the UI thread. Pending startup submissions have a
  32-entry/1 MiB queue bound; failed bootstrap can retry a retained prompt up to
  three times before provider/tool work begins. Stale bootstrap results are
  discarded after explicit selection changes. New session and UI teardown
  shut down workers and cancel discovery, then restore terminal state
  (`src/tui/mod.rs`, `src/tui/startup.rs`).

### Independent inspection and explicit controls

- `--attach JOURNAL` opens a Sav `JournalFollower` without configuration,
  model discovery, tool registry or executor. Plain output is a snapshot unless
  `--follow`; incomplete snapshots and detached followers are labelled
  explicitly. Terminal inspection follows while allowing scrolling/panning and
  remains inspectable after terminal disposition. JSON rendering escapes event
  control bytes; display limits are 4096 bytes per line, 2048 retained lines and
  1 MiB retained text (`src/inspection.rs`).
- Attachment alone supplies no execution control. `--control SOCKET` is an
  explicit independently supplied capability; cancellation/guidance requests
  use bounded Sav exchanges and report queued/rejected/requested status.
  A queued acknowledgement is not delivery. Terminal controls compose at most
  16 KiB guidance; q/Esc/Ctrl+C detach the viewer, not cancel the run. Journal
  and control paths are not locally cross-validated as belonging to one run.
- Native IPC is opt-in with `--allow-control`. Both frontends require sandboxed
  execution and durable logs outside the workspace, validate the actual socket
  path outside the canonical workspace and disclose the capability only to the
  operator presentation, not through runtime events or recording. Headless
  disclosure uses stderr; TUI disclosure is a frontend-only note.
- `SessionHandle::enable_controls` delegates binding to
  `sav::observation::bind_runtime_controls` with the same worker mailbox/token.
  Socket ownership is shared only between worker and handle. Shutdown and
  natural worker exit remove the listener. The library method checks live
  worker/recording presence, but leaves sandboxed executor and private-log
  requirements to its embedding caller (`src/run.rs:77-160`).
- `cancel` targets the active run; `shutdown` permanently closes submissions,
  clears execution ownership and cancels work. Dropping the session handle
  shuts down and joins even if a prompt sender survives. Viewer detachment
  does not transfer or extend worker ownership. Cancellation never claims to
  roll back completed or unknown effects.

### Recording lifecycle and confidentiality

- `SessionLog` opens an all-non-link directory chain, requires a private final
  directory and creates exclusive mode-0600 journal/text files through a held
  directory descriptor. The first prompt adds a bounded prompt-derived filename
  slug; filenames therefore disclose some prompt text to anyone able to list
  that directory. The operational JSONL uses schema/sequence envelopes,
  request/text/output hashes, argument shapes, action hashes, status flags,
  budget provenance and explicitly unknown mutation state. Dispatch and finish
  synchronize journal, text and directory (`src/session_log.rs`).
- Human-readable text records bounded 64 KiB previews, including reasoning,
  stdout/stderr and failures. These are diagnostic text, not authenticated
  effect evidence. Complete provider usage is distinguished from unknown usage
  and estimated budget charges. The files have no cumulative retention quota.
- `SessionTranscript` additionally owns a structured Markdown conversation and
  a separate Sav `RunJournal` for each prompt. The live journal is created on
  prompt/start, its path is announced, prompt/result text is bounded to 8192
  bytes per record and streamed text/reasoning is marked provisional.
  Controls and tool boundaries are checkpointed; dispatch omits argument
  values and result events omit raw output. Normal finish consumes the live
  journal and writes disposition; a later prompt gets a distinct live file.
  An abandoned/unanswered dispatch is not automatically replayed
  (`src/session_transcript.rs`).
- Live journals are not hash-only: they contain prompt, model text and
  reasoning previews. Structured transcript bodies contain full supplied user/
  reasoning/final text, dispatch arguments and nonempty tool stdout, and can
  therefore hold secrets even when the operational journal omits values.
  The structured writer now also uses held non-following directory descriptors,
  exclusive mode-0600 message files and synchronized atomic manifest replacement.
  Session identities are bounded single safe components; existing session
  directories fail rather than being reused. A final answer is recorded once,
  and a new user message clears the structured completion timestamp and restores
  streaming status (`src/transcript_writer.rs:63-106,137-181,227-273`,
  `src/session_transcript.rs:273-291`). These protections prevent the previously
  observed path-redirection writes, but do not make workspace-local logs
  inaccessible to the workspace agent or immune to deletion/disruption.

### Process execution and opaque external backends

- Local sandbox/host tool subprocess supervision uses a dedicated process
  group, nonblocking stdin/stdout/stderr pumping, one combined output cap,
  cancellation/deadline checks, a 250 ms pipe-drain grace and one-second
  reap grace. The leader is observed unreaped before group signalling, reducing
  PID reuse risk; cleanup targets both the owned group and direct child.
  Partial diagnostics and nonzero status remain outcomes. Successful early
  stdin closure, retained pipes and cleanup failures are explicit failures.
  Escaped descendants and uninterruptible kernel work cannot be guaranteed
  terminated by this mechanism (`src/process.rs`).
- Explicit native host execution retains real filesystem/network/environment
  authority. Native helper mutations retain their mapped root restrictions, but
  shell path substitution is a convenience, not confinement. It uses
  120-second tool limits and shared result-size bounds (`src/host.rs`).
- External-agent profiles are explicitly opaque, separate from native model
  transports/tools. CLI dispatch requires an explicit selected profile,
  interactive stdin/stdout, an initial prompt, host acknowledgement and logs.
  It rejects headless execution, no-logs and native tool/context/turn/token
  overrides. Profile enablement and prompt nonblank/64 KiB bounds are checked.
- `external::start` calls the resolved backend's `command_spec`, then delegates
  to `sav::run_supervised_observed` with zero retries, no loop detector,
  a supplied wall deadline, idle deadline at most 3600 seconds and 4 MiB output
  bound. It checkpoints dispatch before starting a single owner-held worker.
  The journal records profile provenance, host-unconfined/opaque/unsupported-
  steering labels and stdout/stderr text plus raw hex. It does not reinterpret
  output as native tool authority, provide native token accounting or isolate
  the external CLI's internal tools (`src/external.rs`).
- `ExternalRun::wait` joins; dropping an unjoined owner cancels and joins.
  Its initial viewer has explicit `c` cancellation, while detachment leaves
  the owner waiting. This is not daemonization or restart/replay. External
  CLI permissions, internal tools, network use and signal/supervision internals
  are opaque or delegated; their correctness is not established by this record.

## Evidence and limits

Implementation inspection was restricted to `skott/src/**`, `skott/tests/**`
and `skott/Cargo.toml`. No intent artifacts, previous implementation record,
peer implementation, Git metadata/history/diff or prior review was read. The
previous record was deleted without reading it and this record was freshly
written.

**Earlier executed evidence, predating recording repairs:** the operator supplied an exit-zero result for
`CARGO_TARGET_DIR=/opt/target cargo test --offline --locked --workspace
--no-fail-fast --quiet`. The explicitly supplied
`/home/stefan/.copilot/session-state/13dd58ac-9d6a-4316-9d9b-bada72cc2b4c/files/workspace-tests-refreshed.log`
was inspected as test-run evidence only. Aggregating its successful result
summaries confirms **1687 passed, 0 failed and 14 ignored across 102 target
summaries**. These are workspace totals, not Skott-only counts. The quiet log
does not name all individual targets/tests, so its aggregate result is not
represented as a separate executed result for each source test listed below.
Skott-labelled diagnostics in the log also show local Rust-resource trials
running. Ignored live trials remain unexecuted by this command.

For that earlier validation, the operator additionally reported successful workspace all-target Clippy with
`-D warnings`, ordinary workspace rustdoc, and strict Sav/Skott rustdoc.
Those validation logs were not inspected. Strict whole-workspace rustdoc was
reported to encounter unrelated warnings; no peer source was consulted to
evaluate them. This observer did not execute a build/test/linter, live provider,
interactive run or exploit reproduction.

**Bounded post-repair refresh:** only the current
`src/transcript_writer.rs`, relevant `src/session_log.rs` portions,
`src/session_transcript.rs` and new `tests/transcript_recording.rs` were
re-inspected. Behavioral reassessment comes from those source/test bodies, not
repair assertions. The operator reports that the five new transcript-recording
tests failed before repair and now pass, and that the one live-journal test
passes. No corresponding fresh test-run log was supplied for this refresh, and
the observer did not execute them. These are attributed targeted results, not
an independently observed post-repair whole-component pass. The operator also
reports a later full-Skott run encountered a repository-scan ENOENT attributed
to concurrent scratch-directory removal; that diagnosis was not independently
reproduced. **No fresh whole-Skott/workspace pass is claimed after repair.**

**Final registry/default refresh:** only current `src/config.rs` and
`tests/shared_registry.rs` were inspected for this follow-up. The source now
exposes the optional `[registry].default_model` binding described above. The
operator reports 25 passing tests across shared-registry/model-selection
targets and mentions targeted Clippy outcomes; no new command was executed or
result log inspected by this observer. These attributed outcomes do not supersede the
whole-component/workspace verification limitation above. No parent local
configuration, intent, Git or peer implementation was inspected.

**Test SOURCE coverage inspected:**

- `tests/transcript_recording.rs`: five new source tests assert that a manifest
  symlink is replaced without changing its victim and receives mode 0600;
  an exclusive message symlink is rejected without overwriting its victim;
  writes stay on the originally held messages directory after pathname
  replacement; one final answer and a continuing prompt have the proper
  structured lifecycle; the fourteen-CJK-character slug does not panic; and
  duplicate/parent-traversing session identities cannot create/reuse a session.
  These are targeted regression assertions, not exhaustive concurrency or
  filesystem fault-injection coverage.
- `tests/shared_registry.rs`: shared native resolution, in-memory pinning across
  registry edits, external/native separation, setup before config discovery.
  Five additional default-binding test bodies (`lines 109-203`) exercise
  retained enumeration and explicit subsets, native options unchanged by the
  binding, byte-identical configuration/registry files after loading,
  unknown/filtered/external/disabled-by-enumeration/wrong-provider/empty
  rejection, local-only model rejection, duplicate-default rejection and
  precedence below explicit/active selection. They do not independently
  inspect Sav's explicit-disabled-profile resolution mechanism.
- `tests/runtime_ipc.rs`: CLI conflicts, actual worker socket guidance into a
  quiet provider, preserved original task, idle rejection, later-task
  cancellation reset, natural-exit/shutdown socket removal, required recording,
  agent-writable socket rejection, and headless binary guidance with capability
  absence from provider requests, JSON stdout and durable files.
- `tests/startup_lifecycle.rs:319-540`: PTY-driven interactive socket
  cancellation and capability confidentiality checks. Other inspected startup
  tests cover responsive input/quit during blocked discovery, hostile ignore
  FIFO, discovery-delayed prompts and late-selection races. This is not PTY
  coverage of every keyboard-guidance/model-switch interleaving.
- `src/run.rs` unit tests: the real submission-to-worker guidance path, retained
  senders, active-provider shutdown, full event-channel teardown and recording
  failure before turns. `tests/steering.rs` additionally asserts conversation
  preservation/order, but most of those tests inject user messages directly and
  do not by themselves exercise a live worker or UI steering.
- `tests/live_journal.rs`: per-prompt live files, bounded text, provisional and
  control events, argument omission, terminal completion and abandoned input.
  `tests/inspection_cli.rs`: configuration-free attachment, escaped snapshot
  text, incomplete/malformed input and explicit queued-control reporting.
- `tests/external_agent.rs`: one supervised opaque shell fixture, literal prompt
  argument handling, separate stdout/stderr, acknowledgement refusal,
  cancellation disposition and CLI batch/nonterminal refusal. These are not
  qualifications of any real third-party coding agent or its permissions.
- `tests/headless_cli.rs`, `tests/model_budgets.rs`,
  `tests/model_selection.rs`, `tests/input_boundaries.rs`: mock-provider
  protocol/output, serving-capacity selection, failure disposition, private log
  scope, model precedence and input diagnostics.
- `tests/loop_integration.rs` and `tests/context_preflight.rs`: local assertions
  against the shared adapter for proposal rejection before effects, retry/
  cancellation/budget closure, bounded result encoding, repeat detection,
  context estimates/compaction and preserved task/tool framing. These do not
  independently establish the uninspected Sav algorithms.
- `tests/native_file_tools.rs`, `tests/component_tests.rs`,
  `tests/subprocess_supervision.rs`: bounded/escaped native pages, exact edits,
  malformed paths/payloads, helper/staging separation and cleanup, request
  construction, fail-closed runner errors, process output/deadline/cancellation
  and moved/escaped-process cases. Fixture runners test local supervision;
  they are not proof of real sandbox isolation.
- `tests/rust_build_environment.rs`, `tests/rustup_inventory.rs`,
  `tests/toolchain_discoverability.rs` contain native-runner/toolchain trials,
  including offline builds, read-only resources, lock handling, installed
  versions/targets and host-state comparisons. These require provisioned host
  resources; one absence trial explicitly returns when system rustup is
  unavailable. Individual native-trial execution is not separately attributed
  from the aggregate quiet log.
- `tests/live_llama.rs` explicitly ignores its three live sandbox/provider
  trials by default. Their asserted confinement/network and model outcomes are
  source coverage only. TUI/Markdown/history/theme unit tests were inspected
  selectively, not exhaustively.

The independent implementation boundary excludes Sav, Galla, Blad, installed
runner/backend binaries, build script, bundled theme data and real external
CLIs. Shared type names and calls describe integration, not independent
verification of registry trust, socket protocol/permissions, transport
validation, journal framing, offline wrappers or enforcement internals.

## Independent engineering and boundary audit

These are source-derived findings and limits, not approval, certification or
compliance conclusions. No production/test files were changed.

### Scoped recording repair reassessment

The following three findings are no longer open on the post-repair source paths
examined. This is a bounded source/test assessment, not a claim of exhaustive
filesystem-race validation or overall compliance.

**F1 — Formerly High: structured transcript host-write redirection — addressed.**

`src/transcript_writer.rs:63-106` now rejects invalid/multicomponent identities,
opens the private parent through the existing no-follow directory walker,
exclusively creates a mode-0700 session and messages directory and retains both
directory descriptors. `src/session_log.rs:154-193` walks each parent component
with `O_NOFOLLOW`. Message writes use descriptor-relative
`O_CREAT | O_EXCL | O_NOFOLLOW`, mode 0600 and file/directory synchronization
(`src/transcript_writer.rs:227-250`). A preplanted message link now fails rather
than following or truncating its target.

Manifest publication creates an exclusive no-follow temporary file in the
pinned directory, synchronizes it, renames it over `session.json` and
synchronizes the directory (`src/transcript_writer.rs:137-173`). Renaming
replaces a manifest symlink itself rather than following its target. Writes do
not reopen the stored display path, so replacing the messages pathname does
not redirect the retained descriptor. The regression bodies at
`tests/transcript_recording.rs:9-58,107-115` directly exercise manifest/message
links, renamed message directories, private manifest mode and session
no-clobber/identity handling.

The repaired writer removes the source mechanism behind the reported
outside-workspace overwrite. Workspace-local records remain writable/deletable
by an agent with that mount's authority; hostile collisions can cause a
recording error, and a renamed messages directory can make the path-based
presentation index cease to locate the held record. These are availability/
presentation limitations, not evidence of retained link-following host writes.

**F2 — Formerly Medium: Unicode prompt-slug panic — addressed.**

`src/session_log.rs:98-123` now lowers the candidate end offset until it is a
UTF-8 character boundary before truncating. Fourteen three-byte CJK characters
therefore truncate safely below 40 bytes. The exact prior trigger is exercised
by `tests/transcript_recording.rs:98-105`. No provider or guidance-path changes
are involved in this repair.

**F3 — Formerly Medium: duplicate finals, stale lifecycle and in-place manifest
rewriting — addressed.**

`src/session_transcript.rs:273-291` now makes one fallible final-answer write.
`src/transcript_writer.rs:176-181` restores streaming status and clears the
completion timestamp when recording a user message. The continuing-recorder
test at `tests/transcript_recording.rs:60-96` asserts one final entry and the
second prompt's active lifecycle. Manifest publication is now the synchronized
temporary-file/rename sequence described under F1, not in-place truncation.

No additional concrete correctness/authority failure was identified in this
bounded repair inspection. Atomic publication does not make the entire
message-plus-manifest sequence transactional: a crash/error can leave an
unindexed exclusive message file, and failures after namespace effects may
leave completed artifacts despite an error return. No recovery/rollback,
crash-injection or arbitrary concurrent-writer guarantee was established.

### Open ancillary findings outside the scoped recording repair

F4 and F5 remain separate ancillary findings. Their previously inspected source
was not re-read in this bounded recording follow-up, and no historical
introduction claim is made.

### F4 — Medium: long native host shell commands lose their original script

`src/tools.rs:468-489` renders commands above 4096 bytes as argv referencing
`/context/0` plus a separate `shell_script`. `src/executor.rs:91-112` stages
that field for sandbox execution. `src/host.rs:77-116` only handles native
`file_request` specially and otherwise executes the rendered argv, ignoring
`shell_script`. Consequently a legal long host command runs the wrapper against
an unstaged host `/context/0`, failing or consuming unrelated existing content
instead of executing the submitted command. Inspected long-command tests
establish rendering/request shape, not this host execution case.

### F5 — Medium: theme loading's advertised byte bound is only a pathname precheck

`src/tui/theme/file.rs:304-322` checks pathname metadata and then separately
calls unbounded `read_to_string`; it neither pins the checked regular inode nor
limits bytes read. Custom theme loading checks `is_file` first at lines
391-399, but replacement/growth can happen between those checks and opening.
The public loader itself also accepts special-file metadata. A replaced FIFO
can block, or a growing/replaced file can exceed 64 KiB during startup or
synchronous UI selection. This differs from the descriptor-based bounded
configuration/history loaders; no race test was executed.

### Resource, authority and lifecycle limits

- Per-event preview bounds and row counts are not whole-session memory/disk
  quotas. Structured bodies/manifest entries and retained prompt history grow;
  rewriting the entire manifest for each added entry causes cumulative
  quadratic index-write work (`src/transcript_writer.rs:137-160,261-262`,
  `src/tui/app.rs:1287`). Logs have no local retention/rotation quota.
- The event channel's full-queue wait checks permanent shutdown, not active
  cancellation (`src/run.rs:62-74`). A library consumer that stops draining must
  shut down/drop the owner, not expect `cancel()` alone to unblock presentation
  backpressure. Worker/startup `join()` also depends on called transport/
  executor/discovery operations honoring cancellation; arbitrary injected
  implementations are not forcibly bounded.
- Process-group ownership is stronger than killing only a leader but not
  containment of every detached descendant. Local tests explicitly clean up an
  escaped fixture process (`tests/subprocess_supervision.rs:300-319`).
  Host execution and opaque external execution must not be advertised as
  sandboxed merely because they have time/output supervision.
- CLI IPC/log-scope guards are not enforced by the type of an arbitrary
  library-supplied recorder/executor. A low-level embedding can enable controls
  with any recorder present; it owns the documented authority/confidentiality
  obligations. Socket-server framing, permissions and malformed-client
  resilience remain delegated, uninspected mechanisms.
- Diagnostic metadata, hashes, sequence numbers and private modes provide no
  independent approval, tamper-proof provenance or proof of filesystem effects.
  Host-unconfined backends themselves retain authority over host-accessible
  logs. Cancellation, failed recording and absent terminal disposition leave
  prior/unknown effects unresolved rather than undone or replayable.

## Long-session source-observation supplement (2026-10-28)

**Evidence boundary:** Source/test inspection only. No commands, tests, or builds were executed. Assertions described here were inspected, not demonstrated by execution; no full-compliance conclusion is made.

Skott delegates its native conversation loop to Sav. Sandboxed operation defaults to no unattended-turn ceiling or cumulative wall/token cutoff. Explicit host execution defaults to one turn, with host overrides bounded to 1–500. Model defaults are a 300-second attempt deadline, three attempts, two-second initial retry backoff, thirty-second maximum backoff, and thirty-second streaming cadence watchdog.

Frontend context selection uses CLI override, model configuration, or provider discovery. Missing capacity fails rather than silently assuming an 8,192-token window. Automatic output reserve is one quarter of capacity, capped at 8,192 tokens. Workers use capacity-based compaction.

A worker retains its conversation, context manager, transport, and recorder across tasks. Each task receives fresh runtime budgets, progress totals, action tracking, and read cursors; conversation compaction counts persist. Ordinary task cancellation is rearmed after the run. Permanent shutdown closes submissions, cancels work, removes controls, and joins the worker. Events use a bounded 128-entry channel; backpressure waits observe shutdown, not ordinary task cancellation.

The mailbox atomically classifies active submissions as guidance and idle submissions as new tasks. Delivered guidance renews Sav’s generation/unattended-turn allowances without resetting cumulative task budgets. Changed model/effort selections do not move an active task: submissions can still steer its existing worker. Idle submission with a changed selection can create a fresh worker.

`App::new_session` now clears running/spinner state, staged prompt, pending answer/reasoning buffers, hidden reasoning, statistics, growth sampling, and staged/cached clipboard escapes before clearing displayed lines and restoring follow mode. The event-loop new-session action separately shuts down the old worker and starts a fresh conversation. The strengthened `ctrl_n_starts_a_new_session` test seeds active generation and stale data, then asserts cleared state and absence of old text after flushing. Clipboard clearing is directly source-observed, not asserted by that test.

Interactive recording defaults under `.skott/runs` and may be disabled. Headless execution requires recording outside the writable workspace, forbids host execution, and bounds its initial prompt to 64 KiB. Optional controls require sandboxed execution and private logs.

Conversation JSONL/text files persist across tasks and do not rotate; text previews are individually bounded, not aggregate-bounded. Each task receives a separate rotating Sav live journal. Dispatch synchronization precedes effects; transcripts/live text can contain sensitive content. Attachment initializes neither models nor executors, and viewer detachment does not itself cancel execution.

Opaque external agents require interactive host acknowledgement and mandatory logs. They have no native steering/token accounting, zero automatic retries, a default 24-hour wall limit, one-hour idle limit, and 4-MiB output bound.

`loop_integration.rs` asserts 541-turn continuation and cross-task compaction; `context_preflight.rs` asserts protected goals, message boundaries, and atomic failure.

## Automatic nextest policy discovery — source-only supplement (2026-10-28)

Source observation only; no tests/builds executed.

The policy resides at `.config/nextest.toml`, nextest’s default discovery location; filesystem inspection found `tools/nextest.toml` absent. `.gitignore` ignores other `.config` contents while explicitly exposing this policy.

The exact selector `package(=skott) & binary(=rust_build_environment) & test(=native_repository_tests_execute_inside_the_workspace_sandbox)` reserves `num-test-threads` at priority 100. Its test body launches sandboxed repository Cargo tests. Priority 50 covers all `skott/rustup_inventory` tests and exact `kvist/sandbox_toolchain/rust_authoring_has_host_inventory_private_defaults_and_offline_builds`, without a whole-pool reservation.

The empty `[profile.ci]` supplies no reservation or timeout overrides, leaving default inheritance applicable. Workflow nextest commands do not explicitly select that profile. Default slow-timeout settings are 30 seconds with termination after ten periods, distinct from Mermaid’s runtime deadline.

Both justfile nextest recipes and both CI workspace commands omit `--config-file`. They specify `--locked --workspace --all-features --test-threads num-cpus --no-fail-fast`; profiling/CI additionally specify `--offline` and libtest JSON-plus format version 0.1.

Rust regression source checks the exact repository-trial reservation and ordinary recipe discovery. Python regressions assert canonical placement, ignore-rule visibility, two nextest commands per entrypoint file without config flags, exact Mermaid scope/order, two priority-100 reservations, priority-50 inventory scope, and CI inheritance settings. The quality workflow invokes Python unittest discovery.

These are inspected assertions, not executed proof. Explicit alternate configurations and `cargo test` can bypass scheduling. Reservations do not coordinate separate runners or external CPU load. Original timing-failure causation was not measured.

## Independent request-preflight observations (2026-10-28)

These observations derive only from an independent context's permitted current
implementation and test source. It executed no tests or builds and read no
intent, queue, prior record, reviews, chat or Git history. This is not compliance
certification.

### Budgets and directory scheduling

One `Preflight` start time and cancellation token span request construction.
Stage changes retain the same 30-second budget; workers do not receive
independent budgets. Workspace inspection shares a one-million-entry atomic
counter, maximum directory depth of 128, and 32 MiB path-byte accounting under
a mutex. Path accounting charges twice each newly scheduled directory's path
length; it is not a measurement of total allocation.

Runner, backend and context-file identity reads each have a separate 256 MiB
bound, with cancellation/time checks between 64 KiB reads. This is not a shared
aggregate byte quota.

Eight scoped workers consume a shared LIFO directory stack. A visited-path set
deduplicates scheduled directories; an active-directory count distinguishes
completion from a temporarily empty queue. Idle workers use a condition
variable with 100 ms polling. All successfully spawned workers are joined before
inspection returns. Worker-spawn errors, inspection errors and panics stop
peers; an exit guard wakes waiters. Panics become build errors. Poisoned
inspection state also returns an error.

### Links and filesystem inspection

Ordinary directories, including hidden directories, are traversed without
name-based exclusions. Relative links resolve against their containing
directory. Existing targets are canonicalized and checked against the writable
root; failed canonicalization receives a lexical containment check.

The link itself is subsequently canonicalized: `NotFound` is tolerated, other
errors propagate, and a resolved target outside the root is rejected. In-scope
directory targets are scheduled by canonical path and physical relative depth,
allowing aliases and cycles to share existing work.

These checks observe a mutable filesystem; they do not establish an immutable
snapshot. Blocking filesystem operations are not interrupted by the cooperative
timeout, and worker joins must finish before return.

### Diagnostics and dispatch

Preflight stages are `runner_identity`, `backend_identity`, `workspace_scope`,
`rust_resources`, `context_identities` and `request_validation`. Stage-start logs
contain cumulative elapsed time. Timeout errors include the current stage,
elapsed milliseconds and narrowing/reduction guidance.

Scope-summary diagnostics report entries, scheduled directories, configured
worker count, duration and failure status. Request and runner diagnostics report
identities, limits and outcome metadata; grant traces include paths, while
environment traces include names rather than values.

The immediate executor propagates request-construction errors before dispatch
and checks cancellation again before execution. The runner execution wall-time
limit is separate from preflight timing.

### Concrete assertions present in source

Quota tests assert exactly one million successful entry admissions, exactly
four remaining admissions across eight competing workers, acceptance at the
path-byte boundary, and rejection beyond it. Depth tests accept 128 levels and
reject 129. Expired-preflight assertions require `stage=workspace_scope` and
`elapsed_ms=`.

Worker tests assert directory-I/O error propagation, panic conversion,
cancellation failure, 65 visits with peak visitation no greater than eight,
and concurrent progress by two independent directories. Alias/cycle tests
assert three distinct directory visits; a hidden-directory escape must fail.
`build_request` scope-link tests reject a nested escaping link and accept
immediate and nested links to an internal file.

Diagnostic integration assertions require stage, scope, request and runner
messages, status seven and failure metadata, while excluding command/output
sentinels. Native repository tests assert nested Cargo success using a separate
target directory. The lock trial requests cancellation after five seconds if
unfinished; separate tests inspect exact nextest exclusivity and automatic
discovery recipe configuration.

These assertions are source evidence, not execution results or evidence of any
original-host failure's cause.

The observer also identified `grant`'s metadata-error closure shadowing its
source-path parameter: its error context uses OS error text instead of the
grant path. That diagnostic defect is outside the traversal/stage changes.

## 2026-11-03 capacity fixture observations

Independent `skott-fixture-observer` inspected only
`tests/headless_cli.rs` and `tests/startup_lifecycle.rs`, excluding intent,
queues, prior records/reviews, history, logs and peers. It executed no tests.

- The headless capacity fixture expects `GET /models` before inference, without
  a CLI capacity override. It supplies 262144 tokens, asserts that same capacity
  and provider provenance in the initial event, and asserts `max_tokens: 8192`.
- The fixture reads the HTTP header terminator and remaining declared body.
  Its Content-Length lookup is case-sensitive and request buffering unbounded.
- The explicit-selection PTY fixture asserts `/models` first rather than an
  active-model `/props` probe. While capacity remains blocked, typed text must
  remain visible. Quit must succeed within one second and restore the terminal.

The blocked-capacity case neither submits a prompt nor releases discovery.
These are source assertions, not executed results or broad security/compliance
certification.

The separate source-blind `skott-fixture-comparator` found no demonstrated scoped
contradiction with the large-capacity, reserve/provenance and editable/cancellable
wait intent. It retained the distinction between source assertions and attributed
execution, and noted that the blocked fixture does not establish queued dispatch
or post-discovery selection preservation.

The implementing context passed five headless and eleven lifecycle tests;
the final all-feature workspace nextest run passed 2219 tests, zero failures
and 15 existing skips. Strict workspace Clippy, formatting and the workspace
documentation test passed. These attributed results are not acceptance,
review receipts or broad certification.

## Shared-area source observations - 2026-11-07

Independent `skott-source-audit` inspected scoped current local source and
assertions without intent, queues, old records/reviews, peer implementations or
history. It changed nothing, ran no tests and found no high-confidence exploitable
vulnerability in scope. These observations supersede older copied-vendor and
full-content readiness descriptions, not their historical evidence.

- Vendoring dispatch precedes application configuration loading; CLI conflicts
  separate it from agent execution. Checking delegates before compiler resolution.
  Provisioning requires bounded regular lock metadata and passes explicit offline,
  force and timeout options. Offline is not OS isolation
  (`src/main.rs:28-49`, `src/cli.rs:18-37,81-83`,
  `src/dependencies.rs:16-59`).
- Rustup queries use argv, a cleared environment, `/` and disabled automatic
  installation. Cargo must resolve to its standard host installation outside the
  workspace. Inventory accepts 1-128 installed toolchains and records bounded,
  non-link runtime trees (`src/rust_environment/host.rs:14-96`,
  `src/rust_environment.rs:455-497`,
  `src/rust_environment/inventory.rs:18-194`).
- Toolchains, staged runtime/registrations, optional user Cargo binaries and vendor
  material receive read-only Toolchain grants. Existing vendor material uses the
  workspace directory directly, not copied packages. Only its exact source may
  overlap the writable workspace; alias masking is delegated to the runner
  (`src/rust_environment.rs:1036-1101`, `src/sandbox.rs:181-207`).
- A startup read lease covers preparation. Each executor call acquires a lease
  before request construction and retains it through synchronous execution/error
  unwinding, not idle time. Arc-owned private staging survives clones and is
  removed by its final owner (`src/rust_environment.rs:678-714,856-892,1104-1110`,
  `src/executor.rs:148-180`).
- Per-call validation checks lock fingerprints, pin digest, publication fences,
  inventory fingerprints, resource identities, tracked files and registration
  links. Installed-material SHA labels derive from metadata, not comprehensive
  content attestation. Resolver configuration is staged during preparation.
  Vendor children are not comprehensively rehashed; a local assertion permits
  readiness after source tampering and then expects native Cargo checksum failure
  (`src/rust_environment.rs:225-235,749-789,944-1033`,
  `tests/vendoring_cli.rs:362-395`).
- Requests declare denied network, private homes/caches and persistent workspace
  targets. Bounded synchronous supervision is delegated; no host fallback appears
  in these paths (`src/rust_environment.rs:1116-1152`,
  `src/sandbox.rs:258-300,596-660`). Dependency, wrapper, process-supervisor and
  backend implementations were outside this observation.

Implementer-attributed production-runner nextest
`21afbe63-ae2a-482d-a3f4-36c6ab3d20b2` passed 399 affected tests across components.
Native cases assert shared host/sandbox device/inode, writable-alias read/write/
chmod denial, active but not idle reader retention, Cargo checksum failure and
explicit offline force repair. Strict workspace Clippy, formatting, affected
doctests and offline release builds subsequently passed.

A separate warm actual-checkout native trial reported shared vendor preparation
at 109 ms and total installed-Rust preparation at 592 ms with eight toolchains
and 87 tracked files. Full Git/list/Cargo execution took 1.253 seconds. The prior
copying trial reported 81,342 ms and 376,894,920 copied bytes. These are attributed
individual observations, not matched cold/representative-hardware benchmarks or
an SLA; the older general performance obligation is not certified by them.
Queue timestamps record later bookkeeping. No acceptance or review receipt is
claimed.

Separate `skott-blind-comparison` compared current scoped intent with this new
independent observation without source, tests, queues or peers. It found
correspondence for direct read-only views, exact overlap, startup/per-call rather
than idle ownership, drift checks, early dispatch, denied network and native
sharing/masking/checksum/repair evidence.

It reported that the older design sentence claiming exact byte identities at
resolve time conflicted with the observed metadata-derived installed-resource
labels. The design now states metadata-derived identities rather than full-content
attestation, preserving this reported discrepancy and its explicit correction.
The comparison retained missing itemized proof of complete provider readiness,
source-table retargeting, shared policy/default parity, every restart diagnostic,
absence of all inventory hashing and all recovery/ownership cases. Excluded peer
internals and finite attributed native tests do not independently establish those
provider guarantees or exhaustive races. The general representative-hardware
30-second obligation remains unverified; warm observations are not matched
speedup or cold-contention proof.

Final implementer-run production nextest
`7eb7fff4-82b4-4b8d-83a5-9c8e576971ab` passed all 399 affected tests after the
lint correction and queue updates. Both release frontends reported the same ready
root tree. This does not accept stale intent, fabricate receipts or certify an SLA.

At `2026-11-07T01:11:50Z`, the implementing context atomically installed
byte-identical release `kvist`, `skott` and `skott-file-tool` binaries without
stopping existing sessions. Installed CLI versions were Kvist 0.3.0 and Skott
0.3.1; both installed frontends reported the same ready vendor tree. The retired
root receipt was removed, generated state/staging ignore rules were checked, and
the optional root policy remains trackable. Final native queue validation passed.

## Live workspace implementation observation — 2026-11-07

Independent `source-only-live-records` observed scoped source/test assertions,
Cargo manifest and directly referenced helpers without intent, queues, old
repository IMPL, docs, reviews, chat/session checkpoints or Git history/diffs.
A source-only follow-up read only `vendoring_cli.rs` and `rustup_inventory.rs`.
The following publishes scoped extracts, not implementer certification.
The observer ran no tests/builds.

### Observed file tools and progress

`ToolRegistry` renders stable shell/read/write/list/find/search/edit/append/patch
definitions without effects. Default command filters are empty; operator
substring/prefix filters remain advisory. Native arguments and result pages
remain bounded, including 64 KiB text, 1024-byte queries and 7000 encoded bytes;
source/result files have no general quota.

`stream.rs::read_page` seeks only a bounded page and UTF-8 edge lookaround.
Ordinary pages have `sha256:null`, actual page digest, total length and
continuation offsets. Full raw-file hashing requires explicit full-hash/guard
input. UTF-8 validation is page-local; adjusted/shrunken offsets are explained.
Directory/search traversal is no-follow and deterministic, with entry/depth
budgets; symlink/special descendants are skipped/counted, not opened.

`search.rs` streams literal/multiline matching with an 8 MiB per-call work
budget. Match pagination is distinct from `next_scan/scan_offset`; query/UTF-8
overlap preserves boundary matches. Deep global line counts can be null, with
local scan/preview coordinates. Resuming an interrupted directory file does
not cover later siblings; pagination rescans live data, not a snapshot.

Mutation streams revisions/replacements through adjacent exclusive temporary
files. Exact single-occurrence matching includes overlaps; append is verbatim,
and unified hunks require exact coordinates/context with no paths/fuzz.
Unrelated bytes, mode, CRLF and missing final newline are preserved. Digest
guards are optional. Preimage/revision checks, sync and descriptor-relative
rename reject detected stale edits; they are not transactional CAS against
arbitrary host writers. A late writer can race final recheck/rename, and a
post-rename sync error can accompany an installed effect.

Prompt cursors use actual validated page metadata, not synthesized full-file
guards; failed reads do not advance. `NativeProgress` observes only authorized
mutation targets, not recursive shell workspace state. Evidence pairs exact
intent/outcome once, is bounded, and loses credit on ambiguity. Unavailable
observations preserve actual outcomes with diagnostics and no progress claim.
Observed changes do not establish causation against concurrent host writers.

### Observed scope and live Rust behavior

The executor retains canonical workspace origin when writable selector parents
could otherwise steer outside grants; operator-controlled outside selectors
remain live. Native helpers execute in the sandbox, not as privileged host file
RPC. Private staging requires a writable canonical parent; a root-filesystem
workspace or unwritable parent cannot stage.

Sandbox requests are Authoring/System/network-denied with exact workspace
authority, disjoint read-only context and an outside runner. Hashing streams
opened initial lengths without a general size cap; directory identities are
labels, not recursive inventories. Defaults retain wall/output/process/FD/RAM
budgets and no FSIZE quota. A separately selected `HostExecutor` is unconstrained
host execution, not a failed-sandbox fallback; every frontend selection was not
audited. Process cleanup/cancellation reports bounded drain/reap uncertainty.

Trusted system/host-home Rustup and operator-owned registrations select
canonical installations outside writable/credential scopes. Writable selector
parents cannot authorize external targets. The selected compiler plus bounded
optional registrations/extensions are read-only; inventory is capped at 128
names with omission guidance. Optional components are ultimately tested by
actual command execution, not full startup capability attestation.

`live.rs::Cache` observes finite topology/configuration/selected-executable
metadata, repreparing/recovering on changes without restart. Mounted contents
remain live; no recursive hashes, runtime receipt, frozen lock digest or reader
lease gate workspace work. This does not prove refresh of every possible
outside change. Missing/unsupported vendor resources and unavailable requested
Rust (including workspace On/forced) do not block independent tools. Generic
System On-profile availability still has explicit error semantics.

Native Cargo configuration retains tables and retargets directory sources;
malformed/unreadable config causes Cargo-local errors without hiding rustc.
Workspace wrappers inject offline but not forced locked resolution; callers'
explicit `--locked` still applies. Homes/cache/selection are invocation-private,
target output persists, and wrappers do not add namespace authority. Explicit
operator provisioning/checks remain separate, with lock/publication validation.

### Source assertions and attributed execution

Tests assert large streamed mutations, 128 MiB hashing under 64 MiB address
space, sparse terabyte head/tail paging, page-local UTF-8, resumable multiline
search, truthful coverage, exact guards/patches, helper isolation and actual
progress pairing. Rust tests assert live recovery, independent optional-tool
failure, private homes, read-only readable vendor aliases, Cargo-native checksum
errors, lock creation/update versus explicit locked refusal and native build/
fmt/test/doc/Clippy chains.

The observed CLI tests distinguish workspace preparation from explicit operator
fences: requested Rust remains advertised with missing lock, recovery markers
or empty resolver state, without preparation rewriting the project; explicit
vendor check retains publication refusal. A local Git fixture builds offline
with readable/nonwritable vendor aliases, diagnoses a subsequent host checksum
change and restores material through explicit force provisioning.
The inventory test compares exposed host/native versions/targets/components,
executes compilers and denies mutations. Lock comparison checks parsed package
tables, not byte/version/format freezing. Before/after host snapshots are
test-only evidence, not runtime leases or atomic whole-tree guarantees.

Parent-attributed final execution passed 1046/1046 Galla+Sav+Skott tests
(`d98adaee-bb82-463d-849b-a366a791ee8a`), native isolation
(`42ed4b28-e551-48a6-8a73-816bab9bfe25`) and exclusive native checkout
(`c901e06d-9fe5-4523-90da-50145bd8433b`). An earlier native checkout failed on
missing offline uuid1.29.1, then passed after adding only that cached package to
the live project vendor; no existing packages, receipt, source or lock changed.
Earlier full-suite fixture/concurrency and stale-policy assertions were
diagnosed, coupled assertions updated, and the complete suite passed with
TMPDIR unset/four threads. Strict checks and matching release binaries passed.
Existing installed launchers/sessions were not deployed or restarted.
Live-model qualifications remain unexecuted; these records neither accept
intent nor certify model behavior, every host race or the whole application.

### Separate source-blind comparison and unresolved arbitration

`source-blind-live-comparison` read current local requirements/contract/design,
relevant global intent/ADR and the fresh independent records. It accessed no
source/test files, cited source filenames, old repository IMPL, queues, Git,
history or prior reviews, and ran nothing. It found substantial scoped
correspondence for streaming/live tools, bounded truthful continuation,
optional guards, prepared-scope progress, live resource recovery, optional
inventory and offline caller-selected locking. Its superseded lower-policy
prose finding was retained chronologically and reconciled with authoritative
policy through a narrow documentation-only follow-up.

Two findings remain **unresolved for explicit human arbitration**:

- The top lost-concurrent-edit requirement is broader than the contract/design
  and observed nontransactional final-preimage-check/rename guarantee.
  Detected stale changes are protected, but arbitrary external writers can
  race publication. Neither intent nor source was automatically changed to
  conceal this qualification.
- The native host snapshot hashes `.rustup` but does not descend external
  linked installations. Read-only use and version/target execution evidence
  do not establish before/after content coverage of every linked root.
  Separate Galla explicit-grant evidence does not certify automatic Skott
  preparation; this verification gap was not concealed.

Finite topology refresh, non-atomic full-digest/page reads, known-origin
protection and unexecuted live-model trials limit broader claims. Existing
Galla package-network/resource/schema limitations are not newly introduced
network-denied workspace failures, but prevent blanket cross-component
certification. Passing attributed executions do not settle arbitration,
accept intent or change recorded accepted revisions/revalidation metadata.

## Native workspace observation (2026-11-07 UTC)

Independent source-only observer `d38462f8-d2f9-4d78-a5bb-c512f6458890`
derived the Skott section of
[`native-cache-observation.md`](../docs/reviews/native-cache-observation.md).
It excluded intent, queues, prior records/reviews and history and executed no
checks. Observed explicit provisioning uses operator host authority; canonical
checks are process-free and other source checks disclose offline host metadata.
Workspace tooling exposes live native data read-only with private
administration, without forced locking. Topology refresh does not recursively
attest cached-package contents or acquire unavailable sources. Existing broader
arbitration remains unaffected; no acceptance is inferred.

Separate source-blind comparer `a6add327-2a55-4006-b3f6-b2750041ff26` recorded
scoped native correspondence and explicit execution limits in
[`native-cache-comparison.md`](../docs/reviews/native-cache-comparison.md).
The nested repository-trial failure remains separate from that comparison.

The nested trial was subsequently reproduced as Galla's ambient-DNS fixture
dependence. A hermetic lookup seam fixes that test without changing Skott grants
or production limits. Fresh source-only and separate source-blind resolver
supplements are retained in the linked reports; final implementer-attributed
execution passes all 25 Rust-environment cases. Historical arbitration and the
broader dogfood failures remain unaffected.
