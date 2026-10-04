<!-- kvist-contract-version: 1 -->

# Agent Runner — Contract

## Run reliability extension

Models may declare `context_limit` and `response_reserve`. CLI values take
precedence; otherwise context is discovered for the selected provider/model,
and generation reserves up to 8192 tokens, at most one quarter of a small
window. Unknown serving capacity fails explicitly rather than assuming 8192.
Capacity discovery is local model transport, not sandbox tool networking.
Startup reports the resolved context/reserve and their provenance; model
switching resolves them again.

Before effects, a `length` generation may be regenerated up to the configured
attempt budget with a doubled reserve when the complete request still fits.
Partial output is provisional and never executable. All attempts share prompt
wall/token limits. Final answers require an ordinary complete stop.

Compaction preserves outstanding user requests verbatim until a successful
answer ends that task, uses a 75% trigger and capacity-based retention target,
and labels summaries as lossy history. Tool previews bound serialized cost and
mark binary/truncated content. Native reads may repeat; opaque/effectful
actions retain a finite repeat breaker with consecutive-stall reset on a
successful distinct action.

Native search/discovery default to excluding generated dependency/build/VCS
and runner-log directories, expose `include_generated` to include them, and
accept bounded literal `file_pattern` filtering. Search additionally accepts
a regular-file scope. Results identify excluded and oversized coverage; direct
reads remain bounded and mutation link restrictions are unchanged.

Rust resource selection occurs on the host before tools and consumes only
installed resources. The effect sandbox receives a validated concrete
toolchain and vendored resolver read-only, with private scratch and denied
network. It never receives the user's home, rustup settings or credentials.
These resources are not engine task approval or compliance evidence.

Journals retain bounded notices and request estimates, nullable provider usage,
capacity provenance and terminal failures. Readable terminal records use
per-prompt turns/time, identify unavailable usage and include the failure.

## Runtime hardening extension

The pre-release API has no compatibility or migration promise.

`agent-runner --headless PROMPT` runs without a terminal. `--json` selects
NDJSON envelopes `{schema_version: 1, sequence: N, event: ...}` with a final
run-summary event. Human diagnostics go to stderr. Headless mode rejects
`--allow-host-execution`, `--host-turns`, and `--no-logs`; it uses exactly the
same sandboxed loop as the UI, not the engine's protected task broker.
Neither interface authorizes engine tasks or creates canonical evidence.
The workspace remains fully writable; protected task execution must use the
engine's narrow broker, never this workspace shell.

`--response-reserve TOKENS` sets the initial generation reserve; absent values
use per-model configuration or a window-aware default. `--max-run-secs SECONDS`
defaults to 86,400 (24 hours) and `--max-run-tokens TOKENS` defaults to
100,000,000 estimated tokens, so a long task runs uninterrupted to a result;
pass smaller values to bound individual runs.
The context limit must be greater than the reserve. Every model request
is preflighted using complete canonical serialization and byte-aware heuristic
accounting, and carries the current attempt's explicit provider output bound.
Oversized immutable/current context is a typed failure before I/O. Compaction
preserves system instructions, the current user goal, and complete
assistant/tool-result groups; summaries are explicitly lossy and nonbinding.
Combined model-facing stdout/stderr and result metadata fit a single bounded
preview; omitted bytes are marked, and every result includes process status.

Recorder operations return `Result`; dispatch acknowledgment precedes effects.
The local journal has a versioned envelope and records content-derived action
identities, process flags, and an explicit disposition, without claiming
filesystem mutation from stdout. Arguments are represented by their hashes and
shape, not raw values. Private transcripts may contain user/model/tool text and
are not guaranteed secret-free. Files are created with no-clobber/private
permissions and are synchronized at dispatch and terminal boundaries.
Session-start metadata identifies the execution scope, workspace, policy digest,
context limit and prompt budgets. Missing library-supplied metadata is explicitly
unspecified. The UI persistently labels sandboxed or HOST UNCONFINED execution;
host-mode model instructions do not promise sandbox confinement.
Required headless logs must be outside the writable workspace. Startup/output
failures may prevent the final NDJSON envelope. A dispatched
call without a result is an unknown effect; there is no automatic replay/resume.

`RunSummary` exposes answer, turns, executed tools, cancelled/exhausted flags
and an optional failure diagnostic. Only a normal `stop` with no tools supplies
a final answer; exhausted/unexpandable length, filtering, unknown finishes, duplicate call IDs and
invalid finish/tool combinations fail without tool execution. Rejected tools
are nonterminal notices with paired error results. Cancellation/budget limits
close every pending call with an explicit not-executed result. Retries receive
distinct attempt notices; partial streamed text is provisional. All waits and
attempt deadlines are clipped to the prompt deadline.

The registry exposes `shell`, `read_file`, `write_file`, `list_dir`,
`find_files`, `search_files`, and `edit_file`, in that stable order.
`read_file` accepts byte `offset` and bounded `limit`; it returns text, digest,
and next-offset metadata. `list_dir`, search and discovery return bounded,
deterministically sorted pages. `edit_file` requires `path`, `old_text`,
`new_text` and `expected_sha256`; it performs one exact replacement only when
the file's digest matches. A stale/no-match/multiple-match result changes
nothing. File tools use the separately installed `agent-runner-file-tool`
Rust helper, mounted read-only outside the writable scope, and private
read-only payload grants. No model-selected path or call ID is used for host
staging. Mutation paths are canonical, confined, and non-link. External
writers are not participants in a transactional lock; preimage checks detect
stale content but do not promise atomic compare-and-swap against arbitrary
external writers.

## Independent review extension

`agent-runner --headless PROMPT --review` appends a single independent review
phase to a successful run. The review is a second, completely fresh session —
new conversation, new model context, new private journal — that receives only
the review system prompt, the original task prompt, the implementation run's
final answer, and the workspace. It receives no implementation transcript,
summary, or compaction history. A run whose disposition is not `completed`
skips the review with a diagnostic note.

The `[review]` table configures the phase (all keys optional; the phase is off
unless `enabled = true` or `--review` is passed):

```toml
[review]
enabled = true                 # phase off by default
model = "reviewer"             # reviewer model id; defaults to the implementation model
thinking_effort = "high"       # reviewer thinking effort; defaults to the session default
apply_fixes = false            # allow the reviewer to edit files after assessing
max_turns = 200                # review turn cap (1..=500)
on_failure = "warn"            # "warn" | "fail" when the review run itself fails
```

`model` MUST name a configured `[[models]]` id and MUST NOT be written to the
configuration file. `--review-model <ID>` and `--review-apply-fixes` override
the configuration for one run; `--no-review` disables the phase for one run
regardless of configuration.

