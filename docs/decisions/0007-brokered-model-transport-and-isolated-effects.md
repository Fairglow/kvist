# ADR 0007: Brokered model transport with isolated effects

## Status

Proposed. Accepted in direction; the implementation tier is in progress and is
not yet promoted to `Accepted` until the incremental plan below builds, tests,
and passes independent security and compliance review. This document records
the decision and its rationale; it is not a compliance certification.

## Context

Running any task through `kvist` was blocked: `task run` executed the external
agent by spawning its model command (for example
`curl http://127.0.0.1:9931/v1/chat/completions`) inside a Bubblewrap
namespace created with `--unshare-net`. Inside that namespace `127.0.0.1` is
the sandbox's own empty loopback, so the call failed with curl exit 7 even
when a local model server was running on the host. Agent registration and
host-side model calls (`agent-run model`) succeed, but the task-execution path
never reaches the model.

Kvist must always reach a model service, either on localhost or on whatever
service the provider uses, and package managers used by the source must reach
their registries to fetch dependencies. That connectivity cannot be removed;
it must be mediated. The authoritative networking decision in
[ADR 0005](0005-mediated-dependency-and-model-networking.md) already requires
the model transport and the tool broker to live outside the effect sandbox and
keeps authoring/verification sandbox effects network-denied, so this decision
implements that boundary rather than re-opening it.

## Decision

Adopt a **brokered execution model**:

- A **host-owned model transport + typed tool broker** run outside the effect
  sandbox. The transport performs the model turn; the broker decides
  authorization, holds credential references, validates decoded tool intents,
  selects an execution backend, and records durable evidence.
- The **effect sandbox (Bubblewrap)** runs only atomic, explicitly authorized
  effects (file edits, `cargo test`, `cargo fetch/add`). It never performs a
  model turn.

Transport choice: **adopt `rig-core` as the model-transport seam only.** Use it
for provider request/response conversion, streaming, usage, and tool-call
decoding for local providers. Exactly pin the reviewed release. Keep the
existing native transport as an explicit `--transport direct` fallback.

Do **not** adopt the `rig` facade or `rig-agent`, Rig tool execution,
provider-hosted tools, Rig-owned persistence, image/audio/vector/memory
features, or arbitrary provider parameters. Rig is the transport boundary; it
is never Kvist's authorization, execution, sandbox, or evidence layer.

Initial scope is **local models only**. A gated TLS experiment for remote
providers is explicitly deferred to a later decision.

## Rationale

- **Brokered over in-sandbox model calls.** Moving the model call onto the
  host removes the loopback-isolation failure at the source and matches ADR
  0005. The high-risk surface (arbitrary filesystem/network effects) stays
  isolated; the model turn is low-risk and belongs with the trusted broker.
- **Rig for transport only.** `rig-core` provides the best available
  provider-wire breadth (Ollama, llama.cpp/OpenAI-compatible) at a contained
  surface. Full framework adoption would fold in tool execution, memory, and
  persistence across Kvist's authority boundaries, which the authority-before-
  effect model forbids. Hybrid, not framework adoption.
- **Explicit fallback.** `rig-core` is pre-1.0 with no MSRV and known
  breaking changes, so automatic replay or blind adoption is unsafe. The
  native transport is a manually selected fallback and conformance oracle.
- **Local first, TLS deferred.** The reviewed local build selects no TLS
  backend and makes no HTTPS claim, so remote providers are out of scope until
  a measured, gated TLS experiment is added and re-reviewed.
- **Keep the fallback.** Retaining the native transport is cheaper than
  single-sourcing transport on a pre-1.0 dependency.

## What Rig guards (transport seam)

- Request and response size bounds; numeric loopback routing only.
- Rejects non-numeric endpoints, redirects, and proxies.
- Stream integrity: malformed/truncated SSE/NDJSON fails closed; nonterminal
  and usage-overflowing responses rejected.
- Cancellation and deadline checks before/after every callback.
- Content isolation: payload-bearing tracing is replaced with a no-op
  dispatcher during framework calls; sentinel tests prove prompts, responses,
  and credentials do not reach the caller journal or errors.
- Encapsulation: no `rig` type escapes the private transport adapter.

## What Rig does NOT guard (Kvist must own)

- **Authorization / egress policy.** Rig performs HTTP; it never decides which
  destinations are allowed. The broker decides.
- **Tool authorization, execution, idempotency, effect class.** Decoded calls
  become untrusted `ToolIntent` values; the broker validates and executes them.
- **Redaction policy and evidence.** Rig returns raw provider material; Kvist
  redacts, bounds, and records the durable evidence itself.
- **Credentials and endpoint approval.** Host-approved references only.
- **TLS / remote trust.** Not provided by the reviewed local build.

## Where actions are performed

| Action | Performs it | Network | Boundary |
| --- | --- | --- | --- |
| Model turn, conversion, streaming, tool-call decode | Host broker (Rig/direct transport) | Host loopback now; remote via later gated TLS | Transport seam |
| Authorization, credential refs, `ToolIntent` validation, backend selection, evidence | Host broker | As policy requires | Authority layer |
| File edits | Effect sandbox | None (`deny`) | Isolation, fail-closed |
| `cargo test --locked` | Effect sandbox | None (`deny` + offline + approved cache) | Isolation |
| `cargo fetch`/`add` | Effect sandbox | Allowlisted proxy (`package-sources`) | App-aware proxy |

## Incremental plan (in progress)

1. Structured provider `endpoint` in agent config, validated numeric-loopback,
   defaulting to the local URL; map provider type to `LocalModelProvider`.
2. Host-side `ModelTransport` turn for `task run` (Rig primary, direct
   fallback), replacing the deny-sandbox model call. Sandbox reserved for
   effects.
3. Sequential `ToolIntent` authorization + in-sandbox effect loop.
4. Durable evidence and TRACE/redaction hardening.
5. Gated TLS experiment for remote providers (deferred).

See the component task list for the ordered, traced queue.

## Alternatives considered

- **Allow the in-sandbox agent to reach host loopback + allowlisted egress**
  ("share host network + proxy"). Achievable but weaker: non-proxy tools gain
  open egress, and it keeps the model turn inside the boundary we deliberately
  moved out. Rejected in favor of brokered.
- **Limit Bubblewrap to local models only.** Conflates model locality (a broker
  property) with sandbox policy. The brokered design is independent of whether
  the model is local or remote and scales to both.
- **Full `rig-agent` adoption.** Crosses authorization-before-execution;
  rejected (see above).
- **Per-agent port/destination declaration.** Untrusted agents cannot be trusted
  to report what they need; ADR 0005 rejects IP/port allowlists in favor of an
  application-aware proxy. Rejected.

## Consequences

Task execution is unblocked for local providers while preserving the ADR 0005
authority model. Maintenance carries the measured rig footprint (about +99
packages and +4.3 MiB over the direct build, above the 75-package advisory
guideline but not a hard blocker) and requires exact-version re-qualification
on every upgrade. Remote providers remain out of scope until the deferred TLS
experiment is built and reviewed.
