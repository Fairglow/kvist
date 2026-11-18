# Sandboxed external agents in Skott: Copilot first

**Status:** Proposal for human review, 2026-11-18. Not an accepted design or a
new Skott interface.

## Recommendation

Run **the Copilot CLI itself**, including its built-in file tools and every
local subprocess, as a session payload in Galla. Keep Galla's network-denied
authoring namespace. Let Copilot own the agent loop, use a configured local or
bring-your-own-key (BYOK) model, and reach it only through a run-scoped,
inference-only channel to a host-owned broker. Start with a single prompt to
qualify the boundary. Use **Agent Client Protocol (ACP) over stdio** for the
first-class Skott interface: a shared adapter in Sav, with Skott owning the
workspace grant, presentation and operator controls.

This supports the **Copilot coding harness** without `--allow-host-execution`.
It does **not** make a GitHub Copilot subscription a BYOK credential or bring
GitHub-hosted features inside Galla. Investigate the official Copilot SDK
separately if using subscription models is essential. Do not adopt Rig for
this integration, and do not weaken the sandbox to get subscription parity.

## What “all effects inside Galla” means

| Requirement | Feasibility |
| --- | --- |
| All agent-initiated **local** file, command and subprocess effects occur under Galla | Yes. Put the whole agent inside Galla, with only the approved workspace writable plus private scratch. Galla enforces the outer grant; Copilot's own tool filters are an additional restriction, not authorization. |
| No general network, host home or credentials available to the agent or its tools | Yes, with an isolated namespace and a narrowly mediated model channel. For work that cannot leave the machine, use local inference and no external network. |
| No effects **anywhere outside** the sandbox, including inference, billing, logging, API writes or cloud agents | Not with a hosted model. A model call discloses prompt and tool context to the approved service and may incur cost or retention. Host-owned journaling is intentional. GitHub MCP, Code Search, `/delegate`, remote tools, publishing and cloud agents are not part of this tier. |
| Full Skott-native policy, accounting and evidence for each internal tool action | Not with an opaque Copilot loop and a writable workspace. ACP reports what the agent says it did; it does not prove or exclusively mediate what ran. Use a typed broker and prohibit other effect paths if per-action authority is required. |

This follows the [root contract](../../ROOT_CONTRACT.md) and
[ADRs 0005](../decisions/0005-mediated-dependency-and-model-networking.md),
[0009](../decisions/0009-brokered-model-transport-and-isolated-effects.md) and
[0015](../decisions/0015-shared-runtime-and-run-inspection.md): model transport
and credentials stay outside the effect sandbox; agent output is not authority.
The host broker is a narrow exception for inference, not a way for tools to
perform host actions. This proposal is for **Skott workspace runs**. Maerg's
protected tasks must keep rejecting external agents that cannot satisfy its
typed broker and evidence contract.

## What exists and what was exercised

- **Skott/Sav:** Skott's [external runner](../../skott/src/external.rs) currently
  renders a [Sav external profile](../../sav/src/registry.rs), supervises a
  single unconfined host process with mandatory logs and no automatic retry,
  and requires terminal acknowledgement. It rejects headless external runs.
  Sav's [ACP catalog client](../../sav/src/catalog.rs) only initializes and
  opens a no-prompt session for model discovery. It does not drive turns or
  qualify isolation, and it must not be invoked implicitly on the host by a
  sandboxed profile's startup.
- **Galla:** The [request CLI](../../galla/src/main.rs) reads one bounded JSON
  request from stdin through EOF. The [payload launcher](../../galla/src/enforcement.rs)
  gives the payload null stdin and forwards bounded stdout/stderr. Existing
  Skott [sandbox dispatch](../../skott/src/sandbox.rs) is a one-request,
  captured-output tool call, not a duplex session. Individual argv values
  are capped at 4 KiB; the current external runner's 64 KiB prompt cannot
  just become one Galla argument. Galla's package-source mode is Cargo-only,
  unavailable for authoring, and its proxy environment is not a suitable
  agent egress boundary: the current implementation does not unshare its
  network namespace in that mode.
