<!-- kvist-contract-version: 1 -->

# Sav Contract

## Shared native runtime API (ADR 0015)

`sav::runtime` exposes `AgentSession`, `ModelIdentity { id, model }`,
`ContextManager`, `RetryPolicy`, `NativeRuntime`, `RunLimits`, `RunSummary`,
`ToolOutcome`, serializable `Event`, and a narrowly scoped runtime `Error`.
`ToolExecutor`, `EventSink` and `Recorder` are host interfaces: no executor,
policy, selected model configuration, terminal or application task type is
provided. Hosts declare descriptions and repeatable observations; those do not
authorize effects or permit automatic tool replay. Required recorder errors
stop execution before subsequent effects. `Recorder::model_event` receives
each provisional `ModelStreamEvent` fallibly before presentation, so a
host-owned live journal can retain partial progress without authorizing it.

`NativeRuntime::run_controlled` takes a cloneable `ControlMailbox` capability.
`steer` requires an active run; `submit` atomically classifies idle submissions
as tasks and running submissions as guidance. The pending mailbox has at most
32 messages and 1 MiB of text; tasks are at most 1 MiB and guidance 64 KiB each.
IDs are monotonically increasing, run-owner-local `GuidanceId`s. `edit` and
`withdraw` succeed only before claim. Delivery preserves submission order,
conversation grouping, original goal, observed effects, wall/token charges,
generation attempt counts and effect-repeat protection. Interrupted provider
partials are provisional and never executable. Remaining stale calls receive
explicit not-executed results before guidance is appended.

Correlated `GuidanceNotice` states are accepted, claimed (interrupting),
delivered, withdrawn or dropped. Delivery means appended to model context,
not provider acknowledgement or successful task completion. The recorder
persists delivered state before the next provider request. Shutdown closes the
capability, discards pending work and prevents subsequent context delivery.
Guidance never becomes a replacement task when a prompt ends.

Low-level host loops may use `begin_run`, `claim`, `withdrawals`,
`dispatch_ready`, `deliver_if_open`, `finish_if_idle`, and `end_run`; the host
then owns recording and delivery obligations. `finish_if_idle` linearizes final
completion against simultaneous submission. `Operation::start` supplies a fresh
provider-only cancellation token which mirrors prompt cancellation, shutdown
and pending guidance within a 10-ms polling interval; it never resets the
prompt token. Executing effects must use the prompt token instead.
`validate_turn` checks complete finish classification and bounded, unique tool
proposals without authorizing them.

## Shared registry configuration (ADR 0015)

`sav::registry` owns the canonical schema-one TOML registry. Its root contains
`schema_version = 1` and named `[profiles.NAME]` tables. A profile has `enabled`
(default true), separate `advertised`, `tested`, and `policy_enabled` capability
lists (default empty), and a `[profiles.NAME.backend]` table. Backend `kind`
is `native` (shared local provider, `base_url`, `model`, optional native limits)
or `external-agent` (typed provider, literal `executable`, structured `arguments`,
and optional `model`). Unknown fields, duplicate names, invalid bounds, inline
credentials, and non-numeric/non-loopback HTTP endpoints fail closed.

`RegistryPaths` resolves complete named definitions, explicit then project
then user; missing optional project/user files are ignored, malformed files are
not. An explicit file must exist. Default user storage is
`$XDG_CONFIG_HOME/sav/registry.toml` or `$HOME/.config/sav/registry.toml`;
project storage is `.kvist/providers.toml`. Resolution never probes a process,
endpoint, loaded model or executable. `ResolvedProfile` pins the selected name,
complete typed profile, source path and content digest. Disabled profiles cannot
be selected. Registry parsing and resolution never authorize execution.

