# KVIST: Architectural Specification & Strategy Document

**Subtitle:** Structured design for autonomous agents.  
**Project Name:** KVIST (`kvist`)  
**Target Engine Implementation:** Rust  
**License:** Business Source License (BSL 1.1) / Dual-licensed for non-commercial open-use  
**Version:** 0.1.0-draft  

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
* **Tool-Agnostic Engine in Rust:** Built as a headless, single-binary CLI engine in Rust, prioritizing performance, local privacy, zero external runtime dependencies, and compatibility with any LLM client (Claude Code, Gemini CLI, Ollama, etc.).

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
    ├── DOCS.md                 <-- Reverse-engineered compliance documentation
    ├── lib.rs                  <-- Public interface & module root
    └── network/                <-- Sub-component directory
        ├── SPEC.md             <-- Sub-component specification
        ├── TODOS.yaml          <-- Sub-component task queue
        ├── DOCS.md             <-- Sub-component compliance doc
        ├── mod.rs              <-- Component interface
        └── protocol/           <-- Child sub-component (recursive)
            ├── SPEC.md
            ├── TODOS.yaml
            ├── DOCS.md
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
│ • Clean-Slate Agent extracts DOCS.md -> Compliance Agent compares      │
│   SPEC.md against DOCS.md                                              │
└───────────────────────────────────┬────────────────────────────────────┘
```

### Stage 1: Specification (`SPEC.md`) & Layered Disclosure
To allow reading at both high-level executive summaries and deep technical details, `SPEC.md` enforces progressive disclosure via collapsible sections:
* **Layer 1 (Executive Summary):** Purpose, rationale ("Why this exists"), and public contract.
* **Layer 2 (Architectural Guarantees):** Performance bounds, concurrency invariants, memory constraints, and dependency policies.
* **Layer 3 (Detailed Strategy & Algorithms):** Concrete algorithms, state machine transitions, and error-handling paths.

*Interactive "Interview" Mode:* To eliminate specification friction, KVIST provides an interactive terminal mode where the AI asks structured questions based on the component type to help the user draft the initial spec.

### Stage 2: Actionable TODO Queue (`TODOS.yaml`)
Before implementation begins, KVIST analyzes the spec for logical gaps and breaks it into atomic tasks. Every component TODO list must include:
1. `write_tests`: Implement failing test cases corresponding to spec requirements.
2. `implement_code`: Fulfill code logic until all tests pass.
3. `security_audit`: Validate memory safety, boundaries, and thread-safety invariants.
4. `compliance_review`: Trigger the triple-blind verification loop.

### Stage 3: Implementation & Native Language Documentation
Implementation agents write executable code alongside language-native docstrings (e.g., `///` in Rust). High-level function syntax is kept in native docstrings rather than bloated inside `SPEC.md`.

### Stage 4: Triple-Blind Compliance Review
To eliminate "hallucinated compliance":
1. **Agent A (Implementor):** Writes code based on `SPEC.md`.
2. **Agent B (Clean-Slate Documenter):** Receives **only** the generated code (no access to `SPEC.md`) and reverse-engineers `DOCS.md`.
3. **Agent C (Compliance Checker):** Compares `SPEC.md` against `DOCS.md` (no access to raw source code). If discrepancies occur, an arbitration flag is raised.

---

## 4. Conflict Arbitration Workflow

When the Compliance Agent detects a mismatch between `SPEC.md` and the reverse-engineered `DOCS.md`, KVIST presents an interactive CLI/Web arbitration prompt:

```text
⚠️ SPEC COMPLIANCE MISMATCH DETECTED in [src/network/protocol]

Spec Requirement: "Must use non-blocking I/O for socket connections."
Implemented Code: "Blocking socket connection detected in frame.rs:42."

Select Arbitration Action:
  [1] Trigger Agent Redesign (Re-prompt implementation agent with feedback)
  [2] Accept Implementation Changes (Update SPEC.md to reflect new behavior)
  [3] Manually Arbitrate (Open diff in user's default editor)
  [4] AI Trade-off Analysis (Ask assistant to evaluate pros/cons before deciding)
```

---

## 5. UI, Editor & Ecosystem Strategy

### Why Rust for Implementation?
Building KVIST in Rust delivers single-binary distribution, zero runtime dependencies, high-performance local file watching, and strict memory safety.

### Triple-Tier Integration Strategy
1. **Headless Engine Core (`kvist-cli` in Rust):** Manages tree state, `TODOS.yaml` parsing, process spawning for local LLMs (`claude`, `gemini-cli`, `ollama`), and context slicing.
2. **Built-in Local Web View (`kvist serve`):** Spins up an embedded lightweight web server (`axum`) serving a single-page web app. Utilizes **Monaco Editor** (VS Code's open-source editor core) to render the interactive collapsible component tree, live progress bars, and compliance diffs.
3. **Native IDE Alignment (LSP / Watcher):** Since specifications and code are plain Markdown, YAML, and Rust files, users continue using their preferred IDE (VS Code + `rust-analyzer`, Neovim, Zed, RustRover). A lightweight `kvist watch` daemon or LSP sidecar surface spec-staleness diagnostics directly inside the user's editor.

---

## 6. Risk Analysis & Edge Cases

| Risk / Edge Case | Architectural Solution in KVIST |
| :--- | :--- |
| **The "Ripple Effect" (Upstream Spec Changes)** | Dependency Graph: Modifying a parent spec automatically marks child specs as `STALE_REVALIDATION_REQUIRED` in `TODOS.yaml`. |
| **Global Architectural Drift** | Root Invariants: Every sub-component agent prompt automatically prepends `ROOT_CONTRACT.md`. |
| **Context Window Overhead** | Strict Context Slicing: Agents only receive local files, parent contracts, and `ROOT_CONTRACT.md`. Peer code is excluded. |
| **Specification Friction** | Template-driven interview mode where the AI asks guided questions to draft initial specs. |

---

## 7. Business Model & Licensing

* **License:** Business Source License 1.1 (BSL 1.1) / Dual-license.
* **Terms:**
  * **100% Free** for non-commercial use, individuals, open-source projects, and small teams.
  * Commercial license required for enterprises exceeding specific revenue/employee thresholds.
  * Automatically converts to an open-source license (Apache 2.0 / MIT) after 3 years.

---

## 8. Implementation Roadmap (PoC in Rust)

### Phase 1: Core CLI Engine (`kvist-cli`)
- [ ] Initialize `kvist.toml` and `ROOT_CONTRACT.md` bootstrapping logic (`kvist init`).
- [ ] Implement directory scanner and terminal tree visualizer (`kvist tree`).
- [ ] Build layered `SPEC.md` parser and generator templates.

### Phase 2: Task Execution & LLM Runner
- [ ] Implement `TODOS.yaml` schema with `serde_yaml`.
- [ ] Build process runner for invoking external LLM CLIs (`claude`, `gemini`, `ollama`).
- [ ] Implement atomic task execution loop with test verification.

### Phase 3: Triple-Blind Verification & Embedded Web UI
- [ ] Implement Clean-Slate Documenter and Compliance Agent pipelines.
- [ ] Embed `axum` web server and Monaco-based visualizer into the single Rust binary (`include_str!`/`rust-embed`).
- [ ] Add `kvist serve` command for browser-based interactive project steering.

---
*KVIST — Structured design for autonomous agents.*
