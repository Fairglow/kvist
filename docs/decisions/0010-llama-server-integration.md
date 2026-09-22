# ADR 0010: First-class llama-server integration via the direct transport

## Status

Accepted. The direct model transport is the first-class integration path for
local llama-server (and Ollama) endpoints; the model server is an external,
externally-supervised dependency, and the agent runtime does not launch or
manage it. This document records the decision and its rationale; it is not a
compliance certification.

## Context

agent-runner and the supervised task path both drive local models over loopback.
llama-server (llama.cpp, OpenAI-compatible) is the preferred local model server,
and the user named llama-server integration as the main focus: they asked whether
agent-runner should start and supervise llama-server ourselves so it could
collect the server's own progress output.

The two candidate approaches:

1. Require an external llama-server and connect to it over loopback with a
   purpose-built transport.
2. Launch and supervise llama-server ourselves as a managed child process, so we
   own its lifecycle and could scrape its progress/logging.

## Decision

Adopt requirement 1: an externally supplied, externally supervised llama-server
connected to by the native DirectModelTransport, and do not launch or manage the
server. Rationale:

- Headless, no-daemon default. Kvist's core must run without a background daemon
  or managed long-lived process. A supervised server is a daemon-like lifecycle;
  keeping the server out of the runtime preserves the headless, single-command,
  no-side-effect contract of the core. See ADR 0005 for the network/authority
  model and ADR 0009 for the transport seam.
- The transport is already first-class. The native DirectModelTransport already
  implements a complete, bounded, fail-closed llama-server integration:
  OpenAI-compatible POST /v1/chat/completions unary and streaming, reasoning
  extraction, fragmented tool-argument assembly, slot-allocation / time-to-first
  token / inter-token cadence watchdogs, and bounded, cancellation-aware I/O.
- Progress is already observable. The transport surfaces working speed, token
  accounting, and context utilization; the durable session log preserves the
  full reasoning trace. Scraping the server's own log lines is unnecessary and
  would couple us to a specific server's log format.
- Lifecycle and credentials stay out of the sandbox. A managed server adds
  process supervision, port selection, shutdown, and credential handling inside
  the authority boundary. Keeping the server external keeps those concerns with
  the operator, not the runtime.

The transport remains provider-aware on top of a common core: llama-server and
Ollama share the bounded streaming/watchdog engine but keep their native request
encoding, slots, reasoning, and tool schemas. This honors the goal of full
per-provider integration without a shared abstraction that would omit each
server's specifics.

### Deferred: optional supervised launch

A future opt-in to launch and supervise llama-server (for example behind an
explicit configuration switch that is off by default) is explicitly deferred to
a later decision, gated on a measured review of the daemon/credential/authority
implications. It is not implemented here.

## Rationale for rejecting supervised launch by default

- No daemon in core. Consistent with the headless, single-command contract.
- Less coupling. Not scraping a specific server's log format keeps the transport
  portable across server revisions.
- Authority. A managed process with port selection and optional credentials
  expands the risk surface inside the boundary we deliberately keep minimal.

## Alternatives considered

- Launch and supervise llama-server ourselves. Provides lifecycle control and
  log scraping, but introduces a daemon, port/credential handling, and
  server-format coupling inside the core. Rejected in favor of the external
  server, with optional supervised launch deferred behind a future gated
  decision.
- Adopt a shared model client crate. Rejected earlier (ADR 0009) in favor of the
  native, directly owned transport to keep the dependency surface minimal.

## Consequences

- llama-server integration is complete and reliable today without any managed
  process; operators run their own server on loopback.
- The transport is the sole local model integration, kept provider-aware on a
  common bounded engine.
- Optional supervised launch is a deferred, gated future decision, not a promise.
  The stats bar reports working speed, context utilization, and a compaction ETA;
  the full reasoning trace is preserved in the durable session log.
