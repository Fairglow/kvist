# KVIST: Architectural Specification & Strategy Document

**Subtitle:** Structured design for autonomous agents.  
**Project Name:** KVIST (`kvist`)  
**Target Engine Implementation:** Rust  
**License:** Business Source License (BSL 1.1) / Dual-licensed for non-commercial open-use  
**Version:** 0.1.0

**Status:** This is the authoritative detailed product direction, not a claim
that every described workflow is automated. [`VISION.md`](VISION.md) owns the
product direction, [`ARCHITECTURE.md`](ARCHITECTURE.md) owns the approved
system decomposition, [`TODO.md`](TODO.md) tracks delivery, and
[`docs/standards.md`](docs/standards.md) records the standards and
interoperability posture.

---

## 1. Executive Summary and Principles

AI-driven development often gains initial speed by giving an agent a broad
goal and a repository. That approach tends to produce architectural drift,
unbounded context, hidden rationale, incomplete verification, and weak human
control. Kvist instead makes the human the architecture and arbitration
authority and gives agents bounded, durable work.

Kvist's governing hierarchy is:

```text
VISION.md
  -> ARCHITECTURE.md
    -> component REQUIREMENTS.md + CONTRACT.md + DESIGN.md
      -> TODOS.yaml
        -> source and tests
          -> independently derived IMPL.md
```

The artifacts have deliberately separate authority:

- `VISION.md` states product direction.
- `ARCHITECTURE.md` states system structure, component boundaries, dependency
  direction, cross-cutting policies, and significant decisions.
- `REQUIREMENTS.md` states component outcomes, constraints, acceptance
  criteria, and verification obligations.
- `CONTRACT.md` states everything a consumer may rely on: provided and
  required interfaces, observable behavior, data formats, errors, security,
  and compatibility posture. Optional native schemas are referenced from this
  file with their exact path and dialect/version.
- `DESIGN.md` states private realization: internal structure, algorithms,
  state transitions, design decisions, failure recovery, and internal security
  mechanisms.
- `TODOS.yaml` is the versioned, traceable execution queue.
- `IMPL.md` is an independently observed implementation record, not intended
  behavior and not user-facing documentation.

No retired single-document component format is recognized. This pre-release
model has no backward-compatibility or migration path and introduces no
project version bump.

### Core tenets

- **Structure before syntax:** approve architecture and component intent before
  implementation.
- **Recursive filesystem components:** each component directory owns its
  requirements, contract, design, queue, observed record, tests, and source.
- **Durable native state:** architecture and workflow state remain inspectable
  in version-controlled Markdown and YAML.
- **Strict context boundaries:** the immediate parent `CONTRACT.md` is the only
  implicit propagated component context. Peer artifacts and parent
  requirements, design, tests, and implementation are excluded. General
  explicitly declared provider-contract materialization remains deferred.
- **Tests before implementation:** every deliverable chain orders test,
  implementation, security audit, then independent compliance review.
- **Advisory review before acceptance:** controlled intent gets a bounded AI
  review opportunity when reasonable, or a visible explicit exception.
  Findings are nonbinding and never determine compliance.
- **Independent verification:** an implementer cannot certify its own work.
- **Headless local core:** core inspection and workflow commands need no cloud
  service, telemetry, credentials, or runtime daemon.
- **Linux-first execution:** executable workflows remain Linux-only until
  another backend has independent native tests and review.

---

## 2. System Architecture and On-Disk Layout

Kvist maps approved component boundaries to directories. A directory becomes a
component only when it owns the complete adjacent artifact model; ordinary
source directories do not become components merely because they exist.

```text
repository-root/
├── VISION.md
├── ARCHITECTURE.md
├── ROOT_CONTRACT.md
├── kvist.toml
└── src/
    ├── REQUIREMENTS.md
    ├── CONTRACT.md
    ├── DESIGN.md
    ├── TODOS.yaml
    ├── IMPL.md
    ├── lib.rs
    └── network/
        ├── REQUIREMENTS.md
        ├── CONTRACT.md
        ├── DESIGN.md
        ├── TODOS.yaml
        ├── IMPL.md
        ├── mod.rs
        └── protocol/
            ├── REQUIREMENTS.md
            ├── CONTRACT.md
            ├── DESIGN.md
            ├── TODOS.yaml
            ├── IMPL.md
            └── frame.rs
```

