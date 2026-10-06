<!-- kvist-implementation-record-version: 1 -->

# Component Implementation Record

## Observed implementation: agent-runner

## Observation basis

This replacement record was derived on 2026-10-02 from this package's Rust
implementation and tests, its Cargo manifest, and the permitted agent-runtime
implementation and tests. No intent documents, previous implementation record,
reviews, Git history, engine implementation, or sandbox-runner implementation
were read. This is an implementation observation, not an intent review,
authorization decision, or compliance certification.

Source paths below are repository-relative. Claims about the external sandbox
are limited to the requests constructed here and the native trials actually
executed. Mock runners demonstrate construction and supervision, not isolation.

## Scoped source-and-test observations — 2026-10-06

This supplement was derived by clean-slate observer
`history-observer`, restricted to current `src/run.rs`,
`src/tui/app.rs` and `src/tui/render.rs`. The observer did not read intent,
queues, this prior record, reviews or Git history and did not execute tests.
The remaining sections retain their earlier observation basis. This supplement
is not compliance certification or component acceptance.

- **Live navigation:** Ctrl+Home sets transcript offset to zero and disables
  following; Ctrl+End selects the bottom and resumes following. Plain Home/End
  fall through to the editor. Live shortcuts require exactly CONTROL; replay
  shortcuts accept modifiers containing CONTROL. Replay intercepts input before
  the editor. (`agent_runner/src/tui/app.rs:742-755,852-862,887-892,1483-1523`)
  Tests assert Ctrl+Home preserves the editor cursor and subsequent output does
  not move the viewport, and Ctrl+End restores following. (`app.rs:3017-3049`)
- **Replay extents:** `replay_scroll` is `usize`. Navigation uses saturating
  arithmetic; downward motion clamps to the bottom. Extents count wrapped source
  lines, wrapped title/hint and separator rows at width-minus-one, minimum one.
  Bottom subtracts visible rows saturatingly. Resize clamps the existing offset
  rather than automatically following a newly enlarged extent.
  (`app.rs:234-239,1483-1523,1292-1301,1570-1591,1643-1650,1718-1721`)
  Tests exercise offsets beyond `u16::MAX`, boundary navigation, Escape, resize
  reduction and empty/short histories. (`app.rs:3323-3373`)
- **Rendering/theme:** Live and replay views share the right-edge scrollbar,
  styled from the current `app.theme.scrollbar`. Replay reserves its last column,
  wraps and slices rows using a `usize` offset, and independently clamps the
  rendered slice. (`agent_runner/src/tui/render.rs:152-214,431-505`)
  Tests assert dark/light scrollbar colors, unobscured wrapped tail content,
  reversible top/bottom rendering and scrollbar presence in empty, short and
  one-column views. (`render.rs:691-751`)
- **Prompt guidance:** Both execution-scope prompts interpolate one shared
  instruction allowing Markdown, recommending CommonMark/GFM, and distinguishing
  presentation from structured tool arguments and trusted metadata. The test
  checks Markdown feature strings in both prompts.
  (`agent_runner/src/run.rs:201-235,532-550`)

Only those source/test ranges were inspected. Referenced wrapping/editor/theme
implementations were not inspected. No concrete logic discrepancy was
established within that scope.

## Package and public surface

`agent_runner/Cargo.toml` defines package `agent-runner` 0.2.0, edition 2024,
minimum Rust 1.95, library `agent_runner`, and binaries `agent-runner` and
`agent-runner-file-tool`. The library forbids unsafe code. Its Unix filesystem
and process APIs, `/proc` use, and Linux-only agent-runtime dependency make the
observed implementation Linux-specific.

`agent_runner/src/lib.rs` exposes:

- Configuration: `Cli`, `parse_effort`, `resolve_config_path`, `Config`, `Model`,
  `ModelProvider`, `SandboxPaths`, `ToolPolicy`, `DEFAULT_WRITE_ROOT`;
  `config::ModelBudgets` and configuration bounds are also publicly accessible.
- Conversation and execution: `AgentSession`, `AgentRunner`, `Event`,
  `EventSink`, `ToolExecutor`, `Recorder`, `RunSummary`, `RunLimits`, `MAX_TURNS`,
  `SandboxExecutor`, `HostExecutor`, `ToolOutcome`, and `system_prompt`.
- Context: `ContextManager`, `Compaction`, token/message estimates, and
  `DEFAULT_CONTEXT_TOKENS`; `context::estimate_request` is public.
- Tool construction: `ExecContext`, `RenderedTool`, `ToolRegistry`,
  `describe_tool_call`, `ToolProfile`, `ProfileSetting`, `ToolchainProbe`.
- Native primitives through `file_tools`, sandbox construction/execution
  through `sandbox`, `RetryPolicy` and its defaults, `SessionLog` and
  `DEFAULT_LOG_DIR`, history listing/replay through `history`,
  `headless::run`, and `tui::{Overrides, run}`.
- Kvist profile import: `KVIST_CONFIG_FILE`, `import_models`,
  `resolve_kvist_config_path`; Markdown rendering: `MarkdownStyles`,
  `RenderedLine`, `render_document`; `Error`, `Result`, `init_logging`.

The worker/thread implementation, CLI module, host module, Markdown module,
and import module are private even when selected items are re-exported.
`toolchain::rust_environment::RustEnvironment` is publicly reachable through
the public toolchain module.

## Configuration and startup

`agent_runner/src/config.rs` reads at most 64 KiB of UTF-8 TOML from a regular
file using `O_NOFOLLOW | O_NONBLOCK`. A second stream bound catches growth.
Parse/type diagnostics identify the line without echoing TOML contents.
The raw configuration and its nested model, sandbox, and policy tables reject
unknown fields.

The accepted input shape is:

- `schema_version = 1`;
- optional `working_directory`, otherwise current directory; a configured
  value must be absolute and an existing directory;
- `default_provider` (llama-server or ollama), which MUST name at least one
  `[[models]]` entry;
- optional `default_thinking_effort`, default `medium`;
- one or more `[[models]]` with `id`, `provider`, `model`, optional `base_url`,
  `is_default` (at most one per provider), `context_limit`, `response_reserve`,
  `deadline_secs`, `max_attempts`, `retry_base_delay_secs`,
  `retry_max_delay_secs`, `cadence_timeout_secs`;
- optional `[sandbox]` with `runner` and `backend`;
- optional `[tool_policy]` with `shell_deny_substrings`,
  `shell_deny_prefixes`, `write_root`;
- optional `[tool_profiles]`, keyed by generic/python/rust/javascript/go/c,
  with serde spellings `on`, `auto`, `off`.

IDs must be unique, provider model text nonblank, deadlines 1–600 seconds,
attempts at least one, backoff delays at most 3600 seconds with base no greater
than maximum, cadence 0–600 seconds. Context is 2–1,048,576 tokens; reserve is
1–1,048,576 and must be smaller than an explicitly paired context. There is
no configured upper bound on `max_attempts`; whole-prompt budgets still apply.
`ProfileSetting::parse` accepts extra synonyms, but the TOML serde enum uses
the three canonical spellings. Configuring generic has no gating effect.

Providers are `llama-server` and `ollama`, with default endpoints
`http://127.0.0.1:9931` and `http://127.0.0.1:11434`. Defaults are deadline
300 seconds, three attempts, 2-second base/30-second maximum backoff, and
30-second cadence; zero cadence disables that watchdog. Transport construction
uses agent-runtime's direct transport with an 8 MiB response limit. Loopback
endpoint validation occurs when constructing that transport, not when parsing
TOML. Default executables are `/usr/local/bin/kvist-sandbox-runner` and
`/usr/bin/bwrap`.

`Model::resolve_budgets` selects context and output overrides independently:
CLI, then model configuration, then provider serving-capacity discovery for
context or `clamp(context / 4, 1, 8192)` for output reserve. Missing/failed
discovery asks for explicit capacity; it does not assume 8192 tokens.
Provenance is recorded as CLI/configuration/provider/automatic.
`DEFAULT_CONTEXT_TOKENS = 8192` remains a library constant, not the production
unknown-model fallback. `Config::from_parts` only checks default-model
membership and fills defaults; it is not equivalent to full TOML validation.
Startup canonicalizes the chosen working directory, including relative or
linked CLI overrides, and rejects non-directories.

`agent_runner/src/cli.rs` searches explicit config, current-directory
`agent-runner.toml`, XDG user config, then XDG system directories. Relative
`XDG_CONFIG_HOME` falls back to `HOME/.config`; system paths come from
colon-separated `XDG_CONFIG_DIRS`, default `/etc/xdg`. It does not search
ancestor projects. `--list-models` loads and prints configured models without a
terminal. `--import-kvist` does not need an agent-runner configuration:
`agent_runner/src/kvist_import.rs` extracts `[agent.profiles]`, ignores unrelated
fields and command templates, and prints sorted `[[models]]` snippets for the
two supported providers, with escaped TOML strings and a 120-second import
deadline default. Import is read-only and does not validate a complete target
configuration or enforce the target's 600-second deadline maximum.

Other flags include model, effort, cwd, profile, logs, context/output budgets,
initial prompt, `--headless`, `--json`, `--allow-host-execution`, and
`--host-turns`. Headless requires a prompt and conflicts with host authority,
disabled logs, listing, and import; JSON requires headless. Whole-prompt CLI
defaults are 1800 seconds and 1,000,000 estimated tokens, with maxima 24 hours
and 1,000,000,000 tokens. Error exit codes are normally 1, selection/terminal/
effort/toolchain/host-turn errors 2, policy errors 3
(`agent_runner/src/error.rs`, `agent_runner/src/main.rs`).

Evidence includes `agent_runner/tests/component_tests.rs`:
`loads_a_valid_configuration`, `rejects_unknown_top_level_fields`,
`rejects_duplicate_model_ids`, `rejects_out_of_bound_cadence`; and
`agent_runner/tests/model_budgets.rs`:
`configured_capacity_and_cli_precedence_have_no_8192_fallback`,
`automatic_generation_reserve_has_room_for_reasoning_but_fits_small_windows`,
`unavailable_discovery_requests_explicit_capacity_instead_of_assuming_8192`.

## Tool model and authority

`agent_runner/src/tools.rs` renders rather than executes tools. `ExecContext`
contains host `workdir` and `call_id`; `RenderedTool` contains `argv`, human
`summary`, optional typed `file_request`, and optional `file_helper`.
Definitions are offered in stable order: shell, read_file, write_file, list_dir,
find_files, search_files, edit_file. Language profiles change shell capability
descriptions/resources, not these tool names.

Shell accepts exactly `{"command": <string>}`: nonblank, NUL-free, at most
16 KiB. Rendering produces absolute bash, `-c`, the unchanged command, and
`agent-runner` as argv0. The built-in case-sensitive substring denylist includes
`rm -rf`, `rm -fr`, `mkfs`, `dd if=`, `dd bs=`, `> /dev/`, `:() {`,
`exec 9<>`, `reboot`, `shutdown`; trimmed-start prefix `mknod ` is denied.
Configuration adds denials rather than removing the minimum. This is an
advisory textual filter, not shell parsing or isolation. Policy identity hashes
sorted, NUL-delimited write-root/deny entries. Unknown tools or malformed
arguments fail before executor effects.

