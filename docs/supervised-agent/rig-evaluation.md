# Rig Evaluation for Kvist

## Decision

Evaluation date: 2026-08-30.

Rig remains directionally promising as a private implementation of the
`supervised-agent` model-transport boundary. The evaluated `rig-core` 0.42.0
release is rejected because it fails Kvist's Rust 1.85 gate. Its large
dependency graph is an additional warning, but was measured against bare Tokio
rather than the not-yet-implemented direct adapter and is not presented as a
marginal comparison. Rig is also not suitable as Kvist's authorization layer,
tool broker, execution backend, sandbox, or durable evidence store.

The experiment used the immutable published `rig-core` 0.42.0 crate, exactly
pinned with default features disabled. That release still directly depends on
Reqwest and Tokio; disabling default features removes additional Reqwest
behavior, derive support, and TLS selection, but does not produce a
transport-free core.

Do not adopt the `rig` facade or `rig-agent`. Do not make Rig messages, tools,
errors, completion requests, provider payloads, or serialized state part of a
public or durable Kvist interface.

Implement the first local Ollama/llama-server transport directly behind the
standalone canonical seam. A later immutable Rig release may be reevaluated
only if a cheap preflight first demonstrates Rust 1.85 compatibility and an
acceptable dependency graph.

## Evidence baseline

