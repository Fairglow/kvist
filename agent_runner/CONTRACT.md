<!-- agent-runner-contract-version: 2 -->

# Agent Runner — Contract

This document defines what `agent-runner` exposes to consumers: the library
public API, the configuration schema, the command-line interface, and the exact
sandbox request it produces. Private algorithm choices live in `DESIGN.md`.

## Library public API

All public items live in the `agent_runner` crate and are re-exported from
`lib.rs`. Consumers (the `agent-runner` binary, and Kvist as a future caller)
interact only through these items.

### `Error` / `Result`

`agent_runner::Error` is the crate error type (thiserror). `agent_runner::Result<T>`
is `Result<T, Error>`. Errors carry an `exit_code() -> u8` and a `describe() ->
String` that produces an actionable, non-secret message. Errors never unwrap the
underlying source when printing; formatting failures degrade gracefully.

### `Config`

```
struct Config {
    schema_version: u32,                       // required, == 1
    working_directory: PathBuf,                // required, absolute, exists
    default_model: String,                     // required, names a model in `models`
    default_thinking_effort: ReasoningEffort,  // required
    models: Vec<Model>,                        // required, >= 1, unique ids
    tool_policy: ToolPolicy,                   // required
    sandbox: SandboxPaths,                     // required
}
```

- `Model { id: String, provider: ModelProvider, base_url: String, model: String,
deadline_secs: u64 }` — `provider` is one of `llama-server`, `ollama`. The
  `id` is the user-facing selector; `model` is the provider-facing selector.
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

### `ToolRegistry`, `ExecContext`, and `StagedWrite`

`agent_runner::tools::ToolRegistry` owns the model-facing tool definitions and
renders a sandbox command for an approved tool intent.

```
struct ToolRegistry { /* bash, policy, profiles */ }
```

- `ToolRegistry::new(policy: ToolPolicy) -> ToolRegistry` builds a registry with
  the built-in minimal (generic) profile set; it never panics.
- `ToolRegistry::discover(policy: ToolPolicy) -> Result<ToolRegistry>` resolves
  the canonical `bash` path and builds a registry, failing if `bash` is missing.
- `ToolRegistry::with_profiles(self, Vec<ToolProfile>)` adds language tool
  profiles (`Generic`, `Rust`, `Python`).
- `ToolRegistry::tool_definitions(&self) -> Vec<ToolDefinition>` — the
  `agent_runtime::ToolDefinition` list exposed to the model (stable order).
- `ToolRegistry::profiles(&self) -> Vec<&'static str>` — the enabled profile
  identifiers, sorted for determinism.
- `ToolRegistry::render(&self, intent: &ToolIntent, context: &ExecContext) ->
Result<RenderedTool>` — maps a model tool intent to an argv to execute inside
  the sandbox, or `Err` when the tool is unknown, the arguments are malformed, or
  the call violates policy.
- `RenderedTool { argv: Vec<String>, summary: String, staged_write: Option<StagedWrite> }` —
  `argv[0]` is an absolute canonical path; `summary` is a short human description
  shown in the UI and logs; `staged_write` is present only for large
  `write_file` calls.
- `ExecContext { workdir: PathBuf, call_id: String }` supplies the renderer the
  host working directory (the sandbox write-root source) and a per-call id.
- `StagedWrite { host_path: PathBuf, sandbox_path: String, target: String }` —
  stages `write_file` content on the host inside the working directory before the
  rendered `mv` moves it into place, so large files can be written without
  exceeding the sandbox argv byte limit.
- `ToolProfile` (ids `generic`, `rust`, `python`) surfaces the relevant package
  and build tools for each language; the generic profile is always present.

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
  sandbox write root, one read-write `scratch` grant, and one read-only
  `context` grant per declared read root (destinations disjoint from the write
  root),
- sets `Network::Deny`, bounded `Resources`, and a `System` toolchain whose
  identity equals `identities.toolchain`,
- computes `identities.{runner,policy,toolchain,command,mount_plan}` as
  `sha256:` digests, and
- returns `Err` if the working directory does not exist, contains symlinks in
  its writable scope, or a read root overlaps the write root.

### `execute` (sandbox executor)

`agent_runner::sandbox::execute(sandbox: &SandboxPaths, request: &SandboxRequest,
cancellation: &CancellationToken) -> Result<ToolOutcome>` — spawns
`kvist-sandbox-runner --kvist-sandbox-request-v1`, writes the request to its
stdin, supervises stdout/stderr (bounded, timeout, cancellation), kills the
process group on interrupt, and returns the captured output as a `ToolOutcome`.
`ToolOutcome` uses byte-accurate capture (`stdout`/`stderr` are `Vec<u8>`)
with `output_text`/`error_text` truncation helpers so non-UTF-8 output never
panics.

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
- `AgentSession::model_selector(&self) -> &str` — the selected model id.
- `AgentSession::next_request(&self) -> Option<ModelRequest>` — returns the next
  turn request, built from the accumulated messages, tool definitions, the
  selected reasoning effort, and `tool_choice = Auto`.
- `AgentSession::apply_assistant(&mut self, turn: ModelTurn) -> Vec<ToolIntent>`
  — folds a model turn (text + tool intents) into the conversation as an
  `Assistant` message, records the final turn's assistant text as the session
  answer when it proposes no tools, and returns the tool intents the turn
  proposed.
- `AgentSession::record_tool_result(&mut self, call_id, name, outcome)` — folds a
  tool result into the conversation as a `ToolResult` message (redacted, bounded).

The session never executes tools itself; it records the tool intents the turn
proposed and lets the caller (via [`ToolExecutor`]) execute them. This keeps it
transport- and environment-independent and unit-testable.