`agent_runner/src/toolchain.rs` keeps generic enabled; configurable profiles
default auto. On/forced requires availability, auto omits unavailable profiles,
off excludes them. Non-Rust probing searches PATH, fixed system directories,
and selected home paths, but accepts only executable canonical paths beneath
the mounted system roots `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`; rustup
proxies are not compilers. This is an interpreter probe, not proof that every
named package manager/build tool works. Manifest-based language detection is
advisory. Rust workspace resolution is a separate concrete-resource path.

In normal execution `SandboxExecutor` is injected. `HostExecutor` is an
explicit unconfined alternative, with inherited host environment, privileges,
and network. It performs string replacement of write-root path occurrences in
shell argv, not semantic shell-path confinement. Native operations instead
map the write-root namespace onto the canonical host workdir and retain their
own mutation checks. Host shell calls allow 120 seconds and 8 KiB combined
capture; native helper calls allow 7001 captured bytes
(`agent_runner/src/host.rs`). Neither executor makes operational logs canonical
task evidence.

## Native filesystem operations

`agent_runner/src/file_tools.rs` defines closed `FileRequest` JSON:
`write_root` and `operation`, the latter tagged by `tool` with `arguments`.
Unknown fields/types fail. Request bytes are limited to 256 KiB; paths to
4096 UTF-8 bytes; absolute lexical paths exclude NUL, empty components, `.`,
`..`, and trailing slash except `/`. Mutations must be strictly inside the
write root on a slash boundary. Read operations can address any permitted
absolute path in the executor's namespace, not only the write root.

- `read_file`: path, optional byte offset (default 0), byte limit (default
  4096, maximum 16384). Reads/hashes a complete regular file of at most 1 MiB,
  requires UTF-8 and boundary-aligned offsets, and returns `content`, `sha256`,
  `offset`, `next_offset`, `total_bytes`. It follows read-path symlinks; unlike
  mutations/search, this API is not a no-link traversal.
- `write_file`: path, UTF-8 content up to 64 KiB, optional lowercase
  `sha256:` plus 64-hex `expected_sha256`. May create or replace; an expected
  digest requires an existing matching preimage.
- `edit_file`: path, nonempty `old_text`, `new_text` (each at most 64 KiB),
  mandatory expected digest. Exactly one occurrence, counting overlapping
  matches, must exist. It retains unrelated bytes, CRLF, missing final newline,
  and original mode.
- `list_dir`: path and entry pagination (offset at most 4096, limit default
  100/max 256); returns sorted `entries` with `name` and `kind`, pagination,
  and total count.
- `find_files`: directory path, literal scoped-path substring `pattern`
  (empty permitted, at most 1024 bytes), pagination, `include_generated`.
  Returns sorted `files`, totals, visited/skipped counts, and `complete`.
- `search_files`: directory or one regular file, nonempty literal `query`
  up to 1024 bytes, optional literal `file_pattern` of the same bound,
  pagination, `include_generated`. Filters scoped relative paths, or basename
  for a file scope. Returns sorted path/line matches with one result per
  matching starting line, byte match offset, a UTF-8-safe line prefix up to
  1024 bytes, and truncation/coverage metadata.

All successful helper output, including newline, fits 7000 encoded bytes.
Pages shrink to fit full JSON escaping and metadata; callers must resume at
the returned offset, not requested page size. A single item that cannot fit
fails explicitly. Search/list pagination offsets index results; read offsets
index bytes. Optional typed strings reject explicit null.

Directory traversal uses held descriptors, no-follow components, and
`/proc/self/fd` enumeration. Recursive discovery/search skips links and, by
default, directories named `.git`, `target`, `node_modules`, `vendor`,
`vendored`, `.agent-runner`; explicit generated opt-in and an explicitly chosen
scope remain available. Traversal is bounded to depth 32 and 4096 visited
entries; unsupported ordinary entries or read errors fail rather than silently
becoming an empty result. Search additionally limits each complete file to
1 MiB, total scanned bytes to 8 MiB, and results to 4096; binary/NUL-bearing,
oversized, linked, generated, and filtered exclusions are counted.
`complete` describes coverage exclusions, independently of remaining pages.
Scanned size changes are rejected, but equal-size concurrent changes are not
a snapshot guarantee.

Mutation traverses the write root and parents without links, opens a bounded
regular preimage, validates it, constructs output metadata before effects,
creates an exclusive mode-0600 sibling replacement, writes/chmods/syncs it,
reopens/rechecks bytes/inode/device/mode, renames atomically, and syncs the
parent. Existing mode is preserved; new files use 0644. RAII unlinks unused
replacement names. This is not transactional compare-and-swap against arbitrary
external writers; a race after recheck or an error after rename can leave an
effect despite a failed result. Parent directories must already exist.

`agent_runner/src/bin/file_tool.rs` requires exactly one payload file, opens it
no-follow/nonblocking, verifies bounded regular-file type, parses/revalidates
the request, and emits one JSON line. Failure prints a diagnostic and exits 2.
The helper is not itself a sandbox.

Executed evidence in `agent_runner/tests/native_file_tools.rs` includes
`edits_preserve_unrelated_bytes_crlf_missing_newline_and_mode`,
`stale_zero_multiple_and_overlapping_matches_never_mutate`,
`mutations_reject_traversal_sibling_prefix_and_symlink_parents_or_targets`,
`escaped_control_and_cjk_read_pages_are_complete_json_with_lossless_progress`,
`native_search_generated_sources_are_excluded_and_counted_by_default`,
`native_search_regular_file_scope_preserves_literal_matching_and_link_denials`,
and `oversized_mutation_metadata_is_rejected_before_file_effects`.

## Sandbox construction and process ownership

`agent_runner/src/executor.rs` checks cancellation before rendering, staging,
request construction, and execution. Native argv is `["/context/1",
"/context/0"]`: helper then payload. The helper defaults beside the running
executable; configured or default helper must be a regular non-link path with
non-link ancestors, outside the canonical writable workspace.

