<!-- kvist-compliance-review-version: 1 -->

# agent_runtime compliance review

## Review basis and limits

This review was regenerated as an independent, source-blind comparison using
only:

- `ROOT_CONTRACT.md`
- `src/agent_runtime/REQUIREMENTS.md`
- `src/agent_runtime/CONTRACT.md`
- `src/agent_runtime/DESIGN.md`
- `src/agent_runtime/IMPL.md`
- `src/agent_runtime/tests/**/*.rs`
- `src/agent_runtime/Cargo.toml` for feature interpretation

No production Rust sources, prior `COMPLIANCE_REVIEW.md`, TODO queues,
project-level vision or architecture documents, chat/session history, Git
history, diffs, status, or test execution were used.

All evidence below is static. Test names and assertions were inspected in
source, but no tests were run. This document records conformance findings and
gaps; it is not an execution certificate.

## Summary

The allowed evidence supports substantial conformance for:

- prompt acquisition and shell-free command rendering
- plain-text and JSON output presentation
- Linux process supervision, retry, cancellation, and cleanup
- bounded profile storage and path resolution
- setup qualification and bounded model discovery
- direct Ollama and llama-server transports
- the default-enabled Rig transport experiment and its explicit fallback rules

The same evidence does **not** support a full compliance claim for the entire
controlled intent bundle. Three material gaps remain explicit:

1. the planned native runtime boundary is still deferred or at least not
   evidenced as implemented;
2. the contract-promised capability/runtime-event surface is not evidenced in
   the public implementation record or static tests; and
3. the contract's tool-descriptor semantics are richer than the public
   `ToolDefinition` surface evidenced by the tests.

Human arbitration is required before treating `agent_runtime` as fully
compliant with its current requirements and contract.

## Implemented behavior evidenced as aligned

| Intent area | Assessment | Static evidence |
| --- | --- | --- |
| `AR-REQ-PROMPT-COMMAND` | Implemented and statically evidenced | `IMPL.md` sections **Prompt acquisition** and **Command template rendering**; `tests/command.rs` covers placeholder expansion, `{prompt_json}`, repeated `{context_files}`, quote rejection, and `{reasoning_effort}` handling; `tests/cli.rs` covers prompt-file execution and per-prompt reasoning-effort selection. |
| `AR-REQ-OUTPUT-PRESENTATION` | Implemented and statically evidenced | `IMPL.md` sections **CLI behavior observed in main.rs**, **Direct transport**, and **Streaming behavior**; `tests/model_cli.rs` covers exact streaming preservation; `tests/cli.rs` covers `run --json` shape, lossy UTF-8 replacement, and `model --json` vs `--show-reasoning` exclusivity. |
| `AR-REQ-SUPERVISION` | Implemented and statically evidenced | `IMPL.md` section **Supervision**; `tests/supervisor.rs` covers success, retry warnings, nonzero exit handling, idle timeout, wall timeout, output exhaustion, descendant cleanup, and retained-output failure; `tests/cli.rs` covers top-level SIGINT cleanup. |
| `AR-REQ-PROFILES` | Implemented and statically evidenced | `IMPL.md` section **Profile storage**; `tests/profiles.rs` covers create/load, formatting preservation, unchanged invalid config, and invalid-name rejection; `tests/cli.rs` covers canonical-vs-legacy path resolution. |
| `AR-REQ-SETUP` | Implemented and statically evidenced for current setup surface | `IMPL.md` sections **Setup wizard behavior**, **Model discovery during setup**, **Setup qualification**, and **Catalog discovery**; `tests/setup.rs`, `tests/catalog.rs`, and `tests/cli.rs` cover bounded discovery, fixed qualification prompt, fallback behavior, acknowledgement gates, current-model defaults, and forced persistence semantics. |
| `AR-REQ-MODEL-TRANSPORT` | Implemented and statically evidenced for direct and Rig transports | `IMPL.md` sections **Model request validation**, **Direct transport**, and **Rig transport**; `tests/model_transport.rs` and feature-gated `tests/rig_transport.rs` cover schema passthrough, endpoint policy, tool-intent mapping, reasoning handling, cancellation, deadlines, response limits, and non-automatic Rig fallback. |
| `AR-REQ-ADAPTER-BOUNDARY` | Substantially aligned in observed transport behavior | `IMPL.md` documents canonical request validation, explicit error mapping, direct/Rig separation, and no automatic replay; `tests/model_transport.rs` and `tests/rig_transport.rs` statically support rejection-first behavior instead of silent downgrade. |

## Implemented versus planned or deferred behavior

### Implemented and evidenced now

- `run`, `model`, `models`, and `setup` CLI surfaces
- profile persistence and resolution
- setup qualification with the fixed prompt `Reply with exactly: OK`
- bounded HTTP catalog discovery for Ollama and llama-server
- bounded ACP catalog discovery for Copilot and Gemini with explicit
  `--allow-host-discovery`
