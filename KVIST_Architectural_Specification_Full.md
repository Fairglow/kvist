# KVIST: Architectural Specification & Strategy Document

**Subtitle:** Structured design for autonomous agents.  
**Project Name:** KVIST (`kvist`)  
**Target Engine Implementation:** Rust  
**License:** Business Source License (BSL 1.1) / Dual-licensed for non-commercial open-use  
**Version:** 0.1.0

**Status:** This is the authoritative product direction, not a claim that every
described workflow is already automated. [`TODO.md`](TODO.md) tracks delivery
status and execution-policy gaps; [`COMPLIANCE_REVIEW.md`](COMPLIANCE_REVIEW.md)
records independent review evidence and discrepancies.

---

## 1. Executive Summary & Vision

In the current landscape of AI-driven software engineering, the industry heavily favors unconstrained autonomous agents—frequently dubbed "vibe coding." While these black-box workflows generate impressive initial velocity, they inevitably suffer from:
1. **Architectural Drift:** Unchecked code generation introducing structural inconsistency.
2. **Context Window Exhaustion:** Unfocused agents losing coherence as codebases grow.
3. **Hidden Technical Debt:** Missing rationale, unverified edge cases, and absent documentation.
4. **Loss of Developer Control:** Developers becoming passive observers rather than active architects.

**KVIST** flips this paradigm. Rather than treating AI as an unguided coder, KVIST enforces a disciplined **Spec-Driven Architecture (SDA)** process. It positions the human user as the Principal System Architect, steering AI agents through a recursive, component-driven design lifecycle.

### Core Tenets
* **Structure Before Syntax:** No source code is written until the component specification, interfaces, and testing strategies are defined and validated.
* **Fractal & Recursive Modularization:** Every application is built as a hierarchical tree of self-contained sub-components ("kvistar" / branches). The exact same design loop applies recursively at every level of depth.
* **Clean-Slate Compliance Verification:** AI agents must never audit their own work in the same session. Compliance is verified by reverse-engineering documentation from code using an isolated, clean-slate agent context.
* **Durable, File-System Native State:** Architecture, specifications, and task queues live directly in the codebase alongside source files—not in ephemeral chat windows or proprietary databases.
* **Tool-Agnostic Engine in Rust:** Built as a headless Rust CLI engine. Core
  workflow commands require neither a cloud service nor a runtime daemon;
  external agent programs are optional, explicitly configured subprocess
  integrations. Generic provider profiles, interactive setup, prompt
  acquisition, command rendering, and process supervision live in a standalone
  library and application that Kvist consumes. Kvist retains role assignment,
  architectural context, execution approval, and task lifecycle. The planned
  native agent runtime separates model transport, bounded orchestration, typed
  tool brokering, policy, execution, and evidence. The standalone component
  owns provider-neutral messages, capabilities, tool descriptors and intents,
  the bounded native loop, runtime events, broker sequencing,
  execution-backend interfaces and reusable Linux implementations, and
  host-authority interfaces. Kvist supplies task policy, grants, approved
  bindings, execution-tier selection, promotion, and canonical compliance
  evidence without creating a reverse dependency. Provider libraries remain
  private transport implementations; opaque coding-agent CLIs are constrained
  as whole processes rather than trusted as authorization boundaries.
* **Linux-First Execution:** Linux is the only supported executable platform
  while the project is maintained and tested by a Linux-only development team.
  Platform-specific execution is isolated behind replaceable boundaries;
  macOS and Windows remain planned rather than nominally supported without
  native testing.

---

## 2. System Architecture & On-Disk Layout

KVIST establishes a 1:1 mapping between the conceptual component hierarchy and the file-system directory tree. Every folder acts as a self-contained module containing its own specification, task queue, reverse-engineered documentation, and implementation files.

```text
repository-root/
├── kvist.toml                  <-- Global project configuration & LLM provider settings
├── ROOT_CONTRACT.md            <-- Global architecture rules & non-negotiable constraints
└── src/
    ├── SPEC.md                 <-- Root component specification
    ├── TODOS.yaml              <-- Execution task queue & progress tracker
    ├── IMPL.md                 <-- Reverse-engineered implementation record
    ├── lib.rs                  <-- Public interface & module root
    └── network/                <-- Sub-component directory
        ├── SPEC.md             <-- Sub-component specification
        ├── TODOS.yaml          <-- Sub-component task queue
        ├── IMPL.md             <-- Sub-component implementation record
        ├── mod.rs              <-- Component interface
        └── protocol/           <-- Child sub-component (recursive)
            ├── SPEC.md
            ├── TODOS.yaml
            ├── IMPL.md
            └── frame.rs
```