The review executes under the identical sandbox request shape, grants, and
network denial as the implementation run. In the default assess-only mode the
native write tools (`write_file`, `edit_file`) are not advertised to the
reviewer; the shell remains available so the reviewer can build and test, and
any shell write in assess-only mode is a reportable deviation, not a granted
capability. With `apply_fixes = true` the full tool set is advertised and the
reviewer MUST assess first, fix, re-run builds and tests, and report both
states.

NDJSON adds two ordered events around the phase: `review_start` (reviewer
model, implementation model, mode, budgets) and `review_summary` (`disposition`,
`verdict`, `findings` as bounded `{severity, category, file, summary}` entries,
`fixes_applied`, `tests_passing`, `assessment`, `report_parsed`). Plain output
prints the implementation answer and then the assessment text. A review
failure under `on_failure = "warn"` (the default) leaves the implementation
disposition and exit status unchanged; under `on_failure = "fail"` the process
exits unsuccessfully after reporting it.

Review journals are named with a `-review-` marker in the record stem, are
private and no-clobber, live outside the writable workspace, and their
metadata records the implementation model, the reviewer model, and the mode.
The review is advisory: it authorizes no engine task, mints no canonical
evidence, determines no compliance, and its report is not an acceptance
receipt. The implementation answer embedded in the review prompt is untrusted
data. The phase adds no persistent state beyond the journal; reruns review the
current workspace and are never resumed or cached.

## Boundary and ownership

This document defines what `agent-runner` exposes to consumers: the library
public API, the configuration schema, the command-line interface, and the exact
sandbox request it produces. Private algorithm choices live in `DESIGN.md`.

## Model selection and activation

`agent-runner` MUST prefer a model that the provider already has loaded over a
statically configured one, so an already-active model is used without paying a
model load/switch. When no explicit model is selected for the session, the
active model (if any) MUST take precedence over `default_model`. The active
model is discovered by a bounded, read-only, loopback-only probe of each
configured provider (Ollama `GET /api/ps` for the loaded model and serving
context; llama-server model-qualified `GET /props?model=…`), reusing the
existing discovery bounds (5 s, 1 MiB) and MUST NOT send inference, modify the
provider, or widen endpoint authority. A probe failure (provider down,
malformed, or no model loaded) MUST NOT fail startup and MUST fall through to
the next rule. The selection precedence MUST be:

1. An explicit selection wins and is never overridden by provider state: the
   `--model` CLI flag (which `kvist prompt` passes for the role's profile) and
   the in-TUI Tab model selector.
2. Otherwise, if a probe finds a loaded model whose provider-facing name
   matches a configured entry's `model` field (matched by provider model name,
   not the user-facing `id`), that entry MUST be selected, and the session MUST
   announce that it uses the already-active model with no switch. When several
   providers are configured and more than one reports a loaded match, the
   result is ambiguous and MUST fall to rule 4 (ask, or fail headless).
3. Otherwise, if exactly one model is configured, it MUST be auto-selected as a
   convenience. This is the only case where the session starts without an
   explicit choice and without an active-provider match.
4. Otherwise (no active match and several configured models), the session MUST
   not silently guess:
   - Interactive (TUI): start with no model selected; the Tab selector and the
     existing prompt-hold machinery apply, so no bootstrap or model load occurs
     until the user selects a model and submits.
   - Headless: fail fast before provider inference with an actionable diagnostic
     that lists the configured ids, names any active provider model found, and
     suggests `--model` and the `[[models]]` entry to add.

`default_model` is a required, last-resort configuration field. It is used only
when rules 1–3 resolve nothing (for example, several configured models with no
active-provider match in a mode that does not defer to the user), and MUST NOT
override an active model, an explicit selection, or a single configured model.
When the provider has an active model that is not present in `[[models]]`, the
tool SHOULD offer to configure it: it MAY print a ready-to-paste `[[models]]`
entry (provider, base_url, provider model name, and sane defaults) derived the
same way as `--import-kvist`, and MUST NOT modify the configuration file
automatically.

## Provided interfaces

All public items live in the `agent_runner` crate and are re-exported from
`lib.rs`. Consumers (the `agent-runner` binary, and Kvist as a future caller)
interact only through these items.

### `Error` / `Result`

`agent_runner::Error` is the crate domain error type. `agent_runner::Result<T>`
is `Result<T, Error>`. Errors carry an `exit_code() -> u8` and a `describe() ->
String` that produces an actionable, non-secret message. Errors never unwrap the
underlying source when printing; formatting failures degrade gracefully.

### `Config`

```
struct Config {
    schema_version: u32,                       // required, == 1
    working_directory: PathBuf,                // required, absolute, exists
    default_model: String,                     // required, names a model in `models`; last-resort fallback, see "Model selection and activation"
    default_thinking_effort: ReasoningEffort,  // required
    models: Vec<Model>,                        // required, >= 1, unique ids
    tool_policy: ToolPolicy,                   // required
    tool_profiles: BTreeMap<ToolProfile, ProfileSetting>,  // per-language gating
    sandbox: SandboxPaths,                     // required
    review: ReviewPolicy,                      // optional independent review phase; off by default
}
```

- `Model { id: String, provider: ModelProvider, base_url: String, model: String,
context_limit: Option<usize>, response_reserve: Option<u32>,
deadline_secs: u64, max_attempts: u32, retry_base_delay_secs: u64,
retry_max_delay_secs: u64, cadence_timeout_secs: u64 }` — `provider` is one of
  `llama-server`, `ollama`. The `id` is the user-facing selector; `model` is the
  provider-facing selector. The retry fields bound transient-failure recovery for a
  turn (see `AgentRunner`); `max_attempts` defaults to `DEFAULT_MAX_ATTEMPTS`,
  delays to `DEFAULT_RETRY_BASE_DELAY` / `DEFAULT_RETRY_MAX_DELAY`. `deadline_secs`
  is the per-turn generation budget (default 300s). `cadence_timeout_secs` sets the
  inter-token cadence watchdog: a turn that sends no token for this many seconds
  after the first token is treated as stalled and retried, so a generous
  `deadline_secs` cannot become a silent multi-minute hang; `0` disables the
  watchdog (default 30s).
- `ReviewPolicy { enabled: bool, model: Option<String>, thinking_effort:
Option<ReasoningEffort>, apply_fixes: bool, max_turns: u32, on_failure:
ReviewFailurePolicy }` — every field is optional in the file; an absent table
  means the phase is disabled. `model` names a configured model id; `max_turns`
  is bounded 1..=500; `on_failure` is `warn` (default) or `fail`. Unknown
  `[review]` keys fail at load like other unknown keys. See "Independent review
  extension".
