# Kvist documentation

This directory holds the design, decision, standards, and deep-dive material
that the root [`README.md`](../README.md) and the per-component READMEs link
into. The root README gives the product overview and the component map; the
component READMEs describe each component's purpose and how to build and run
it, with the `kvist` CLI contract, sandboxed-execution, and toolchain model
documented in [`maerg/README.md`](../maerg/README.md). This directory goes
deeper: the
authoritative command design, the architectural decisions, the standards
posture, and the agent runtime analysis.

## Guides

| Document | What it covers |
| -------- | -------------- |
| [Command-Set Design](command-set.md) | The authoritative design for the `kvist` command-line surface: the four core concepts, the lifecycle, and the rules that keep the interface simple, predictable, and self-explanatory. |
| [Component Naming](component-naming.md) | Rationale and source of truth for the component names; every component is named after a plant part that mirrors its role in the system. |
| [Standards and Interoperability Profile](standards.md) | Which external standards and practices Kvist follows, uses as inspiration, supports through native artifacts, or has considered and set aside. |
| [Why Kvist](why-kvist.md) | A grounded comparison of Kvist with coding agents, specification kits, agent frameworks, sandboxes, and governance tools. |
| [Logging and Observability Guide](logging.md) | How Kvist logs and why: the two audiences (human operators and the runtime), levels, and what is recorded. |
| [Sandbox Toolchains and Vendoring](sandbox-toolchains-and-vendoring.md) | How toolchains and their dependencies are mounted into the sandbox, how they are discovered and used, and how to add or change vendored dependencies per toolchain. |

## Agent runtime

The analysis and design for the reusable agent runtime that `sav` provides and
`skott` and `maerg` consume.

| Document | What it covers |
| -------- | -------------- |
| [Architecture](architecture.md) | The layered agent architecture organized by authority: which layer owns what, and why the stable abstraction is a set of backend roles rather than "provider." |
| [Runtime Selection](runtime-selection.md) | Selection guidance for native, Rig-run, containerized Rig-agent, and external agent drivers. |
| [Runner Hardening](runner-hardening.md) | The security-first workspace agent: implemented improvements, native-helper installation, headless llama-server usage, authority limits, and the independent qualification record. |
| [User Guide](user-guide.md) | Standalone agent runtime user guide: building, configuring, and running `sav-run`. |

## Decisions (`decisions/`)

Immutable, numbered Architecture Decision Records (ADRs). Each records a
decision with material structural, security, compatibility, cost, or
reversibility consequences.

| ADR | Decision |
| --- | -------- |
| [0001](decisions/0001-separate-component-intent.md) | Separate component requirements, contract, and design |
| [0002](decisions/0002-advisory-document-review.md) | Advisory document review before acceptance |
| [0003](decisions/0003-align-rust-workspace-with-components.md) | Align the Rust workspace with component ownership |
| [0004](decisions/0004-bubblewrap-runner-and-sandbox-v1.md) | Bubblewrap runner and redefined sandbox protocol version 1 |
| [0005](decisions/0005-mediated-dependency-and-model-networking.md) | Mediate dependency and model networking |
| [0006](decisions/0006-supervised-attempts-and-promotion.md) | Supervised attempts, recovery, and promotion |
| [0007](decisions/0007-interactive-shell-and-library-relegation.md) | Interactive shell and engine library relegation |
| [0008](decisions/0008-top-level-workspace-peer-components.md) | Top-level peer components for workspace dogfooding |
| [0009](decisions/0009-brokered-model-transport-and-isolated-effects.md) | Brokered model transport with isolated effects |
| [0010](decisions/0010-llama-server-integration.md) | First-class llama-server integration via the direct transport |
| [0011](decisions/0011-vendored-offline-builds.md) | Vendored offline builds for sandbox verification |
| [0012](decisions/0012-pinned-rust-toolchains.md) | Pinned Rust toolchains provisioned on the host |
| [0013](decisions/0013-per-language-offline-verification.md) | Per-language offline verification topology |
| [0014](decisions/0014-readonly-rust-authoring-build-context.md) | Read-only Rust authoring build context |

## Proposals (`proposals/`)

Proposals for human approval. These are not accepted architecture decisions;
they describe intended work and its rationale.

| Proposal | What it covers |
| -------- | -------------- |
| [First-class Markdown transcripts](proposals/agent-runner-markdown-transcripts.md) | Markdown for message content, versioned YAML for session structure, and one shared semantic transcript viewer for live and historical output. |
| [Prompt Steering, History, and Queueing Plan](proposals/prompt-steering-and-history-plan.md) | Steering an active task, session history, and prompt queueing. |
