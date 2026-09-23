<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

This record summarizes behavior observed in the agent_runner crate and its tests.

## Observed public contract

- The agent_runner crate re-exports its public surface from lib.rs: the Error and
Result pair (with exit_code and describe), Config, ToolRegistry, ToolPolicy,
SandboxRequestBuilder, the sandbox execute helper, ToolOutcome, AgentSession,
AgentRunner, RunSummary, ContextManager, and the typed Event set.

Config::load reads and validates the TOML configuration; unknown top-level fields,
unknown profile keys, and bound violations fail at load. schema_version must be 1.

The agent-runner binary accepts -c/--config, -m/--model, -e/--effort, --cwd,
-p/--profile, --log-dir, --context-limit, --no-logs, --list-models, and as an
opt-out --allow-host-execution and --host-turns. With no interactive stdin it
prints an actionable error and exits non-zero.

Per tool call the executor emits exactly one version-one SandboxRequest matching
the shared kvist_sandbox_runner protocol and the closed kvist-sandbox-probe-v1 schema.

## Observed requirements and constraints

- Authority is established once in configuration: tools never prompt the person, the
sandbox is the default execution scope, and writes outside the working directory are
rejected on a slash boundary before the sandbox request is built.

Fail closed: a missing or unverified sandbox runner/backend never triggers host
execution; a rustup stub is not advertised as a buildable tool-chain.

Inputs (config, tool arguments, subprocess output) are validated for schema and
bounds; argv entries are bounded and NUL-free; the request size is bounded before
parsing.

Behavior is deterministic: stable ordering of tool definitions, grants, and
identities, each identity a sha256 digest.

## Observed internal design and failure paths

Separation of concerns: config/toolchain/tools/sandbox build trusted structures from
untrusted inputs; session owns the model-agnostic loop; run owns the worker thread;
tui owns presentation only. session hands argv to an Executor trait and never performs
blocking subprocess I/O itself.

The loop is transport-agnostic: it consumes a ModelTransport from agent_runtime,
streams Event over a bounded channel, and delegates tool execution to the
kvist-sandbox-runner subprocess.

Recovery: a turn that hits a recoverable temporal transport error is retried with
capped exponential backoff and a growing per-turn deadline; a shared CancellationToken
terminates the sandbox process group on interrupt; a turn that exhausts its retry
budget is reported as Event::Failed.

Context stays bounded: ContextManager compacts older turns into a rolling summary
while always keeping the most recent turn in full.