- `SandboxPaths { runner: PathBuf, backend: PathBuf }` — absolute paths to the
  `kvist-sandbox-runner` executable and the Bubblewrap backend. Either may point
  at a binary on disk; both are hashed at request construction and the backend
  is re-verified at runtime by the runner.
- `ReasoningEffort` mirrors `agent_runtime::ReasoningEffort`
  (`none|minimal|low|medium|high|xhigh|max`) with `FromStr`/`as_str`.

`Config::load(path)` reads and validates a TOML file, returns `Err` on missing,
wrong `schema_version`, unknown model selector, circular/invalid tool policy,
or any bound violation. `Config::from_parts(...)` builds an in-memory config for
tests. The default working directory is the process current directory when not
specified.
Configuration reads use a held regular non-link descriptor and a 64-KiB
read-time byte bound, not only a pathname metadata check.

### `ToolRegistry`, `ExecContext`, and `RenderedTool`

`agent_runner::tools::ToolRegistry` owns the model-facing tool definitions and
renders a sandbox command for an approved tool intent.

```
struct ToolRegistry { /* bash, policy, profiles */ }
```

- `ToolRegistry::new(policy: ToolPolicy) -> ToolRegistry` builds a registry
  advertising only the fixed generic base, without probing or claiming language
  availability. Production language advertisement uses `resolve`.
- `ToolRegistry::resolve(policy, settings, probe, forced) ->
Result<ToolRegistry>` resolves the canonical `bash` path and the honest, gated
  profile set (see `ToolProfile`, `ProfileSetting`, `ToolchainProbe`,
  `resolve_profiles`). `Generic` is always advertised; each configurable profile
  follows its setting — `On` (including a `forced` CLI profile) advertises the
  profile and fails with `Error::ToolchainUnavailable` if its interpreter does not
  reach the sandbox, `Auto` advertises only when available, and `Off` never does.
  `resolve` fails if `bash` is missing or a requested profile is unavailable.
- `ToolRegistry::with_profiles(self, Vec<ToolProfile>)` narrows the advertised
  profiles (used by tests); otherwise `new`/`resolve` decide the set.
- `ToolRegistry::tool_definitions(&self) -> Vec<ToolDefinition>` — the
  `agent_runtime::ToolDefinition` list exposed to the model (stable order).
- `ToolRegistry::profiles(&self) -> Vec<&'static str>` — the enabled profile
  identifiers, sorted for determinism.
- `ToolRegistry::render(&self, intent: &ToolIntent, context: &ExecContext) ->
Result<RenderedTool>` — maps a model tool intent to an argv to execute inside
  the sandbox, or `Err` when the tool is unknown, the arguments are malformed, or
  the call violates policy.
- `RenderedTool { argv: Vec<String>, summary: String, file_request: Option<FileRequest>,
file_helper: Option<PathBuf>, shell_script: Option<String> }` —
  `argv[0]` is an absolute canonical path; `summary` is a short human description
  shown in the UI and logs. Native operations carry a typed payload and helper
  path, not files already written to the workspace. `shell_script` is `Some`
  only for shell commands longer than one 4096-byte argv entry: the executor
  stages the exact command text as a private regular file, which the request
  mounts read-only at `/context/0`, and `argv` reads it via the wrapper
  `exec bash -c "$(cat /context/0)" agent-runner`; it is never set together
  with `file_request`, and the semantics (script text, exit status, `$0`)
  match the inline form exactly.
- `ExecContext { workdir: PathBuf, call_id: String }` supplies the renderer the
  host working directory and a descriptive per-call id. IDs never select
  staging paths.
- `ToolRegistry::with_file_helper(path)` explicitly selects the installed helper.
  Production resolution defaults to `agent-runner-file-tool` adjacent to the
  calling executable. The executor rejects absent, nonregular, linked or
  workspace-contained helpers; it never falls back to host execution.
- `ToolProfile` (ids `generic`, `python`, `rust`, `javascript`, `go`, `c`)
  surfaces the relevant package and build tools for each language; `Generic` is
  the always-present base and is never configured or gated (see
  `Tool profiles, settings, and detection`).

### Native file-tool inputs and results

All argument objects are closed. Paths are canonical absolute UTF-8, at most
4096 bytes, without NUL, traversal, duplicate separators or trailing separators
except `/`. Digests are `sha256:` plus 64 lowercase hexadecimal digits.

| Tool           | Required arguments                                         | Optional arguments                                                                                                  |
| -------------- | ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `shell`        | `command`: nonblank, NUL-free, at most 16384 bytes         | None. Commands over 4096 bytes execute as a staged `/context/0` script file, so the full bound is always executable |
| `read_file`    | `path`                                                     | `offset=0` (0..=1048576 bytes), `limit=4096` (1..=16384 bytes)                                                      |
| `write_file`   | `path`, `content`                                          | `expected_sha256`                                                                                                   |
| `list_dir`     | `path`                                                     | `offset=0` (0..=4096 entries), `limit=100` (1..=256 entries)                                                        |
| `find_files`   | `path`, `pattern` (literal substring, empty matches all)   | Same pagination as listing; `include_generated=false`                                                               |
| `search_files` | `path`, `query` (nonempty literal substring)               | Same pagination as listing; `include_generated=false`, optional literal `file_pattern`                              |
| `edit_file`    | `path`, nonempty `old_text`, `new_text`, `expected_sha256` | None                                                                                                                |

Content/old/new text each fit 65536 bytes; pattern/query/file_pattern fit 1024 bytes and
reject NUL. Explicit null is not omission. Native request JSON fits 262144
bytes. Complete reads, mutation preimages, scanned files and resulting edits
fit 1048576 bytes/file. Complete native stdout, including LF, fits 7000 bytes.
Pages shrink to fit encoded JSON without truncating digest or entry metadata;
an irreducibly oversized entry is an explicit error.

Result objects:

| Operation  | JSON fields                                                                                                                                                                                                                                                                      |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Read       | `content`, `sha256`, `offset`, `next_offset`, `total_bytes`                                                                                                                                                                                                                      |
| Write/edit | `path`, `sha256`, `total_bytes`                                                                                                                                                                                                                                                  |
| List       | `entries:[{name,kind}]`, `offset`, `next_offset`, `total`                                                                                                                                                                                                                        |
| Find       | `files:[absolute_path]`, `offset`, `next_offset`, `total`, `skipped_symlinks`, `skipped_generated`, `visited_entries`, `complete`                                                                                                                                                |
| Search     | `matches:[{path,line,match_byte_offset,content,truncated}]`, `offset`, `next_offset`, `total`, `skipped_symlinks`, `skipped_binary`, `skipped_generated`, `skipped_oversized`, `excluded_files`, `visited_entries`, `scanned_bytes`, `complete`, `scope` (`file` or `directory`) |

