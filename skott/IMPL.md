<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

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