The adoption baseline is the immutable
[`v0.42.0` tag](https://github.com/0xPlaygrounds/rig/tree/v0.42.0), commit
`d5a34986a1ad57f1e9c5984b82f8d7438ffc717e` (annotated tag object
`1dc55ad47370849fd516f7a83e4bb1ea9e2b6e68`).

At that tag:

- the workspace version is 0.42.0 and the license is MIT;
- the repository toolchain is Rust 1.94.0;
- package manifests do not declare a `rust-version`;
- `rig-core` directly depends on Reqwest with JSON, streaming, and multipart,
  Tokio runtime/synchronization, tracing, event-source handling, schema and
  serialization crates;
- `rig-core` defaults enable Reqwest extras, derive, and Rustls;
- `rig-agent` is a separate crate;
- the later `rig-run`, `rig-reqwest`, and `rig-rmcp` splits do not exist as
  sibling crates in this release.

Sources:

- [v0.42.0 workspace manifest](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/Cargo.toml)
- [v0.42.0 rig-core manifest](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/crates/rig-core/Cargo.toml)
- [v0.42.0 Rust toolchain](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/rust-toolchain.toml)
- [v0.42.0 rig-agent](https://github.com/0xPlaygrounds/rig/tree/v0.42.0/crates/rig-agent)

Rig remains pre-1.0 and its README warns of breaking changes. Exact version
pinning and upgrade conformance are mandatory.

The current `main` branch has since separated more responsibilities, including
a sans-I/O `rig-run`, a transport crate, and an MCP adapter. Those changes are
useful evidence of direction, but they are not evidence about 0.42.0 and cannot
be used to approve the rejected dependency. They may justify reevaluating a
later immutable release.

Sources for direction only:

- [current rig-run design](https://github.com/0xPlaygrounds/rig/blob/main/crates/rig-run/src/lib.rs)
- [current rig-reqwest manifest](https://github.com/0xPlaygrounds/rig/blob/main/crates/rig-reqwest/Cargo.toml)
- [current rig-rmcp adapter](https://github.com/0xPlaygrounds/rig/blob/main/crates/rig-rmcp/src/lib.rs)

## What can be reused

### Completion and streaming conversion

`rig_core::completion::CompletionModel` is a useful implementation seam for
unary completion, streaming, and capability reporting. Rig also contains
substantial provider-specific conversion, streamed-frame assembly, terminal
usage/finish metadata, and tool-call identity handling.

A private Kvist adapter can:

1. translate a closed `supervised-agent` model request into a Rig request;
2. invoke a selected Rig provider through an approved endpoint;
3. translate text, structured tool calls, usage, finish reason, model identity,
   provider request identity, and terminal errors into standalone runtime
   events;
4. discard or quarantine non-approved provider material;
5. expose no Rig type to Kvist or other callers.

Sources:

- [completion request and model traits](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/crates/rig-core/src/completion/request.rs)
- [stream normalization](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/crates/rig-core/src/streaming)
- [tool-call bridge](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/crates/rig-core/src/providers/internal/tool_call_bridge.rs)

### Ollama

Rig includes an Ollama provider with endpoint configuration, native chat
conversion, NDJSON streaming assembly, and provider-error handling. This is a
credible local transport candidate.

Kvist must still own the approved endpoint and credential reference. The Rig
adapter performs HTTP; it does not decide whether network access is authorized.

Source: [v0.42.0 Ollama provider](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/crates/rig-core/src/providers/ollama.rs).

### llama.cpp

Rig's llama.cpp provider targets llama-server's OpenAI-compatible surface. It
contains useful explicit handling for local differences, including model-field
behavior and restricted tool-choice forms. This aligns with Kvist's
capability-negotiated approach.

Source: [v0.42.0 llama.cpp provider](https://github.com/0xPlaygrounds/rig/tree/v0.42.0/crates/rig-core/src/providers/llamacpp).

Support is still deployment-specific. Kvist must test the exact llama-server
version, GGUF model, Jinja/chat template, tool-call encoding, structured output,
stream termination, and cancellation.

## Where Rig does not fit

### Request and message types are too broad

Rig requests and messages include provider extensions, raw provider material,
tool definitions, hosted-provider behavior, and telemetry choices. That is
useful for faithful adapters but broader than the standalone canonical
contract.

Arbitrary provider JSON cannot cross the adapter. A provider extension must be
typed, namespaced, size-bounded, allowlisted for the exact adapter, and subject
to host approval and redaction.

### rig-agent crosses authority boundaries

`rig-agent` combines completion construction, provider invocation, memory,
hooks, and tool execution. Kvist deliberately separates those responsibilities
so an authorization record exists before an execution request.

Rig hooks are useful extension points, but an optional callback that can rewrite
or skip a tool call is not a durable policy decision. Tool concurrency also
creates ordering and side-effect questions that Kvist's first native loop avoids
by supporting sequential calls only.

Sources:

- [v0.42.0 agent runner](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/crates/rig-agent/src/agent/runner.rs)
- [v0.42.0 hooks](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/crates/rig-agent/src/agent/hook.rs)

Decision: do not use `rig-agent` as the standalone runtime.

### Rig tools are not the Kvist broker

Rig tool types provide model-facing schemas and implementations, but Kvist also
requires stable tool identity/version, effect class, canonical resource scope,
authorization evidence, execution-backend selection, idempotency, uncertainty,
and artifact evidence.

The transport prototype may use Rig's tool-definition wire conversion and
tool-call decoding. It must not register or execute a Rig tool. A decoded call
becomes an untrusted standalone `ToolIntent`.

### Persistence is not canonical evidence

No Rig transcript, cache, provider payload, or serialized run state becomes
Kvist's durable workflow record. Current-main `rig-run` explicitly warns that
its serialized state can contain raw conversation/provider data and is not
cross-version stable. Even if a later pinned release is evaluated, that state
can only be a version-pinned, redacted, non-authoritative attachment.

## Logging and telemetry risk

The pinned provider path can serialize complete request or response values at
TRACE under Rig tracing targets. Preventing raw payloads from entering the
canonical journal is insufficient if prompts, tool results, or credentials can
reach logs.

Source: [v0.42.0 provider trace helper](https://github.com/0xPlaygrounds/rig/blob/v0.42.0/crates/rig-core/src/providers/internal/mod.rs).

The prototype must:

- install a restrictive tracing policy for Rig targets;
- disable content telemetry by default;
- run sentinel prompts, tool arguments/results, endpoint credentials, and
  provider errors with TRACE globally enabled;
- prove sentinels do not appear in logs, errors, events, or attachments;
- keep diagnostics bounded and redacted;
- reject adoption if safe filtering cannot be enforced from the adapter.

## Measured dependency, TLS, and MSRV result

The exact prototype dependency is:

```toml
rig-core = { version = "=0.42.0", default-features = false }
```

The throwaway 2026-08-30 experiment measured:

| Gate | Result |
| --- | --- |
| Rust 1.85 | **Fail.** `rig-core` itself produced `E0658` errors for unstable let-expression chains. |
| Current Rust 1.98 | Pass for a linked OpenAI completion path. |
| Locked package count | Warning. 136 packages versus 8 for a bare Tokio baseline: delta 128. This is not the future marginal comparison against the direct adapter. |
| Release binary size | Pass. 3,993,408 bytes versus 616,808: delta 3,376,600 bytes, limit 15 MiB. |
| TLS with defaults disabled | As intended for local-only scope: no Rustls, native-tls, or OpenSSL selected. |
| Advisories | Pass: `cargo deny check advisories`. |
| Licenses | Pass for transitive dependencies under the documented permissive allowlist. |

The size measurement deliberately links a real OpenAI completion call so Cargo
cannot discard the transport path. The baseline includes the same Tokio
current-thread runtime. These are decision measurements, not production
benchmarks.

The MSRV failure is sufficient to reject 0.42.0; raising Kvist's MSRV was
considered and rejected because the project contract requires Rust 1.85. No
live model or tool-intent conformance was attempted because it cannot reverse
that decision.

## Prototype scope

For a future immutable release, create a non-default internal adapter.
Production behavior and public APIs do not depend on it until the final
decision is approved.

Support:

- text system, user, assistant, and tool-result messages;
- unary and streamed text;
- Kvist tool descriptor to provider tool-schema translation;
- provider tool-call to canonical `ToolIntent` translation;
- provider/model/request identity, usage, and finish reason;
- explicit cancellation and deadlines;
- local Ollama and llama-server;
- one deterministic fake OpenAI-compatible server.

Reject:

- Rig tool execution;
- `rig-agent`;
- provider-hosted tools;
- arbitrary additional parameters;
- image, audio, document, vector, and memory features;
- Rig-owned persistence;
- raw provider payloads in default events or evidence.

## Conformance tests

### Text and streaming

- unary success;
- split SSE and NDJSON frames;
- ordering and terminal assembly;
- non-success, malformed, truncated, and missing-terminal responses;
- cancellation and deadline races;
- normalized provider/model/request identity, usage, and finish reason.

### Structured tool intent

- standalone tool descriptor to each provider's schema;
- one valid call and streamed argument assembly;
- stable call identity across chunks;
- malformed and invalid argument JSON;
- duplicate IDs and duplicate terminal calls;
- multiple calls and ordering, while execution remains sequential;
- unsupported named/parallel tool choice;
- no silent capability downgrade;
- tool results translated into the next model request;
- exact server/model/chat-template matrix for live tests.

Unavailable live services or models are recorded as `unevaluated`, never
`passed`.

### Authority and redaction

- endpoint and credential references come only from approved test profiles;
- no API accepts untyped provider JSON;
- no Rig tool implementation can execute;
- no provider-hosted tool is enabled;
- no Rig type escapes the private adapter;
- raw requests, responses, credentials, and complete transcripts are absent
  from default runtime events and Kvist evidence;
- TRACE sentinel tests cover logs and formatted errors.

### Build and supply chain

- locked Linux build and tests;
- Rust 1.85 build;
- complete feature and dependency inventory;
- no facade, `rig-agent`, derive, vector, memory, or provider companion crate;
- local-only build has no selected TLS backend and makes no HTTPS claim;
- optional Rustls experiment is measured separately;
- release binary delta is at most 15 MiB and the target-specific normal/build
  package delta is at most 75 unless a documented human exception is approved;
- every transitive license is one of MIT, Apache-2.0, BSD-2-Clause,
  BSD-3-Clause, ISC, Unicode-3.0, Zlib, or CDLA-Permissive-2.0; every other,
  unknown, or unlicensed dependency requires explicit legal approval;
- `cargo deny check advisories` has no unwaived advisory. A waiver records advisory ID,
  reachability, mitigation, owner, expiry, and review evidence.

Use the same Rust toolchain, `x86_64-unknown-linux-gnu` target, release profile,
and feature set for candidate and baseline. Count target-specific normal/build
packages with:

```sh
cargo tree --locked --target x86_64-unknown-linux-gnu \
  -p supervised-agent -e normal,build --prefix none |
  sed 's/ (\*)$//' | sort -u | wc -l
```

Measure the unstripped release executable from a fresh target directory. The
first direct adapter is compared with the standalone crate before transport
dependencies; a future Rig adapter is compared with the reviewed direct
adapter. The decision matrix records commands, lockfile digests, tool versions,
features, counts, sizes, license result, advisory result, and exceptions.

## Go/no-go gates

Adopt a future release only if:

1. the exact crate and resolved dependencies build and test on Rust 1.85;
2. the private adapter exposes only standalone-owned canonical types;
3. endpoint, credentials, capability decisions, cancellation, deadlines, and
   redaction remain host-controlled;
4. fake, Ollama, and llama-server text/stream conformance passes;
5. transport-only structured tool-intent conformance passes for the exact
   deployment matrix;
6. malformed and truncated streams fail closed;
7. TRACE sentinel tests demonstrate no content leakage;
8. dependency, size, license, advisory, and local-HTTP/TLS decisions satisfy the
   objective gates;
9. an exact-version upgrade cannot bypass the conformance suite.

The 0.42.0 experiment failed gate 1. Its dependency was not added to Kvist. The
approved next path is a small direct local HTTP adapter, preserving the same
conformance suite and private seam. A no-go result is a successful experiment
because its evidence prevents an unsuitable dependency from becoming
structural.

## Direction assessment

Rig's philosophy remains broadly compatible at the transport boundary: its host is
expected to own policy and operational controls. That is exactly why it cannot
replace Kvist's broker, execution, or evidence layers.

The current-main movement toward smaller crates and a sans-I/O loop is
directionally useful, but fast pre-1.0 restructuring is also a maintenance
warning. Kvist should reevaluate immutable releases rather than track `main`,
prefer upstream patches for narrow general defects, and avoid a fork until an
essential accepted adapter proves impossible upstream.

The immediate durable target is:

```text
supervised-agent canonical model contract
    -> private direct local HTTP adapter
        -> approved local provider

Kvist host policy
    -> typed broker
        -> Linux execution backend
            -> durable Kvist evidence
```

A future Rig adapter may replace only the direct transport box after all gates
pass.