`next_offset` is the actual continuation offset or null. Beyond-end offsets
fail; exactly-at-end offsets return an empty terminal page. Read digests cover
the whole bounded regular UTF-8 file; offsets must be UTF-8 boundaries and a
limit too small for the next character fails. Reads may follow links to paths
accessible in the sandbox namespace; the helper itself is not a read sandbox.
Directory operations reject linked ancestors, list links and skip/count links
in recursive traversal. Results sort by name/path, with search then by line.
Search uses one-based line numbers and a first byte offset per matching line;
previews exclude LF, retain CR and are bounded to 1024 UTF-8 bytes.

Listing caps 4096 entries/directory. Recursive operations cap 4096 visited
descendants and depth 32. Search caps 8388608 scanned bytes and 4096 matching
lines; binary files are counted and skipped. Discovery does not inspect file
contents. Unreadable/unsupported ordinary entries fail explicitly.

Generated directory basenames `.git`, `target`, `node_modules`, `vendor`,
`vendored` and `.agent-runner` are pruned unless `include_generated=true`;
an explicitly selected generated scope remains accessible. `skipped_generated`
counts pruned directory roots, not hidden descendants. Search accepts a regular
file as well as a directory. `file_pattern` matches a literal substring of the
scope-relative path, or the basename for a file scope. Oversized scanned files
are metadata-skipped before allocation; direct reads still fail their bound.
`complete` is false when coverage is skipped/filtered, independent of pagination.

Write/edit parents must exist. New files use mode 0644; replacements preserve
mode bits, not ownership, ACLs, xattrs or hard-link alias updates. Whole-file
write creates or replaces; supplying a digest requires an existing matching
file. Exact edit requires existing UTF-8 and precisely one occurrence,
including rejecting overlapping matches. Atomic replacement is per file;
arbitrary external writers are not locked transactionally.

### `Tool profiles, settings, and detection`

`agent_runner::toolchain` owns the honest advertisement of language tool-chains.

- `ToolProfile` — the set of profiles (`Generic`, `Python`, `Rust`, `JavaScript`,
  `Go`, `C`). `Generic` is the always-on base (coreutils, git, the read-only
  `/usr` layout) and is never configured or gated; the other five are the
  configurable set (`ToolProfile::CONFIGURABLE`), each named by `id()` and mapped
  by `from_id()` (also the CLI/config spelling). `toolkit()` describes the
  package and build tools each profile surfaces.
- `ProfileSetting` (`On` | `Auto` | `Off`) — how a configurable profile is
  enabled. `parse`/`FromStr` accept synonyms (`automatic`, `detect`, `disabled`,
  `none`, …).
- `ToolchainProbe` — whether a profile reaches the tool. `HostProbe` is the
  production probe; availability is judged against the sandbox's read-only
  `MOUNTED_SYSTEM_DIRS` (`/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`), never the
  host `PATH`. `language_available` canonicalises a candidate and ignores any
  interpreter whose path ends in `rustup` (a `rustup` stub is not a real
  compiler), so a stubbed `cargo`/`rustc` cannot be advertised as usable.
- `detect_languages` — advisory: a `BTreeSet<ToolProfile>` derived from root
  manifests (`Cargo.toml`, `pyproject.toml`, `package.json`, `go.mod`,
  `CMakeLists.txt`, …). It informs logging and recommendations and never gates
  use.
- `resolve_profiles` — the gate: from the configured settings, a probe, and an
  optional `forced` profile it returns the ordered set of configurable profiles
  to advertise. `On` requires availability (else `Error::ToolchainUnavailable`),
  `Auto` advertises only when available, `Off` never does. `ToolRegistry::resolve`
  always prepends `Generic`.

- `ToolRegistry::resolve_for_workspace(policy, settings, probe, forced, workdir)`
  supplements the System probe with a bounded, already-installed Rust selection.
  `diagnostics()` returns operational preparation notes; callers record/display
  them. Explicit Rust `off` skips preparation; forced Rust overrides settings.
  Root `rust-toolchain.toml`/`rust-toolchain` selects an installed channel,
  otherwise the standard host rustup default is used. No installation, user
  home/config/credential mount or network is authorized. Concrete resources
  are read-only under `/rust`, with private Cargo scratch. Normal PATH Cargo
  prepends `--offline --locked`; commands requiring resolution need an existing,
  matching `Cargo.lock` and fail without updating it when it is missing or
  stale. Provision locks and vendors separately. This wrapper is not a promise
  to rewrite explicit alternate Cargo paths or prevent authorized workspace
  edits to lockfiles.
  A bounded `.kvist/vendored` snapshot uses sandbox-native paths rather than
  trusting host-absolute paths in project configuration. Pin drift fails closed;
  restart after legitimate pin/vendor changes. Old `resolve` remains System-only.

### `ToolPolicy`

```
struct ToolPolicy {
    shell_deny_prefixes: Vec<String>,          // safe defaults present
    shell_deny_substrings: Vec<String>,        // safe defaults present
    write_root: String,                        // sandbox path, default "/workspace"
}
```

The denylists are applied to the shell command string before any sandbox
request is built. Defaults forbid clearly destructive commands
(`rm -rf`, `mkfs`, `dd`, `:(){ :|:& };`, kernel reloads, etc.). Defaults are
safe; configuration may add entries but must not remove the built-in minimum.

- The `write_root` confines every `write_file` target. Enforcement is on a
  slash boundary, so a sibling such as `/workspace-evil` is rejected when the
  write root is `/workspace`, not merely rejected when it fails a bare
  `starts_with` prefix check. `render` rejects any `write_file` target outside
  the write root before a sandbox request is built.
- `ToolPolicy::shell_permitted(&self, command: &str) -> bool` applies the deny
  prefixes and substrings to a candidate shell command.
- `ToolPolicy::identity(&self) -> String` is a stable `sha256:` digest bound into
  every sandbox request so the request can be traced back to the policy it ran
  under.

### `SandboxRequestBuilder`

`agent_runner::sandbox::build_request(sandbox: &SandboxPaths,
request: &BuildRequest) -> Result<SandboxRequest>` — produces a version-one
`SandboxRequest` (the shared `kvist_sandbox_runner::protocol` type) in the
`Authoring` phase. `BuildRequest { argv, working_directory, read_roots,
environment, policy, resources }` carries the inputs; `execute` renders the tool
argv and passes it here. It:

- resolves and hashes the runner and backend identities,
- builds one read-write `authoring` grant mapping the working directory to the
  sandbox write root, and one read-only `context` grant per exact regular
  non-link context file (missing/directory sources fail)
  (destinations disjoint from the write root). A staged shell script is one such
  context file at `/context/0`; its content identity is the grant identity, while
  `identities.command` identifies the (small) argv wrapper; no scratch grant is
  declared — the sandbox's private `/tmp` tmpfs serves as scratch and `HOME`
  points there,
