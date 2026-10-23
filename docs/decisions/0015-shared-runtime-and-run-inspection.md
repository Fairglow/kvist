# ADR 0015: Shared agent runtime and independent run inspection

## Status

Accepted direction, explicitly approved by the human on 2026-10-23.
Implementation proceeds in independently verified commits. This decision is
not an implementation record, acceptance receipt, or compliance certification.

## Context

Sav supplies provider discovery, direct local model transport, command profiles,
and supervision. Skott links that library and supplies an interactive/headless
workspace agent. Maerg separately coordinates protected task authoring. Their
configuration and native loop mechanisms have diverged. Catalog discovery for
Copilot does not make that external coding agent a native model transport.

## Decision

Sav owns the shared provider/profile resolver, adapter capabilities, native
conversation/loop mechanisms, runtime controls and event mechanisms. It never
imports Skott or maerg types. Shared configuration is filesystem-native,
bounded and inspectable; no service or separate repository is required.
Discovered availability is not configuration or authorization. Resolve an
explicit task profile once and retain its identity throughout the attempt.

Skott owns terminal presentation and standalone workspace-agent integration.
Its headless path uses the same runtime. Maerg retains approved context,
roles, task policy, protected tools, acquisition, verification, promotion,
acceptance and canonical evidence. Galla retains enforcement. Sharing loop
mechanisms must not share or silently broaden tool authority.

Native model and external agent are distinct backend kinds. The former returns
untrusted model turns for a host-owned loop. The latter owns its own loop and
is supervised through an explicit execution boundary. Copilot/Gemini discovery
does not qualify either for protected task authoring. Unsupported combinations
fail before effects; there is no unconfined fallback or nested native tool loop.

Use a common, versioned, bounded event journal for live inspection. The run
owner records events as they happen; observers replay and follow without owning
execution or canonical evidence. Observer disconnection never cancels a run.
Private transcripts and maerg evidence remain separate even when they share
recording mechanisms. Required recording failure prevents further effects.

Read-only observation is the default. Cancellation and guidance require an
explicit, private local control capability. Guidance is bounded, ordered,
correlated and recorded before delivery at safe boundaries. It cannot change
approved intent, tool grants or independent-review context. An executing tool
is never interrupted merely to steer; remaining stale calls receive explicit
not-executed results. Cancellation reports partial/unknown effects, not rollback.
Shutdown prevents delivery and automatic replay.

The registry and wizard share discovery, validation, qualification and atomic
persistence. Frontends own presentation only. Qualification discloses process
authority and possible billing, stores no inline credentials, and does not
authorize later execution. Interactive defaults may prefer a loaded model;
protected task bindings remain explicit and reproducible.

## Alternatives

- Merging Skott and Sav into a terminal-dependent executable would couple
  headless execution to presentation without removing authority differences.
- A third provider service/crate adds indirection before there is a distinct
  deployment or ownership need.
- Launching today's workspace Skott for protected tasks would replace narrow
  maerg grants with workspace shell authority and is rejected.
- Modeling external agents as text transports hides their own effects and loop.

## Delivery and verification

Define local requirements, contracts, design and ordered tasks before code.
Unify configuration, record live events and support read-only attachment,
consolidate native mechanics, add the external backend, and wire authorized
controls. Test malformed input, bounds, recording failure, observer loss,
steering races, budget preservation, complete tool-result pairing, shutdown,
and preservation of protected broker behavior. Use deterministic fixtures;
live provider qualification remains an explicit operator action.

Clean-slate source observations and a separate source-blind intent comparison
remain necessary. No generated receipt or acceptance is implied by this ADR.