Payload staging uses a private mode-0700 generated directory in the workdir's
canonical parent and a synchronized mode-0600 generated file. The model's
path/call ID never chooses host staging names. The parent must be writable;
filesystem-root workspaces cannot stage there. Payload and helper become
read-only, byte-hashed context files; the RAII payload owner outlives execution
and cleans up on success, build failure, or spawn failure.

`agent_runner/src/sandbox.rs` constructs a shared protocol request with
`protocol = kvist-sandbox-request-v1`, version 1, phase Authoring, argv,
write-root working directory, environment, denied network with empty allowed
sources, resources, identities, System toolchain rooted at `/usr`, grants, and
no cache/scratch request objects. It invokes the dependency's local validation;
the dependency implementation was not inspected.

The workdir is canonicalized and granted read-write at the policy write root
(default `/workspace`). Independently installed runner must be outside it.
Runner/backend identities hash regular no-follow files, bounded to 256 MiB.
Backend is identified as Bubblewrap with canonical path/digest. Declared
`read_roots` support regular non-link files only, outside the workspace,
mapped read-only to `/context/N`. Directory authoring identities bind path/
domain data, not a complete workspace content snapshot. Command identity binds
NUL-separated argv; mount-plan identity binds destinations/access/purpose.

Workspace preflight checks scope-escaping symlinks, permits links provably
within scope, follows in-scope directory targets with a visited set, and uses
lexical containment for unresolved targets. Limits are 30 seconds, 1,000,000
entries, depth 128, and 32 MiB retained directory-path accounting, with
cancellation checks throughout. This is a preflight observation, not a
transactional filesystem freeze or comprehensive hardlink/special-file audit.

Requested default tool limits are 120,000 ms, 1 MiB combined output,
256 processes, 4096 files, 64 MiB/file, and 1 GiB scratch.
Environment starts with fixed system PATH and sandbox `/tmp` HOME, accepts
portable caller names, and excludes `LD_*`, uppercase proxy variables, `GIT_*`,
selected Cargo source/registry/HTTP names and registry token. Executing the
request serializes at most 1 MiB JSON to the runner's stdin with
`--kvist-sandbox-request-v1`. Failure never selects a host fallback.

`agent_runner/src/process.rs` is a private single-threaded pump for both
executors and bounded rustup queries. It spawns a new process group, uses
nonblocking stdin/stdout/stderr, 8192-byte chunks and 5-ms idle polls, and shares
one exact capture budget across streams. It drains output while feeding stdin,
closes stdin after the request, and treats a successful early exit before
complete delivery as failure. Ordinary nonzero exits preserve status and partial
bytes rather than becoming spawn/protocol errors.

Cancellation, timeout, and overflow trigger SIGKILL of the owned group and
direct child; the direct kill also covers a leader that moved groups.
`waitid(WNOWAIT)` observes leader exit without recycling its PID before
signalling. Cleanup is attempted on every owned-child exit, with a 250-ms pipe
drain window and 1-second reap window. Retained pipes/escaped descendants
produce explicit errors, not delayed success; escaped descendants and
uninterruptible kernel work cannot be universally terminated.

Construction evidence includes
`agent_runner/tests/component_tests.rs::build_request_produces_a_closed_authoring_request`;
payload evidence includes
`agent_runner/tests/native_file_tools.rs::private_payload_and_helper_are_readonly_hashed_context_files_and_cleaned_after_failure`.
Executed `agent_runner/tests/subprocess_supervision.rs` covers
`sandbox_writes_complete_large_request_without_deadlocking_on_output`,
`successful_early_stdin_close_is_not_false_success`,
`sandbox_retained_pipes_are_explicit_failure_not_delayed_success`,
`independent_calls_do_not_share_cancellation_or_process_groups`, and the
`moved_direct_child_*` cancellation/timeout/overflow cases.

## Installed Rust and offline resources

`agent_runner/src/rust_environment.rs` resolves an existing installation; it
does not install dependencies/toolchains or consume engine toolchain manifests.
It reads only top-level `rust-toolchain.toml` or `rust-toolchain`; both present
is an error. Pins are bounded to 64 KiB. TOML permits channel, components,
targets, profile only. Channel accepts bounded release/version tokens, not
paths/options/custom linked names; component/target lists have at most 32
bounded tokens, profiles minimal/default/complete. Requested components are
checked against a limited supported list and requested target directories.

Trusted `/usr/bin/rustup` is resolved beneath `/usr`, outside the workspace.
Host HOME must be a canonical non-link directory outside it. Rustup queries run
from `/` with cleared environment, HOME, fixed PATH, `RUSTUP_AUTO_INSTALL=0`,
16 KiB output, and remaining preparation time. Absent pin uses host
active-toolchain selection once; explicit pin never silently falls back.
Concrete cargo must be exactly beneath
`HOME/.rustup/toolchains/<selection>/bin/cargo`. Layout, executable cargo/rustc/
rustdoc, native component manifest/libcore/libstd, and system cc/ar/as are
checked and hashed; native library ambiguity/absence fails.

Outside-workspace generated resources include a fixed Cargo wrapper and a
private copy of `.kvist/vendored`, or an empty vendor directory. Snapshot
rejects linked ancestors, links, special files, multi-link files, and observed
file/directory drift. Bounds: 256 MiB/file, 1 GiB total file bytes, 100,000
entries, depth 64, 32 MiB retained paths, 30-second preparation budget. Clone
ownership is shared via Arc; final owner removes staged resources.