- sets `Network::Deny`, bounded `Resources`, and a `System` toolchain whose
  identity equals `identities.toolchain`,
- computes `identities.{runner,policy,toolchain,command,mount_plan}` as
  `sha256:` digests, and
- returns `Err` if the working directory does not exist, contains a symbolic
  link whose resolved target escapes the writable scope (following such a link
  for writing could leave the sandbox), or a read root overlaps the write root.
  Symlinks that stay inside the scope are allowed -- Node's `.bin` links and
  in-project aliases are common and cannot escape -- so real projects work
  unchanged. The escaping link and the path it points at are named in the error
  so the cause is actionable. The check is recursive and de-duplicated, and the
  runner re-validates the immediate scope as a second line.

### `execute` (sandbox executor)

`agent_runner::sandbox::execute(sandbox: &SandboxPaths, request: &SandboxRequest,
cancellation: &CancellationToken) -> Result<ToolOutcome>` — spawns
`kvist-sandbox-runner --kvist-sandbox-request-v1`, writes the request to its
stdin, supervises stdout/stderr (bounded, timeout, cancellation), kills the
process group on interrupt, and returns the captured output as a `ToolOutcome`.
Captured stdout plus stderr and intermediate buffering are bounded; final
draining cannot bypass the cap. Stdin writes and post-exit drains check the same
deadline/cancellation. Retained descendant descriptors cause explicit cleanup
failure, never indefinite joining or a successful result. Escaped host
descendants or uninterruptible kernel work cannot be promised forcibly
terminated; failures remain explicit.
`ToolOutcome` uses byte-accurate capture (`stdout`/`stderr` are `Vec<u8>`)
with `output_text`/`error_text` truncation helpers so non-UTF-8 output never
panics.

Sandbox request preflight streams regular non-link identity files in 64-KiB
blocks, with a 256-MiB/file read-time bound. Workspace inspection bounds
1,000,000 entries, depth 128 (root depth zero), and 32 MiB of charged directory
path bytes for its two retained representations. Both stages check cancellation
between I/O operations and share a 30-second cooperative preflight limit;
the executor supplies the prompt's cancellation/deadline token. Oversized
workspaces fail explicitly with narrowing guidance. Filesystem/kernel calls
are not preemptible, and preflight is not a substitute for namespace enforcement.

### `ToolOutcome`

```
struct ToolOutcome {
    exited: bool,
    status: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
    output_limit_exceeded: bool,
    cancelled: bool,
}
```

`ToolOutcome::rejected()` builds a zeroed, failed outcome used to record a tool
that was rejected by policy or failed before producing real output, keeping the
durable record faithful without inventing sandbox results.

### `AgentSession`

`agent_runner::session::AgentSession` holds the ordered conversation and drives
turns against any `ModelTransport` from `agent_runtime`.

```
struct AgentSession {
    model: Model,                                // id + provider + base_url + model + deadline
    thinking_effort: ReasoningEffort,
    system_prompt: String,
    messages: Vec<ModelMessage>,
    tool_defs: Vec<ToolDefinition>,
    answer: Option<String>,
}
```

- `AgentSession::new(model, thinking_effort, tool_defs, system_prompt)` — builds
  one turn request shape.
- `AgentSession::push_user(&mut self, text)` — appends a `User` message.
  It clears any previous final answer.
- `AgentSession::model_selector(&self) -> &str` — the selected model id.
- `AgentSession::next_request(&self) -> Option<ModelRequest>` — returns the next
  turn request, built from the accumulated messages, tool definitions, the
  selected reasoning effort, and `tool_choice = Auto`.
- `AgentSession::apply_assistant(&mut self, turn: ModelTurn) -> Vec<ToolIntent>`
  — folds a model turn (text + tool intents) into the conversation as an
  `Assistant` message, records the final turn's assistant text as the session
  answer only for a normal nonblank `Stop` with no tools, and returns the tool intents the turn
  proposed.
- `AgentSession::record_tool_result(&mut self, call_id, name, outcome)` — folds a
  tool result into the conversation as a `ToolResult` message. Combined
  stdout/stderr, process flags and truncation markers fit 8 KiB; this preview
  is not automatic secret redaction.

The session never executes tools itself; it records the tool intents the turn
proposed and lets the caller (via [`ToolExecutor`]) execute them. This keeps it
transport- and environment-independent and unit-testable.

### `ToolExecutor` and `Recorder`

- `ToolExecutor::execute(&self, intent, cancellation) -> Result<ToolOutcome>` —
  renders and executes one tool intent inside the sandbox. The loop is
  transport- and environment-independent because it hands argv to this trait
  rather than spawning processes directly.
- `Recorder` is a fallible pluggable operational sink: `session_start`,
  `on_prompt`, `request`, `turn_start`, `turn_finish`, `tool_dispatch`,
  `tool_result` and `session_finish` return `Result`. Dispatch must be
  acknowledged before invoking the executor. Recording errors terminate further
  effects. `SessionLog` synchronizes dispatch/result and terminal records. Its
  private diagnostic transcript bounds each text item to 64 KiB; it is neither
  a complete replay checkpoint nor guaranteed secret-free.
- `Recorder::on_prompt` is called once per prompt as it is received by the
  worker, before any turn, and defaults to a no-op. The durable session log
  uses it to fold the first prompt's leading words into the record name (see
  Data and schemas); other recorders may ignore it.

### `Event`

The loop emits the following typed `Event`s, which the UI renders and the
worker streams over a bounded channel:

```
enum Event {
    TurnStart { model: String },
    AttemptStart { attempt: u32 },
    Reasoning(String),
    Text(String),
    ToolCall { description: String, name: String },
    ToolResult { description: String, name: String, failed: bool },
    Finished { message: String },
    Failed(String),
    Note(String),
    PromptEnd { exhausted: bool, cancelled: bool },
    Progress {
        token_accounting: TokenAccounting, // provider, estimated, unavailable
        input_tokens: u64,
        output_tokens: u64,
        context_tokens: usize,
        context_limit: usize,
        context_utilization: f64,
        compaction_progress: f64,
        tokens_per_sec: f64,
        total_tokens: u64,
        elapsed_secs: f64,
    },
}
```

`Event::ToolCall` and `Event::ToolResult` carry both the raw `name` and a short
human `description` of what the call applies to (the file, directory, or
command). The description is produced by `tools::describe_tool_call`, the same
function that builds each `RenderedTool.summary`, so the live "tool proposed"
line matches the executed one.

