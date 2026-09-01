<!-- kvist-compliance-review-version: 1 -->
# Agent Runtime Compliance Review

## Review identity and method

This is an independent, source-blind compliance review of the `agent-runtime`
component. The reviewer compared the component's approved intent
(`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`), the immediate parent
`../CONTRACT.md` (`kvist.engine`), and the global `ROOT_CONTRACT.md` against the
independently derived `IMPL.md`.

The reviewer did **not** read source files, tests, `Cargo.toml`/manifests, Git
history, `TODOS.yaml`, any prior compliance report, prior reviews, architecture
or vision documents, or chat history. Observed-behavior evidence is taken solely
from `IMPL.md` plus the bounded verification evidence recorded below. The prior
report was replaced without being read.

## Bounded verification evidence considered

The following out-of-band verification signals were supplied for this review and
are treated as static acceptance evidence only:

- All-feature workspace `fmt`, `check`, and `clippy` clean, and 101 tests
  passing, prior to the final small CLI addition.
- After the final CLI addition: the four targeted `models_*` CLI tests and
  all-feature `agent-runtime` `clippy` passing.
- Model-catalog, setup, CLI, and root wizard tests passing.
- Live installed Copilot and Gemini catalogs listed successfully, including the
  `auto` entry.
- A final code review reporting no significant issues prior to the final
  explicit unsupported-provider `models` CLI addition.

These signals do not include a source-blind reviewer-run test invocation; they
are accepted as reported. `IMPL.md` itself notes that no test command was run
while it was derived, so live pass/fail status is asserted from these external
signals rather than reproduced here.

## Requirement-level assessment

| Requirement | Verdict | Basis |
| --- | --- | --- |
| AR-REQ-PROMPT-COMMAND | Compliant | `IMPL` resolves exactly one bounded prompt source, renders documented placeholders with a shell-free parser, rebuilds each retry from fresh input with an appended prior-attempt/uncertain-side-effect notice, supports multiple per-`run` selectable named profiles, and fails a requested reasoning effort when the template lacks `{reasoning_effort}` rather than dropping it or converting it to shell text. |
| AR-REQ-OUTPUT-PRESENTATION | Compliant | Plain text forwards only bounded provider content with no trailer; JSON captures streams and emits one `{"content":...}` object with lossy U+FFFD replacement; direct model mode keeps provider reasoning separate, surfaces it only via `--show-reasoning` (stderr) or canonical JSON, and never labels it hidden chain-of-thought. |
| AR-REQ-SUPERVISION | Compliant | Bounded streaming, idle/loop detection, cancellation, wall/output limits, process-group termination and reaping before return/retry, and `OutputStreamsRetained` for escaped descendants are all present; only idle timeout and detected stdout repetition retry. |
| AR-REQ-PROFILES | Compliant | Strict bounded TOML (`schema_version = 1`, 64 KiB, 128 profiles, name/provider/command bounds), full pre- and post-mutation validation, formatting-preserving `toml_edit` upsert, safe XDG/HOME resolution with canonical-over-legacy precedence, and synchronized atomic replace of a real regular non-link file. |
| AR-REQ-SETUP | Compliant | Terminal-neutral I/O, maintained provider templates, bounded probes, catalog-first numbered selection with manual entry gated behind a synthetic final custom choice, HTTP discovery for Ollama/llama-server and ACP session model lists for Copilot/Gemini, visible discovery-failure fallback plus custom, manual identification for file/wrapper providers, mandatory fixed-prompt qualification (`Reply with exactly: OK`) with no second acknowledgement, non-persistence on failure except `--force`, and bounded non-forwarded qualification output. The standalone `models` command lists the same catalogs non-interactively in text/JSON with time/byte/count/ID bounds, sends no prompt, cleans up helpers, and requires `--allow-host-discovery` for ACP providers. |
| AR-REQ-MODEL-TRANSPORT | Compliant | `DirectModelTransport` implements bounded unary and streamed Ollama (`/api/chat`) and llama-server (`/v1/chat/completions`) with explicit endpoints, requests, responses, deadlines, cancellation, identities, usage, finish reason, and typed errors, and translates tool intent without executing tools. |
| AR-REQ-NATIVE-RUNTIME | Approved-deferred | The provider-neutral native loop, typed broker, host-authorization traits, execution backend, and redacted runtime events are described as planned/future in intent and are not implemented. Deferral is explicitly sanctioned by the requirement ("planned") and the design's future-layers note; canonical tool-intent types exist but no effectful path. |
| AR-REQ-ADAPTER-BOUNDARY | Compliant | The optional `RigModelTransport` is private, exact-pinned, default-disabled, immediately translated to canonical types, registers no tools, converts provider tool calls to untrusted intents, and cannot own policy, authorization, evidence, or public serialized state. |

