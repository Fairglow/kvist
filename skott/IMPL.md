<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

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