`Event::Progress` carries the live stats the UI shows: working speed
(`tokens_per_sec`), context utilization and the compaction progress bar, plus
cumulative `total_tokens` and `elapsed_secs` for progress.

### `AgentRunner` (loop)

`agent_runner::session::AgentRunner { max_turns }` runs the multi-turn loop and
returns a [`RunSummary`]. Its generic `run` drives one session:

```
AgentRunner::run::<M, E, S>(
    &self, session, transport, executor, sink, cancellation,
    context, recorder,
) -> Result<RunSummary>
```

repeats: `next_request`, stream the turn forwarding text/reasoning/tool-intent
events, execute each tool intent (bounded, cancellable), apply its results, and
stop only on a validated normal `Stop` with nonblank text and no tools.
Compaction trims only the model context; the optional `recorder` is an operational record. The
loop reports token accounting and compaction via `Event::Progress`.

`AgentRunner` also carries a `RetryPolicy` that makes a turn resilient to
temporal, recoverable failures. When `drive_turn` sees an error for which
`agent_runtime::Error::is_retryable` returns `true` (a dropped connection, a
provider timeout, or a transient server error), it waits `RetryPolicy::backoff_delay`
— exponential growth capped at the policy's maximum delay — and replays the turn
with a fresh request. Each retry is granted a larger budget than the last: the
streaming call's per-turn deadline is the transport's configured deadline
multiplied by the attempt number, capped at that deadline times `max_attempts`,
so a turn that merely ran past one deadline can finish once an attempt has room
for the whole generation instead of timing out identically on every identical
try. Cancellation is never retried. Between attempts the loop emits an
`Event::Note` (`retrying N/M in Xs with a Ys budget`) so the user sees the retry
is in progress and that a longer budget is being granted; the retry budget is
`max_attempts` total tries. A turn that exhausts the budget, or one that fails
with a non-retryable error, is surfaced as `Event::Failed` and stops the session
while returning a failed [`RunSummary`] with `turns` set. Infrastructure,
recording and event-delivery errors return `Err`, never a successful default.
Repeated identical argument hashes are rejected and eventually circuit-break;
the detector does not observe or prove unchanged filesystem state.

Injected-library `RunLimits` defaults to a 24-hour (86,400-second) prompt wall
budget, 100,000,000 conservatively estimated request/reserve tokens, and 1024
response tokens, so a prompt runs uninterrupted to a result, a detected
hang/loop, or a real failure. Production startup resolves the selected
model's reserve separately.
Allowed maxima are 24 hours, 1,000,000,000 estimated tokens, and 1,048,576
response tokens; each must be positive. Every attempt, including retries,
charges its complete estimated input plus reserve before I/O. Turn limits are
1..=500. Provider attempt deadlines and backoff are clipped to the prompt
deadline; cancellation is cooperative for injected transports/executors.

### `RunSummary`

```
struct RunSummary {
    answer: Option<String>,
    turns: u32,
    tools_executed: u32,
    cancelled: bool,
    exhausted: bool,
    budget_exhausted: bool,
    failure: Option<String>,
}
```

`success()` requires an answer without failure/cancellation/exhaustion.
`disposition()` returns `completed`, `failed`, `cancelled`, `turn_limit`,
`budget_exhausted` or `no_work`. A previous prompt's answer is never reused.

### `ContextManager` (rolling context)

`agent_runner::context::ContextManager` bounds the model context across a session
so long-running work stays reliable. It estimates the token size of the next
request (`estimate_request`), reports when it crosses a warm-up threshold
(75% of the window by default), and compacts the oldest completed turns into a
rolling summary while the most recent turns stay in full.

- `ContextManager::for_model(limit_tokens)` — production capacity-based retention
  targeting 65% once the 75% trigger is reached, preserving outstanding user
  messages after the last accepted text-only final answer, systems and newest
  complete tool group. Protected context may exceed the target but not hard limit.
- `ContextManager::new(limit_tokens, keep_full_turns)` — compaction starts at
  75% of the window; recent complete groups are retained when they fit.
- `ContextManager::prepare(&mut self, &mut ModelRequest, response_reserve) ->
Result<Option<Compaction>>` is the authoritative pre-I/O path. It estimates
  complete canonical serialization, sets the provider output bound, compacts
  complete groups, and checks input plus reserve. System messages, the latest
  user goal and the newest complete group are retained. Inconsistent
  call/result identities and irreducible overflow are errors; failure changes
  neither request nor rolling summary. Summaries are explicitly lossy and
  non-authoritative. This is a byte-aware heuristic, not an exact tokenizer.
- `ContextManager::compact(&mut self, messages, tool_definitions) ->
(Vec<ModelMessage>, Compaction)` — keeps as many of the most recent turns in
  full as fit under the hard `limit_tokens`, rolling the rest into the summary.
  It keeps reducing how many recent turns stay full (compacting more) until the
  estimated context is under the limit, always keeping the single most recent
  turn in full as a best effort. This older diagnostic API cannot reject
  irreducible requests and is not the runner's sending path; use `prepare`.
- `ContextManager::should_compact`, `utilization`, and `compaction_progress`
  drive the live stats and the compaction progress bar shown in the UI.

## Required interfaces

- **Model transport.** The session and loop are transport-agnostic: they consume a
  `ModelTransport` from `agent_runtime` for streaming turns and never spawn or
  talk to a model directly.
- **Executor.** The loop hands tool intents to `ToolExecutor`; the production
  executor renders argv/payloads and supervises execution. Loop policy is unit-testable with a
  recording executor and never perform blocking subprocess I/O themselves.
- **Sandbox runner.** Tool execution is delegated to the externally installed
  `kvist-sandbox-runner` subprocess (`--kvist-sandbox-request-v1`); `agent-runner`
  writes a version-one `SandboxRequest` to its stdin and supervises bounded,
  timed, cancellable output.
- **Events and recording.** The loop emits typed `Event`s over a bounded channel to
  the UI and writes durable session records (`session_start`, `turn_start`,
  `turn_finish`, `tool_result`, `session_finish`) to a pluggable `Recorder`.
- **Cancellation.** A shared `CancellationToken` from `agent_runtime` is checked
  between turns and terminates the sandbox process group on interrupt.

## Data and schemas