`NativeModel` is also the parser/validator for frontend-local explicit native
overrides. `LocalModelProvider` is the shared local provider identifier.
`NativeModel::transport` constructs bounded native transport; external agents
cannot produce one. Persistence validates the complete registry and atomically
replaces a bounded regular non-link file. Explicit setup shares discovery and
qualification, discloses host authority and possible billing, and persists no
credentials. Forced failed qualification does not mark a capability tested.

Registry files are at most 64 KiB and contain at most 128 profiles, including
the resolved union. Names use 1–128 ASCII letters/digits or `.`, `_`, `-`, `:`.
Each capability list contains at most 32 unique identifiers with the same
syntax. Model IDs are nonblank, control-free strings of at most 1024 bytes;
catalog discovery does not confer authority on them.

Native optional fields are `context_limit` (2–1048576), `response_reserve`
(1–1048576, less than configured context), `deadline_secs` (1–600, default
300), `max_attempts` (1–32, default 3), `retry_base_delay_secs` (default 2),
`retry_max_delay_secs` (default 30, at most 3600 and at least the base delay),
and `cadence_timeout_secs` (0–600, default 30; zero disables the watchdog).
The endpoint defaults to numeric-loopback Ollama port 11434 or llama-server
port 9931. Construction and validation perform no network I/O.

External providers are `copilot`, `gemini`, `claude`, `llama-cli` or
`custom-script`. Executables are nonblank, control-free strings of at most
4096 bytes. At most 128 literal arguments, each at most 16384 bytes and
NUL-free, are stored; credential-bearing flags/values are refused. An optional
external model uses the same model-ID bounds. `command_spec` substitutes
`{prompt}`, `{model}`, `{cwd}`, and `{reasoning_effort}` once; an entire
`{context_files}` argument expands into separate paths. Inserted values are
not interpreted again. Rendered argv is bounded to 256 entries and 2 MiB, with
a 1 MiB prompt bound. Returning a command specification never executes it.

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Boundary and ownership

- **Component ID:** `sav`
- **Contract IDs:** `sav.library/v1`, `sav.cli/v1`
- **Owner:** standalone `sav` crate
- **Consumers:** Kvist and other embedding Rust applications; standalone CLI
  users

The component owns reusable prompt, command, profile, supervision, model
transport, capability, loop, broker, execution-backend interface, and runtime
event mechanisms. It does not own embedding-host roles, task state, policy
decisions, grants, approved resources or credentials, promotion, or canonical
compliance evidence.

## Provided interfaces

The public mechanisms below accept host-selected authority and propagate
bounded, explicit outcomes without owning application policy.

### Live diagnostic journals and private controls

Record serialization itself stops at the 64-KiB line bound. Invalid initial
metadata is rejected before creating a file, so it cannot consume a no-clobber
run identity. Serializable application events must also fit the JSON parser's
default nesting bound; validation failure does not append partial bytes.

`run_supervised_observed` supervises one opaque external-agent command with a
caller-supplied cancellation token and a fallible per-stream bounded-byte
observer, retaining bounded captured output. It requires `max_retries = 0`:
unknown external effects must not be replayed. The existing supervisor still
owns process-group cleanup, deadlines and output limits. This API is a process
mechanism, not isolation; the embedding host must explicitly choose an
acknowledged host command or an approved enforcement command. Observer failure
terminates and reaps the process and remains an error. External process output
does not become native tool intent and cannot mint task approval.

`sav::observation` exposes `RunJournal`, `JournalFollower`, `JournalRecord`,
`ControlServer`, `ControlRequest`, `ControlAction`, `ControlReply` and
`send_control`. Journals are private no-clobber regular files. Each newline
delimited JSON record has `schema_version: 1`, one stable `run_id`, a contiguous
one-based `sequence`, and a JSON object `event` with a string `type`. The writer
creates `run_start` and explicitly finishes with `run_end`. Records fit 64 KiB,
the journal fits 64 MiB, and IDs fit 128 ASCII identifier bytes.
`record` appends; `checkpoint` synchronizes before effects; `finish` synchronizes
a terminal disposition. Failed writes poison the writer. Dropping it does not
invent a terminal result or imply absent effects.