Read-only Toolchain-purpose grants map the installation, wrapper runtime and
vendor snapshot to `/rust/toolchain`, `/rust/runtime`, `/rust/vendor`.
PATH prefers the wrapper and concrete installation; RUSTC/RUSTDOC point at the
concrete binaries. HOME, CARGO_HOME, and CARGO_TARGET_DIR use sandbox scratch;
CARGO_NET_OFFLINE=true. The wrapper invokes concrete cargo with `--offline`
and command-line source replacement directed at `/rust/vendor`; it does not
add `--locked`. Missing dependencies require separate host provisioning.
Host homes, credentials, ambient Cargo/Rust overrides, and forged workspace
resource manifests do not become new host grants.

Each tool request rechecks workspace identity, project-pin digest, selected
directory device/inode identities, and selected executable/library/wrapper
hashes. Diagnostics expose selection and digests. This is not a digest of every
toolchain file, nor continuous revalidation of arbitrary host changes. Auto Rust
reports omission; explicit Rust fails startup on resolution failure. Host
opt-out uses the simpler profile resolver, not these sandbox Rust resources.

Native executed tests in `agent_runner/tests/rust_build_environment.rs` were
`native_installed_rust_compiles_links_and_documents_offline`,
`native_vendor_snapshot_overrides_host_paths_without_credentials_or_mutation`,
and `changing_project_pin_after_resolution_fails_before_effects`.
They exercise compiler/linker/rustdoc plus Cargo test/doc, offline vendor
resolution despite misleading project config, snapshot independence,
read-only resources, absent host credential/home mounts, denied host-loopback
access, scratch target location, and changed-pin rejection. These specific
trials do not independently verify all external runner quotas or all platforms.

## Conversation, effects, accounting, and recovery

`agent_runner/src/session.rs` provides the injectable blocking loop.
`AgentSession` owns model, effort, host system prompt, ordered messages, tool
definitions and last answer. Pushing a user clears the previous answer.
Requests prepend the host system prompt, offer Auto tools and configured effort,
and omit output schema. Only a nonblank Stop turn with no tool intents becomes
an answer. The low-level assistant helper folds messages; the runner separately
validates before accepting a turn.

For each prompt, `AgentRunner::run` validates turn count 1–50 and whole-prompt
limits, starts an owned deadline watcher, starts the recorder, performs request
preflight, and then charges estimated request plus reserved output before each
attempt. It never refunds retries based on missing/actual usage. Watcher checks
interrupts/cancellation at bounded intervals (25 ms) and cancels when wall time
expires; injected transports/executors must honor cancellation. Run summary
contains answer, turns, tools_executed, cancelled, exhausted, budget_exhausted,
failure; disposition precedence is budget_exhausted, cancelled, failed,
turn_limit, completed, no_work.

Sequence is request record before provider call; returned turn/reasoning and
usage record before acceptance; whole-turn validation before any tool effects;
synchronized dispatch record before calling an executor; outcome folded into
conversation, recorded, and reported afterwards. Recording or sink errors stop
effects and attempt terminal recording. Unexpected executor errors stop the
prompt; policy/render errors instead become rejected tool results and notices
so the model can recover. Interrupted/unexecuted remaining calls get explicit
rejection results, preserving complete tool groups for later prompts.
Completed tools are not automatically re-executed by transport retries.

Accepted final shape is Stop with nonblank answer/no tools, or ToolCalls with
at least one call. Length, filter, unknown or inconsistent endings cannot
authorize effects. A turn has at most 32 calls; IDs are unique within the turn,
nonempty, at most 256 bytes/NUL-free; names nonempty, at most 128 bytes/NUL-free;
arguments objects with at most 1 MiB encoded bytes. Rendering/transport imposes
additional name/argument restrictions.

Transient agent-runtime errors receive deterministic capped exponential
backoff (default three total attempts). Retries reuse unchanged accepted
history and announce prior streaming text as provisional. Deadline grows
linearly by attempt number, capped by total attempts and remaining prompt time.
Cancellation, malformed responses, bounds and non-transient errors do not
retry. A Length turn can be regenerated before effects with doubled reserve
(up to 1,048,576), only if it fits the current context and shared token charge.
Such partial turns are recorded separately; unsuccessful recovery is not a
final answer.

An action hash ring blocks repeated non-read arguments before effects and
eventually trips a four-stall circuit breaker. Native read/list/find/search are
exempt. Repeated read_file without offset receives a byte-pagination notice.
Executed non-read actions enter history; successful outcomes reset stalls but
do not erase repeat history. The runner does not use observation hashing or
reasoning similarity, and it does not change provider temperature when the
shared detector returns a temperature-jitter decision. Its notices explicitly
do not claim filesystem change was observed.

Model tool results include process flags/status and a combined encoded preview
bounded to 8 KiB, with truncation notice. Binary/NUL/invalid-UTF8 output becomes
byte count and SHA-256 rather than expanded binary text. This preview is
separate from raw captured bytes and transcript limits. `ToolOutcome::failed`
requires an observed zero status without timeout/cancel/overflow.

Events are tagged `{type, data}` snake_case: turn_start, attempt_start,
reasoning, text, tool_call, tool_result, finished, prompt_end, failed, note,
progress. Proposals are not execution acknowledgements. Progress reports
provider/estimated/unavailable provenance; streamed output is provisionally
estimated from character count, emitted at roughly 250-ms intervals. Accepted
turn usage accumulates provider totals, with incomplete usage marked unavailable.
These displayed totals differ from conservative budget charges and context
estimates. Terminal answer is cleared on failure/cancel/budget exhaustion.

