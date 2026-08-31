<!-- kvist-vision-version: 1 -->
# Project Vision

## Purpose

Kvist restores engineering discipline and human authority to AI-assisted
software development. Instead of treating an agent conversation as the durable
source of truth, Kvist makes product intent, architecture, component
requirements, consumer contracts, internal designs, execution plans, and
independent implementation evidence explicit version-controlled artifacts.

The human remains the principal architect and final approver. Agents can
propose, design, implement, document, and review work, but they do so through
bounded roles and inspectable state.

## Outcomes and stakeholders

Kvist is intended for developers and architects who need AI assistance without
accepting architectural drift, hidden context, unreviewed product decisions, or
self-certified implementations.

Successful use produces:

- a product vision approved before architecture;
- an explicit component hierarchy with narrow dependency direction;
- testable requirements separated from consumer contracts and internal design;
- an advisory AI review opportunity for controlled intent before
  acceptance, with visible acknowledgement or deliberate exception;
- task queues traceable to stable requirement and contract identifiers;
- bounded agent execution with explicit authority;
- implementation records derived independently from source;
- uncertain no-clobber intent proposals and comparisons derived from observed
  records without claiming to recover stakeholder intent;
- contract clauses traceable to approved test evidence without treating tests
  as proof; and
- discrepancies retained for human arbitration.

The primary stakeholders are the human architect, component designers,
implementers, integrators, security reviewers, compliance reviewers, and
operators of the local CLI.

## Scope and non-goals

Kvist is a headless, filesystem-native Rust CLI. Core project inspection and
workflow state do not require cloud services, telemetry, credentials, or a
runtime daemon. External model and coding-agent programs are optional
subprocess integrations.

The component hierarchy maps intentionally to directories. Every component
owns adjacent `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`,
`IMPL.md`, and implementation files.

Kvist is not intended to:

- autonomously choose product direction or silently resolve ambiguity;
- let an AI review finding, score, or severity decide acceptance or compliance;
- use peer implementations as implicit component context;
- make Markdown a substitute for executable tests or native interface schemas;
- claim that an external agent's internal tool loop is an authorization
  boundary;
- infer compliance from implementation success; or
- provide nominal platform support without native testing.

Linux is the only supported executable platform while the project is developed
and tested by a Linux-only team. Other platforms remain planned behind explicit
replaceable boundaries.

The initial enforceable advisory-review target is the local component intent
bundle—`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, and a canonical projection
of task definitions from `TODOS.yaml`. Generated intent drafts are included.
It does not govern every Markdown file.
Project-level vision, architecture, root-contract, ADR, and referenced
native-schema review needs a later project-level acceptance surface.
Kvist-generated evidence, including `IMPL.md`, compliance and review reports,
status, and attempt logs, is exempt.

## Principles and priorities

1. **Human authority before autonomy.** Product and architecture decisions need
   explicit human approval.
2. **Structure before syntax.** Vision and architecture precede component
   requirements, contracts, designs, tests, and implementation.
3. **One authoritative home per fact.** Requirements state what must hold,
   contracts state what consumers may rely on, designs state how a component
   intends to satisfy them, and implementation records state what code
   observably does.
4. **Recursive component ownership.** The same lifecycle applies at each
   component boundary.
5. **Strict context boundaries.** A component receives local intent and
   implementation context, the immediate parent contract, explicitly required
   provider contracts, and global constraints—not peer or parent internals.
6. **Durable, inspectable state.** Workflow state lives in version-controlled
   project artifacts rather than chat history or opaque storage.
7. **Independent compliance evidence.** An implementer cannot certify its own
   work.
8. **Advisory review preserves human authority.** Review exposes possible
   shortcomings, but findings may be wrong or overly exacting and never compel
   a change or determine compliance. When review is enabled, acceptance requires
   an acknowledged review opportunity or an exact-bundle exception; a visible
   project opt-out may disable that gate. Agreement with the reviewer is never
   required.
9. **Deterministic and safe behavior.** Validation, ordering, serialization,
   failures, and authority boundaries are explicit and reproducible.