- **Local smoke evidence:** The installed **Copilot CLI 1.0.112** started and
  returned an ACP v1 `initialize` response in a bare Bubblewrap namespace
  with cleared environment, private home, no credentials and no external
  network. Separately, the installed **Galla v1** runner accepted a
  `network: deny` authoring request that ran Copilot in offline BYOK mode with
  an in-namespace synthetic OpenAI-compatible SSE server. The first
  `/v1/chat/completions` response proposed a file-creation call; the second
  request contained the tool result. Copilot exited successfully and the
  disposable workspace contained the exact expected file. The installed npm
  launcher failed to unpack with 64 MiB of scratch; the repeat with a 1 GiB
  ceiling passed. That is a packaging observation, not a universal minimum.
  These probes used no real project data, account or model. They do not prove
  the host broker, ACP turns through Galla, real-model compatibility,
  subprocess lifecycle under cancellation, or subscription authentication.

## Proposed integration

### Launch and workspace boundary

1. **Provision explicitly.** Require a versioned Copilot runtime and its
   dependencies installed outside the workspace (or a narrow read-only
   runtime grant). Resolve the actual Linux executable/bundle, including
   interpreters and libraries as applicable. Record the observed version and
   runtime identity; a launcher path alone does not identify a package.
   Never install, auto-update or acquire dependencies on run startup.
2. **Make the profile explicit.** Pin the backend, transport, selected model,
   workspace and network policy for the run. Build an absolute, shell-free
   launch target with sandbox-relative paths and a purpose-built environment.
   Use private `HOME`/`COPILOT_HOME` and a Skott-supplied, secret-free provider
   configuration. Do not inherit host tokens, auth helpers, credential
   sockets, SSH agents, Docker sockets, host session stores or operator
   controls. Use the existing offline toolchain/cache resources, read-only
   where applicable. Dependency acquisition stays a separate operator action.
3. **Minimize what is mounted and enabled.** Start with a disposable checkout
   or an explicitly approved live workspace. A writable mount permits direct
   writes throughout that mount; `.gitignore` and Copilot tool settings are
   not exclusions. Keep secrets, canonical Kvist artifacts, logs and control
   files out of the granted tree; exclude Git administrative state initially
   unless deliberately included in the disposable scope. Do not expose host
   sockets or special files through workspace grants. Do not auto-trust
   project hooks, plugins, extension servers or user settings; start with
   file/search/shell tools, no remote MCP or native Copilot delegation.
   Copilot's [own sandbox](https://docs.github.com/en/copilot/concepts/about-cloud-and-local-sandboxes)
   does not OS-confine its in-process file tools. Galla must remain the outer
   boundary even if Copilot's sandbox is enabled. Qualify nesting when
   managed policy requires it; fail rather than override enterprise policy.
4. **Separate launch control from payload input.** Add an explicit,
   versioned Galla session mode with separate bounded launch metadata and
   payload I/O. For example, pass validated launch metadata on a dedicated
   descriptor and reserve stdio for ACP. Keep runner lifecycle evidence
   distinguishable from payload output. Do not reinterpret existing v1 JSON
   after EOF or pass credentials/prompt text through argv. The Skott owner
   holds the session and cancellation capability; Galla owns and reaps the
   namespace and its descendants. Preserve one-shot v1 behavior for existing
   tools and verification.

### Inference boundary

Use the documented Copilot [custom-provider configuration][cli-models] and
[`COPILOT_OFFLINE=true`][cli-auth] for a credential-free runtime pointed at a
synthetic or real local endpoint. The flag suppresses GitHub authentication
and telemetry; **it is not a network control**. An Ollama or llama-server on
host `127.0.0.1` is not reachable at that address from Galla's private
network namespace. A co-located synthetic server is a good test fixture; for
production, keep the trusted model service outside the writable effect
sandbox. Qualify a real endpoint for streaming, tool calling, model identity
and context limits. Start with one tested wire format, such as OpenAI Chat
Completions; qualify Responses or other APIs separately.