### `ToolExecutor` and `Recorder`

- `ToolExecutor::execute(&self, intent, cancellation) -> Result<ToolOutcome>` —
  renders and executes one tool intent inside the sandbox. The loop is
  transport- and environment-independent because it hands argv to this trait
  rather than spawning processes directly.
- `Recorder` is a durable, pluggable sink for the session record
  (`session_start`, `turn_start`, `turn_finish`, `tool_result`,
  `session_finish`). The production [`SessionLog`] captures the full reasoning
  trace here, so thinking stays inspectable even after compaction removes it from
  the model context.

### `Event`

The loop emits the following typed `Event`s, which the UI renders and the
worker streams over a bounded channel:

```
enum Event {
    TurnStart { model: String },
    Reasoning(String),
    Text(String),
    ToolCall { name: String },
    ToolResult { name: String, failed: bool },
    Finished { message: String },
    Failed(String),
    Note(String),
    Progress {
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
stop on `FinishReason::Stop` or zero pending tool intents. Compaction trims only
the model context; the optional `recorder` is the durable source of truth. The
loop reports token accounting and compaction via `Event::Progress`.

### `RunSummary`

```
struct RunSummary {
    answer: Option<String>,
    turns: u32,
    tools_executed: u32,
    cancelled: bool,
    exhausted: bool,
}
```

### `ContextManager` (rolling context)

`agent_runner::context::ContextManager` bounds the model context across a session
so long-running work stays reliable. It estimates the token size of the next
request (`estimate_messages`), reports when it crosses a warm-up threshold
(75% of the window by default), and compacts the oldest completed turns into a
rolling summary while the most recent turns stay in full.

- `ContextManager::new(limit_tokens, keep_full_turns)` — compaction starts at
  75% of the window and always keeps the last `keep_full_turns` completed turns
  in full.
- `ContextManager::compact(&mut self, messages, tool_definitions) ->
(Vec<ModelMessage>, Compaction)` — keeps as many of the most recent turns in
  full as fit under the hard `limit_tokens`, rolling the rest into the summary.
  It keeps reducing how many recent turns stay full (compacting more) until the
  estimated context is under the limit, always keeping the single most recent
  turn in full as a best effort. This guarantees the live context stays bounded
  even when individual turns are large, so the session can run for long durations
  without repeatedly sending requests the model rejects.
- `ContextManager::should_compact`, `utilization`, and `compaction_progress`
  drive the live stats and the compaction progress bar shown in the UI.

## Configuration schema (TOML)

```toml
schema_version = 1
working_directory = "/abs/path/optional"
default_model = "local"
default_thinking_effort = "medium"

[[models]]
id = "local"
provider = "llama-server"
base_url = "http://127.0.0.1:9931"
model = "qwen2.5-14b"
deadline_secs = 120

[[models]]
id = "ollama"
provider = "ollama"
base_url = "http://127.0.0.1:11434"
model = "qwen2.5"
deadline_secs = 120

[sandbox]
runner = "/usr/local/bin/kvist-sandbox-runner"
backend = "/usr/bin/bwrap"

[tool_policy]
# Denylist entries are appended to the built-in safe minimum.
shell_deny_substrings = []
shell_deny_prefixes = []
```

`schema_version` must be `1`. Unknown top-level fields fail. Each `[[models]]`
needs a unique `id`, a known `provider`, a non-empty `base_url` and `model`, and
a bounded `deadline_secs` (1..=600). `sandbox.runner` and `sandbox.backend`
default to resolved system locations when omitted.

## Command-line interface

```
agent-runner [OPTIONS] [PROMPT]

Options:
  -c, --config <PATH>         Path to the TOML configuration (default: search
                              $CONFIG_HOME/agent-runner/config.toml, then the
                              working directory for kvist.toml-style config)
  -m, --model <ID>            Select a configured model id for this session
  -e, --effort <LEVEL>        Set the thinking effort for this session
      --cwd <PATH>            Set the working directory (must exist)
  -p, --profile <NAME>        Select a language tool profile for this session
  --log-dir <PATH>            Directory for the session journal and transcript
  --context-limit <TOKENS>    Model context window in tokens (default 8192)
  --no-logs                   Skip the durable session journal and transcript
  --list-models               Print the configured models and exit
  -h, --help                  Print help
  -V, --version               Print version
```

- With no interactive terminal on stdin, the tool prints an actionable error and
  exits non-zero.
- A positional `PROMPT` starts the session and submits the first prompt; the
  session can continue with further input in the UI.
- `--list-models` is non-interactive and exits zero.
- `--config`, `--model`, `--effort`, `--cwd`, `--profile` override configuration
  and are validated before the UI starts. `--log-dir`, `--context-limit`, and
  `--no-logs` configure the durable session record and the compaction window.

## Sandbox request contract

The executor emits exactly one `SandboxRequest` per tool call, shaped as in
`src/sandbox.rs`, matching `sandbox_runner/schema/kvist-sandbox-probe-v1.schema.json`
and the runner's closed version-one protocol. Guarantees:

- `phase = "authoring"`, `network.mode = "deny"`, no Cargo cache.
- `argv[0]` is an absolute canonical path; every entry is bounded and NUL-free.
- The working directory is absolute and canonical.
- Exactly one read-write `authoring` grant (working directory → sandbox write
  root), one read-write `scratch` grant, and zero or more read-only `context`
  grants, with disjoint destinations and no symlinked writable sources.
- `resources` are nonzero and within the runner's safe maxima.
- Every identity is a `sha256:` digest; `identities.toolchain` equals the
  toolchain block identity.

Any deviation is reported as an `Err` before spawning the runner.
