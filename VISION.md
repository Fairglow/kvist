# KVIST: Project Vision & Strategic Blueprint

<!-- kvist-vision-version: 1 -->

## 1. Executive Summary & Core Philosophy

Modern AI software engineering predominantly relies on unconstrained, conversational agent workflows—often termed "vibe coding." While these approaches provide rapid prototyping speed, they inevitably degrade into:
- **Architectural Drift:** Structural decay and inconsistent subsystem boundaries over successive AI iterations.
- **Context Window Exhaustion:** AI prompt bloat, memory loss, and hallucinated interfaces as repositories grow.
- **Hidden Technical Debt:** Unverified edge cases, absence of rationale, and unverifiable self-certified code.
- **Erosion of Human Agency:** The human operator is reduced to a passive consumer of diffs rather than the principal systems architect.

**Kvist** re-establishes engineering discipline through **Spec-Driven Architecture (SDA)** for human-directed, agentic software engineering. Kvist positions the human user as the **Principal Architect and Final Approver**, orchestrating autonomous AI agent roles across a recursive, verifiable component lifecycle.

```
+-----------------------------------------------------------------------------+
|                           HUMAN PRINCIPAL ARCHITECT                         |
|     (Vision -> Component Hierarchy -> Spec Approval -> Final Arbitration)   |
+--------------------------------------+--------------------------------------+
                                       |
                   +-------------------+-------------------+
                   |         KVIST ENGINE CORE (Rust)      |
                   |   - Filesystem-native state           |
                   |   - Sandboxed execution barrier       |
                   |   - Cryptographic policy approval     |
                   |   - DAG task runner & verifier        |
                   +-------------------+-------------------+
                                       |
    +------------------+---------------+------------------+------------------+
    |                  |                                  |                  |
    v                  v                                  v                  v
[Architect Agent]  [Designer Agent]              [Developer Agent]   [Reviewer Agents]
Decomposes vision  Derives traceable              Executes test-first Clean-slate IMPL.md
into SPEC.md       task DAG into TODOS.yaml       implementations     & source-blind audit
```

### Core Tenets
1. **Structure Before Syntax:** No code is generated before contracts, interface boundaries, algorithmic invariants, and test requirements are formally specified and human-approved.
2. **Recursive Fractal Architecture:** Systems are modeled as self-similar hierarchical component trees ("kvistar" / branches). Each node encapsulates its own specification, task DAG, implementation record, and source files.
3. **Decoupled Agent Roles & Isolated Contexts:** Prompts and agent contexts are strictly bounded. Sub-component agents see only local component files, the immediate parent interface, and the global root contract—never peer implementations.
4. **Triple-Blind Compliance Verification:** Agents must never grade their own work in the same session. Compliance is verified by reverse-engineering documentation (`IMPL.md`) from source via an isolated agent, then comparing it against `SPEC.md` via an independent auditor.
5. **Durable, Filesystem-Native Provenance:** System state, specifications, and execution DAGs reside directly on disk alongside source code, versioned via Git or Jujutsu (`jj`), without external database dependencies or ephemeral chat locks.
6. **Sandboxed Zero-Trust Execution:** Agent programs and verification suites execute within strict isolation barriers enforcing network denial, component-only filesystem mounts, CPU/output caps, and cryptographic approval verification.

---

## 2. End-to-End Architectural Lifecycle