### Context and dependency rules

Work on a component may use its local intent documents and queue,
`ROOT_CONTRACT.md`, and the immediate parent `CONTRACT.md`. The parent is the
nearest ancestor component and may be separated from the child by transparent
namespace directories. A consumer must not need a provider's `DESIGN.md`,
`IMPL.md`, tests, or source to use the provider.

Only an immediate parent contract revision propagates implicitly. Parent
requirements and design changes do not stale a child by themselves. A local
requirements or design change stales the local queue; a local contract change
also matters to declared consumers. A general dependency-contract graph is
deferred until its durable representation and context-materialization rules
are designed. In particular, general explicitly declared provider-contract
materialization is not part of current task execution.

The root engine and reusable `agent-runtime` component have the stable
decomposition recorded in [`ARCHITECTURE.md`](ARCHITECTURE.md). The dependency
direction remains one way: Kvist consumes the runtime contract, and the runtime
does not import Kvist types.

---

## 3. Recursive Lifecycle

### Stage 1: Product and architecture approval

The human architect approves `VISION.md`, `ARCHITECTURE.md`, and
`ROOT_CONTRACT.md`. Architecturally significant decisions use immutable
numbered ADRs. The decision to separate component intent is recorded in
[`docs/decisions/0001-separate-component-intent.md`](docs/decisions/0001-separate-component-intent.md).
The decision to require a nonbinding review opportunity while separating its
receipt from compliance is recorded in
[`docs/decisions/0002-advisory-document-review.md`](docs/decisions/0002-advisory-document-review.md).

Review of project-level vision, architecture, root contract, ADRs, and
referenced native schemas is target behavior that requires a later
project-level acceptance surface. Current component commands do not enforce it.

### Stage 2: Component intent

For an approved component boundary:

```bash
kvist component new COMPONENT_DIR
kvist component validate COMPONENT_DIR
kvist component accept COMPONENT_DIR
```

`component new` creates deterministic no-clobber templates for
`REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`. `component validate`
validates all three without rewriting them. Current `component accept`
structurally validates the local intent set and immediate parent contract, then
records exact revisions in the queue. It does not enforce AI review and must
not be described as doing so.

The architect may draft these files manually or with agent assistance, but
unresolved product decisions remain explicit. Consumer-facing semantics never
belong only in `DESIGN.md`; internal algorithms never become consumer promises
merely because they are described in `CONTRACT.md`.

The target acceptance gate covers exact digests of the three local intent
documents plus a canonical projection of task definitions in `TODOS.yaml`.
The projection contains `id`, `title`, `description`, `context`, `purpose`,
`expected_outcome`, `kind`, `depends_on`, and `requirements`; it excludes all
`component` metadata and task `status`, `timestamps`, `blocked_reason`, and
`recovery_state`. Generated intent drafts are not exempt. Generated evidence,
including `IMPL.md`, compliance and review reports, status, and attempt logs,
is exempt. The gate does not govern every arbitrary Markdown file.

A separate planned review operation uses the existing shell-free, bounded,
approved or acknowledged agent path. Its context is exactly the local intent
set and canonical task projection, `ROOT_CONTRACT.md`, and the immediate parent
`CONTRACT.md`; it excludes peer and parent internals. Separate authoring and
reviewing contexts are preferred, but provenance is not proof of independence
or review quality.

Kvist writes versioned receipts and redacted bounded reports under the
component's `.kvist/reviews/`. Receipts bind exact target digests and scope,
reviewing and authoring context identity when known,
provider/profile/model/tool identity, Kvist version, timestamp, report digest
and path, and acknowledgement or exception. Model output cannot mint a
receipt; raw transcripts and secrets are not retained. Review files are
VCS-trackable but are outside the five-artifact set, do not trigger component
candidacy or staleness, are not task context, and are not compliance evidence.