Executed loop evidence includes `agent_runner/tests/loop_integration.rs`:
`hardening_duplicate_call_ids_reject_the_entire_turn`,
`hardening_record_failure_precedes_effects_and_closes_unsuccessfully`,
`cancelled_multicall_turn_is_valid_for_a_subsequent_prompt`,
`reliability_length_is_regenerated_before_any_tool_effects`,
`reliability_length_recovery_respects_window_and_shared_token_budgets`,
`reliability_legitimate_native_rereads_do_not_trip_effect_breakers`,
`a_follow_up_prompt_carries_the_earlier_prompt_in_context`, and
`each_retry_grants_a_larger_and_capped_budget`.

## Context and durable records

`agent_runner/src/context.rs` estimates four UTF-8 bytes/token, full serialized
canonical request including escaping/schema/optional fields, plus fixed
request/message/tool framing. This is not a tokenizer or guaranteed upper
bound. Serialization failure estimates usize::MAX.

`ContextManager::prepare` validates complete assistant/tool-result groups even
for small requests, preserves all system messages, latest genuine user goal,
outstanding user goals since the latest nonblank text-only assistant completion,
and newest safe group. Each call has exactly one correctly named result;
duplicate/orphan/incomplete/interrupted groups fail. Turn-local IDs may recur
in later complete groups.

Preflight sets the output bound and requires full estimated input plus reserve
at or below context limit. Warmup is approximately 75%; model-aware retention
uses available capacity rather than a fixed six-group cap and targets 65%
after compaction when possible. Older eligible groups become bounded
lossy/non-authoritative User history, never System instructions. The rolling
summary is at most 4000 characters and further shrinks/vanishes to fit.
Irreducible excess errors without changing request/manager state. Legacy
`compact`, `estimate_messages`, and `AgentSession::maybe_compact` do not provide
this complete-request/reserve/group guarantee.

Executed `agent_runner/tests/context_preflight.rs` includes
`serialized_estimate_accounts_for_every_canonical_field_and_framing`,
`malformed_groups_fail_even_when_the_request_is_small`,
`an_error_after_a_success_does_not_change_the_rolling_summary`,
`reliability_continuation_retains_the_complete_outstanding_original_goal`,
and `reliability_large_window_retains_history_according_to_capacity`.

`agent_runner/src/session_log.rs` opens generated no-clobber 0600 journal/
transcript files under a private final 0700 directory, traversing all components
no-follow with held descriptors. It rejects `.`, `..`, links, or nonprivate
final directory; existing ancestor directory privacy is not universally
required. Directory creation and effect-boundary writes are synchronized.
Envelope is schema_version 1, monotonic sequence, event.

Records include session_start (prompt ordinal, IDs, scope, metadata,
canonical_evidence=false), model_request (attempt, request hash, model, counts,
tool names, bound, estimate), turn_start (text/reasoning hashes, response/model/
provider identities), turn_finish (usage/reason), tool_dispatch (call/name,
argument shape, action hash), tool_result (output hashes, process flags,
captured bytes, state_mutated=null), notice, session_finish (disposition,
counts, failure, success, usage completeness, estimated budget charge).
Argument values and raw output are not in the structured tool record; notices/
diagnostics and metadata can still contain sensitive text. The transcript
retains user/answer/reasoning/tool diagnostic text, bounded to 64 KiB per text
item, not a total-session byte cap. It is potentially sensitive.

Each submitted prompt resets accounting within a worker's shared log.
Pending/unreported attempt usage makes final provider totals unknown; estimates
remain separately reported. Dispatch and terminal finish sync both files and
directory. A dispatch without result is an unknown effect, not a replayable
checkpoint. Default interactive logs are within the writable workspace and
explicitly not protected evidence.

`agent_runner/src/history.rs` reads diagnostic transcripts only, newest filename
first, with 4096 directory-entry and 5 MiB/file limits, no-link descriptor reads,
growth bounds, and lossy UTF-8 replay. Missing/unusable histories do not block
the UI. Replay is viewing, not command execution or model-state restoration.
Unit evidence includes `private_no_clobber_files_and_versioned_records`,
`argument_values_and_output_are_not_in_journal_and_mutation_is_unknown`
in `agent_runner/src/session_log.rs`, and
`history_rejects_link_targets_and_link_ancestors` in
`agent_runner/src/history.rs`.

## Headless and terminal behavior

`agent_runner/src/headless.rs` accepts one nonblank prompt of at most 64 KiB,
requires recording, and forbids host execution. It canonicalizes workdir and
requires log paths outside its writable scope, with no dot traversal and
SessionLog's no-link/private-directory checks. Default logs use absolute
XDG_STATE_HOME or HOME/.local/state plus `agent-runner/runs`; unlike config
discovery, relative state base is an error.

JSON emits ordered flushed NDJSON envelopes with schema_version 1/sequence,
starting run_start (scope/budgets/provenance/canonical_evidence=false), loop
events, and final run_summary with disposition. Plain mode writes only a
successful final answer to stdout; notices/diagnostics go to stderr.
Returned loop errors are emitted as failed summaries before error propagation;
startup failures do not manufacture success events. Main returns 0 for success,
130 for cancellation summaries, otherwise 1; direct startup/errors use error
mapping. Controls other than newline/tab are visibly escaped in plain output
and error descriptions; JSON retains original text via JSON escaping and
private transcripts retain original text.
`agent_runner/tests/headless_cli.rs::headless_returns_ordered_versioned_events_without_a_terminal`,
`headless_truncation_has_an_unsuccessful_final_disposition_and_exit`,
`plain_answer_escapes_terminal_commands_but_private_text_is_preserved`,
`headless_rejects_agent_writable_logs_before_model_io`, and
`startup_uses_selected_serving_capacity_without_a_cli_override` were executed.