Kvist governs development through four deterministic, recursive stages:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ STAGE 1: SPECIFICATION DECOMPOSITION                                        │
│ • Input: Project Vision or Parent Contract.                                 │
│ • Tooling: Interactive Interview mode or Architect Agent assistance.       │
│ • Artifact: 3-Layer Progressive Disclosure SPEC.md                          │
│   - Layer 1: Purpose & Public Interface Contract                            │
│   - Layer 2: Invariants, Performance Bounds, Memory & Concurrency Rules     │
│   - Layer 3: Concrete Algorithms, State Machines, Failure Paths             │
│ • Gate: Explicit Human Architect Review & Acceptance (SHA-256 bound)        │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ STAGE 2: ACTIONABLE TASK QUEUE DERIVATION (TODOS.yaml)                       │
│ • Tooling: Designer Agent analyzing SPEC.md for testable deliverables.       │
│ • Schema: Strictly ordered DAG with typed lifecycle kinds:                  │
│   1. test              (Define executable contracts & mocked dependencies)  │
│   2. implementation    (Write code satisfying tests & native docstrings)    │
│   3. security-audit    (Verify memory safety, bounds, & threat vectors)     │
│   4. compliance-review (Trigger clean-slate documentation & arbitration)    │
│ • Gate: Human Architect inspects dependencies & accepts task queue.         │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ STAGE 3: SANDBOXED TEST-FIRST EXECUTION                                     │
│ • Tooling: Developer Agent invoked via sandboxed runner (network: deny).    │
│ • Invariant: Test execution precedes implementation code.                   │
│ • Context Boundary: Local component files + parent contract + root rules.    │
│ • Post-Execution: Automatic runner execution of approved test policies.     │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│ STAGE 4: TRIPLE-BLIND COMPLIANCE & ARBITRATION                              │
│ • Step A: Clean-Slate Documenter Agent inspects source/tests -> IMPL.md     │
│           (Strictly denied access to SPEC.md).                              │
│ • Step B: Source-Blind Compliance Agent compares SPEC.md against IMPL.md    │
│           (Strictly denied access to source code).                          │
│ • Step C: Human Arbitration for any detected divergence:                    │
│   [Option 1] Trigger Agent Redesign (Code fix against current contract)     │
│   [Option 2] Propose Specification Update (Update SPEC.md & revalidate)     │
│   [Option 3] Manual Source Patching / Explicit Override                     │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Filesystem-Native On-Disk Layout

Kvist maintains a 1:1 parity between software component boundaries and directory hierarchies.

```text
repository-root/
├── kvist.toml                  # Project configuration, VCS rules, sandbox & model policies
├── ROOT_CONTRACT.md            # Global architecture invariants, security rules, & coding standards
└── src/
    ├── SPEC.md                 # Root component specification
    ├── TODOS.yaml              # Root task execution DAG & revalidation state
    ├── IMPL.md                 # Root reverse-engineered implementation record
    ├── lib.rs                  # Module root & public surface
    └── network/                # Sub-component directory
        ├── SPEC.md             # Sub-component contract (derives from src/SPEC.md)
        ├── TODOS.yaml          # Sub-component execution queue
        ├── IMPL.md             # Sub-component implementation record
        ├── mod.rs              # Component boundary
        └── protocol/           # Child component (recursive encapsulation)
            ├── SPEC.md
            ├── TODOS.yaml
            ├── IMPL.md
            └── frame.rs
```

### Context Isolation Matrix

| Artifact | Read By | Excluded From | Purpose |
| :--- | :--- | :--- | :--- |
| `ROOT_CONTRACT.md` | All Agents & Tools | N/A | Enforces non-negotiable global architectural and safety rules. |
| `SPEC.md` | Designer, Developer, Compliance Checker | Clean-Slate Documenter | Defines intended requirements, interfaces, and constraints. |
| `TODOS.yaml` | Kvist CLI, Designer, Developer | Clean-Slate Documenter | Persists durable execution DAG, state, and requirement links. |
| `IMPL.md` | Compliance Checker, Human Architect | Implementation Developer | Records observed code behavior without confirmation bias. |
| Local Source Code | Developer, Clean-Slate Documenter | Compliance Checker | Executable implementation and language-native documentation. |
| Peer Components | None (Component isolated) | Active Sub-Component Agents | Prevents cross-module context pollution and prompt explosion. |

---

## 4. Specialized Agent Roles & Interaction Model

Kvist formalizes AI participation into five discrete, specialized roles rather than a single monolithic assistant:

```
+-------------------+---------------------------------------------------------+
| Agent Role        | Primary Responsibility & Operational Boundaries         |
+-------------------+---------------------------------------------------------+
| Architect Agent   | Decomposes high-level vision into layered component     |
|                   | specifications (SPEC.md). Assists during spec interview.|
+-------------------+---------------------------------------------------------+
| Designer Agent    | Analyzes approved SPEC.md and generates the directed    |
|                   | acyclic task queue (TODOS.yaml) in required order.      |
+-------------------+---------------------------------------------------------+
| Developer Agent   | Implements tests and source code within the component   |
|                   | sandbox according to active task prompt and context.    |
+-------------------+---------------------------------------------------------+
| Clean-Slate       | Inspects implemented source code and tests in complete  |
| Documenter Agent  | isolation (no SPEC.md access) to generate IMPL.md.      |
+-------------------+---------------------------------------------------------+
| Source-Blind      | Compares SPEC.md against IMPL.md (no source access) to  |
| Compliance Auditor| produce an objective compliance matrix and discrepancy  |
|                   | report for human arbitration.                           |
+-------------------+---------------------------------------------------------+
```

