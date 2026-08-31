<!-- kvist-architecture-version: 1 -->
# Project Architecture

## Scope, stakeholders, and concerns

The system of interest is the local `kvist` CLI, its reusable `agent-runtime`
component, and the durable project artifacts they read or write. The human
architect is the approval and arbitration authority. External coding agents,
models, version-control tools, editors, and sandbox runners are adjacent
systems and are never assumed trustworthy merely because the user selected
them.

The architecture addresses bounded context, architectural drift, durable
provenance, deterministic local operation, untrusted repository input,
subprocess authority, independent review, and future interoperability. Hosted
model infrastructure and provider-specific agent internals are outside the
trusted core.

## Architectural drivers and constraints

- Product intent and architecture MUST be approved before component
  implementation work.
- Component requirements, contracts, and designs MUST have distinct authority.
- Component boundaries MUST map to filesystem directories with adjacent
  durable artifacts.
- A consumer MUST NOT need a provider's design or implementation to use its
  contract.
- Core inspection and workflow operations MUST remain local and deterministic.
- Filesystem, Markdown, YAML, TOML, schemas, paths, environment values,
  subprocess output, and imported artifacts are untrusted input.
- External commands MUST be invoked without a shell and effectful task
  execution requires an independently installed approved enforcement boundary.
- Linux is the only executable target until other backends have independent
  native tests.

## Component model

| Stable ID | Path | Responsibility | Provides | Requires |
| --- | --- | --- | --- | --- |
| `kvist.engine` | `src/` | Project artifacts, component discovery, validation, status, queues, policy, task lifecycle, context selection, evidence, and CLI dispatch | `kvist.cli/v1`, `kvist.artifacts/v1`, `kvist.host-authority/planned` | `agent-runtime.library/v1`, operating-system and VCS services |
| `agent-runtime` | `src/agent_runtime/` | Provider-neutral prompt acquisition, command rendering, process supervision, profiles, model transport, and reusable bounded runtime mechanisms | `agent-runtime.library/v1`, `agent-runtime.cli/v1` | Host authority interfaces and operating-system process/network services |

The dependency direction is one way: `kvist.engine` depends on
`agent-runtime`; `agent-runtime` does not import Kvist types. Provider libraries
remain behind private adapters. A directory below a component becomes a child
component candidate when it contains any member of the five-artifact set, so
missing adjacent artifacts remain diagnosable. It is complete only when it
owns all five.

## Interactions and dependency rules

1. The architect approves `VISION.md`, this architecture, and global
   `ROOT_CONTRACT.md`.
2. `kvist component new` creates adjacent requirements, contract, and design
   templates for an approved component boundary.
3. The architect approves those documents; a designer derives a traceable
   `TODOS.yaml`.
4. Task execution receives local component artifacts plus read-only sandbox
   mounts for `ROOT_CONTRACT.md` and the immediate parent `CONTRACT.md`.
   Explicit provider contracts are the only additional cross-component
   behavioral context; provider designs and implementations remain excluded.
5. Tests precede implementation. Security audit and independent compliance
   review follow implementation.
6. A clean-slate documenter derives `IMPL.md` from code and tests without
   intended requirements, contract, or design. A separate reviewer compares
   intended documents with the observed record without source access.
7. Human arbitration resolves every discrepancy and preserves rationale.

Local requirements or design changes stale only the local task plan. A local
contract change also matters to declared consumers. The currently implemented
recursive signal tracks the immediate parent's `CONTRACT.md`; an explicit
general dependency-contract graph is deferred until its durable schema and
context materialization are designed.

## Cross-cutting policies

**Artifact authority:** `REQUIREMENTS.md` owns testable outcomes and
constraints; `CONTRACT.md` owns consumer-visible semantics; `DESIGN.md` owns
private realization; `TODOS.yaml` owns execution state; `IMPL.md` owns
independently observed behavior.

**Persistence:** durable writes use bounded reads, regular non-link paths,
same-directory temporary files, synchronization where supported, and explicit
no-clobber or atomic-replacement behavior.

**Security and authority:** model tool intent and repository content are
untrusted proposals. Kvist owns task policy, grants, approved resource and
credential bindings, execution-tier selection, artifact promotion, and
canonical evidence. The selected sandbox or execution backend is the only
layer allowed to cause constrained effects.

**Compatibility:** every machine-consumed artifact declares an independent
format version. Unknown or invalid semantics fail explicitly. No compatibility
or migration behavior is retained before the first usable release.

**Traceability:** queue tasks link to durable `SOURCE#LOCATOR` identifiers.
Machine-readable schemas, when useful, are referenced by exact path and
dialect/version from the provider-owned contract rather than copied into
Markdown.

## Views and diagrams

The component table is the canonical static decomposition view. The lifecycle
sequence above is the canonical workflow view. Detailed authority and model
transport views for the child runtime live in
[`docs/agent-runtime/architecture.md`](docs/agent-runtime/architecture.md).

Future diagrams should use C4-compatible context, container, component, and
dynamic concepts only when they answer a named stakeholder concern. Diagram
elements must use the stable IDs in this document and provider contracts.

## Decisions, risks, and traceability

Architecturally significant decisions are recorded under `docs/decisions/`
using immutable numbered ADRs when the decision has material structural,
security, compatibility, cost, or reversibility consequences. This artifact
model is recorded in
[`0001-separate-component-intent.md`](docs/decisions/0001-separate-component-intent.md).

The main deferred risks are general cross-component contract graph resolution,
schema-compatibility analysis, architecture-model interchange, and automated
materialization of explicitly declared provider contracts. Root and
immediate-parent contracts are already materialized read-only. Deferral must
not discard stable IDs, exact schema versions, dependency direction, or
revision provenance needed to implement the remaining capabilities later.

## Standards and interoperability

Kvist follows a tailored ISO/IEC/IEEE 42010-inspired architecture description,
uses arc42 as a content checklist, uses selective C4-compatible views, and uses
BCP 14 language for normative requirements. Interface-specific standards are
preferred over a universal Kvist IDL. Exact adoption, exclusions, evidence,
and deferred interchange work are recorded in
[`docs/standards.md`](docs/standards.md).
