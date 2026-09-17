# ADR 0009: Brokered model transport with isolated effects

## Status

Accepted. The decision and its implementation are in place: the supervised task
path performs the model turn on the host against a numeric loopback gateway,
brokers the turn's tool intents, and applies every authorized effect inside the
effect sandbox. This document records the decision and its rationale; it is
not a compliance certification.

## Context

Running any task through `kvist` was blocked: `task run` executed the external
agent by spawning its model command (for example
`curl http://127.0.0.1:9931/v1/chat/completions`) inside a Bubblewrap
namespace created with `--unshare-net`. Inside that namespace `127.0.0.1` is
the sandbox's own empty loopback, so the call failed with curl exit 7 even
when a local model server was running on the host. Agent registration and
host-side model calls (`agent-run model`) succeed, but the task-execution path
never reaches the model.

The first revision of this decision proposed adopting `rig-core` as a
model-transport seam with the native transport kept as a fallback. That
direction was reversed: the native transport is retained as the **sole** model
transport and `rig-core` was removed from the dependency graph. The brokered
boundary itself — model turn and tool broker outside the effect sandbox,
effects isolated inside it — is unchanged and implements the authoritative
networking decision in
[ADR 0005](0005-mediated-dependency-and-model-networking.md), which requires
the model transport and tool broker to live outside the effect sandbox and
keeps authoring/verification sandbox effects network-denied.

## Decision

Adopt a **brokered execution model** with the native transport as the sole
model transport:

- A **host-owned model transport + typed tool broker** run outside the effect
  sandbox. The transport performs the model turn over a numeric loopback
  gateway; the broker (`kvist::authoring`) reduces the turn's untrusted tool
  intents to capability-bound effects and records durable evidence.
- The **effect sandbox (Bubblewrap)** runs only atomic, explicitly authorized
  effects. It never performs a model turn.
- **Non-loopback commands are refused.** The selected model command must
  target a numeric loopback model gateway; anything else fails the turn with
  `AgentCommandNotModelGateway`. No agent command ever runs on the host
  outside the effect sandbox, so there is no host-subprocess fallback and no
  double execution.

The turn advertises the closed authoring tool set (`write_file`, `edit_file`).
Each intent the model proposes is reduced by the broker to a `CheckedIntent`
under a deny-by-default policy; a dropped intent fails the turn. Every
authorized effect is applied by the kvist binary itself inside the effect
sandbox (`kvist authoring-apply`) against a read-only staged-intent mount at
`/workspace/authoring/intent.json`; the host never writes component state for
an effect.

The model phase runs under one shared wall-clock budget (`TurnBudget`, the
profile timeout) covering the liveness probe, every turn attempt, and retry
backoff; the per-attempt transport deadline is the remaining budget. The
gateway is liveness-probed (TCP connect only, no HTTP) before the first
attempt, and transient availability failures are retried with a short fixed
backoff, bounded by the attempt limit and the shared budget. When `--stream`
is set, streamed text deltas are relayed to standard output with the run's
redaction values applied; a streamed attempt that has emitted text is never
retried.

Initial scope is **local models only.** A gated TLS experiment for remote
providers is explicitly deferred to a later decision.

## Rationale

- **Brokered over in-sandbox model calls.** Moving the model call onto the
  host removes the loopback-isolation failure at the source and matches ADR 0005. The high-risk surface (arbitrary filesystem effects) stays isolated;
  the model turn is low-risk and belongs with the trusted broker.
- **Native transport only.** The native `DirectModelTransport` already covers
  the local provider wire (Ollama, llama.cpp/OpenAI-compatible) with bounded,
  fail-closed decoding. Single-sourcing transport on a pre-1.0 external
  dependency was not justified; the framework's tool execution, memory, and
  persistence would also cross Kvist's authority boundaries, which the
  authority-before-effect model forbids.
- **Refuse non-loopback rather than run it.** A host subprocess fallback would
  let an arbitrary configured command run with full host authority and then be
  re-executed in the sandbox (double execution). Failing closed keeps every
  agent command inside the effect sandbox or refused outright.
- **One shared budget.** A per-attempt deadline plus separate probe time let
  the model phase exceed the configured timeout by roughly 3x. A single shared
  budget bounds the whole phase to the configured timeout plus scheduling
  slack.
- **Local first, TLS deferred.** The reviewed local build selects no TLS
  backend and makes no HTTPS claim, so remote providers are out of scope until
  a measured, gated TLS experiment is added and re-reviewed.

## Where actions are performed

| Action                                                  | Performs it                                    | Network                                        | Boundary               |
| ------------------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | ---------------------- |
| Model turn, conversion, streaming, tool-call decode     | Host transport (native `DirectModelTransport`) | Host loopback only; remote via later gated TLS | Transport seam         |
| Liveness probe, bounded retry, shared wall-clock budget | Host                                           | Loopback TCP connect                           | Transport seam         |
| Tool-intent authorization, evidence, redaction          | Host broker (`kvist::authoring`)               | As policy requires                             | Authority layer        |
| File edits (staged `authoring-apply`)                   | Effect sandbox                                 | None (`deny`)                                  | Isolation, fail-closed |
| `cargo test --locked`                                   | Effect sandbox                                 | None (`deny` + offline + approved cache)       | Isolation              |
| `cargo fetch`/`add`                                     | Effect sandbox                                 | Allowlisted proxy (`package-sources`)          | App-aware proxy        |

## Alternatives considered

- **`rig-core` as a transport seam (this decision's first revision).** Rejected
  in favor of keeping the native transport as the sole seam: it avoids a
  pre-1.0 dependency with no MSRV and keeps the transport surface minimal and
  directly owned.
- **Allow the in-sandbox agent to reach host loopback + allowlisted egress**
  ("share host network + proxy"). Achievable but weaker: non-proxy tools gain
  open egress, and it keeps the model turn inside the boundary we deliberately
  moved out. Rejected in favor of brokered.
- **Host subprocess fallback for non-loopback commands.** Runs an arbitrary
  configured command on the host with full authority, then re-executes it in
  the sandbox. Rejected in favor of refusing non-loopback commands.
- **Per-agent port/destination declaration.** Untrusted agents cannot be trusted
  to report what they need; ADR 0005 rejects IP/port allowlists in favor of an
  application-aware proxy. Rejected.

## Consequences

Task execution is unblocked for local providers while preserving the ADR 0005
authority model. The model phase is bounded by one shared wall-clock budget,
every agent command is either a numeric loopback gateway or refused, and every
authorized effect is applied only inside the effect sandbox by the kvist
binary itself. Remote providers remain out of scope until the deferred TLS
experiment is built and reviewed. Committed attempt journals retain the
machine-absolute paths recorded at execution time; rewriting them would
falsify the evidence record, so they are preserved as-is.