---

## 5. Security Architecture & Execution Boundary

Kvist guarantees deterministic safety and defense-in-depth across every workflow surface:

1. **Sandboxed Runner Execution:**
   - Tasks executed via `kvist task run` invoke an external, pre-validated sandbox runner outside the repository tree.
   - The runner enforces strict network denial (`network = "deny"`), single component directory mounts (`mount = "component"`), stripped environments, and execution resource limits.
2. **Cryptographically Authenticated Approval:**
   - Before executing code, `kvist task approve-policy` writes a versioned, non-secret record in user-owned local state outside the repo.
   - Approvals are authenticated via a cryptographically secure random user secret bound to canonical project and worktree hashes, preventing malicious repositories from self-approving hostile execution policies.
   - On Linux, probes and requests launch private descriptor-bound copies of freshly verified runner bytes, eliminating Time-of-Check to Time-of-Use (TOCTOU) binary substitution attacks.
3. **Automated Redaction & Audit Trails:**
   - Agent outputs, test results, and error logs are subjected to literal redaction policies and allowlisted environment scrubbing before being committed to JSONL attempt logs.
   - Maximum output buffers (65 KiB default, 1 MiB hard cap) and timeouts (300s default, 3600s hard cap) prevent hung subprocesses or denial-of-service spam.

---

## 6. Long-Term Strategic Roadmap

```
+-----------------------------------------------------------------------------+
| PHASE 1: CORE CLI & FILESYSTEM ENGINE (Completed)                           |
| • Rust single-binary CLI engine (`init`, `doctor`, `status`, `tree`).       |
| • Deterministic 3-layer SPEC.md validation & atomic no-clobber persistence. |
| • Read-only discovery bounds and VCS (Git / Jujutsu) tracking validation.   |
+-----------------------------------------------------------------------------+
                                       |
                                       ▼
+-----------------------------------------------------------------------------+
| PHASE 2: QUEUE DAG, RUNNER & POLICY APPROVAL (Completed / In Progress)      |
| • Versioned TODOS.yaml schema with immutable task IDs & requirement links.  |
| • Bounded sandboxed task runner with cryptographic policy verification.     |
| • Upstream spec hash tracking and attributable stale detection.             |
| • Test verification policies with automated redaction & JSONL logging.      |
+-----------------------------------------------------------------------------+
                                       |
                                       ▼
+-----------------------------------------------------------------------------+
| PHASE 3: AGENTIC WORKFLOW AUTOMATION & REVIEWS (Current Focus)              |
| • Interactive terminal "Interview" mode for conversational spec drafting.   |
| • Automated Designer Agent for programmatic TODOS.yaml DAG generation.      |
| • Clean-Slate Documenter & Source-Blind Compliance arbitration engine.      |
| • Penetration testing and automated security audit task pipelines.          |
+-----------------------------------------------------------------------------+
                                       |
                                       ▼
+-----------------------------------------------------------------------------+
| PHASE 4: DEVELOPER EXPERIENCE & ECOSYSTEM (Planned)                         |
| • Headless `kvist watch` daemon and Language Server Protocol (LSP) sidecar  |
|   providing live spec-staleness diagnostics in Zed, Neovim, and VS Code.    |
| • Embedded lightweight local web interface (`kvist serve` via Axum)         |
|   featuring Monaco Editor, interactive DAG graphs, and visual diffs.        |
+-----------------------------------------------------------------------------+
```

---

## 7. Strategic Alignment with System Documents

This Vision document serves as the authoritative strategic anchor for the Kvist ecosystem:
- **Alignment with `README.md`:** Establishes the rationale and contract for CLI commands, sandboxing constraints, and platform support guarantees.
- **Alignment with `GUIDE.md`:** Clarifies the mental model for the human architect's interaction loop, execution scripts, and revalidation workflows.
- **Alignment with `KVIST_Architectural_Specification_Full.md`:** Provides the high-level operational doctrine and vision that the technical architecture formally implements.