For ACP/CLI, use one run-private relay on **sandbox loopback** crossing the
boundary only via an explicitly granted IPC capability (for example, an
owned descriptor to the broker; the exact mechanism belongs in Galla's
session contract). No host loopback sharing, general HTTP proxy, host socket
directory, or host filesystem grants for the relay. The broker binds one
approved upstream and model server-side, constructs its own request and adds
credentials only on the host when needed. Start by allowing only
`POST /v1/chat/completions`; reject arbitrary authorities, redirects,
authorization headers from the payload, and unrelated endpoints. Permit tool
schemas for local calls, not provider-managed browsing, code execution or
uploads. Bound request and response size, concurrency, idle and total time,
and model usage/cost where observable. Tie broker lifetime and cancellation
to the run; don't use transparent forwarding as the implementation of this
policy.

Treat the channel as usable by **any process in the sandbox**, regardless of
how it's handed to Copilot. It cannot safely carry arbitrary HTTP, spawn
commands or return credentials. Even a fixed inference endpoint can transmit
any readable workspace data in a prompt. Default to local inference. Remote
BYOK needs an explicit architecture decision for the opaque workspace tier
and independent opt-in policy for provider, data scope, retention and
spending before enabling it; no project data should be sent just to qualify
a profile. No retries of uncertain workspace effects or whole runs.

### Agent protocol and Skott UX

- **One-shot qualification:** use the [programmatic CLI][cli-programmatic]
  with a pinned version, bounded prompt input and JSONL output, narrow tool selection and no
  interactive login. Exercise a real edit and offline test command. Keep
  it a supportable batch fallback only when it enforces the same boundary;
  no second, weaker execution path.
- **First-class sessions:** Sav negotiates [Copilot ACP][cli-acp] v1, creates a
  session in the sandbox cwd, sends prompt turns, and maps output, plans, tool
  updates and stop reasons into the common journal. Skott exposes the same
  backend in TUI and headless mode, with a scope badge and a readable,
  bounded, redacted host-owned transcript. Do not copy the existing opaque
  runner's raw-byte logging into the new path. Cap frame sizes, queue lengths
  and total throughput; protocol and stderr stay separate. Never advertise
  host filesystem or terminal callbacks. Deny unsupported reverse requests;
  interactive permission dialogs or a preapproved headless policy can only
  narrow the sandbox grant. ACP is in public preview: pin and qualify the
  CLI/protocol pair, and surface unsupported capabilities.
- **Lifecycle:** acknowledge cancellation, ask ACP to stop, and enforce a
  deadline before tearing down the entire sandbox and broker. Dropping an
  observer does not cancel the owner; a mandatory recording failure does.
  Report whether the agent finished, was denied, failed, was cancelled, or
  left uncertain edits. Never present process exit as human acceptance or
  rollback. Start with follow-up prompts at turn boundaries and no persisted
  Copilot resume. Expose live steering, resume, token or tool limits only
  when the selected transport actually implements them.

## Alternatives and dependencies