When project review is required, target acceptance requires a current receipt
plus explicit human acknowledgement or an explicit per-bundle exception
containing actor, timestamp, reason, and exact digests. A visible project
`[review] required = false` opt-out disables the per-bundle gate. With no
configured agent, a review-required project needs an explicit exception rather
than an implicit pass. Findings may be wrong or overly exacting; no finding,
score, or severity blocks acceptance or determines compliance. `component
accept` remains deterministic and local and never spawns an agent or makes a
network call. Review defaults to required when its configuration is absent, and
generated projects state `[review] required = true` explicitly.

### Stage 3: Traceable task planning

A designer derives `TODOS.yaml` from the accepted requirements, contract, and
design. Queue provenance records:

```yaml
component:
  requirements_revision: sha256:...
  contract_revision: sha256:...
  design_revision: sha256:...
  parent_contract: null
```

For a child, `parent_contract.path` is computed to the actual nearest ancestor
component across transparent namespace directories. It contains one or more
`..` segments followed by `CONTRACT.md`; for example, `../CONTRACT.md` or
`../../../CONTRACT.md`. The mapping also records the reviewed revision. Tasks
carry durable requirement locators and explicit dependency edges. Each
deliverable chain orders:

1. `write_tests`
2. `implement_code`
3. `security_audit`
4. `compliance_review`

### Stage 4: Tests and implementation

Tests are written from the approved intent before production code. The
implementer receives only the bounded context authorized for that component
and cannot read peer implementation details merely for convenience. Native
language documentation describes code-level use; it does not replace the
consumer contract or observed implementation record.

The contract-verification target adds stable clause locators, initially the
existing heading anchors. Explicit clause IDs require an explicit
format/version decision. Tests map to clauses, and approved execution evidence
feeds a traceability report that identifies uncovered and failed clauses. Tests
are evidence, not proof; this report is not code coverage and does not replace
compliance comparison of `CONTRACT.md`, `IMPL.md`, and test evidence.

### Stage 5: Clean-slate record and independent compliance