Followers hold a non-following regular-file descriptor, retain at most one
64-KiB partial line and return at most 256 records per poll. An incomplete
trailing line is pending, not malformed until the caller declares end of input.
Unknown versions, sequence gaps, mixed run IDs, malformed records and data after
the terminal event fail explicitly. EOF alone is not successful completion.
Replaying diagnostics never executes recorded actions.

Controls use a process-owned Unix socket inside a fresh private temporary
directory. The socket path is an explicit local control capability, separate
from the journal path. Socket messages are newline-delimited strict version-one
JSON requests with positive numeric `id` and tagged `action` (`cancel` or
`guide` with nonblank UTF-8 `text`, at most 16 KiB). Requests/replies fit 20 KiB;
individual I/O waits and the complete exchange are bounded. The host's handler
decides whether an operation is allowed and returns an acknowledgement
(`queued`, `cancellation_requested` or `rejected`), never a claim of delivery.
Guidance delivery is a later journal event. Socket errors and malformed input
do not invoke the handler. The owner closes and joins its listener on drop;
observer/client loss cannot terminate the run.

### Additional runtime and transport mechanisms

The shared runtime boundary described in ADR 0015 separates registry,
native-loop, external-agent and observation interfaces. Registry facts and
resolution belong to Sav; application role bindings and authorization do not.
Run events are bounded, versioned diagnostic observations, never approval or
canonical compliance evidence. Observers have no effect authority; controls
require an explicitly granted private capability. Native hosts supply their
own tools, execution and recorders; a refusal never selects another executor.
Exact exported Rust APIs and CLI grammar are documented alongside delivery.

`DirectModelTransport::loaded_model` reads `/props` for standalone llama-server
identity (`model`, `model_alias`, then `model_path`). Router-mode properties
trigger read-only `/models` discovery, returning an actually loaded entry's
`id`, never an unloaded/loading entry or the router alias. Both requests share
the caller's deadline and cancellation; responses are bounded to 1 MiB and
catalogs to 128 entries. Returned identifiers are nonblank, control-free and
at most 256 bytes. Ollama continues to use `/api/ps`.

`sav::offline_rust` supplies `INITIALIZE` (a shell fragment for private
`/tmp/rustup-home` settings and read-only `/rust/rustup-home/toolchains`
registrations), the closed `RustTool` enum and `TOOLS`, plus `wrapper(tool)`.
Callers stage returned scripts read-only at `/rust/runtime`, initialize before
effects and mount validated installations and vendor at `/rust/*`. Unknown
names are not exposed by `TOOLS`; the caller is responsible for using that
closed set. No host discovery, installation, shell execution or grant decision
is performed by this module. The typed executable set prevents interpolation
of untrusted script fragments.

The Cargo wrapper's vendored-source path is `/rust/vendor`; callers must expose
their validated read-only registry there, outside writable workspace mounts.

Optional host-owned `KVIST_CARGO_WORKDIR` selects the Cargo build-view directory;
`KVIST_COMPONENT_BUILD_DIR` maps source argument paths in that view back to
`/workspace/component` for compiler/doc/formatter tools. Callers must provide
validated sandbox-native directories; neither variable grants filesystem access.

The Rust library exposes bounded prompt acquisition, command-template
rendering, typed supervision policy and attempt context, supervised host
execution, named profile storage and setup operations, provider-neutral model
messages/turns, and local direct transports.

The standalone `sav-run` CLI provides:

- `setup [--registry PATH] [--force]` for shared profile qualification and update;
- `registry [--registry PATH] [--profile NAME]` for filesystem-only JSON inspection;
- `models --provider PROVIDER [--endpoint URL] [--executable PATH]
[--allow-host-discovery] [--json]` for bounded provider-model discovery;
- `run` for supervised provider command execution from positional, file,
  editor, or redirected prompt input, with named profile selection,
  `--reasoning-effort`, and optional `--json` output; and