```toml
schema_version = 1
working_directory = "/abs/path/optional"
# Last-resort model when no explicit selection, no active provider model, and no
# single configured model resolve the session's model. See "Model selection and activation".
default_model = "local"
default_thinking_effort = "medium"

[[models]]
id = "local"
provider = "llama-server"
base_url = "http://127.0.0.1:9931"
model = "qwen2.5-14b"
deadline_secs = 300
cadence_timeout_secs = 30

[[models]]
id = "ollama"
provider = "ollama"
base_url = "http://127.0.0.1:11434"
model = "qwen2.5"
deadline_secs = 300
cadence_timeout_secs = 30

[sandbox]
runner = "/usr/local/bin/kvist-sandbox-runner"
backend = "/usr/bin/bwrap"

[tool_policy]
# Denylist entries are appended to the built-in safe minimum.
shell_deny_substrings = []
shell_deny_prefixes = []

# Per-language tool-chain advertisement. Each key is a configurable profile id
# (python, rust, javascript, go, c). `Generic` is always present and is not
# configured. `auto` advertises only when the interpreter reaches the sandbox;
# `on` advertises and fails at startup when it does not; `off` never advertises.
[tool_profiles]
python = "auto"
rust = "auto"
javascript = "auto"
go = "auto"
c = "auto"

# Optional independent review phase for headless runs; off unless enabled.
[review]
enabled = true
model = "reviewer"
thinking_effort = "high"
apply_fixes = false
max_turns = 200
on_failure = "warn"
```

`schema_version` must be `1`. Unknown top-level fields fail. Each `[[models]]`
needs a unique `id`, a known `provider`, a non-empty `base_url` and `model`, and
a bounded `deadline_secs` (1..=600) and `cadence_timeout_secs` (0..=600; `0`
disables the inter-token cadence watchdog). The slot-allocation watchdog (the
provider accepting a request, including model load or switch) and the
time-to-first-token watchdog (long-prompt prefill) are each granted the full
per-turn `deadline_secs`, so a slow model switch completes within the turn
budget instead of failing; a stall after the first token is still caught by
the independent cadence watchdog. `sandbox.runner` and `sandbox.backend`
default to resolved system locations when omitted. `[tool_profiles]` accepts the
ids `python`, `rust`, `javascript`, `go`, and `c` (any other key fails); each
value is `on`, `auto`, or `off`. Unknown profile keys and unrecognized settings
fail at load. An optional `[review]` table configures the independent review
phase (see "Independent review extension"): `model` must name a configured
model id, `thinking_effort` a known effort, `max_turns` 1..=500,
`on_failure` `warn` or `fail`; unknown keys fail at load.

## Command-line interface

```
agent-runner [OPTIONS] [PROMPT]

Options:
  -c, --config <PATH>         Path to the TOML configuration (default: search
                              ./agent-runner.toml, then the per-user
                              agent-runner/config.toml, then XDG_CONFIG_DIRS
                              (default /etc/xdg))
  -m, --model <ID>            Select a configured model id for this session
  -e, --effort <LEVEL>        Set the thinking effort for this session
      --cwd <PATH>            Set the working directory (must exist)
  -p, --profile <NAME>        Select a language tool profile
                              (generic, python, rust, javascript, go, c)
  --log-dir <PATH>            Directory for the session journal and transcript
  --context-limit <TOKENS>    Serving window override (model config/discovery otherwise)
  --response-reserve <TOKENS> Initial output reserve (model config/window-aware otherwise)
  --max-run-secs <SECONDS>    Whole-prompt wall budget (default 86400)
  --max-run-tokens <TOKENS>   Estimated attempt budget (default 100000000)
  --headless <PROMPT>         Terminal-free, sandbox-only, required recording
  --json                     Version-one ordered NDJSON (headless only)
  --no-logs                   Skip the durable session journal and transcript
  --list-models               Print the configured models and exit
  --import-kvist              Print model entries imported from kvist.toml
  --kvist-config <PATH>       Explicit import source (default ./kvist.toml)
      --allow-host-execution  Bypass the Bubblewrap sandbox and run the agent's
                              commands directly on the host. Never the default;
                              without it the agent is confined to the sandbox and
                              runs multi-turn.
      --host-turns <N>        Maximum autonomous turns a single prompt may
                              drive, only when --allow-host-execution is set
                              (default 1, range 1..=500).
      --review                Run the independent review phase after a
                              completed headless run (config `[review]`
                              otherwise; off by default)
      --no-review             Skip the review phase for this run
      --review-model <ID>     Reviewer model for this run
      --review-apply-fixes    Let the reviewer apply fixes after assessing
  -h, --help                  Print help
  -V, --version               Print version
```

- Interactive mode requires a terminal; otherwise use `--headless`.
- A positional `PROMPT` starts the session and submits the first prompt; the
  session can continue with further input in the UI.
- `--list-models` is non-interactive and exits zero.
- The sandbox is the default execution scope and is multi-turn (`500` turns). The
  `--allow-host-execution` flag opts out of the sandbox: the agent runs with host
  privileges, so it is single-turn by default to prevent a single prompt from
  driving an unbounded autonomous loop under those privileges. `--host-turns`
  raises that cap (restricted to `1..=500`) and is only meaningful with
  `--allow-host-execution`; a cap outside the range is rejected before the UI
  starts. `--host-turns` requires `--allow-host-execution`.
- `--config`, `--model`, `--effort`, `--cwd`, `--profile` override configuration
  and are validated before the UI starts. A `--profile` name is one of
  `generic`, `python`, `rust`, `javascript`, `go`, `c`; selecting an unavailable
  profile (via `-p` or a setting of `on`) fails startup rather than advertising a
  tool the sandbox cannot run. `--log-dir`, `--context-limit`, and `--no-logs`
  configure the durable session record and the compaction window.
- `--review`, `--no-review`, `--review-model`, and `--review-apply-fixes`
  apply to headless runs. `--review-model` must name a configured model id and
  is validated before the UI starts, as with `--model`; `--review-apply-fixes`
  only has an effect when the review phase runs.
  Headless mode rejects host/no-log flags and conflicting list/import modes.
  Prompts must be nonblank and at most 64 KiB. Plain stdout contains only the
  successful final answer (and, when the review phase runs, the review
  assessment after it); NDJSON ends with a run-summary disposition and, when
  the review phase runs, a review-summary disposition.
  Interactive logs default to `.agent-runner/runs`; headless logs default to
  `$XDG_STATE_HOME/agent-runner/runs` or `$HOME/.local/state/agent-runner/runs`.
  Required logs must be private, non-linked and outside the writable workspace.
  Plain answers/diagnostics visibly escape terminal controls except LF/tab;
  JSON and private transcripts preserve original text. History reads reject
  nonregular files and links in any path component; metadata and bounded reads
  cap a file at 5 MiB, including concurrent growth. Directories over 4096 entries
  and unusable entries generate diagnostics. Replay is lossy UTF-8 diagnostic
  display, not executable resume or trusted task evidence.

## Behavioral guarantees

- The loop is multi-turn: it repeats `next_request`, streams text, reasoning, and
  tool-intent events, executes each tool intent (bounded and cancellable) inside
  the sandbox, folds results back, and accepts only a validated normal final
  answer. Tool proposals require `ToolCalls`; truncated, filtered, unknown,
  inconsistent and duplicate-ID turns fail before execution.