The clean-slate documenter derives a fresh `IMPL.md` from source, tests,
manifests, and necessary non-intent build configuration. It must not read
`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, a prior
`IMPL.md`, root or architecture intent, prior reviews, chat history, or Git
history.

A separate source-blind compliance reviewer then compares the approved
`REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md` with the fresh `IMPL.md` and
test evidence. The reviewer does not read source and is not the implementer or
documenter. Every item is recorded as compliant, mismatched, approved-deferred,
or underspecified. No implementation context may self-certify.

An independently generated `IMPL.md` may also seed a planned `propose intent`
or `derive draft` workflow. That workflow writes no-clobber draft
`REQUIREMENTS.md` and `DESIGN.md` only, marks uncertainty, and does not claim to
recover stakeholder intent or create a normative `CONTRACT.md`. Generated
drafts still need human review, advisory AI review or exception,
acknowledgement, and acceptance.

A separate advisory comparison may report differences between `IMPL.md` and
existing intent without changing either. It does not use compliance verdict
vocabulary and is not compliance evidence. This workflow remains distinct from
source-based `reverse-discover`; a contract generated by reverse discovery is
a non-normative draft.

### Stage 6: Human arbitration

The architect resolves discrepancies by requesting implementation work,
approving an explicit intent change, or recording a reasoned exception. The
original discrepancy and decision rationale remain durable. Automation must
not silently edit intent documents or `IMPL.md` to manufacture agreement.

---

## 4. CLI, Status, and Durable State

The current component document surface is
`kvist component new|validate|accept`. Status supports
`--only-documents` for document-focused output. Retired command spellings and
queue fields are not aliases.

Advisory review, observed-intent proposal/comparison, project-level acceptance,
and contract-clause traceability commands are planned. Their names and syntax
are provisional and are not part of the current interface.

Status compares exact UTF-8 byte revisions for local requirements, contract,
and design and, for children, the immediate parent contract. It reports
attributable missing, invalid, unsupported, stale, blocked, or current state
without persisting derived staleness.

Kvist writes durable state through regular non-link paths, bounded reads,
same-directory temporary files, synchronization where supported, and explicit
no-clobber or atomic replacement. Machine-consumed formats carry independent
version markers, but the pre-release artifact split retains no compatibility
or migration behavior.

Planned `.kvist/reviews/` receipts and reports are additional versioned
workflow evidence, not component artifacts. Only absence of required current
review evidence, acknowledgement, or exception may block target acceptance;
the report's content cannot.

---

## 5. Agent Runtime and Execution Authority

Kvist owns task policy, grants, approved resource and credential bindings,
execution-tier selection, artifact promotion, and canonical evidence. The
standalone `agent-runtime` component owns provider-neutral prompt acquisition,
command rendering, process supervision, profiles, model transport, canonical
tool intent, and reusable bounded runtime mechanisms.

Provider libraries remain private adapters. A model tool call is untrusted
intent, not authorization. Opaque coding-agent CLIs are constrained as whole
processes by the selected external execution boundary. Core inspection does
not require a model or network.

Current sandboxed task execution provides one writable mount at
`/workspace/component` and read-only context at
`/workspace/context/ROOT_CONTRACT.md` plus, for a child,
`/workspace/context/PARENT_CONTRACT.md`. The parent file is sourced from the
actual nearest ancestor component across transparent namespace directories.
General provider-contract context materialization remains deferred.

The direct local HTTP transport is the default and fallback. The exactly pinned
optional Rig adapter is a non-default transport prototype. Live provider
matrices, independent security audit, and compliance review remain promotion
gates; optional availability is not a completed interoperability claim.
Detailed authority and transport decisions live in
[`docs/agent-runtime/architecture.md`](docs/agent-runtime/architecture.md) and
[`docs/agent-runtime/rig-evaluation.md`](docs/agent-runtime/rig-evaluation.md).

---

## 6. Standards and Interoperability

Kvist uses a tailored ISO/IEC/IEEE 42010-inspired architecture description,
arc42 as a content checklist, selective C4-compatible views, BCP 14 normative
language, and immutable ADRs. Interface-native schemas such as OpenAPI,
AsyncAPI, JSON Schema, Protocol Buffers, or WIT may be referenced from the
provider's `CONTRACT.md` when useful.

Kvist does not claim completed general import/export interoperability.
ReqIF, architecture-model exchange, Structurizr export, schema validation, and
other adapters remain deferred or import/export-ready only to the extent
documented in [`docs/standards.md`](docs/standards.md). Stable identifiers,
exact versions, direction, and provenance are retained so a future adapter can
declare what it preserves or loses.

---

## 7. Risks and Deferred Work

| Risk | Architectural response |
| --- | --- |
| Upstream ripple | Only the nearest ancestor component `CONTRACT.md` propagates implicitly; general provider-contract materialization remains deferred. |
| Context growth | Local intent, queue, root constraints, and explicitly authorized contracts bound the work context. |
| Hallucinated compliance | Clean-slate observation and separate source-blind comparison prevent implementer self-certification. |
| Review becomes an approval oracle | Findings remain advisory; a review-required acceptance checks only exact-digest review opportunity and acknowledgement or explicit exception, while a visible project opt-out disables the gate. Receipts are not compliance evidence. |
| Artifact ambiguity | Requirements, contract, design, task state, and observed behavior have distinct authority. |
| Unsafe execution | External commands are shell-free and effectful task execution requires an independently installed approved enforcement boundary. |
| Nominal portability | Linux is the only executable target until another backend has independent native evidence. |
| Interchange overclaim | Native schema references and retained identifiers support future adapters without claiming currently deferred conformance or interoperability. |

Advisory review evidence and project-level acceptance, IMPL-derived intent
proposals and comparison, contract-clause traceability, visual editor, LSP,
web, generalized dependency graphs, architecture exchange, and full
compliance-workflow automation remain planned until their contracts, security
boundaries, tests, and independent evidence exist.

---

## 8. Delivery Planning

The implementation roadmap, task contexts, acceptance criteria, and status
live in [`TODO.md`](TODO.md). This document preserves detailed architectural
strategy; `VISION.md`, `ARCHITECTURE.md`, component intent documents,
`TODOS.yaml`, and `IMPL.md` retain their distinct authorities.

---
*KVIST — Structured design for autonomous agents.*