- `model` for text-only unary or streaming local model requests, with explicit
  provider/model selection or `--profile NAME [--registry PATH]`, an optional `--output-schema` JSON Schema,
  `--reasoning-effort`, `--show-reasoning`, and optional `--json` output.

`run` requires exactly one command template or stored profile and explicit
`--allow-host-execution`. `model` exposes no tools and performs no host process
execution. Multiple case-sensitive profiles from different providers and
models MAY coexist in one configuration and are selected per `run` invocation.

## Required interfaces

The library requires caller-supplied working directory, context paths,
supervision policy, cancellation, profile storage location or I/O streams, and
provider configuration appropriate to the operation.

The planned native runtime requires embedding-host authority interfaces that
decide over canonical tool, arguments, resources, credentials, policy
revision, and budget. Another application may implement those traits without
depending on Kvist.

## Data and schemas

Shared registry TOML uses the typed schema defined in the shared registry
section. Raw command-template library helpers do not resolve backend identity
and are not a compatibility parser for registry files.
Provider-neutral model types represent messages, text, tool intent/results,
provider-supplied reasoning, provider/model and request identities, usage,
finish reason, capabilities, runtime events, and typed errors.

`ModelRequest.output_schema` is an optional component-owned JSON object using
Kvist's bounded common provider subset of JSON Schema 2020-12 keywords. The
subset excludes references, definitions, conditionals, `oneOf`, pattern
properties, and unknown keywords; schemas are limited to 256 KiB, 32 levels,
and 4,096 schema nodes. It is sent unchanged as Ollama `format` or
OpenAI-compatible `response_format.json_schema` with the stable envelope name
`kvist_output`. It is a provider-native generation constraint, not evidence
that returned text conforms to the schema.

Tool descriptors and intents use component-owned JSON Schema-compatible data
shapes where structured validation is required. A descriptor records stable ID
and version, input/output schema, effect class, required capability/resource
scope, timeout/output bounds, and retry/idempotency semantics. No external
schema file is currently the canonical library API.

`ModelRequest.max_output_tokens: Option<u32>` bounds generation when supplied.
Accepted values are `1..=1_048_576`; invalid values fail before network I/O.
llama-server receives `max_tokens`; Ollama receives `options.num_predict`.
An absent bound leaves existing provider defaults unchanged.

`DirectModelTransport::context_limit(model, cancellation)` discovers an
optional effective serving capacity, bounded to 1..=1048576 tokens, without
inference. llama-server uses model-qualified `/props`; Ollama uses matching
loaded-model `/api/ps` metadata (an untagged name also matches its `:latest`
form). Absent metadata is `None`, malformed advertised
metadata is an error. Discovery retains numeric-loopback-only transport,
bounded responses and a bounded deadline. llama-server streaming requests
include `stream_options.include_usage`; providers may still omit usage, which
remains `None`. Canonical/wire requests fit 8 MiB, retaining all other bounds.

`{prompt}`, `{prompt_json}`, `{context_files}`, `{target_directory}`, and
`{reasoning_effort}` are the documented command-template placeholders.
`{prompt_json}` emits one complete JSON string value. A requested reasoning
effort fails if the selected command does not declare
`{reasoning_effort}`. An absent effort removes a standalone
`{reasoning_effort}` argument and its immediately preceding option.

## Behavioral guarantees

Quotes group command arguments, but no shell syntax is interpreted. An empty
standalone context placeholder removes its immediately preceding option.
Prompt acquisition accepts exactly one explicit source, otherwise redirected
input, otherwise an optional editor at a terminal.

In plain-text mode, prompt execution forwards bounded provider output live and
does not append a completion message. When standard error is an interactive
terminal, the CLI identifies the initial prompt before provider output. JSON
mode captures bounded provider streams and emits exactly one object containing
the provider standard output as `content`. Captured bytes are decoded as UTF-8
with each invalid sequence replaced by U+FFFD.