`agent_runner/src/tui/mod.rs` requires terminal stdin, initializes raw mode,
alternate screen, mouse capture and Ratatui. Sandboxed prompts permit 50 turns;
host opt-out defaults to one and accepts explicit 1–50 caps. Scope remains
visible in UI/record/system prompt; host mode does not claim confinement.
`agent_runner/src/run.rs` owns a blocking worker, 128-slot event channel,
unbounded prompt channel, cancellation token, and context across prompts.
Drop cancels, stops backpressure/waits, and joins. Cancel resets the token for
later prompts; a fatal recorder/channel error ends the worker. Model/effort
changes start a fresh worker on next submission.

`agent_runner/src/tui/app.rs` bounds submissions to 16,384 characters, keeps
up to 5000 visible transcript rows, buffers streamed paragraphs/reasoning,
shows provisional retry notices, completion/failure/cancellation/cap status,
spinner, estimated context/ETA, provenance-specific speed, scrolling and
collapse/reveal reasoning.
The subsequently observed `App::stats_line` rendering shows the output-only
generation speed (`N t/s`) alongside the session-wide average (`avg N t/s`),
marks a provisional streamed speed with a `~` prefix, and renders `? t/s` when
usage is unavailable rather than presenting zero as an estimate. Cumulative
processed tokens use short human-readable units (`123.4k tok`); the compaction
field is shown only while the live context is past the warm-up threshold; and
every field is padded to a fixed width so the columns stay put as magnitudes
change. This rendering refinement was observed by reading only that function
after the test executions recorded below; those executions are not claimed as
verification of this subsequent rendering change.
Ctrl+Enter submits; Enter inserts a newline except on a blank line after the
first; Shift+Enter inserts; Tab/Shift+Tab select model/effort; Ctrl+C cancels
when running and quits when idle; Ctrl+Q quits; Ctrl+D quits with empty editor.
Esc opens menu/history/replay, Ctrl+H help, Ctrl+P/N prompt history, Ctrl+L
clear, Ctrl+T reasoning view, Ctrl+S scrollbar. Explicit copy emits an OSC 52
base64 clipboard sequence, suppressing duplicate copies.

Menu “New session” currently clears display/status; it does not notify the
worker to reset conversation/context. Prompt history and queued prompts are not
globally byte-bounded. Visible-line limits are not a comprehensive cap on
pending Markdown/reasoning/source buffers. Terminal teardown is explicit on
the normal setup/run path, not a catch-all restoration guard for every partial
setup failure. Interactive UI completion is not itself a prompt-success exit
certification. These are source observations, not terminal trials.

`agent_runner/src/markdown.rs` uses pulldown-cmark/syntect for headings,
emphasis, code, quotes, lists/task lists, tables, links, strikethrough, and rules,
wrapping styled rows and code/table tails. Tests use a TestBackend/pure rendering,
not a real interactive terminal. Worker tests
`worker_drop_joins_with_a_retained_prompt_sender` and
`worker_drop_joins_when_the_event_queue_is_full` were executed from
`agent_runner/src/run.rs`. Logging initializes once, stderr only, using
AGENT_RUNNER_LOG then RUST_LOG, default warn (debug in unit tests), ANSI only
for terminal stderr (`agent_runner/src/logging.rs`).

## Executed verification and limits

All commands used `TMPDIR` pointing to project-local fixture storage; that
storage and observation-only captured outputs were removed before this record
was written. No source/test edits were made by this observer.

1. `cargo test -p agent-runner -p agent-runtime --offline --locked` initially
   stopped in runner loop integration: 39 passed, one failed.
   `reliability_successful_distinct_effect_resets_repeat_stalls` observed three
   executed calls versus the compiled assertion's expectation of two.
   A later permitted source read showed that assertion expecting three.
   The targeted rerun passed (1 passed, 39 filtered). The observation therefore
   spans a changing workspace, not an immutable source snapshot; the initial
   failure is retained here rather than retroactively called successful.
2. `cargo test -p agent-runner --offline --locked --test model_budgets
   --test native_file_tools --test rust_build_environment
   --test subprocess_supervision` passed: 3, 36, 3, and 26 tests respectively;
   three native Rust tests remained ignored in that invocation.
3. `KVIST_RUST_TEST_RUNNER=/opt/proj/kvist/target/rust-environment-runner/debug/kvist-sandbox-runner
   cargo test -p agent-runner --offline --locked --test rust_build_environment
   -- --include-ignored` passed all six tests, including the three named native
   trials above. The executable was used, not inspected.
4. Final `cargo test -p agent-runner --offline --locked` passed:

   | Test target | Passed | Ignored |
   | --- | ---: | ---: |
   | Library unit tests | 190 | 0 |
   | component_tests | 35 | 0 |
   | context_preflight | 20 | 0 |
   | headless_cli | 5 | 0 |
   | input_boundaries | 3 | 0 |
   | live_llama | 0 | 3 |
   | loop_integration | 40 | 0 |
   | model_budgets | 3 | 0 |
   | native_file_tools | 36 | 0 |
   | rust_build_environment | 3 | 3 |
   | subprocess_supervision | 26 | 0 |

   Total 361 passed, six ignored, zero failures; both binary unit targets and
   doctests ran zero tests. A separate runner doctest invocation also passed
   with zero tests.

The live llama-server/model-driven sandbox tests in
`agent_runner/tests/live_llama.rs` were not enabled. Native Rust trials do not
substitute for those inference workflows. No formatter/linter, other packages'
tests, real interactive terminal session, or comprehensive external sandbox
resource-limit assessment was performed. Named coverage above is source/test
evidence backed by the executions described here, not a compliance conclusion.

## Subsequent observation: native-page encoding and compacted references

This source/test refresh supersedes earlier descriptions of the native output
size check and the argument prefix retained in compacted summaries. It changes
no observation about the runtime package and makes no compliance conclusion.

### Native output encoding