### Key Layout Rationale
* **Self-Containment:** A developer or agent inspecting `src/network/protocol` has all context immediately adjacent in the same directory.
* **Context Isolation:** When an agent works on a sub-component, KVIST injects only the local directory files, the immediate parent interface contract, and `ROOT_CONTRACT.md`. Peer code implementations are excluded, preventing prompt bloat and distraction.
* **VCS Native:** Standard version control tools (`git`, `jj`) diff, branch, and merge specifications and task queues just like source code.

---

## 3. The 4-Stage Lifecycle Process

Every node in the component tree progresses through a structured 4-stage recursive lifecycle.

```text
┌────────────────────────────────────────────────────────────────────────┐
│ Stage 1: SPECIFICATION                                                 │
│ • Interactive "Interview" Mode or AI-Drafted Blueprint                 │
│ • Defines Purpose, Rationale, Contracts, Constraints, & Algorithms    │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Feasibility & Completeness Check
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│ Stage 2: TASK BREAKDOWN (TODOS.yaml)                                   │
│ • Mandatory Order: Test -> Implement -> Security -> Review             │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Execute Tasks via Agent
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│ Stage 3: IMPLEMENTATION & NATIVE DOCS                                  │
│ • Source Code + In-Code Docstrings (e.g., rustdoc /// comments)        │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Trigger Clean-Slate Review Loop
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│ Stage 4: TRIPLE-BLIND COMPLIANCE REVIEW                                │
│ • Clean-Slate Agent extracts IMPL.md -> Compliance Agent compares      │
│   SPEC.md against IMPL.md                                              │
└───────────────────────────────────┬────────────────────────────────────┘
```

### Stage 1: Specification (`SPEC.md`) & Layered Disclosure
To allow reading at both high-level executive summaries and deep technical details, `SPEC.md` enforces progressive disclosure via collapsible sections:
* **Layer 1 (Executive Summary):** Purpose, rationale ("Why this exists"), and public contract.
* **Layer 2 (Architectural Guarantees):** Performance bounds, concurrency invariants, memory constraints, and dependency policies.
* **Layer 3 (Detailed Strategy & Algorithms):** Concrete algorithms, state machine transitions, and error-handling paths.

The human architect begins with a project-level vision, then iteratively
decomposes it into one or more hierarchical components. The architect may
draft specifications manually, collaborate with an agent, or ask an architect
agent to propose the decomposition and component contracts. The human reviews,
refines, and explicitly approves the resulting specification before its queue
is designed.

*Planned interactive "Interview" Mode:* To eliminate specification friction, a
future terminal mode will ask structured questions based on the component type
to help the architect and architect agent draft the initial spec. It is not a
current command.

### Stage 2: Actionable TODO Queue (`TODOS.yaml`)
After the human approves a component specification, a designer agent analyzes
it for logical gaps and drafts its specialized atomic task queue. The human may
review and improve the queue; designer and human iterate until the human
accepts it. Every component TODO list must include:
1. `write_tests`: Implement failing test cases corresponding to spec requirements.
2. `implement_code`: Fulfill code logic until all tests pass.
3. `security_audit`: Validate memory safety, boundaries, and thread-safety invariants.
4. `compliance_review`: Trigger the triple-blind verification loop.

### Stage 3: Implementation & Native Language Documentation
Implementation agents write executable code alongside language-native docstrings (e.g., `///` in Rust). High-level function syntax is kept in native docstrings rather than bloated inside `SPEC.md`.

### Stage 4: Triple-Blind Compliance Review
To eliminate "hallucinated compliance":
1. **Agent A (Implementor):** Writes code based on `SPEC.md`.
2. **Agent B (Clean-Slate Documenter):** Receives **only** the generated code (no access to `SPEC.md`) and reverse-engineers `IMPL.md`.
3. **Agent C (Compliance Checker):** Compares `SPEC.md` against `IMPL.md` (no access to raw source code). If discrepancies occur, an arbitration flag is raised.

---

## 4. Conflict Arbitration Workflow (Planned)