Direct model streaming emits answer-text deltas and, when requested and
supplied by the provider, separate reasoning deltas. Plain text writes only
answer content to standard output; `--show-reasoning` writes provider-supplied
reasoning to standard error. `--json` emits one canonical terminal turn and is
mutually exclusive with `--show-reasoning`.
Plain text does not add line termination that was absent from provider answer
content.

The direct transport is the sole model transport. Structured-output schemas
and callable tools cannot be requested in the same turn; that combination
fails before provider I/O. Callers must parse and validate returned content
independently.

llama-server preserves explicit `finish_reason: "stop"` as `FinishReason::Stop`,
including when tool intents are present. Missing or null unary finish reasons,
or streams ending with `[DONE]` without an explicit non-null finish reason,
return `FinishReason::Other("unknown")`. Null or absent reasons on intermediate
stream deltas do not replace an explicit reason already received. These turns
remain untrusted; callers decide whether a finish reason permits final output
or tool dispatch. Non-string, oversized, or control-bearing finish reasons
return a typed malformed-response error.

Ollama's native `done: true` convention is distinct: stop, missing, or null
`done_reason` yields `ToolCalls` when native tool calls are present and `Stop`
otherwise. Both providers preserve explicit length, content-filter, and
provider-specific reasons without treating accompanying tools as success.

`DirectModelTransport::with_cadence_timeout` enables a streaming generation
progress watchdog. It starts after the first decoded nonempty answer/reasoning
fragment or native tool name/argument progress, and resets only on subsequent
such progress. llama-server tool name and JSON argument-string fragments count
as soon as their containing provider records decode; Ollama's decoded native
tool calls count before terminal intent delivery. Comments, heartbeats, empty
deltas, role/identity/control fields, usage-only records, HTTP chunk framing,
and undecoded record bytes do not reset cadence. Unary requests ignore cadence.

The slot watchdog bounds HTTP header acquisition; the first-body watchdog
bounds initial streaming body acquisition, not the first decoded token.
Receiving an incomplete initial record does not start cadence. Until meaningful
progress decodes, the caller's wall deadline and response/record byte bounds
still apply. Once cadence starts, an incomplete later record remains subject
to that cadence deadline. The earliest applicable deadline and cancellation
remain effective through response framing and event delivery.

A streaming transport call may carry an explicit per-call deadline that
overrides the transport's configured deadline, so a retrying caller can grant
an individual attempt more time without changing that configured bound. The
default streaming call uses the configured deadline; transports that cannot vary
the deadline keep their fixed bound via the default streaming path.

The supervisor retries only configured idle timeouts and deterministic repeated
output. Nonzero exit, spawn failure, stream failure, invalid input,
cancellation, wall timeout, output exhaustion, and write failure are terminal.
Before a retry it terminates and waits for the process group and supplies a
deterministic warning that earlier side effects may remain.

Profile updates preserve unrelated content, validate complete input and output,
and atomically create or replace the selected regular non-link file. Canonical
and retired configuration locations are never consulted by shared resolution.

Setup always tests the generated profile with the fixed prompt
`Reply with exactly: OK`. It neither asks for a test prompt nor asks for a
second host-execution confirmation. A failed test does not persist the profile
unless setup was invoked with `--force`; no interactive save-after-failure
prompt is offered. Qualification command output is captured within the
supervision bound rather than forwarded; setup reports only qualification
status.

For Ollama and llama-server, setup obtains model identifiers from `/api/tags`
and `/v1/models` respectively using numeric-loopback HTTP endpoints with
explicit ports. For Copilot and Gemini CLI, setup starts the selected
executable in ACP mode, initializes protocol version 1, and creates a no-prompt
session in the setup working directory with an empty MCP-server list and no
client-provided filesystem, terminal, or MCP bridges. The provider remains an
opaque host-authority process. Setup consumes `models.availableModels` from the
correlated `session/new` response.