In `agent_runner/src/file_tools.rs::output_fits`, `MAX_OUTPUT_BYTES` is 7000.
The function serializes the complete outcome to a JSON string and rejects a
raw encoding of 7000 bytes or more. It then serializes that string as a JSON
string value, including quotes and escaping, and requires this nested encoding
also to be strictly below 7000 bytes. Either serialization failure returns a
file-outcome error. `ensure_output_bound` rejects an outcome that fails this
check; read-page and listing-page sizing also use `output_fits`.
The limit is therefore not merely a raw-JSON length check, nor a bound on the
entire enclosing model request.

`agent_runner/tests/loop_integration.rs::reliability_native_page_metadata_survives_outer_model_json_escaping`
uses repeated quote, backslash, and newline content, generates a native read
page, records it into a session, and parses the resulting model-facing payload
as complete JSON. It checks retained digest, byte-based `next_offset`, total
file size, and a serialized complete tool-result content length at most 8192
bytes.

### Compacted argument and result references

`agent_runner/src/context.rs::turn_summary` now emits an explicit `path=`
reference for a string-valued tool argument path before the short serialized
full-arguments prefix. The path is JSON-string escaped and uses
`truncate(path, 4096)`; this retains the first 4096 Unicode characters and adds
a leading `...` when truncated, rather than guaranteeing a final reference of
at most 4096 characters or bytes. The full-arguments prefix remains
`truncate(json, 100)`.
For tool results, the function attempts to parse JSON after the first newline
and emits available `path`, `offset`, `next_offset`, `total_bytes`, and `sha256`
fields before a short result preview. These references remain summary text,
not execution authority. Aggregate summary limits and subsequent shrinking
still apply; complete retention of every long path is not guaranteed.

`agent_runner/tests/context_preflight.rs::reliability_compacted_native_references_precede_lossy_body_prefixes`
places a long path after a 200-character argument field and combines an
8000-character result body with pagination/digest metadata. After compaction,
it checks retention of the complete fixture path, both offsets, digest, and
process status despite lossy body/argument previews.

### Additional executed evidence

Using project-local fixture storage via `TMPDIR`, independently executed:

- `cargo test -p agent-runner --offline --locked --test loop_integration reliability_native_page_metadata --quiet`:
  one passed, zero failed/ignored, 40 filtered out.
- `cargo test -p agent-runner --offline --locked --test context_preflight reliability_compacted_native_references --quiet`:
  one passed, zero failed/ignored, 20 filtered out.

These are targeted executions after the two refinements, not reruns of the
earlier complete runner suite. The local fixture directory was removed.

## Subsequent observation: locked Rust authoring

This refresh supersedes the earlier observation that the generated Cargo
wrapper does not add `--locked`. Unaffected implementation observations and
their separately recorded execution evidence remain unchanged.

### Current wrapper and diagnostic

`agent_runner/src/rust_environment.rs:600–611` creates the trusted Cargo shim
with `create_new`, mode `0500`, synchronizes it, and adds its hash to the
executables revalidated before execution. The script now executes:

```text
/rust/toolchain/bin/cargo --offline --locked --config 'source.crates-io.replace-with="vendored-sources"' --config 'source.vendored-sources.directory="/rust/vendor"' "$@"
```

Thus ordinary calls through this wrapper receive `--locked` even when the
caller supplies neither `--offline` nor `--locked`. The fixed offline vendor
replacement is retained. `diagnostic` at
`agent_runner/src/rust_environment.rs:629–652` now explicitly describes Cargo
as offline and locked and instructs separate provisioning of a matching
`Cargo.lock`. The source does not introduce automatic lockfile generation or
repair. This is a wrapper option, not a read-only filesystem grant for the
workspace lockfile or proof about every Cargo subcommand or explicit write.

### Current regression coverage

`agent_runner/tests/rust_build_environment.rs::normal_cargo_requires_matching_lock_without_creating_or_updating_it`
constructs a sandbox executor using an explicitly selected runner and
`/usr/bin/bwrap`. Without caller-supplied lock flags, it checks:

- `cargo check` fails for a missing lockfile, reports `--locked`, and does not
  create the lockfile.
- `cargo build` and `cargo metadata --format-version=1` reject a stale package
  version, report `--locked`, and leave the stale bytes unchanged.
- A matching lock permits `cargo --version`, `cargo check`, and
  `cargo metadata --format-version=1 --no-deps`; lock bytes stay unchanged and
  no workspace `target` directory appears.

This test is normally ignored because it requires the native runner and
Bubblewrap. The existing installed-toolchain and vendor-snapshot native
fixtures now explicitly provide matching lockfiles
(`agent_runner/tests/rust_build_environment.rs:161,257–261`) before their
compilation/documentation trials.

### Supplied execution evidence inspected

The following files were explicitly supplied as execution evidence, not
intent or review material. They were read directly; this refresh did not
independently rerun their commands.

- `/home/stefan/.copilot/session-state/b6381b27-3338-4699-8a06-e12c43d5ce0e/files/runner-locked-regression-before.txt`:
  the new lock regression failed at the missing-lock assertion; zero passed,
  one failed, six filtered out. This is the observed before-result, not a
  claim that the present source still fails.
- `/home/stefan/.copilot/session-state/b6381b27-3338-4699-8a06-e12c43d5ce0e/files/runner-locked-native-after.txt`:
  all seven `rust_build_environment` tests passed, zero failed/ignored/filtered.
  The names include the new lock regression, installed-toolchain build/doc,
  vendor-snapshot, pin-drift, invalid-pin, absent-install, and symlink tests.
  Current source has three ordinary tests and four normally ignored native
  tests, all represented in that output.

These logs show the specific native regression and target results, not a
complete assessment of external sandbox enforcement. No result is claimed
here for the further full affected suite, strict lint, or release rebuild
reported as in progress.
<!-- kvist-implementation-record-version: 1 -->

# Component Implementation Record