## Contract-clause assessment

- **Provided CLI surface** (`setup`, `models`, `run`, `model`): Present as
  specified, including `run`'s exactly-one-of `--command`/`--profile` plus
  mandatory `--allow-host-execution`, and `model`'s tool-free no-host-execution
  behavior with Rig rejecting reasoning/effort. Compliant.
- **Provider input matrix / defaults**: `ollama` `127.0.0.1:11434`,
  `llama-server` `127.0.0.1:9931`, `copilot`/`gemini` executable defaults, ACP
  acknowledgement required outside setup, and `--executable`/`--endpoint`
  cross-rejection all match. Compliant.
- **Unsupported catalogs**: `llama-cli` and `custom-script` return an explicit
  `UnsupportedCapability` "provider model catalog" error before any spawn or
  directory lookup, matching "return an explicit unsupported-catalog error
  without spawning discovery." This is the final CLI addition and is Compliant.
- **`models` output shape**: One validated model ID per line (text) and one
  object with `format_version: 1`, `provider`, nullable `current_model_id`, and
  ordered `models` (`id`/`name`/optional `description`) with printable-ASCII,
  brace-free IDs and control-free bounded names/descriptions. Compliant.
- **Catalog selection semantics**: First-occurrence dedup in provider order,
  current-model retention only when the exact ID is present else first-model
  default, empty/wholly-invalid catalog treated as discovery failure, and
  `Other model ID...` appended only at presentation. Compliant.
- **Setup qualification/behavior**: Fixed prompt, no test-prompt/second-ack
  prompts, failure non-persistence except `--force` (non-interactive, visible),
  bounded captured (not forwarded) qualification output. Compliant.
- **ACP discovery**: Spawns `<exe> --acp` in a new process group, exchanges only
  `initialize` (protocol version 1) and `session/new` with absolute `cwd` and
  empty `mcpServers`, advertises no client fs/terminal/MCP capability, rejects
  provider-to-client requests and mis-correlated IDs, bounds records/bytes/time,
  and unconditionally reaps. Matches contract and design. Compliant.
- **Model/JSON/reasoning presentation**: `--json` is one canonical `ModelTurn`
  and mutually exclusive with `--show-reasoning`; plain text adds no absent line
  termination; Rig rejects reasoning presentation. Compliant.
- **Errors/security/authority**: Redacted HTTP-status errors omit provider
  bodies; local transports and catalog HTTP enforce numeric-loopback,
  explicit-port, no-proxy/redirect/TLS/DNS restrictions; host execution named as
  non-isolated; `--allow-host-execution` / `--allow-host-discovery`
  acknowledgements present. Compliant.
- **Tool descriptor data shape** (stable ID/version, input/output schema, effect
  class, capability/resource scope, timeout/output bounds, retry/idempotency):
  The full broker-facing descriptor is part of the deferred native runtime.
  `IMPL` documents model-facing tool *definitions* (name/description/parameters)
  and untrusted tool *intents*, but not the complete effect-class/capability
  descriptor. Approved-deferred with the native runtime (see discrepancy D-1).