The catalog preserves the first valid occurrence of each model ID in provider
order. Its current model is retained only when that exact ID is in the list;
otherwise the first model is the default selection. An empty or wholly invalid
catalog is a discovery failure. Setup appends `Other model ID...` after every
successful or fallback list and requests free-form input only after that entry
is selected. Discovery failure is visible and offers `auto` for Copilot or
Gemini, `default` for llama-server, or `llama3.1:8b` for Ollama before the
custom entry. Gemini's advertised or fallback `auto` is stored explicitly as
`--model auto`.

`models` exposes the same catalogs without running inference. Text output is
one validated model ID per line. JSON output is one object with
`format_version: 1`, `provider`, nullable `current_model_id`, and ordered
`models`; each model has `id`, `name`, and optional `description`. IDs are
1–256 printable ASCII bytes excluding braces. Names and descriptions are
bounded UTF-8 without control characters. JSON success is one object on stdout;
failures use stderr and nonzero status.

The provider input matrix is:

| Provider       | Discovery input                                                                | Default                  |
| -------------- | ------------------------------------------------------------------------------ | ------------------------ |
| `ollama`       | numeric-loopback HTTP endpoint, default `http://127.0.0.1:11434`               | first advertised model   |
| `llama-server` | numeric-loopback HTTP endpoint, default `http://127.0.0.1:9931`                | first advertised model   |
| `copilot`      | executable, default `copilot`; ACP host acknowledgement required outside setup | advertised current model |
| `gemini`       | executable, default `gemini`; ACP host acknowledgement required outside setup  | advertised current model |

`llama-cli` and custom wrappers return an explicit unsupported-catalog error
without spawning discovery. Their setup paths request an explicit file or
wrapper because neither exposes a provider model inventory.

Model transports return canonical output or explicit typed failure and never
authorize or execute tools. Unsupported capability fails rather than silently
degrading. External agents remain opaque whole processes.

## Errors and failure semantics

Blank, conflicting, oversized, non-UTF-8, non-regular, link-like, malformed,
unsupported, or unknown inputs fail before the relevant provider effect.
Provider and HTTP failures preserve bounded actionable context without
including raw payloads or secrets.

Cancellation, timeout, output overflow, loop detection, destination closure,
and descendants retaining output descriptors run explicit cleanup. A
descendant that escapes the process group is detected as terminal failure; the
component does not claim authority it cannot enforce.

## Security and authority

Host execution inherits the caller's filesystem, credential, executable, and
network authority. The standalone `run` CLI requires explicit acknowledgment
and the library names this mode directly. Standalone ACP model discovery
requires `--allow-host-discovery` for one no-prompt provider session. Setup
qualification treats the explicit setup action as acknowledgement for the
bounded provider-discovery commands and exact generated test command; this
convenience is not isolation and does not authorize any later `run`. ACP
discovery may initialize the selected CLI's configured provider session under
that inherited authority but does not send a prompt. Context paths do not
restrict access.

Endpoint probes are advisory and bounded. Local transports disable proxies and
redirects, allow only numeric loopback HTTP endpoints, bound request/response
data, and suppress payload-bearing third-party tracing from canonical output.

Tool calls are untrusted intent. Only a typed broker may validate and
canonicalize arguments, request a host decision, dispatch an allowed request
through the selected backend, validate/redact the result, and emit a bounded
runtime event.

## Compatibility and verification

The current public contract is pre-release and intentionally carries no
backward-compatibility commitment. Profile schema and provider-neutral types
remain explicitly versioned before a compatibility promise is made.

Conformance uses deterministic command, profile, setup, supervisor, direct
transport, stream, and cancellation tests. Real provider
promotion requires comparison with the direct adapter, dependency review,
security audit, and independent compliance review.