When the planned compliance workflow detects a mismatch between `SPEC.md` and
the reverse-engineered `IMPL.md`, it must stop automated progress and retain
the discrepancy for explicit human arbitration. The following illustrates the
intended decision surface; it is not a current CLI or web command:

```text
⚠️ SPEC COMPLIANCE MISMATCH DETECTED in [src/network/protocol]

Spec Requirement: "Must use non-blocking I/O for socket connections."
Implemented Code: "Blocking socket connection detected in frame.rs:42."

Select Arbitration Action:
  [1] Trigger Agent Redesign (Re-prompt implementation agent with feedback)
  [2] Propose Implementation Changes (Prepare a reviewed SPEC.md update)
  [3] Manually Arbitrate (Open diff in user's default editor)
  [4] AI Trade-off Analysis (Ask assistant to evaluate pros/cons before deciding)
```

Every option must preserve the original discrepancy and decision rationale in
version-controlled component artifacts. No option may overwrite `SPEC.md` or
`IMPL.md` implicitly: an architect must review and explicitly accept any
proposed contract or implementation change before task execution can resume.

---

## 5. UI, Editor & Ecosystem Strategy (Planned)

### Why Rust for Implementation?
Rust supports the intended single-binary, portable core and strong memory-safety
guarantees. The current product surface is headless and Linux-only. Source and
protocol design should remain portable, but a platform is enabled only after
its process, filesystem, and sandbox behavior has a maintained native test
matrix. File watching, web, and editor integrations remain deferred.

### Triple-Tier Integration Strategy
1. **Headless Engine Core (`kvist-cli` in Rust):** Manages tree state,
   `TODOS.yaml` parsing, context slicing, and Kvist-specific execution policy.
   It delegates provider-neutral prompt acquisition and process supervision to
   the standalone `agent-runtime` component. That component will classify
   native model, one-shot model, external-agent, and plan-only backends;
   capability support is advertised, independently tested, and policy-enabled
   separately. The small direct local HTTP transport remains the default and
   fallback. Exactly pinned `rig-core` 0.42.0 may be selected through a
   non-default adapter after raising the project MSRV to Rig's upstream-tested
   Rust 1.94 toolchain. Rig remains behind standalone-owned canonical types and
   cannot own authorization, tool execution, evidence, or sandbox policy.
2. **Built-in Local Web View (`kvist serve`):** Spins up an embedded lightweight web server (`axum`) serving a single-page web app. Utilizes **Monaco Editor** (VS Code's open-source editor core) to render the interactive collapsible component tree, live progress bars, and compliance diffs.
3. **Native IDE Alignment (LSP / Watcher):** Since specifications and code are plain Markdown, YAML, and Rust files, users continue using their preferred IDE (VS Code + `rust-analyzer`, Neovim, Zed, RustRover). A lightweight `kvist watch` daemon or LSP sidecar surface spec-staleness diagnostics directly inside the user's editor.

---

## 6. Risk Analysis & Edge Cases

| Risk / Edge Case | Architectural Solution in KVIST |
| :--- | :--- |
| **The "Ripple Effect" (Upstream Spec Changes)** | `status` compares component and immediate-parent specification revisions and reports attributable stale evidence. Persisting revalidation remains an explicit human-reviewed write. |
| **Global Architectural Drift** | Root and immediate-parent contracts define the intended boundary. The current task runner explicitly supplies only component artifacts; future role-specific context must be documented rather than inferred. |
| **Context Window Overhead** | `task run` declares the component's `SPEC.md`, `TODOS.yaml`, and `IMPL.md` as agent context. It does not add parent or peer implementations. |
| **Specification Friction** | A template-driven interview mode is planned; it is not a current command. |

---

## 7. Business Model & Licensing

* **License:** Business Source License 1.1 (BSL 1.1) / Dual-license.
* **Terms:**
  * **100% Free** for non-commercial use, individuals, open-source projects, and small teams.
  * Commercial license required for enterprises exceeding specific revenue/employee thresholds.
  * Automatically converts to an open-source license (Apache 2.0 / MIT) after 3 years.

---

## 8. Delivery Planning

The implementation roadmap, task contexts, acceptance criteria, and status
live in [`TODO.md`](TODO.md). This specification defines the target
architecture and constraints; the tracker is the authoritative, versioned
execution plan and must be updated when this document changes planned scope.

---
*KVIST — Structured design for autonomous agents.*