## Parent- and root-contract assessment

- **Parent (`kvist.engine`) required interface**: The parent requires
  `agent-runtime.library/v1` for prompt acquisition, command rendering,
  profiles, setup, process supervision, and model transports. `IMPL` exports all
  of these. The parent's `agent setup` catalog-first, acknowledgement, and
  qualification-failure semantics are consistent with the runtime behavior the
  component exposes. Compliant.
- **Root contract**: Linux-only executable target (compile-time non-Linux
  rejection), unsafe forbidden, shell-free direct spawning, explicit one-off host
  acknowledgement for host execution/discovery, and version-marked artifacts are
  all honored. Compliant.

## Design-conformance assessment

Observed structure matches the design's module responsibilities and algorithms:
shell-free command parsing with `{prompt_json}` full JSON encoding and
option-elision for empty standalone placeholders; formatting-preserving profile
mutation with atomic persistence; catalog normalization into a bounded ordered
`ModelCatalog` with adapter-owned fallbacks used only on discovery failure;
minimal ACP v1 client; bounded loopback-only direct HTTP with suppressed Rig
tracing; and narrow idle/loop-only retry with process-group cleanup. No design
deviation was identified. Compliant.

## Discrepancy register

- **D-1 (Approved-deferred).** The contract/requirements describe a
  provider-neutral native runtime with a typed broker and full tool descriptors
  (effect class, capability/resource scope, timeout/output bounds,
  retry/idempotency) plus host-authorization backends. `IMPL` implements the
  canonical types and untrusted tool-intent conversion but not the effectful
  loop/broker/backend or the complete descriptor shape. Intent explicitly marks
  this as planned/future; no misrepresentation of it as implemented was found.
  Not a blocker.
- **D-2 (Underspecified, non-blocking).** The contract's `models` interface
  signature enumerates `--provider/--endpoint/--executable/--allow-host-discovery/--json`,
  while `IMPL` also exposes `--timeout` and `--max-response-bytes`. These flags
  materialize the requirements' mandated time/output-byte discovery bounds and do
  not contradict any clause; the contract simply does not enumerate them. Refine
  the contract interface listing to name these bound controls. Not a blocker.
- **D-3 (Underspecified, non-blocking).** The contract states Gemini's advertised
  or fallback `auto` is stored explicitly as `--model auto`. `IMPL` records
  Gemini's `--model` template and `auto` as the documented Copilot/Gemini
  discovery fallback, and the bounded evidence confirms live catalogs listed
  `auto`, but `IMPL` does not restate the exact `--model auto` storage wording.
  This is an `IMPL` description gap rather than a behavioral conflict. Consider
  making the `IMPL` note explicit. Not a blocker.

No mismatched (intent-vs-observed conflicting) discrepancies were identified.

## Conclusion

- Requirements AR-REQ-PROMPT-COMMAND, AR-REQ-OUTPUT-PRESENTATION,
  AR-REQ-SUPERVISION, AR-REQ-PROFILES, AR-REQ-SETUP, AR-REQ-MODEL-TRANSPORT, and
  AR-REQ-ADAPTER-BOUNDARY are **compliant**.
- AR-REQ-NATIVE-RUNTIME and the associated full tool-descriptor contract shape
  are **approved-deferred** consistent with intent (D-1).
- D-2 and D-3 are **underspecified** documentation-refinement items, not
  behavioral conflicts.

**Blocker status: NO BLOCKER.** The implemented, in-scope behavior conforms to
the requirements, consumer contract, parent contract, root contract, and design.
The open items are a sanctioned deferral (D-1) and two non-blocking
documentation refinements (D-2, D-3) recorded here for explicit human
arbitration rather than silent artifact edits. This review is advisory acceptance
evidence and does not itself constitute a Kvist review receipt.