- A turn that hits a recoverable, temporal transport error is retried with capped
  exponential backoff and a per-turn deadline that grows with the attempt number
  and caps at the deadline times `max_attempts`; an exhausted budget is reported
  as `Event::Failed`, never hidden, and cancellation is never retried.
- The model context stays bounded across long sessions: `ContextManager` compacts
  the oldest completed turns into a rolling summary (trimmed to `MAX_SUMMARY_CHARS`)
  while retaining the current goal and newest complete group. Irreducible
  overflow fails before I/O; records are bounded diagnostics, not a full audit
  transcript or canonical task evidence.
- The executor emits exactly one `SandboxRequest` per tool call and reports any
  deviation as an `Err` before spawning the runner.
- The terminal UI never renders a line wider than its box: transcript text is
  pre-wrapped at the box's inner width, and the help, menu, session-history,
  and replay panels soft-wrap their lines at word boundaries, so every panel
  keeps its borders on narrow terminals; wrapped text keeps the source line's
  leading indentation on continuation lines, and scroll extents account for the
  wrapped line count so all content stays reachable.
- Transcript rows are filled to the full inner width (the border excluded), so
  each block reads as an enclosed textbox: the prompt echo, reasoning,
  placeholders, and agent-produced content (markdown, tool calls/results,
  notices, terminal status) each carry their own subtle, low-contrast
  background, and single-line tool rows share the agent-content background.
- The terminal UI starts immediately: model transport setup and the provider
  connection/load run on a background worker while the transcript is shown in a
  starting state, and the first prompt is held and submitted when the worker is
  ready. A bootstrap failure is a non-fatal notice; the next submission
  starts a fresh bootstrap. Changing the model or effort and submitting
  cancels the old worker, discards its stale bootstrap, and starts a new one
  with the held prompt. The per-turn deadline bounds the whole turn, including
  the model load/switch (see Data and schemas), not a short fixed probe.
- Model selection and activation (see "Model selection and activation") apply
  before any bootstrap: an explicit `--model`/Tab selection and a
  single-configured-model convenience always select their model; otherwise a
  bounded read-only provider probe selects an already-loaded model when it
  matches a configured entry; and only when neither resolves and several models
  are configured does the TUI start with no model selected (no bootstrap and no
  model load until the user selects and submits) and headless fails fast with an
  actionable diagnostic. `default_model` is a last-resort fallback that MUST NOT
  override an active model, an explicit selection, or a single configured
  model. When a matched provider model is not configured, the tool offers a
  ready-to-paste `[[models]]` entry and never writes the configuration file.
- The ESC menu closes on selection: choosing a history item transitions to the
  replay overlay, and choosing "New session" (Enter or `n`) starts a fresh
  session. `Ctrl+N` starts a new session from anywhere; `Ctrl+P` steps
  backward through prompt history.
- Terminal prompt submissions are bounded at 1,048,576 characters; headless
  prompts remain bounded at 64 KiB by the command line.
- Session records are named `session-{UTC date-time}-{pid}-{n}` plus, after the
  the first prompt, a slug of its first words (punctuation collapsed, at most 40
  characters), e.g. `session-2026-10-03T14-22-05Z-4242-1-fix-the-bug.log`;
  journal and transcript are renamed together before further writes.
- A headless run whose disposition is `completed` and whose review is enabled
  appends exactly one review phase: a fresh session with the resolved reviewer
  model, the review system prompt, and the review prompt (original task prompt
  plus implementation answer), its own bounded budgets, and its own private
  journal with the `-review-` stem marker. The phase never sees the
  implementation transcript, never widens sandbox authority, and its failure
  does not change the implementation disposition under the default
  `on_failure = "warn"`.

## Errors and failure semantics

- `agent_runner::Error` and `agent_runner::Result<T>` carry an
  `exit_code() -> u8` and a `describe() -> String` that produce actionable,
  non-secret messages; formatting failures degrade gracefully and errors never
  unwrap their underlying source when printing.
- A tool rejected by policy or that fails before producing real output is recorded
  as a zeroed `ToolOutcome::rejected()`, keeping the durable record faithful
  without fabricating sandbox results.
- A missing or unverified sandbox runner or backend fails closed with an
  actionable diagnostic and never falls back to unconstrained host execution; a
  write whose target escapes the working directory is rejected before the request
  is built, on a slash boundary so `/workspace-evil` is rejected when the write
  root is `/workspace`.
- Non-UTF-8 subprocess output is captured byte-accurately and truncated via
  `output_text`/`error_text` helpers, so malformed output never panics.

## Security and authority

The executor emits exactly one `SandboxRequest` per tool call, shaped as in
`src/sandbox.rs`, matching `../sandbox_runner/schema/kvist-sandbox-request-v1.schema.json`
and the runner's closed version-one protocol. Guarantees:

- `phase = "authoring"`, `network.mode = "deny"`, no Cargo cache.
- `argv[0]` is an absolute canonical path; every entry is bounded and NUL-free.
- The working directory is absolute and canonical.
- Exactly one read-write `authoring` grant (working directory → sandbox write
  root) and zero or more read-only `context` grants, with disjoint
  destinations and only in-scope writable sources (a writable symlink whose
  resolved target escapes the scope is rejected, not declared); the environment
  sets `HOME=/tmp`, the runner's private tmpfs scratch area.
- `resources` are nonzero and within the runner's safe maxima.
  Every identity is a `sha256:` digest; `identities.toolchain` equals the
  toolchain block identity.
- The review phase reuses the closed version-one sandbox request shape and
  grants; the default assess-only mode withholds `write_file` and `edit_file`
  from the reviewer, and review reports are advisory model output — not
  canonical evidence, compliance decisions, or acceptance receipts.

Any deviation is reported as an `Err` before spawning the runner.

## Compatibility and verification

- The sandbox request matches the shared version-one `kvist_sandbox_runner::protocol`
  wire shape and the closed version-one request schema (JSON Schema draft 2020-12), so the
  wire format stays identical to the runner's own statement of the contract.
- `schema_version` is `1`; unknown top-level fields, unknown profile keys, and
  unrecognized settings fail at load. `deadline_secs` is bounded to `1..=600` and
  `cadence_timeout_secs` to `0..=600` (`0` disables the inter-token cadence watchdog).
- Tool definitions, grants, and identities use stable, deterministic ordering, and
  every identity is a `sha256:` digest that binds the request to the policy,
  toolchain, command, and mount plan it ran under.
- The session, loop, rendering, and CLI are transport- and environment-independent
  and unit-tested with an injected transport and a recording executor.