| Option | Assessment |
| --- | --- |
| **Programmatic Copilot CLI only** | Smallest useful trial. No new framework. Good for batch, but subprocess JSONL does not replace session negotiation, permission handling or reliable multi-turn control. |
| **ACP via a small Sav client or the [official Rust ACP crate](https://agentclientprotocol.com/libraries/rust)** | Preferred for full workspace UX and adding another compatible agent. Reuse Sav's profile, discovery and journal concepts; don't stretch the discovery-only client into the production engine without separate lifecycle design. Prefer the pinned ACP crate if its dependency/MSRV, framing bounds and lifecycle integrate cleanly; otherwise implement only the qualified ACP subset. Neither choice enforces effects. |
| **Official [Copilot SDK for Rust][sdk] with a Galla-owned subprocess** | Useful for Copilot-specific events, callbacks and possibly model mediation without a loopback relay. Recent upstream Rust code documents a [model HTTP/WebSocket request handler][sdk-handler] for both GitHub Copilot API (CAPI) and BYOK traffic. Spike it with a compatible pinned runtime and Galla-owned transport. `Client::from_streams` can attach to supplied streams; independently verify whether the selected release supports registering the model handler on that path or needs a Galla-aware spawned-stdio adapter. Do not use the SDK's default in-process runtime in Skott or automatic runtime downloads. A callback that **returns** a token to the runtime is not credential isolation. Prove that login, entitlement/policy, refresh and inference work with raw and derived credentials retained on the host and no general forwarding. Inspect what responses are returned to the runtime as well as requests. No evidence yet that the normal subscription path meets that bar. If a supported host-only flow is unavailable, ship BYOK only. |
| **Host agent with only typed tools, every tool effect delegated to Galla** | Potential route to per-action review or normal hosted authentication. Requires a runtime mode that guarantees all built-ins, shell, hooks, remote tools and other effect paths are off; optionally use a narrow custom tool surface, not a generic host MCP server. ACP permissions and documented provider filters alone do not prove this. The effect boundary can be sound, but this design does not put the whole external agent in Galla. Do not use it to qualify protected Maerg work without a separate design. |
| **Rig library / Rig coding agent** | [Rig](https://github.com/0xPlaygrounds/rig) provides provider adapters, tool loops and a separate coding agent. It does not turn Copilot into a sandboxed process, solve subscription credential separation, or replace Galla's enforcement. ADR 0009 already removed the `rig-core` experiment. A pinned Rig CLI could later be another external payload under the same boundary; embedding Rig today duplicates Sav/Skott loop machinery and expands dependencies without solving the integration. |
| **Copilot's own sandbox, Docker/VM, host execution, or native Skott** | A Copilot sandbox or container alone is not Galla and has a different authority model; a separately approved container/VM could be defense in depth, not a shortcut. Keep explicit host execution distinct, never a fallback. Native Skott with a local model remains the option when precise native tool control matters more than Copilot's loop. Instructions and skills alone cannot enforce the boundary. |

## Delivery and effort

Rough **engineering time for one engineer familiar with the code**, assuming
a pinned Linux runtime and an available compatible local model. Ranges include
targeted tests and hardening; external approval, independent review scheduling
and vendor changes are not calendar commitments.

| Increment | Exit criterion | Estimate |
| --- | --- | --- |
| Contained spike | Reproducible fixture through installed Galla; real local model can consume the restricted channel and edit/run an offline test; decide whether the SDK callback is simpler than a relay. Do not ship the spike as a security boundary. | 3–5 days |
| Supported sandboxed batch | Approved Galla session/IPC contract and broker policy, pinned runtime, bounded I/O, cancellation and ownership, actionable startup errors, Skott headless journaling, no fallback. | +1.5–2.5 weeks |
| First-class ACP workspace backend | Sav session adapter, Skott TUI/headless parity, permissions, inspection, interoperability fixtures, docs and independent confinement review. | +1.5–2.5 weeks |
| Remote BYOK or GitHub subscription | Remote BYOK adds approved TLS/credential lifecycle and data/cost policy. Separately time-box an SDK/CAPI compatibility spike; estimate supported subscription work only after the credential-flow gate passes. | Remote BYOK +1–2 weeks; subscription spike 3–5 days, delivery unknown |

The local supported path is roughly **4–6 engineer-weeks** through ACP.
The major cost is the cross-process authority and lifecycle work, not command
rendering or JSON parsing. Galla changes are security-sensitive; Sav gains a
parallel session backend, not a replacement for `ModelTransport`; Skott gets
explicit profile startup and presentation; Maerg retains its protected policy.
No new mandatory daemon, cloud dependency, or change to native runs is needed.

Before implementation, agree on the workspace tier and local-only default,
then update the affected component intent and ordered task queues. Gate
release on deterministic fixtures that exercise successful edits and offline
tests; no host file/credential/control access or general network; exact broker
routes and budgets; malformed frames and denied callbacks; cancellation,
orphan cleanup, backpressure, journal failure and absence of replay. Test that
missing sandbox/runtime/policy fails closed and that protected task rejection
and native execution stay unchanged. Requalify supported Copilot releases
against the fixture and one explicitly approved real-model run. Follow
implementation with an independent security audit and separate source-only
observation/source-blind compliance review.

[cli-models]: https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-llm-models
[cli-auth]: https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli#offline-mode
[cli-programmatic]: https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference
[cli-acp]: https://docs.github.com/en/copilot/reference/copilot-cli-reference/acp-server
[sdk]: https://github.com/microsoft/copilot-sdk/blob/main/rust/README.md
[sdk-handler]: https://github.com/microsoft/copilot-sdk/blob/main/rust/src/copilot_request_handler.rs