- direct local model transport
- default-enabled Rig transport experiment with explicit `--transport direct`
  selection
- supervision and Linux process-group cleanup

### Planned or deferred, not evidenced as implemented

- the provider-neutral native runtime described in
  `AR-REQ-NATIVE-RUNTIME`
- a public typed broker / host-authorization / execution-backend boundary
- public runtime-event and capability-state surfaces described in the contract
- rich tool-descriptor semantics beyond the minimal request-time tool
  definition shape evidenced by tests

## Discrepancies and explicit gaps

### Finding 1: native runtime boundary remains deferred or not evidenced

**Intent**

- `AR-REQ-NATIVE-RUNTIME` says the planned native runtime MUST separate model
  transport, bounded agent loop, typed tool broker, host authorization,
  execution backend, and redacted runtime events.
- `CONTRACT.md` says the component owns reusable capability, loop, broker,
  execution-backend interface, and runtime event mechanisms, and that the
  planned native runtime requires embedding-host authority interfaces.
- `DESIGN.md` describes those runtime layers as future structure.

**Observed**

- `IMPL.md` documents command rendering, prompt acquisition, profiles, setup,
  catalog discovery, supervision, direct transport, Rig transport, and
  canonical model/request types.
- The observed public library surface in `IMPL.md` does **not** list a native
  loop coordinator, typed tool broker, host-authorization traits,
  execution-backend interface, or runtime-event API.
- The static tests cover command, CLI, setup, catalog, supervision, and model
  transport behavior, but no test names or observed assertions cover a native
  runtime loop or host-authorized tool execution path.

**Assessment**

This is not a minor documentation omission. The controlled intent presents a
native runtime boundary as a real component concern, while the allowed
implementation evidence stops at transport, supervision, and untrusted
tool-intent representation. Treat this scope as **planned/deferred** rather
than implemented. Full compliance with the total current intent bundle is not
evidenced.

### Finding 2: capability-state and runtime-event surfaces are not evidenced

**Intent**

- `REQUIREMENTS.md` requires capabilities to be tracked separately as
  advertised, conformance-tested, and policy-enabled.
- `CONTRACT.md` says provider-neutral model types represent capabilities and
  runtime events.

**Observed**

- `IMPL.md` explicitly documents `ReasoningEffort`, `LocalModelProvider`,
  `ToolChoice`, `ToolIntent`, `ModelUsage`, `ModelTurn`, and `ModelCatalog`.
- No corresponding capability-state data type or runtime-event shape is
  described in the implementation record.
- The static tests exercise capability-related rejection behavior
  (for example, Rig rejecting reasoning-effort requests and Ollama rejecting
  `ToolChoice::Required`), but they do not evidence a separate public
  capability-state model or runtime-event surface.

**Assessment**

This gap may be part of the same deferred native-runtime scope, but it should
still be recorded explicitly because the contract currently describes these as
consumer-visible concepts. On the allowed evidence, they are **not presently
evidenced** as implemented public interfaces.

### Finding 3: the contract overstates the current tool-descriptor surface

**Intent**

`CONTRACT.md` says a tool descriptor records stable ID and version,
input/output schema, effect class, required capability/resource scope,
timeout/output bounds, and retry/idempotency semantics.

**Observed**

- `IMPL.md` re-exports `ToolDefinition`, but does not document its shape.
- In both `tests/model_transport.rs` and feature-gated `tests/rig_transport.rs`,
  `ToolDefinition` is instantiated with only:
  - `name`
  - `description`
  - `parameters`
- No allowed evidence shows fields for versioning, effect class, resource
  scope, timeout/output limits, retry semantics, or output schema on the
  public descriptor type.

**Assessment**

This is a concrete contract-to-implementation mismatch, not merely a missing
test. Consumers cannot currently rely on the richer descriptor semantics
described by `CONTRACT.md` based on the allowed implementation evidence.
Either the contract needs narrowing in a future intent change or the public
tool-definition surface needs expansion and evidence.

## Notes on non-discrepancies that remain intentional

The following current behaviors appear intentional and are supported by the
allowed evidence rather than being compliance failures:

- Rig is the default `model` transport in the default feature set, while
  `--transport direct` remains explicit and failures are not replayed
  automatically.
- Rig rejects reasoning-effort and reasoning-output requests before provider
  I/O; the contract and design already describe that limitation.
- `models` intentionally rejects `llama-cli` and custom-wrapper discovery
  instead of spawning unsupported catalog flows.
- Setup uses current host authority for bounded discovery and qualification and
  does not claim sandboxing or isolation.

## Final assessment

`agent_runtime` has strong static conformance evidence for its current
command/CLI/profile/setup/catalog/supervision/transport implementation.
However, the allowed evidence does **not** justify a clean full-compliance
claim against the entire present requirements and contract bundle. The native
runtime boundary, capability/runtime-event surfaces, and rich tool-descriptor
contract remain deferred or unsupported by the observed public implementation
record and static tests.
